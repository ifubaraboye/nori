//! Nyaa's RSS search.

use tokio_util::sync::CancellationToken;
use url::Url;

use super::super::http::HttpClient;
use super::super::model::SearchResult;
use super::super::rss;

pub async fn search(
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut url = Url::parse("https://nyaa.si/")?;
    url.query_pairs_mut()
        .append_pair("page", "rss")
        .append_pair("q", query.trim())
        .append_pair("c", "0_0")
        .append_pair("f", "0");
    let body = http.get(url.as_str(), 1, cancel).await?;
    Ok(rss::parse_nyaa(&String::from_utf8_lossy(&body)))
}
