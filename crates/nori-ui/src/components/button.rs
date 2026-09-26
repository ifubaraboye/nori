use gpui::{
    App, ClickEvent, Hsla, IntoElement, RenderOnce, Role, SharedString, Stateful, Window, div,
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
    /// Multiplies every metric — height, padding, icon and text. The default
    /// of 1.0 keeps the shared dense size; a button that wants to read as
    /// slightly more substantial than its neighbours scales itself rather than
    /// making every other dense button grow with it.
    size_scale: f32,
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
            size_scale: 1.,
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

    /// Scale the whole button. `1.05` is a 5% larger button, gaps included.
    pub fn scaled(mut self, scale: f32) -> Self {
        self.size_scale = scale;
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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::current(cx);
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
            ButtonStyle::Accent => {
                // A filled accent button. The base is the accent itself, which
                // is what the name always implied — it used to fill with
                // `text` instead, which painted a near-white slab in dark mode
                // and a near-black one in light.
                // Hover and press step the accent's lightness so the button
                // still answers the pointer, which a same-colour pair would not.
                let lifted = Hsla {
                    l: (theme.accent.l * 1.08).min(1.),
                    ..theme.accent
                };
                let sunk = Hsla {
                    l: theme.accent.l * 0.92,
                    ..theme.accent
                };
                (theme.accent, lifted, sunk, theme.on_accent, theme.accent)
            }
        };
        let scale = self.size_scale;
        let height = px(if self.dense { 22. } else { 28. } * scale);
        let horizontal_padding = px(if self.dense { 5. } else { 8. } * scale);
        let icon_size = (if self.dense { 12. } else { 14. }) * scale;
        let padding = self.padding.unwrap_or(horizontal_padding);

        // `id` is `Copy`, but it is moved into the builder below, so read it
        // first for the selector.
        let debug_id = self.id.to_string();
        let mut element: Stateful<gpui::Div> = div()
            .id(self.id)
            // Every other element in the app exposes a debug selector, so a
            // button is findable by name in a visual test like the rest.
            .debug_selector(move || debug_id.clone())
            .h(height)
            .px(padding)
            .flex()
            .items_center()
            .gap(px((if self.dense { 4. } else { 6. }) * scale))
            .border_1()
            // A disabled button drops its border too, not just its fill. The
            // accent style's border *is* the accent, so a disabled accent
            // button was left wearing a bright outline around a muted label.
            .border_color(if self.disabled {
                transparent_black()
            } else {
                border
            })
            .bg(if self.disabled {
                theme.surface
            } else {
                background
            })
            .text_size(px((if self.dense { 11. } else { 12. }) * scale))
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
