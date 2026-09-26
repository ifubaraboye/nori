//! How many messages sit in each folder, without downloading them.
//!
//! The sidebar used to count the mails Nori happened to have in its index,
//! which meant every folder the user had not opened read as zero. That is worse
//! than a slow number: it looks like the mail is gone.
//!
//! Getting the real figures took some finding, and two plausible routes are
//! dead ends worth writing down:
//!
//! - `labels.list` does **not** carry `messagesTotal`. It returns id, name,
//!   type and the visibility flags, and nothing else. Counting from it yields
//!   zero for every folder, which is the bug this replaced.
//! - `messages.list` returns a `resultSizeEstimate` that answers 201 for every
//!   query, capped and unreliable. Useless as a count.
//!
//! What works is `labels.get` on each system label, which does return
//! `messagesTotal` and `messagesUnread`, at one unit apiece. Six calls, six
//! units, and every folder is right without downloading any of its mail.
//!
//! Bodies are unaffected: still fetched one mail at a time when a mail is
//! opened. A folder count is free and a body is 20 units, so pre-fetching a
//! folder's bodies to save a round trip on open would spend thousands of units
//! on something that already costs one click.

/// One count per folder, in the order Nori's sidebar lists them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FolderCounts {
    pub inbox: u64,
    pub starred: u64,
    pub sent: u64,
    pub drafts: u64,
    pub archive: u64,
    pub trash: u64,
    /// Unread in the Inbox, which is the only folder whose badge Nori shows.
    pub inbox_unread: u64,
}

impl FolderCounts {
    /// Build the counts from a per-label lookup.
    ///
    /// `lookup` is given a Gmail label id and answers its `messagesTotal` and
    /// `messagesUnread`. It is a closure rather than a slice because the counts
    /// have to be fetched label by label — see the module note.
    ///
    /// `mailbox_total` is `users.profile`'s `messagesTotal`: everything in the
    /// account, Trash and Spam included.
    pub fn from_lookup<F>(lookup: F, mailbox_total: u64) -> Self
    where
        F: Fn(&str) -> Option<(u64, u64)>,
    {
        let total = |id: &str| lookup(id).map(|(total, _)| total).unwrap_or(0);

        let inbox = total("INBOX");
        let inbox_unread = lookup("INBOX").map(|(_, unread)| unread).unwrap_or(0);
        let sent = total("SENT");
        let drafts = total("DRAFT");
        // Nori has one Trash and Gmail has two places a message can be binned,
        // so both count towards it.
        let trash = total("TRASH") + total("SPAM");

        Self {
            inbox,
            inbox_unread,
            starred: total("STARRED"),
            sent,
            drafts,
            // There is no `ARCHIVE` label: Gmail models Archive as whatever is
            // left once the named folders are removed, so it is derived. Each
            // subtraction saturates because the account total and the folder
            // totals are reported independently and can disagree by a message;
            // a negative count in a sidebar is far worse than one slightly off.
            archive: mailbox_total
                .saturating_sub(inbox)
                .saturating_sub(sent)
                .saturating_sub(drafts)
                .saturating_sub(trash),
            trash,
        }
    }
}
