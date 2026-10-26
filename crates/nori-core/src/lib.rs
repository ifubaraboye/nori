//! Headless engine for Nori: settings, indexer search, and the torrent queue.
//!
//! Nothing in this crate depends on GPUI, so its tests run without a display.

pub mod config;
pub mod search;
pub mod torrent;
