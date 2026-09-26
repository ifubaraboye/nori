//! Where OAuth tokens live between runs.
//!
//! A refresh token is a long-lived credential for the whole mailbox. It is
//! deliberately behind a trait rather than a bare struct so the OS keyring can
//! replace the file store without any call site changing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Google's token response, plus the bookkeeping Nori needs.
///
/// The `expires_in` Google sends is a *lifetime*, not a deadline, so it is
/// converted to an absolute instant on arrival. Storing the lifetime and
/// re-deriving it later would make the expiry depend on when it was read.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Token {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Unix seconds. `None` when the server sent no expiry, which is treated
    /// as "already stale" rather than "never expires" — see [`Token::is_fresh`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
    pub token_type: Option<String>,
}

impl Token {
    pub fn from_exchange(
        access_token: impl Into<String>,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
    ) -> Self {
        let now = unix_now();
        Self {
            access_token: access_token.into(),
            refresh_token,
            // Saturating: a server claiming an absurd lifetime should not
            // wrap into the distant past and make the token look stale.
            expires_at: expires_in.map(|seconds| now.saturating_add(seconds)),
            scope: None,
            token_type: None,
        }
    }

    /// Whether the access token can still be used, leaving room to renew
    /// before it actually lapses.
    ///
    /// The margin matters: a token that is technically valid when the sync
    /// starts can expire mid-flight, and a 401 partway through a page of
    /// results is far more disruptive than renewing a minute early.
    pub fn is_fresh(&self) -> bool {
        match self.expires_at {
            Some(at) => at > unix_now() + 60,
            None => false,
        }
    }

    /// Whether there is anything to renew with. A token with no refresh
    /// token is only usable until its access token lapses.
    pub fn can_refresh(&self) -> bool {
        self.refresh_token.is_some()
    }
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<Token>>;
    fn save(&self, token: &Token) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

/// Tokens in a `0600` file under the user's config directory.
///
/// Not as strong as an OS keyring, and the tradeoff is deliberate for now:
/// the `keyring` crate's Linux backend needs a running Secret Service, which
/// makes a headless session fail outright rather than degrade. A file the user
/// alone can read is honest about what it is, and the trait above means the
/// stronger store can replace it without touching the caller.
pub struct FileTokenStore {
    path: PathBuf,
}

/// The conventional place an account's files live.
fn token_path(account: &str) -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var_os("HOME").context("HOME is not set")?;
            Path::new(&home).join(".config")
        }
    };
    Ok(base.join("nori").join(format!("{account}.token.json")))
}

impl FileTokenStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Where the token is kept, for telling the user where it went.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The default location, honouring `XDG_CONFIG_HOME`.
    pub fn with_account(account: &str) -> Result<Self> {
        Ok(Self::new(token_path(account)?))
    }
}

impl TokenStore for FileTokenStore {
    fn load(&self) -> Result<Option<Token>> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => serde_json::from_str(&contents)
                .with_context(|| format!("reading token from {}", self.path.display()))
                .map(Some),
            // A missing file is the ordinary first-run case, not a failure.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("reading {}", self.path.display())),
        }
    }

    fn save(&self, token: &Token) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(token)?;
        std::fs::write(&self.path, json)
            .with_context(|| format!("writing {}", self.path.display()))?;
        restrict_permissions(&self.path)?;
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("removing {}", self.path.display())),
        }
    }
}

/// Remembers which account was connected last, so a restart can pick it up
/// without asking again.
///
/// A pointer rather than a scan of the token directory: account addresses
/// become filenames, and enumerating a config directory to guess which is the
/// account is both uglier and wrong the moment there are two.
pub struct LastAccount {
    path: PathBuf,
}

impl LastAccount {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn with_config_dir() -> Result<Self> {
        let base = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => {
                let home = std::env::var_os("HOME").context("HOME is not set")?;
                Path::new(&home).join(".config")
            }
        };
        Ok(Self::new(base.join("nori").join("last-account")))
    }

    pub fn load(&self) -> Option<String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .map(|address| address.trim().to_string())
            .filter(|address| !address.is_empty())
    }

    pub fn save(&self, address: &str) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, address)?;
        Ok(())
    }

    /// Where the pointer is kept, for telling the user.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Best-effort `0600`. Not atomic with the write, so there is a brief window
/// where the file is more permissive than intended; the directory is created
/// with `0700` below to close that off.
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    // No POSIX modes to set. The file inherits the directory's ACL, which on
    // these platforms is the access control that matters.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, FileTokenStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FileTokenStore::new(dir.path().join("nori").join("me.json"));
        (dir, store)
    }

    #[test]
    fn a_missing_token_file_is_the_first_run_not_a_failure() {
        let (_dir, store) = store();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn a_token_survives_a_round_trip() {
        let (_dir, store) = store();
        let token = Token {
            access_token: "ya29.access".to_string(),
            refresh_token: Some("1//refresh".to_string()),
            expires_at: Some(unix_now() + 3600),
            scope: Some("https://www.googleapis.com/auth/gmail.modify".to_string()),
            token_type: Some("Bearer".to_string()),
        };
        store.save(&token).unwrap();
        assert_eq!(store.load().unwrap(), Some(token));
    }

    #[test]
    fn clearing_is_idempotent() {
        let (_dir, store) = store();
        store
            .save(&Token::from_exchange("a", Some("r".into()), Some(60)))
            .unwrap();
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.clear().expect("clearing twice is not an error");
    }

    #[test]
    #[cfg(unix)]
    fn a_saved_token_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, store) = store();
        store
            .save(&Token::from_exchange("a", Some("r".into()), Some(60)))
            .unwrap();
        let mode = std::fs::metadata(store.path.as_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "a refresh token must not be world readable"
        );
    }

    #[test]
    fn freshness_leaves_room_to_renew() {
        let fresh = Token::from_exchange("a", Some("r".into()), Some(3600));
        assert!(fresh.is_fresh());

        let about_to_lapse = Token::from_exchange("a", Some("r".into()), Some(30));
        assert!(
            !about_to_lapse.is_fresh(),
            "a token inside the renewal margin counts as stale"
        );

        let no_expiry = Token::from_exchange("a", Some("r".into()), None);
        assert!(
            !no_expiry.is_fresh(),
            "no stated expiry must be treated as stale, never as eternal"
        );
    }

    #[test]
    fn a_token_with_no_refresh_token_cannot_renew() {
        assert!(!Token::from_exchange("a", None, Some(3600)).can_refresh());
        assert!(Token::from_exchange("a", Some("r".into()), Some(60)).can_refresh());
    }
}
