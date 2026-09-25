use gpui::{
    App, ClickEvent, IntoElement, RenderOnce, Role, SharedString, Stateful, Window, div,
    prelude::*, px, transparent_black,
};

use super::Icon;
use crate::theme::Theme;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub enum ButtonStyle {
    Ghost,
    Subtle,
    Accent,
}

#[derive(IntoElement)]
pub struct Button {
    id: SharedString,
    label: SharedString,
    icon: Option<Icon>,
    style: ButtonStyle,
    disabled: bool,
    dense: bool,
    padding: Option<gpui::Pixels>,
    aria_label: Option<SharedString>,
    focus_ring: bool,
    hover_fill: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            style: ButtonStyle::Ghost,
            disabled: false,
            dense: false,
            padding: None,
            aria_label: None,
            focus_ring: true,
            hover_fill: true,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn dense(mut self) -> Self {
        self.dense = true;
        self
    }

    /// Override the horizontal padding. Non-dense by default at 8px, which is
    /// sized for a label; an icon-only button wants less.
    pub fn px(mut self, padding: f32) -> Self {
        self.padding = Some(px(padding));
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    /// Drop the focus ring, for chrome buttons that read as icon-only
    /// affordances and look ringed while the pointer rests on them. The
    /// button stays focusable, tabbable, and labelled, so assistive tech
    /// still finds it; only the painted ring goes away.
    pub fn without_focus_ring(mut self) -> Self {
        self.focus_ring = false;
        self
    }

    /// Drop the hover and press fills, for icon-only chrome where a lit
    /// rectangle under the pointer reads as heavier than the icon itself.
    pub fn without_hover_fill(mut self) -> Self {
        self.hover_fill = false;
        self
    }

    pub fn on_click(
        mut self,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(on_click));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = Theme::dark();
        let aria_label = self.aria_label.unwrap_or_else(|| self.label.clone());
        let (background, hover, active, foreground, border) = match self.style {
            ButtonStyle::Ghost => (
                transparent_black(),
                theme.hover,
                theme.active,
                theme.muted,
                transparent_black(),
            ),
            ButtonStyle::Subtle => (
                theme.surface,
                theme.raised,
                theme.active,
                theme.text,
                theme.hairline_strong,
            ),
            ButtonStyle::Accent => (theme.text, theme.text, theme.text, theme.canvas, theme.text),
        };
        let height = if self.dense { px(22.) } else { px(28.) };
        let horizontal_padding = if self.dense { px(5.) } else { px(8.) };
        let icon_size = if self.dense { 12. } else { 14. };
        let padding = self.padding.unwrap_or(horizontal_padding);

        let mut element: Stateful<gpui::Div> = div()
            .id(self.id)
            .h(height)
            .px(padding)
            .flex()
            .items_center()
            .gap(px(if self.dense { 4. } else { 6. }))
            .border_1()
            .border_color(border)
            .bg(if self.disabled {
                theme.surface
            } else {
                background
            })
            .text_size(px(if self.dense { 11. } else { 12. }))
            .text_color(if self.disabled {
                theme.faint
            } else {
                foreground
            })
            .cursor(if self.disabled {
                gpui::CursorStyle::Arrow
            } else {
                gpui::CursorStyle::PointingHand
            })
            .role(Role::Button)
            .aria_label(aria_label)
            .focusable()
            .tab_stop(true)
            .when(self.focus_ring, |this| {
                this.focus_visible(|style| style.border_color(theme.focus))
            })
            .when(self.hover_fill, |this| {
                this.hover(|style| style.bg(if self.disabled { theme.surface } else { hover }))
                    .active(|style| style.bg(if self.disabled { theme.surface } else { active }))
            })
            .when(!self.disabled, |this| {
                this.cursor_pointer()
                    .when_some(self.on_click, |this, on_click| this.on_click(on_click))
            });

        if let Some(icon) = self.icon {
            element = element.child(crate::components::Icon::new(
                icon.path,
                if self.dense {
                    icon.size.min(icon_size)
                } else {
                    icon.size
                },
                if self.disabled {
                    theme.faint
                } else {
                    foreground
                },
            ));
        }
        if !self.label.is_empty() {
            element = element.child(self.label);
        }
        element
    }
}
