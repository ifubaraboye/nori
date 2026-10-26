//! The choice sheet shown when several results are downloaded together.

use gpui::{AnyElement, Context, Div, FontWeight, div, prelude::*, px};
use nori_core::torrent::BatchMode;

use crate::app::App;
use crate::icons;
use crate::theme::Theme;
use crate::widgets::{self, UI_FONT};

pub fn modal(app: &App, theme: &Theme, cx: &mut Context<App>) -> AnyElement {
    let count = app
        .batch_prompt
        .as_ref()
        .map_or(0, |prompt| prompt.rows.len());

    let header = div()
        .flex()
        .items_start()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.0)
                .min_w(px(0.))
                .gap(px(3.))
                .child(
                    div()
                        .font_family(UI_FONT)
                        .text_size(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(format!("Download {count} Selected")),
                )
                .child(widgets::text(
                    "Choose how this batch should start.",
                    11.,
                    theme.text_secondary,
                )),
        )
        .child(
            widgets::icon_button(theme, icons::X, 12., theme.text_secondary)
                .id("batch-close")
                .on_click(cx.listener(|this, _, _, cx| this.cancel_batch(cx))),
        );

    let card = div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .w(px(460.))
        .p(px(20.))
        .bg(theme.canvas)
        .border_1()
        .border_color(theme.border_strong)
        .shadow_lg()
        .id("batch-card")
        .occlude()
        .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
        .child(header)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(option(
                    theme,
                    widgets::primary_button(theme, "Start All")
                        .w_full()
                        .id("batch-start-all")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_batch(BatchMode::AllAtOnce, cx)
                        })),
                    "Start every selected torrent immediately.",
                ))
                .child(option(
                    theme,
                    widgets::button(theme, "Queue One At A Time")
                        .w_full()
                        .id("batch-queue")
                        .on_click(
                            cx.listener(|this, _, _, cx| this.confirm_batch(BatchMode::Queue, cx)),
                        ),
                    "Download in the order shown, the next starts when one finishes.",
                )),
        )
        .child(
            div().flex().items_center().justify_end().child(
                widgets::toolbar_button(theme, "Cancel", false)
                    .id("batch-cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_batch(cx))),
            ),
        );

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::hsla(0., 0., 0., 0.45))
        .id("batch-layer")
        .occlude()
        .on_click(cx.listener(|this, _, _, cx| this.cancel_batch(cx)))
        .child(card)
        .into_any_element()
}

fn option(theme: &Theme, button: impl IntoElement, description: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(button)
        .child(widgets::text(description, 10.5, theme.text_tertiary))
}
