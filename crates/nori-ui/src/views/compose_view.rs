use gpui::{
    Context, Entity, EventEmitter, IntoElement, Render, Role, Subscription, Window, div,
    prelude::*, px,
};

use super::super::components::{Button, ButtonStyle, Icon, TextField};
use crate::actions::Dismiss;
use crate::model::DraftSeed;
use crate::theme::Theme;

pub enum ComposeEvent {
    Dismiss,
}

impl EventEmitter<ComposeEvent> for ComposeView {}

pub struct ComposeView {
    to: Entity<TextField>,
    subject: Entity<TextField>,
    message: Entity<TextField>,
    /// Kept so a change to the theme global repaints this view. Without it a
    /// switch flipped from the settings page would leave an open compose pane
    /// holding the old palette.
    _theme_sub: Subscription,
}

impl ComposeView {
    pub fn new(seed: DraftSeed, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Each field carries a visible label, so its placeholder is empty.
        // Labelling both spelled "To" and "Subject" twice, once above the
        // field and once inside it, read as a stutter rather than a hint.
        let to = cx.new(|cx| TextField::new("compose-to", "", seed.to, true, 1, cx));
        let subject = cx.new(|cx| TextField::new("compose-subject", "", seed.subject, true, 2, cx));
        // The body keeps its placeholder: it has no label of its own, since a
        // full-height box under the subject needs no naming.
        let message =
            cx.new(|cx| TextField::new("compose-message", "Message", seed.body, false, 3, cx));
        let to_focus = to.read(cx).focus_handle();
        window.focus(&to_focus, cx);
        let theme_sub = cx.observe_global::<Theme>(|_, cx| cx.notify());
        Self {
            to,
            subject,
            message,
            _theme_sub: theme_sub,
        }
    }

    fn dismiss(&mut self, _: &Dismiss, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(ComposeEvent::Dismiss);
    }
}

impl Render for ComposeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let entity = cx.entity();

        div()
            .id("compose-pane")
            .debug_selector(|| "compose-pane".into())
            .role(Role::Dialog)
            .aria_label("Compose")
            // A pane, not a tile: it fills the height it is given and takes
            // its width from the split, so it reads as a second column
            // beside the mail list rather than a card floating on top of it.
            .h_full()
            .w_full()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .on_action(cx.listener(Self::dismiss))
            .child(
                div()
                    .h(px(46.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(15.))
                    .border_b_1()
                    .border_color(theme.hairline)
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("Compose"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("compose-close", "")
                            .dense()
                            .icon(Icon::new("icons/close.svg", 14., theme.muted))
                            .style(ButtonStyle::Ghost)
                            .on_click(move |_event, _window, cx| {
                                cx.stop_propagation();
                                entity.update(cx, |_, cx| cx.emit(ComposeEvent::Dismiss));
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px(px(17.))
                    .pt(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(11.))
                    .child(field("To", self.to.clone(), theme))
                    .child(field("Subject", self.subject.clone(), theme))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .border_1()
                            .border_color(theme.hairline)
                            .bg(theme.canvas)
                            .px(px(10.))
                            .py(px(8.))
                            .child(self.message.clone()),
                    ),
            )
            .child(
                div()
                    .h(px(46.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(15.))
                    .border_t_1()
                    .border_color(theme.hairline)
                    .child(
                        Button::new("compose-attach", "Attach")
                            .dense()
                            .icon(Icon::new("icons/paperclip.svg", 13., theme.faint))
                            .style(ButtonStyle::Ghost)
                            .disabled(true),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("compose-send", "Send ↗")
                            .dense()
                            .icon(Icon::new("icons/send.svg", 13., theme.faint))
                            .style(ButtonStyle::Accent)
                            .disabled(true),
                    ),
            )
    }
}

fn field(label: &'static str, field: Entity<TextField>, theme: Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.faint)
                .child(label),
        )
        .child(
            div()
                .h(px(28.))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme.hairline)
                .child(field),
        )
}
