use std::rc::Rc;

use gpui::{
    App, FocusHandle, IntoElement, ListSizingBehavior, Pixels, Point, RenderOnce, Role,
    ScrollStrategy, UniformListScrollHandle, Window, div, prelude::*, uniform_list,
};

use crate::components::EmailRow;
use crate::model::{Density, EmailId, EmailSummary, Label};
use crate::theme::Theme;

type InboxOpenHandler = Rc<dyn Fn(EmailId, &mut Window, &mut App) + 'static>;
type InboxStarHandler = Rc<dyn Fn(EmailId, &mut Window, &mut App) + 'static>;
type InboxMenuHandler = Rc<dyn Fn(EmailId, Point<Pixels>, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct Inbox {
    rows: Vec<EmailSummary>,
    /// The labels each row carries, resolved by the caller in row order.
    /// Kept beside the summaries rather than inside them so mailbox state
    /// and label state never disturb each other.
    labels: Vec<Vec<Label>>,
    selected_index: usize,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    density: Density,
    on_open: InboxOpenHandler,
    on_star: InboxStarHandler,
    on_open_menu: InboxMenuHandler,
}

impl Inbox {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rows: Vec<EmailSummary>,
        labels: Vec<Vec<Label>>,
        selected_index: usize,
        focus_handle: FocusHandle,
        scroll_handle: UniformListScrollHandle,
        density: Density,
        on_open: impl Fn(EmailId, &mut Window, &mut App) + 'static,
        on_star: impl Fn(EmailId, &mut Window, &mut App) + 'static,
        on_open_menu: impl Fn(EmailId, Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            rows,
            labels,
            selected_index,
            focus_handle,
            scroll_handle,
            density,
            on_open: Rc::new(on_open),
            on_star: Rc::new(on_star),
            on_open_menu: Rc::new(on_open_menu),
        }
    }
}

impl RenderOnce for Inbox {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::current(cx);
        let Self {
            rows,
            labels,
            selected_index,
            focus_handle,
            scroll_handle,
            density,
            on_open,
            on_star,
            on_open_menu,
        } = self;
        let list = uniform_list("email-list", rows.len(), move |range, _window, _cx| {
            range
                .map(|index| {
                    let row = rows[index].clone();
                    let row_labels = labels.get(index).cloned().unwrap_or_default();
                    let selected = index == selected_index;
                    // Three handlers each need their own id, so the row's is
                    // cloned per handler rather than copied implicitly.
                    let id = row.id.clone();
                    let open = on_open.clone();
                    let star = on_star.clone();
                    let menu = on_open_menu.clone();
                    let open_id = id.clone();
                    let star_id = id.clone();
                    EmailRow::new(
                        row,
                        selected,
                        density,
                        row_labels,
                        move |window, cx| open(open_id.clone(), window, cx),
                        move |window, cx| star(star_id.clone(), window, cx),
                        move |position, window, cx| menu(id.clone(), position, window, cx),
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
            .min_w_0()
            .flex()
            .flex_col()
            .bg(theme.canvas)
            .child(list)
    }
}

pub fn scroll_selected_into_view(scroll_handle: &UniformListScrollHandle, selected_index: usize) {
    scroll_handle.scroll_to_item(selected_index, ScrollStrategy::Nearest);
}
