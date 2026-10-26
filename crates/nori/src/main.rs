//! Nori: a native torrent finder and download queue.

mod app;
mod icons;
mod input;
mod theme;
mod views;
mod widgets;

use std::borrow::Cow;
use std::sync::Arc;

use gpui::{
    App, AppContext, Bounds, KeyBinding, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, px, size,
};
use gpui_platform::application;
use nori_core::config::Config;
use nori_core::search::Searcher;
use nori_core::torrent::TorrentManager;

use crate::app::{
    App as NoriApp, CopyPicked, CycleSort, DownloadPicked, Escape, FocusSearch, OpenFolder,
    OpenPicked, OpenSettings, PasteMagnet, PauseDownload, PauseSeed, Quit, RemoveDownload,
    RemoveSeed, SearchAgain, SelectAllResults, SelectNext, SelectPrev, Submit, ToggleFilter,
    ToggleHideDead,
};
use crate::input::{
    Backspace, Copy, Cut, Delete, End, Home, Left, Paste, Right, SelectAll, SelectLeft, SelectRight,
};

const RESULTS: Option<&str> = Some("Results && !TextInput");
const DOWNLOADS: Option<&str> = Some("Downloads && !TextInput");
const SEEDING: Option<&str> = Some("Seeding && !TextInput");
const TEXT_INPUT: Option<&str> = Some("TextInput");

fn bindings() -> Vec<KeyBinding> {
    vec![
        // Typing. Scoped to the field so the same letters stay available to
        // the section shortcuts everywhere else.
        KeyBinding::new("backspace", Backspace, TEXT_INPUT),
        KeyBinding::new("delete", Delete, TEXT_INPUT),
        KeyBinding::new("left", Left, TEXT_INPUT),
        KeyBinding::new("right", Right, TEXT_INPUT),
        KeyBinding::new("shift-left", SelectLeft, TEXT_INPUT),
        KeyBinding::new("shift-right", SelectRight, TEXT_INPUT),
        KeyBinding::new("secondary-a", SelectAll, TEXT_INPUT),
        KeyBinding::new("secondary-v", Paste, TEXT_INPUT),
        KeyBinding::new("secondary-c", Copy, TEXT_INPUT),
        KeyBinding::new("secondary-x", Cut, TEXT_INPUT),
        KeyBinding::new("home", Home, TEXT_INPUT),
        KeyBinding::new("end", End, TEXT_INPUT),
        // Window-wide.
        KeyBinding::new("secondary-k", FocusSearch, None),
        KeyBinding::new("secondary-r", SearchAgain, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-comma", OpenSettings, None),
        KeyBinding::new("secondary-v", PasteMagnet, Some("!TextInput")),
        KeyBinding::new("escape", Escape, None),
        KeyBinding::new("enter", Submit, None),
        KeyBinding::new("secondary-q", Quit, None),
        // Browse.
        KeyBinding::new("d", DownloadPicked, RESULTS),
        KeyBinding::new("secondary-a", SelectAllResults, RESULTS),
        KeyBinding::new("y", CopyPicked, RESULTS),
        KeyBinding::new("s", CycleSort, RESULTS),
        KeyBinding::new("f", ToggleFilter, RESULTS),
        KeyBinding::new("z", ToggleHideDead, RESULTS),
        KeyBinding::new("/", FocusSearch, RESULTS),
        KeyBinding::new("enter", OpenPicked, RESULTS),
        KeyBinding::new("down", SelectNext, RESULTS),
        KeyBinding::new("up", SelectPrev, RESULTS),
        // Downloads.
        KeyBinding::new("p", PauseDownload, DOWNLOADS),
        KeyBinding::new("c", RemoveDownload, DOWNLOADS),
        KeyBinding::new("e", OpenFolder, DOWNLOADS),
        KeyBinding::new("down", SelectNext, DOWNLOADS),
        KeyBinding::new("up", SelectPrev, DOWNLOADS),
        // Seeding.
        KeyBinding::new("p", PauseSeed, SEEDING),
        KeyBinding::new("c", RemoveSeed, SEEDING),
        KeyBinding::new("e", OpenFolder, SEEDING),
        KeyBinding::new("down", SelectNext, SEEDING),
        KeyBinding::new("up", SelectPrev, SEEDING),
    ]
}

fn main() {
    application().run(|cx: &mut App| {
        let fonts: Vec<Cow<'static, [u8]>> = vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf") as &[u8]),
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf") as &[u8]),
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf") as &[u8]),
            Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Bold.ttf") as &[u8]),
            Cow::Borrowed(
                include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf") as &[u8],
            ),
            Cow::Borrowed(
                include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Bold.ttf") as &[u8],
            ),
        ];
        if let Err(err) = cx.text_system().add_fonts(fonts) {
            eprintln!("could not load bundled fonts: {err}");
        }

        cx.bind_keys(bindings());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("nori-async")
            .build()
            .expect("build the async runtime");
        let runtime = Box::leak(Box::new(runtime)).handle().clone();

        let searcher = Arc::new(Searcher::new().expect("build the HTTP client"));
        let cfg = Config::load();
        let manager = TorrentManager::new(cfg.clone());

        let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(760.), px(560.))),
            titlebar: Some(TitlebarOptions {
                title: Some("Nori".into()),
                ..Default::default()
            }),
            app_id: Some("nori-torrent".into()),
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };

        let handle = cx
            .open_window(options, |window, cx| {
                let app = cx.new(|cx| NoriApp::new(cfg, manager, runtime, searcher, cx));
                let startup_error = app.read(cx).manager.startup_error();
                app.update(cx, |app, cx| {
                    app.focus_search(window, cx);
                    if let Some(error) = startup_error {
                        app.set_toast(error, cx);
                    }
                });
                app
            })
            .expect("open the Nori window");

        cx.activate(false);
        let _ = handle;
    });
}
