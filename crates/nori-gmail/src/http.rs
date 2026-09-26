//! The one place ureq is configured, and the error type everything shares.
//!
//! The errors matter more than the plumbing. A dead access token, an aged-out
//! history id, and an unplugged network all arrive as "the request failed",
//! and the three need opposite responses from the caller: re-authenticate,
//! resync, retry later. Collapsing them into a string would push that
//! distinction back into every call site, so it is made once, here.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// The token was rejected. The only fix is the user signing in again.
    Unauthorized,
    /// Gmail's per-user quota is exhausted. Nothing is wrong and nobody needs
    /// to sign in again — the answer is to slow down and try later.
    ///
    /// Its own case because it is easy to confuse with [`Self::Unauthorized`]:
    /// both arrive as a 403, and treating a rate limit as a dead token sends
    /// the user through a pointless re-consent that cannot help.
    RateLimited { detail: String },
    /// Gmail no longer knows the stored `historyId`, which happens after about
    /// a week. It means a full resync, not a failure.
    HistoryExpired,
    /// Nothing could be reached. Worth retrying.
    Unreachable { endpoint: String, reason: String },
    /// The server answered, and the answer was a refusal.
    Api {
        endpoint: String,
        status: u16,
        detail: String,
    },
    /// A response that should have been understood was not.
    Malformed { endpoint: String, reason: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Phrased for a settings pane, where this is the whole story.
            Self::Unauthorized => write!(f, "the grant is no longer valid, sign in again"),
            Self::RateLimited { detail } => {
                write!(f, "Gmail's rate limit was reached ({detail})")
            }
            Self::HistoryExpired => write!(f, "the stored history id has aged out"),
            Self::Unreachable { endpoint, reason } => {
                write!(f, "{endpoint} could not be reached: {reason}")
            }
            Self::Api {
                endpoint,
                status,
                detail,
            } => write!(f, "{endpoint} returned {status}: {detail}"),
            Self::Malformed { endpoint, reason } => {
                write!(f, "could not read a response from {endpoint}: {reason}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// Whether retrying without the user could plausibly work.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Unauthorized | Self::HistoryExpired => false,
            // Worth retrying, but only after waiting: retrying immediately is
            // what turns one rate limit into a sustained one.
            Self::RateLimited { .. } => true,
            Self::Unreachable { .. } => true,
            // A 5xx is worth another go; a 4xx will fail the same way again.
            Self::Api { status, .. } => *status >= 500,
            Self::Malformed { .. } => false,
        }
    }
}

/// A blocking agent with one deliberate setting changed.
///
/// `http_status_as_error` is on by default, which turns a 400 into a bare
/// `Err` and throws the body away. Gmail puts the only useful part of a
/// refusal in that body — `invalid_grant` versus a typo'd scope versus a quota
/// error are three different problems — so it is turned off and every status
/// is interpreted here instead.
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into()
}

/// The largest response Nori will read. A Gmail page of message metadata is
/// kilobytes, so this is generous by orders of magnitude while still bounding
/// what a hostile or broken endpoint can allocate.
const MAX_BODY: u64 = 32 * 1024 * 1024;

/// Read a response body, mapping a transport failure to [`Error::Unreachable`].
///
/// The limit is not optional: ureq's unbounded `read_to_string` will happily
/// grow a `String` until memory runs out.
pub fn read_body(
    response: Result<http::Response<ureq::Body>, ureq::Error>,
    endpoint: &str,
) -> Result<String, Error> {
    let response = response.map_err(|error| transport(error, endpoint))?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    let text = body
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .map_err(|error| transport(error, endpoint))?;
    text_with_status(text, status, endpoint)
}

/// Pair a body with its status, refusing anything that is not a success.
///
/// Google's own error shape is unwrapped where it matches, because
/// `{"error": {"message": ...}}` is otherwise several layers of guessing.
pub fn text_with_status(body: String, status: u16, endpoint: &str) -> Result<String, Error> {
    if (200..300).contains(&status) {
        return Ok(body);
    }
    if status == 401 {
        return Err(Error::Unauthorized);
    }
    // 403 and 429 both cover "no" for different reasons, and the reason decides
    // what happens next. Gmail spends 403 on rate limiting as well as on
    // permissions, so the body has to be consulted rather than the status
    // alone. Getting this wrong is how a rate limit turns into a re-consent
    // loop that can never succeed.
    if status == 403 || status == 429 {
        let detail = detail_of(&body);
        if is_rate_limit(&detail) {
            return Err(Error::RateLimited { detail });
        }
        if status == 429 {
            return Err(Error::RateLimited { detail });
        }
    }
    Err(Error::Api {
        endpoint: endpoint.to_string(),
        status,
        detail: detail_of(&body),
    })
}

/// Pull the human-readable part out of a Gmail error body.
pub fn detail_of(body: &str) -> String {
    #[derive(serde::Deserialize)]
    struct Wrapped {
        error: Body,
    }
    #[derive(serde::Deserialize)]
    struct Body {
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        description: Option<String>,
    }

    // The token endpoint wraps the code in `error`/`error_description` as
    // siblings, while the API wraps a `message` in an `error` object. Both
    // shapes have to survive, because `invalid_grant` only ever arrives in the
    // first and is the one case the UI must name.
    #[derive(serde::Deserialize)]
    struct Flat {
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        error_description: Option<String>,
    }
    if let Ok(Flat {
        error: Some(error),
        error_description,
    }) = serde_json::from_str::<Flat>(body)
    {
        return match error_description {
            Some(description) => format!("{error}: {description}"),
            None => error,
        };
    }
    match serde_json::from_str::<Wrapped>(body) {
        Ok(wrapped) => {
            let Wrapped { error } = wrapped;
            error
                .message
                .or(error.description)
                .unwrap_or_else(|| body.to_string())
        }
        Err(_) => body.to_string(),
    }
}

/// Whether a refusal is the rate limiter rather than a permission problem.
fn is_rate_limit(detail: &str) -> bool {
    let detail = detail.to_lowercase();
    detail.contains("quota exceeded")
        || detail.contains("ratelimitexceeded")
        || detail.contains("rate limit")
        || detail.contains("quota metric")
}

fn transport(error: ureq::Error, endpoint: &str) -> Error {
    Error::Unreachable {
        endpoint: endpoint.to_string(),
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_token_is_named_as_such_rather_than_as_a_status() {
        let error = text_with_status(
            r#"{"error":{"message":"Invalid Credentials"}}"#.into(),
            401,
            "https://example.test",
        )
        .unwrap_err();
        assert!(matches!(error, Error::Unauthorized));
        assert!(!error.is_transient(), "retrying a dead token cannot work");
    }

    /// A 403 that means "slow down" must not read as "sign in again". Gmail
    /// sends both under 403, and conflating them sends the user round a
    /// re-consent that cannot possibly help.
    #[test]
    fn a_quota_refusal_is_not_mistaken_for_a_dead_token() {
        let body = r#"{"error":{"code":403,"message":"Quota exceeded for quota metric
            'Total Query Cost' and limit 'Units per minute per user' of service
            'gmail.googleapis.com'","errors":[{"reason":"rateLimitExceeded"}]}}"#;
        let error = text_with_status(body.into(), 403, "https://example.test").unwrap_err();
        assert!(
            matches!(error, Error::RateLimited { .. }),
            "a rate limit must be its own case, got {error}"
        );
        assert!(error.is_transient(), "a rate limit clears on its own");
    }

    #[test]
    fn a_permission_refusal_stays_a_permission_refusal() {
        let body = r#"{"error":{"code":403,"message":"Insufficient Permission"}}"#;
        let error = text_with_status(body.into(), 403, "https://example.test").unwrap_err();
        assert!(
            !matches!(error, Error::RateLimited { .. }),
            "a permission problem is not fixed by waiting"
        );
    }

    #[test]
    fn a_429_is_always_a_rate_limit() {
        let error = text_with_status("slow down".into(), 429, "https://example.test").unwrap_err();
        assert!(matches!(error, Error::RateLimited { .. }));
    }

    #[test]
    fn only_server_faults_are_worth_retrying() {
        let server = text_with_status("boom".into(), 503, "u").unwrap_err();
        assert!(server.is_transient());
        let client = text_with_status("nope".into(), 400, "u").unwrap_err();
        assert!(
            !client.is_transient(),
            "a 400 will fail identically on retry"
        );
        let network = Error::Unreachable {
            endpoint: "u".into(),
            reason: "no route".into(),
        };
        assert!(network.is_transient());
    }

    #[test]
    fn the_api_error_shape_is_unwrapped() {
        let body = r#"{"error":{"code":404,"message":"Requested entity was not found."}}"#;
        assert_eq!(detail_of(body), "Requested entity was not found.");
    }

    #[test]
    fn the_token_error_shape_is_unwrapped_too() {
        let body =
            r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#;
        let detail = detail_of(body);
        assert!(detail.contains("invalid_grant"), "{detail}");
        assert!(detail.contains("expired or revoked"), "{detail}");
    }

    #[test]
    fn a_body_that_is_not_json_is_passed_through_verbatim() {
        assert_eq!(detail_of("<html>502</html>"), "<html>502</html>");
    }

    #[test]
    fn a_success_passes_its_body_through_untouched() {
        assert_eq!(text_with_status("ok".into(), 200, "u").unwrap(), "ok");
    }
}
