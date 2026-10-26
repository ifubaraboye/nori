//! Downloads and seeding: the queue as rows, with the two or three actions
//! that matter on each one.

use std::ops::Range;

use gpui::{AnyElement, ClickEvent, Context, div, prelude::*, px, uniform_list};
use nori_core::search::human_size;
use nori_core::torrent::{Item, Status};

use crate::app::{App, Section};
use crate::icons;
use crate::theme::Theme;
use crate::widgets::{self, UI_FONT};

pub fn downloads_view(app: &App, theme: &Theme, _width: f32, cx: &mut Context<App>) -> AnyElement {
    downloads_list(app, theme, cx)
}

fn downloads_list(app: &App, theme: &Theme, cx: &mut Context<App>) -> AnyElement {
    let active = app.active_downloads();

    let mut column = div().flex().flex_col().size_full().min_h(px(0.)).child(
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(40.))
            .px(px(16.))
            .flex_shrink_0()
            .border_b_1()
            .border_color(theme.border)
            .child(widgets::strong("Downloads", 12., theme.text)),
    );

    if active.is_empty() {
        return column.child(empty_downloads(theme, cx)).into_any_element();
    }

    let count = active.len();
    let theme_copy = *theme;
    column = column.child(
        uniform_list(
            "downloads",
            count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                let active = this.active_downloads();
                let mut rows = Vec::with_capacity(range.len());
                for index in range {
                    let Some(item) = active.get(index).cloned() else {
                        continue;
                    };
                    rows.push(download_row(this, &theme_copy, index, &item, cx));
                }
                rows
            }),
        )
        .flex_grow(1.0)
        .w_full()
        .min_h(px(0.))
        .track_scroll(&app.dl_scroll),
    );

    column.into_any_element()
}

pub fn seeding_view(app: &App, theme: &Theme, _width: f32, cx: &mut Context<App>) -> AnyElement {
    seeding_list(app, theme, cx)
}

fn seeding_list(app: &App, theme: &Theme, cx: &mut Context<App>) -> AnyElement {
    let done = app.completed_downloads();

    let mut bar = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(40.))
        .px(px(16.))
        .flex_shrink_0()
        .border_b_1()
        .border_color(theme.border)
        .child(widgets::strong("Seeding", 12., theme.text))
        .child(div().flex_grow(1.0));
    if !app.cfg.seeding_enabled {
        bar = bar.child(widgets::text("Seeding Off", 10., theme.text_tertiary));
    }

    let mut column = div().flex().flex_col().size_full().min_h(px(0.)).child(bar);

    if done.is_empty() {
        return column
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(10.))
                    .flex_grow(1.0)
                    .child(icons::icon(icons::SEED, 28., theme.text_tertiary))
                    .child(widgets::strong("Nothing to seed", 15., theme.text)),
            )
            .into_any_element();
    }

    let count = done.len();
    let theme_copy = *theme;
    column = column.child(
        uniform_list(
            "seeding",
            count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                let done = this.completed_downloads();
                let mut rows = Vec::with_capacity(range.len());
                for index in range {
                    let Some(item) = done.get(index).cloned() else {
                        continue;
                    };
                    rows.push(seed_row(this, &theme_copy, index, &item, cx));
                }
                rows
            }),
        )
        .flex_grow(1.0)
        .w_full()
        .min_h(px(0.))
        .track_scroll(&app.seed_scroll),
    );

    column.into_any_element()
}

fn download_row(
    app: &App,
    theme: &Theme,
    index: usize,
    item: &Item,
    cx: &mut Context<App>,
) -> AnyElement {
    let selected = app.dl_selected == Some(index);
    let hovered = app.dl_hover == Some(index);

    let title = div()
        .flex_grow(1.0)
        .min_w(px(0.))
        .font_family(UI_FONT)
        .text_size(px(14.))
        .line_height(px(19.))
        .text_color(theme.text)
        .truncate()
        .child(item.name.clone());

    let meta = if item.status == Status::Queued {
        format!("Queued  ·  {}", human_size(item.size_bytes))
    } else {
        format!(
            "{} / {}  ·  {}  ·  {}  ·  {} Peer{}",
            human_size(item.downloaded),
            human_size(item.size_bytes),
            speed_text(item.speed),
            eta_text(item),
            item.peers,
            if item.peers == 1 { "" } else { "s" }
        )
    };
    let meta_color = if item.status == Status::Queued {
        theme.text_tertiary
    } else {
        theme.text_secondary
    };

    let body = div()
        .flex()
        .flex_col()
        .flex_grow(1.0)
        .min_w(px(0.))
        .gap(px(5.))
        .child(title)
        .child(widgets::progress(theme, item.fraction() as f32))
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(meta_color)
                .truncate()
                .child(meta),
        );

    let mut row = div()
        .flex()
        .items_center()
        .w_full()
        .gap(px(12.))
        .h(px(62.))
        .px(px(16.))
        .cursor_pointer()
        .child(body);

    row = if selected {
        row.bg(theme.row_selected())
    } else {
        row.hover(move |style| style.bg(theme.row_hover()))
    };

    if hovered {
        let paused = !item.status.is_active();
        let toggle_id = item.id.clone();
        let stop_id = item.id.clone();
        let remove_id = item.id.clone();
        let folder_item = item.clone();
        row = row.child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    widgets::icon_button(
                        theme,
                        if paused { icons::PLAY } else { icons::PAUSE },
                        18.,
                        theme.text_secondary,
                    )
                    .id(("dl-toggle", index))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let result = if paused {
                            this.manager.resume(&toggle_id)
                        } else {
                            this.manager.pause(&toggle_id)
                        };
                        if let Err(error) = result {
                            this.set_toast(error, cx);
                        } else {
                            cx.notify();
                        }
                    })),
                )
                .child(
                    widgets::icon_button(theme, icons::STOP, 20., theme.danger)
                        .id(("dl-stop", index))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Err(error) = this.manager.pause(&stop_id) {
                                this.set_toast(error, cx);
                            } else {
                                cx.notify();
                            }
                        })),
                )
                .child(
                    widgets::icon_button(theme, icons::FOLDER, 18., theme.text_secondary)
                        .id(("dl-folder", index))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_folder(&folder_item, cx);
                        })),
                )
                .child(
                    widgets::icon_button(theme, icons::TRASH, 18., theme.danger)
                        .id(("dl-remove", index))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Err(error) = this.manager.remove(&remove_id) {
                                this.set_toast(error, cx);
                            } else {
                                this.set_toast("Removed from the list; its files stay on disk", cx);
                            }
                        })),
                ),
        );
    }

    row.id(("download", index))
        .on_hover(cx.listener(move |this, hovered: &bool, _window, cx| {
            if *hovered {
                if this.dl_hover != Some(index) {
                    this.dl_hover = Some(index);
                    cx.notify();
                }
            } else if this.dl_hover == Some(index) {
                this.dl_hover = None;
                cx.notify();
            }
        }))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.focus_list(window, cx);
            this.dl_selected = Some(index);
            cx.notify();
        }))
        .into_any_element()
}

fn seed_row(
    app: &App,
    theme: &Theme,
    index: usize,
    item: &Item,
    cx: &mut Context<App>,
) -> AnyElement {
    let selected = app.seed_selected == Some(index);
    let hovered = app.seed_hover == Some(index);
    // Only a running seed reports transfer numbers; a stopped one is just a
    // finished torrent sitting in the list.
    let seeding = !item.seed_paused && app.cfg.seeding_enabled;
    let meta = format!(
        "{} Uploaded  ·  ↑ {}/s  ·  {} Peer{}",
        human_size(item.uploaded),
        human_size(item.up_speed),
        item.peers,
        if item.peers == 1 { "" } else { "s" }
    );

    let mut body = div()
        .flex()
        .flex_col()
        .flex_grow(1.0)
        .min_w(px(0.))
        .gap(px(4.))
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(14.))
                .line_height(px(19.))
                .text_color(theme.text)
                .truncate()
                .child(item.name.clone()),
        );
    if seeding {
        body = body.child(
            div()
                .font_family(UI_FONT)
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(theme.text_secondary)
                .truncate()
                .child(meta),
        );
    }

    let mut row = div()
        .flex()
        .items_center()
        .w_full()
        .gap(px(12.))
        .h(px(if seeding { 58. } else { 42. }))
        .px(px(16.))
        .cursor_pointer()
        .child(body);

    row = if selected {
        row.bg(theme.row_selected())
    } else {
        row.hover(move |style| style.bg(theme.row_hover()))
    };

    if hovered {
        let remove_id = item.id.clone();
        let folder_item = item.clone();
        let toggle_id = item.id.clone();
        let can_seed = app.cfg.seeding_enabled;
        let controls = div().flex().items_center().gap(px(4.)).child(
            widgets::icon_button(
                theme,
                if seeding { icons::STOP } else { icons::PLAY },
                20.,
                theme.text_secondary,
            )
            .id(("seed-toggle", index))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !can_seed {
                    this.set_toast("Seeding is off in Settings", cx);
                    return;
                }
                this.manager.set_seed_paused(&toggle_id, seeding);
                cx.notify();
            })),
        );
        row = row.child(
            controls
                .child(
                    widgets::icon_button(theme, icons::FOLDER, 18., theme.text_secondary)
                        .id(("seed-folder", index))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_folder(&folder_item, cx);
                        })),
                )
                .child(
                    widgets::icon_button(theme, icons::TRASH, 18., theme.danger)
                        .id(("seed-remove", index))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Err(error) = this.manager.remove(&remove_id) {
                                this.set_toast(error, cx);
                            } else {
                                cx.notify();
                            }
                        })),
                ),
        );
    }

    row.id(("seed", index))
        .on_hover(cx.listener(move |this, hovered: &bool, _window, cx| {
            if *hovered {
                if this.seed_hover != Some(index) {
                    this.seed_hover = Some(index);
                    cx.notify();
                }
            } else if this.seed_hover == Some(index) {
                this.seed_hover = None;
                cx.notify();
            }
        }))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.focus_list(window, cx);
            this.seed_selected = Some(index);
            cx.notify();
        }))
        .into_any_element()
}

fn empty_downloads(theme: &Theme, cx: &mut Context<App>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .flex_grow(1.0)
        .child(icons::icon(icons::DOWNLOAD, 28., theme.text_tertiary))
        .child(widgets::strong("No downloads yet", 15., theme.text))
        .child(
            widgets::primary_button(theme, "Browse")
                .id("empty-browse")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.navigate(Section::Browse, cx);
                    this.focus_list(window, cx);
                })),
        )
        .into_any_element()
}

fn speed_text(speed: i64) -> String {
    if speed <= 0 {
        "—".into()
    } else {
        format!("{}/s", human_size(speed))
    }
}

fn eta_text(item: &Item) -> String {
    let Some(seconds) = item.eta_secs() else {
        return "—".into();
    };
    if seconds < 60 {
        format!("{seconds}s left")
    } else if seconds < 3600 {
        format!("{}m left", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h {}m left", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{}d {}h left", seconds / 86_400, (seconds % 86_400) / 3600)
    }
}
