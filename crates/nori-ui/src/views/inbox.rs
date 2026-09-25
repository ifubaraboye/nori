use std::rc::Rc;

use gpui::{
    App, FocusHandle, IntoElement, ListSizingBehavior, RenderOnce, Role, ScrollStrategy,
    UniformListScrollHandle, Window, div, prelude::*, uniform_list,
};

use crate::components::EmailRow;
use crate::model::{Density, EmailId, EmailSummary};
use crate::theme::Theme;

type InboxOpenHandler = Rc<dyn Fn(EmailId, &mut Window, &mut App) + 'static>;
type InboxStarHandler = Rc<dyn Fn(EmailId, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct Inbox {
    rows: Vec<EmailSummary>,
    selected_index: usize,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    density: Density,
    theme: Theme,
    on_open: InboxOpenHandler,
    on_star: InboxStarHandler,
}

impl Inbox {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rows: Vec<EmailSummary>,
        selected_index: usize,
        focus_handle: FocusHandle,
        scroll_handle: UniformListScrollHandle,
        density: Density,
        theme: Theme,
        on_open: impl Fn(EmailId, &mut Window, &mut App) + 'static,
        on_star: impl Fn(EmailId, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            rows,
            selected_index,
            focus_handle,
            scroll_handle,
            density,
            theme,
            on_open: Rc::new(on_open),
            on_star: Rc::new(on_star),
        }
    }
}

impl RenderOnce for Inbox {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let Self {
            rows,
            selected_index,
            focus_handle,
            scroll_handle,
            density,
            theme,
            on_open,
            on_star,
        } = self;
        let list = uniform_list("email-list", rows.len(), move |range, _window, _cx| {
            range
                .map(|index| {
                    let row = rows[index].clone();
                    let selected = index == selected_index;
                    let id = row.id;
                    let open = on_open.clone();
                    let star = on_star.clone();
                    EmailRow::new(
                        row,
                        selected,
                        density,
                        theme,
                        move |window, cx| open(id, window, cx),
                        move |window, cx| star(id, window, cx),
                    )
                })
                .collect()
        })
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .track_scroll(&scroll_handle)
        .flex_1()
        .min_h_0()
        .w_full()
        .debug_selector(|| "email-list".into());

        div()
            .id("inbox")
            .key_context("Inbox")
            .track_focus(&focus_handle)
            .tab_index(0)
            .focus_visible(|style| style.border_color(theme.focus))
            .role(Role::List)
            .aria_label("Email list")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme.canvas)
            .child(list)
    }
}

pub fn scroll_selected_into_view(scroll_handle: &UniformListScrollHandle, selected_index: usize) {
    scroll_handle.scroll_to_item(selected_index, ScrollStrategy::Nearest);
}
