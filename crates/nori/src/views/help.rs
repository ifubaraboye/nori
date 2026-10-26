//! The keyboard cheat sheet.

use gpui::{Context, FontWeight, div, prelude::*, px};

use crate::app::App;
use crate::theme::Theme;
use crate::widgets::{MONO_FONT, UI_FONT, text};

struct Shortcut {
    keys: &'static str,
    label: &'static str,
}

fn shortcuts() -> Vec<Shortcut> {
    let cmd = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    };
    vec![
        Shortcut {
            keys: "Ctrl K",
            label: "Focus the search field",
        },
        Shortcut {
            keys: "Click",
            label: "Open a result's details",
        },
        Shortcut {
            keys: "Checkbox",
            label: "Choose a result for the batch",
        },
        Shortcut {
            keys: "Ctrl A",
            label: "Choose every visible result",
        },
        Shortcut {
            keys: "↵",
            label: "Download the chosen result",
        },
        Shortcut {
            keys: "d",
            label: "Download the chosen results",
        },
        Shortcut {
            keys: "y",
            label: "Copy the chosen magnet links",
        },
        Shortcut {
            keys: "s",
            label: "Cycle the sort order",
        },
        Shortcut {
            keys: "f",
            label: "Show the results filter",
        },
        Shortcut {
            keys: "z",
            label: "Hide torrents without seeders",
        },
        Shortcut {
            keys: "p",
            label: "Pause or resume a download",
        },
        Shortcut {
            keys: "e",
            label: "Open a download's folder",
        },
        Shortcut {
            keys: "c",
            label: "Remove a download from the list",
        },
        Shortcut {
            keys: "Ctrl V",
            label: "Add a magnet link from the clipboard",
        },
        Shortcut {
            keys: "Ctrl R",
            label: "Search again",
        },
        Shortcut {
            keys: "Ctrl ,",
            label: "Settings",
        },
        Shortcut {
            keys: "Esc",
            label: "Close a dialog, clear the filter or selection",
        },
    ]
    .into_iter()
    .map(|shortcut| {
        let keys = match shortcut.keys {
            "Ctrl K" => Box::leak(format!("{cmd} K").into_boxed_str()),
            "Ctrl V" => Box::leak(format!("{cmd} V").into_boxed_str()),
            "Ctrl R" => Box::leak(format!("{cmd} R").into_boxed_str()),
            "Ctrl ," => Box::leak(format!("{cmd} ,").into_boxed_str()),
            "Ctrl A" => Box::leak(format!("{cmd} A").into_boxed_str()),
            other => other,
        };
        Shortcut {
            keys,
            label: shortcut.label,
        }
    })
    .collect()
}

pub fn modal(app: &App, theme: &Theme, cx: &mut Context<App>) -> impl IntoElement {
    let mut list = div().flex().flex_col().gap(px(6.));
    for shortcut in shortcuts() {
        list = list.child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .w(px(64.))
                        .flex_shrink_0()
                        .font_family(MONO_FONT)
                        .text_size(px(10.5))
                        .text_color(theme.text_tertiary)
                        .font_weight(FontWeight::MEDIUM)
                        .child(shortcut.keys),
                )
                .child(text(shortcut.label, 11., theme.text)),
        );
    }
    let _ = app;

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::hsla(0., 0., 0., 0.45))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .w(px(460.))
                .p(px(20.))
                .bg(theme.canvas)
                .border_1()
                .border_color(theme.border_strong)
                .shadow_lg()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_grow(1.0)
                                .font_family(UI_FONT)
                                .text_size(px(16.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("Keyboard"),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(26.))
                                .cursor_pointer()
                                .hover(move |s| s.bg(theme.overlay_strong))
                                .id("help-close")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.help_open = false;
                                    cx.notify();
                                }))
                                .child(crate::icons::icon(
                                    crate::icons::X,
                                    12.,
                                    theme.text_secondary,
                                )),
                        ),
                )
                .child(list)
                .child(
                    div()
                        .font_family(UI_FONT)
                        .text_size(px(10.5))
                        .text_color(theme.text_ghost)
                        .child("Search results are public index listings; Nori hosts nothing."),
                ),
        )
}
