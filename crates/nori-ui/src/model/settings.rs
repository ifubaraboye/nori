/// Settings page model.
///
/// The prototype has no persistence layer, so these values live in memory
/// for the lifetime of the `SettingsView` that owns them. Layout and wording
/// follow Waku's settings shell (nav column + capped, card-based content);
/// the fields themselves are Nori's own mail settings.
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

    /// Extra terms the nav search matches on, beyond the label itself.
    pub fn keywords(self) -> &'static str {
        match self {
            Self::General => "general notifications unread badges privacy read confirm",
            Self::Appearance => "appearance theme dark rows density compact contrast",
            Self::Mail => "mail tabs conversation threads preview attachments",
            Self::Account => "account email address imap sync receipts signature",
            Self::About => "about version build gpui license",
        }
    }

    /// The pages a nav query leaves visible, in nav order. `query` must
    /// already be trimmed and lowercased; an empty query keeps every page.
    pub fn visible(query: &str) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|page| {
                query.is_empty()
                    || page.label().to_lowercase().contains(query)
                    || page.keywords().contains(query)
            })
            .collect()
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
            Self::OpenInTab => "Keep a tab for every opened message so you can jump back.",
            Self::GroupConversations => "Thread replies together under the most recent message.",
            Self::ShowAttachments => "Render attachment chips inline instead of a footer list.",
            Self::CheckForMail => "Refresh the mailbox list on a timer while the app is open.",
            Self::ReadReceipts => "Tell senders when you have read their message.",
        }
    }
}

/// In-memory settings. Every value is a prototype default; nothing here is
/// written to disk yet.
#[derive(Clone, Copy, Debug)]
pub struct SettingsState {
    pub mark_read_on_open: bool,
    pub unread_badges: bool,
    pub confirm_before_archive: bool,
    pub compact_rows: bool,
    pub show_sender: bool,
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
            open_in_tab: true,
            group_conversations: false,
            show_attachments: false,
            check_for_mail: true,
            read_receipts: false,
        }
    }

    pub fn get(self, setting: Setting) -> bool {
        match setting {
            Setting::MarkReadOnOpen => self.mark_read_on_open,
            Setting::UnreadBadges => self.unread_badges,
            Setting::ConfirmBeforeArchive => self.confirm_before_archive,
            Setting::CompactRows => self.compact_rows,
            Setting::ShowSender => self.show_sender,
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
            Setting::OpenInTab => self.open_in_tab = next,
            Setting::GroupConversations => self.group_conversations = next,
            Setting::ShowAttachments => self.show_attachments = next,
            Setting::CheckForMail => self.check_for_mail = next,
            Setting::ReadReceipts => self.read_receipts = next,
        }
        next
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_keeps_every_page() {
        assert_eq!(SettingsPage::visible(""), SettingsPage::ALL.to_vec());
    }

    #[test]
    fn query_matches_label_or_keywords() {
        let matched = SettingsPage::visible("thread");
        assert_eq!(matched, vec![SettingsPage::Mail]);
        assert_eq!(SettingsPage::visible("version"), vec![SettingsPage::About]);
        assert!(SettingsPage::visible("nothing here").is_empty());
    }

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
}
