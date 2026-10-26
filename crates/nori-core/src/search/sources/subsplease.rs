//! SubsPlease's JSON API.

use std::collections::HashMap;

use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::super::format::parse_unix_seconds;
use super::super::http::HttpClient;
use super::super::magnet::{magnet_size, parse_magnet};
use super::super::model::SearchResult;

const API: &str = "https://subsplease.org/api/";
const RESOLUTIONS: &[&str] = &["1080", "720", "480"];

#[derive(Deserialize)]
struct Download {
    #[serde(default)]
    res: String,
    #[serde(default)]
    magnet: String,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(default)]
    show: String,
    #[serde(default)]
    episode: String,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    downloads: Vec<Download>,
}

pub async fn search(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut url = Url::parse(API)?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("tz", "UTC");
        let q = query.trim();
        if q.is_empty() {
            pairs.append_pair("f", "latest");
        } else {
            pairs.append_pair("f", "search");
            pairs.append_pair("s", q);
        }
    }
    let body = http.get(url.as_str(), 1, cancel).await?;
    let trimmed = body
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    if trimmed.is_empty() || trimmed == b"[]" || trimmed == b"null" {
        return Ok(Vec::new());
    }
    let entries: HashMap<String, Entry> = serde_json::from_slice(&body)?;
    let mut out = Vec::new();
    for entry in entries.into_values() {
        let Some(download) = pick_download(&entry.downloads) else {
            continue;
        };
        let Some(parsed) = parse_magnet(&download.magnet) else {
            continue;
        };
        let mut name = if entry.show.is_empty() {
            "Unknown".into()
        } else {
            entry.show
        };
        if !entry.episode.is_empty() {
            name.push_str(" - ");
            name.push_str(&entry.episode);
        }
        let res = if download.res.is_empty() {
            "?"
        } else {
            &download.res
        };
        name.push_str(&format!(" [{res}p]"));
        out.push(SearchResult {
            info_hash: parsed.hash,
            name,
            size_bytes: magnet_size(&download.magnet),
            source: "subsplease".into(),
            magnet: download.magnet.clone(),
            added: parse_unix_seconds(&entry.release_date),
            ..Default::default()
        });
    }
    out.sort_by(|a, b| b.added.cmp(&a.added));
    Ok(out)
}

fn pick_download(downloads: &[Download]) -> Option<&Download> {
    for res in RESOLUTIONS {
        if let Some(d) = downloads
            .iter()
            .find(|d| d.res == *res && !d.magnet.is_empty())
        {
            return Some(d);
        }
    }
    downloads.iter().find(|d| !d.magnet.is_empty())
}
