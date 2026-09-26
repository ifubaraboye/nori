use nori_gmail::{RichBlock, text_blocks};
use serde::{Deserialize, Deserializer, Serialize};

/// A mail's stable identity.
///
/// Backed by a string, not a number. A synced mailbox takes its ids from the
/// server, where they are opaque (`18d5f3c2b1a09e4`), and the count is
/// unbounded: a `u16` would cap a real mailbox at 65,535 mails. Sample mail
/// keeps its old number as the string, so ids stay readable in tests and in
/// debug selectors.
///
/// Deliberately not `Copy`: an id is small and passed by value far more often
/// than it is mutated, so a clone at a handful of call sites is cheaper than
/// the silent aliasing `Copy` invites.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct EmailId(pub String);

impl EmailId {
    /// Numbered sample mail. Real mail carries the server's own id instead.
    pub fn sample(number: u16) -> Self {
        Self(number.to_string())
    }
}

impl From<u16> for EmailId {
    fn from(number: u16) -> Self {
        Self::sample(number)
    }
}

impl From<&str> for EmailId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl std::fmt::Display for EmailId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
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

/// Where a mail came from, which decides whether a change leaves the machine.
///
/// Sample mail is deliberately write-back-free: starring it must not fire a
/// request at a server that has never heard of it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// Sample mail, held only in memory for the length of the session.
    #[default]
    Sample,
    /// Mail synced from a remote account, named by its address so more than
    /// one account can be connected later without ambiguity.
    Remote { account: String },
}

/// Bodies cached before rich mail read plain strings; current files
/// read structured blocks. Accept both on the way in so upgrading never
/// orphans a cached mailbox, and always write the new shape.
fn deserialize_body<'de, D>(deserializer: D) -> Result<Vec<RichBlock>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BodyRepr {
        Rich(Vec<RichBlock>),
        Plain(Vec<String>),
    }

    match BodyRepr::deserialize(deserializer)? {
        BodyRepr::Rich(blocks) => Ok(blocks),
        BodyRepr::Plain(paragraphs) => Ok(text_blocks(paragraphs)),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Email {
    pub id: EmailId,
    pub sender: String,
    pub address: String,
    pub recipients: Vec<String>,
    pub subject: String,
    pub preview: String,
    /// Structured blocks since rich bodies landed; plain strings before.
    /// Old index files still carry the string form, so this deserializes
    /// both and writes only the new one. Without the fallback, upgrading
    /// would orphan every cached mailbox into a full re-fetch.
    #[serde(deserialize_with = "deserialize_body")]
    pub body: Vec<RichBlock>,
    pub timestamp: String,
    pub full_date: String,
    pub mailbox: Mailbox,
    pub unread: bool,
    pub starred: bool,
    /// Pinned mails are the only ones that get a tab in the tab strip, and
    /// they survive switching mailboxes. Unpinned mails open as a transient
    /// view with no tab of their own.
    pub pinned: bool,
    /// Whether `body` holds real text yet.
    ///
    /// Synced mail arrives as metadata only — headers, snippet, labels — and
    /// the body is fetched when a mail is opened. An empty `body` cannot
    /// distinguish "not fetched" from "the message genuinely has no body", and
    /// reading the difference wrong means showing an empty reading pane for a
    /// mail that has content. So the state is explicit.
    pub body_loaded: bool,
    pub origin: Origin,
}

impl Default for Email {
    fn default() -> Self {
        Self {
            // Empty on purpose. A real mail always sets `id` in the same
            // literal, and `..Default::default()` keeps that assignment
            // visible at the construction site instead of hiding it as a
            // positional argument in a constructor.
            id: EmailId(String::new()),
            sender: String::new(),
            address: String::new(),
            recipients: Vec::new(),
            subject: String::new(),
            preview: String::new(),
            body: Vec::new(),
            timestamp: String::new(),
            full_date: String::new(),
            mailbox: Mailbox::Inbox,
            unread: false,
            starred: false,
            pinned: false,
            body_loaded: false,
            origin: Origin::Sample,
        }
    }
}

impl Email {
    /// The account a change to this mail should be written back to.
    ///
    /// `None` for sample mail, which has no server behind it. Every write-back
    /// path goes through this, so a sample mailbox stays silent without each
    /// call site remembering to ask where the mail came from — and the check
    /// and the account name cannot drift apart.
    pub fn write_back_account(&self) -> Option<&str> {
        match &self.origin {
            Origin::Sample => None,
            Origin::Remote { account } => Some(account),
        }
    }
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
}

impl Email {
    pub fn summary(&self) -> EmailSummary {
        EmailSummary {
            id: self.id.clone(),
            sender: self.sender.clone(),
            subject: self.subject.clone(),
            preview: self.preview.clone(),
            timestamp: self.timestamp.clone(),
            unread: self.unread,
            starred: self.starred,
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.sender.to_lowercase().contains(&query)
            || self.subject.to_lowercase().contains(&query)
            || self.preview.to_lowercase().contains(&query)
            || nori_gmail::plain_text(&self.body)
                .to_lowercase()
                .contains(&query)
    }
}

#[derive(Clone, Debug, Default)]
pub struct DraftSeed {
    pub to: String,
    pub subject: String,
    pub body: String,
}

impl DraftSeed {
    /// Whether the seed asks for nothing in particular.
    ///
    /// A blank seed is how "just open a compose pane" is spelled, as opposed to
    /// a reply seeding a recipient and subject. The distinction is what lets
    /// a parked draft come back on the next blank request while a real reply
    /// still wins.
    pub fn is_blank(&self) -> bool {
        self.to.trim().is_empty() && self.subject.trim().is_empty() && self.body.trim().is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Gmail's opaque cursor for "what changed since". Lives here rather than
    /// in the sync layer because it describes the store's contents: clearing the
    /// mail has to clear it too, or the next incremental sync would ask about
    /// changes to mail that is gone.
    synced_history_id: Option<String>,
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
            synced_history_id: None,
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
        self.active_tab.clone()
    }

    pub fn workspace_view(&self) -> WorkspaceView {
        self.workspace_view.clone()
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

    /// Mail Nori holds in a folder, read or not.
    pub fn count(&self, mailbox: Mailbox) -> usize {
        self.emails
            .iter()
            .filter(|email| Self::in_folder(email, mailbox))
            .count()
    }

    /// Unread mail Nori actually holds in a folder.
    ///
    /// This is what the sidebar badge counts, deliberately. Gmail will happily
    /// report that an Inbox holds six thousand messages, and a badge saying so
    /// is worse than useless here: Nori has only synced the tail of it, so the
    /// number advertises mail that cannot be opened, scrolled to, or searched.
    /// Counting what is held makes the badge a promise Nori can keep.
    pub fn unread_count(&self, mailbox: Mailbox) -> usize {
        self.emails
            .iter()
            .filter(|email| email.unread && Self::in_folder(email, mailbox))
            .count()
    }

    fn in_folder(email: &Email, mailbox: Mailbox) -> bool {
        match mailbox {
            Mailbox::Inbox => email.mailbox == Mailbox::Inbox,
            Mailbox::Starred => email.starred,
            mailbox => email.mailbox == mailbox,
        }
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

    /// By reference: an `EmailId` is no longer `Copy`, so a by-value id would
    /// force a clone at every lookup.
    pub fn email(&self, id: &EmailId) -> Option<&Email> {
        self.emails.iter().find(|email| email.id == *id)
    }

    fn index_of(&self, id: &EmailId) -> Option<usize> {
        self.emails.iter().position(|email| email.id == *id)
    }

    /// Insert a mail, or update the one already carrying this id.
    ///
    /// Returns whether the mail was new. Synced metadata is merged rather than
    /// blindly overwriting, because a refresh must not silently unpin
    /// anything: `pinned` is Nori's own state with no server equivalent, and
    /// losing it would close every open tab on every sync.
    pub fn upsert(&mut self, email: Email) -> bool {
        match self.index_of(&email.id) {
            Some(index) => {
                let pinned = self.emails[index].pinned;
                self.emails[index] = email;
                self.emails[index].pinned = pinned;
                false
            }
            None => {
                self.emails.push(email);
                true
            }
        }
    }

    /// Replace the whole mailbox with a locally cached index.
    ///
    /// Distinct from a sync: nothing has been asked of the network, and the
    /// file is the authority. Used at launch so the list is on screen before
    /// any request is made.
    pub fn restore(&mut self, emails: Vec<Email>, history_id: Option<String>) {
        self.emails = emails;
        self.synced_history_id = history_id;
        // The tabs come back, rebuilt from what is pinned rather than dropped.
        // A pin is Nori's own state and the only thing that gives a mail a
        // tab, so a launch that cleared them reopened every pinned mail as
        // unpinned: the pin was in the file, and the tab strip came up empty
        // anyway. Nothing is activated — the mailbox is what a launch should
        // show, and auto-opening the last read mail would be a worse surprise
        // than an inactive tab.
        self.tabs = self
            .emails
            .iter()
            .filter(|email| email.pinned)
            .map(|email| email.id.clone())
            .collect();
        self.active_tab = None;
        self.workspace_view = WorkspaceView::Mailbox;
        self.history_back.clear();
        self.history_forward.clear();
        self.clamp_selection();
    }

    /// Everything the local index needs.
    pub fn snapshot(&self) -> Vec<Email> {
        self.emails.clone()
    }

    /// The cursor for the next incremental sync, if one has been recorded.
    pub fn synced_history_id(&self) -> Option<&str> {
        self.synced_history_id.as_deref()
    }

    /// Record where the next incremental sync should start from. `None` forces
    /// a full resync, which is the safe reading of a cursor that cannot be
    /// trusted.
    pub fn set_synced_history_id(&mut self, id: Option<String>) {
        self.synced_history_id = id;
    }

    /// Drop every mail, as a sign-out does.
    pub fn clear(&mut self) {
        self.synced_history_id = None;
        self.emails.clear();
        self.tabs.clear();
        self.active_tab = None;
        self.workspace_view = WorkspaceView::Mailbox;
        self.history_back.clear();
        self.history_forward.clear();
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        self.selected_index = self
            .selected_index
            .min(self.visible_emails().len().saturating_sub(1));
    }

    /// Attach a fetched body, as lazy loading does when a mail is opened.
    ///
    /// Refuses to mark the body loaded if the mail is gone, so a fetch that
    /// lands after the user deleted the mail cannot resurrect it.
    pub fn set_body(&mut self, id: &EmailId, body: Vec<RichBlock>) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.emails[index].body = body;
        self.emails[index].body_loaded = true;
        true
    }

    /// Remove a mail outright, closing its tab if it had one. Sync uses this
    /// when the server reports a mail deleted rather than moved to Trash.
    pub fn remove(&mut self, id: &EmailId) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.emails.remove(index);
        self.close_tab(id.clone());
        self.clamp_selection();
        true
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
            self.tabs.push(id.clone());
        }
        self.active_tab = if pinned { Some(id.clone()) } else { None };
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
                self.tabs.push(id.clone());
            }
            if self.workspace_view == WorkspaceView::Email(id.clone()) {
                self.active_tab = Some(id.clone());
            }
        } else {
            self.tabs.retain(|tab| *tab != id);
            if self.active_tab == Some(id.clone()) {
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
            view: self.workspace_view.clone(),
            mailbox: self.selected_mailbox,
            selected_index: self.selected_index,
        }
    }

    fn restore_entry(&mut self, entry: NavEntry) {
        self.selected_mailbox = entry.mailbox;
        self.selected_index = entry.selected_index;
        self.active_tab = match &entry.view {
            WorkspaceView::Email(id) => self.tabs.contains(id).then(|| id.clone()),
            WorkspaceView::Mailbox => None,
        };
        self.workspace_view = entry.view;
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
        let was_active = self.active_tab.as_ref() == Some(&id);
        self.tabs.remove(index);

        if !was_active {
            return;
        }

        self.active_tab = if self.tabs.is_empty() {
            self.workspace_view = WorkspaceView::Mailbox;
            None
        } else if index < self.tabs.len() {
            Some(self.tabs[index].clone())
        } else {
            let previous = self.tabs[index - 1].clone();
            self.workspace_view = WorkspaceView::Email(previous.clone());
            Some(previous)
        };
    }

    pub fn close_active_tab(&mut self) {
        if let Some(id) = self.active_tab.clone() {
            self.close_tab(id);
        }
    }

    pub fn cycle_tab(&mut self, direction: i32) {
        if self.tabs.is_empty() {
            return;
        }
        let current = self
            .active_tab
            .as_ref()
            .and_then(|active| self.tabs.iter().position(|tab| tab == active))
            .unwrap_or(0) as i32;
        let len = self.tabs.len() as i32;
        let next = (current + direction).rem_euclid(len) as usize;
        let id = self.tabs[next].clone();
        self.active_tab = Some(id.clone());
        self.workspace_view = WorkspaceView::Email(id.clone());
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
        let id = store.visible_emails()[0].id.clone();
        // Only a pinned mail earns a tab, so pin before opening.
        store.toggle_pin(id.clone());
        assert!(store.open_email(id.clone()));
        store.close_tab(id.clone());
        assert!(store.open_email(id.clone()));
        assert!(store.open_email(id.clone()));
        assert_eq!(store.tabs(), std::slice::from_ref(&id));
    }

    #[test]
    fn unpinned_mail_opens_without_a_tab() {
        let mut store = store();
        let id = store.visible_emails()[0].id.clone();
        assert!(store.open_email(id.clone()));
        assert!(store.tabs().is_empty());
        assert_eq!(store.active_tab(), None);
        assert_eq!(store.workspace_view(), WorkspaceView::Email(id.clone()));
    }

    #[test]
    fn pinning_adds_a_tab_and_unpinning_removes_it() {
        let mut store = store();
        let id = store.visible_emails()[0].id.clone();
        assert!(store.toggle_pin(id.clone()));
        assert_eq!(store.tabs(), std::slice::from_ref(&id));
        assert!(store.open_email(id.clone()));
        assert_eq!(store.active_tab(), Some(id.clone()));
        // Unpinning the open mail drops its tab and returns to the mailbox.
        store.toggle_pin(id.clone());
        assert!(store.tabs().is_empty());
        assert_eq!(store.workspace_view(), WorkspaceView::Mailbox);
    }

    #[test]
    fn back_returns_to_the_mailbox_and_forward_reopens_the_mail() {
        let mut store = store();
        let id = store.visible_emails()[0].id.clone();
        store.toggle_pin(id.clone());
        assert!(!store.can_go_back());
        store.open_email(id.clone());
        assert!(store.can_go_back(), "opening a mail is a step back");

        store.go_back();
        assert_eq!(store.workspace_view(), WorkspaceView::Mailbox);
        assert_eq!(store.active_tab(), None);
        // The pinned tab itself survives, so the mail is still reachable.
        assert_eq!(store.tabs(), std::slice::from_ref(&id));
        assert!(store.can_go_forward());

        store.go_forward();
        assert_eq!(store.workspace_view(), WorkspaceView::Email(id.clone()));
        assert_eq!(store.active_tab(), Some(id.clone()));
        assert!(!store.can_go_forward());
    }

    #[test]
    fn closing_active_tab_selects_right_then_left() {
        let mut store = store();
        let ids: Vec<_> = store
            .visible_emails()
            .into_iter()
            .take(3)
            .map(|email| email.id.clone())
            .collect();
        for id in &ids {
            store.toggle_pin(id.clone());
            store.open_email(id.clone());
        }
        store.open_email(ids[0].clone());
        store.close_tab(ids[0].clone());
        assert_eq!(store.active_tab(), Some(ids[1].clone()));
        store.open_email(ids[2].clone());
        store.close_tab(ids[2].clone());
        assert_eq!(store.active_tab(), Some(ids[1].clone()));
        store.close_tab(ids[1].clone());
        assert_eq!(store.active_tab(), None);
    }

    #[test]
    fn tab_cycling_wraps_in_both_directions() {
        let mut store = store();
        let ids: Vec<_> = store
            .visible_emails()
            .into_iter()
            .take(2)
            .map(|email| email.id.clone())
            .collect();
        for id in &ids {
            store.toggle_pin(id.clone());
        }
        store.open_email(ids[0].clone());
        store.open_email(ids[1].clone());
        store.cycle_tab(1);
        assert_eq!(store.active_tab(), Some(ids[0].clone()));
        store.cycle_tab(-1);
        assert_eq!(store.active_tab(), Some(ids[1].clone()));
    }

    #[test]
    fn back_from_a_trash_mail_returns_to_the_previous_mailbox() {
        let mut store = store();
        // Start in the inbox and open a mail there.
        let inbox_id = store.visible_emails()[0].id.clone();
        store.open_email(inbox_id.clone());
        // Jump to Trash and open one of its mails.
        store.select_mailbox(Mailbox::Trash);
        let trash_id = store.visible_emails()[0].id.clone();
        store.open_email(trash_id.clone());
        assert_eq!(
            store.workspace_view(),
            WorkspaceView::Email(trash_id.clone())
        );
        assert_eq!(store.selected_mailbox(), Mailbox::Trash);

        // Backing out of the Trash mail lands on the Trash list, because that
        // is the view it was opened from.
        store.go_back();
        assert_eq!(store.workspace_view(), WorkspaceView::Mailbox);
        assert_eq!(store.selected_mailbox(), Mailbox::Trash);

        // Backing again crosses to the inbox and the mail opened there, so
        // the arrows retrace mailboxes and not just views.
        store.go_back();
        assert_eq!(
            store.workspace_view(),
            WorkspaceView::Email(inbox_id.clone())
        );
        assert_eq!(store.selected_mailbox(), Mailbox::Inbox);

        // And forward walks the same trail back out.
        store.go_forward();
        assert_eq!(store.selected_mailbox(), Mailbox::Trash);
        store.go_forward();
        assert_eq!(
            store.workspace_view(),
            WorkspaceView::Email(trash_id.clone())
        );
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

    /// A synced mail stub: what the list can draw before any body is fetched.
    fn synced(id: &str) -> Email {
        Email {
            id: EmailId::from(id),
            sender: "remote@example.com".to_string(),
            subject: "From the server".to_string(),
            origin: Origin::Remote {
                account: "me@example.com".to_string(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn upsert_adds_then_updates_in_place() {
        let mut store = MailStore::new(Vec::new());
        assert!(store.upsert(synced("a")), "a new id reports as inserted");
        assert!(!store.upsert(synced("a")), "the same id reports as updated");
        assert_eq!(store.emails().len(), 1, "an update must not append");

        let mut changed = synced("a");
        changed.subject = "Edited".to_string();
        store.upsert(changed);
        assert_eq!(store.email(&EmailId::from("a")).unwrap().subject, "Edited");
    }

    #[test]
    fn a_fetched_body_is_marked_loaded() {
        let mut store = MailStore::new(Vec::new());
        let mail = synced("a");
        assert!(
            !mail.body_loaded,
            "synced mail starts as metadata only, with no body to show"
        );
        store.upsert(mail);

        let id = EmailId::from("a");
        assert!(store.set_body(&id, nori_gmail::text_blocks(vec!["Hello".to_string()])));
        let mail = store.email(&id).unwrap();
        assert!(mail.body_loaded);
        assert_eq!(
            mail.body,
            nori_gmail::text_blocks(vec!["Hello".to_string()])
        );

        assert!(
            !store.set_body(&EmailId::from("gone"), vec![]),
            "a body landing after the mail was deleted must not revive it"
        );
    }

    #[test]
    fn an_index_written_before_rich_bodies_still_loads() {
        // Bodies cached before rich mail read plain strings. The shape
        // changed under them; the old files must still parse, or upgrading
        // orphans every cached mailbox into a full re-fetch and an empty
        // list meanwhile.
        let old = serde_json::json!({
            "id": "abc",
            "sender": "S",
            "address": "s@x.io",
            "recipients": [],
            "subject": "Hi",
            "preview": "Hi",
            "body": ["Hello", "World"],
            "timestamp": "t",
            "fullDate": "f",
            "mailbox": "inbox",
            "unread": false,
            "starred": false,
            "pinned": false,
            "bodyLoaded": true,
            "origin": "sample",
        });
        let mail: Email = serde_json::from_value(old).expect("old bodies must still parse");
        assert_eq!(
            mail.body,
            nori_gmail::text_blocks(vec!["Hello".to_string(), "World".to_string()]),
            "legacy string bodies become plain blocks"
        );

        // And the new shape round-trips unchanged.
        let json = serde_json::to_value(&mail).expect("new bodies must serialize");
        let again: Email = serde_json::from_value(json).expect("new bodies must deserialize");
        assert_eq!(again.body, mail.body);
    }

    #[test]
    fn removing_a_mail_closes_its_tab() {
        let mut store = MailStore::new(mock_emails());
        let id = store.visible_emails()[0].id.clone();
        store.toggle_pin(id.clone());
        assert!(store.tabs().contains(&id));

        assert!(store.remove(&id));
        assert!(store.email(&id).is_none());
        assert!(
            !store.tabs().contains(&id),
            "a tab for a mail that no longer exists would open onto nothing"
        );
        assert!(!store.remove(&id), "removing twice is not a change");
    }

    #[test]
    fn signing_out_empties_the_store() {
        let mut store = store();
        store.toggle_pin(store.visible_emails()[0].id.clone());
        store.clear();
        assert!(store.emails().is_empty());
        assert!(store.tabs().is_empty());
        assert_eq!(store.workspace_view(), WorkspaceView::Mailbox);
        assert!(!store.can_go_back(), "history must not outlive the mail");
    }

    /// A pin has to outlive a restart, or pinning is a gesture that does
    /// nothing.
    ///
    /// The full round trip: pin, snapshot to the index, read that index back
    /// the way a launch does, and the tab is there. The middle step is the one
    /// that matters — `pinned` was already in the file, so a test that only
    /// checked the store would have passed while the app dropped the tabs on
    /// the way back in.
    #[test]
    fn a_pin_survives_the_index_round_trip() {
        let mut store = store();
        let pinned = store.visible_emails()[0].id.clone();
        let left_alone = store.visible_emails()[1].id.clone();
        store.toggle_pin(pinned.clone());

        // What goes to disk.
        let written = serde_json::to_string(&store.snapshot()).expect("the index serialises");

        // What a launch reads.
        let read_back: Vec<Email> = serde_json::from_str(&written).expect("the index loads");

        let mut reopened = MailStore::new(Vec::new());
        reopened.restore(read_back, Some("cursor".to_string()));

        assert_eq!(
            reopened.tabs(),
            std::slice::from_ref(&pinned),
            "the pinned mail keeps its tab across a restart"
        );
        assert!(
            reopened.email(&pinned).is_some_and(|email| email.pinned),
            "and is still pinned"
        );
        assert!(
            !reopened.tabs().contains(&left_alone),
            "an unpinned mail earns no tab, however many times it is restored"
        );
    }

    /// Restoring must not throw the user into a mail.
    ///
    /// Tabs coming back is the fix; auto-opening one is not. A launch that
    /// dropped you into the last thing you were reading is a worse surprise
    /// than an inactive tab, so the mailbox is what a launch shows.
    #[test]
    fn a_restored_pin_is_not_activated() {
        let mut store = store();
        let pinned = store.visible_emails()[0].id.clone();
        store.toggle_pin(pinned.clone());

        let restored = store.snapshot();
        let mut reopened = MailStore::new(Vec::new());
        reopened.restore(restored, None);

        assert_eq!(reopened.tabs().len(), 1, "the tab is there");
        assert_eq!(reopened.active_tab(), None, "but nothing is open");
        assert_eq!(
            reopened.workspace_view(),
            WorkspaceView::Mailbox,
            "a launch shows the mailbox"
        );
    }

    /// Unpinning before a restart must not come back as pinned.
    ///
    /// The other direction of the same round trip: a restore that rebuilt tabs
    /// from a stale pin would resurrect a tab the user had just closed.
    #[test]
    fn an_unpinned_mail_does_not_return_with_a_tab() {
        let mut store = store();
        let id = store.visible_emails()[0].id.clone();
        store.toggle_pin(id.clone());
        store.toggle_pin(id.clone());
        assert!(store.tabs().is_empty(), "unpinning closed the tab");

        let restored = store.snapshot();
        let mut reopened = MailStore::new(Vec::new());
        reopened.restore(restored, None);

        assert!(
            reopened.tabs().is_empty(),
            "an unpin has to be as durable as a pin, or closing a tab is undone \
             by quitting"
        );
    }
}
