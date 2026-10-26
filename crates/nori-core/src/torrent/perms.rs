//! Keep completed files ordinary read/write files.

use std::path::Path;

/// Make every named file owner-writable (0644). Returns true when a file is
/// missing or could not be changed yet, so promotion can be retried.
#[cfg(unix)]
pub fn make_writable(root: &Path, paths: &[String]) -> bool {
    use std::os::unix::fs::PermissionsExt;

    let mut pending = false;
    for relative in paths {
        if relative
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        {
            continue;
        }
        let mut path = root.to_path_buf();
        for part in relative.split('/') {
            path.push(part);
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            if !path.exists() {
                pending = true;
            }
            continue;
        };
        if meta.permissions().mode() & 0o200 != 0 {
            continue;
        }
        if std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).is_err() {
            pending = true;
        }
    }
    pending
}

#[cfg(not(unix))]
pub fn make_writable(_root: &Path, _paths: &[String]) -> bool {
    false
}
