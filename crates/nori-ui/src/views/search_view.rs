use gpui::{
    App, Context, Entity, EventEmitter, IntoElement, Render, Role, Subscription, Window, div,
    prelude::*, px,
};

use super::super::components::{Icon, TextField};
use crate::actions::{Dismiss, SearchMoveDown, SearchMoveUp, SearchOpenSelected};
use crate::model::{Email, EmailId};
use crate::theme::Theme;

pub enum SearchEvent {
    Open(EmailId),
    Dismiss,
    /// The user typed something new.
    ///
    /// Searching is Gmail's job, not Nori's, so the view does not answer this
    /// itself: it reports the text and waits to be handed results. Only when
    /// there is no account to ask does it fall back to filtering what it already
    /// holds.
    QueryChanged(String),
}

impl EventEmitter<SearchEvent> for SearchView {}

pub struct SearchView {
    query: Entity<TextField>,
    all_emails: Vec<Email>,
    /// Whether Gmail can be asked. False in the sample-data build, where the
    /// only thing to search is what is already on screen.
    remote: bool,
    /// Results from Gmail, filled in as they arrive.
    results: Vec<Email>,
    /// True between a query going out and the last of its results landing.
    searching: bool,
    /// Why the last search could not run, if it could not.
    failed: Option<String>,
    /// The text behind `results`. Kept so an arriving batch can be told apart
    /// from the answer to a query the user has already moved on from.
    shown_query: String,
    selected_index: usize,
    /// Kept so a change to the theme global repaints this view; an open search
    /// dialog would otherwise hold the old palette.
    _theme_sub: Subscription,
    /// Watches the text field. The field reports no edits of its own, so the
    /// content is diffed here instead.
    _query_sub: Subscription,
}

impl SearchView {
    pub fn new(
        all_emails: Vec<Email>,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| TextField::new("search-query", "Search mail", "", true, 1, cx));
        let query_focus = query.read(cx).focus_handle();
        window.focus(&query_focus, cx);
        let theme_sub = cx.observe_global::<Theme>(|_, cx| cx.notify());
        let query_sub = cx.observe(&query, |this, query, cx| {
            let text = query.read(cx).content().to_string();
            // An empty box is not a search. Leaving it empty clears the answer
            // rather than asking Gmail for everything.
            if text != this.shown_query && !text.trim().is_empty() {
                this.results.clear();
                this.searching = true;
                // A fresh query earns a fresh chance; yesterday's failure says
                // nothing about this one.
                this.failed = None;
            }
            this.shown_query = text.clone();
            this.selected_index = 0;
            cx.notify();
            if !text.trim().is_empty() {
                cx.emit(SearchEvent::QueryChanged(text));
            }
        });
        Self {
            query,
            all_emails,
            remote,
            results: Vec::new(),
            searching: false,
            failed: None,
            shown_query: String::new(),
            selected_index: 0,
            _theme_sub: theme_sub,
            _query_sub: query_sub,
        }
    }

    /// A search is still running.
    pub fn set_searching(&mut self, searching: bool, cx: &mut Context<Self>) {
        if self.searching != searching {
            self.searching = searching;
            cx.notify();
        }
    }

    /// The last search could not be run. Said plainly, because the alternative
    /// is an empty list that looks like an answer.
    pub fn set_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.searching = false;
        self.failed = Some(error);
        cx.notify();
    }

    /// Add hits, ignoring any that arrived after the user moved on.
    ///
    /// A slow request for `old` can land after the user has typed `new`, and
    /// appending it would put answers to the wrong question in the list.
    pub fn add_results(&mut self, query: &str, found: Vec<Email>, cx: &mut Context<Self>) {
        if query != self.shown_query {
            return;
        }
        self.results.extend(found);
        cx.notify();
    }

    /// The email behind a result, for handing to the reading view.
    pub fn email(&self, id: &EmailId) -> Option<Email> {
        self.results
            .iter()
            .chain(self.all_emails.iter())
            .find(|email| &email.id == id)
            .cloned()
    }

    fn results(&self, cx: &App) -> Vec<Email> {
        if self.remote {
            return self.results.clone();
        }
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
            cx.emit(SearchEvent::Open(email.id.clone()));
        }
    }

    fn dismiss(&mut self, _: &Dismiss, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SearchEvent::Dismiss);
    }
}

impl Render for SearchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
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
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("search-results-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_scroll()
                            .children(results.iter().enumerate().map(|(index, email)| {
                                let id = email.id.clone();
                                let selected = index == selected_index;
                                let result_entity = entity.clone();
                                let open_id = id.clone();
                                div()
                                    .id(format!("search-result-{id}"))
                                    .debug_selector(move || format!("search-result-{id}"))
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
                                        result_entity.update(cx, |_, cx| {
                                            cx.emit(SearchEvent::Open(open_id.clone()))
                                        });
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
                    // A remote search is a round trip, and a list that stays
                    // blank while it happens reads as a failed search rather
                    // than a slow one. One quiet line says which it is.
                    .when(self.remote && !self.shown_query.trim().is_empty(), |this| {
                        let label = if let Some(error) = &self.failed {
                            format!("Search failed: {error}")
                        } else if self.searching {
                            match self.results.len() {
                                0 => "Searching all mail".to_string(),
                                found => format!("{found} found · searching"),
                            }
                        } else if self.results.is_empty() {
                            "Nothing in Gmail matches".to_string()
                        } else {
                            format!("{} from Gmail", self.results.len())
                        };
                        this.child(
                            div()
                                .flex_none()
                                .h(px(34.))
                                .flex()
                                .items_center()
                                .px(px(18.))
                                .border_t_1()
                                .border_color(theme.hairline)
                                .text_size(px(10.5))
                                .text_color(theme.faint)
                                .child(label),
                        )
                    }),
            )
    }
}
