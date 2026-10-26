//! Indexer dispatch.

pub mod apibay;
pub mod bittorrented;
pub mod eztv;
pub mod fitgirl;
pub mod nyaa;
pub mod subsplease;
pub mod x1337;
pub mod yts;

use tokio_util::sync::CancellationToken;

use super::http::HttpClient;
use super::model::{SearchResult, SourceMeta};

pub async fn run(
    source: &SourceMeta,
    http: &HttpClient,
    query: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<SearchResult>> {
    match source.key {
        "fitgirl" => fitgirl::search(http, query, cancel).await,
        "yts" => yts::search(http, query, cancel).await,
        "tpb-movies" => apibay::search_movies(http, query, cancel).await,
        "tpb-tv" => apibay::search_tv(http, query, cancel).await,
        "x1337-movies" => x1337::search_movies(http, query, cancel).await,
        "x1337-tv" => x1337::search_tv(http, query, cancel).await,
        "eztv" => eztv::search(http, query, cancel).await,
        "nyaa" => nyaa::search(http, query, cancel).await,
        "subsplease" => subsplease::search(http, query, cancel).await,
        "bittorrented" => bittorrented::search(http, query, cancel).await,
        other => anyhow::bail!("unknown source: {other}"),
    }
}
