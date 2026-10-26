//! Size and date formatting shared by the list and the details pane.

use chrono::{DateTime, NaiveDateTime, Utc};
use regex::Regex;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

static SIZE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*([\d.]+)\s*([kmgt]i?b)?\s*$").unwrap());

/// Read a human size such as `1.4 GiB` or `700 MB`.
pub fn parse_size(s: &str) -> i64 {
    let Some(caps) = SIZE_RE.captures(s) else {
        return 0;
    };
    let Ok(value) = caps[1].parse::<f64>() else {
        return 0;
    };
    let mult = match caps
        .get(2)
        .map(|m| m.as_str().to_ascii_uppercase())
        .as_deref()
    {
        Some("KB") => 1e3,
        Some("MB") => 1e6,
        Some("GB") => 1e9,
        Some("TB") => 1e12,
        Some("KIB") => 1024.0,
        Some("MIB") => 1024.0 * 1024.0,
        Some("GIB") => 1024.0 * 1024.0 * 1024.0,
        Some("TIB") => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    (value * mult) as i64
}

/// Read a feed date into Unix seconds.
pub fn parse_unix_seconds(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    if let Ok(t) = DateTime::parse_from_rfc2822(s) {
        return t.timestamp();
    }
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return t.timestamp();
    }
    for fmt in ["%a, %e %b %Y %H:%M:%S %z", "%a, %e %b %Y %H:%M:%S"] {
        if let Ok(t) = DateTime::parse_from_str(s, fmt) {
            return t.timestamp();
        }
    }
    if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return t.and_utc().timestamp();
    }
    0
}

/// Format a byte count for the list.
pub fn human_size(n: i64) -> String {
    if n <= 0 {
        return "—".into();
    }
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Format how long ago something was added.
pub fn human_age(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let secs = (now - unix).max(0);
    if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else if secs < 30 * 86_400 {
        format!("{}d", secs / 86_400)
    } else if secs < 365 * 86_400 {
        format!("{}mo", secs / (30 * 86_400))
    } else {
        format!("{}y", secs / (365 * 86_400))
    }
}

/// Seconds since the Unix epoch.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn iso_unix(s: &str) -> i64 {
    if s.is_empty() {
        return 0;
    }
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.timestamp())
        .unwrap_or(0)
}

pub fn utc_from_ymd(year: i32, month: u32, day: u32) -> i64 {
    use chrono::TimeZone;
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0)
        .single()
        .map(|t| t.timestamp())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse_common_suffixes() {
        assert_eq!(parse_size("1.4 GiB"), 1_503_238_553);
        assert_eq!(parse_size("700 MB"), 700_000_000);
        assert_eq!(parse_size("2 KB"), 2_000);
        assert_eq!(parse_size("nope"), 0);
    }

    #[test]
    fn human_sizes_round() {
        assert_eq!(human_size(0), "—");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1024), "1.0 KB");
    }
}
