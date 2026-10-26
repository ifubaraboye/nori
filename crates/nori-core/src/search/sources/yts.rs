//! YTS's JSON API.

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::super::http::HttpClient;
use super::super::magnet::{build_magnet, normalize_hash, query_escape};
use super::super::model::SearchResult;

const HOSTS: &[&str] = &["https://yts.mx", "https://yts.am", "https://yts.rs"];

pub async fn search(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    let q = query.trim();
    let params = if q.is_empty() {
        "limit=50&sort_by=date_added".to_owned()
    } else {
        format!("limit=50&query_term={}", query_escape(q))
    };
    let mut last_err = None;
    for host in HOSTS {
        let url = format!("{host}/api/v2/list_movies.json?{params}");
        let body = match http.get(&url, 1, cancel).await {
            Ok(body) => body,
            Err(e) => {
                if cancel.is_cancelled() {
                    return Err(e);
                }
                last_err = Some(e);
                continue;
            }
        };
        let parsed: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                last_err = Some(anyhow::anyhow!("YTS returned invalid JSON: {e}"));
                continue;
            }
        };
        return Ok(parse_movies(&parsed));
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no YTS host answered")))
}

fn parse_movies(parsed: &Value) -> Vec<SearchResult> {
    let mut out = Vec::new();
    let movies = parsed
        .pointer("/data/movies")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for movie in movies {
        let base = movie
            .get("title_long")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| movie.get("title").and_then(Value::as_str))
            .unwrap_or("Unknown");
        let added = movie
            .get("date_uploaded_unix")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let Some(torrents) = movie.get("torrents").and_then(Value::as_array) else {
            continue;
        };
        for torrent in torrents {
            let hash = torrent.get("hash").and_then(Value::as_str).unwrap_or("");
            if hash.is_empty() {
                continue;
            }
            let mut name = base.to_owned();
            let quality = torrent.get("quality").and_then(Value::as_str).unwrap_or("");
            let kind = torrent.get("type").and_then(Value::as_str).unwrap_or("");
            let mut tag = quality.to_owned();
            if !kind.is_empty() {
                if !tag.is_empty() {
                    tag.push(' ');
                }
                tag.push_str(kind);
            }
            if !tag.is_empty() {
                name.push_str(" [");
                name.push_str(&tag);
                name.push(']');
            }
            let hash = normalize_hash(hash);
            out.push(SearchResult {
                info_hash: hash.clone(),
                name: name.clone(),
                size_bytes: torrent
                    .get("size_bytes")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                seeders: torrent.get("seeds").and_then(Value::as_i64).unwrap_or(0),
                leechers: torrent.get("peers").and_then(Value::as_i64).unwrap_or(0),
                source: "yts".into(),
                magnet: build_magnet(&hash, &name, &[]),
                added,
                ..Default::default()
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_quality_tags_and_health() {
        let payload = json!({
            "data": {
                "movies": [{
                    "title_long": "Dune (2021)",
                    "date_uploaded_unix": 1700000000,
                    "torrents": [{
                        "hash": "c12fe1c06bba254a9dc9f519b335aa7c1367a88a",
                        "quality": "1080p",
                        "type": "bluray",
                        "size_bytes": 2000,
                        "seeds": 50,
                        "peers": 4
                    }]
                }]
            }
        });
        let out = parse_movies(&payload);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Dune (2021) [1080p bluray]");
        assert_eq!(out[0].seeders, 50);
        assert_eq!(out[0].leechers, 4);
        assert_eq!(out[0].size_bytes, 2000);
        assert_eq!(out[0].added, 1700000000);
        assert!(out[0].magnet.contains("xt=urn:btih:c12fe1c0"));
    }

    #[test]
    fn skips_movies_without_torrents_or_hashes() {
        let payload = json!({
            "data": {
                "movies": [
                    { "title": "Empty" },
                    { "title": "No hash", "torrents": [{ "quality": "720p" }] }
                ]
            }
        });
        assert!(parse_movies(&payload).is_empty());
        assert!(parse_movies(&json!({})).is_empty());
    }
}
