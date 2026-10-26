//! Magnet construction and parsing.

use std::collections::HashSet;

use base32::Alphabet;
use url::Url;

/// Public trackers a magnet gets when its source does not carry an announce
/// list. The HTTP entries keep discovery working where UDP is blocked.
pub const DEFAULT_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.dler.org:6969/announce",
    "http://tracker.opentrackr.org:1337/announce",
    "http://tracker.openbittorrent.com:80/announce",
    "http://tracker.dler.org:6969/announce",
    "https://tracker.tamersunion.org:443/announce",
];

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedMagnet {
    pub hash: String,
    pub name: String,
    pub magnet: String,
}

/// Percent-encode query values, leaving spaces as `+`.
pub fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

/// Build a magnet URI, with `extra` trackers first and the defaults after.
pub fn build_magnet(info_hash: &str, name: &str, extra: &[String]) -> String {
    let mut seen = HashSet::new();
    let mut out = format!(
        "magnet:?xt=urn:btih:{}&dn={}",
        info_hash,
        query_escape(name)
    );
    let defaults = DEFAULT_TRACKERS.iter().copied();
    for t in extra.iter().map(String::as_str).chain(defaults) {
        let t = t.trim();
        if t.is_empty() || !seen.insert(t.to_owned()) {
            continue;
        }
        out.push_str("&tr=");
        out.push_str(&query_escape(t));
    }
    out
}

/// Turn a base32 info hash into hex, and lowercase hex.
pub fn normalize_hash(raw: &str) -> String {
    let raw = raw.trim();
    if raw.len() == 32 {
        if let Some(bytes) = base32::decode(
            Alphabet::Rfc4648 { padding: false },
            &raw.to_ascii_uppercase(),
        ) {
            if bytes.len() == 20 {
                return bytes.iter().map(|b| format!("{b:02x}")).collect();
            }
        }
    }
    raw.to_ascii_lowercase()
}

/// Read an info hash and display name out of a magnet URI.
pub fn parse_magnet(s: &str) -> Option<ParsedMagnet> {
    let s = s.trim();
    if !s.to_ascii_lowercase().starts_with("magnet:?") {
        return None;
    }
    let url = Url::parse(s).ok()?;
    let mut raw = None;
    let mut name = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "xt" => raw = Some(value.into_owned()),
            "dn" => name = Some(value.into_owned()),
            _ => {}
        }
    }
    let raw = raw?;
    let prefix = "urn:btih:";
    if !raw.to_ascii_lowercase().starts_with(prefix) {
        return None;
    }
    let raw = &raw[prefix.len()..];
    if raw.len() != 40 && raw.len() != 32 {
        return None;
    }
    let hash = normalize_hash(raw);
    let name = name
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| hash.clone());
    Some(ParsedMagnet {
        hash,
        name,
        magnet: s.to_owned(),
    })
}

/// Whether the whole input is a hex (40) or base32 (32) info hash.
pub fn is_info_hash(s: &str) -> bool {
    let s = s.trim();
    match s.len() {
        40 => s.bytes().all(|b| b.is_ascii_hexdigit()),
        32 => s
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || (b'2'..=b'7').contains(&b)),
        _ => false,
    }
}

/// A magnet's announce list.
pub fn trackers_of(magnet: &str) -> Vec<String> {
    let Ok(url) = Url::parse(magnet.trim()) else {
        return Vec::new();
    };
    if !url.scheme().eq_ignore_ascii_case("magnet") {
        return Vec::new();
    }
    url.query_pairs()
        .filter(|(k, _)| k == "tr")
        .map(|(_, v)| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .collect()
}

/// Append the trackers `primary` is missing, keeping its other parameters byte for byte.
pub fn merge_trackers(primary: &str, others: &[String]) -> String {
    let own = trackers_of(primary);
    if own.is_empty() {
        return primary.to_owned();
    }
    let mut seen: HashSet<String> = own.into_iter().collect();
    let mut out = primary.trim().to_owned();
    let mut extra = Vec::new();
    for other in others {
        for t in trackers_of(other) {
            if seen.insert(t.clone()) {
                extra.push(t);
            }
        }
    }
    for t in extra {
        out.push_str("&tr=");
        out.push_str(&query_escape(&t));
    }
    out
}

/// `xl` parameter some indexers add to a magnet.
pub fn magnet_size(magnet: &str) -> i64 {
    Url::parse(magnet)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "xl")
                .and_then(|(_, v)| v.parse::<i64>().ok())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_magnet() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let m = build_magnet(hash, "A Movie [1080p]", &[]);
        let p = parse_magnet(&m).unwrap();
        assert_eq!(p.hash, hash);
        assert_eq!(p.name, "A Movie [1080p]");
        assert!(m.contains("&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce"));
    }

    #[test]
    fn normalizes_base32() {
        // 20 zero bytes in base32.
        assert_eq!(
            normalize_hash("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            "0000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn detects_hashes() {
        assert!(is_info_hash("0123456789abcdef0123456789abcdef01234567"));
        assert!(!is_info_hash("nope"));
    }
}
