//! The on-disk mail index.
//!
//! Gmail's quota allows 300 mails a minute per user, so re-reading a mailbox
//! on every launch costs a minute of waiting and a minute of quota to produce
//! exactly what was on screen a second ago. The fix is to keep the index
//! locally and ask Gmail only what changed — the `historyId` cursor already
//! does that, but a cursor is only useful if there is something for it to be
//! applied *to*.
//!
//! So Nori writes down what it fetched. On launch the list is rebuilt from this
//! file in milliseconds, and the network is used for the handful of mails that
//! arrived since, not the whole mailbox.
//!
//! This holds Nori's own model rather than Gmail's, deliberately. Round-tripping
//! through Gmail's shape would mean mapping `Mailbox` back to `INBOX` and
//! inventing label ids for the custom ones, and any drift there would show up
//! as mail quietly jumping between folders.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{Email, EmailId, Label, LabelId};

/// Everything needed to rebuild the list without the network.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    /// The account this belongs to, so a file for one account is never loaded
    /// for another.
    #[serde(default)]
    pub account: String,
    pub emails: Vec<Email>,
    pub labels: Vec<Label>,
    /// Mail id -> label ids, exactly as `LabelStore` holds them.
    #[serde(default)]
    pub assignments: Vec<(EmailId, Vec<LabelId>)>,
    /// Where the next incremental sync starts. `None` forces a full fetch,
    /// which is the safe reading of a cursor that cannot be trusted.
    #[serde(default)]
    pub history_id: Option<String>,
}

impl Index {
    /// Whether there is anything worth showing. An index with no mail is
    /// indistinguishable from a first run, and treating it as one would wipe a
    /// mailbox the user legitimately emptied.
    pub fn is_empty(&self) -> bool {
        self.emails.is_empty()
    }
}

/// Whether `mail_count` mails may overwrite the cached `current` index.
///
/// Never when the store holds fewer mails than the file: an incremental
/// applied to an incomplete store, or any save racing a clear, must not
/// destroy the cache a full sync built. That is exactly how a mailbox
/// degrades to a single mail on disk while the server still holds
/// thousands. A legitimately emptied mailbox still converges, because the
/// next launch replays the deletes incrementally.
pub fn may_replace_index(current: &Index, mail_count: usize) -> bool {
    current.emails.len() <= mail_count
}

/// Reads and writes an [`Index`] beside the account's token.
pub struct IndexCache {
    path: PathBuf,
}

impl IndexCache {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn with_account(account: &str) -> Result<Self> {
        let base = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => {
                let home = std::env::var_os("HOME").context("HOME is not set")?;
                Path::new(&home).join(".config")
            }
        };
        Ok(Self::new(
            base.join("nori").join(format!("{account}.index.json")),
        ))
    }

    /// The stored index, or `None` on a first run.
    ///
    /// A file that will not parse is treated as absent rather than fatal: a
    /// corrupt cache should cost one slow sync, not a broken app.
    pub fn load(&self, account: &str) -> Option<Index> {
        let contents = std::fs::read_to_string(&self.path).ok()?;
        let index: Index = serde_json::from_str(&contents).ok()?;
        (index.account == account).then_some(index)
    }

    /// Write the index, atomically.
    ///
    /// Written to a sibling and renamed, so a crash part-way through leaves the
    /// previous index intact rather than a half-written file that fails to parse
    /// on the next launch — which would turn a hiccup into a full re-fetch.
    pub fn save(&self, index: &Index) -> Result<()> {
        let Some(parent) = self.path.parent() else {
            return Ok(());
        };
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        let json = serde_json::to_vec(index)?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, &json)
            .with_context(|| format!("writing {}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))?;
        restrict(&self.path);
        Ok(())
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The index holds sender addresses and subjects, so it is not world readable.
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_with(mails: usize) -> Index {
        Index {
            emails: (0..mails).map(|_| Email::default()).collect(),
            ..Index::default()
        }
    }

    #[test]
    fn a_thin_store_must_not_clobber_a_fuller_file() {
        assert!(!may_replace_index(&index_with(80), 1));
        assert!(!may_replace_index(&index_with(80), 0));
    }

    #[test]
    fn steady_state_and_growth_may_save() {
        assert!(may_replace_index(&index_with(80), 80));
        assert!(may_replace_index(&index_with(80), 83));
    }

    #[test]
    fn a_missing_file_is_no_veto() {
        assert!(may_replace_index(&Index::default(), 0));
    }
}
