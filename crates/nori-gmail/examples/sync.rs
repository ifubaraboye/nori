//! Runs the same sync the app runs, against the saved token, and prints what
//! came back. This is how the data path gets checked without a window.
//!
//! ```sh
//! cargo run -p nori-gmail --example sync -- you@gmail.com
//! ```

use nori_gmail::{Sync, SyncOutcome};

fn main() -> anyhow::Result<()> {
    let account = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "me@gmail.com".to_string());
    let credentials = nori_gmail::Credentials::from_env()?;
    let store = nori_gmail::FileTokenStore::with_account(&account)?;
    let mut sync = Sync::new(nori_gmail::agent(), &credentials, &store)?;

    println!("== full sync ==");
    let started = std::time::Instant::now();
    let snapshot = sync.full()?;
    println!("  took:     {:?}", started.elapsed());
    println!("  account:  {}", snapshot.account);
    println!("  labels:   {}", snapshot.labels.len());
    for label in &snapshot.labels {
        println!("             {:<20} #{:06x}", label.name, label.colour);
    }
    println!("  mail:     {}", snapshot.mail.len());
    println!(
        "  history:  {}",
        snapshot.history_id.clone().unwrap_or_default()
    );

    // The interesting cases are the ones the MIME walker gets wrong.
    let mut with_body = 0;
    let mut empty_body = 0;
    let mut no_snippet = 0;
    let mut no_date = 0;
    let body_started = std::time::Instant::now();
    for id in snapshot
        .mail
        .iter()
        .take(40)
        .map(|m| m.id.clone())
        .collect::<Vec<_>>()
    {
        let bodies = sync.bodies(&[id])?;
        match bodies.into_iter().next() {
            Some((_, body)) if body.is_empty() => empty_body += 1,
            Some(_) => with_body += 1,
            None => {}
        }
    }
    for mail in &snapshot.mail {
        if mail.preview.is_empty() {
            no_snippet += 1;
        }
        if mail.full_date == "Unknown date" {
            no_date += 1;
        }
    }
    println!("\n== data quality ==");
    println!(
        "  bodies fetched: {with_body} with text, {empty_body} empty (of 40 sampled, {:?})",
        body_started.elapsed()
    );
    println!("  missing snippet: {no_snippet} of {}", snapshot.mail.len());
    println!("  missing date:    {no_date} of {}", snapshot.mail.len());

    // Show a few rows so the field mapping can be eyeballed.
    println!("\n== sample rows ==");
    for mail in snapshot.mail.iter().take(6) {
        let mut flags = Vec::new();
        for (label, name) in [
            ("UNREAD", "unread"),
            ("STARRED", "starred"),
            ("TRASH", "trash"),
        ] {
            if mail.has_label(label) {
                flags.push(name);
            }
        }
        println!("  [{}] {}", flags.join(","), mail.timestamp);
        println!("      from:    {} <{}>", mail.sender, mail.address);
        println!("      subject: {}", mail.subject);
        println!("      preview: {}", truncate(&mail.preview, 70));
        println!("      labels:  {:?}", mail.label_ids);
    }

    // A body, so the MIME walker can be judged on real mail.
    if let Some(mail) = snapshot.mail.iter().find(|m| !m.preview.is_empty()) {
        let bodies = sync.bodies(std::slice::from_ref(&mail.id))?;
        println!("\n== one body: {} ==", mail.subject);
        let text = bodies
            .first()
            .map(|(_, blocks)| nori_gmail::plain_text(blocks))
            .unwrap_or_default();
        for (index, paragraph) in text.split("\n\n").enumerate() {
            println!("  [{index}] {}", truncate(paragraph, 78));
        }
    }

    // Every mailbox query, because Archive is defined by exclusion and a typo
    // there quietly puts the Inbox back inside it.
    println!("\n== mailbox queries ==");
    for (name, query) in [
        ("Inbox", "in:inbox"),
        ("Starred", "is:starred"),
        ("Sent", "in:sent"),
        ("Drafts", "in:drafts"),
        (
            "Archive",
            "in:anywhere -in:inbox -in:sent -in:drafts -in:trash -in:spam -is:starred",
        ),
        ("Trash", "{in:trash in:spam}"),
    ] {
        let started = std::time::Instant::now();
        let mail = sync.fetch(query, 12)?;
        let inbox = mail.iter().filter(|m| m.has_label("INBOX")).count();
        let flagged = if name == "Archive" && inbox > 0 {
            "  <-- ARCHIVE LEAKED THE INBOX"
        } else {
            ""
        };
        println!(
            "  {name:8} {query:64} -> {} mail in {:?}{flagged}",
            mail.len(),
            started.elapsed()
        );
    }

    // The sidebar's numbers. One unit, and they are right for folders whose mail
    // was never downloaded.
    println!("\n== folder counts (7 units, no mail downloaded) ==");
    let counts = sync.folder_counts()?;
    println!("  inbox:    {}", counts.inbox);
    println!("  starred:  {}", counts.starred);
    println!("  sent:     {}", counts.sent);
    println!("  drafts:   {}", counts.drafts);
    println!("  archive:  {}", counts.archive);
    println!("  trash:    {}", counts.trash);
    println!("  inbox unread: {}", counts.inbox_unread);
    let local = snapshot
        .mail
        .iter()
        .filter(|m| m.has_label("INBOX"))
        .count();
    println!(
        "  (inbox count from the server is {} vs {} downloaded)",
        counts.inbox, local
    );

    // The incremental path, which is the one that has to notice an aged-out
    // cursor and recover rather than getting stuck.
    println!("\n== incremental ==");
    match sync.incremental(&snapshot.history_id.clone().unwrap_or_default())? {
        Some(delta) => {
            let outcome = SyncOutcome::Changes(delta);
            match outcome {
                SyncOutcome::Changes(delta) => println!(
                    "  nothing changed: {} changed, {} deleted, new history {}",
                    delta.changed.len(),
                    delta.deleted.len(),
                    delta.history_id.unwrap_or_default()
                ),
                _ => unreachable!(),
            }
        }
        None => println!("  the stored history id has aged out, so a full sync is required"),
    }

    println!("\nAll good. Nori will pick this up on launch.");
    Ok(())
}

fn truncate(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let cut: String = flat.chars().take(limit).collect();
    format!("{cut}…")
}
