use std::rc::Rc;

use gpui::{App, FocusHandle, IntoElement, RenderOnce, Role, Window, div, prelude::*, px};

use super::super::components::{Button, ButtonStyle, Icon};
use crate::model::{Email, Label, LabelId};
use crate::theme::Theme;

type EmailActionHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;
type LabelHandler = Rc<dyn Fn(LabelId, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct EmailView {
    email: Email,
    focus_handle: FocusHandle,
    theme: Theme,
    on_reply: EmailActionHandler,
    on_reply_all: EmailActionHandler,
    on_forward: EmailActionHandler,
    on_toggle_pin: EmailActionHandler,
    /// Every label that exists, and the ones this mail already carries.
    labels: Vec<Label>,
    assigned: Vec<LabelId>,
    on_toggle_label: LabelHandler,
}

impl EmailView {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        email: Email,
        focus_handle: FocusHandle,
        theme: Theme,
        on_reply: impl Fn(&mut Window, &mut App) + 'static,
        on_reply_all: impl Fn(&mut Window, &mut App) + 'static,
        on_forward: impl Fn(&mut Window, &mut App) + 'static,
        on_toggle_pin: impl Fn(&mut Window, &mut App) + 'static,
        labels: Vec<Label>,
        assigned: Vec<LabelId>,
        on_toggle_label: impl Fn(LabelId, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            email,
            focus_handle,
            theme,
            on_reply: Rc::new(on_reply),
            on_reply_all: Rc::new(on_reply_all),
            on_forward: Rc::new(on_forward),
            on_toggle_pin: Rc::new(on_toggle_pin),
            labels,
            assigned,
            on_toggle_label: Rc::new(on_toggle_label),
        }
    }
}

/// The label picker row under the body: every label as a toggle, the ones this
/// mail carries filled, the rest outlined. Clicking adds or removes.
fn label_row(
    labels: &[Label],
    assigned: &[LabelId],
    theme: Theme,
    on_toggle: &LabelHandler,
) -> impl IntoElement {
    div()
        .id("email-labels")
        .debug_selector(|| "email-labels".into())
        .mt(px(15.))
        .flex()
        .items_center()
        .gap(px(6.))
        .flex_wrap()
        .children(labels.iter().map(|label| {
            let id = label.id;
            let is_on = assigned.contains(&id);
            let (text, border, fill) = label.chip();
            let on_click = on_toggle.clone();
            div()
                .id(("email-label", id as usize))
                .px(px(8.))
                .h(px(20.))
                .flex()
                .items_center()
                .text_size(px(11.))
                .cursor_pointer()
                .role(Role::Button)
                .aria_selected(is_on)
                .aria_label(format!(
                    "{} {}",
                    if is_on { "Remove" } else { "Add" },
                    label.name
                ))
                .focusable()
                .tab_stop(true)
                .focus_visible(|style| style.border_color(theme.focus))
                .border_1()
                .border_color(if is_on { border } else { theme.hairline_strong })
                .bg(if is_on { fill } else { theme.canvas })
                .hover(|style| style.bg(if is_on { fill } else { theme.hover }))
                .text_color(if is_on { text } else { theme.muted })
                .on_click(move |_event, window, cx| on_click(id, window, cx))
                .child(label.name.clone())
        }))
        .when(labels.is_empty(), |this| {
            this.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.faint)
                    .child("No labels yet \u{2014} create one in the sidebar"),
            )
        })
}

impl RenderOnce for EmailView {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let email = self.email;
        let on_reply = self.on_reply.clone();
        let on_reply_all = self.on_reply_all.clone();
        let on_forward = self.on_forward.clone();
        let on_toggle_pin = self.on_toggle_pin.clone();
        let pinned = email.pinned;
        let labels = self.labels;
        let assigned = self.assigned;
        let on_toggle_label = self.on_toggle_label.clone();

        div()
            .id(("email-view", email.id.0 as usize))
            .track_focus(&self.focus_handle)
            .tab_index(0)
            .focus_visible(|style| style.border_color(theme.focus))
            .flex_1()
            .min_h_0()
            .overflow_scroll()
            .bg(theme.canvas)
            .child(
                div()
                    .w(px(720.))
                    .max_w_full()
                    .px(px(44.))
                    .pt(px(32.))
                    .pb(px(48.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(email.subject.clone()),
                    )
                    .child(
                        div()
                            .mt(px(17.))
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .child(format!("{} <{}>", email.sender, email.address)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.muted)
                                            .child(format!("To: {}", email.recipients.join(", "))),
                                    ),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.ghost)
                                    .child(email.full_date.clone()),
                            ),
                    )
                    .child(div().mt(px(23.)).h(px(1.)).bg(theme.hairline))
                    .child(div().mt(px(25.)).flex().flex_col().gap(px(15.)).children(
                        email.body.iter().map(|paragraph| {
                            div()
                                .text_size(px(14.))
                                .line_height(px(22.))
                                .text_color(theme.text)
                                .child(paragraph.clone())
                        }),
                    ))
                    .child(div().mt(px(28.)).h(px(1.)).bg(theme.hairline))
                    .child(label_row(&labels, &assigned, theme, &on_toggle_label))
                    .child(
                        div()
                            .mt(px(15.))
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .child(
                                Button::new("reply-button", "Reply")
                                    .dense()
                                    .icon(Icon::new("icons/reply.svg", 13., theme.muted))
                                    .style(ButtonStyle::Subtle)
                                    .on_click(move |_event, window, cx| on_reply(window, cx)),
                            )
                            .child(
                                Button::new("reply-all-button", "Reply All")
                                    .dense()
                                    .icon(Icon::new("icons/reply-all.svg", 13., theme.muted))
                                    .style(ButtonStyle::Subtle)
                                    .on_click(move |_event, window, cx| on_reply_all(window, cx)),
                            )
                            .child(
                                Button::new("forward-button", "Forward")
                                    .dense()
                                    .icon(Icon::new("icons/forward.svg", 13., theme.muted))
                                    .style(ButtonStyle::Subtle)
                                    .on_click(move |_event, window, cx| on_forward(window, cx)),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id(if pinned { "email-unpin" } else { "email-pin" })
                                    .debug_selector(move || {
                                        if pinned {
                                            "email-unpin".to_string()
                                        } else {
                                            "email-pin".to_string()
                                        }
                                    })
                                    .child(
                                        Button::new(
                                            "email-pin-button",
                                            if pinned { "Unpin" } else { "Pin" },
                                        )
                                        .dense()
                                        .icon(Icon::new(
                                            if pinned {
                                                "icons/pin.svg"
                                            } else {
                                                "icons/pin-off.svg"
                                            },
                                            13.,
                                            if pinned { theme.accent } else { theme.muted },
                                        ))
                                        .style(ButtonStyle::Subtle)
                                        .on_click(
                                            move |_event, window, cx| on_toggle_pin(window, cx),
                                        ),
                                    ),
                            ),
                    ),
            )
    }
}
