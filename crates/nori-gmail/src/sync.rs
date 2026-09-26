//! Turning Gmail into mail Nori can show.
//!
//! Policy lives here rather than in the view layer: what to fetch, in what
//! order, how much of it, and how to read Gmail's more awkward corners. What
//! comes out is plain data with no gpui and no `nori-ui` types, so the whole
//! of this is testable without a window and the UI only has to copy fields.
//!
//! Gmail's raw label ids are passed through untouched. Deciding that
//! `label_ids` means "Inbox, unread" is the view layer's job, because
//! `Mailbox` is defined there and this crate must not grow a dependency on it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use anyhow::{Context, Result};
use base64::Engine as _;

use crate::counts::FolderCounts;
use crate::gmail::{self, GMAIL_BASE};
use crate::http;
use crate::oauth::{self, Credentials};
use crate::rich::RichBlock;
use crate::stream::MailStream;
use crate::token::{Token, TokenStore};

/// How many mails a first sync indexes.
///
/// Sized against [`QUOTA_UNITS_PER_MINUTE`], not chosen for taste. Gmail charges
/// 20 units per `messages.get` and allows 6,000 units per minute *per user*, so
/// the hard ceiling is 300 mails a minute however the requests are arranged.
///
/// The first sync is also Inbox-only — see [`INBOX`] — so this is the top of the
/// folder the user is looking at, not a sample of everything. What is left in
/// the budget goes to the folders they actually open, at
/// [`MAILBOX_FETCH_BUDGET`] each, and to keeping the index current.
///
/// The consequence is worth stating plainly: indexing a mailbox of several
/// thousand mails in one go is **not possible**, not slow — at 240 a minute it
/// is a twenty-minute job that would be throttled long before it finished.
/// Gmail's own web client does not do it either.
pub const METADATA_BUDGET: usize = 80;

/// How many `messages.get` requests may be in flight at once.
const FETCH_CONCURRENCY: usize = 4;

/// Gmail's quota, in units per minute, **per user per project**.
///
/// This is the limit that actually binds, and it is far tighter than the
/// 1,200,000/min per-project figure: 6,000 units works out at 300
/// `messages.get` calls a minute, so indexing a mailbox of several thousand
/// mails is a multi-minute job by arithmetic, not by accident. Google cut this
/// limit on 2026-05-01 for new projects.
const QUOTA_UNITS_PER_MINUTE: u64 = 6_000;

/// Nori must never budget for more than Google allows. If Google lowers the
/// limit, this is where it should fail.
const _: () = assert!(SELF_LIMIT_UNITS_PER_MINUTE <= QUOTA_UNITS_PER_MINUTE);
/// The one that actually matters: a first sync and a folder fetch landing in
/// the same minute must still fit inside the self-limit, or opening a folder
/// during a sync earns a 403. This is the check that caught the original
/// budget being larger than a single allowance.
const _: () = assert!(
    (METADATA_BUDGET + MAILBOX_FETCH_BUDGET) as u64 * COST_MESSAGE_GET
        <= SELF_LIMIT_UNITS_PER_MINUTE
);

/// What each call costs, from Gmail's published quota table.
const COST_MESSAGE_GET: u64 = 20;
const COST_MESSAGE_LIST: u64 = 5;

/// The ceiling on request rate, in units per minute, that Nori allows itself.
///
/// Four fifths of [`QUOTA_UNITS_PER_MINUTE`], leaving a fifth of headroom: a
/// sync also spends units on `messages.list`, the label read and the odd
/// write-back, and anything else using the same account should not be pushed
/// over by Nori. Spending the whole allowance would be faster by about twenty
/// seconds and would risk a 403 on every run, which is a far worse trade.
const SELF_LIMIT_UNITS_PER_MINUTE: u64 = QUOTA_UNITS_PER_MINUTE * 4 / 5;

/// The most pages of ids walked in one fetch, as a backstop against a server
/// that hands back a fresh `nextPageToken` forever. A page holds
/// [`gmail::PAGE_SIZE`] ids, so this is far more than any budget here needs.
const MAX_PAGES: usize = 10;

/// Take a lock, recovering from poisoning rather than propagating it.
///
/// A poisoned mutex means a worker panicked part-way through a fetch. Its slot
/// is left `Pending`, which reads as "this mail is missing" — and abandoning
/// the entire sync over one worker panic would be worse than the mail it
/// loses, so the guard is taken anyway.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Paces requests so a sync cannot outrun the account's quota.
///
/// The workers share one of these, so the limit is on the sync as a whole
/// rather than per thread — a per-thread limit multiplied by the thread count
/// is exactly how a parallel fetch turns into a 403.
struct RateLimiter {
    /// The wait between one unit of quota being spent.
    spacing: std::time::Duration,
    next: Mutex<Option<std::time::Instant>>,
}

impl RateLimiter {
    fn new(units_per_minute: u64) -> Self {
        Self {
            spacing: std::time::Duration::from_secs_f64(60.0 / units_per_minute.max(1) as f64),
            next: Mutex::new(None),
        }
    }

    /// Block until `units` of quota may be spent.
    ///
    /// A ticket queue: take the next slot, push the queue on, then sleep until
    /// that specific moment. The tempting version — re-check and re-reserve on
    /// every wake-up — livelocks, because each thread that finds the slot taken
    /// pushes the deadline further out, so the queue advances faster than time
    /// passes and no thread is ever granted one.
    fn acquire(&self, units: u64) {
        let grant = {
            let mut next = lock(&self.next);
            let now = std::time::Instant::now();
            let grant = next.unwrap_or(now).max(now);
            *next = Some(grant + self.spacing * units as u32);
            grant
        };
        let now = std::time::Instant::now();
        if grant > now {
            std::thread::sleep(grant - now);
        }
    }
}

/// /// One message's slot while the workers race to fill it.
enum Slot {
    Pending,
    Fetched(gmail::MessageMetadata),
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteLabel {
    /// Gmail's id, kept verbatim so assignments can be written back.
    pub id: String,
    pub name: String,
    /// Nori's packed colour, from a stable hash of the name.
    pub colour: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteMail {
    pub id: String,
    /// The display name, falling back to the address when there is none.
    pub sender: String,
    pub address: String,
    pub recipients: Vec<String>,
    pub subject: String,
    /// Gmail's snippet: enough for a list row without fetching the body.
    pub preview: String,
    /// `None` until fetched. Distinguishing "not fetched" from "empty" matters,
    /// and collapsing them shows a blank reading pane for a mail that has text.
    pub body: Option<Vec<String>>,
    /// Short date for the list, e.g. "14:32" or "3 Mar".
    pub timestamp: String,
    /// Full date for the reading pane.
    pub full_date: String,
    /// Raw Gmail label ids. The caller decides what they mean.
    pub label_ids: Vec<String>,
}

impl RemoteMail {
    pub fn has_label(&self, label: &str) -> bool {
        self.label_ids.iter().any(|id| id == label)
    }
}

/// What a sync produced.
///
/// Two shapes because applying them is genuinely different: a full result
/// replaces the mailbox, a delta merges into it. Collapsing them would mean
/// either re-downloading everything on every pass or dropping changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncOutcome {
    Full(Snapshot),
    Changes(Incremental),
}

impl From<Snapshot> for SyncOutcome {
    fn from(snapshot: Snapshot) -> Self {
        Self::Full(snapshot)
    }
}

impl From<Incremental> for SyncOutcome {
    fn from(delta: Incremental) -> Self {
        Self::Changes(delta)
    }
}

/// A delta with its changed mail already fetched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Incremental {
    pub changed: Vec<RemoteMail>,
    pub deleted: Vec<String>,
    pub history_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub account: String,
    pub labels: Vec<RemoteLabel>,
    pub mail: Vec<RemoteMail>,
    /// Opaque cursor for the next incremental sync. `None` before the first
    /// successful sync, which is the signal to do a full one.
    pub history_id: Option<String>,
}

/// What changed since the last sync.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Delta {
    /// Fetch metadata for these: new mail, or mail whose labels moved.
    pub changed: Vec<String>,
    /// Gone from the mailbox entirely, not merely trashed.
    pub deleted: Vec<String>,
    pub history_id: Option<String>,
}

impl Delta {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.deleted.is_empty()
    }
}

pub struct Sync<'a> {
    agent: ureq::Agent,
    credentials: &'a Credentials,
    store: &'a dyn TokenStore,
    token: Token,
}

impl<'a> Sync<'a> {
    pub fn new(
        agent: ureq::Agent,
        credentials: &'a Credentials,
        store: &'a dyn TokenStore,
    ) -> Result<Self> {
        let token = store
            .load()?
            .context("no saved token; the account is not signed in")?;
        Ok(Self {
            agent,
            credentials,
            store,
            token,
        })
    }

    pub fn token(&self) -> &Token {
        &self.token
    }

    /// Renew the access token if it is close to lapsing.
    ///
    /// Centralised so that no request can be sent with a token that dies
    /// mid-flight, which is otherwise a 401 in the middle of a page of results
    /// and a sync that half-applies.
    pub fn authorize(&mut self) -> Result<()> {
        if self.token.is_fresh() {
            return Ok(());
        }
        let refresh_token = self.token.refresh_token.as_deref().context(
            "the access token has expired and there is no refresh token to renew it with",
        )?;
        let renewed = oauth::refresh(&self.agent, self.credentials, refresh_token)
            .context("renewing the access token")?;
        self.store.save(&renewed)?;
        self.token = renewed;
        Ok(())
    }

    /// Fetch the account, its labels, and as much recent mail as the budget
    /// allows. This is the first sync and the fallback after `historyId` ages
    /// out.
    pub fn full(&mut self) -> Result<Snapshot> {
        self.full_with(None)
    }

    /// As [`Self::full`], reporting progress as the mail arrives.
    pub fn full_with(&mut self, stream: Option<&MailStream>) -> Result<Snapshot> {
        self.authorize()?;
        let profile = gmail::profile(&self.agent, &self.token)?;
        let labels = self.labels()?;
        let mail = self.fetch_with(INBOX, METADATA_BUDGET, stream)?;

        Ok(Snapshot {
            account: profile.email,
            labels,
            mail,
            history_id: profile.history_id,
        })
    }

    /// Index the newest `budget` mails matching a Gmail search.
    ///
    /// The quota is per minute per user, so a full mailbox cannot be indexed in
    /// one pass — see [`METADATA_BUDGET`]. Fetching per mailbox instead is what
    /// makes that workable: the mail a person is actually looking at is fetched
    /// when they open the folder, and a folder they never open costs nothing.
    /// Newest first, because a truncated list is only useful if the top of it
    /// is the mail that just arrived.
    pub fn fetch(&mut self, query: &str, budget: usize) -> Result<Vec<RemoteMail>> {
        self.fetch_with(query, budget, None)
    }

    /// As [`Self::fetch`], handing each message over as it arrives.
    ///
    /// `stream` is optional so the command line tools and the incremental path
    /// — which fetch a handful of mails and finish before anyone could read a
    /// progress line — do not have to make one.
    pub fn fetch_with(
        &mut self,
        query: &str,
        budget: usize,
        stream: Option<&MailStream>,
    ) -> Result<Vec<RemoteMail>> {
        self.authorize()?;
        let mut ids = Vec::new();
        let mut page_token: Option<String> = None;
        let limiter = RateLimiter::new(SELF_LIMIT_UNITS_PER_MINUTE);
        for _ in 0..MAX_PAGES {
            if ids.len() >= budget {
                break;
            }
            limiter.acquire(COST_MESSAGE_LIST);
            let page =
                gmail::list_messages(&self.agent, &self.token, query, page_token.as_deref())?;
            ids.extend(page.messages.into_iter().map(|message| message.id));
            match page.next_page {
                Some(next) => page_token = Some(next),
                None => break,
            }
        }
        ids.truncate(budget);
        if let Some(stream) = stream {
            stream.set_total(ids.len());
        }

        let mail: Vec<RemoteMail> = self
            .metadata_for(&ids, stream)?
            .into_iter()
            .flatten()
            .filter_map(|metadata| to_mail(&metadata))
            .collect();
        if let Some(stream) = stream {
            stream.finish();
        }
        Ok(mail)
    }

    /// Fold a delta into fetched mail, ready to apply.
    ///
    /// Doing the fetching here rather than in the view layer keeps the two
    /// quota-relevant decisions — which ids are worth a `messages.get`, and
    /// what to do about a 404 — in one place with the numbers next to them.
    pub fn incremental(&mut self, since: &str) -> Result<Option<Incremental>> {
        let Some(delta) = self.delta(since)? else {
            return Ok(None);
        };
        let metadata = self.metadata_for(&delta.changed, None)?;
        let changed = metadata
            .into_iter()
            .flatten()
            .filter_map(|metadata| to_mail(&metadata))
            .collect();
        Ok(Some(Incremental {
            changed,
            deleted: delta.deleted,
            history_id: delta.history_id,
        }))
    }

    /// Ask what changed since the last sync.
    ///
    /// `Ok(None)` means the stored id has aged out and a full sync is needed.
    /// Gmail drops history after about a week, so this is a normal outcome
    /// rather than a failure, and treating it as one would leave the app
    /// permanently stuck on a stale mailbox.
    pub fn delta(&mut self, since: &str) -> Result<Option<Delta>> {
        self.authorize()?;
        let history = match gmail::history(&self.agent, &self.token, since) {
            Ok(history) => history,
            // A 404 on the history endpoint is the documented signal that the
            // id is too old, not that the request was wrong.
            Err(http::Error::Api { status: 404, .. }) => return Ok(None),
            Err(error) => return Err(error.into()),
        };

        let mut delta = Delta {
            history_id: history.history_id.clone(),
            ..Delta::default()
        };
        let mut seen = std::collections::HashSet::new();
        for entry in &history.history {
            for added in &entry.added {
                if seen.insert(added.id.clone()) {
                    delta.changed.push(added.id.clone());
                }
            }
            for label in &entry.labels {
                if seen.insert(label.id.clone()) {
                    delta.changed.push(label.id.clone());
                }
            }
            for deleted in &entry.deleted {
                if seen.insert(deleted.id.clone()) {
                    delta.deleted.push(deleted.id.clone());
                }
            }
        }
        Ok(Some(delta))
    }

    /// Pull the full body for mails a delta flagged, so a resync does not
    /// re-download every body it already has.
    ///
    /// Each body arrives structured: the HTML part parsed to blocks when it
    /// exists and reads as something, plain paragraphs otherwise. Either way
    /// the caller stores one type.
    pub fn bodies(&mut self, ids: &[String]) -> Result<Vec<(String, Vec<RichBlock>)>> {
        let mut bodies = Vec::new();
        for id in ids {
            self.authorize()?;
            let full = gmail::message_full(&self.agent, &self.token, id)?;
            bodies.push((id.clone(), rich_body(&full)));
        }
        Ok(bodies)
    }

    /// Write a label change back. Read state and star both arrive here.
    pub fn modify(&mut self, id: &str, add: &[&str], remove: &[&str]) -> Result<()> {
        self.authorize()?;
        gmail::modify_labels(&self.agent, &self.token, id, add, remove)?;
        Ok(())
    }

    /// Every label Gmail knows about, including the system ones and their
    /// message counts. Kept separate from [`Self::labels`] so the counts that
    /// only system labels carry are not thrown away with the rest.
    fn all_labels(&mut self) -> Result<Vec<gmail::GmailLabel>> {
        self.authorize()?;
        Ok(gmail::labels(&self.agent, &self.token)?)
    }

    /// How many messages are in each folder, without downloading any of them.
    ///
    /// Seven units: a profile read for the account total, then one `labels.get`
    /// per system label. Neither `labels.list` nor `messages.list` can answer
    /// this — see the `counts` module for why that cost some finding.
    pub fn folder_counts(&mut self) -> Result<FolderCounts> {
        self.authorize()?;
        let profile = gmail::profile(&self.agent, &self.token)?;
        let agent = self.agent.clone();
        let token = self.token.clone();
        Ok(FolderCounts::from_lookup(
            |id| {
                gmail::label(&agent, &token, id)
                    .ok()
                    .and_then(|label| Some((label.messages_total?, label.messages_unread?)))
            },
            profile.total,
        ))
    }

    fn labels(&mut self) -> Result<Vec<RemoteLabel>> {
        let all = self.all_labels()?;
        Ok(all
            .into_iter()
            .filter(|label| !label.is_system() && label.is_visible())
            .map(|label| RemoteLabel {
                // Gmail's own colour when it has one, so a label keeps its
                // colour across clients.
                colour: label
                    .color
                    .as_deref()
                    .and_then(gmail_colour)
                    .unwrap_or_else(|| colour_of(&label.name)),
                id: label.id,
                name: label.name,
            })
            .collect())
    }

    /// One `messages.get` per id, several at a time.
    ///
    /// Gmail has no batch endpoint, so this is unavoidably one request per
    /// mail; `format=metadata` is what keeps each response small, and running
    /// them concurrently is what makes a large mailbox finish in seconds. The
    /// slots are indexed rather than appended so the result order matches the
    /// input, which the list relies on for its newest-first ordering.
    fn metadata_for(
        &self,
        ids: &[String],
        stream: Option<&MailStream>,
    ) -> Result<Vec<Option<gmail::MessageMetadata>>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let slots: Vec<Mutex<Slot>> = (0..ids.len()).map(|_| Mutex::new(Slot::Pending)).collect();
        let next = AtomicUsize::new(0);
        let failure: Mutex<Option<anyhow::Error>> = Mutex::new(None);
        let limiter = RateLimiter::new(SELF_LIMIT_UNITS_PER_MINUTE);

        thread::scope(|scope| {
            for _ in 0..FETCH_CONCURRENCY.min(ids.len()) {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= ids.len() {
                            break;
                        }
                        // Stop handing out work once something has gone wrong, so a
                        // dead token does not fan out into thousands of requests.
                        if lock(&failure).is_some() {
                            break;
                        }
                        limiter.acquire(COST_MESSAGE_GET);
                        let outcome =
                            match gmail::message_metadata(&self.agent, &self.token, &ids[index]) {
                                Ok(metadata) => Slot::Fetched(metadata),
                                // A single missing message is not a reason to abandon the
                                // sync: it is usually a mail deleted between listing the
                                // ids and fetching one of them.
                                Err(http::Error::Api { status: 404, .. }) => Slot::Missing,
                                Err(error) => {
                                    *lock(&failure) = Some(error.into());
                                    break;
                                }
                            };
                        // Straight into the list, before the whole fetch is
                        // done, so the inbox fills in as it goes. A message that
                        // turned out to be missing has nothing to show.
                        if let Some(stream) = stream
                            && let Slot::Fetched(metadata) = &outcome
                            && let Some(mail) = to_mail(metadata)
                        {
                            stream.push(mail);
                        }
                        *lock(&slots[index]) = outcome;
                    }
                });
            }
        });

        if let Some(error) = lock(&failure).take() {
            return Err(error);
        }
        let mut mail = Vec::with_capacity(ids.len());
        for slot in slots {
            // Taking the mutex by value: the workers are all joined, so this
            // is the only reference left and the slot can be moved out rather
            // than cloned.
            match slot
                .into_inner()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
            {
                Slot::Fetched(metadata) => mail.push(Some(metadata)),
                Slot::Missing | Slot::Pending => mail.push(None),
            }
        }
        Ok(mail)
    }
}

/// Project metadata onto a row the list can draw.
fn to_mail(metadata: &gmail::MessageMetadata) -> Option<RemoteMail> {
    if metadata.id.is_empty() {
        return None;
    }
    let from = metadata.header("From").unwrap_or_default();
    let (sender, address) = split_address(from);
    let recipients = metadata
        .header("To")
        .map(|to| {
            to.split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let seconds = metadata.unix_seconds();
    Some(RemoteMail {
        id: metadata.id.clone(),
        sender: if sender.is_empty() {
            address.clone()
        } else {
            sender
        },
        address,
        recipients,
        subject: metadata
            .header("Subject")
            .unwrap_or("(no subject)")
            .to_string(),
        preview: metadata.snippet.clone(),
        body: None,
        timestamp: seconds.map(short_date).unwrap_or_default(),
        full_date: seconds
            .map(full_date)
            .unwrap_or_else(|| "Unknown date".to_string()),
        label_ids: metadata.label_ids.clone(),
    })
}

/// `"Ada Lovelace <ada@example.com>"` -> `("Ada Lovelace", "ada@example.com")`.
fn split_address(from: &str) -> (String, String) {
    let from = from.trim();
    if from.is_empty() {
        return (String::new(), String::new());
    }
    match (from.rfind('<'), from.rfind('>')) {
        (Some(open), Some(close)) if close > open => (
            from[..open].trim().trim_matches('"').to_string(),
            from[open + 1..close].trim().to_string(),
        ),
        _ => (String::new(), from.to_string()),
    }
}

/// A stable colour for a label name, so a label is the same colour every run
/// and on every machine without anything being stored.
pub fn colour_of(name: &str) -> u32 {
    // FNV-1a, chosen for being short and for spreading single-character
    // changes across the whole range, which a label's name is made of.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    // Nori's existing palette: muted, low-saturation fills rather than
    // saturated chips, so the hue is a hint and not a spotlight.
    const PALETTE: [u32; 8] = [
        0x8a_9a_5b, // moss
        0x5b_8a_9a, // slate
        0x9a_8a_5b, // sand
        0x8a_5b_9a, // plum
        0x5b_9a_8a, // sea
        0x9a_6b_5b, // clay
        0x6b_5b_9a, // indigo
        0x7a_7a_5b, // olive
    ];
    PALETTE[(hash % PALETTE.len() as u32) as usize]
}

/// How many mails a single on-demand folder fetch indexes.
///
/// Set on its own rather than as a fraction of [`METADATA_BUDGET`], because the
/// two answer different questions. The first sync is the app's opening act and
/// wants to be cheap; a folder the user just clicked into can afford to be
/// useful. The pair still has to fit one quota minute together, which is what
/// the assertion near [`SELF_LIMIT_UNITS_PER_MINUTE`] checks.
pub const MAILBOX_FETCH_BUDGET: usize = 100;

/// The Inbox, which is what a first sync indexes.
///
/// Deliberately not `in:anywhere`. A first sync used to pull the newest mails
/// from *every* folder, which spent most of the budget on Archive and Trash
/// that the user had not asked to look at and would have to open separately
/// anyway. The Inbox is where the app opens, so it is what gets fetched; the
/// rest is reached by opening the folder.
pub const INBOX: &str = "in:inbox";

/// Every mailbox, including Trash and Spam. Only useful for a deliberate
/// everything-sweep, which nothing currently does.
pub const ANYWHERE: &str = "in:anywhere";

/// Gmail's own palette, so a label created in Gmail keeps its colour here.
fn gmail_colour(id: &str) -> Option<u32> {
    let value = match id {
        "color1" | "COLOR_1" => 0xdd_6b_5b,
        "color2" | "COLOR_2" => 0xe8_9c_3c,
        "color3" | "COLOR_3" => 0x8a_9a_5b,
        "color4" | "COLOR_4" => 0x5b_8a_9a,
        "color5" | "COLOR_5" => 0x9a_6b_5b,
        "color6" | "COLOR_6" => 0x9a_5b_8a,
        "color7" | "COLOR_7" => 0x6b_5b_9a,
        "color8" | "COLOR_8" => 0x5b_9a_8a,
        _ => return None,
    };
    Some(value)
}

/// A compact date for a list row. The year is dropped for recent mail, which
/// is what keeps the column narrow.
fn short_date(unix: u64) -> String {
    let (year, month, day, hour, minute) = parts(unix);
    let now = crate::token::unix_now();
    let (this_year, ..) = parts(now);
    let month_names = MONTHS;
    if year == this_year {
        format!(
            "{} {} {:02}:{:02}",
            day,
            month_names[month as usize - 1],
            hour,
            minute
        )
    } else {
        format!("{} {} {}", day, month_names[month as usize - 1], year)
    }
}

fn full_date(unix: u64) -> String {
    let (year, month, day, hour, minute) = parts(unix);
    format!(
        "{} {} {} {year} {hour:02}:{minute:02}",
        day,
        MONTHS[month as usize - 1],
        year
    )
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Break a Unix timestamp into civil date parts, in UTC.
fn parts(unix: u64) -> (i64, u32, u32, u32, u32) {
    let days = (unix / 86_400) as i64;
    let seconds = unix % 86_400;
    // Howard Hinnant's civil_from_days, which is exact for the whole range
    // and avoids a date dependency for five fields.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (
        year,
        month,
        day,
        (seconds / 3_600) as u32,
        ((seconds % 3_600) / 60) as u32,
    )
}

/// Structured body for one message: the HTML part parsed to blocks when it
/// exists and reads as something, plain paragraphs otherwise. The caller
/// stores one type either way.
pub fn rich_body(message: &serde_json::Value) -> Vec<RichBlock> {
    let mut plain = Vec::new();
    let mut html = Vec::new();
    collect_parts(message.get("payload"), &mut plain, &mut html);
    let text = html.join("\n");
    let parsed = crate::rich::parse_html_body(&text);
    if parsed.is_empty() {
        crate::rich::text_blocks(body_paragraphs(message))
    } else {
        parsed
    }
}

/// Pull readable paragraphs out of a Gmail message body.
///
/// Gmail returns MIME, and the body is usually several parts deep, sometimes
/// base64url-encoded, and frequently HTML-only. Walking it properly matters:
/// a mail that renders blank because the walker gave up is indistinguishable
/// from a mail with no content.
pub fn body_paragraphs(message: &serde_json::Value) -> Vec<String> {
    let mut plain = Vec::new();
    let mut html = Vec::new();
    collect_parts(message.get("payload"), &mut plain, &mut html);

    let source = if plain.is_empty() { html } else { plain };
    let text = source
        .iter()
        .map(|part| part.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let stripped = if source_is_html(message) {
        html_to_text(&text)
    } else {
        text
    };

    normalize_plain_text(&stripped)
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether the chosen part was HTML, so a plain-text-only message is not run
/// through the tag stripper for nothing.
fn source_is_html(message: &serde_json::Value) -> bool {
    fn walk(part: &serde_json::Value) -> Option<String> {
        let mime = part.get("mimeType")?.as_str()?;
        if mime == "text/plain" && has_data(part) {
            return Some(mime.to_string());
        }
        if mime == "text/html" && has_data(part) {
            return Some(mime.to_string());
        }
        part.get("parts")?.as_array()?.iter().find_map(walk)
    }
    let Some(mime) = message.get("payload").and_then(walk) else {
        return false;
    };
    mime == "text/html"
}

fn has_data(part: &serde_json::Value) -> bool {
    part.get("body")
        .and_then(|body| body.get("data"))
        .and_then(|data| data.as_str())
        .is_some_and(|data| !data.is_empty())
}

fn collect_parts(
    part: Option<&serde_json::Value>,
    plain: &mut Vec<String>,
    html: &mut Vec<String>,
) {
    let Some(part) = part else { return };

    // A leaf first, then recurse. Gmail puts the useful part at varying depths
    // depending on how many alternatives the sender's client produced.
    let leaf = part.get("mimeType").and_then(|mime| mime.as_str()).zip(
        part.get("body")
            .and_then(|body| body.get("data"))
            .and_then(|data| data.as_str())
            .filter(|data| !data.is_empty()),
    );

    if let Some((mime, data)) = leaf {
        let decoded = decode_body(data);
        match mime {
            "text/plain" => plain.push(decoded),
            "text/html" => html.push(decoded),
            _ => {}
        }
        // A part with data is a leaf; descending past it would double the text
        // if a client also included children.
        return;
    }
    if let Some(parts) = part.get("parts").and_then(|parts| parts.as_array()) {
        for child in parts {
            collect_parts(Some(child), plain, html);
        }
    }
}

/// Bodies arrive base64url-encoded, without padding.
fn decode_body(data: &str) -> String {
    let padded = match data.len() % 4 {
        0 => data.to_string(),
        remainder => format!("{data}{}", "=".repeat(4 - remainder)),
    };
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(padded.trim_end_matches('='))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        // Returning the raw data beats returning nothing: a mangled body is
        // still readable, an absent one is not.
        .unwrap_or_else(|| data.to_string())
}

/// Tidy a plain-text body for reading, whatever part it came from.
///
/// Marketing mail wraps every link in angle brackets and pads with blank
/// runs; neither survives contact with a reader. Normalising here rather
/// than at render keeps every downstream consumer — list previews, search,
/// the reading view — working from the same clean text.
fn normalize_plain_text(text: &str) -> String {
    collapse_blank_lines(&unwrap_angle_urls(text.trim()))
}

/// Strip the angle brackets ESPs wrap links in (`<https://…>` → `https://…`),
/// so the reading view shows the link itself rather than line noise.
///
/// Only `http(s)` URLs unwrap: anything else in brackets (an address, a
/// placeholder) is content, not a link, and an unclosed bracket is left
/// alone rather than guessed at.
fn unwrap_angle_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let after_open = &rest[open + 1..];
        let is_url = after_open.starts_with("http://") || after_open.starts_with("https://");
        // The URL runs to the first whitespace, bracket, or end of text, and
        // only unwraps when its closing bracket is actually there.
        let mut close = None;
        for (i, c) in after_open.char_indices() {
            if c.is_whitespace() || c == '<' {
                break;
            }
            if c == '>' {
                close = Some(i);
                break;
            }
        }
        match (is_url, close) {
            (true, Some(end)) => {
                out.push_str(&rest[..open]);
                out.push_str(&after_open[..end]);
                rest = &after_open[end + 1..];
            }
            _ => {
                out.push_str(&rest[..=open]);
                rest = after_open;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Collapse runs of three or more consecutive blank lines to one.
/// Whitespace-only lines count as blank, so padded runs collapse too; one
/// or two blank lines survive as paragraph spacing for the splitter.
fn collapse_blank_lines(text: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut blanks = 0usize;
    for line in text.split('\n') {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks <= 2 {
                kept.push("");
            }
        } else {
            blanks = 0;
            kept.push(line);
        }
    }
    // Leading and trailing blank runs go entirely: a body starts and ends
    // on content, not air.
    while kept.first().is_some_and(|line| line.is_empty()) {
        kept.remove(0);
    }
    while kept.last().is_some_and(|line| line.is_empty()) {
        kept.pop();
    }
    kept.join("\n")
}

/// Enough HTML to text to read a mail: drop the tags, keep the breaks.
fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for character in html.chars() {
        match character {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                // Block-level tags become paragraph breaks, so the structure
                // survives rather than collapsing into one wall of text.
                let name = tag
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "p" | "br" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "blockquote"
                ) {
                    out.push_str("\n\n");
                }
            }
            _ if !in_tag => out.push(character),
            // Inside a tag: collect the name so the `>` arm can recognise it.
            // Skipping these without recording them is what made every block
            // element read as one unbroken paragraph.
            _ => tag.push(character),
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Quota is per-minute per project, so a runaway loop here is a self-inflicted
/// 429. Kept public so the sync can report why it stopped.
pub const fn metadata_budget() -> usize {
    METADATA_BUDGET
}

pub fn base_url() -> &'static str {
    GMAIL_BASE
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_sender_with_a_display_name_is_split_into_both_halves() {
        assert_eq!(
            split_address("Ada Lovelace <ada@example.com>"),
            ("Ada Lovelace".to_string(), "ada@example.com".to_string())
        );
        assert_eq!(
            split_address("\"Lovelace, Ada\" <ada@example.com>"),
            ("Lovelace, Ada".to_string(), "ada@example.com".to_string())
        );
    }

    #[test]
    fn a_bare_address_becomes_its_own_display_name() {
        let (sender, address) = split_address("ada@example.com");
        assert_eq!(sender, "", "there is no display name to show");
        assert_eq!(address, "ada@example.com");
    }

    #[test]
    fn a_malformed_header_does_not_panic() {
        for input in ["", "   ", "<unclosed", "no-at-sign", "a < b > c"] {
            let _ = split_address(input);
        }
    }

    /// The real gate, and the reason `type` has an explicit rename: without it
    /// every system label decodes as a user label and `INBOX` shows up in the
    /// sidebar as one of Nori's own.
    /// The limiter must terminate and keep to its rate. A livelock here does
    /// not fail loudly — the sync simply never finishes — so it is worth a test
    /// that can only pass by returning.
    #[test]
    fn the_rate_limiter_drains_and_keeps_to_its_rate() {
        // 1,000 units per minute is 60ms per unit.
        let limiter = RateLimiter::new(1_000);
        let started = std::time::Instant::now();
        for _ in 0..5 {
            limiter.acquire(1);
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed >= std::time::Duration::from_millis(180),
            "five units at 60ms apart must not complete in {elapsed:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "the limiter must not stall; took {elapsed:?}"
        );
    }

    /// Concurrent callers share one budget. A per-thread limiter multiplied by
    /// the thread count is exactly how a parallel fetch turns into a 403.
    #[test]
    fn concurrent_callers_share_the_budget() {
        let limiter = RateLimiter::new(1_000);
        let started = std::time::Instant::now();
        thread::scope(|scope| {
            for _ in 0..4 {
                let limiter = &limiter;
                scope.spawn(move || {
                    for _ in 0..5 {
                        limiter.acquire(1);
                    }
                });
            }
        });
        let elapsed = started.elapsed();
        assert!(
            elapsed >= std::time::Duration::from_millis(240),
            "20 units at 60ms apart must take over a second, took {elapsed:?}"
        );
    }

    /// The whole reason [`METADATA_BUDGET`] is 250 and not "all of it": a sync
    /// has to finish inside the per-user allowance, and that allowance is 300
    /// mails a minute, not the 1.2 million a careless reading of the quota page
    /// suggests.
    #[test]
    fn the_budget_fits_inside_one_quota_minute() {
        // The case that actually bites: a first sync and a folder fetch in the
        // same minute. Each alone would fit, so a check on either one in
        // isolation passes while the combination still earns a 403.
        let together = (METADATA_BUDGET + MAILBOX_FETCH_BUDGET) as u64 * COST_MESSAGE_GET;
        assert!(
            together <= SELF_LIMIT_UNITS_PER_MINUTE,
            "a first sync and a folder fetch together need {together} units but the \
             self-limit is {SELF_LIMIT_UNITS_PER_MINUTE}"
        );
    }

    #[test]
    fn only_user_labels_become_nori_labels() {
        let system: gmail::GmailLabel =
            serde_json::from_str(r#"{"id": "INBOX", "name": "INBOX", "type": "system"}"#).unwrap();
        assert!(system.is_system());

        let user: gmail::GmailLabel = serde_json::from_str(
            r#"{"id": "Label_7", "name": "School", "type": "user",
                "messageListVisibility": "show"}"#,
        )
        .unwrap();
        assert!(!user.is_system(), "a Nori label must be a user label");
    }

    #[test]
    fn a_label_colour_is_stable_across_runs() {
        assert_eq!(colour_of("Work"), colour_of("Work"));
        assert_ne!(colour_of("Work"), colour_of("Personal"));
    }

    #[test]
    fn gammails_own_colours_win_over_the_generated_one() {
        // A label made in Gmail should not change colour just because Nori
        // was upgraded.
        assert_eq!(gmail_colour("color3"), Some(0x8a_9a_5b));
        assert_eq!(gmail_colour("#ff0000"), None);
    }

    #[test]
    fn dates_split_correctly_across_a_year_boundary() {
        // 2023-01-01T00:00:00Z, and 2024-02-29T11:34:00Z — the leap day, in
        // UTC, which is the timezone Gmail's `internalDate` is defined in.
        assert_eq!(parts(1_672_531_200), (2023, 1, 1, 0, 0));
        assert_eq!(parts(1_709_206_440), (2024, 2, 29, 11, 34));
    }

    #[test]
    fn a_recent_mail_omits_the_year_from_its_list_date() {
        let now = crate::token::unix_now();
        assert!(!short_date(now).contains(&now.to_string()[..2]));
        assert!(short_date(now).contains(':'), "a list row shows the time");
    }

    #[test]
    fn a_plain_text_body_splits_into_paragraphs() {
        let message = json!({
            "payload": {
                "mimeType": "text/plain",
                "body": { "data": encode("First para.\n\nSecond para.") },
            }
        });
        assert_eq!(
            body_paragraphs(&message),
            vec!["First para.", "Second para."]
        );
    }

    #[test]
    fn an_html_only_body_becomes_readable_text() {
        let message = json!({
            "payload": {
                "mimeType": "text/html",
                "body": { "data": encode("<p>Hello</p><p>World &amp; friends</p>") },
            }
        });
        assert_eq!(body_paragraphs(&message), vec!["Hello", "World & friends"]);
    }

    #[test]
    fn a_multipart_message_prefers_its_plain_alternative() {
        let message = json!({
            "payload": {
                "mimeType": "multipart/alternative",
                "body": { "data": "" },
                "parts": [
                    { "mimeType": "text/plain", "body": { "data": encode("Readable.") } },
                    { "mimeType": "text/html", "body": { "data": encode("<p>Readable.</p>") } },
                ],
            }
        });
        assert_eq!(body_paragraphs(&message), vec!["Readable."]);
    }

    #[test]
    fn a_nested_multipart_walk_reaches_the_leaf() {
        let message = json!({
            "payload": {
                "mimeType": "multipart/mixed",
                "body": { "data": "" },
                "parts": [
                    {
                        "mimeType": "multipart/alternative",
                        "body": { "data": "" },
                        "parts": [
                            { "mimeType": "text/plain", "body": { "data": encode("Deep.") } },
                        ],
                    },
                    { "mimeType": "application/pdf", "body": { "data": "ignored" } },
                ],
            }
        });
        assert_eq!(body_paragraphs(&message), vec!["Deep."]);
    }

    #[test]
    fn an_attachment_does_not_become_body_text() {
        let message = json!({
            "payload": {
                "mimeType": "multipart/mixed",
                "body": { "data": "" },
                "parts": [
                    { "mimeType": "text/plain", "body": { "data": encode("See attached.") } },
                    {
                        "filename": "report.pdf",
                        "mimeType": "application/pdf",
                        "body": { "attachmentId": "abc", "data": encode("%PDF-1.4") },
                    },
                ],
            }
        });
        assert_eq!(body_paragraphs(&message), vec!["See attached."]);
    }

    #[test]
    fn a_message_with_no_readable_part_yields_no_paragraphs() {
        let message = json!({
            "payload": { "mimeType": "multipart/mixed", "body": { "data": "" }, "parts": [] }
        });
        assert!(body_paragraphs(&message).is_empty());
        // A message with no payload at all must not panic either.
        assert!(body_paragraphs(&json!({})).is_empty());
    }

    #[test]
    fn an_undecodable_body_falls_back_to_the_raw_text() {
        let message = json!({
            "payload": { "mimeType": "text/plain", "body": { "data": "!!!not base64!!!" } }
        });
        let paragraphs = body_paragraphs(&message);
        assert_eq!(paragraphs.len(), 1, "better raw than nothing");
    }

    #[test]
    fn html_entities_and_breaks_survive_the_strip() {
        let text = html_to_text("<div>a<br>b</div><p>c &amp; d</p>");
        assert!(text.contains('a') && text.contains('b') && text.contains('c'));
        assert!(text.contains("&"), "an entity must be decoded, not dropped");
        assert!(!text.contains('<') && !text.contains('>'));
    }

    #[test]
    fn angle_wrapped_links_unwrap_to_bare_urls() {
        let message = json!({
            "payload": {
                "mimeType": "text/plain",
                "body": { "data": encode("Shop the sale\n<https://shop.example/e>\n<https://www.example.com/>") },
            }
        });
        let paragraphs = body_paragraphs(&message);
        assert!(
            paragraphs.iter().all(|p| !p.contains('<')),
            "no brackets should survive unwrapping: {paragraphs:?}"
        );
        assert!(
            paragraphs
                .iter()
                .any(|p| p.contains("https://shop.example/e")),
            "the URL itself must survive: {paragraphs:?}"
        );
    }

    #[test]
    fn brackets_around_anything_but_a_url_are_left_alone() {
        let message = json!({
            "payload": {
                "mimeType": "text/plain",
                "body": { "data": encode("Write <bob@example.com> or <https://x.example>") },
            }
        });
        let paragraphs = body_paragraphs(&message);
        assert!(
            paragraphs.iter().any(|p| p.contains("<bob@example.com>")),
            "an address in brackets is content, not a link: {paragraphs:?}"
        );
        assert!(
            paragraphs.iter().any(|p| p.contains("https://x.example")),
            "the real URL still unwraps: {paragraphs:?}"
        );
    }

    #[test]
    fn padded_blank_runs_collapse_and_ends_trim() {
        let message = json!({
            "payload": {
                "mimeType": "text/plain",
                "body": { "data": encode("\n\n  \nFirst.\n \n \n \nSecond.\n\n   \n") },
            }
        });
        assert_eq!(body_paragraphs(&message), vec!["First.", "Second."]);
    }

    #[test]
    fn rich_body_prefers_structured_html() {
        let message = json!({
            "payload": {
                "mimeType": "multipart/alternative",
                "body": { "data": "" },
                "parts": [
                    { "mimeType": "text/plain", "body": { "data": encode("Hi there") } },
                    { "mimeType": "text/html", "body": { "data": encode("<h1>Hi</h1><p>there</p>") } },
                ],
            }
        });
        let blocks = rich_body(&message);
        assert!(
            matches!(blocks.first(), Some(RichBlock::Heading { .. })),
            "HTML structure must win over the plain alternative: {blocks:?}"
        );
    }

    #[test]
    fn rich_body_falls_back_to_plain_paragraphs() {
        let message = json!({
            "payload": {
                "mimeType": "text/plain",
                "body": { "data": encode("Just text") },
            }
        });
        assert_eq!(
            rich_body(&message),
            crate::rich::text_blocks(vec!["Just text".to_string()])
        );
    }

    #[test]
    fn a_metadata_message_projects_onto_a_list_row() {
        let metadata = gmail::MessageMetadata {
            id: "18d5f3c2".into(),
            thread_id: "t1".into(),
            label_ids: vec!["INBOX".into(), "UNREAD".into()],
            snippet: "Hello there".into(),
            internal_date: Some("1709206440000".into()),
            payload: Some(gmail::Headers {
                headers: vec![
                    gmail::Header {
                        name: "From".into(),
                        value: "Ada <ada@example.com>".into(),
                    },
                    gmail::Header {
                        name: "Subject".into(),
                        value: "Analytical".into(),
                    },
                    gmail::Header {
                        name: "To".into(),
                        value: "me@x.com, you@x.com".into(),
                    },
                ],
            }),
        };
        let mail = to_mail(&metadata).expect("a complete message projects");
        assert_eq!(mail.sender, "Ada");
        assert_eq!(mail.address, "ada@example.com");
        assert_eq!(mail.recipients, vec!["me@x.com", "you@x.com"]);
        assert!(mail.has_label("UNREAD"));
        assert_eq!(mail.body, None, "a body is fetched lazily or not at all");
        assert!(mail.full_date.contains("2024"));
    }

    #[test]
    fn a_senderless_message_falls_back_to_its_address() {
        let metadata = gmail::MessageMetadata {
            id: "1".into(),
            payload: Some(gmail::Headers {
                headers: vec![gmail::Header {
                    name: "From".into(),
                    value: "noreply@service.com".into(),
                }],
            }),
            ..gmail::MessageMetadata::default()
        };
        let mail = to_mail(&metadata).unwrap();
        assert_eq!(mail.sender, "noreply@service.com");
    }

    #[test]
    fn a_message_with_no_headers_still_projects() {
        let mail = to_mail(&gmail::MessageMetadata {
            id: "1".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(mail.subject, "(no subject)");
        assert!(mail.recipients.is_empty());
        assert!(mail.timestamp.is_empty());
        assert_eq!(mail.full_date, "Unknown date");
    }

    #[test]
    fn a_message_with_no_id_is_refused() {
        assert!(
            to_mail(&gmail::MessageMetadata::default()).is_none(),
            "a mail with no id could never be opened again"
        );
    }

    fn encode(text: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(text.as_bytes())
    }
}
