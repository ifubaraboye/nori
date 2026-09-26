use std::rc::Rc;

use gpui::{
    App, IntoElement, Pixels, Point, RenderOnce, Role, SharedString, Window, div, prelude::*, px,
    transparent_black,
};

use super::{Button, ButtonStyle, Icon};
use crate::model::{Density, EmailSummary, Label};
use crate::theme::Theme;

/// Shared, so both the star button and the row body can hand a copy to their
/// own click handler.
type RowHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// Opens the row's overflow menu at the click point. The position travels with
/// the click because the list is virtualized: a row can be unmounted and
/// remounted at a different y, so asking the list for a row's bounds after the
/// fact would place the menu wrongly once the list scrolls.
type MenuHandler = Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App) + 'static>;

/// The unread accent bar's width. Flush left, full row height.
const ACCENT_BAR_WIDTH: f32 = 3.;

/// Sender column width in the single-line layout, so senders line up.
const SENDER_COLUMN: f32 = 200.;

/// Trailing column for the time or date in the three-line layout, where the
/// timestamp sits on the row's centre line next to the star.
const TIMESTAMP_COLUMN: f32 = 76.;

/// Star size. Not the dense 12px default: the star is the row's only
/// affordance and reads too small against 13px text at that size.
const STAR_SIZE: f32 = 16.;

#[derive(IntoElement)]
pub struct EmailRow {
    email: EmailSummary,
    selected: bool,
    density: Density,
    /// The mail's assigned labels, resolved by the caller. Shown as chips
    /// immediately left of the subject; empty for unlabelled mail, so rows
    /// without labels lay out exactly as if chips did not exist.
    labels: Vec<Label>,
    on_open: RowHandler,
    on_star: RowHandler,
    on_open_menu: MenuHandler,
}

impl EmailRow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        email: EmailSummary,
        selected: bool,
        density: Density,
        labels: Vec<Label>,
        on_open: impl Fn(&mut Window, &mut App) + 'static,
        on_star: impl Fn(&mut Window, &mut App) + 'static,
        on_open_menu: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            email,
            selected,
            density,
            labels,
            on_open: Rc::new(on_open),
            on_star: Rc::new(on_star),
            on_open_menu: Rc::new(on_open_menu),
        }
    }

    /// The unread marker. A bar rather than a weight change, because it is
    /// the one signal that survives a glance while scrolling fast. Selection
    /// is the background, so the two never fight for the same channel.
    ///
    /// The slot is always laid out, filled or not. Reserving it is what keeps
    /// the sender column on the same x whether or not the mail is unread;
    /// rendering it only when needed would shift every read row 3px left.
    fn render_unread_bar(&self, theme: Theme) -> gpui::AnyElement {
        div()
            .w(px(ACCENT_BAR_WIDTH))
            .h_full()
            .flex_none()
            .bg(if self.email.unread {
                theme.accent
            } else {
                transparent_black()
            })
            .into_any_element()
    }

    /// The overflow menu. Label assignment lives here; the assigned labels
    /// themselves ride on the row as chips, immediately left of the subject,
    /// so what a mail carries is visible without opening anything.
    fn render_overflow(&self, theme: Theme) -> gpui::AnyElement {
        let id = self.email.id.clone();
        let on_open_menu = self.on_open_menu.clone();
        let handle = Button::new(format!("row-menu-{id}"), "")
            // Not dense: dense clamps the glyph to 12px, which reads too small
            // beside the 13px row text.
            .px(4.)
            .icon(Icon::new("icons/ellipsis.svg", 14., theme.ghost))
            .style(ButtonStyle::Ghost)
            .aria_label(format!("More actions for {}", self.email.sender))
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                on_open_menu(event.position(), window, cx);
            });
        // Icon-only chrome, so no hover fill and no focus ring to box it in.
        // Wrapped so the row owns a named selector for this button, the way
        // the star and the divider do; `Button` only sets an element id.
        div()
            .id(format!("email-row-menu-{id}"))
            .debug_selector(move || format!("row-menu-{id}"))
            .flex_none()
            .flex()
            .items_center()
            .child(handle.without_hover_fill().without_focus_ring())
            .into_any_element()
    }

    /// The mail's labels as chips, immediately left of the subject. Square,
    /// like every other surface here, and small: solid label text over a
    /// translucent wash of the same hue, the same palette the sidebar and
    /// the label menu draw from. Nothing renders when the mail carries no
    /// labels, so unlabelled rows keep their exact current layout.
    fn render_labels(&self) -> Option<gpui::AnyElement> {
        if self.labels.is_empty() {
            return None;
        }
        let email_id = self.email.id.clone();
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .children(self.labels.iter().map(|label| {
                    let (text, border, fill) = label.chip();
                    let label_id = label.id;
                    let selector_id = email_id.clone();
                    div()
                        .id(format!("row-label-{selector_id}-{label_id}"))
                        .debug_selector(move || format!("row-label-{selector_id}-{label_id}"))
                        .px(px(6.))
                        .py(px(1.))
                        .max_w(px(120.))
                        .border_1()
                        .border_color(border)
                        .bg(fill)
                        .text_size(px(11.))
                        .text_color(text)
                        .truncate()
                        .child(label.name.clone())
                }))
                .into_any_element(),
        )
    }

    fn render_star(&self, theme: Theme) -> gpui::AnyElement {
        let id = self.email.id.clone();
        let star_label: SharedString = if self.email.starred {
            "Remove star".into()
        } else {
            "Add star".into()
        };
        // The click handler has to own its copy: a borrowed handler cannot
        // escape into the element tree.
        let on_star = self.on_star.clone();
        Button::new(format!("star-{id}"), "")
            // Deliberately not dense: the dense variant clamps the icon to
            // 12px, which reads too small next to 13px row text.
            .px(4.)
            .icon(Icon::new(
                if self.email.starred {
                    "icons/star-filled.svg"
                } else {
                    "icons/star.svg"
                },
                STAR_SIZE,
                if self.email.starred {
                    theme.accent
                } else {
                    theme.ghost
                },
            ))
            .style(ButtonStyle::Ghost)
            .aria_label(star_label)
            .on_click(move |_event, window, cx| {
                cx.stop_propagation();
                on_star(window, cx);
            })
            .into_any_element()
    }

    /// Single line: sender, chip, then subject and preview running together
    /// behind an em dash. The preview shrinks first, since the subject is
    /// what identifies the mail.
    fn render_compact(&self, theme: Theme) -> gpui::AnyElement {
        let id = self.email.id.clone();
        let weight = if self.email.unread {
            gpui::FontWeight::SEMIBOLD
        } else {
            gpui::FontWeight::NORMAL
        };
        let unread_color = if self.email.unread {
            theme.text
        } else {
            theme.muted
        };

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(10.))
            // Clears the unread bar, and keeps the same left edge on rows
            // without one so the sender column never shifts between them.
            .pl(px(8.))
            .pr(px(10.))
            .child(
                div()
                    .id(format!("email-row-sender-{id}"))
                    .debug_selector(move || format!("email-row-{id}-sender"))
                    .flex_none()
                    .w(px(self
                        .density
                        .sender_column_width()
                        .unwrap_or(SENDER_COLUMN)))
                    .text_size(px(13.))
                    .text_color(unread_color)
                    .truncate()
                    .child(self.email.sender.clone()),
            )
            // Subject and preview read as one sentence, so the preview
            // starts right after the subject rather than being pushed to the
            // far right, and the em dash carries the break between them.
            // Assigned labels sit immediately left of the subject.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when_some(self.render_labels(), |this, labels| this.child(labels))
                    .child(
                        div()
                            .min_w(px(0.))
                            .text_size(px(13.))
                            .font_weight(weight)
                            .text_color(theme.text)
                            .truncate()
                            .child(self.email.subject.clone()),
                    )
                    .when(self.density.shows_preview(), |this| {
                        this.child(
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(theme.ghost)
                                .child("—"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                // Outweighs the subject when the row runs
                                // out of room, so the preview clips first.
                                .flex_shrink(3.)
                                .text_size(px(12.))
                                .text_color(theme.ghost)
                                .truncate()
                                .child(self.email.preview.clone()),
                        )
                    }),
            )
            .into_any_element()
    }

    /// Three lines: sender and time, subject, preview. More room per row for
    /// reading, fewer messages on screen.
    fn render_comfortable(&self, theme: Theme) -> gpui::AnyElement {
        let weight = if self.email.unread {
            gpui::FontWeight::SEMIBOLD
        } else {
            gpui::FontWeight::NORMAL
        };

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(px(14.))
            .py(px(7.))
            .flex()
            .flex_col()
            // The stack has to fit inside a fixed-height row. Without this it
            // overflows and the last line is clipped mid-glyph.
            .justify_center()
            .gap(px(2.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .font_weight(weight)
                    .text_color(theme.text)
                    .truncate()
                    .child(self.email.sender.clone()),
            )
            // The subject line carries the labels on its left, so what the
            // mail is tagged with reads as part of the title row.
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .when_some(self.render_labels(), |this, labels| this.child(labels))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.5))
                            .font_weight(weight)
                            .text_color(theme.text)
                            .truncate()
                            .child(self.email.subject.clone()),
                    ),
            )
            .when(self.density.shows_preview(), |this| {
                this.child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.muted)
                        .truncate()
                        .child(self.email.preview.clone()),
                )
            })
            .into_any_element()
    }

    /// The timestamp, in the trailing column beside the star so both sit on
    /// the row's centre line rather than one floating at the top.
    fn render_timestamp(&self, theme: Theme) -> gpui::AnyElement {
        div()
            .flex_none()
            .w(px(TIMESTAMP_COLUMN))
            .text_size(px(10.5))
            .text_color(theme.ghost)
            .child(self.email.timestamp.clone())
            .into_any_element()
    }
}

impl RenderOnce for EmailRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::current(cx);
        let id = self.email.id.clone();
        let label = format!("{}: {}", self.email.sender, self.email.subject);
        let on_open = self.on_open.clone();
        let selected = self.selected;
        let row_background = if selected {
            theme.selected_layer
        } else {
            theme.canvas
        };

        div()
            .id(format!("email-row-{id}"))
            .debug_selector(move || format!("email-row-{id}"))
            .h(px(self.density.row_height()))
            .w_full()
            .flex()
            .items_stretch()
            .border_b_1()
            .border_color(theme.hairline)
            .bg(if std::env::var_os("NORI_ROW_PROBE").is_some() {
                gpui::green()
            } else {
                row_background
            })
            .hover(|style| {
                style.bg(if selected {
                    theme.selected_layer
                } else {
                    theme.hover
                })
            })
            .active(|style| {
                style.bg(if selected {
                    theme.selected_layer
                } else {
                    theme.active
                })
            })
            .cursor_pointer()
            .role(Role::ListItem)
            .aria_label(label)
            .aria_selected(selected)
            .on_click(move |_event, window, cx| on_open(window, cx))
            .child(self.render_unread_bar(theme))
            .child(match self.density {
                Density::Compact => self.render_compact(theme),
                Density::Comfortable => self.render_comfortable(theme),
            })
            // In the three-line layout the time moves out of the text stack
            // and onto the row's centre line, level with the star.
            .when(self.density == Density::Comfortable, |this| {
                this.child(
                    div()
                        .flex_none()
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .child(self.render_timestamp(theme)),
                )
            })
            .child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .child(self.render_overflow(theme))
                    .child(self.render_star(theme)),
            )
    }
}
