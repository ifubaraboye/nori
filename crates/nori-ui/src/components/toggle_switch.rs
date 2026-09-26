use std::rc::Rc;

use gpui::{
    App, CursorStyle, IntoElement, Role, SharedString, Toggled, Window, div, prelude::*, px,
    transparent_black,
};

use crate::theme::Theme;

/// Shared by the click and the key handler, so a switch can be operated by
/// pointer and by keyboard without cloning a closure.
type ToggleHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

const TRACK_WIDTH: f32 = 30.;
const TRACK_HEIGHT: f32 = 18.;
const KNOB_SIZE: f32 = 14.;
const INSET: f32 = 2.;

/// Settings switch: a 30x18 track with a 14px knob, on = accent track with
/// the knob parked right, off = neutral track with the knob parked left.
///
/// Exposed as a switch role with a toggled state, focusable, and togglable
/// with Enter or Space so it is not pointer-only.
#[derive(IntoElement)]
pub struct ToggleSwitch {
    id: SharedString,
    /// The id, also exposed as a debug selector so visual tests can find a
    /// switch by the same name they set on the element.
    debug_selector: SharedString,
    label: SharedString,
    on: bool,
    on_toggle: Option<ToggleHandler>,
}

impl ToggleSwitch {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>, on: bool) -> Self {
        let id = id.into();
        Self {
            debug_selector: id.clone(),
            id,
            label: label.into(),
            on,
            on_toggle: None,
        }
    }

    pub fn on_toggle(mut self, on_toggle: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(on_toggle));
        self
    }
}

impl RenderOnce for ToggleSwitch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::current(cx);
        let on = self.on;
        let on_toggle = self.on_toggle;

        div()
            .id(self.id)
            .debug_selector(move || self.debug_selector.to_string())
            .w(px(TRACK_WIDTH))
            .h(px(TRACK_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .justify_start()
            .when(on, |this| this.justify_end())
            .px(px(INSET))
            .rounded(px(TRACK_HEIGHT / 2.))
            .border_1()
            .border_color(if on {
                transparent_black()
            } else {
                theme.hairline_strong
            })
            .bg(if on { theme.accent } else { theme.surface })
            .cursor(CursorStyle::PointingHand)
            .role(Role::Switch)
            .aria_label(self.label.clone())
            .aria_toggled(if on { Toggled::True } else { Toggled::False })
            .focusable()
            .tab_stop(true)
            .focus_visible(|style| style.border_color(theme.focus))
            .hover(|style| style.bg(if on { theme.accent } else { theme.raised }))
            .when_some(on_toggle, |this, on_toggle| {
                let on_key = on_toggle.clone();
                this.on_click(move |_event, window, cx| on_toggle(window, cx))
                    .on_key_down(move |event, window, cx| {
                        if !event.keystroke.modifiers.modified()
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            on_key(window, cx);
                            cx.stop_propagation();
                        }
                    })
            })
            .child(
                div()
                    .size(px(KNOB_SIZE))
                    .flex_none()
                    .rounded(px(KNOB_SIZE / 2.))
                    .bg(if on { theme.canvas } else { theme.faint }),
            )
    }
}
