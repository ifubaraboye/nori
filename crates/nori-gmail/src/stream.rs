//! Mail as it arrives, rather than in one lump at the end.
//!
//! A fetch that collects everything and hands it over at the finish line makes
//! the user wait through an empty list for no reason. Each message goes to the
//! UI the moment its metadata comes back instead, so the inbox fills in while
//! the fetch is still running and what the user watches is their own mail.
//!
//! There is deliberately nothing else here — no progress bar, no counter, no
//! skeleton. An earlier version had all three and the honest result was a
//! thirty-second wait spent looking at invented rows and a progress line that
//! read as part of the header. What is actually happening is already visible:
//! mail is arriving.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use crate::sync::RemoteMail;

/// The sending half, held by the fetch. `Arc` so the worker threads can share
/// one stream.
#[derive(Debug, Default)]
pub struct MailStream {
    sender: Mutex<Option<Sender<RemoteMail>>>,
}

/// The receiving half, owned by the UI task that drains it.
pub type IncomingMail = Receiver<RemoteMail>;

/// A stream and its receiving end. They are made together because the pair is
/// useless apart, and handing back only the sender is how mails get fetched
/// and thrown away.
pub fn mail_stream() -> (Arc<MailStream>, IncomingMail) {
    let (sender, receiver) = mpsc::channel();
    let stream = Arc::new(MailStream {
        sender: Mutex::new(Some(sender)),
    });
    (stream, receiver)
}

impl MailStream {
    /// Hand one fetched message to the UI. Called from the worker threads.
    pub fn push(&self, mail: RemoteMail) {
        if let Some(sender) = lock(&self.sender).as_ref() {
            // A send error means the UI is gone, which is not a fetch failure:
            // the mail is simply no longer wanted by anyone.
            let _ = sender.send(mail);
        }
    }

    /// Close the stream. The UI's receive loop ends when the channel empties.
    pub fn finish(&self) {
        lock(&self.sender).take();
    }
}

/// Take everything waiting, without blocking.
///
/// `try_recv` rather than `recv` because this runs on the foreground executor:
/// blocking there would freeze the UI, which is the one thing the whole feature
/// exists to avoid.
pub fn drain(receiver: &mut IncomingMail, out: &mut Vec<RemoteMail>) -> bool {
    let mut open = true;
    loop {
        match receiver.try_recv() {
            Ok(mail) => out.push(mail),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                open = false;
                break;
            }
        }
    }
    open
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A poisoned lock means a worker panicked mid-send. The mail that was in
    // flight is one message the user can fetch again, so there is nothing to
    // salvage by refusing to continue.
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
