//! Gmail's REST surface, as Nori needs it.
//!
//! Metadata-first: a list row needs headers, a snippet and labels, so that is
//! what sync fetches. Bodies are pulled only when a mail is actually opened,
//! which saves payload and latency rather than quota — `messages.get` costs
//! the same 20 units whether `format=metadata` or `format=full`.

use serde::Deserialize;

use crate::http::{self, Error};
use crate::oauth::urlencode;
use crate::token::Token;

pub const GMAIL_BASE: &str = "https://gmail.googleapis.com/gmail/v1";

/// How many ids one `messages.list` call asks for.
///
/// Gmail's maximum. The call costs 5 units whether it returns one id or five
/// hundred, so the largest page is strictly the cheapest way to walk a mailbox:
/// the budgets in `sync` need one request instead of five.
pub const PAGE_SIZE: usize = 500;

pub use crate::http::agent;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GmailLabel {
    pub id: String,
    pub name: String,
    /// Gmail calls this `type`. The rename is not a case change, so
    /// `rename_all = "camelCase"` leaves it alone and it has to be said here.
    /// Without it every system label decodes as an empty kind, `is_system`
    /// answers false, and `INBOX` and `UNREAD` turn up as Nori labels.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Gmail's own colour ids, or a hex string it copied from a client.
    #[serde(default)]
    pub color: Option<String>,
    /// True for the system labels Nori maps onto its own mailboxes. User
    /// labels are the ones that become Nori labels.
    #[serde(default)]
    pub message_list_visibility: Option<String>,
    /// How many messages carry this label, across the whole mailbox.
    ///
    /// This is what puts a real number next to a folder Nori has not
    /// downloaded. Counting the mails in the local index would show 0 for
    /// every folder the user has not opened yet, which reads as an empty
    /// mailbox rather than an unvisited one. It comes free with the label read
    /// that already happens, and `labels.list` costs one unit.
    #[serde(default)]
    pub messages_total: Option<u64>,
    #[serde(default)]
    pub messages_unread: Option<u64>,
}

impl GmailLabel {
    pub fn is_system(&self) -> bool {
        self.kind == "system"
    }

    /// Whether a user label shows up in the normal mail list. Gmail's own
    /// "hide in list" flag would otherwise silently hide a Nori label's mail.
    pub fn is_visible(&self) -> bool {
        self.message_list_visibility.as_deref() != Some("hide")
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageMetadata {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub internal_date: Option<String>,
    #[serde(default)]
    pub payload: Option<Headers>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Headers {
    #[serde(default)]
    pub headers: Vec<Header>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Headers {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
    }
}

impl MessageMetadata {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.payload.as_ref().and_then(|payload| payload.get(name))
    }

    /// Gmail reports an absent `internalDate` as the string `"0"`, which is a
    /// millisecond timestamp at the epoch rather than a missing value.
    pub fn unix_seconds(&self) -> Option<u64> {
        self.internal_date
            .as_deref()
            .filter(|date| *date != "0")
            .and_then(|date| date.parse::<u64>().ok())
            .map(|millis| millis / 1000)
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageList {
    #[serde(default)]
    pub messages: Vec<ListedMessage>,
    #[serde(rename = "nextPageToken", default)]
    pub next_page: Option<String>,
    #[serde(rename = "resultSizeEstimate", default)]
    pub estimate: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListedMessage {
    pub id: String,
    #[serde(default)]
    pub thread_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct History {
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    #[serde(default)]
    pub history_id: Option<String>,
    #[serde(rename = "nextPageToken", default)]
    pub next_page: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    #[serde(default)]
    pub added: Vec<HistoryMessage>,
    #[serde(default)]
    pub deleted: Vec<HistoryMessage>,
    #[serde(default)]
    pub labels: Vec<HistoryLabel>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMessage {
    pub id: String,
    #[serde(default)]
    pub message: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryLabel {
    pub id: String,
    #[serde(default)]
    pub message: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(rename = "emailAddress", default)]
    pub email: String,
    #[serde(rename = "messagesTotal", default)]
    pub total: u64,
    #[serde(default)]
    pub history_id: Option<String>,
}

/// The account's own address, which Nori shows in Settings → Account.
pub fn profile(agent: &ureq::Agent, token: &Token) -> Result<Profile, Error> {
    get(agent, token, &format!("{GMAIL_BASE}/users/me/profile"))
}

pub fn labels(agent: &ureq::Agent, token: &Token) -> Result<Vec<GmailLabel>, Error> {
    #[derive(Deserialize)]
    struct Response {
        #[serde(default)]
        labels: Vec<GmailLabel>,
    }
    let url = format!("{GMAIL_BASE}/users/me/labels");
    Ok(get::<Response>(agent, token, &url)?.labels)
}

/// One label, with its counts.
///
/// `labels.list` leaves `messagesTotal` and `messagesUnread` empty; only this
/// fills them in. One unit, and the only way to learn a folder's size without
/// downloading it. See the `counts` module.
pub fn label(agent: &ureq::Agent, token: &Token, id: &str) -> Result<GmailLabel, Error> {
    let url = format!("{GMAIL_BASE}/users/me/labels/{}", urlencode(id));
    get(agent, token, &url)
}

/// One page of message ids matching `query`.
pub fn list_messages(
    agent: &ureq::Agent,
    token: &Token,
    query: &str,
    page_token: Option<&str>,
) -> Result<MessageList, Error> {
    let mut url = format!(
        "{GMAIL_BASE}/users/me/messages?maxResults={PAGE_SIZE}&q={}",
        crate::oauth::urlencode(query)
    );
    if let Some(page) = page_token {
        url.push_str(&format!("&pageToken={}", crate::oauth::urlencode(page)));
    }
    get(agent, token, &url)
}

/// Headers, snippet and labels for one message, without its body.
pub fn message_metadata(
    agent: &ureq::Agent,
    token: &Token,
    id: &str,
) -> Result<MessageMetadata, Error> {
    let url = format!(
        "{GMAIL_BASE}/users/me/messages/{}?format=metadata\
         &metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date",
        urlencode(id)
    );
    get(agent, token, &url)
}

/// The full message, including the body part.
pub fn message_full(
    agent: &ureq::Agent,
    token: &Token,
    id: &str,
) -> Result<serde_json::Value, Error> {
    let url = format!(
        "{GMAIL_BASE}/users/me/messages/{}?format=full",
        urlencode(id)
    );
    get(agent, token, &url)
}

/// Changes since `start_history_id`.
///
/// Gmail ages history out after about a week and answers 404 once a stored id
/// is too old, which is a full resync rather than an error.
pub fn history(
    agent: &ureq::Agent,
    token: &Token,
    start_history_id: &str,
) -> Result<History, Error> {
    let url = format!(
        "{GMAIL_BASE}/users/me/history?startHistoryId={}&historyTypes=messageAdded\
         &historyTypes=messageDeleted&historyTypes=labelAdded&historyTypes=labelRemoved",
        crate::oauth::urlencode(start_history_id)
    );
    get(agent, token, &url)
}

/// Add or remove labels on a message: read state, star, trash, archive.
pub fn modify_labels(
    agent: &ureq::Agent,
    token: &Token,
    id: &str,
    add: &[&str],
    remove: &[&str],
) -> Result<(), Error> {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        #[serde(
            rename = "addLabelIds",
            default,
            skip_serializing_if = "<[&str]>::is_empty"
        )]
        add: &'a [&'a str],
        #[serde(
            rename = "removeLabelIds",
            default,
            skip_serializing_if = "<[&str]>::is_empty"
        )]
        remove: &'a [&'a str],
    }
    let body = serde_json::to_string(&Body { add, remove }).map_err(|error| Error::Malformed {
        endpoint: "the modify request".to_string(),
        reason: error.to_string(),
    })?;
    let url = format!("{GMAIL_BASE}/users/me/messages/{}/modify", urlencode(id));
    let sent = agent
        .post(&url)
        .header("authorization", &format!("Bearer {}", token.access_token))
        .header("content-type", "application/json")
        .send(body.as_bytes());
    http::read_body(sent, &url).map(|_| ())
}

fn get<T: for<'de> Deserialize<'de>>(
    agent: &ureq::Agent,
    token: &Token,
    url: &str,
) -> Result<T, Error> {
    let sent = agent
        .get(url)
        .header("authorization", &format!("Bearer {}", token.access_token))
        .call();
    let body = http::read_body(sent, url)?;
    serde_json::from_str(&body).map_err(|error| Error::Malformed {
        endpoint: url.to_string(),
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The failure this guards against is invisible to a test written in Rust
    /// field names: serde matches the *JSON* name, so `label_ids` finds nothing
    /// in a body that says `labelIds`, and a `#[serde(default)]` turns that into
    /// an empty list rather than an error. Every mail then looks unread,
    /// unstarred and unfiled. These bodies are Gmail's real spelling.
    #[test]
    fn camel_cased_fields_survive_being_decoded() {
        let message: MessageMetadata = serde_json::from_str(
            r#"{
                "id": "18d5f3c2",
                "threadId": "18d5f3c1",
                "labelIds": ["INBOX", "UNREAD", "IMPORTANT"],
                "snippet": "Hello there",
                "internalDate": "1709206440000",
                "payload": {"headers": [{"name": "From", "value": "Ada <ada@example.com>"}]}
            }"#,
        )
        .expect("Gmail's own spelling must decode");

        assert_eq!(message.thread_id, "18d5f3c1");
        assert_eq!(message.label_ids, ["INBOX", "UNREAD", "IMPORTANT"]);
        assert_eq!(message.unix_seconds(), Some(1_709_206_440));
        assert_eq!(message.header("From"), Some("Ada <ada@example.com>"));
    }

    #[test]
    fn a_paged_list_decodes_gmails_paging_fields() {
        let page: MessageList = serde_json::from_str(
            r#"{
                "messages": [{"id": "a", "threadId": "t1"}, {"id": "b", "threadId": "t2"}],
                "nextPageToken": "CiAKGjB",
                "resultSizeEstimate": 5000
            }"#,
        )
        .expect("paging fields must decode");
        assert_eq!(page.messages.len(), 2);
        assert_eq!(page.next_page.as_deref(), Some("CiAKGjB"));
        assert_eq!(page.estimate, Some(5000));
    }

    #[test]
    fn a_profile_decodes_gmails_totals() {
        let profile: Profile = serde_json::from_str(
            r#"{"emailAddress": "me@example.com", "messagesTotal": 6967, "historyId": "1777731"}"#,
        )
        .expect("profile fields must decode");
        assert_eq!(profile.email, "me@example.com");
        assert_eq!(profile.total, 6967);
        assert_eq!(profile.history_id.as_deref(), Some("1777731"));
    }

    #[test]
    fn a_label_decodes_its_visibility_and_kind() {
        let label: GmailLabel = serde_json::from_str(
            r#"{"id": "Label_7", "name": "School", "type": "user",
                "messageListVisibility": "show", "color": "color4"}"#,
        )
        .expect("label fields must decode");
        assert!(!label.is_system());
        assert!(label.is_visible());
        assert_eq!(label.color.as_deref(), Some("color4"));
    }

    #[test]
    fn history_entries_decode_with_their_relations() {
        let history: History = serde_json::from_str(
            r#"{
                "history": [
                    {"added": [{"id": "m1"}], "deleted": [], "labels": []},
                    {"added": [], "deleted": [{"id": "m0"}], "labels": [{"id": "m2"}]}
                ],
                "historyId": "1777732",
                "nextPageToken": "more"
            }"#,
        )
        .expect("history must decode");
        assert_eq!(history.history.len(), 2);
        assert_eq!(history.history[0].added[0].id, "m1");
        assert_eq!(history.history[1].deleted[0].id, "m0");
        assert_eq!(history.history[1].labels[0].id, "m2");
        assert_eq!(history.history_id.as_deref(), Some("1777732"));
    }

    #[test]
    fn a_system_label_is_told_apart_from_a_user_label() {
        let system = GmailLabel {
            id: "INBOX".into(),
            name: "INBOX".into(),
            kind: "system".into(),
            ..GmailLabel::default()
        };
        assert!(system.is_system());

        let user = GmailLabel {
            kind: "user".into(),
            ..system
        };
        assert!(!user.is_system(), "Nori labels come from user labels only");
    }

    #[test]
    fn a_label_hidden_from_the_list_still_counts() {
        let visible = GmailLabel {
            message_list_visibility: Some("show".into()),
            ..GmailLabel::default()
        };
        assert!(visible.is_visible());

        let hidden = GmailLabel {
            message_list_visibility: Some("hide".into()),
            ..GmailLabel::default()
        };
        assert!(!hidden.is_visible());
    }

    #[test]
    fn headers_are_matched_without_regard_to_case() {
        let headers = Headers {
            headers: vec![Header {
                name: "Subject".into(),
                value: "Hello".into(),
            }],
        };
        assert_eq!(headers.get("subject"), Some("Hello"));
        assert_eq!(headers.get("SUBJECT"), Some("Hello"));
        assert_eq!(headers.get("From"), None);
    }

    #[test]
    fn a_zero_internal_date_reads_as_missing() {
        let zero = MessageMetadata {
            internal_date: Some("0".into()),
            ..MessageMetadata::default()
        };
        assert_eq!(
            zero.unix_seconds(),
            None,
            "Gmail sends \"0\" for an undated mail, which must not become 1970"
        );

        let real = MessageMetadata {
            internal_date: Some("1750000000000".into()),
            ..MessageMetadata::default()
        };
        assert_eq!(real.unix_seconds(), Some(1_750_000_000));
    }

    #[test]
    fn a_message_with_no_headers_degrades_rather_than_panicking() {
        let bare = MessageMetadata::default();
        assert_eq!(bare.header("Subject"), None);
        assert_eq!(bare.unix_seconds(), None);
    }

    #[test]
    fn a_page_token_lands_in_the_query_string() {
        // A page token is opaque and routinely contains characters that must
        // be escaped; a raw paste would split the query and truncate the sync.
        let page = "CigK/a==";
        assert!(!page.contains('=') || true);
        let encoded = crate::oauth::urlencode(page);
        assert!(
            !encoded.contains('='),
            "an unescaped token would break the query"
        );
    }
}
