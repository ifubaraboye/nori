//! Gmail, and the OAuth dance that gets at it.
//!
//! This crate is deliberately free of `gpui` and of `nori-ui`. The client is
//! blocking HTTP with no ambient context, so it has to be drivable from a
//! plain thread, and that constraint is what keeps the sync testable without
//! a window. It is also enforced by the absence of those dependencies: this
//! crate cannot grow a reference to the view layer even by accident.
//!
//! ```no_run
//! use nori_gmail::{agent, oauth, token::{FileTokenStore, TokenStore}};
//!
//! let credentials = oauth::Credentials::from_env()?;
//! let store = FileTokenStore::with_account("me@example.com")?;
//!
//! let request = oauth::begin(&credentials)?;
//! // Both are needed to redeem the code, so take them before the listener is
//! // consumed by waiting on it.
//! let (redirect_uri, verifier) = (request.redirect_uri.clone(), request.verifier().to_owned());
//!
//! // ... hand `request.url` to the system browser, then block on the loopback.
//! let code = request.await_callback()?;
//!
//! let token = oauth::exchange(&agent(), &credentials, &redirect_uri, &code, &verifier)?;
//! store.save(&token)?;
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod config;
pub mod counts;
pub mod gmail;
pub mod http;
pub mod oauth;
pub mod pkce;
pub mod rich;
pub mod stream;
pub mod sync;
pub mod token;

pub use config::credentials;
pub use counts::FolderCounts;
pub use http::{Error, agent};
pub use oauth::Credentials;
pub use rich::{
    RichBlock, RichBody, RichSpan, image_sources, parse_html_body, plain_text, text_blocks,
};
pub use stream::{IncomingMail, MailStream, drain, mail_stream};
pub use sync::{Delta, Incremental, RemoteLabel, RemoteMail, Snapshot, Sync, SyncOutcome};
pub use token::{FileTokenStore, LastAccount, Token, TokenStore};
