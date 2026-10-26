//! Indexer search: registry, fan-out, cache, dedupe and formatting.

pub mod cache;
pub mod format;
pub mod http;
pub mod magnet;
pub mod model;
pub mod rss;
pub mod sources;

use std::collections::HashMap;
use std::sync::Arc;

use async_channel::Sender;
pub use tokio_util::sync::CancellationToken;

pub use cache::Cache;
pub use format::{human_age, human_size, parse_size, parse_unix_seconds};
pub use http::HttpClient;
pub use magnet::{
    ParsedMagnet, build_magnet, is_info_hash, magnet_size, merge_trackers, normalize_hash,
    parse_magnet, trackers_of,
};
pub use model::{Group, SOURCES, SearchResult, SourceMeta, sources, sources_for};

/// One source's answer, good or bad.
#[derive(Clone, Debug)]
pub struct SearchEvent {
    pub source: String,
    pub results: Vec<SearchResult>,
    pub error: Option<String>,
}

/// Fans a query out to every source at once.
pub struct Searcher {
    http: HttpClient,
    cache: Cache,
}

impl Searcher {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            http: HttpClient::new()?,
            cache: Cache::new(),
        })
    }

    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// Spawn every source, sending each answer as it arrives. Cancelling
    /// `cancel` stops outstanding requests and suppresses their events.
    ///
    /// The caller supplies the runtime: the UI runs its own reactor rather
    /// than relying on an ambient tokio context.
    pub fn run(
        self: &Arc<Self>,
        runtime: &tokio::runtime::Handle,
        sources: Vec<&'static SourceMeta>,
        query: String,
        cancel: CancellationToken,
        tx: Sender<SearchEvent>,
    ) {
        for source in sources {
            let this = self.clone();
            let query = query.clone();
            let cancel = cancel.clone();
            let tx = tx.clone();
            runtime.spawn(async move {
                if cancel.is_cancelled() {
                    return;
                }
                let cached = this.cache.get(source.key, &query);
                let results = match cached {
                    Some(results) => Ok(results),
                    None => sources::run(source, &this.http, &query, &cancel).await,
                };
                if cancel.is_cancelled() {
                    return;
                }
                let event = match results {
                    Ok(results) => {
                        if this.cache.get(source.key, &query).is_none() {
                            this.cache.insert(source.key, &query, results.clone());
                        }
                        SearchEvent {
                            source: source.key.into(),
                            results,
                            error: None,
                        }
                    }
                    Err(e) => SearchEvent {
                        source: source.key.into(),
                        results: Vec::new(),
                        error: Some(e.to_string()),
                    },
                };
                let _ = tx.send(event).await;
            });
        }
    }
}

/// Fold rows describing the same torrent into one.
pub fn dedupe(list: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut by_hash: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<SearchResult> = Vec::with_capacity(list.len());
    for result in list {
        if result.info_hash.is_empty() {
            continue;
        }
        if let Some(&index) = by_hash.get(&result.info_hash) {
            out[index] = merge_result(out[index].clone(), result);
        } else {
            by_hash.insert(result.info_hash.clone(), out.len());
            out.push(result);
        }
    }
    out
}

fn merge_result(a: SearchResult, b: SearchResult) -> SearchResult {
    let (mut win, lost) = if b.seeders > a.seeders {
        (b, a)
    } else {
        (a, b)
    };
    win.magnet = merge_trackers(&win.magnet, &[lost.magnet]);
    if win.num_files == 0 {
        win.num_files = lost.num_files;
    }
    if win.added == 0 {
        win.added = lost.added;
    }
    if win.size_bytes == 0 {
        win.size_bytes = lost.size_bytes;
    }
    win
}

/// Healthiest first, then newest.
pub fn sort_default(list: &mut [SearchResult]) {
    list.sort_by(|a, b| {
        b.seeders
            .cmp(&a.seeders)
            .then_with(|| b.added.cmp(&a.added))
    });
}

/// Keep rows whose name holds every token, scoring the exact phrase first.
pub fn filter_by_text(list: &[SearchResult], text: &str) -> Vec<SearchResult> {
    let tokens: Vec<String> = text
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if tokens.is_empty() {
        return list.to_vec();
    }
    let phrase = tokens.join(" ");
    let mut hits: Vec<(SearchResult, i32)> = Vec::new();
    for result in list {
        let name = result.name.to_lowercase();
        if !tokens.iter().all(|t| name.contains(t)) {
            continue;
        }
        let mut score = 10;
        if name.contains(&phrase) {
            score += 50;
        }
        hits.push((result.clone(), score));
    }
    hits.sort_by(|a, b| b.1.cmp(&a.1));
    hits.into_iter().map(|(r, _)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(hash: &str, seeders: i64, added: i64) -> SearchResult {
        SearchResult {
            info_hash: hash.into(),
            name: "Example".into(),
            seeders,
            added,
            magnet: build_magnet(hash, "Example", &[]),
            ..Default::default()
        }
    }

    #[test]
    fn dedupe_keeps_the_healthier_row() {
        let a = row("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 5, 0);
        let mut b = row("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 50, 9);
        b.num_files = 3;
        let out = dedupe(vec![a, b]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].seeders, 50);
        assert_eq!(out[0].num_files, 3);
        assert_eq!(out[0].added, 9);
    }

    #[test]
    fn sorting_prefers_seeders_then_age() {
        let mut rows = vec![row("a", 1, 10), row("b", 5, 1), row("c", 5, 9)];
        sort_default(&mut rows);
        assert_eq!(rows[0].info_hash, "c");
        assert_eq!(rows[1].info_hash, "b");
    }

    #[test]
    fn filter_requires_every_token() {
        let mut r = row("a", 1, 0);
        r.name = "Big Buck Bunny 1080p".into();
        assert_eq!(filter_by_text(&[r.clone()], "bunny 1080").len(), 1);
        assert_eq!(filter_by_text(&[r], "bunny 720").len(), 0);
    }
}
