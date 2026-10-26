//! Five-minute per-source result cache.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::model::SearchResult;

const TTL: Duration = Duration::from_secs(5 * 60);
const MAX: usize = 100;

#[derive(Default)]
pub struct Cache {
    entries: Mutex<HashMap<String, (Instant, Vec<SearchResult>)>>,
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, source: &str, query: &str) -> Option<Vec<SearchResult>> {
        let key = key(source, query);
        let entries = self.entries.lock();
        entries
            .get(&key)
            .filter(|(at, _)| at.elapsed() < TTL)
            .map(|(_, results)| results.clone())
    }

    pub fn insert(&self, source: &str, query: &str, results: Vec<SearchResult>) {
        let key = key(source, query);
        let mut entries = self.entries.lock();
        entries.insert(key, (Instant::now(), results));
        while entries.len() > MAX {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            entries.remove(&oldest);
        }
    }

    pub fn clear(&self) {
        self.entries.lock().clear();
    }
}

fn key(source: &str, query: &str) -> String {
    format!("{}::{}", source, query.trim().to_ascii_lowercase())
}
