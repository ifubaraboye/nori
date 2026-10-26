//! Small UI pieces built straight from the waku tokens.
//!
//! Everything is a plain `div`, so hover, focus and action wiring stay with
//! the view that owns the behaviour.

use gpui::{Div, FontWeight, Hsla, SharedString, div, prelude::*, px, relative};

use crate::icons;
use crate::theme::Theme;

pub const UI_FONT: &str = "Inter";
pub const MONO_FONT: &str = "JetBrainsMono Nerd Font";

pub fn text(label: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    let size = size + 1.0;
    div()
        .font_family(UI_FONT)
        .text_size(px(size))
        .text_color(color)
        .line_height(px((size * 1.45).ceil()))
        .child(label.into())
}

pub fn strong(label: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    text(label, size, color)
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
}

pub fn muted(label: impl Into<SharedString>, size: f32, theme: &Theme) -> Div {
    text(label, size, theme.text_secondary)
}

pub fn uppercase(label: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    let size = size + 1.0;
    div()
        .font_family(UI_FONT)
        .text_size(px(size))
        .text_color(color)
        .line_height(px(size * 1.45))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.into())
}

pub fn mono(label: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    let size = size + 1.0;
    div()
        .font_family(MONO_FONT)
        .text_size(px(size))
        .text_color(color)
        .line_height(px(size * 1.5))
        .whitespace_nowrap()
        .child(label.into())
}

/// A quiet button: the default affordance for everything that is not the
/// single primary action on a screen.
pub fn button(theme: &Theme, label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(10.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.raised)
        .text_size(px(12.))
        .text_color(theme.text_secondary)
        .font_family(UI_FONT)
        .cursor_pointer()
        .hover(|s| {
            s.bg(theme.overlay_strong)
                .text_color(theme.text)
                .border_color(theme.border_strong)
        })
        .child(label.into())
}

pub fn primary_button(theme: &Theme, label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(30.))
        .px(px(14.))
        .bg(theme.inverse)
        .text_size(px(12.5))
        .text_color(theme.on_inverse)
        .font_family(UI_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .child(label.into())
}

pub fn toolbar_button(theme: &Theme, label: impl Into<SharedString>, active: bool) -> Div {
    let color = if active {
        theme.accent
    } else {
        theme.text_tertiary
    };
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(24.))
        .px(px(8.))
        .text_size(px(11.5))
        .text_color(color)
        .font_family(UI_FONT)
        .cursor_pointer()
        .hover(move |s| {
            s.bg(theme.overlay_strong)
                .text_color(if active { theme.accent } else { theme.text })
        })
        .child(label.into())
}

pub fn icon_button(theme: &Theme, glyph: &'static [u8], size: f32, color: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.))
        .cursor_pointer()
        .hover(move |s| s.bg(theme.overlay_strong))
        .child(icons::icon(glyph, size, color))
}

pub fn segmented(theme: &Theme, options: &[(&'static str, bool)]) -> Div {
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(1.))
        .p(px(2.))
        .bg(theme.inset)
        .border_1()
        .border_color(theme.border);
    for (label, active) in options {
        let active = *active;
        let mut item = div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(24.))
            .px(px(10.))
            .text_size(px(11.5))
            .font_family(UI_FONT)
            .cursor_pointer();
        item = if active {
            item.bg(theme.raised)
                .text_color(theme.text)
                .font_weight(FontWeight::MEDIUM)
        } else {
            item.text_color(theme.text_tertiary)
                .hover(move |s| s.text_color(theme.text_secondary))
        };
        row = row.child(item.child(*label));
    }
    row
}

pub fn card(theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .gap(px(9.))
        .p(px(14.))
        .bg(theme.raised)
        .border_1()
        .border_color(theme.border)
}

pub fn section(theme: &Theme, title: &'static str, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .gap(px(8.))
        .child(uppercase(title, 13., theme.text_secondary))
        .child(card(theme).child(body))
}

pub fn divider(theme: &Theme) -> Div {
    div().h(px(1.)).w_full().bg(theme.border)
}

pub fn badge(theme: &Theme, label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(15.))
        .min_w(px(15.))
        .px(px(4.))
        .bg(theme.accent)
        .text_size(px(9.5))
        .text_color(theme.on_inverse)
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.into())
}

pub fn progress(theme: &Theme, fraction: f32) -> Div {
    let fraction = fraction.clamp(0.0, 1.0);
    let mut inner = div().h(px(3.)).bg(theme.accent);
    inner = if fraction <= 0.0 {
        inner.w(px(0.))
    } else {
        inner.w(relative(fraction))
    };
    div()
        .w_full()
        .h(px(3.))
        .bg(theme.border_strong)
        .overflow_hidden()
        .child(inner)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Off,
    On,
    Mixed,
}

impl From<bool> for CheckState {
    fn from(checked: bool) -> Self {
        if checked { Self::On } else { Self::Off }
    }
}

pub fn checkbox(theme: &Theme, checked: bool, label: impl Into<SharedString>) -> Div {
    checkbox_with_state(theme, checked.into(), label)
}

pub fn checkbox_with_state(
    theme: &Theme,
    state: CheckState,
    label: impl Into<SharedString>,
) -> Div {
    let (mark_bg, mark_border) = if state == CheckState::Off {
        (theme.canvas, theme.border_strong)
    } else {
        (theme.accent, theme.accent)
    };
    let mut mark = div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(15.))
        .border_1()
        .border_color(mark_border)
        .bg(mark_bg)
        .flex_shrink_0();
    match state {
        CheckState::On => mark = mark.child(icons::icon(icons::CHECK, 10., theme.on_inverse)),
        CheckState::Mixed => mark = mark.child(icons::icon(icons::MINUS, 10., theme.on_inverse)),
        CheckState::Off => {}
    }
    div()
        .flex()
        .items_center()
        .gap(px(9.))
        .py(px(3.))
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        .child(mark)
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(11.5))
                .text_color(theme.text)
                .line_height(px(17.))
                .child(label.into()),
        )
}

/// A compact way to change a small integer.
pub fn stepper_value(theme: &Theme, value: impl Into<SharedString>, width: f32) -> Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .w(px(width))
        .text_size(px(11.5))
        .text_color(theme.text)
        .font_family(UI_FONT)
        .child(value.into())
}

pub fn chevron_button(theme: &Theme, up: bool) -> Div {
    let glyph = if up {
        icons::CHEVRON_UP
    } else {
        icons::CHEVRON_DOWN
    };
    icon_button(theme, glyph, 12., theme.text_secondary).size(px(24.))
}
