//! The result list and the detail pane.
//!
//! The list is the screen's centre of gravity: rows stay flat, colour marks
//! health and the source, and the detail pane only appears once a result is
//! chosen so the rows never have to share the window with an empty panel.

use std::ops::Range;

use gpui::{
    AnyElement, Context, CursorStyle, Div, Entity, FontWeight, Hsla, MouseButton, MouseMoveEvent,
    SharedString, TextAlign, Window, div, list, prelude::*, px, uniform_list,
};
use nori_core::search::model::{source_by_id, source_tag};
use nori_core::search::{SearchResult, human_age, human_size};
use nori_core::torrent::ItemFile;

use crate::app::App;
use crate::icons;
use crate::theme::{Theme, source_tint};
use crate::widgets::{self, UI_FONT};

pub fn view(
    app: &App,
    theme: &Theme,
    width: f32,
    _window: &mut Window,
    cx: &mut Context<App>,
) -> AnyElement {
    let rows = app.pick();
    if width < 900. {
        if app.detail_open && !rows.is_empty() {
            return compact_detail(app, theme, width, cx);
        }
        return results_pane(app, theme, width, cx);
    }
    if rows.is_empty() {
        return results_pane(app, theme, width, cx);
    }
    let detail_width = constrained_detail_width(app.detail_width, width);
    div()
        .flex()
        .items_stretch()
        .size_full()
        .min_h(px(0.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.0)
                .min_w(px(0.))
                .child(results_pane(app, theme, width, cx)),
        )
        .child(
            div()
                .id("detail-resize-handle")
                .w(px(6.))
                .h_full()
                .flex_shrink_0()
                .cursor(CursorStyle::ResizeLeftRight)
                .flex()
                .justify_center()
                .child(div().w(px(1.)).h_full().bg(theme.border))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.detail_resizing = true;
                        cx.notify();
                    }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .w(px(detail_width))
                .flex_shrink_0()
                .child(detail_pane(app, theme, width, cx)),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
            if !this.detail_resizing || !event.dragging() {
                return;
            }
            let window_width = window.bounds().size.width.as_f32();
            let max_width = (window_width * 0.7).min(window_width - 420.).max(280.);
            this.detail_width = (window_width - event.position.x.as_f32()).clamp(280., max_width);
            cx.notify();
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.detail_resizing {
                    this.detail_resizing = false;
                    cx.notify();
                }
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.detail_resizing {
                    this.detail_resizing = false;
                    cx.notify();
                }
            }),
        )
        .into_any_element()
}

fn constrained_detail_width(requested: f32, window_width: f32) -> f32 {
    let max_width = (window_width * 0.7).min(window_width - 420.).max(280.);
    requested.clamp(280., max_width)
}

fn compact_detail(app: &App, theme: &Theme, width: f32, cx: &mut Context<App>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .size_full()
        .min_h(px(0.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(42.))
                .px(px(12.))
                .flex_shrink_0()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    widgets::button(theme, "Back")
                        .h(px(24.))
                        .px(px(10.))
                        .id("compact-back")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.detail_open = false;
                            this.focus_list(window, cx);
                            cx.notify();
                        })),
                )
                .child(widgets::strong("Details", 12., theme.text)),
        )
        .child(detail_pane(app, theme, width, cx))
        .into_any_element()
}

fn results_pane(app: &App, theme: &Theme, width: f32, cx: &mut Context<App>) -> AnyElement {
    let side = if width >= 1020. { 196. } else { 54. };
    let detail = if width >= 900. && !app.pick().is_empty() {
        constrained_detail_width(app.detail_width, width) + 6.
    } else {
        0.
    };
    let results_width = width - side - detail;

    let mut column = div()
        .flex()
        .flex_col()
        .size_full()
        .min_h(px(0.))
        .child(toolbar(app, theme, results_width, width, cx));

    if app.show_filter {
        column = column.child(
            div()
                .flex()
                .items_center()
                .px(px(16.))
                .py(px(6.))
                .flex_shrink_0()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .flex_grow(1.0)
                        .h(px(28.))
                        .px(px(9.))
                        .bg(theme.inset)
                        .border_1()
                        .border_color(theme.border)
                        .text_color(theme.text)
                        .child(app.filter_input.clone()),
                ),
        );
    }

    let content: AnyElement = if app.shown.is_empty() {
        empty_results(app, theme, cx)
    } else {
        let count = app.shown.len();
        let theme = *theme;
        let compact = width < 900.;
        uniform_list(
            "results",
            count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                let mut rows = Vec::with_capacity(range.len());
                for index in range {
                    let Some(result) = this.shown.get(index).cloned() else {
                        continue;
                    };
                    rows.push(result_row(this, &theme, index, &result, compact, cx));
                }
                rows
            }),
        )
        .flex_grow(1.0)
        .w_full()
        .min_h(px(0.))
        .track_scroll(&app.result_scroll)
        .into_any_element()
    };

    column.child(content).into_any_element()
}

fn toolbar(
    app: &App,
    theme: &Theme,
    results_width: f32,
    window_width: f32,
    cx: &mut Context<App>,
) -> Div {
    let compact = results_width < 580.;
    let outer_pad = if compact { 12. } else { 16. };

    let mut bar = div()
        .flex()
        .items_center()
        .gap(px(if compact { 6. } else { 10. }))
        .h(px(40.))
        .px(px(outer_pad))
        .flex_shrink_0()
        .border_b_1()
        .border_color(theme.border);

    if app.loading > 0 {
        bar = bar.child(widgets::text("Searching", 11., theme.text_secondary));
    } else {
        let chosen = app
            .shown
            .iter()
            .filter(|result| app.chosen.contains(&result.info_hash))
            .count();
        let check_state = if chosen == 0 {
            widgets::CheckState::Off
        } else if chosen == app.shown.len() {
            widgets::CheckState::On
        } else {
            widgets::CheckState::Mixed
        };
        if !app.shown.is_empty() {
            bar = bar.child(
                widgets::checkbox_with_state(theme, check_state, "")
                    .id("select-all-results")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_all_results(window, cx);
                    })),
            );
        }
        let plural = if app.shown.len() == 1 { "" } else { "s" };
        bar = bar.child(widgets::strong(
            format!("{} result{plural}", app.shown.len()),
            11.,
            theme.text,
        ));
    }

    bar = bar.child(div().flex_grow(1.0));

    if !app.chosen.is_empty() {
        bar = bar.child(widgets::text(
            format!("{} selected", app.chosen.len()),
            11.,
            theme.accent,
        ));
    }

    if !app.shown.is_empty() && !app.pick().is_empty() && !app.detail_open && window_width < 900. {
        bar = bar.child(
            widgets::toolbar_button(theme, "Details", false)
                .id("toolbar-details")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.detail_open = true;
                    cx.notify();
                })),
        );
    }

    bar = bar
        .child(
            widgets::toolbar_button(theme, "Filter", app.show_filter)
                .id("toolbar-filter")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.show_filter = !this.show_filter;
                    if this.show_filter {
                        this.focus_filter(window, cx);
                    }
                    cx.notify();
                })),
        )
        .child(
            widgets::toolbar_button(
                theme,
                if !compact && app.sort.is_active() {
                    app.sort.label()
                } else {
                    "Sort".to_string()
                },
                app.sort_open || app.sort.is_active(),
            )
            .id("toolbar-sort")
            .on_click(cx.listener(|this, _, window, cx| {
                if this.sort_open {
                    this.close_sort_menu();
                } else {
                    this.sort_open = true;
                    this.sort_submenu = None;
                    this.sort_menu_at = Some(window.mouse_position());
                }
                cx.notify();
            })),
        );

    bar
}

fn result_row(
    app: &App,
    theme: &Theme,
    index: usize,
    result: &SearchResult,
    compact: bool,
    cx: &mut Context<App>,
) -> AnyElement {
    let pad = if compact { 12. } else { 16. };
    let preview_result = result.clone();
    let selected = app.selected == Some(index);
    let chosen = app.chosen.contains(&result.info_hash);
    let active = selected || chosen;

    let mut meta: Vec<(String, Hsla)> = vec![
        (source_tag(&result.source).to_owned(), theme.text_tertiary),
        (human_size(result.size_bytes), theme.text_tertiary),
    ];
    if result.num_files > 0 {
        let plural = if result.num_files == 1 { "" } else { "s" };
        meta.push((
            format!("{} file{plural}", result.num_files),
            theme.text_tertiary,
        ));
    }
    let age = human_age(result.added);
    if !age.is_empty() {
        meta.push((age, theme.text_tertiary));
    }
    if app.reports_health(&result.source) {
        let seeders = if result.seeders == 1 {
            "Seeder"
        } else {
            "Seeders"
        };
        let leechers = if result.leechers == 1 {
            "Leecher"
        } else {
            "Leechers"
        };
        meta.push((format!("{} {seeders}", result.seeders), theme.success));
        meta.push((format!("{} {leechers}", result.leechers), theme.danger));
    }
    let meta: Vec<(SharedString, Hsla)> = meta
        .into_iter()
        .map(|(text, color)| (text.into(), color))
        .collect();

    let mut row = div()
        .flex()
        .items_center()
        .w_full()
        .gap(px(10.))
        .h(px(50.))
        .px(px(pad))
        .cursor_pointer()
        .child({
            let hash = result.info_hash.clone();
            widgets::checkbox(theme, chosen, "")
                .id(("result-check", index))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.focus_list(window, cx);
                    if let Some(result) = this
                        .shown
                        .iter()
                        .find(|result| result.info_hash == hash)
                        .cloned()
                    {
                        this.toggle_chosen(&result);
                    }
                    this.selected = None;
                    this.reset_detail_expansion();
                    this.refresh();
                    this.request_preview_for_pick(cx);
                    cx.notify();
                    cx.stop_propagation();
                }))
        })
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.0)
                .min_w(px(0.))
                .gap(px(2.))
                .child(crate::app::result_title(result.name.clone(), theme))
                .child(crate::app::result_meta(&meta, theme)),
        );

    row = if active {
        row.bg(theme.row_selected())
    } else {
        row.hover(move |style| style.bg(theme.row_hover()))
    };

    row.id(("result", index))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.focus_list(window, cx);
            this.clear_chosen();
            this.reset_detail_expansion();
            this.selected = Some(index);
            this.request_preview(&preview_result, cx);
            if compact {
                this.detail_open = true;
            }
            this.refresh();
            cx.notify();
        }))
        .into_any_element()
}

fn searching_view(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .size_full()
        .p(px(32.))
        .child(icons::icon(icons::SEARCH, 26., theme.text_tertiary))
        .child(widgets::strong("Asking the sources...", 15., theme.text))
        .into_any_element()
}

fn empty_results(app: &App, theme: &Theme, cx: &mut Context<App>) -> AnyElement {
    if app.loading > 0 {
        return searching_view(theme);
    }

    let query = app.search_input.read(cx).text().trim().to_owned();
    let browse_latest = app.results.is_empty() && query.is_empty();

    let mut body = div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .size_full()
        .p(px(32.))
        .child(icons::icon(icons::SEARCH, 28., theme.text_tertiary))
        .child(widgets::strong("Nothing here yet", 15., theme.text))
        .child(
            widgets::text(
                "Type what you are looking for, or press Browse latest to see what the sources have now.",
                12.,
                theme.text_secondary,
            )
            .text_align(TextAlign::Center)
            .max_w(px(360.)),
        );

    if browse_latest {
        body = body.child(
            widgets::primary_button(theme, "Browse latest")
                .id("browse-latest")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.start_search(cx);
                    this.focus_list(window, cx);
                })),
        );
    }

    body.into_any_element()
}

fn detail_pane(app: &App, theme: &Theme, width: f32, cx: &mut Context<App>) -> AnyElement {
    let rows = app.pick();
    if rows.len() > 1 {
        return multi_detail_pane(app, theme, width, rows, cx);
    }

    let mut pane = div().flex().flex_col().size_full().min_h(px(0.)).p(px(18.));

    if rows.is_empty() {
        return pane
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(icons::icon(icons::LINK, 24., theme.text_tertiary))
            .child(widgets::strong("Select a result", 13., theme.text))
            .child(widgets::text(
                "Its details and actions show here.",
                11.,
                theme.text_secondary,
            ))
            .child(widgets::text(
                "Tick a result's box to choose it.",
                11.,
                theme.text_secondary,
            ))
            .child(widgets::text(
                "Tick several, then press d to download them together.",
                11.,
                theme.text_secondary,
            ))
            .into_any_element();
    }

    let result = rows[0].clone();
    let count = rows.len();

    let title_row = div().flex().items_start().gap(px(8.)).child(
        div()
            .flex_grow(1.0)
            .min_w(px(0.))
            .font_family(UI_FONT)
            .text_size(px(17.))
            .line_height(px(23.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.text)
            .child(result.name.clone()),
    );

    let mut header = div()
        .flex()
        .items_center()
        .justify_between()
        .child(widgets::uppercase("Details", 11., theme.text_tertiary));
    if width >= 900. {
        header = header.child(
            widgets::icon_button(theme, icons::X, 13., theme.text_secondary)
                .id("detail-close")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_detail(cx);
                    this.focus_list(window, cx);
                })),
        );
    }

    let mut source_row = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(source_tint(&result.source, theme))
                .child(source_tag(&result.source)),
        )
        .child(widgets::text(
            format!("· {}", human_size(result.size_bytes)),
            11.,
            theme.text_secondary,
        ));
    let age = human_age(result.added);
    if !age.is_empty() {
        source_row = source_row.child(widgets::text(format!("· {age}"), 11., theme.text_secondary));
    }

    let stats_card = result_stats(theme, &result, app.reports_health(&result.source));

    let label = if count > 1 {
        format!("Download {count}")
    } else {
        "Download".to_owned()
    };

    let home = source_by_id(&result.source).map(|source| source.home);

    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .max_w(px(620.))
        .child(header)
        .child(title_row);
    if count > 1 {
        body = body.child(widgets::text(
            format!("{count} Selected"),
            12.,
            theme.accent,
        ));
    }
    body = body
        .child(source_row)
        .child(stats_card)
        .child(contents_preview(app, theme, &result, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    widgets::primary_button(theme, label)
                        .w_full()
                        .id("detail-download")
                        .on_click(cx.listener(|this, _, _, cx| this.download_picked(cx))),
                )
                .child(
                    widgets::button(theme, "Copy Magnet")
                        .w_full()
                        .id("detail-copy")
                        .on_click(cx.listener(|this, _, _, cx| this.copy_picked(cx))),
                )
                .child(
                    widgets::button(theme, "View Source")
                        .w_full()
                        .id("detail-open-source")
                        .on_click(cx.listener(move |_this, _, _, cx| {
                            if let Some(home) = home {
                                cx.open_url(home);
                            }
                        })),
                ),
        );

    pane = pane.child(
        div()
            .id("detail-scroll")
            .flex_grow(1.0)
            .min_h(px(0.))
            .overflow_y_scroll()
            .child(body),
    );
    pane.into_any_element()
}

/// Two or more checked results become a compact accordion: every torrent is
/// visible by its title and swarm summary, and opening one reveals the full
/// stats card and infohash without losing the rest of the selection.
fn multi_detail_pane(
    app: &App,
    theme: &Theme,
    width: f32,
    rows: Vec<SearchResult>,
    cx: &mut Context<App>,
) -> AnyElement {
    let count = rows.len();
    if app.detail_list.item_count() != count {
        app.detail_list.reset(count);
    }

    let entity = cx.entity();
    let card_theme = *theme;
    let open_hash = if app.detail_touched {
        app.detail_expanded.clone()
    } else {
        rows.first().map(|result| result.info_hash.clone())
    };
    let health: Vec<bool> = rows
        .iter()
        .map(|result| app.reports_health(&result.source))
        .collect();
    let preview_hash = app.preview_hash.clone();
    let preview_files = app.preview_files.clone();
    let preview_loading = app.preview_loading;
    let preview_error = app.preview_error.clone();

    let mut pane = div()
        .flex()
        .flex_col()
        .size_full()
        .min_h(px(0.))
        .p(px(18.))
        .gap(px(12.));

    let mut header = div()
        .flex()
        .items_center()
        .justify_between()
        .flex_shrink_0()
        .child(widgets::uppercase("Details", 11., theme.text_tertiary));
    if width >= 900. {
        header = header.child(
            widgets::icon_button(theme, icons::X, 13., theme.text_secondary)
                .id("detail-close")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_detail(cx);
                    this.focus_list(window, cx);
                })),
        );
    }

    let cards = list(app.detail_list.clone(), move |index, _, _cx| {
        let Some(result) = rows.get(index) else {
            return div().into_any_element();
        };
        let open = open_hash.as_deref() == Some(result.info_hash.as_str());
        let mut row = div().w_full().child(selected_result_card(
            &card_theme,
            entity.clone(),
            index,
            result,
            open,
            health.get(index).copied().unwrap_or(true),
            preview_hash.as_deref() == Some(result.info_hash.as_str()),
            &preview_files,
            preview_loading,
            preview_error.as_deref(),
        ));
        if index + 1 < count {
            row = row.pb(px(8.));
        }
        row.into_any_element()
    })
    .size_full()
    .min_h(px(0.))
    .max_w(px(620.));

    let actions = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .flex_shrink_0()
        .pt(px(12.))
        .border_t_1()
        .border_color(theme.border)
        .child(
            widgets::primary_button(theme, format!("Download All"))
                .w_full()
                .id("detail-download")
                .on_click(cx.listener(|this, _, _, cx| this.download_picked(cx))),
        )
        .child(
            widgets::button(theme, "Copy Magnets")
                .w_full()
                .id("detail-copy")
                .on_click(cx.listener(|this, _, _, cx| this.copy_picked(cx))),
        );

    pane.child(header)
        .child(div().flex_grow(1.0).min_h(px(0.)).w_full().child(cards))
        .child(actions)
        .into_any_element()
}

fn selected_result_card(
    theme: &Theme,
    entity: Entity<App>,
    index: usize,
    result: &SearchResult,
    open: bool,
    reports_health: bool,
    preview_matches: bool,
    preview_files: &[ItemFile],
    preview_loading: bool,
    preview_error: Option<&str>,
) -> Div {
    let hash = result.info_hash.clone();
    let toggle_hash = hash.clone();

    let mut summary: Vec<(SharedString, Hsla)> = vec![
        (
            source_tag(&result.source).to_owned().into(),
            source_tint(&result.source, theme),
        ),
        (human_size(result.size_bytes).into(), theme.text_tertiary),
    ];
    let age = human_age(result.added);
    if !age.is_empty() {
        summary.push((age.into(), theme.text_tertiary));
    }

    let health: Vec<(SharedString, Hsla)> = if reports_health {
        vec![
            (
                format!("{} Seeders", count_text(result.seeders)).into(),
                theme.success,
            ),
            (
                format!("{} Leechers", count_text(result.leechers)).into(),
                theme.danger,
            ),
        ]
    } else {
        vec![
            ("Seeders —".into(), theme.text_secondary),
            ("Leechers —".into(), theme.text_secondary),
        ]
    };

    let mut header = div()
        .id(("detail-card", index))
        .flex()
        .items_center()
        .w_full()
        .gap(px(9.))
        .p(px(12.))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.overlay))
        .on_click(move |_, _, cx| {
            entity.update(cx, |this, cx| {
                this.toggle_detail_result(&toggle_hash, open, cx);
            });
        })
        .child(icons::icon(
            if open {
                icons::CHEVRON_UP
            } else {
                icons::CHEVRON_DOWN
            },
            12.,
            theme.text_tertiary,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.0)
                .min_w(px(0.))
                .gap(px(2.))
                .child(if open {
                    widgets::strong(result.name.clone(), 12.5, theme.text)
                        .whitespace_normal()
                        .w_full()
                } else {
                    widgets::strong(result.name.clone(), 12.5, theme.text)
                        .truncate()
                        .w_full()
                })
                .child(crate::app::result_meta(&summary, theme))
                .child(crate::app::result_meta(&health, theme)),
        );
    if open {
        header = header.bg(theme.inset);
    }

    let mut card = div()
        .flex()
        .flex_col()
        .w_full()
        .border_1()
        .border_color(theme.border)
        .child(header);
    if open {
        let preview = expanded_contents_preview(
            theme,
            result,
            preview_matches,
            preview_files,
            preview_loading,
            preview_error,
        );
        card = card.child(
            div()
                .p(px(12.))
                .border_t_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(result_stats(theme, result, reports_health))
                        .child(preview),
                ),
        );
    }
    card
}

fn expanded_contents_preview(
    theme: &Theme,
    result: &SearchResult,
    preview_matches: bool,
    files: &[ItemFile],
    loading: bool,
    error: Option<&str>,
) -> Div {
    let mut preview = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .pt(px(10.))
        .border_t_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(widgets::uppercase(
                    "Torrent Contents",
                    9.,
                    theme.text_tertiary,
                ))
                .child(widgets::text(
                    if preview_matches && !files.is_empty() {
                        format!(
                            "{} files · {}",
                            files.len(),
                            human_size(files.iter().map(|f| f.size).sum())
                        )
                    } else {
                        format!("{} files", count_text(result.num_files))
                    },
                    10.,
                    theme.text_secondary,
                )),
        );

    if !preview_matches || loading {
        preview = preview.child(widgets::text(
            "Loading file names and sizes…",
            10.,
            theme.text_secondary,
        ));
    } else if let Some(error) = error {
        preview = preview.child(widgets::text(
            format!("Could not load contents: {error}"),
            10.,
            theme.text_tertiary,
        ));
    } else if files.is_empty() {
        preview = preview.child(widgets::text(
            "No file list was provided.",
            10.,
            theme.text_tertiary,
        ));
    } else {
        let mut rows = div()
            .id("accordion-contents-files")
            .flex()
            .flex_col()
            .min_h(px(0.))
            .max_h(px(190.))
            .overflow_y_scroll();
        for file in files {
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(27.))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex_grow(1.0)
                            .min_w(px(0.))
                            .font_family(UI_FONT)
                            .text_size(px(10.5))
                            .text_color(theme.text)
                            .truncate()
                            .child(file.path.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(UI_FONT)
                            .text_size(px(10.))
                            .text_color(theme.text_secondary)
                            .child(human_size(file.size)),
                    ),
            );
        }
        preview = preview.child(rows);
    }

    preview
}

fn contents_preview(
    app: &App,
    theme: &Theme,
    result: &SearchResult,
    cx: &mut Context<App>,
) -> AnyElement {
    let files = if app.preview_hash.as_deref() == Some(result.info_hash.as_str()) {
        app.preview_files.clone()
    } else {
        Vec::new()
    };
    let loading =
        app.preview_hash.as_deref() != Some(result.info_hash.as_str()) || app.preview_loading;
    let error = if app.preview_hash.as_deref() == Some(result.info_hash.as_str()) {
        app.preview_error.clone()
    } else {
        None
    };
    let total: i64 = files.iter().map(|file| file.size).sum();

    let mut section = div()
        .flex()
        .flex_col()
        .gap(px(7.))
        .pt(px(10.))
        .border_t_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(widgets::uppercase(
                    "Torrent Contents",
                    9.,
                    theme.text_tertiary,
                ))
                .child(widgets::text(
                    if files.is_empty() {
                        format!("{} files", count_text(result.num_files))
                    } else {
                        format!("{} files · {}", files.len(), human_size(total))
                    },
                    10.,
                    theme.text_secondary,
                )),
        );

    if loading {
        section = section.child(widgets::text(
            "Loading file names and sizes from torrent metadata…",
            10.5,
            theme.text_secondary,
        ));
    } else if let Some(error) = error {
        section = section.child(widgets::text(
            format!("Could not load torrent contents: {error}"),
            10.,
            theme.text_tertiary,
        ));
    } else if files.is_empty() {
        section = section.child(widgets::text(
            "This torrent did not provide a file list.",
            10.5,
            theme.text_tertiary,
        ));
    } else {
        let theme_copy = *theme;
        let row_count = files.len();
        section = section.child(
            uniform_list(
                "torrent-preview-files",
                row_count,
                cx.processor(move |_this, range: Range<usize>, _window, _cx| {
                    range
                        .filter_map(|index| files.get(index))
                        .map(|file| {
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .h(px(27.))
                                .border_b_1()
                                .border_color(theme_copy.border)
                                .child(
                                    div()
                                        .flex_grow(1.0)
                                        .min_w(px(0.))
                                        .font_family(UI_FONT)
                                        .text_size(px(10.5))
                                        .text_color(theme_copy.text)
                                        .truncate()
                                        .child(file.path.clone()),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .font_family(UI_FONT)
                                        .text_size(px(10.))
                                        .text_color(theme_copy.text_secondary)
                                        .child(human_size(file.size)),
                                )
                                .into_any_element()
                        })
                        .collect()
                }),
            )
            .h(px(190.))
            .max_h(px(230.))
            .w_full(),
        );
    }

    section.into_any_element()
}

fn result_stats(theme: &Theme, result: &SearchResult, reports_health: bool) -> Div {
    let mut metrics: Vec<(&'static str, String, Hsla)> = Vec::new();
    if reports_health {
        metrics.push(("Seeders", count_text(result.seeders), theme.success));
        metrics.push(("Leechers", count_text(result.leechers), theme.danger));
    } else {
        metrics.push(("Seeders", "—".to_owned(), theme.text_secondary));
        metrics.push(("Leechers", "—".to_owned(), theme.text_secondary));
    }
    if result.num_files > 0 {
        metrics.push(("Files", count_text(result.num_files), theme.text));
    }

    let mut stats = div().flex().items_stretch().w_full();
    for (index, (label, value, color)) in metrics.into_iter().enumerate() {
        if index > 0 {
            stats = stats.child(div().w(px(1.)).flex_shrink_0().bg(theme.border));
        }
        stats = stats.child(detail_stat(theme, label, value, color, index > 0));
    }

    div()
        .flex()
        .flex_col()
        .p(px(14.))
        .bg(theme.inset)
        .border_1()
        .border_color(theme.border)
        .child(stats.pb(px(12.)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .pt(px(12.))
                .border_t_1()
                .border_color(theme.border)
                .child(widgets::uppercase("Infohash", 9., theme.text_tertiary))
                .child(crate::app::hash_text(result.info_hash.clone(), theme)),
        )
}

fn detail_stat(
    theme: &Theme,
    label: &'static str,
    value: String,
    color: Hsla,
    separated: bool,
) -> Div {
    let mut cell = div()
        .flex()
        .flex_col()
        .flex_grow(1.0)
        .min_w(px(0.))
        .gap(px(4.));
    if separated {
        cell = cell.pl(px(14.));
    }
    cell.child(widgets::uppercase(label, 9., theme.text_tertiary))
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(17.))
                .line_height(px(21.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(value),
        )
}

/// Thousands separators keep swarm counts readable at a glance.
fn count_text(value: i64) -> String {
    let digits = value.max(0).to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
