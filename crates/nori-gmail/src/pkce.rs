//! PKCE (RFC 7636) for the installed-app authorization code flow.
//!
//! A desktop app cannot keep a client secret — it ships in the binary, so
//! anyone can read it. PKCE replaces that secret with a per-attempt random
//! value that never leaves the machine until the code comes back, which is why
//! Google's own native-app guidance requires it.

use base64::Engine as _;
use sha2::{Digest, Sha256};

/// A single authorization attempt's PKCE pair, plus the `state` that ties the
/// eventual callback to this attempt.
pub struct Pkce {
    /// Held only for the length of one sign-in. It is needed to redeem the
    /// code and is never written to disk.
    pub verifier: String,
    /// Sent to the authorization endpoint so Google can check it later.
    pub challenge: String,
    /// Echoed back on the callback and compared, so a callback from some other
    /// flow cannot be fed into this one.
    pub state: String,
}

impl Pkce {
    pub fn generate() -> Self {
        let verifier = random_base64url(32);
        Self {
            challenge: challenge_for(&verifier),
            state: random_base64url(16),
            verifier,
        }
    }

    /// Whether a callback belongs to this attempt.
    ///
    /// Constant-time is overkill for a value that only ever crosses the
    /// loopback on the same machine, but it costs nothing to not leak a
    /// timing side channel in a security check.
    pub fn matches_state(&self, returned: &str) -> bool {
        if returned.len() != self.state.len() {
            return false;
        }
        let difference = returned
            .bytes()
            .zip(self.state.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b));
        difference == 0
    }
}

/// The S256 challenge for a verifier: BASE64URL(SHA256(verifier)), unpadded.
pub fn challenge_for(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn random_base64url(bytes: usize) -> String {
    // 3 bytes encode to 4 base64 characters, so this is the smallest input
    // that yields at least `bytes` characters of entropy.
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).expect("the OS CSPRNG is required for PKCE");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example from RFC 7636 appendix B. If this drifts, every
    /// sign-in breaks in a way that is very hard to read from the failure.
    #[test]
    fn the_s256_challenge_matches_the_rfc_example() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_for(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_challenge_is_derived_from_its_own_verifier() {
        let pkce = Pkce::generate();
        assert_eq!(pkce.challenge, challenge_for(&pkce.verifier));
        assert!(!pkce.challenge.contains('='), "PKCE is unpadded base64url");
        assert!(!pkce.challenge.contains('+') && !pkce.challenge.contains('/'));
    }

    #[test]
    fn two_attempts_never_share_a_verifier() {
        let first = Pkce::generate();
        let second = Pkce::generate();
        assert_ne!(first.verifier, second.verifier);
        assert_ne!(first.state, second.state);
    }

    #[test]
    fn state_matching_rejects_anything_that_is_not_ours() {
        let pkce = Pkce::generate();
        assert!(pkce.matches_state(&pkce.state));
        assert!(!pkce.matches_state(""));
        assert!(
            !pkce.matches_state(&format!("{}x", pkce.state)),
            "a one-character change must not pass"
        );
        assert!(
            !pkce.matches_state(&pkce.state[..pkce.state.len() - 1]),
            "a truncation must not pass"
        );
    }
}
