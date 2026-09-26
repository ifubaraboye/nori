//! Bridging Gmail into Nori's own model.
//!
//! Everything here is pure: it turns a `nori_gmail` snapshot into the
//! `Email` and `Label` values the rest of the app already understands, and
//! decides what the account pane should say. No I/O, no gpui context, so the
//! awkward decisions — which Gmail label wins when a mail has several, what a
//! missing body means — are testable on their own.

use nori_gmail::{RemoteLabel, RemoteMail};

use super::{Email, EmailId, Mailbox, Origin};

/// How the account pane should read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AccountState {
    /// Nothing connected. The prototype's sample mail is still in the list.
    #[default]
    Disconnected,
    /// A browser is open and Nori is waiting for the redirect.
    Connecting,
    Connected {
        address: String,
        /// Mails in the local index, for the pane's summary.
        mail: usize,
        labels: usize,
        /// What each folder holds on the server. The sidebar's badge for every
        /// folder but the Inbox, which counts what Nori holds instead — see
        /// [`AccountState::count_of`].
        counts: Option<nori_gmail::FolderCounts>,
    },
    /// The grant is no longer valid. Expected in a Testing-mode app every
    /// seven days, so it is a state and not an error.
    NeedsReauth {
        address: String,
    },
    Failed {
        reason: String,
    },
}

impl AccountState {
    pub fn address(&self) -> Option<&str> {
        match self {
            Self::Connected { address, .. } | Self::NeedsReauth { address } => Some(address),
            _ => None,
        }
    }

    /// Whether the account can sync as it stands. A stale grant is connected
    /// but not usable, and treating it as usable would run a sync that is
    /// guaranteed to fail.
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    /// The badge beside a folder in the sidebar.
    ///
    /// The Inbox is the exception, and it is the exception because of what its
    /// number is for. Six thousand unread is not a figure anyone acts on, and
    /// Nori cannot stand behind it either: only the tail of the Inbox is held,
    /// so the badge would be advertising mail that cannot be opened, scrolled to
    /// or searched. Unread-and-held is a number the list can keep.
    ///
    /// Every other folder is the opposite case, which is why it is the opposite
    /// case. They are small, they are fetched whole when opened, and their count
    /// is an honest measure of how much is left below the fold — so they say
    /// it. Before the folders were fetched this way they had nothing to show at
    /// all, and a blank sidebar next to a full mailbox read as lost mail.
    pub fn count_of(&self, mailbox: Mailbox, unread_held: usize) -> usize {
        if mailbox == Mailbox::Inbox {
            return unread_held;
        }
        let Some(counts) = self.folder_counts() else {
            return unread_held;
        };
        let count = match mailbox {
            Mailbox::Inbox => unreachable!("handled above"),
            Mailbox::Starred => counts.starred,
            Mailbox::Sent => counts.sent,
            Mailbox::Drafts => counts.drafts,
            Mailbox::Archive => counts.archive,
            Mailbox::Trash => counts.trash,
        };
        usize::try_from(count).unwrap_or(usize::MAX)
    }

    fn folder_counts(&self) -> Option<&nori_gmail::FolderCounts> {
        match self {
            Self::Connected { counts, .. } => counts.as_ref(),
            _ => None,
        }
    }
}

/// The Gmail search that returns one mailbox's mail.
///
/// Gmail's mailboxes are not folders you can name. `in:inbox` works, but
/// Archive is only the *absence* of everything else, so it has to be spelled
/// as a set of exclusions. Braces mean "or" in Gmail's search syntax, which is
/// how Trash gets Spam as well: Nori has one Trash, and a message Gmail filed
/// under Spam is not something the user can find anywhere else.
pub fn gmail_query(mailbox: Mailbox) -> &'static str {
    match mailbox {
        Mailbox::Inbox => "in:inbox",
        Mailbox::Starred => "is:starred",
        Mailbox::Sent => "in:sent",
        Mailbox::Drafts => "in:drafts",
        Mailbox::Archive => {
            "in:anywhere -in:inbox -in:sent -in:drafts -in:trash -in:spam -is:starred"
        }
        Mailbox::Trash => "{in:trash in:spam}",
    }
}

/// Gmail's system labels, in the order that decides a mail's mailbox.
///
/// Order matters because they are not mutually exclusive: a mail you sent to
/// yourself can be `SENT` *and* `INBOX`, and a starred inbox mail is
/// `INBOX` + `STARRED`. Trash has to win, or a deleted mail keeps showing up
/// in the inbox it was never removed from.
const MAILBOX_PRIORITY: [(&str, Mailbox); 7] = [
    ("TRASH", Mailbox::Trash),
    ("SPAM", Mailbox::Trash),
    ("DRAFT", Mailbox::Drafts),
    ("SENT", Mailbox::Sent),
    ("SENT_MAIL", Mailbox::Sent),
    ("INBOX", Mailbox::Inbox),
    ("ARCHIVE", Mailbox::Archive),
];

/// Which mailbox a set of Gmail labels puts a mail in.
pub fn mailbox_of(label_ids: &[String]) -> Mailbox {
    MAILBOX_PRIORITY
        .iter()
        .find(|(gmail, _)| label_ids.iter().any(|id| id == gmail))
        .map(|(_, mailbox)| *mailbox)
        .unwrap_or(Mailbox::Archive)
}

/// Turn synced metadata into a row the list can draw.
///
/// The body is left unset and `body_loaded` false: sync fetches headers and a
/// snippet, and the body arrives when the mail is opened. Claiming otherwise
/// would show an empty reading pane for a mail that has text.
pub fn to_email(remote: &RemoteMail, account: &str) -> Email {
    Email {
        id: EmailId::from(remote.id.as_str()),
        sender: remote.sender.clone(),
        address: remote.address.clone(),
        recipients: remote.recipients.clone(),
        subject: remote.subject.clone(),
        preview: remote.preview.clone(),
        body: Vec::new(),
        timestamp: remote.timestamp.clone(),
        full_date: remote.full_date.clone(),
        mailbox: mailbox_of(&remote.label_ids),
        unread: remote.has_label("UNREAD"),
        starred: remote.has_label("STARRED"),
        // Pinned is deliberately not set: it is Nori's own state, and a sync
        // must never decide it.
        pinned: false,
        body_loaded: false,
        origin: Origin::Remote {
            account: account.to_string(),
        },
    }
}

/// What a synced label needs in order to be created locally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelSeed {
    pub name: String,
    pub colour: u32,
    /// Gmail's own id, so an assignment can be written back. Kept out of
    /// `Label` itself, which has no room for it and is not this crate's to
    /// change.
    pub remote_id: String,
}

pub fn to_label_seed(remote: &RemoteLabel) -> LabelSeed {
    LabelSeed {
        name: remote.name.clone(),
        colour: remote.colour,
        remote_id: remote.id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_gmail::RemoteLabel;

    fn remote(labels: &[&str]) -> RemoteMail {
        RemoteMail {
            id: "18d5f3c2".into(),
            sender: "Ada".into(),
            address: "ada@example.com".into(),
            recipients: vec!["me@example.com".into()],
            subject: "Analytical".into(),
            preview: "The snippet".into(),
            body: None,
            timestamp: "4 Mar".into(),
            full_date: "4 Mar 2026 09:12".into(),
            label_ids: labels.iter().map(|l| l.to_string()).collect(),
        }
    }

    #[test]
    fn a_trashed_mail_does_not_stay_in_the_inbox() {
        // Gmail keeps the INBOX label on a mail it trashed, so priority is the
        // only thing standing between a deleted mail and the inbox list.
        assert_eq!(
            mailbox_of(&["INBOX".into(), "TRASH".into()]),
            Mailbox::Trash
        );
        assert_eq!(mailbox_of(&["INBOX".into(), "SPAM".into()]), Mailbox::Trash);
    }

    #[test]
    fn drafts_and_sent_win_over_the_inbox() {
        assert_eq!(
            mailbox_of(&["INBOX".into(), "DRAFT".into()]),
            Mailbox::Drafts
        );
        assert_eq!(mailbox_of(&["INBOX".into(), "SENT".into()]), Mailbox::Sent);
        assert_eq!(mailbox_of(&["SENT_MAIL".into()]), Mailbox::Sent);
    }

    #[test]
    fn an_ordinary_inbox_mail_lands_in_the_inbox() {
        assert_eq!(
            mailbox_of(&["INBOX".into(), "UNREAD".into(), "CATEGORY_PERSONAL".into()]),
            Mailbox::Inbox
        );
    }

    #[test]
    fn a_starred_inbox_mail_is_still_an_inbox_mail() {
        let email = to_email(&remote(&["INBOX", "STARRED", "UNREAD"]), "me@example.com");
        assert_eq!(email.mailbox, Mailbox::Inbox);
        assert!(
            email.starred,
            "the star is its own flag; the Starred *view* filters on it"
        );
    }

    #[test]
    fn a_mail_with_no_known_label_is_archived_rather_than_lost() {
        assert_eq!(mailbox_of(&[]), Mailbox::Archive);
        assert_eq!(mailbox_of(&["IMPORTANT".into()]), Mailbox::Archive);
    }

    #[test]
    fn synced_mail_arrives_unread_unchecked_and_unloaded() {
        let email = to_email(&remote(&["INBOX", "UNREAD"]), "me@example.com");
        assert!(email.unread);
        assert!(
            !email.body_loaded,
            "a body that was never fetched is not empty"
        );
        assert!(email.body.is_empty());
    }

    #[test]
    fn synced_mail_carries_its_server_id_and_its_account() {
        let email = to_email(&remote(&["INBOX"]), "me@example.com");
        assert_eq!(email.id, EmailId::from("18d5f3c2"));
        assert_eq!(email.write_back_account(), Some("me@example.com"));
        assert_eq!(
            email.origin,
            Origin::Remote {
                account: "me@example.com".into()
            }
        );
    }

    #[test]
    fn sample_mail_is_never_written_back() {
        // A sample mailbox has no server behind it, so a star change must not
        // turn into a request. `is_remote` is the single check every
        // write-back path makes.
        let sample = Email {
            id: EmailId::sample(1),
            subject: "Sample".into(),
            ..Default::default()
        };
        assert_eq!(sample.write_back_account(), None);

        let remote = to_email(&remote(&["INBOX"]), "me@example.com");
        assert_eq!(remote.write_back_account(), Some("me@example.com"));
    }

    #[test]
    fn a_sync_must_not_decide_what_is_pinned() {
        let email = to_email(&remote(&["INBOX"]), "me@example.com");
        assert!(
            !email.pinned,
            "pinning is Nori's own state; a sync that set it would pin whatever \
             the server happened to list"
        );
    }

    #[test]
    fn the_account_pane_reads_the_state_it_is_given() {
        assert_eq!(AccountState::default(), AccountState::Disconnected);
        assert!(AccountState::Disconnected.address().is_none());
        assert!(!AccountState::Disconnected.is_usable());

        let connected = AccountState::Connected {
            address: "me@example.com".into(),
            mail: 12,
            labels: 3,
            counts: None,
        };
        assert_eq!(connected.address(), Some("me@example.com"));
        assert!(connected.is_usable());

        let stale = AccountState::NeedsReauth {
            address: "me@example.com".into(),
        };
        assert!(
            !stale.is_usable(),
            "a dead grant must not read as connected, or sync runs against it"
        );
        assert_eq!(
            stale.address(),
            Some("me@example.com"),
            "still names the account"
        );
    }

    #[test]
    fn a_label_keeps_the_colour_it_has_in_gmail() {
        let seed = to_label_seed(&RemoteLabel {
            id: "Label_7".into(),
            name: "Work".into(),
            colour: 0x5b_8a_9a,
        });
        assert_eq!(seed.name, "Work");
        assert_eq!(seed.colour, 0x5b_8a_9a);
        assert_eq!(seed.remote_id, "Label_7");
    }
}
