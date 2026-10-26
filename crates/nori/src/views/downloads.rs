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

pub fn downloads_view(app: &App, theme: &Theme, width: f32, cx: &mut Context<App>) -> AnyElement {
    let list = downloads_list(app, theme, cx);
    let Some(item) = app
        .dl_selected
        .and_then(|index| app.active_downloads().get(index).cloned())
    else {
        return list;
    };
    files_layout(app, theme, width, list, &item, FilesKind::Download, cx)
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

pub fn seeding_view(app: &App, theme: &Theme, width: f32, cx: &mut Context<App>) -> AnyElement {
    let list = seeding_list(app, theme, cx);
    let Some(item) = app
        .seed_selected
        .and_then(|index| app.completed_downloads().get(index).cloned())
    else {
        return list;
    };
    files_layout(app, theme, width, list, &item, FilesKind::Seed, cx)
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
        bar = bar.child(widgets::text(
            "Seeding Off",
            10.,
            theme.text_tertiary,
        ));
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

#[derive(Clone, Copy)]
enum FilesKind {
    Download,
    Seed,
}

impl FilesKind {
    fn clear(self, app: &mut App) {
        match self {
            FilesKind::Download => app.dl_selected = None,
            FilesKind::Seed => app.seed_selected = None,
        }
    }
}

/// Put the file pane beside the queue, or replace the queue with it on a
/// narrow window.
fn files_layout(
    app: &App,
    theme: &Theme,
    width: f32,
    list: AnyElement,
    item: &Item,
    kind: FilesKind,
    cx: &mut Context<App>,
) -> AnyElement {
    if width < 900. {
        return compact_files(app, theme, item, kind, cx);
    }
    let pane_width = if width < 1120. { 300. } else { 340. };
    div()
        .flex()
        .size_full()
        .min_h(px(0.))
        .child(div().flex_grow(1.0).min_w(px(0.)).h_full().child(list))
        .child(
            div()
                .w(px(pane_width))
                .h_full()
                .flex_shrink_0()
                .border_l_1()
                .border_color(theme.border)
                .child(files_pane(app, theme, item, kind, false, cx)),
        )
        .into_any_element()
}

fn compact_files(
    app: &App,
    theme: &Theme,
    item: &Item,
    kind: FilesKind,
    cx: &mut Context<App>,
) -> AnyElement {
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
                .h(px(40.))
                .px(px(12.))
                .flex_shrink_0()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    widgets::button(theme, "Back")
                        .id("files-back")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            kind.clear(this);
                            cx.notify();
                        })),
                )
                .child(widgets::strong("Files", 12., theme.text)),
        )
        .child(files_pane(app, theme, item, kind, true, cx))
        .into_any_element()
}

fn files_pane(
    app: &App,
    theme: &Theme,
    item: &Item,
    kind: FilesKind,
    compact: bool,
    cx: &mut Context<App>,
) -> AnyElement {
    let count = item.files.len();
    let total: i64 = if item.size_bytes > 0 {
        item.size_bytes
    } else {
        item.files.iter().map(|file| file.size).sum()
    };

    let mut header = div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .flex_shrink_0()
        .px(px(16.))
        .pt(px(14.))
        .pb(px(10.))
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(theme.text)
                .truncate()
                .child(item.name.clone()),
        )
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(10.5))
                .line_height(px(15.))
                .text_color(theme.text_secondary)
                .child(format!(
                    "{} files  ·  {}  ·  {}",
                    count,
                    human_size(total),
                    item.status.label()
                )),
        );
    if item.status == Status::Downloading {
        header = header.child(widgets::progress(theme, item.fraction() as f32));
    }
    if item.status == Status::Completed && !app.cfg.seeding_enabled {
        header = header.child(widgets::text(
            "Seeding is off in Settings. Files are complete.",
            10.,
            theme.text_tertiary,
        ));
    }

    let files = item.files.clone();
    let completed = item.status == Status::Completed;
    let theme_copy = *theme;
    let body: AnyElement = if files.is_empty() {
        div()
            .flex()
            .flex_grow(1.0)
            .items_center()
            .justify_center()
            .px(px(20.))
            .child(widgets::text(
                "The file list appears once the torrent's metadata is ready.",
                10.5,
                theme.text_tertiary,
            ))
            .into_any_element()
    } else {
        let rows = files.len();
        uniform_list(
            "item-files",
            rows,
            cx.processor(move |_this, range: Range<usize>, _window, _cx| {
                range
                    .filter_map(|index| {
                        files
                            .get(index)
                            .map(|file| file_row(&theme_copy, file, completed))
                    })
                    .collect()
            }),
        )
        .flex_grow(1.0)
        .w_full()
        .min_h(px(0.))
        .into_any_element()
    };

    let footer = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_shrink_0()
        .px(px(16.))
        .py(px(10.))
        .border_t_1()
        .border_color(theme.border)
        .child(
            widgets::button(theme, "Show in Folder")
                .id("files-folder")
                .on_click(cx.listener({
                    let item = item.clone();
                    move |this, _, _, cx| {
                        this.open_folder(&item, cx);
                    }
                })),
        );

    let mut pane = div().flex().flex_col().size_full().min_h(px(0.));
    if !compact {
        pane = pane.child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .h(px(30.))
                .px(px(8.))
                .flex_shrink_0()
                .child(
                    widgets::icon_button(theme, icons::X, 12., theme.text_secondary)
                        .id("files-close")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            kind.clear(this);
                            cx.notify();
                        })),
                ),
        );
    }
    pane.child(header)
        .child(body)
        .child(footer)
        .into_any_element()
}

fn file_row(theme: &Theme, file: &nori_core::torrent::ItemFile, completed: bool) -> AnyElement {
    let fraction = if file.size > 0 {
        (file.downloaded as f64 / file.size as f64).clamp(0.0, 1.0) as f32
    } else {
        1.0
    };
    let path = file.path.rsplit('/').next().unwrap_or(&file.path);
    let folder = file
        .path
        .rsplit_once('/')
        .map(|(folder, _)| folder)
        .unwrap_or_default();

    let mut label = div()
        .flex()
        .flex_col()
        .flex_grow(1.0)
        .min_w(px(0.))
        .gap(px(1.))
        .child(
            div()
                .font_family(UI_FONT)
                .text_size(px(11.))
                .line_height(px(15.))
                .text_color(theme.text)
                .truncate()
                .child(path.to_owned()),
        );
    if !folder.is_empty() {
        label = label.child(
            div()
                .font_family(UI_FONT)
                .text_size(px(9.5))
                .line_height(px(13.))
                .text_color(theme.text_tertiary)
                .truncate()
                .child(folder.to_owned()),
        );
    }

    let mut row = div()
        .flex()
        .flex_col()
        .w_full()
        .gap(px(4.))
        .px(px(16.))
        .py(px(6.))
        .child(
            div().flex().items_center().gap(px(10.)).child(label).child(
                div()
                    .flex_shrink_0()
                    .font_family(UI_FONT)
                    .text_size(px(10.))
                    .text_color(theme.text_secondary)
                    .child(human_size(file.size)),
            ),
        );
    if !completed && fraction < 1.0 {
        row = row.child(widgets::progress(theme, fraction));
    }
    row.into_any_element()
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
                        if paused {
                            this.manager.resume(&toggle_id);
                        } else {
                            this.manager.pause(&toggle_id);
                        }
                        cx.notify();
                    })),
                )
                .child(
                    widgets::icon_button(theme, icons::STOP, 20., theme.danger)
                        .id(("dl-stop", index))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.manager.pause(&stop_id);
                            cx.notify();
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
                            this.manager.remove(&remove_id);
                            this.set_toast("Removed from the list; its files stay on disk", cx);
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
                            this.manager.remove(&remove_id);
                            cx.notify();
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
