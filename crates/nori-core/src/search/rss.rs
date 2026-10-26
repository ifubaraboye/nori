//! RSS helpers for the WordPress and Nyaa feeds.

use std::sync::LazyLock;

use regex::Regex;

use super::format::parse_unix_seconds;
use super::magnet::{normalize_hash, parse_magnet};
use super::model::SearchResult;

static ITEM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<item>(.*?)</item>").unwrap());
static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<title>(.*?)</title>").unwrap());
static DATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<pubDate>(.*?)</pubDate>").unwrap());
static MAGNET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)href="(magnet:\?xt=urn:btih:[^"]+)""#).unwrap());

pub fn items(xml: &str) -> Vec<&str> {
    ITEM_RE
        .captures_iter(xml)
        .filter_map(|c| c.get(1).map(|m| m.as_str()))
        .collect()
}

pub fn tag(re: &Regex, s: &str) -> String {
    re.captures(s)
        .and_then(|c| c.get(1))
        .map(|m| clean_text(m.as_str()))
        .unwrap_or_default()
}

/// Trim a tag's contents and unwrap it if the feed wrapped it in CDATA.
fn clean_text(raw: &str) -> String {
    let mut text = raw.trim();
    if let Some(rest) = text.strip_prefix("<![CDATA[") {
        text = rest.strip_suffix("]]>").unwrap_or(rest);
    }
    html_escape::decode_html_entities(text.trim())
        .trim()
        .to_owned()
}

/// Pull entries of a WordPress-style feed that carry a magnet in their body.
pub fn parse_items(xml: &str, source: &str) -> Vec<SearchResult> {
    let mut out = Vec::new();
    for item in items(xml) {
        let Some(caps) = MAGNET_RE.captures(item) else {
            continue;
        };
        let magnet = html_escape::decode_html_entities(&caps[1]).into_owned();
        let Some(parsed) = parse_magnet(&magnet) else {
            continue;
        };
        let name = tag(&TITLE_RE, item);
        let added = DATE_RE
            .captures(item)
            .map(|c| parse_unix_seconds(c[1].trim()))
            .unwrap_or(0);
        out.push(SearchResult {
            info_hash: parsed.hash,
            name: if name.is_empty() {
                "Unknown".into()
            } else {
                name
            },
            source: source.into(),
            magnet,
            added,
            ..Default::default()
        });
    }
    out
}

/// Nyaa's RSS uses namespaced tags and a CDATA title.
pub fn parse_nyaa(xml: &str) -> Vec<SearchResult> {
    static HASH_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)<nyaa:infoHash>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</nyaa:infoHash>").unwrap()
    });
    static SEEDS_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<nyaa:seeders>(.*?)</nyaa:seeders>").unwrap());
    static LEECH_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<nyaa:leechers>(.*?)</nyaa:leechers>").unwrap());
    static SIZE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<nyaa:size>(.*?)</nyaa:size>").unwrap());

    let mut out = Vec::new();
    for item in items(xml) {
        let hash = normalize_hash(&tag(&HASH_RE, item));
        let name = tag(&TITLE_RE, item);
        if hash.len() != 40 || name.is_empty() {
            continue;
        }
        let magnet = super::magnet::build_magnet(&hash, &name, &[]);
        out.push(SearchResult {
            info_hash: hash,
            name,
            size_bytes: super::format::parse_size(&tag(&SIZE_RE, item)),
            seeders: tag(&SEEDS_RE, item).parse().unwrap_or(0),
            leechers: tag(&LEECH_RE, item).parse().unwrap_or(0),
            source: "nyaa".into(),
            magnet,
            added: DATE_RE
                .captures(item)
                .map(|c| parse_unix_seconds(c[1].trim()))
                .unwrap_or(0),
            ..Default::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_wordpress_items_with_magnets() {
        let xml = r#"<?xml version="1.0"?><rss><channel>
<item>
  <title>Dune &amp; Friends</title>
  <pubDate>Wed, 25 Jun 2025 10:00:00 +0000</pubDate>
  <link>https://example.test/dune</link>
  <description><![CDATA[<a href="magnet:?xt=urn:btih:c12fe1c06bba254a9dc9f519b335aa7c1367a88a&amp;dn=Dune">magnet</a>]]></description>
</item>
<item><title>No magnet here</title></item>
</channel></rss>"#;
        let rows = parse_items(xml, "fitgirl");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Dune & Friends");
        assert_eq!(
            rows[0].info_hash,
            "c12fe1c06bba254a9dc9f519b335aa7c1367a88a"
        );
        assert_eq!(rows[0].source, "fitgirl");
        assert!(rows[0].added > 0);
    }

    #[test]
    fn reads_nyaa_namespaced_rows() {
        let xml = r#"<rss><channel><item>
  <title><![CDATA[Some Anime - 01 [1080p]]]></title>
  <nyaa:infoHash>c12fe1c06bba254a9dc9f519b335aa7c1367a88a</nyaa:infoHash>
  <nyaa:seeders>120</nyaa:seeders>
  <nyaa:leechers>8</nyaa:leechers>
  <nyaa:size>1.4 GiB</nyaa:size>
</item></channel></rss>"#;
        let rows = parse_nyaa(xml);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Some Anime - 01 [1080p]");
        assert_eq!(rows[0].seeders, 120);
        assert_eq!(rows[0].leechers, 8);
        assert_eq!(rows[0].size_bytes, 1_503_238_553);
        assert!(rows[0].magnet.contains("xt=urn:btih:c12fe1c06"));
    }

    #[test]
    fn drops_rows_without_a_usable_hash() {
        let xml = r#"<rss><channel><item>
  <title>Broken</title>
  <nyaa:infoHash>short</nyaa:infoHash>
</item></channel></rss>"#;
        assert!(parse_nyaa(xml).is_empty());
    }
}
