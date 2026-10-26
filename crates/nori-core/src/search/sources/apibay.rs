//! The Pirate Bay's apibay JSON API.

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::super::http::HttpClient;
use super::super::magnet::{build_magnet, normalize_hash, query_escape};
use super::super::model::SearchResult;

const BASE: &str = "https://apibay.org";
const MOVIE_CATS: &[i64] = &[201, 202, 207, 209];
const TV_CATS: &[i64] = &[205, 208];

pub async fn search_movies(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    search(http, query, "tpb-movies", MOVIE_CATS, "207", cancel).await
}

pub async fn search_tv(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    search(http, query, "tpb-tv", TV_CATS, "208", cancel).await
}

async fn search(
    http: &HttpClient,
    query: &str,
    source: &str,
    cats: &[i64],
    top_category: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    let q = query.trim();
    let body = if q.is_empty() {
        http.get(
            &format!("{BASE}/precompiled/data_top100_{top_category}.json"),
            1,
            cancel,
        )
        .await?
    } else {
        query_apibay(http, q, cancel).await?
    };
    let items: Vec<Value> = serde_json::from_slice(&body)?;
    let mut out = Vec::new();
    for item in items {
        if !q.is_empty() {
            let category = flex(&item, "category").parse::<i64>().unwrap_or(0);
            if !cats.contains(&category) {
                continue;
            }
        }
        if let Some(result) = to_result(&item, source) {
            out.push(result);
        }
    }
    Ok(out)
}

async fn query_apibay(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<u8>> {
    let body = http
        .get(
            &format!("{BASE}/q.php?q={}", query_escape(query)),
            1,
            cancel,
        )
        .await?;
    let no_results = serde_json::from_slice::<Vec<Value>>(&body)
        .map(|items| is_no_results(&items))
        .unwrap_or(false);
    if no_results {
        return http
            .get(
                &format!("{BASE}/q.php?q={}&cat=0", query_escape(query)),
                1,
                cancel,
            )
            .await;
    }
    Ok(body)
}

fn is_no_results(items: &[Value]) -> bool {
    items.len() == 1 && flex(&items[0], "id") == "0"
}

fn to_result(item: &Value, source: &str) -> Option<SearchResult> {
    let hash = normalize_hash(&flex(item, "info_hash"));
    if hash.is_empty() || flex(item, "id") == "0" || hash.len() != 40 {
        return None;
    }
    let mut name = flex(item, "name");
    if name.is_empty() {
        name = "Unknown".into();
    }
    let size_bytes = flex(item, "size").parse::<i64>().unwrap_or(0);
    let seeders = flex(item, "seeders").parse::<i64>().unwrap_or(0);
    let leechers = flex(item, "leechers").parse::<i64>().unwrap_or(0);
    let num_files = flex(item, "num_files").parse::<i64>().unwrap_or(0);
    let added = flex(item, "added").parse::<i64>().unwrap_or(0);
    Some(SearchResult {
        info_hash: hash.clone(),
        name: name.clone(),
        size_bytes,
        seeders,
        leechers,
        num_files,
        source: source.into(),
        magnet: build_magnet(&hash, &name, &[]),
        added,
    })
}

/// apibay sends most fields as strings, but precompiled files use numbers.
fn flex(item: &Value, key: &str) -> String {
    match item.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_numeric_payloads() {
        let item = json!({
            "id": 207001,
            "name": "Top Movie",
            "info_hash": "c12fe1c06bba254a9dc9f519b335aa7c1367a88a",
            "seeders": 42,
            "leechers": 3,
            "num_files": 2,
            "size": 1000,
            "added": 1700000000,
            "category": 207
        });
        let row = to_result(&item, "tpb-movies").expect("valid row");
        assert_eq!(row.seeders, 42);
        assert_eq!(row.leechers, 3);
        assert_eq!(row.num_files, 2);
        assert_eq!(row.size_bytes, 1000);
        assert_eq!(row.added, 1700000000);
        assert!(row.magnet.contains("c12fe1c0"));
    }

    #[test]
    fn rejects_the_sentinel_and_short_hashes() {
        assert!(to_result(&json!({ "id": "0" }), "tpb-movies").is_none());
        assert!(
            to_result(
                &json!({ "id": "1", "info_hash": "short", "name": "x" }),
                "tpb-movies"
            )
            .is_none()
        );
    }

    #[test]
    fn empty_answers_are_recognised() {
        assert!(is_no_results(&[
            json!({ "id": "0", "name": "No results returned" })
        ]));
        assert!(!is_no_results(&[json!({ "id": "1" })]));
    }
}
