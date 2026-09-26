//! Tests that touch real sockets.
//!
//! The loopback tests are the ones that matter: the redirect listener is
//! security-relevant, and a version that compiles but mishandles a callback is
//! worse than no sign-in at all. They bind `127.0.0.1` on an OS-chosen port,
//! which needs no network and no browser.

use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use nori_gmail::oauth;

/// Pull a query parameter out of a URL, without a URL parser dependency.
fn param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                out.push(u8::from_str_radix(&value[index + 1..index + 3], 16).unwrap_or(b'?'));
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn credentials() -> oauth::Credentials {
    oauth::Credentials {
        client_id: "test.apps.googleusercontent.com".to_string(),
        client_secret: "test-secret".to_string(),
    }
}

/// Ask the real listener for a code the way a browser would, and read the page
/// it answers with.
fn visit(url: &str, query: &str) -> (u16, String) {
    let authority = url.trim_start_matches("http://");
    let mut stream = TcpStream::connect(authority).expect("connecting to the loopback listener");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("bounding the read");
    let request =
        format!("GET /?{query} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("sending the callback");
    stream.flush().expect("flushing the callback");

    let mut response = String::new();
    BufReader::new(&stream)
        .read_to_string(&mut response)
        .expect("reading the callback response");
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    (status, response)
}

#[test]
fn a_browser_returning_a_code_ends_the_sign_in() {
    let request = oauth::begin(&credentials()).unwrap();
    let state = param(&request.url, "state").expect("the URL carries a state");
    let state_param = state.clone();
    let verifier = request.verifier().to_owned();
    assert!(!state_param.is_empty() && !verifier.is_empty());
    assert!(!param(&request.url, "code_challenge").unwrap().is_empty());

    // The listener blocks, so the visit has to be on another thread.
    let redirect_uri = request.redirect_uri.clone();
    let waiter = std::thread::spawn(move || request.await_callback());

    let (status, body) = visit(&redirect_uri, &format!("code=4/0Aean&state={state_param}"));
    let code = waiter
        .join()
        .expect("the listener thread did not panic")
        .expect("a valid callback");

    assert_eq!(status, 200);
    assert_eq!(code.as_str(), "4/0Aean");
    assert!(
        body.contains("Nori is connected"),
        "the user needs to be told the sign-in worked: {body}"
    );
    assert!(
        !body.contains(&verifier),
        "the PKCE verifier must never be rendered into a browser page"
    );
    assert!(!body.contains(&state_param), "nor the state");
}

#[test]
fn a_callback_for_some_other_sign_in_is_refused() {
    let request = oauth::begin(&credentials()).unwrap();
    let redirect_uri = request.redirect_uri.clone();
    let waiter = std::thread::spawn(move || request.await_callback());

    // The state belongs to a different attempt, exactly as it would if a stale
    // browser tab or another app's flow hit this port.
    let (status, body) = visit(&redirect_uri, "code=4/0Aean&state=someone-elses-attempt");
    let result = waiter.join().expect("the listener thread did not panic");

    assert_eq!(status, 400);
    assert!(result.is_err(), "a mismatched state must not yield a code");
    assert!(
        !body.contains("someone-elses-attempt"),
        "no echoing the state back"
    );
    assert!(!body.contains("4/0Aean"), "no echoing the code back");
}

#[test]
fn a_user_who_declined_is_told_so_rather_than_shown_success() {
    let request = oauth::begin(&credentials()).unwrap();
    let state = param(&request.url, "state").unwrap();
    let redirect_uri = request.redirect_uri.clone();
    let waiter = std::thread::spawn(move || request.await_callback());

    let (status, body) = visit(
        &redirect_uri,
        &format!("error=access_denied&error_description=The+user+said+no&state={state}"),
    );
    let result = waiter.join().expect("the listener thread did not panic");

    assert_eq!(status, 400, "a refusal must not be dressed up as success");
    assert!(!body.contains("Nori is connected"));
    let error = result
        .expect_err("a refusal must not yield a code")
        .to_string();
    assert!(error.contains("access_denied"), "{error}");
    assert!(error.contains("The user said no"), "{error}");
}

#[test]
fn a_second_visit_gets_no_code() {
    // The listener serves exactly one callback and then gives up its listener,
    // so a replayed or duplicated redirect cannot redeem a second code.
    let request = oauth::begin(&credentials()).unwrap();
    let state = param(&request.url, "state").unwrap();
    let redirect_uri = request.redirect_uri.clone();
    let waiter = std::thread::spawn(move || request.await_callback());

    let (first, _) = visit(&redirect_uri, &format!("code=first&state={state}"));
    assert_eq!(first, 200);
    assert!(waiter.join().unwrap().is_ok());

    // Nothing is listening any more, so a replay cannot even connect.
    assert!(
        TcpStream::connect(redirect_uri.trim_start_matches("http://")).is_err(),
        "the listener must not survive the callback that consumed it"
    );
}

#[test]
fn the_pkce_challenge_in_the_url_matches_its_own_verifier() {
    // Guards the pairing end to end: a challenge computed from a different
    // verifier produces a 400 from Google that is very hard to read.
    let request = oauth::begin(&credentials()).unwrap();
    let challenge = param(&request.url, "code_challenge").unwrap();
    assert_eq!(
        challenge,
        nori_gmail::pkce::challenge_for(request.verifier())
    );
    assert!(param(&request.url, "code_challenge_method").as_deref() == Some("S256"));
    assert_eq!(
        param(&request.url, "access_type").as_deref(),
        Some("offline")
    );
    assert_eq!(
        param(&request.url, "response_type").as_deref(),
        Some("code")
    );
}

/// Reach Google's real token endpoint with credentials that cannot work.
///
/// This is the one test that needs the network, and it is worth having: it
/// proves the rustls build, the certificate chain and the error-body parsing
/// all work against the live endpoint, without needing anyone's account.
#[test]
#[ignore = "needs network; run with --ignored"]
fn the_live_token_endpoint_rejects_bad_credentials_with_a_readable_reason() {
    let agent = nori_gmail::agent();
    let body = "code=nonsense&client_id=test.apps.googleusercontent.com&client_secret=nope\
                &redirect_uri=http://127.0.0.1:1&grant_type=authorization_code\
                &code_verifier=x";
    let sent = agent
        .post(oauth::TOKEN_ENDPOINT)
        .header("content-type", "application/x-www-form-urlencoded")
        .send(body.as_bytes());

    let error = nori_gmail::http::read_body(sent, oauth::TOKEN_ENDPOINT)
        .expect_err("nonsense credentials must not be accepted");

    // The load-bearing assertion is that this is *not* `Unreachable`. A
    // transport failure here would mean the rustls build, the certificate chain
    // or the proxy is broken, and every later test would be meaningless.
    assert!(
        !matches!(error, nori_gmail::Error::Unreachable { .. }),
        "the live endpoint was not reached, so nothing else here proves anything: {error}"
    );

    // And that the reason survives, rather than a bare status. Google answers
    // a bad client with 401, which is a refusal Nori can explain, not a
    // transport fault.
    assert!(
        !error.is_transient(),
        "a refusal from a real server must not look like a blip worth retrying: {error}"
    );
    println!("live endpoint replied: {error}");
}
