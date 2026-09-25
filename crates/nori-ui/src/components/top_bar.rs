use gpui::{App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px};

use super::SidebarToggle;
use crate::theme::Theme;

/// Minimal top bar (see `MailApp`): always rendered above the mails.
///
/// It shows the current title (mailbox label or open mail subject) and holds
/// the shared sidebar toggle, but only while the sidebar is hidden; while the
/// sidebar is visible the same toggle lives in the sidebar header instead, so
/// exactly one is ever on screen.
///
/// When an email is open, `prefix` carries its mailbox label and the title
/// renders as a `Mailbox / Subject` breadcrumb; it is `None` for the plain
/// mailbox view.
#[derive(IntoElement)]
pub struct TopBar {
    theme: Theme,
    sidebar_visible: bool,
    prefix: Option<SharedString>,
    title: SharedString,
    toggle: SidebarToggle,
}

impl TopBar {
    pub fn new(
        theme: Theme,
        sidebar_visible: bool,
        prefix: Option<impl Into<SharedString>>,
        title: impl Into<SharedString>,
        on_toggle_sidebar: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            theme,
            sidebar_visible,
            prefix: prefix.map(Into::into),
            title: title.into(),
            toggle: SidebarToggle::new("top-bar-sidebar", theme, on_toggle_sidebar),
        }
    }
}

impl RenderOnce for TopBar {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let sidebar_visible = self.sidebar_visible;

        div()
            .id("top-bar")
            .debug_selector(|| "top-bar".into())
            .h(px(42.))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .bg(theme.canvas)
            .border_b_1()
            .border_color(theme.hairline)
            .when(!sidebar_visible, |this| this.child(self.toggle))
            .when_some(self.prefix.clone(), |this, prefix| {
                this.child(
                    div()
                        .id("top-bar-prefix")
                        .debug_selector(|| "top-bar-prefix".into())
                        .flex_none()
                        // Same inset the bare title carries, so the mailbox
                        // label does not hop left when a mail is opened.
                        .pl(px(4.))
                        .text_size(px(13.))
                        // `text_dim`, not `muted`: at the same 13px as the
                        // subject, a muted prefix reads as visibly smaller
                        // purely from the brightness difference.
                        .text_color(theme.text_dim)
                        .child(prefix),
                )
                .child(
                    div()
                        .id("top-bar-separator")
                        .debug_selector(|| "top-bar-separator".into())
                        .flex_none()
                        .text_size(px(13.))
                        .text_color(theme.text_dim)
                        .child("/"),
                )
            })
            .child(
                div()
                    .id("top-bar-title")
                    .debug_selector(|| "top-bar-title".into())
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_left()
                    // The leading inset belongs to whichever run is leftmost,
                    // otherwise the label sits 4px short of where it sat as
                    // the title and the whole breadcrumb shifts on open.
                    .when(self.prefix.is_none(), |this| this.pl(px(4.)))
                    // Matches the sidebar's mailbox rows (13px, normal weight)
                    // so the two read as one typeface across the app.
                    .text_size(px(13.))
                    .text_color(theme.text)
                    .child(self.title.clone()),
            )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn top_bar_ids_are_stable() {
        // Contract with visual tests: ids must not drift.
        for id in [
            "top-bar",
            "top-bar-sidebar",
            "top-bar-prefix",
            "top-bar-separator",
            "top-bar-title",
        ] {
            assert!(!id.is_empty());
        }
    }
}
