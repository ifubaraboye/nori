use std::rc::Rc;

use gpui::{App, IntoElement, RenderOnce, Role, SharedString, Window, div, prelude::*, px};

use super::Icon;
use crate::theme::Theme;

type ToggleHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// Shared sidebar toggle, Waku-style 26px box with a 14px icon.
///
/// One instance lives in the sidebar header while the sidebar is visible;
/// the other lives in the top bar while it is hidden, so exactly one toggle
/// is on screen at any time.
#[derive(IntoElement)]
pub struct SidebarToggle {
    id: SharedString,
    on_toggle: ToggleHandler,
}

impl SidebarToggle {
    pub fn new(
        id: impl Into<SharedString>,
        on_toggle: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            on_toggle: Rc::new(on_toggle),
        }
    }
}

impl RenderOnce for SidebarToggle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::current(cx);
        let on_toggle = self.on_toggle.clone();
        let on_toggle_key = self.on_toggle.clone();
        let id = self.id.clone();

        div()
            .id(id.clone())
            .debug_selector(move || id.to_string())
            .size(px(26.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .role(Role::Button)
            .aria_label("Toggle sidebar")
            .focusable()
            .tab_stop(true)
            .focus_visible(|style| style.border_color(theme.focus))
            .hover(|style| style.bg(theme.hover))
            .active(|style| style.bg(theme.active))
            .child(Icon::new("icons/panel-left.svg", 14., theme.muted))
            .on_click(move |_event, window, cx| on_toggle(window, cx))
            .on_key_down(move |event, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    on_toggle_key(window, cx);
                }
            })
    }
}
