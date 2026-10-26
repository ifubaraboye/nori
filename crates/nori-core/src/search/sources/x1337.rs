//! 1337x's HTML search, including its detail pages for magnets.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use regex::Regex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::super::format::{parse_size, utc_from_ymd};
use super::super::http::HttpClient;
use super::super::magnet::parse_magnet;
use super::super::model::SearchResult;

const HOSTS: &[&str] = &["1337x.to", "1337x.st", "x1337x.ws", "1337xx.to"];
const MAX_DETAILS: usize = 4;
const STOP_WORDS: &[&str] = &["the", "a", "an", "of", "and", "or", "to"];

static HOST_INDEX: AtomicUsize = AtomicUsize::new(0);
static ROW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)href="(/torrent/[^"]+)"[^>]*>([^<]+)</a>"#).unwrap());
static SIZE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)class="coll-4 size[^"]*">\s*([\d.]+\s*[KMGT]i?B)"#).unwrap()
});
static SEEDS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)class="coll-2 seeds[^"]*">\s*(\d+)"#).unwrap());
static LEECH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)class="coll-3 leeches[^"]*">\s*(\d+)"#).unwrap());
static MAGNET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)magnet:\?xt=urn:btih:[^"'<>\s]+"#).unwrap());
static DATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)Date uploaded</strong>\s*<span>\s*([A-Za-z]{3})\.?\s+(\d{1,2})[a-z]{2}\s*'(\d{2})",
    )
    .unwrap()
});

#[derive(Debug)]
struct Row {
    name: String,
    path: String,
    seeders: i64,
    leechers: i64,
    size_bytes: i64,
}

pub async fn search_movies(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    search(http, query, "Movies", "x1337-movies", cancel).await
}

pub async fn search_tv(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    search(http, query, "TV", "x1337-tv", cancel).await
}

async fn search(
    http: &HttpClient,
    query: &str,
    category: &str,
    source: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    let q = query.trim();
    let path = if q.is_empty() {
        if category == "TV" {
            "/popular-tv".to_owned()
        } else {
            "/popular-movies".to_owned()
        }
    } else {
        format!(
            "/category-search/{}/{}/1/",
            super::super::magnet::query_escape(q),
            category
        )
    };

    let start = HOST_INDEX.load(Ordering::Relaxed);
    let mut base = String::new();
    let mut page = String::new();
    let mut last_err = None;
    for offset in 0..HOSTS.len() {
        let idx = (start + offset) % HOSTS.len();
        let host = HOSTS[idx];
        let retries = if offset == 0 { 2 } else { 0 };
        match http
            .get(&format!("https://{host}{path}"), retries, cancel)
            .await
        {
            Ok(body) => {
                HOST_INDEX.store(idx, Ordering::Relaxed);
                base = format!("https://{host}");
                page = String::from_utf8_lossy(&body).into_owned();
                break;
            }
            Err(e) => {
                if cancel.is_cancelled() {
                    return Err(e);
                }
                last_err = Some(e);
            }
        }
    }
    if base.is_empty() {
        return Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no 1337x host answered")));
    }

    let mut rows = parse_rows(&page);
    rows = filter_rows(rows, q);
    rows.sort_by(|a, b| b.seeders.cmp(&a.seeders));
    rows.truncate(MAX_DETAILS);

    let mut tasks = JoinSet::new();
    for (index, row) in rows.iter().enumerate() {
        let http = http.clone();
        let url = format!("{base}{}", row.path);
        let cancel = cancel.clone();
        tasks.spawn(async move {
            let body = http.get(&url, 1, &cancel).await.ok()?;
            let body = String::from_utf8_lossy(&body);
            let magnet = MAGNET_RE.find(&body)?;
            Some((
                index,
                html_escape::decode_html_entities(magnet.as_str()).into_owned(),
                upload_date(&body),
            ))
        });
    }
    let mut details = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        if let Ok(Some(detail)) = joined {
            details.push(detail);
        }
    }
    details.sort_by_key(|(index, _, _)| *index);

    let mut out = Vec::new();
    for (index, magnet, added) in details {
        let Some(parsed) = parse_magnet(&magnet) else {
            continue;
        };
        let row = &rows[index];
        out.push(SearchResult {
            info_hash: parsed.hash,
            name: row.name.clone(),
            size_bytes: row.size_bytes,
            seeders: row.seeders,
            leechers: row.leechers,
            source: source.into(),
            magnet,
            added,
            ..Default::default()
        });
    }
    Ok(out)
}

fn parse_rows(page: &str) -> Vec<Row> {
    let Some(start) = page.find("table-list") else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for chunk in page[start..].split("<tr").skip(1) {
        let Some(caps) = ROW_RE.captures(chunk) else {
            continue;
        };
        rows.push(Row {
            path: caps[1].to_owned(),
            name: html_escape::decode_html_entities(&caps[2])
                .trim()
                .to_owned(),
            size_bytes: SIZE_RE
                .captures(chunk)
                .map(|c| parse_size(&c[1]))
                .unwrap_or(0),
            seeders: SEEDS_RE
                .captures(chunk)
                .and_then(|c| c[1].parse().ok())
                .unwrap_or(0),
            leechers: LEECH_RE
                .captures(chunk)
                .and_then(|c| c[1].parse().ok())
                .unwrap_or(0),
        });
    }
    rows
}

/// Drop rows that only share a stop word with the query.
fn filter_rows(rows: Vec<Row>, query: &str) -> Vec<Row> {
    let tokens: Vec<String> = query
        .trim()
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let mut need: Vec<&str> = tokens
        .iter()
        .map(String::as_str)
        .filter(|t| !STOP_WORDS.contains(t))
        .collect();
    if need.is_empty() {
        need = tokens.iter().map(String::as_str).collect();
    }
    if need.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|row| {
            let name = row.name.to_lowercase();
            need.iter().all(|token| name.contains(token))
        })
        .collect()
}

fn upload_date(page: &str) -> i64 {
    let Some(caps) = DATE_RE.captures(page) else {
        return 0;
    };
    let month = match caps[1].to_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return 0,
    };
    let day = caps[2].parse::<u32>().unwrap_or(0);
    let year = 2000 + caps[3].parse::<i32>().unwrap_or(0);
    utc_from_ymd(year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_rows() {
        let page = r#"
<table class="table-list">
<tr><td><a href="/torrent/1/a/">Some Show S01 1080p</a>
<td class="coll-4 size">1.2 GB</td><td class="coll-2 seeds">40</td><td class="coll-3 leeches">5</td></tr>
</table>"#;
        let rows = parse_rows(page);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Some Show S01 1080p");
        assert_eq!(rows[0].seeders, 40);
        assert_eq!(rows[0].leechers, 5);
        assert_eq!(rows[0].size_bytes, 1_200_000_000);
    }

    #[test]
    fn parses_upload_dates() {
        assert!(upload_date("Date uploaded</strong> <span>Jun. 26th  '26") > 0);
        assert_eq!(upload_date("no date here"), 0);
    }

    #[test]
    fn filtering_requires_every_meaningful_token() {
        let rows = vec![
            Row {
                name: "Dune Part Two 2160p".into(),
                path: "/a".into(),
                seeders: 1,
                leechers: 0,
                size_bytes: 1,
            },
            Row {
                name: "Dune 1984".into(),
                path: "/b".into(),
                seeders: 1,
                leechers: 0,
                size_bytes: 1,
            },
        ];
        let kept = filter_rows(rows, "dune part");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/a");
    }
}
