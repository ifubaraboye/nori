#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EmailId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mailbox {
    Inbox,
    Starred,
    Sent,
    Drafts,
    Archive,
    Trash,
}

impl Mailbox {
    pub const NAV_ITEMS: [Self; 6] = [
        Self::Inbox,
        Self::Starred,
        Self::Sent,
        Self::Drafts,
        Self::Archive,
        Self::Trash,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Starred => "Starred",
            Self::Sent => "Sent",
            Self::Drafts => "Drafts",
            Self::Archive => "Archive",
            Self::Trash => "Trash",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Email {
    pub id: EmailId,
    pub sender: String,
    pub address: String,
    pub recipients: Vec<String>,
    pub subject: String,
    pub preview: String,
    pub body: Vec<String>,
    pub timestamp: String,
    pub full_date: String,
    pub mailbox: Mailbox,
    pub unread: bool,
    pub starred: bool,
    /// Pinned mails are the only ones that get a tab in the tab strip, and
    /// they survive switching mailboxes. Unpinned mails open as a transient
    /// view with no tab of their own.
    pub pinned: bool,
    /// Optional category tag, rendered as a chip in the compact list. `None`
    /// means the mail is untagged and the row simply shows no chip.
    pub label: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EmailSummary {
    pub id: EmailId,
    pub sender: String,
    pub subject: String,
    pub preview: String,
    pub timestamp: String,
    pub unread: bool,
    pub starred: bool,
    pub label: Option<String>,
}

impl Email {
    pub fn summary(&self) -> EmailSummary {
        EmailSummary {
            id: self.id,
            sender: self.sender.clone(),
            subject: self.subject.clone(),
            preview: self.preview.clone(),
            timestamp: self.timestamp.clone(),
            unread: self.unread,
            starred: self.starred,
            label: self.label.clone(),
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.sender.to_lowercase().contains(&query)
            || self.subject.to_lowercase().contains(&query)
            || self.preview.to_lowercase().contains(&query)
            || self
                .body
                .iter()
                .any(|paragraph| paragraph.to_lowercase().contains(&query))
    }
}

#[derive(Clone, Debug, Default)]
pub struct DraftSeed {
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceView {
    Mailbox,
    Email(EmailId),
}

#[derive(Clone, Debug)]
pub enum Overlay {
    Search,
    Compose,
}

/// One stop in the sidebar arrows' history.
///
/// `WorkspaceView::Mailbox` alone does not say *which* mailbox, so retracing
/// from a Trash mail back to the list has to restore the mailbox and the row
/// cursor as well as the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavEntry {
    pub view: WorkspaceView,
    pub mailbox: Mailbox,
    pub selected_index: usize,
}

#[derive(Debug)]
pub struct MailStore {
    emails: Vec<Email>,
    selected_mailbox: Mailbox,
    selected_index: usize,
    tabs: Vec<EmailId>,
    active_tab: Option<EmailId>,
    workspace_view: WorkspaceView,
    overlay: Option<Overlay>,
    history_back: Vec<NavEntry>,
    history_forward: Vec<NavEntry>,
}

impl MailStore {
    pub fn new(emails: Vec<Email>) -> Self {
        Self {
            emails,
            selected_mailbox: Mailbox::Inbox,
            selected_index: 0,
            tabs: Vec::new(),
            active_tab: None,
            workspace_view: WorkspaceView::Mailbox,
            overlay: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
        }
    }

    pub fn emails(&self) -> &[Email] {
        &self.emails
    }

    pub fn selected_mailbox(&self) -> Mailbox {
        self.selected_mailbox
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn tabs(&self) -> &[EmailId] {
        &self.tabs
    }

    pub fn active_tab(&self) -> Option<EmailId> {
        self.active_tab
    }

    pub fn workspace_view(&self) -> WorkspaceView {
        self.workspace_view
    }

    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    pub fn select_mailbox(&mut self, mailbox: Mailbox) {
        if self.selected_mailbox == mailbox && self.workspace_view == WorkspaceView::Mailbox {
            return;
        }
        // Switching mailboxes is a navigation step the arrows can retrace,
        // so opening a mail in Trash and going back lands in the inbox.
        self.push_history();
        self.selected_mailbox = mailbox;
        self.selected_index = 0;
        self.workspace_view = WorkspaceView::Mailbox;
        self.active_tab = None;
    }

    pub fn visible_emails(&self) -> Vec<&Email> {
        self.emails
            .iter()
            .filter(|email| match self.selected_mailbox {
                Mailbox::Inbox => email.mailbox == Mailbox::Inbox,
                Mailbox::Starred => email.starred,
                mailbox => email.mailbox == mailbox,
            })
            .collect()
    }

    pub fn count(&self, mailbox: Mailbox) -> usize {
        self.emails
            .iter()
            .filter(|email| match mailbox {
                Mailbox::Inbox => email.mailbox == Mailbox::Inbox,
                Mailbox::Starred => email.starred,
                mailbox => email.mailbox == mailbox,
            })
            .count()
    }

    pub fn visible_summaries(&self) -> Vec<EmailSummary> {
        self.visible_emails()
            .into_iter()
            .map(|email| email.summary())
            .collect()
    }

    pub fn selected_email(&self) -> Option<&Email> {
        self.visible_emails().get(self.selected_index).copied()
    }

    pub fn email(&self, id: EmailId) -> Option<&Email> {
        self.emails.iter().find(|email| email.id == id)
    }

    pub fn open_email(&mut self, id: EmailId) -> bool {
        let Some(email) = self.emails.iter_mut().find(|email| email.id == id) else {
            return false;
        };
        email.unread = false;
        let pinned = email.pinned;
        self.push_history();
        // Only a pinned mail earns a tab; an unpinned one opens transiently.
        if pinned && !self.tabs.contains(&id) {
            self.tabs.push(id);
        }
        self.active_tab = if pinned { Some(id) } else { None };
        self.workspace_view = WorkspaceView::Email(id);
        true
    }

    /// Pin or unpin a mail. Pinning adds its tab; unpinning removes the tab
    /// and, if it was the open view, returns to the mailbox.
    pub fn toggle_pin(&mut self, id: EmailId) -> bool {
        let Some(email) = self.emails.iter_mut().find(|email| email.id == id) else {
            return false;
        };
        email.pinned = !email.pinned;
        if email.pinned {
            if !self.tabs.contains(&id) {
                self.tabs.push(id);
            }
            if self.workspace_view == WorkspaceView::Email(id) {
                self.active_tab = Some(id);
            }
        } else {
            self.tabs.retain(|tab| *tab != id);
            if self.active_tab == Some(id) {
                self.active_tab = None;
            }
            if self.workspace_view == WorkspaceView::Email(id) {
                self.workspace_view = WorkspaceView::Mailbox;
            }
        }
        true
    }

    /// Whether there is anywhere to go back to, and anywhere to go forward to.
    /// The sidebar's chevrons follow these, Waku-style.
    pub fn can_go_back(&self) -> bool {
        !self.history_back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.history_forward.is_empty()
    }

    /// Step back through visited views, recording the current one so it can be
    /// returned to with `go_forward`. Restores the mailbox and row cursor too,
    /// so backing out of a Trash mail returns to the mailbox you came from.
    pub fn go_back(&mut self) {
        let Some(previous) = self.history_back.pop() else {
            return;
        };
        self.history_forward.push(self.current_entry());
        self.restore_entry(previous);
    }

    /// Step forward through views already visited back out of.
    pub fn go_forward(&mut self) {
        let Some(next) = self.history_forward.pop() else {
            return;
        };
        self.history_back.push(self.current_entry());
        self.restore_entry(next);
    }

    fn current_entry(&self) -> NavEntry {
        NavEntry {
            view: self.workspace_view,
            mailbox: self.selected_mailbox,
            selected_index: self.selected_index,
        }
    }

    fn restore_entry(&mut self, entry: NavEntry) {
        self.workspace_view = entry.view;
        self.selected_mailbox = entry.mailbox;
        self.selected_index = entry.selected_index;
        self.active_tab = match entry.view {
            WorkspaceView::Email(id) => self.tabs.contains(&id).then_some(id),
            WorkspaceView::Mailbox => None,
        };
    }

    /// Record a navigation so the arrows can retrace it.
    fn push_history(&mut self) {
        if self.history_back.last() != Some(&self.current_entry()) {
            self.history_back.push(self.current_entry());
        }
        self.history_forward.clear();
    }

    pub fn close_tab(&mut self, id: EmailId) {
        let Some(index) = self.tabs.iter().position(|tab| *tab == id) else {
            return;
        };
        let was_active = self.active_tab == Some(id);
        self.tabs.remove(index);

        if !was_active {
            return;
        }

        self.active_tab = if self.tabs.is_empty() {
            self.workspace_view = WorkspaceView::Mailbox;
            None
        } else if index < self.tabs.len() {
            Some(self.tabs[index])
        } else {
            let previous = self.tabs[index - 1];
            self.workspace_view = WorkspaceView::Email(previous);
            Some(previous)
        };
    }

    pub fn close_active_tab(&mut self) {
        if let Some(id) = self.active_tab {
            self.close_tab(id);
        }
    }

    pub fn cycle_tab(&mut self, direction: i32) {
        if self.tabs.is_empty() {
            return;
        }
        let current = self
            .active_tab
            .and_then(|active| self.tabs.iter().position(|tab| *tab == active))
            .unwrap_or(0) as i32;
        let len = self.tabs.len() as i32;
        let next = (current + direction).rem_euclid(len) as usize;
        let id = self.tabs[next];
        self.active_tab = Some(id);
        self.workspace_view = WorkspaceView::Email(id);
        if let Some(email) = self.emails.iter_mut().find(|email| email.id == id) {
            email.unread = false;
        }
    }

    pub fn move_selection(&mut self, delta: i32) {
        let len = self.visible_emails().len();
        if len == 0 {
            self.selected_index = 0;
            return;
        }
        let current = self.selected_index as i32;
        self.selected_index = (current + delta).rem_euclid(len as i32) as usize;
    }

    pub fn toggle_star(&mut self, id: EmailId) {
        if let Some(email) = self.emails.iter_mut().find(|email| email.id == id) {
            email.starred = !email.starred;
        }
    }

    pub fn set_overlay(&mut self, overlay: Option<Overlay>) {
        self.overlay = overlay;
    }

    /// Dismiss an open overlay; without one, fall through to `go_back`.
    pub fn dismiss(&mut self) {
        if self.overlay.take().is_some() {
            return;
        }
        self.go_back();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::mock::mock_emails;

    fn store() -> MailStore {
        MailStore::new(mock_emails())
    }

    #[test]
    fn opening_duplicate_email_activates_existing_tab() {
        let mut store = store();
        let id = store.visible_emails()[0].id;
        // Only a pinned mail earns a tab, so pin before opening.
        store.toggle_pin(id);
        assert!(store.open_email(id));
        store.close_tab(id);
        assert!(store.open_email(id));
        assert!(store.open_email(id));
        assert_eq!(store.tabs(), &[id]);
    }

    #[test]
    fn unpinned_mail_opens_without_a_tab() {
        let mut store = store();
        let id = store.visible_emails()[0].id;
        assert!(store.open_email(id));
        assert!(store.tabs().is_empty());
        assert_eq!(store.active_tab(), None);
        assert_eq!(store.workspace_view(), WorkspaceView::Email(id));
    }

    #[test]
    fn pinning_adds_a_tab_and_unpinning_removes_it() {
        let mut store = store();
        let id = store.visible_emails()[0].id;
        assert!(store.toggle_pin(id));
        assert_eq!(store.tabs(), &[id]);
        assert!(store.open_email(id));
        assert_eq!(store.active_tab(), Some(id));
        // Unpinning the open mail drops its tab and returns to the mailbox.
        store.toggle_pin(id);
        assert!(store.tabs().is_empty());
        assert_eq!(store.workspace_view(), WorkspaceView::Mailbox);
    }

    #[test]
    fn back_returns_to_the_mailbox_and_forward_reopens_the_mail() {
        let mut store = store();
        let id = store.visible_emails()[0].id;
        store.toggle_pin(id);
        assert!(!store.can_go_back());
        store.open_email(id);
        assert!(store.can_go_back(), "opening a mail is a step back");

        store.go_back();
        assert_eq!(store.workspace_view(), WorkspaceView::Mailbox);
        assert_eq!(store.active_tab(), None);
        // The pinned tab itself survives, so the mail is still reachable.
        assert_eq!(store.tabs(), &[id]);
        assert!(store.can_go_forward());

        store.go_forward();
        assert_eq!(store.workspace_view(), WorkspaceView::Email(id));
        assert_eq!(store.active_tab(), Some(id));
        assert!(!store.can_go_forward());
    }

    #[test]
    fn closing_active_tab_selects_right_then_left() {
        let mut store = store();
        let ids: Vec<_> = store
            .visible_emails()
            .into_iter()
            .take(3)
            .map(|email| email.id)
            .collect();
        for id in &ids {
            store.toggle_pin(*id);
            store.open_email(*id);
        }
        store.open_email(ids[0]);
        store.close_tab(ids[0]);
        assert_eq!(store.active_tab(), Some(ids[1]));
        store.open_email(ids[2]);
        store.close_tab(ids[2]);
        assert_eq!(store.active_tab(), Some(ids[1]));
        store.close_tab(ids[1]);
        assert_eq!(store.active_tab(), None);
    }

    #[test]
    fn tab_cycling_wraps_in_both_directions() {
        let mut store = store();
        let ids: Vec<_> = store
            .visible_emails()
            .into_iter()
            .take(2)
            .map(|email| email.id)
            .collect();
        for id in &ids {
            store.toggle_pin(*id);
        }
        store.open_email(ids[0]);
        store.open_email(ids[1]);
        store.cycle_tab(1);
        assert_eq!(store.active_tab(), Some(ids[0]));
        store.cycle_tab(-1);
        assert_eq!(store.active_tab(), Some(ids[1]));
    }

    #[test]
    fn back_from_a_trash_mail_returns_to_the_previous_mailbox() {
        let mut store = store();
        // Start in the inbox and open a mail there.
        let inbox_id = store.visible_emails()[0].id;
        store.open_email(inbox_id);
        // Jump to Trash and open one of its mails.
        store.select_mailbox(Mailbox::Trash);
        let trash_id = store.visible_emails()[0].id;
        store.open_email(trash_id);
        assert_eq!(store.workspace_view(), WorkspaceView::Email(trash_id));
        assert_eq!(store.selected_mailbox(), Mailbox::Trash);

        // Backing out of the Trash mail lands on the Trash list, because that
        // is the view it was opened from.
        store.go_back();
        assert_eq!(store.workspace_view(), WorkspaceView::Mailbox);
        assert_eq!(store.selected_mailbox(), Mailbox::Trash);

        // Backing again crosses to the inbox and the mail opened there, so
        // the arrows retrace mailboxes and not just views.
        store.go_back();
        assert_eq!(store.workspace_view(), WorkspaceView::Email(inbox_id));
        assert_eq!(store.selected_mailbox(), Mailbox::Inbox);

        // And forward walks the same trail back out.
        store.go_forward();
        assert_eq!(store.selected_mailbox(), Mailbox::Trash);
        store.go_forward();
        assert_eq!(store.workspace_view(), WorkspaceView::Email(trash_id));
        assert_eq!(store.selected_mailbox(), Mailbox::Trash);
    }

    #[test]
    fn reselecting_the_same_mailbox_is_not_a_history_step() {
        let mut store = store();
        store.select_mailbox(Mailbox::Trash);
        assert!(store.can_go_back(), "the first switch is a step");
        // Re-selecting the mailbox already shown must not stack a second step.
        store.select_mailbox(Mailbox::Trash);
        store.go_back();
        assert_eq!(store.selected_mailbox(), Mailbox::Inbox);
        assert!(
            !store.can_go_back(),
            "one back should exhaust the history, so the repeat added nothing"
        );
    }

    #[test]
    fn search_matches_sender_subject_and_body() {
        let store = store();
        let matches = |query: &str| {
            store
                .emails()
                .iter()
                .filter(|email| email.matches(query))
                .count()
        };
        assert!(matches("john") >= 1);
        assert!(matches("invoice") >= 1);
        assert!(matches("quarterly planning") >= 1);
        assert_eq!(matches("not present"), 0);
    }
}
