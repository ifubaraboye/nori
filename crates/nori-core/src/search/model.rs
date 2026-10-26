//! The data every indexer returns.

use serde::{Deserialize, Serialize};

/// A category a source can feed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Group {
    Games,
    Movies,
    TV,
    Anime,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Movies, Group::TV, Group::Anime, Group::Games];

    pub fn label(self) -> &'static str {
        match self {
            Group::Games => "Games",
            Group::Movies => "Movies",
            Group::TV => "TV",
            Group::Anime => "Anime",
        }
    }
}

/// One torrent row, as a source describes it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub info_hash: String,
    pub name: String,
    pub size_bytes: i64,
    pub seeders: i64,
    pub leechers: i64,
    pub num_files: i64,
    pub source: String,
    pub magnet: String,
    /// Unix seconds.
    pub added: i64,
}

/// Static description of an indexer.
#[derive(Clone, Copy, Debug)]
pub struct SourceMeta {
    pub key: &'static str,
    pub title: &'static str,
    pub cats: &'static [Group],
    pub home: &'static str,
    /// True when seeders and leechers are real swarm counts.
    pub health: bool,
}

impl SourceMeta {
    pub fn id(&self) -> &'static str {
        self.key
    }

    pub fn label(&self) -> &'static str {
        self.title
    }

    pub fn groups(&self) -> &'static [Group] {
        self.cats
    }

    pub fn in_group(&self, group: Option<Group>) -> bool {
        match group {
            None => true,
            Some(g) => self.cats.contains(&g),
        }
    }

    pub fn reports_health(&self) -> bool {
        self.health
    }

    pub fn groups_label(&self) -> String {
        self.cats
            .iter()
            .map(|g| g.label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

const MOVIES: &[Group] = &[Group::Movies];
const TV: &[Group] = &[Group::TV];
const ANIME: &[Group] = &[Group::Anime];
const GAMES: &[Group] = &[Group::Games];
const MOVIES_TV: &[Group] = &[Group::Movies, Group::TV];

/// The registry in display order: dedicated sources first, so their rows win
/// ties in the merged list.
pub static SOURCES: &[SourceMeta] = &[
    SourceMeta {
        key: "fitgirl",
        title: "FitGirl",
        cats: GAMES,
        home: "https://fitgirl-repacks.site",
        health: false,
    },
    SourceMeta {
        key: "yts",
        title: "YTS",
        cats: MOVIES,
        home: "https://yts.mx",
        health: true,
    },
    SourceMeta {
        key: "tpb-movies",
        title: "TPB",
        cats: MOVIES,
        home: "https://thepiratebay.org",
        health: true,
    },
    SourceMeta {
        key: "x1337-movies",
        title: "1337x",
        cats: MOVIES,
        home: "https://1337x.to",
        health: true,
    },
    SourceMeta {
        key: "eztv",
        title: "EZTV",
        cats: TV,
        home: "https://eztvx.to",
        health: true,
    },
    SourceMeta {
        key: "tpb-tv",
        title: "TPB",
        cats: TV,
        home: "https://thepiratebay.org",
        health: true,
    },
    SourceMeta {
        key: "x1337-tv",
        title: "1337x",
        cats: TV,
        home: "https://1337x.to",
        health: true,
    },
    SourceMeta {
        key: "nyaa",
        title: "Nyaa",
        cats: ANIME,
        home: "https://nyaa.si",
        health: true,
    },
    SourceMeta {
        key: "subsplease",
        title: "SubsPlease",
        cats: ANIME,
        home: "https://subsplease.org",
        health: false,
    },
    SourceMeta {
        key: "bittorrented",
        title: "BitTorrented",
        cats: MOVIES_TV,
        home: "https://bittorrented.com",
        health: true,
    },
];

pub fn sources() -> &'static [SourceMeta] {
    SOURCES
}

pub fn source_by_id(id: &str) -> Option<&'static SourceMeta> {
    SOURCES.iter().find(|s| s.key == id)
}

/// Sources feeding a category, or all of them for `None`.
pub fn sources_for(group: Option<Group>) -> Vec<&'static SourceMeta> {
    SOURCES.iter().filter(|s| s.in_group(group)).collect()
}

/// The short label shown on a result row.
pub fn source_tag(id: &str) -> &'static str {
    match id {
        "fitgirl" => "FG",
        "yts" => "YTS",
        "eztv" => "EZTV",
        "nyaa" => "NYAA",
        "subsplease" => "SUB",
        "tpb-movies" | "tpb-tv" => "TPB",
        "x1337-movies" | "x1337-tv" => "1337",
        "bittorrented" => "BT",
        _ => "•",
    }
}
