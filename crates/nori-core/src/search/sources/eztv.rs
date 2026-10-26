//! EZTV's JSON API. It accepts no text query, so a show name is matched
//! against the recent index and then expanded by IMDb id.

use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::Value;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::super::http::HttpClient;
use super::super::magnet::{build_magnet, normalize_hash};
use super::super::model::SearchResult;

const APIS: &[&str] = &[
    "https://eztvx.to/api/get-torrents",
    "https://eztv.wf/api/get-torrents",
    "https://eztv.tf/api/get-torrents",
];
const PAGE_LIMIT: usize = 100;
const INDEX_PAGES: usize = 5;
const MAX_SHOWS: usize = 2;
const MAX_RESULTS: usize = 100;
const INDEX_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, Deserialize)]
struct EztvTorrent {
    #[serde(default)]
    title: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    magnet_url: String,
    #[serde(default)]
    imdb_id: String,
    #[serde(default)]
    seeds: i64,
    #[serde(default)]
    peers: i64,
    #[serde(default)]
    size_bytes: Value,
    #[serde(default)]
    date_released_unix: i64,
}

#[derive(Debug, Default, Deserialize)]
struct EztvResponse {
    #[serde(default)]
    torrents: Vec<EztvTorrent>,
}

static INDEX: LazyLock<Mutex<Option<(Instant, Vec<EztvTorrent>)>>> =
    LazyLock::new(|| Mutex::new(None));

async fn fetch_page(
    http: &HttpClient,
    page: usize,
    imdb_id: &str,
    retries: usize,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<EztvTorrent>> {
    let mut last_err = None;
    for base in APIS {
        let mut url = format!("{base}?limit={PAGE_LIMIT}&page={page}");
        if !imdb_id.is_empty() {
            url.push_str("&imdb_id=");
            url.push_str(imdb_id);
        }
        match http.get(&url, retries, cancel).await {
            Ok(body) => {
                let parsed: EztvResponse = serde_json::from_slice(&body)?;
                return Ok(parsed.torrents);
            }
            Err(e) => {
                if cancel.is_cancelled() {
                    return Err(e);
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no EZTV host answered")))
}

async fn recent(http: &HttpClient, cancel: &CancellationToken) -> anyhow::Result<Vec<EztvTorrent>> {
    if let Some((at, rows)) = INDEX.lock().as_ref() {
        if at.elapsed() < INDEX_TTL {
            return Ok(rows.clone());
        }
    }
    let mut rows = fetch_page(http, 1, "", 1, cancel).await?;
    for page in 2..=INDEX_PAGES {
        match fetch_page(http, page, "", 0, cancel).await {
            Ok(more) if !more.is_empty() => rows.extend(more),
            _ => break,
        }
    }
    *INDEX.lock() = Some((Instant::now(), rows.clone()));
    Ok(rows)
}

pub async fn search(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    let q = query.trim();
    if q.is_empty() {
        let rows = fetch_page(http, 1, "", 1, cancel).await?;
        return Ok(to_results(rows));
    }

    let tokens: Vec<String> = q
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let recent = recent(http, cancel).await?;
    let mut hits: Vec<EztvTorrent> = recent
        .iter()
        .filter(|t| matches(t, &tokens))
        .cloned()
        .collect();
    let rows_before = hits.len();

    let ids = show_ids(&hits);
    let mut tasks = JoinSet::new();
    for id in ids {
        let http = http.clone();
        let cancel = cancel.clone();
        tasks.spawn(async move { fetch_page(&http, 1, &id, 0, &cancel).await });
    }
    while let Some(joined) = tasks.join_next().await {
        let Ok(Ok(rows)) = joined else {
            continue;
        };
        for row in rows {
            if matches(&row, &tokens) {
                hits.push(row);
            }
        }
    }
    let mut out = to_results(hits);
    out.sort_by(|a, b| b.seeders.cmp(&a.seeders));
    out.truncate(MAX_RESULTS);
    let _ = rows_before;
    Ok(out)
}

fn matches(t: &EztvTorrent, tokens: &[String]) -> bool {
    let haystack = format!("{} {}", t.title, t.filename).to_lowercase();
    tokens.iter().all(|token| haystack.contains(token))
}

fn show_ids(hits: &[EztvTorrent]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for t in hits {
        let id = t.imdb_id.trim();
        if id.is_empty() || id == "0" || ids.iter().any(|seen| seen == id) {
            continue;
        }
        ids.push(id.to_owned());
        if ids.len() == MAX_SHOWS {
            break;
        }
    }
    ids
}

fn to_results(rows: Vec<EztvTorrent>) -> Vec<SearchResult> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in rows {
        let hash = normalize_hash(&t.hash);
        if hash.len() != 40 || !seen.insert(hash.clone()) {
            continue;
        }
        let mut name = t.title;
        if name.is_empty() {
            name = t.filename;
        }
        if name.is_empty() {
            name = hash.clone();
        }
        let mut magnet = html_escape::decode_html_entities(&t.magnet_url).into_owned();
        if magnet.is_empty() {
            magnet = build_magnet(&hash, &name, &[]);
        }
        out.push(SearchResult {
            info_hash: hash,
            name,
            size_bytes: size_of(&t.size_bytes),
            seeders: t.seeds,
            leechers: t.peers,
            source: "eztv".into(),
            magnet,
            added: t.date_released_unix,
            ..Default::default()
        });
    }
    out
}

fn size_of(raw: &Value) -> i64 {
    if let Some(n) = raw.as_i64() {
        return n;
    }
    raw.as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}
