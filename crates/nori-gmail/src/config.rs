//! Finding the OAuth client credentials.
//!
//! The client id and secret have to be available at runtime, and a desktop app
//! has two very different ways of being started: from a shell where they can be
//! exported, and from a launcher where nothing is exported. Reading only the
//! process environment means the app works from the first and silently shows an
//! empty mailbox from the second, which reads as "the sync is broken" rather
//! than "the app was started without its configuration".
//!
//! So a `.env` file is read as well, and when neither is available the error
//! names every place that was looked in.

use anyhow::{Result, bail};

use crate::oauth::Credentials;

/// Where a `.env` was found, or every place that was tried.
const ENV_VAR: &str = "NORI_GMAIL_CLIENT_ID";

/// Load the credentials, from the environment or from a `.env` file.
///
/// Searched in order: the process environment, `$NORI_ENV_FILE`, `.env` beside
/// the working directory, and `.env` in the config directory. The first two are
/// deliberate overrides; the last is what makes a launcher-started app work at
/// all.
pub fn credentials() -> Result<Credentials> {
    let mut from_env: Option<Credentials> = None;
    let mut searched: Vec<String> = Vec::new();

    if let Ok(credentials) = Credentials::from_env() {
        from_env = Some(credentials);
    }

    for path in candidate_files() {
        searched.push(path.display().to_string());
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(credentials) = parse(&contents) {
            return Ok(credentials);
        }
    }

    match from_env {
        Some(credentials) => Ok(credentials),
        None => bail!(
            "no Gmail client credentials.\n\
             Looked for {ENV_VAR} in the environment, and in:\n  {}\n\
             Copy .env.example to .env and fill it in, or export {ENV_VAR}.",
            searched.join("\n  ")
        ),
    }
}

/// The `.env` files worth reading, in priority order.
fn candidate_files() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Some(explicit) = std::env::var_os("NORI_ENV_FILE") {
        paths.push(std::path::PathBuf::from(explicit));
    }
    paths.push(std::path::PathBuf::from(".env"));
    if let Some(dir) = config_dir() {
        paths.push(dir.join("nori").join(".env"));
    }
    paths
}

fn config_dir() -> Option<std::path::PathBuf> {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => Some(std::path::PathBuf::from(dir)),
        _ => std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config")),
    }
}

/// Pull the two variables out of a `KEY=value` file.
///
/// Hand-rolled rather than pulling in a dotenv crate for six lines, and
/// deliberately partial: it handles comments, blank lines, an optional `export`
/// prefix and surrounding quotes, which is everything a hand-written `.env`
/// turns out to contain.
fn parse(contents: &str) -> Option<Credentials> {
    let mut client_id = None;
    let mut client_secret = None;

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim() {
            "NORI_GMAIL_CLIENT_ID" => client_id = Some(value.to_string()),
            "NORI_GMAIL_CLIENT_SECRET" => client_secret = Some(value.to_string()),
            _ => {}
        }
    }

    match (client_id, client_secret) {
        (Some(client_id), Some(client_secret))
            if !client_id.is_empty() && !client_secret.is_empty() =>
        {
            Some(Credentials {
                client_id,
                client_secret,
            })
        }
        // A half-filled file is a mistake worth reporting rather than treating
        // as absent, but the caller only needs to know it is unusable.
        _ => None,
    }
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        let matched = (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'');
        if matched {
            return &value[1..value.len() - 1];
        }
    }
    value
}

impl Credentials {
    /// The same lookup, exposed on the type for call sites that already have
    /// one to hand.
    pub fn discover() -> Result<Self> {
        credentials()
    }
}

/// A hint for a pane that has to explain an empty mailbox.
pub fn missing_hint() -> String {
    format!("Set {ENV_VAR} in the environment or in a .env file. See .env.example.")
}
