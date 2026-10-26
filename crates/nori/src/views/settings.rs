//! Settings: a single readable column of small cards. Every control saves as
//! it is used, so there is no Save button to forget.

use gpui::{AnyElement, Context, FontWeight, div, prelude::*, px};
use nori_core::config::ThemePreference;
use nori_core::search::sources;

use crate::app::App;
use crate::icons;
use crate::theme::Theme;
use crate::widgets::{self, UI_FONT};

pub fn view(app: &App, theme: &Theme, cx: &mut Context<App>) -> AnyElement {
    let mut page = div()
        .flex()
        .flex_col()
        .gap(px(24.))
        .w_full()
        .p(px(28.))
        .pt(px(24.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(widgets::strong("Settings", 18., theme.text))
                .child(widgets::text(
                    "Changes save automatically.",
                    11.,
                    theme.text_secondary,
                )),
        );

    // Downloads ---------------------------------------------------------
    let folder = div()
        .flex()
        .items_center()
        .gap(px(16.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.0)
                .min_w(px(0.))
                .gap(px(3.))
                .child(widgets::text("Folder", 12., theme.text))
                .child(
                    div()
                        .font_family(UI_FONT)
                        .text_size(px(11.5))
                        .line_height(px(17.))
                        .text_color(theme.text_secondary)
                        .truncate()
                        .child(app.cfg.download_dir.clone()),
                )
                .child(widgets::text(
                    "Where downloads are saved. Applies after restart.",
                    10.,
                    theme.text_tertiary,
                )),
        )
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .gap(px(8.))
                .child(
                    widgets::button(theme, "Choose Folder")
                        .id("choose-folder")
                        .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
                )
                .child(
                    widgets::button(theme, "Open")
                        .id("open-folder")
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.open_with_system(std::path::Path::new(&this.cfg.download_dir));
                        })),
                ),
        );

    let max_row = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.0)
                .gap(px(2.))
                .child(widgets::text("Downloads At Once", 12., theme.text))
                .child(widgets::text(
                    "Maximum simultaneous downloads. 0 = unlimited. Queued batches run one at a time.",
                    10.,
                    theme.text_tertiary,
                )),
        )
        .child(widgets::stepper_value(
            theme,
            app.cfg.max_downloads.to_string(),
            28.,
        ))
        .child(
            widgets::icon_button(theme, icons::MINUS, 11., theme.text_secondary)
                .id("max-downloads-minus")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cfg.max_downloads = (this.cfg.max_downloads - 1).max(0);
                    this.manager.set_max_downloads(this.cfg.max_downloads);
                    this.save_config();
                    cx.notify();
                })),
        )
        .child(
            widgets::icon_button(theme, icons::PLUS, 11., theme.text_secondary)
                .id("max-downloads-plus")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cfg.max_downloads = (this.cfg.max_downloads + 1).min(20);
                    this.manager.set_max_downloads(this.cfg.max_downloads);
                    this.save_config();
                    cx.notify();
                })),
        );

    let seed_row = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow(1.0)
                .gap(px(2.))
                .child(widgets::text("Seed For", 12., theme.text))
                .child(widgets::text(
                    "How long to seed after finishing. 0 = forever.",
                    10.,
                    theme.text_tertiary,
                )),
        )
        .child(widgets::stepper_value(
            theme,
            seed_minutes_label(app.cfg.seed_time_minutes),
            60.,
        ))
        .child(
            widgets::icon_button(theme, icons::MINUS, 11., theme.text_secondary)
                .id("seed-minutes-minus")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cfg.seed_time_minutes = (this.cfg.seed_time_minutes - 10).max(0);
                    this.manager.set_seed_time(this.cfg.seed_time_minutes);
                    this.save_config();
                    cx.notify();
                })),
        )
        .child(
            widgets::icon_button(theme, icons::PLUS, 11., theme.text_secondary)
                .id("seed-minutes-plus")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cfg.seed_time_minutes = (this.cfg.seed_time_minutes + 10).min(1440);
                    this.manager.set_seed_time(this.cfg.seed_time_minutes);
                    this.save_config();
                    cx.notify();
                })),
        );

    page = page.child(widgets::section(
        theme,
        "Downloads",
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(folder)
            .child(widgets::divider(theme))
            .child(max_row)
            .child(seed_row),
    ));

    // Results -----------------------------------------------------------
    let hide_dead = app.hide_dead;
    page = page.child(widgets::section(
        theme,
        "Results",
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_grow(1.0)
                    .gap(px(2.))
                    .child(widgets::text("Hide Dead Torrents", 12., theme.text))
                    .child(widgets::text(
                        "Hide results with zero seeders. Sources without swarm data are kept.",
                        10.,
                        theme.text_tertiary,
                    )),
            )
            .child(
                widgets::checkbox(theme, hide_dead, "")
                    .id("hide-dead")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.hide_dead = !hide_dead;
                        this.cfg.hide_dead = this.hide_dead;
                        this.refresh();
                        this.save_config();
                        cx.notify();
                    })),
            ),
    ));

    // Sources -----------------------------------------------------------
    let mut source_grid = div().grid().grid_cols(2).gap(px(4.));
    for source in sources() {
        let id = source.key;
        let enabled = app.cfg.enabled(id);
        let label = format!("{}  ·  {}", source.title, source.groups_label());
        source_grid = source_grid.child(widgets::checkbox(theme, enabled, label).id(id).on_click(
            cx.listener(move |this, _, _, cx| {
                let next = !this.cfg.enabled(id);
                this.cfg.set_enabled(id, next);
                this.save_config();
                cx.notify();
            }),
        ));
    }
    let source_list = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(widgets::text(
            "Choose which indexers to search.",
            10.,
            theme.text_tertiary,
        ))
        .child(source_grid);
    page = page.child(widgets::section(theme, "Sources", source_list));

    // Theme -------------------------------------------------------------
    let current = app.cfg.theme_preference();
    let mut segmented = div()
        .flex()
        .items_center()
        .gap(px(1.))
        .p(px(2.))
        .bg(theme.inset)
        .border_1()
        .border_color(theme.border);
    for (label, preference, key) in [
        ("System", ThemePreference::System, "system"),
        ("Light", ThemePreference::Light, "light"),
        ("Dark", ThemePreference::Dark, "dark"),
    ] {
        let active = current == preference;
        let mut option = div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(24.))
            .px(px(12.))
            .font_family(UI_FONT)
            .text_size(px(11.5))
            .cursor_pointer()
            .id(key)
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.cfg.theme != key {
                    this.cfg.theme = key.to_owned();
                    this.save_config();
                    cx.notify();
                }
            }));
        option = if active {
            option
                .bg(theme.raised)
                .text_color(theme.text)
                .font_weight(FontWeight::MEDIUM)
        } else {
            option
                .text_color(theme.text_tertiary)
                .hover(move |style| style.text_color(theme.text_secondary))
        };
        segmented = segmented.child(option.child(label));
    }
    page = page.child(widgets::section(
        theme,
        "Theme",
        div()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_grow(1.0)
                    .gap(px(2.))
                    .child(widgets::text("Appearance", 12., theme.text))
                    .child(widgets::text(
                        "Chang your app appearance",
                        10.,
                        theme.text_tertiary,
                    )),
            )
            .child(div().flex_shrink_0().child(segmented)),
    ));

    // Trackers ----------------------------------------------------------
    page = page.child(widgets::section(
        theme,
        "Trackers",
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(widgets::text(
                "Extra announce URLs added to every magnet.",
                10.,
                theme.text_tertiary,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(30.))
                    .px(px(9.))
                    .bg(theme.inset)
                    .border_1()
                    .border_color(theme.border)
                    .text_color(theme.text)
                    .child(app.trackers_input.clone()),
            ),
    ));

    div()
        .id("settings-scroll")
        .size_full()
        .overflow_y_scroll()
        .child(page)
        .into_any_element()
}

fn seed_minutes_label(minutes: i32) -> String {
    if minutes <= 0 {
        "Forever".into()
    } else if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{:.1} h", minutes as f64 / 60.0)
    }
}
