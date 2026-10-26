//! The torrent queue behind the download and seeding views.

pub mod manager;
pub mod perms;
pub mod playlist;

pub use manager::{BatchMode, Item, Status, TorrentManager};
