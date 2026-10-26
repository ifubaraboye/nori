//! BitTorrented's invite-gated JSON API.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::super::format::iso_unix;
use super::super::http::HttpClient;
use super::super::magnet::{build_magnet, normalize_hash};
use super::super::model::SearchResult;

const BASE: &str = "https://bittorrented.com";

static HEX_HASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-f0-9]{40}$").unwrap());

pub async fn search(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    let q = query.trim();
    if q.len() < 3 {
        return Ok(Vec::new());
    }
    let mut url = Url::parse(&format!("{BASE}/api/search/torrents"))?;
    url.query_pairs_mut()
        .append_pair("q", q)
        .append_pair("type", "video")
        .append_pair("limit", "50")
        .append_pair("sortBy", "seeders")
        .append_pair("sortOrder", "desc");
    let body = http.get(url.as_str(), 1, cancel).await?;
    let parsed: Value = serde_json::from_slice(&body)?;
    if let Some(error) = parsed
        .get("error")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty())
    {
        anyhow::bail!("{error}");
    }
    let mut out = Vec::new();
    let results = parsed
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in results {
        let hash = normalize_hash(
            item.get("torrent_infohash")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        if !HEX_HASH.is_match(&hash) {
            continue;
        }
        let mut name = item
            .get("torrent_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if name.is_empty() {
            name = hash.clone();
        }
        out.push(SearchResult {
            info_hash: hash.clone(),
            name: name.clone(),
            size_bytes: item
                .get("torrent_total_size")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            seeders: item
                .get("torrent_seeders")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            leechers: item
                .get("torrent_leechers")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            num_files: item
                .get("torrent_file_count")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            source: "bittorrented".into(),
            magnet: build_magnet(&hash, &name, &[]),
            added: iso_unix(
                item.get("torrent_created_at")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        });
    }
    Ok(out)
}
