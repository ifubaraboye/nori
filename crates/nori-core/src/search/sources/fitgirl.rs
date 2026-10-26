//! FitGirl's WordPress feed.

use std::collections::HashSet;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::super::http::HttpClient;
use super::super::magnet::query_escape;
use super::super::model::SearchResult;
use super::super::rss;

const BASE: &str = "https://fitgirl-repacks.site";
const PAGE_SIZE: usize = 10;
const DEPTH: usize = 3;

pub fn feed_url(query: &str, page: usize) -> String {
    let q = query.trim();
    let mut url = if q.is_empty() {
        format!("{BASE}/feed/")
    } else {
        format!("{BASE}/?s={}&feed=rss2&orderby=relevance", query_escape(q))
    };
    if page > 1 {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&format!("paged={page}"));
    }
    url
}

pub async fn search(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    let first = http.get(&feed_url(query, 1), 1, cancel).await?;
    let first_text = String::from_utf8_lossy(&first);
    let mut results = rss::parse_items(&first_text, "fitgirl");
    if first_text.matches("<item>").count() < PAGE_SIZE {
        return Ok(results);
    }

    let mut tasks = JoinSet::new();
    for page in 2..=DEPTH {
        let http = http.clone();
        let url = feed_url(query, page);
        let cancel = cancel.clone();
        tasks.spawn(async move { http.get(&url, 2, &cancel).await });
    }
    let mut seen: HashSet<String> = results.iter().map(|r| r.info_hash.clone()).collect();
    while let Some(joined) = tasks.join_next().await {
        let Ok(Ok(body)) = joined else {
            continue;
        };
        for result in rss::parse_items(&String::from_utf8_lossy(&body), "fitgirl") {
            if seen.insert(result.info_hash.clone()) {
                results.push(result);
            }
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_urls_match_the_wordpress_shape() {
        assert_eq!(feed_url("", 1), "https://fitgirl-repacks.site/feed/");
        assert!(feed_url("dune part two", 1).contains("?s=dune+part+two&feed=rss2"));
        assert!(feed_url("", 3).ends_with("/feed/?paged=3"));
        assert!(feed_url("dune", 2).ends_with("&paged=2"));
    }
}
