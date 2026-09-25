use std::rc::Rc;

use gpui::{
    App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px, transparent_black,
};

use super::Icon;
use crate::model::EmailId;
use crate::theme::Theme;

type SelectTabHandler = Rc<dyn Fn(EmailId, &mut Window, &mut App) + 'static>;
type CloseTabHandler = Rc<dyn Fn(EmailId, &mut Window, &mut App) + 'static>;

/// Space between tabs. A tab is a pinned mail held open, so the gap is what
/// separates one held mail from the next rather than letting them read as a
/// run of text.
const TAB_GAP: f32 = 8.;

/// Close glyph size. The old 11px sat small against 12px tab text.
const CLOSE_GLYPH: f32 = 11. * 1.05;

#[derive(IntoElement)]
pub struct EmailTabs {
    tabs: Vec<(EmailId, SharedString)>,
    active: Option<EmailId>,
    theme: Theme,
    scroll_handle: gpui::ScrollHandle,
    on_select: SelectTabHandler,
    on_close: CloseTabHandler,
}

impl EmailTabs {
    pub fn new(
        tabs: Vec<(EmailId, SharedString)>,
        active: Option<EmailId>,
        theme: Theme,
        scroll_handle: gpui::ScrollHandle,
        on_select: impl Fn(EmailId, &mut Window, &mut App) + 'static,
        on_close: impl Fn(EmailId, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            tabs,
            active,
            theme,
            scroll_handle,
            on_select: Rc::new(on_select),
            on_close: Rc::new(on_close),
        }
    }
}

impl RenderOnce for EmailTabs {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let scroll_handle = self.scroll_handle;
        let on_select = self.on_select.clone();
        let on_close = self.on_close.clone();
        let tabs = self.tabs;
        let active = self.active;

        div()
            .id("email-tabs")
            .debug_selector(|| "email-tabs".into())
            .track_scroll(&scroll_handle)
            .h(px(40.))
            .w_full()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(TAB_GAP))
            .px(px(8.))
            // Darker than the content it sits above, so the active tab's
            // lighter surface has something to be lighter than.
            .bg(theme.inset)
            .border_b_1()
            .border_color(theme.hairline)
            .overflow_x_scroll()
            .children(tabs.into_iter().map(|(id, label)| {
                let is_active = active == Some(id);
                let on_select = on_select.clone();
                let on_close = on_close.clone();
                let close_id = id;
                div()
                    .id(("email-tab", id.0 as usize))
                    .h(px(28.))
                    .min_w(px(118.))
                    .max_w(px(220.))
                    // Square, and no drawn edge. The tab's depth comes from
                    // being a lighter surface on a darker strip, not from a
                    // border or a radius around a flat fill.
                    .rounded(px(0.))
                    .border_1()
                    .border_color(transparent_black())
                    .when(is_active, |this| this.shadow_sm())
                    .pl(px(10.))
                    // Distance from the tab's own right edge, so a smaller
                    // value pushes the × further right. 6px leaves a margin
                    // without parking the glyph in the middle of the tab.
                    .pr(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .bg(if is_active {
                        theme.raised
                    } else {
                        transparent_black()
                    })
                    .text_size(px(12.))
                    .text_color(if is_active { theme.text } else { theme.muted })
                    .cursor_pointer()
                    .hover(|style| style.bg(if is_active { theme.raised } else { theme.hover }))
                    .on_click(move |_event, window, cx| on_select(id, window, cx))
                    .child(div().min_w_0().flex_1().truncate().child(label.clone()))
                    .child(
                        // A bare glyph: no border and no hover fill, so the
                        // close affordance never draws a box around the ×.
                        div()
                            .id(format!("close-tab-{}", id.0))
                            .size(px(18.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .aria_label(format!("Close {}", label))
                            .on_click(move |_event, window, cx| {
                                cx.stop_propagation();
                                on_close(close_id, window, cx);
                            })
                            .child(Icon::new("icons/close.svg", CLOSE_GLYPH, theme.ghost)),
                    )
            }))
    }
}
