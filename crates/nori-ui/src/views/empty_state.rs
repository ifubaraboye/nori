//! What a list shows when it has nothing in it.
//!
//! This exists because the alternative was worse. The app used to start with a
//! handful of invented messages already in the store, which meant that someone
//! who had never connected an account opened Nori to a list of mail that was
//! not theirs — real-looking senders, real-looking subjects, none of it real. A
//! placeholder is fine in a fixture and unforgivable in a product, because the
//! only way to tell the difference is to already know which is which.
//!
//! So the list says what is true instead, and says it in a way that is worth
//! reading: why it is empty, and what would fill it. An empty state is the one
//! screen a first-run user is guaranteed to see, and "nothing here" on its own
//! is a dead end.

use gpui::{SharedString, div, prelude::*, px};

use crate::theme::Theme;

/// Why the list is empty, which decides what the list says about it.
pub enum Empty {
    /// No account has been connected, so there is no mail to show and no way to
    /// fetch any. The only thing worth offering is the way in.
    NotSignedIn,
    /// Connected, but this folder is empty. Distinct from the above because the
    /// advice is different: there is nothing to fix, only to wait.
    Folder(&'static str),
    /// No account yet and one is on its way: the browser round trip. The list
    /// will stay empty for the whole of it, which is exactly why it has to say
    /// so — an empty pane with no explanation reads as a broken app.
    SigningIn,
    /// Signed in as this address, and the mailbox is being read right now.
    FetchingMail(String),
}

/// The empty list.
pub fn render(
    theme: Theme,
    empty: &Empty,
    on_sign_in: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    // Both halves are `SharedString` because two of the four bodies are built
    // rather than literal: they name the phase or the account.
    let (title, body): (SharedString, SharedString) = match empty {
        Empty::NotSignedIn => (
            "Nothing To Be Found.".into(),
            "Connect a Google account and your mail will appear here.".into(),
        ),
        Empty::SigningIn => (
            "Signing in…".into(),
            "Finish in the browser tab that just opened. Nori is waiting for it, \
             then it starts fetching your mail."
                .into(),
        ),
        Empty::FetchingMail(name) => (
            "Fetching your mail…".into(),
            format!(
                "Signed in as {name}. Reading your mailbox now, this may take \
                 a while. Your mail appears as it arrives."
            )
            .into(),
        ),
        Empty::Folder(name) => (
            format!("Nothing in {name}").into(),
            "Mail that reaches this folder will show up here.".into(),
        ),
    };

    // Named so the wording is assertable: which of these four is on screen is
    // the difference between "signed in and working" and "signed in and
    // apparently stuck", so it should not be eyeballed in a test.
    let kind = match empty {
        Empty::NotSignedIn => "not-signed-in",
        Empty::SigningIn => "signing-in",
        Empty::FetchingMail(_) => "fetching",
        Empty::Folder(_) => "folder",
    };

    let mut column = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(9.))
        .px(px(32.))
        .bg(theme.canvas)
        .child(
            div()
                .id(format!("empty-state-{kind}"))
                .debug_selector(move || format!("empty-state-{kind}"))
                .text_size(px(15.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.muted)
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.5))
                .text_color(theme.faint)
                .text_align(gpui::TextAlign::Center)
                .max_w(px(340.))
                .child(body),
        );

    // Only offered where there is something to do about it. A button on the
    // empty-folder state would be a button that cannot do anything, and a
    // control that cannot do anything is worse than no control.
    if let Empty::NotSignedIn = empty {
        column = column.child(
            div()
                .id("empty-sign-in")
                .debug_selector(|| "empty-sign-in".into())
                .mt(px(6.))
                .cursor_pointer()
                .px(px(14.))
                .py(px(7.))
                .border_1()
                .border_color(theme.hairline_strong)
                .bg(theme.raised)
                .text_size(px(12.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text)
                .hover(|style| style.bg(theme.hover))
                .active(|style| style.bg(theme.active))
                .on_click(move |_event, window, cx| on_sign_in(window, cx))
                .child("Sign in with Google"),
        );
    }

    column.into_any_element()
}
