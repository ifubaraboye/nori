use gpui::{
    App, Context, Entity, EventEmitter, IntoElement, Render, Role, Window, div, prelude::*, px,
};

use super::super::components::{Icon, TextField};
use crate::actions::{Dismiss, SearchMoveDown, SearchMoveUp, SearchOpenSelected};
use crate::model::{Email, EmailId};
use crate::theme::Theme;

pub enum SearchEvent {
    Open(EmailId),
    Dismiss,
}

impl EventEmitter<SearchEvent> for SearchView {}

pub struct SearchView {
    query: Entity<TextField>,
    all_emails: Vec<Email>,
    selected_index: usize,
    theme: Theme,
}

impl SearchView {
    pub fn new(all_emails: Vec<Email>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| TextField::new("search-query", "Search mail", "", true, 1, cx));
        let query_focus = query.read(cx).focus_handle();
        window.focus(&query_focus, cx);
        Self {
            query,
            all_emails,
            selected_index: 0,
            theme: Theme::dark(),
        }
    }

    fn results(&self, cx: &App) -> Vec<Email> {
        let query = self.query.read(cx).content();
        self.all_emails
            .iter()
            .filter(|email| email.matches(query))
            .cloned()
            .collect()
    }

    fn move_down(&mut self, _: &SearchMoveDown, _window: &mut Window, cx: &mut Context<Self>) {
        let count = self.results(cx).len();
        if count > 0 {
            self.selected_index = (self.selected_index + 1) % count;
            cx.notify();
        }
    }

    fn move_up(&mut self, _: &SearchMoveUp, _window: &mut Window, cx: &mut Context<Self>) {
        let count = self.results(cx).len();
        if count > 0 {
            self.selected_index = (self.selected_index + count - 1) % count;
            cx.notify();
        }
    }

    fn open_selected(
        &mut self,
        _: &SearchOpenSelected,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(email) = self.results(cx).get(self.selected_index) {
            cx.emit(SearchEvent::Open(email.id));
        }
    }

    fn dismiss(&mut self, _: &Dismiss, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SearchEvent::Dismiss);
    }
}

impl Render for SearchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let results = self.results(cx);
        let selected_index = self.selected_index.min(results.len().saturating_sub(1));
        let entity = cx.entity();

        div()
            .id("search-dialog")
            .debug_selector(|| "search-dialog".into())
            .key_context("Search")
            .role(Role::Dialog)
            .aria_label("Search mail")
            .w(px(680.))
            .max_w_full()
            .h(px(430.))
            .flex()
            .flex_col()
            .bg(theme.raised)
            .border_1()
            .border_color(theme.hairline_strong)
            .shadow_lg()
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::open_selected))
            .on_action(cx.listener(Self::dismiss))
            .child(
                div()
                    .h(px(60.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(18.))
                    .border_b_1()
                    .border_color(theme.hairline)
                    .child(Icon::new("icons/search.svg", 16., theme.muted))
                    .child(self.query.clone())
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(3.))
                            .text_size(px(10.5))
                            .text_color(theme.faint)
                            .child("Esc"),
                    ),
            )
            .child(
                div()
                    .id("search-results-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .children(results.iter().enumerate().map(|(index, email)| {
                        let id = email.id;
                        let selected = index == selected_index;
                        let result_entity = entity.clone();
                        div()
                            .id(("search-result", id.0 as usize))
                            .debug_selector(move || format!("search-result-{}", id.0))
                            .min_h(px(54.))
                            .mx(px(6.))
                            .mt(px(2.))
                            .px(px(8.))
                            .py(px(7.))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .border_1()
                            .border_color(if selected {
                                theme.hairline_strong
                            } else {
                                theme.raised
                            })
                            .bg(if selected {
                                theme.selected_layer
                            } else {
                                theme.raised
                            })
                            .hover(|style| style.bg(theme.hover))
                            .cursor_pointer()
                            .on_click(move |_event, _window, cx| {
                                cx.stop_propagation();
                                result_entity.update(cx, |_, cx| cx.emit(SearchEvent::Open(id)));
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .truncate()
                                            .child(email.sender.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(12.))
                                            .text_color(theme.muted)
                                            .truncate()
                                            .child(email.subject.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(theme.ghost)
                                            .child(email.timestamp.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme.faint)
                                    .truncate()
                                    .child(email.preview.clone()),
                            )
                    })),
            )
    }
}
