//! Playlist generation for finished downloads.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

const MEDIA_EXTENSIONS: &[&str] = &[
    "aac", "aiff", "avi", "flac", "flv", "m2ts", "m4a", "m4v", "mka", "mkv", "mov", "mp3", "mp4",
    "mpeg", "mpg", "oga", "ogg", "ogv", "opus", "wav", "webm", "wma", "wmv",
];

/// Drop a `playlist.m3u` into every folder of a finished torrent that holds
/// more than one playable file. Existing user playlists are never replaced.
pub fn write_playlists(root: &Path, paths: &[String]) {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    let mut files: HashSet<String> = HashSet::new();

    for raw in paths {
        let file = raw.replace('\\', "/");
        files.insert(file.clone());
        let parts: Vec<&str> = file.split('/').collect();
        if parts.len() < 2 || file.contains('\\') || file.contains('\0') {
            continue;
        }
        if parts
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == "..")
        {
            continue;
        }
        let ext = file
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default();
        if !MEDIA_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        for i in 1..parts.len() {
            let dir = parts[..i].join("/");
            let entry = parts[i..].join("/");
            groups.entry(dir).or_default().push(entry);
        }
    }

    for (dir, mut entries) in groups {
        let target = format!("{dir}/playlist.m3u");
        if entries.len() < 2 || files.contains(&target) {
            continue;
        }
        entries.sort_by(|a, b| natural_cmp(a, b));
        let mut body = String::from("#EXTM3U\n");
        for entry in entries {
            body.push_str("./");
            body.push_str(&entry);
            body.push('\n');
        }
        let full = join_inside(root, &target);
        let Some(full) = full else {
            continue;
        };
        let Ok(mut file) = OpenOptions::new().write(true).create_new(true).open(full) else {
            continue;
        };
        let _ = file.write_all(body.as_bytes());
    }
}

fn join_inside(root: &Path, relative: &str) -> Option<PathBuf> {
    if relative
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return None;
    }
    let mut out = root.to_path_buf();
    for part in relative.split('/') {
        out.push(part);
    }
    Some(out)
}

/// Compare names the way a file manager does: `Lesson 2` before `Lesson 10`.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut ai, mut bi) = (0, 0);
    while ai < a.len() && bi < b.len() {
        if a[ai].is_ascii_digit() && b[bi].is_ascii_digit() {
            let (an, a_end) = number_at(a, ai);
            let (bn, b_end) = number_at(b, bi);
            match an.cmp(&bn) {
                std::cmp::Ordering::Equal => {
                    ai = a_end;
                    bi = b_end;
                    continue;
                }
                other => return other,
            }
        }
        if a[ai] != b[bi] {
            return a[ai].cmp(&b[bi]);
        }
        ai += 1;
        bi += 1;
    }
    a.len().cmp(&b.len())
}

fn number_at(s: &[u8], start: usize) -> (u64, usize) {
    let mut value = 0u64;
    let mut end = start;
    while end < s.len() && s[end].is_ascii_digit() {
        value = value
            .saturating_mul(10)
            .saturating_add((s[end] - b'0') as u64);
        end += 1;
    }
    (value, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_sorts_numbers_by_value() {
        assert_eq!(
            natural_cmp("Lesson 2", "Lesson 10"),
            std::cmp::Ordering::Less
        );
        assert_eq!(natural_cmp("a", "b"), std::cmp::Ordering::Less);
    }
}
