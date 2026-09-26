//! Mail as it arrives, rather than in one lump at the end.
//!
//! The first version of the loading state drew placeholder rows and it was
//! wrong: a skeleton is for a wait too short to notice, where it stops the
//! layout jumping. This wait is up to half a minute, and thirty seconds of
//! invented content is worse than an empty list — it promises a shape the real
//! mail will not keep.
//!
//! So nothing is faked. Each message is handed over the moment its metadata
//! comes back and put straight into the list, so the inbox visibly fills in
//! while the fetch is still running. What the user watches is their own mail.
//!
//! The counter rides along because it is nearly free once the plumbing exists,
//! and "34 of 80" is more honest than a bar that might be lying about how much
//! is left.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use crate::sync::RemoteMail;

/// The sending half, held by the fetch. `Arc` so the worker threads can share
/// one stream.
#[derive(Debug, Default)]
pub struct MailStream {
    sender: Mutex<Option<Sender<RemoteMail>>>,
    done: AtomicUsize,
    total: AtomicUsize,
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
        done: AtomicUsize::new(0),
        total: AtomicUsize::new(0),
    });
    (stream, receiver)
}

impl MailStream {
    /// How many messages the fetch is aiming for.
    pub fn set_total(&self, total: usize) {
        self.total.store(total, Ordering::Relaxed);
    }

    /// Hand one fetched message to the UI. Called from the worker threads.
    pub fn push(&self, mail: RemoteMail) {
        self.done.fetch_add(1, Ordering::Relaxed);
        if let Some(sender) = lock(&self.sender).as_ref() {
            // A send error means the UI is gone, which is not a fetch failure:
            // the mail is simply no longer wanted by anyone.
            let _ = sender.send(mail);
        }
    }

    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    /// "34 of 80", or nothing to say while the total is still unknown.
    pub fn label(&self) -> Option<String> {
        let total = self.total();
        (total > 0).then(|| format!("{} of {}", self.done(), total))
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
    // A poisoned lock means a worker panicked mid-send. The counter is still
    // sound, and the mail that was in flight is one message the user can fetch
    // again, so there is nothing to salvage by refusing to continue.
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
