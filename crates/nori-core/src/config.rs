//! Settings and file locations, following XDG on Linux.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Directory name under the XDG roots. The suffix keeps Nori's state separate
/// from the other desktop app that also calls itself Nori.
pub const APP_DIR: &str = "nori-torrent";

/// What the app remembers between runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub download_dir: String,
    pub trackers: Vec<String>,
    /// 0 means no limit.
    pub max_downloads: i32,
    /// 0 keeps seeding.
    pub seed_time_minutes: i32,
    /// When false, finished torrents stay in Seeding but do not send data.
    #[serde(default = "yes")]
    pub seeding_enabled: bool,
    pub disabled_sources: Vec<String>,
    pub hide_dead: bool,
    /// `system`, `light`, or `dark`.
    pub theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            download_dir: default_download_dir().to_string_lossy().into_owned(),
            trackers: Vec::new(),
            max_downloads: 0,
            seed_time_minutes: 0,
            seeding_enabled: true,
            disabled_sources: Vec::new(),
            hide_dead: false,
            theme: "system".into(),
        }
    }
}

/// The per-user configuration directory.
pub fn config_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(x).join(APP_DIR);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".").join(".config"))
        .join(APP_DIR)
}

/// The per-user data directory.
pub fn data_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(x).join(APP_DIR);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from(".").join(".local/share"))
        .join(APP_DIR)
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn queue_path() -> PathBuf {
    data_dir().join("queue.json")
}

pub fn default_download_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Downloads")
        .join("nori")
}

fn yes() -> bool {
    true
}

impl Config {
    /// Read the settings, falling back to defaults for missing or malformed fields.
    pub fn load() -> Self {
        let mut cfg = Self::default();
        let Ok(raw) = fs::read_to_string(config_path()) else {
            return cfg;
        };
        let Ok(parsed) = serde_json::from_str::<Config>(&raw) else {
            return cfg;
        };
        if !parsed.download_dir.trim().is_empty() {
            cfg.download_dir = parsed.download_dir;
        }
        cfg.trackers = clean_trackers(&parsed.trackers);
        cfg.max_downloads = parsed.max_downloads.max(0);
        cfg.seed_time_minutes = parsed.seed_time_minutes.max(0);
        cfg.seeding_enabled = parsed.seeding_enabled;
        cfg.disabled_sources = parsed.disabled_sources;
        cfg.hide_dead = parsed.hide_dead;
        cfg.theme = match parsed.theme.as_str() {
            "light" | "dark" => parsed.theme,
            _ => "system".into(),
        };
        cfg
    }

    /// Write the settings atomically.
    pub fn save(&self) -> anyhow::Result<()> {
        fs::create_dir_all(config_dir())?;
        let raw = serde_json::to_string_pretty(self)?;
        let tmp = config_path().with_extension("json.tmp");
        fs::write(&tmp, raw)?;
        fs::rename(tmp, config_path())?;
        Ok(())
    }

    pub fn enabled(&self, source_id: &str) -> bool {
        !self.disabled_sources.iter().any(|id| id == source_id)
    }

    pub fn set_enabled(&mut self, source_id: &str, on: bool) {
        self.disabled_sources.retain(|id| id != source_id);
        if !on {
            self.disabled_sources.push(source_id.to_owned());
        }
    }

    pub fn theme_preference(&self) -> ThemePreference {
        match self.theme.as_str() {
            "light" => ThemePreference::Light,
            "dark" => ThemePreference::Dark,
            _ => ThemePreference::System,
        }
    }

    pub fn download_path(&self) -> PathBuf {
        PathBuf::from(&self.download_dir)
    }

    pub fn queue_path(&self) -> PathBuf {
        queue_path()
    }

    pub fn set_download_dir(&mut self, path: impl AsRef<Path>) {
        self.download_dir = path.as_ref().to_string_lossy().into_owned();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

/// Read announce URLs from a list or from free-form text.
pub fn parse_trackers(text: &str) -> Vec<String> {
    clean_trackers(&[text.to_owned()])
}

fn clean_trackers(input: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for raw in input {
        for field in raw.split(|c: char| c == ',' || c == '\n' || c == ' ' || c == '\t') {
            let t = field.trim();
            if t.is_empty() || !seen.insert(t.to_owned()) {
                continue;
            }
            let lower = t.to_ascii_lowercase();
            if lower.starts_with("udp://")
                || lower.starts_with("http://")
                || lower.starts_with("https://")
                || lower.starts_with("wss://")
            {
                out.push(t.to_owned());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_seeding_switch_defaults_on() {
        let cfg: Config = serde_json::from_str(r#"{"download_dir":"/tmp/nori"}"#).unwrap();
        assert!(cfg.seeding_enabled);
    }

    #[test]
    fn trackers_are_cleaned_and_deduped() {
        let got = parse_trackers(
            "udp://a:1/announce, https://b:2/announce\nudp://a:1/announce ftp://nope",
        );
        assert_eq!(got, vec!["udp://a:1/announce", "https://b:2/announce"]);
    }

    #[test]
    fn enabled_sources_round_trip() {
        let mut cfg = Config::default();
        assert!(cfg.enabled("yts"));
        cfg.set_enabled("yts", false);
        assert!(!cfg.enabled("yts"));
        cfg.set_enabled("yts", true);
        assert!(cfg.enabled("yts"));
    }
}
