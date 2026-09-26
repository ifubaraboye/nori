//! The installed-app authorization code flow.
//!
//! Google does not support incremental authorization for installed apps, and
//! there is no embedded webview worth using here: the consent screen is a
//! Google account login, and putting that in a webview is how clients get
//! their credentials stolen. So Nori opens the system browser, listens on a
//! loopback port for the redirect, and closes the tab again itself.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::http;
use crate::pkce::Pkce;
use crate::token::Token;

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// Google's "access_type=offline" is what produces a refresh token at all.
/// Without it the exchange returns an access token that dies in an hour and
/// the user has to consent again.
const SCOPES: &str = "https://www.googleapis.com/auth/gmail.modify \
https://www.googleapis.com/auth/userinfo.email";

/// A loopback listener, already bound, so the redirect URI is known before
/// the browser is ever launched.
///
/// Port 0 lets the OS pick a free port. That is not a workaround: Google's
/// loopback redirect matching ignores the port, so a range of them is correct
/// and picking one fixed port would just invite collisions.
pub struct LoopbackRedirect {
    listener: TcpListener,
    pub redirect_uri: String,
}

impl LoopbackRedirect {
    pub fn bind() -> Result<Self> {
        let listener = TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
            .context("binding a loopback port for the OAuth redirect")?;
        let port = listener.local_addr()?.port();
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}"),
        })
    }

    /// Wait for the browser to come back with a code.
    ///
    /// Gives up after [`CALLBACK_TIMEOUT`]. Without a deadline a sign-in that
    /// the user abandons leaves a thread blocked forever holding the listener,
    /// and the app has no way to say so.
    pub fn await_callback(self, expected_state: &str) -> Result<Code> {
        self.listener
            .set_nonblocking(true)
            .context("clearing the listener's blocking mode")?;
        let deadline = std::time::Instant::now() + CALLBACK_TIMEOUT;

        loop {
            if std::time::Instant::now() > deadline {
                bail!("timed out after {CALLBACK_TIMEOUT:?} waiting for the browser to return");
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .context("restoring blocking mode for the callback")?;
                    return self.handle(stream, expected_state);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error).context("waiting for the OAuth redirect"),
            }
        }
    }

    fn handle(&self, mut stream: std::net::TcpStream, expected_state: &str) -> Result<Code> {
        // The request line carries the whole query string, and only one request
        // is served, so the connection is closed immediately afterwards.
        let mut request_line = String::new();
        BufReader::new(&stream)
            .read_line(&mut request_line)
            .context("reading the OAuth callback request")?;

        let query = request_line
            .split_whitespace()
            .nth(1)
            .and_then(|target| target.split_once('?'))
            .map(|(_, query)| query)
            .unwrap_or_default();

        let params: std::collections::HashMap<String, String> =
            url_decode_pairs(query).context("parsing the OAuth callback query")?;

        let outcome = self.interpret(&params, expected_state);
        match &outcome {
            Ok(_) => {
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: text/html; charset=utf-8\r\n\
                     Connection: close\r\n\r\n\
                     <!doctype html><meta charset=utf-8>\
                     <title>Nori</title>\
                     <body style=\"font:16px system-ui;padding:3rem;max-width:32rem\">\
                     <h1>Nori is connected</h1>\
                     <p>You can close this tab and go back to Nori.</p>"
                )?;
            }
            Err(_) => {
                // The detail stays out of the page: this text is rendered in
                // the user's browser, and a token or a verifier must never
                // reach it.
                write!(
                    stream,
                    "HTTP/1.1 400 Bad Request\r\n\
                     Content-Type: text/html; charset=utf-8\r\n\
                     Connection: close\r\n\r\n\
                     <!doctype html><meta charset=utf-8>\
                     <title>Nori</title>\
                     <body style=\"font:16px system-ui;padding:3rem;max-width:32rem\">\
                     <h1>Sign-in did not complete</h1>\
                     <p>You can close this tab. Nori will report the problem.</p>"
                )?;
            }
        }
        let _ = stream.flush();
        outcome
    }

    fn interpret(
        &self,
        params: &std::collections::HashMap<String, String>,
        expected_state: &str,
    ) -> Result<Code> {
        if let Some(error) = params.get("error") {
            let description = params
                .get("error_description")
                .cloned()
                .unwrap_or_else(|| "no description".to_string());
            bail!("the user or Google refused authorization: {error} ({description})");
        }
        let state = params
            .get("state")
            .ok_or_else(|| anyhow!("the callback carried no state"))?;
        if state != expected_state {
            bail!("the callback's state did not match this sign-in attempt");
        }
        let code = params
            .get("code")
            .ok_or_else(|| anyhow!("the callback carried no authorization code"))?;
        Ok(Code(code.clone()))
    }
}

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// The authorization code, redeemed exactly once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code(String);

impl Code {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
}

/// The credentials this build needs. Read from the environment, never compiled
/// in: a desktop app's client secret is extractable from the binary anyway,
/// but committing it would also leak it to everyone else who can read the
/// repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
}

impl Credentials {
    pub fn from_env() -> Result<Self> {
        let client_id =
            std::env::var("NORI_GMAIL_CLIENT_ID").context("NORI_GMAIL_CLIENT_ID is not set")?;
        let client_secret = std::env::var("NORI_GMAIL_CLIENT_SECRET")
            .context("NORI_GMAIL_CLIENT_SECRET is not set")?;
        Ok(Self {
            client_id,
            client_secret,
        })
    }
}

/// Everything the browser needs in order to start a sign-in.
///
/// The PKCE pair and the listener are private with accessors rather than
/// public fields, because the two halves have to belong to the *same* attempt.
/// A public `pkce` invites `begin()` twice, then redeeming one code with the
/// other's verifier, which fails as an inscrutable server error.
pub struct AuthorizationRequest {
    /// Hand this to the system browser.
    pub url: String,
    /// Needed to redeem the code, and readable after [`Self::await_callback`]
    /// has consumed the listener.
    pub redirect_uri: String,
    pkce: Pkce,
    redirect: LoopbackRedirect,
}

impl AuthorizationRequest {
    /// The verifier, needed only to redeem the code.
    pub fn verifier(&self) -> &str {
        &self.pkce.verifier
    }

    /// Block until the browser comes back, checking the callback against this
    /// attempt's own state.
    pub fn await_callback(self) -> Result<Code> {
        self.redirect.await_callback(&self.pkce.state)
    }
}

pub fn begin(credentials: &Credentials) -> Result<AuthorizationRequest> {
    let pkce = Pkce::generate();
    let redirect = LoopbackRedirect::bind()?;

    let url = format!(
        "{AUTH_ENDPOINT}\
         ?client_id={}\
         &redirect_uri={}\
         &response_type=code\
         &scope={}\
         &access_type=offline\
         &prompt=consent\
         &code_challenge={}\
         &code_challenge_method=S256\
         &state={}",
        urlencode(&credentials.client_id),
        urlencode(&redirect.redirect_uri),
        urlencode(SCOPES),
        urlencode(&pkce.challenge),
        urlencode(&pkce.state),
    );

    Ok(AuthorizationRequest {
        url,
        redirect_uri: redirect.redirect_uri.clone(),
        pkce,
        redirect,
    })
}

/// Trade an authorization code for tokens.
pub fn exchange(
    agent: &ureq::Agent,
    credentials: &Credentials,
    redirect_uri: &str,
    code: &Code,
    verifier: &str,
) -> Result<Token> {
    let form = form_body(&[
        ("code", code.as_str()),
        ("client_id", credentials.client_id.as_str()),
        ("client_secret", credentials.client_secret.as_str()),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
        ("code_verifier", verifier),
    ]);

    let parsed: TokenResponse = post_form(agent, &form)?;

    Ok(Token {
        access_token: parsed.access_token,
        // A re-consent can return no refresh token, which means "keep using
        // the one you have" rather than "there is none". Dropping the stored
        // one here would sign the user out every time they refreshed a
        // session in the browser.
        refresh_token: parsed.refresh_token,
        expires_at: parsed
            .expires_in
            .map(|seconds| crate::token::unix_now().saturating_add(seconds)),
        scope: parsed.scope,
        token_type: parsed.token_type,
    })
}

/// Renew an access token. The previous refresh token normally stays valid.
pub fn refresh(
    agent: &ureq::Agent,
    credentials: &Credentials,
    refresh_token: &str,
) -> Result<Token> {
    let form = form_body(&[
        ("client_id", credentials.client_id.as_str()),
        ("client_secret", credentials.client_secret.as_str()),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ]);

    let parsed: TokenResponse = post_form(agent, &form)?;

    Ok(Token {
        access_token: parsed.access_token,
        // Refresh responses omit the refresh token entirely, so the caller's
        // copy is carried over. Losing it here would make the second renewal
        // impossible.
        refresh_token: Some(refresh_token.to_string()),
        expires_at: parsed
            .expires_in
            .map(|seconds| crate::token::unix_now().saturating_add(seconds)),
        scope: parsed.scope,
        token_type: parsed.token_type,
    })
}

fn form_body(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn post_form<T: for<'de> Deserialize<'de>>(agent: &ureq::Agent, form: &str) -> Result<T> {
    let sent = agent
        .post(TOKEN_ENDPOINT)
        .header("content-type", "application/x-www-form-urlencoded")
        .send(form.as_bytes());
    let response = sent.map_err(|error| anyhow!("{TOKEN_ENDPOINT} failed: {error}"))?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    let raw = body
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .with_context(|| format!("reading the token response from {TOKEN_ENDPOINT}"))?;

    let text = match http::text_with_status(raw.clone(), status, TOKEN_ENDPOINT) {
        Ok(text) => text,
        // `invalid_grant` is the expected end of a Testing-mode app's life, not
        // a malfunction, and it is the one failure the user can act on. Google
        // returns it as a 400 whose body names the reason, so the status alone
        // throws away the only part worth showing.
        Err(http::Error::Api { detail, .. }) if detail.contains("invalid_grant") => {
            bail!("the grant is no longer valid; the user must sign in again ({detail})")
        }
        Err(error) => return Err(error.into()),
    };
    serde_json::from_str(&text).with_context(|| format!("decoding {text}"))
}

/// Percent-encode for a query string or form body.
///
/// Google's endpoints are strict about `+` and the scope separator, and a
/// scope string sent unencoded turns into a single invalid scope. Written out
/// rather than pulled in as a dependency for one function.
pub fn urlencode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            // Space has to be `%20`, not `+`: the endpoints are form-encoded,
            // where `+` is a space, and a scope's spaces are separators rather
            // than separators-after-decoding.
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn url_decode_pairs(query: &str) -> Result<std::collections::HashMap<String, String>> {
    let mut params = std::collections::HashMap::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        params.insert(urldecode(key)?, urldecode(value)?);
    }
    Ok(params)
}

fn urldecode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = value
                    .get(index + 1..index + 3)
                    .ok_or_else(|| anyhow!("a percent escape in the callback was cut short"))?;
                out.push(
                    u8::from_str_radix(hex, 16)
                        .with_context(|| format!("{hex} is not a percent escape"))?,
                );
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).context("the callback carried invalid UTF-8")
}

/// Run `body` on a thread, handing the result back over a channel.
///
/// The loopback listener blocks, and it must not block the UI thread, so the
/// browser wait and the code exchange both happen off-thread.
pub fn on_background<T: Send + 'static>(
    body: impl FnOnce() -> Result<T> + Send + 'static,
) -> mpsc::Receiver<Result<T>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(body());
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_callback_carrying_the_right_state_yields_its_code() {
        let redirect = LoopbackRedirect::bind().unwrap();
        let code = redirect
            .interpret(&params(&[("code", "abc"), ("state", "xyz")]), "xyz")
            .unwrap();
        assert_eq!(code.as_str(), "abc");
    }

    #[test]
    fn a_mismatched_state_is_refused() {
        let redirect = LoopbackRedirect::bind().unwrap();
        let error = redirect
            .interpret(&params(&[("code", "abc"), ("state", "other")]), "xyz")
            .unwrap_err();
        assert!(error.to_string().contains("state"));
    }

    #[test]
    fn a_refusal_from_google_is_surfaced_not_swallowed() {
        let redirect = LoopbackRedirect::bind().unwrap();
        let error = redirect
            .interpret(
                &params(&[
                    ("error", "access_denied"),
                    ("error_description", "The user said no"),
                ]),
                "xyz",
            )
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("access_denied"), "{message}");
        assert!(message.contains("The user said no"), "{message}");
    }

    #[test]
    fn a_callback_with_no_code_is_refused() {
        let redirect = LoopbackRedirect::bind().unwrap();
        assert!(
            redirect
                .interpret(&params(&[("state", "xyz")]), "xyz")
                .is_err()
        );
    }

    #[test]
    fn the_loopback_port_is_chosen_by_the_os_and_stays_local() {
        let redirect = LoopbackRedirect::bind().unwrap();
        assert!(redirect.redirect_uri.starts_with("http://127.0.0.1:"));
        assert_ne!(redirect.redirect_uri, "http://127.0.0.1:0");
    }

    /// The listener is bound before the browser is opened, because the port
    /// has to appear in the redirect URI that the browser is sent to.
    #[test]
    fn the_redirect_is_bound_before_the_url_is_built() {
        let credentials = Credentials {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
        };
        let request = begin(&credentials).unwrap();
        let port = request
            .redirect_uri
            .rsplit(':')
            .next()
            .and_then(|port| port.parse::<u16>().ok())
            .expect("the redirect URI ends in a port");
        assert!(port > 0, "a redirect of port 0 would never be reachable");
        // Percent-encoded, because the whole thing is a query parameter value.
        // The unencoded form would be read as extra query separators.
        assert!(
            request.url.contains(&urlencode(&request.redirect_uri)),
            "the URL must carry the very URI the listener is bound to"
        );
        assert!(request.url.contains(&request.pkce.challenge));
    }

    #[test]
    fn the_authorization_url_carries_everything_pkce_needs() {
        let credentials = Credentials {
            client_id: "id.apps.googleusercontent.com".to_string(),
            client_secret: "secret".to_string(),
        };
        let request = begin(&credentials).unwrap();
        for required in [
            "code_challenge_method=S256",
            "access_type=offline",
            "response_type=code",
            "prompt=consent",
            "code_challenge=",
            "client_id=id.apps.googleusercontent.com",
        ] {
            assert!(
                request.url.contains(required),
                "missing {required} in {}",
                request.url
            );
        }
        // The scope separator must survive encoding as `%20`, or the whole
        // scope string arrives as one invalid scope.
        assert!(
            request.url.contains("gmail.modify%20https"),
            "scopes must be space-separated after decoding: {}",
            request.url
        );
    }

    #[test]
    fn a_scoped_request_never_leaks_the_secret() {
        let credentials = Credentials {
            client_id: "id".to_string(),
            client_secret: "GOCSPX-super-secret".to_string(),
        };
        let request = begin(&credentials).unwrap();
        assert!(
            !request.url.contains("GOCSPX-super-secret"),
            "the secret belongs in the token exchange only, never in a URL that              lands in browser history"
        );
    }

    #[test]
    fn the_callback_query_round_trips() {
        let decoded = url_decode_pairs("code=a%2Fb&state=xyz&error_description=not+ok").unwrap();
        assert_eq!(decoded["code"], "a/b");
        assert_eq!(decoded["error_description"], "not ok");
    }

    #[test]
    fn a_malformed_callback_query_is_an_error_not_a_panic() {
        assert!(url_decode_pairs("code=%zz").is_err());
        assert!(url_decode_pairs("code=%4").is_err());
    }
}
