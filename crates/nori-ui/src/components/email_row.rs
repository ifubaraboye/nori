use std::rc::Rc;

use gpui::{
    App, Hsla, IntoElement, RenderOnce, Role, SharedString, Window, div, prelude::*, px,
    transparent_black,
};

use super::{Button, ButtonStyle, Icon};
use crate::model::{Density, EmailSummary};
use crate::theme::Theme;

/// Shared, so both the star button and the row body can hand a copy to their
/// own click handler.
type RowHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

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

/// Label chip hues. Six spreads that stay distinguishable on the dark canvas;
/// the same label always lands on the same one, so the chip is scannable
/// without storing a colour per mail.
const CHIP_HUES: [Hsla; 6] = [
    Hsla {
        h: 0.54,
        s: 0.52,
        l: 0.62,
        a: 1.,
    },
    Hsla {
        h: 0.36,
        s: 0.45,
        l: 0.60,
        a: 1.,
    },
    Hsla {
        h: 0.08,
        s: 0.55,
        l: 0.63,
        a: 1.,
    },
    Hsla {
        h: 0.75,
        s: 0.42,
        l: 0.66,
        a: 1.,
    },
    Hsla {
        h: 0.60,
        s: 0.48,
        l: 0.68,
        a: 1.,
    },
    Hsla {
        h: 0.15,
        s: 0.50,
        l: 0.66,
        a: 1.,
    },
];

/// Labels are stored as plain lowercase tags, so capitalisation is a display
/// concern. Doing it here rather than in the data means a label arriving from
/// anywhere else still reads as a word.
fn capitalize(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The three tones one label chip needs, all from a single hue.
///
/// A solid fill at full saturation reads as a block of colour dropped into the
/// row rather than part of it, so only the text carries the hue at full
/// strength; the fill and the border are that same hue washed back.
struct LabelChip {
    text: Hsla,
    border: Hsla,
    fill: Hsla,
}

fn chip_style(label: &str) -> LabelChip {
    let mut hash: u32 = 2166136261;
    for byte in label.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16777619);
    }
    let base = CHIP_HUES[(hash % CHIP_HUES.len() as u32) as usize];
    let washed = |a: f32| Hsla {
        h: base.h,
        s: base.s,
        l: base.l,
        a,
    };
    LabelChip {
        text: base,
        border: washed(0.45),
        fill: washed(0.16),
    }
}

#[derive(IntoElement)]
pub struct EmailRow {
    email: EmailSummary,
    selected: bool,
    density: Density,
    theme: Theme,
    on_open: RowHandler,
    on_star: RowHandler,
}

impl EmailRow {
    pub fn new(
        email: EmailSummary,
        selected: bool,
        density: Density,
        theme: Theme,
        on_open: impl Fn(&mut Window, &mut App) + 'static,
        on_star: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            email,
            selected,
            density,
            theme,
            on_open: Rc::new(on_open),
            on_star: Rc::new(on_star),
        }
    }

    /// The unread marker. A bar rather than a weight change, because it is
    /// the one signal that survives a glance while scrolling fast. Selection
    /// is the background, so the two never fight for the same channel.
    ///
    /// The slot is always laid out, filled or not. Reserving it is what keeps
    /// the sender column on the same x whether or not the mail is unread;
    /// rendering it only when needed would shift every read row 3px left.
    fn render_unread_bar(&self) -> gpui::AnyElement {
        div()
            .w(px(ACCENT_BAR_WIDTH))
            .h_full()
            .flex_none()
            .bg(if self.email.unread {
                self.theme.accent
            } else {
                transparent_black()
            })
            .into_any_element()
    }

    /// Optional label chip. Rendered only when the mail carries a label, so
    /// untagged rows close the gap instead of reserving space for nothing.
    ///
    /// Square corners, a washed-back fill and border in the label's hue, and
    /// the text carrying it at full strength. Fully saturated blocks read as
    /// foreign objects dropped into the row.
    fn render_label(&self) -> Option<gpui::AnyElement> {
        let label = self.email.label.as_deref()?;
        let chip = chip_style(label);
        Some(
            div()
                .flex_none()
                .max_w(px(120.))
                .px(px(7.))
                .h(px(18.))
                .flex()
                .items_center()
                .rounded(px(0.))
                .bg(chip.fill)
                .border_1()
                .border_color(chip.border)
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(chip.text)
                .child(SharedString::from(capitalize(label)))
                .into_any_element(),
        )
    }

    fn render_star(&self) -> gpui::AnyElement {
        let id = self.email.id;
        let star_label: SharedString = if self.email.starred {
            "Remove star".into()
        } else {
            "Add star".into()
        };
        // The click handler has to own its copy: a borrowed handler cannot
        // escape into the element tree.
        let on_star = self.on_star.clone();
        Button::new(format!("star-{}", id.0), "")
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
                    self.theme.accent
                } else {
                    self.theme.ghost
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
    fn render_compact(&self) -> gpui::AnyElement {
        let theme = self.theme;
        let id = self.email.id;
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
                    .id(("email-row-sender", id.0 as usize))
                    .debug_selector(move || format!("email-row-{}-sender", id.0))
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
            .when_some(self.render_label(), |this, chip| this.child(chip))
            // Subject and preview read as one sentence, so the preview
            // starts right after the subject rather than being pushed to the
            // far right, and the em dash carries the break between them.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
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
    fn render_comfortable(&self) -> gpui::AnyElement {
        let theme = self.theme;
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
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(weight)
                    .text_color(theme.text)
                    .truncate()
                    .child(self.email.subject.clone()),
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
    fn render_timestamp(&self) -> gpui::AnyElement {
        div()
            .flex_none()
            .w(px(TIMESTAMP_COLUMN))
            .text_size(px(10.5))
            .text_color(self.theme.ghost)
            .child(self.email.timestamp.clone())
            .into_any_element()
    }
}

impl RenderOnce for EmailRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let id = self.email.id;
        let label = format!("{}: {}", self.email.sender, self.email.subject);
        let on_open = self.on_open.clone();
        let selected = self.selected;
        let row_background = if selected {
            theme.selected_layer
        } else {
            theme.canvas
        };

        div()
            .id(("email-row", id.0 as usize))
            .debug_selector(move || format!("email-row-{}", id.0))
            .h(px(self.density.row_height()))
            .w_full()
            .flex()
            .items_stretch()
            .border_b_1()
            .border_color(theme.hairline)
            .bg(row_background)
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
            .child(self.render_unread_bar())
            .child(match self.density {
                Density::Compact => self.render_compact(),
                Density::Comfortable => self.render_comfortable(),
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
                        .child(self.render_timestamp()),
                )
            })
            .child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .child(self.render_star()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EmailId;

    fn summary(unread: bool, label: Option<&str>) -> EmailSummary {
        EmailSummary {
            id: EmailId(1),
            sender: "Arlene McCoy".into(),
            subject: "Application for Product Manager position".into(),
            preview: "A meeting".into(),
            timestamp: "10:42 AM".into(),
            unread,
            starred: false,
            label: label.map(str::to_string),
        }
    }

    #[test]
    fn a_label_always_maps_to_the_same_chip_colour() {
        let first = chip_style("recruiting");
        let second = chip_style("recruiting");
        assert_eq!(first.text.h, second.text.h);
        assert_eq!(first.text.l, second.text.l);
    }

    #[test]
    fn a_chip_washes_its_fill_and_border_back_but_not_its_text() {
        let chip = chip_style("recruiting");
        // The solid fill read as a foreign block, so only the text carries
        // the hue at full strength.
        assert_eq!(chip.text.a, 1.);
        assert!(chip.fill.a < 0.3, "the fill should be a wash, not a block");
        assert!(
            chip.border.a < chip.text.a,
            "the border sits under the text"
        );
        // All three come from one hue, so the chip never reads as two colours.
        assert_eq!(chip.text.h, chip.fill.h);
        assert_eq!(chip.text.s, chip.fill.s);
    }

    #[test]
    fn different_labels_can_land_on_different_chips() {
        let hues: Vec<f32> = [
            "recruiting",
            "travel",
            "signature",
            "finance",
            "hiring",
            "design",
        ]
        .iter()
        .map(|label| chip_style(label).text.h)
        .collect();
        let mut unique = hues.clone();
        unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
        unique.dedup();
        assert!(
            unique.len() > 1,
            "labels should not all collapse onto one chip colour"
        );
    }

    #[test]
    fn a_label_reads_as_a_capitalized_word() {
        assert_eq!(capitalize("project"), "Project");
        assert_eq!(capitalize("recruiting"), "Recruiting");
        // Already capitalized, and a single letter, both stay as they are.
        assert_eq!(capitalize("Project"), "Project");
        assert_eq!(capitalize("x"), "X");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn capitalizing_keeps_the_rest_of_the_label_untouched() {
        assert_eq!(capitalize("in-review"), "In-review");
        assert_eq!(capitalize("two words"), "Two words");
    }

    #[test]
    fn compact_rows_are_forty_pixels_and_comfortable_are_seventy() {
        assert_eq!(Density::Compact.row_height(), 40.);
        assert_eq!(Density::Comfortable.row_height(), 70.);
        let _ = (summary(true, None), summary(false, Some("x")));
    }
}
