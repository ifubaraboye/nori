//! One-shot sign-in, for proving the OAuth flow outside the app.
//!
//! Run it, click through the browser, and it prints what the account is. It
//! writes the token where Nori will look for it, so a successful run means
//! Nori only has to find the file, not re-ask Google for permission.
//!
//! ```sh
//! export NORI_GMAIL_CLIENT_ID='...apps.googleusercontent.com'
//! export NORI_GMAIL_CLIENT_SECRET='GOCSPX-...'
//! cargo run -p nori-gmail --example signin -- you@gmail.com
//! ```

use nori_gmail::TokenStore as _;

fn main() -> anyhow::Result<()> {
    let account = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "me@gmail.com".to_string());

    let credentials = nori_gmail::oauth::Credentials::from_env()?;
    let store = nori_gmail::FileTokenStore::with_account(&account)?;
    let agent = nori_gmail::agent();

    println!("Signing in as {account}");

    let request = nori_gmail::oauth::begin(&credentials)?;
    let (redirect_uri, verifier) = (request.redirect_uri.clone(), request.verifier().to_owned());

    println!(
        "\nIf a browser did not open, paste this into one:\n\n  {}\n",
        request.url
    );
    println!("Waiting for the redirect on {redirect_uri} ...");

    let code = request.await_callback()?;
    println!("Got a code. Exchanging ...");

    let token = nori_gmail::oauth::exchange(&agent, &credentials, &redirect_uri, &code, &verifier)?;
    println!("Token acquired.");

    // Prove it works rather than assuming it does.
    let profile = nori_gmail::gmail::profile(&agent, &token)?;
    println!("  address:  {}", profile.email);
    println!("  messages: {}", profile.total);
    println!("  history:  {}", profile.history_id.unwrap_or_default());

    let labels = nori_gmail::gmail::labels(&agent, &token)?;
    let user_labels: Vec<_> = labels.iter().filter(|label| !label.is_system()).collect();
    println!(
        "  labels:   {} system, {} user",
        labels.len() - user_labels.len(),
        user_labels.len()
    );
    for label in user_labels.iter().take(10) {
        println!("             {}", label.name);
    }

    let page = nori_gmail::gmail::list_messages(&agent, &token, "in:inbox", None)?;
    println!("  inbox:    {} on the first page", page.messages.len());

    store.save(&token)?;

    // The app has no way to guess which of the token files in the config
    // directory is the account, so signing in from the command line has to
    // leave the same pointer the in-app flow does. Without this the token is
    // on disk and the app still opens on sample mail.
    let pointer = nori_gmail::LastAccount::with_config_dir()?;
    pointer.save(&profile.email)?;

    println!("\nSaved to {}", store.path().display());
    println!(
        "Recorded as the current account in {}",
        pointer.path().display()
    );
    println!("Nori can now pick this up without another consent screen.");
    Ok(())
}
