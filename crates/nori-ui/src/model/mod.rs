pub mod account;
pub mod cache;
pub mod density;
pub mod labels;
pub mod mail;
pub mod mock;
pub mod settings;

pub use account::{AccountState, gmail_query, to_email, to_label_seed};
pub use cache::{Index, IndexCache, may_replace_index};
pub use density::Density;
pub use labels::{Label, LabelId, LabelStore};
pub use mail::{
    DraftSeed, Email, EmailId, EmailSummary, MailStore, Mailbox, Origin, Overlay, WorkspaceView,
};
pub use settings::{Setting, SettingsPage, SettingsState};
