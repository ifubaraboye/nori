//! Settings page model, and the file it is kept in.
//!
//! Layout and wording follow Waku's settings shell (nav column + capped,
//! card-based content); the fields themselves are Nori's own mail settings.
//! They are written to disk on every change, so a switch flipped in Settings
//! is still flipped the next time the app opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    General,
    Appearance,
    Mail,
    Account,
    About,
}

impl SettingsPage {
    /// Every page, in nav order.
    pub const ALL: [Self; 5] = [
        Self::General,
        Self::Appearance,
        Self::Mail,
        Self::Account,
        Self::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Mail => "Mail",
            Self::Account => "Account",
            Self::About => "About",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::General => "icons/settings.svg",
            Self::Appearance => "icons/appearance.svg",
            Self::Mail => "icons/mail.svg",
            Self::Account => "icons/user.svg",
            Self::About => "icons/info.svg",
        }
    }

    /// The nav row's element id. Static so tests can look it up directly.
    pub fn nav_id(self) -> &'static str {
        match self {
            Self::General => "settings-nav-page-general",
            Self::Appearance => "settings-nav-page-appearance",
            Self::Mail => "settings-nav-page-mail",
            Self::Account => "settings-nav-page-account",
            Self::About => "settings-nav-page-about",
        }
    }
}

/// One toggle row on a settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    MarkReadOnOpen,
    UnreadBadges,
    ConfirmBeforeArchive,
    CompactRows,
    ShowSender,
    LightMode,
    OpenInTab,
    GroupConversations,
    ShowAttachments,
    CheckForMail,
    ReadReceipts,
}

impl Setting {
    pub fn label(self) -> &'static str {
        match self {
            Self::MarkReadOnOpen => "Mark as read on open",
            Self::UnreadBadges => "Unread count badges",
            Self::ConfirmBeforeArchive => "Confirm before archiving",
            Self::CompactRows => "Tighten the mail list to one line per message",
            Self::ShowSender => "Show sender in message view",
            Self::LightMode => "Light mode",
            Self::OpenInTab => "Open messages in a tab",
            Self::GroupConversations => "Group conversations",
            Self::ShowAttachments => "Show attachments inline",
            Self::CheckForMail => "Check for new mail every 5 minutes",
            Self::ReadReceipts => "Send read receipts",
        }
    }

    /// The switch element's id. Static, so it is usable as a debug selector
    /// in visual tests.
    pub fn element_id(self) -> &'static str {
        match self {
            Self::MarkReadOnOpen => "settings-toggle-mark-read-on-open",
            Self::UnreadBadges => "settings-toggle-unread-badges",
            Self::ConfirmBeforeArchive => "settings-toggle-confirm-before-archive",
            Self::CompactRows => "settings-toggle-compact-rows",
            Self::ShowSender => "settings-toggle-show-sender",
            Self::LightMode => "settings-toggle-light-mode",
            Self::OpenInTab => "settings-toggle-open-in-tab",
            Self::GroupConversations => "settings-toggle-group-conversations",
            Self::ShowAttachments => "settings-toggle-show-attachments",
            Self::CheckForMail => "settings-toggle-check-for-mail",
            Self::ReadReceipts => "settings-toggle-read-receipts",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::MarkReadOnOpen => {
                "Clear the unread dot as soon as a message is opened, the way most clients do."
            }
            Self::UnreadBadges => "Show unread totals next to each mailbox in the sidebar.",
            Self::ConfirmBeforeArchive => "Ask before moving messages out of the current mailbox.",
            Self::CompactRows => {
                "Show sender, label, subject and preview on one line instead of three, so \
                 roughly twice as many messages fit on screen."
            }
            Self::ShowSender => "Keep the sender line visible above the message body.",
            Self::LightMode => {
                "Use the light palette. The switch takes effect immediately and is \
                 remembered for next time."
            }
            Self::OpenInTab => "Keep a tab for every opened message so you can jump back.",
            Self::GroupConversations => "Thread replies together under the most recent message.",
            Self::ShowAttachments => "Render attachment chips inline instead of a footer list.",
            Self::CheckForMail => "Refresh the mailbox list on a timer while the app is open.",
            Self::ReadReceipts => "Tell senders when you have read their message.",
        }
    }
}

/// Every setting, and what it defaults to.
///
/// `Default` is `new()` on purpose. It is what a missing field in an older or
/// partial settings file falls back to, so the defaults in one place are also
/// the defaults for anything added later.
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsState {
    pub mark_read_on_open: bool,
    pub unread_badges: bool,
    pub confirm_before_archive: bool,
    pub compact_rows: bool,
    pub show_sender: bool,
    pub light_mode: bool,
    pub open_in_tab: bool,
    pub group_conversations: bool,
    pub show_attachments: bool,
    pub check_for_mail: bool,
    pub read_receipts: bool,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            mark_read_on_open: true,
            unread_badges: true,
            confirm_before_archive: false,
            // Compact is the default: the three-line row is the one to opt
            // into when you want more preview per message, not the other way
            // around.
            compact_rows: true,
            show_sender: true,
            // Dark is the default palette, and what a launch with no
            // settings file — or one written before this setting existed —
            // starts from.
            light_mode: false,
            open_in_tab: true,
            group_conversations: false,
            show_attachments: false,
            check_for_mail: true,
            read_receipts: false,
        }
    }
}

impl SettingsState {
    pub fn get(self, setting: Setting) -> bool {
        match setting {
            Setting::MarkReadOnOpen => self.mark_read_on_open,
            Setting::UnreadBadges => self.unread_badges,
            Setting::ConfirmBeforeArchive => self.confirm_before_archive,
            Setting::CompactRows => self.compact_rows,
            Setting::ShowSender => self.show_sender,
            Setting::LightMode => self.light_mode,
            Setting::OpenInTab => self.open_in_tab,
            Setting::GroupConversations => self.group_conversations,
            Setting::ShowAttachments => self.show_attachments,
            Setting::CheckForMail => self.check_for_mail,
            Setting::ReadReceipts => self.read_receipts,
        }
    }

    /// Assign one setting. Returns true when the value actually changed, so
    /// callers can skip a redraw when it did not.
    pub fn set(&mut self, setting: Setting, enabled: bool) -> bool {
        if self.get(setting) == enabled {
            return false;
        }
        // Toggle moves to the opposite value, which is the one we want
        // precisely because `get` just said the two differ.
        self.toggle(setting);
        true
    }

    /// Flip one setting and report the new value, so callers can announce
    /// the change to assistive tech.
    pub fn toggle(&mut self, setting: Setting) -> bool {
        let next = !self.get(setting);
        match setting {
            Setting::MarkReadOnOpen => self.mark_read_on_open = next,
            Setting::UnreadBadges => self.unread_badges = next,
            Setting::ConfirmBeforeArchive => self.confirm_before_archive = next,
            Setting::CompactRows => self.compact_rows = next,
            Setting::ShowSender => self.show_sender = next,
            Setting::LightMode => self.light_mode = next,
            Setting::OpenInTab => self.open_in_tab = next,
            Setting::GroupConversations => self.group_conversations = next,
            Setting::ShowAttachments => self.show_attachments = next,
            Setting::CheckForMail => self.check_for_mail = next,
            Setting::ReadReceipts => self.read_receipts = next,
        }
        next
    }
}

/// The defaults, by another name.
///
/// `serde` needs this: a field absent from a settings file written by an
/// older version falls back to `Default`, so this is also how a setting added
/// in a later release behaves for someone who already had a file. Routing it
/// through `new()` keeps the two from drifting apart.
impl Default for SettingsState {
    fn default() -> Self {
        Self::new()
    }
}

/// The settings file, beside the account files in the config directory.
///
/// Named once and not per account: these are preferences, not mail, so they
/// follow the person rather than the mailbox.
pub struct SettingsStore {
    path: std::path::PathBuf,
}

impl SettingsStore {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The default location, honouring `XDG_CONFIG_HOME` the way the token and
    /// index files already do, so settings are not the one thing living
    /// somewhere else.
    ///
    /// Not compiled into tests: a test builds its store with [`Self::new`] and
    /// a path of its own, so nothing here can be talked into writing over the
    /// settings of whoever is running the suite.
    #[cfg(not(test))]
    pub fn with_config_dir() -> anyhow::Result<Self> {
        let base = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
            _ => {
                let home =
                    std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
                std::path::PathBuf::from(home).join(".config")
            }
        };
        Ok(Self::new(base.join("nori").join("settings.json")))
    }

    /// The stored settings, or `None` on a first run.
    ///
    /// A file that will not parse is treated as absent rather than fatal, for
    /// the same reason the mail index does it: losing a settings file costs the
    /// defaults, and refusing to start would cost the user their mailbox.
    pub fn load(&self) -> Option<SettingsState> {
        let contents = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    /// Write the settings, atomically.
    ///
    /// Written to a sibling and renamed, so an interrupted write cannot leave
    /// a truncated file that fails to parse on the next launch and silently
    /// resets every switch the user had flipped.
    pub fn save(&self, state: &SettingsState) -> anyhow::Result<()> {
        use anyhow::Context as _;

        let Some(parent) = self.path.parent() else {
            return Ok(());
        };
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(state)?)
            .with_context(|| format!("writing {}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_flips_and_reports_the_new_value() {
        let mut state = SettingsState::new();
        assert!(state.mark_read_on_open);
        assert!(!state.toggle(Setting::MarkReadOnOpen));
        assert!(!state.get(Setting::MarkReadOnOpen));
        assert!(state.toggle(Setting::MarkReadOnOpen));
        // Unrelated settings are untouched.
        assert!(state.get(Setting::UnreadBadges));
    }

    #[test]
    fn setting_a_value_reports_whether_it_moved() {
        let mut state = SettingsState::new();
        // Compact is the default, so this one starts on.
        assert!(state.get(Setting::CompactRows));
        assert!(!state.set(Setting::CompactRows, true));
        assert!(state.set(Setting::CompactRows, false));
        assert!(!state.get(Setting::CompactRows));
        assert!(state.set(Setting::CompactRows, true));
        assert!(state.get(Setting::CompactRows));
    }

    #[test]
    fn compact_rows_are_on_out_of_the_box() {
        assert!(SettingsState::new().compact_rows);
    }

    /// A switch flipped in Settings has to still be flipped next launch.
    ///
    /// The whole point of the file: without it every launch started from
    /// `new()` and the user's choices were gone.
    #[test]
    fn a_flipped_setting_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("nori-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let store = SettingsStore::new(dir.join("settings.json"));

        // First run: defaults, nothing on disk.
        assert!(store.load().is_none(), "a first run has no settings file");

        // Flip two settings and write them out.
        let mut state = SettingsState::new();
        assert!(state.set(Setting::LightMode, true));
        assert!(state.set(Setting::CompactRows, false));
        store.save(&state).expect("save");

        // Next launch.
        let reloaded = store.load().expect("the file is there");
        assert!(reloaded.get(Setting::LightMode), "light mode must persist");
        assert!(
            !reloaded.get(Setting::CompactRows),
            "and so must the row density"
        );
        // Untouched settings keep their defaults rather than becoming false.
        assert!(
            reloaded.get(Setting::UnreadBadges),
            "a setting nobody touched must come back at its default, not false"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file written before a setting existed must still load, with the new
    /// one at its default rather than refusing the whole file.
    #[test]
    fn a_file_from_an_older_version_still_loads() {
        let dir = std::env::temp_dir().join(format!("nori-settings-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("settings.json");

        // One field, as if only that setting had been written back then.
        std::fs::write(&path, r#"{"lightMode":true}"#).expect("write");
        let state: SettingsState = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
            .expect("a partial file must still parse");
        assert!(
            state.get(Setting::LightMode),
            "the field that is there is read"
        );
        assert!(
            state.get(Setting::MarkReadOnOpen),
            "and the ones that are not fall back to the defaults"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A truncated or hand-edited file must not stop the app starting.
    #[test]
    fn an_unreadable_file_falls_back_to_the_defaults() {
        let dir = std::env::temp_dir().join(format!("nori-settings-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("settings.json");

        std::fs::write(&path, "{ not json").expect("write");
        let store = SettingsStore::new(&path);
        assert!(
            store.load().is_none(),
            "a corrupt file reads as a first run, not a failure to start"
        );
        assert!(
            SettingsState::default().get(Setting::UnreadBadges),
            "and the defaults a first run falls back to are the real ones"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn light_mode_is_off_out_of_the_box() {
        // Dark out of the box: there is no settings file on a first run.
        let mut state = SettingsState::new();
        assert!(!state.get(Setting::LightMode));
        assert!(state.toggle(Setting::LightMode));
        assert!(state.get(Setting::LightMode));
        assert!(state.set(Setting::LightMode, false));
        assert!(!state.get(Setting::LightMode));
    }
}
