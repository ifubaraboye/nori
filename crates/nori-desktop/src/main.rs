use std::{borrow::Cow, path::Path, sync::Arc};

use anyhow::Result;
use gpui::{
    App, AppContext, AssetSource, Bounds, KeyBinding, Menu, MenuItem, QuitMode, SharedString,
    TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions, actions, px, size,
};
use gpui_platform::application;
use nori_ui::{MailApp, Theme, ToggleSidebar, input, register_key_bindings};

actions!(nori_desktop, [Quit]);

pub const APP_ID: &str = "dev.nori.prototype";
pub const APP_NAME: &str = "Nori";

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let asset: &'static [u8] = match path {
            "icons/mail.svg" => include_bytes!("../assets/icons/mail.svg"),
            "icons/search.svg" => include_bytes!("../assets/icons/search.svg"),
            "icons/compose.svg" => include_bytes!("../assets/icons/compose.svg"),
            "icons/close.svg" => include_bytes!("../assets/icons/close.svg"),
            "icons/plus.svg" => include_bytes!("../assets/icons/plus.svg"),
            "icons/star.svg" => include_bytes!("../assets/icons/star.svg"),
            "icons/star-filled.svg" => include_bytes!("../assets/icons/star-filled.svg"),
            "icons/archive.svg" => include_bytes!("../assets/icons/archive.svg"),
            "icons/inbox.svg" => include_bytes!("../assets/icons/inbox.svg"),
            "icons/sent.svg" => include_bytes!("../assets/icons/sent.svg"),
            "icons/drafts.svg" => include_bytes!("../assets/icons/drafts.svg"),
            "icons/trash.svg" => include_bytes!("../assets/icons/trash.svg"),
            "icons/settings.svg" => include_bytes!("../assets/icons/settings.svg"),
            "icons/gear.svg" => include_bytes!("../assets/icons/gear.svg"),
            "icons/send.svg" => include_bytes!("../assets/icons/send.svg"),
            "icons/paperclip.svg" => include_bytes!("../assets/icons/paperclip.svg"),
            "icons/reply.svg" => include_bytes!("../assets/icons/reply.svg"),
            "icons/reply-all.svg" => include_bytes!("../assets/icons/reply-all.svg"),
            "icons/forward.svg" => include_bytes!("../assets/icons/forward.svg"),
            "icons/panel-left.svg" => include_bytes!("../assets/icons/panel-left.svg"),
            "icons/chevron-down.svg" => include_bytes!("../assets/icons/chevron-down.svg"),
            "icons/chevron-right.svg" => include_bytes!("../assets/icons/chevron-right.svg"),
            "icons/chevron-left.svg" => include_bytes!("../assets/icons/chevron-left.svg"),
            "icons/ellipsis.svg" => include_bytes!("../assets/icons/ellipsis.svg"),
            "icons/pencil.svg" => include_bytes!("../assets/icons/pencil.svg"),
            "icons/arrow-left.svg" => include_bytes!("../assets/icons/arrow-left.svg"),
            "icons/arrow-right.svg" => include_bytes!("../assets/icons/arrow-right.svg"),
            "icons/pin.svg" => include_bytes!("../assets/icons/pin.svg"),
            "icons/pin-off.svg" => include_bytes!("../assets/icons/pin-off.svg"),
            "icons/link.svg" => include_bytes!("../assets/icons/link.svg"),
            "icons/bell.svg" => include_bytes!("../assets/icons/bell.svg"),
            "icons/appearance.svg" => include_bytes!("../assets/icons/appearance.svg"),
            "icons/user.svg" => include_bytes!("../assets/icons/user.svg"),
            "icons/info.svg" => include_bytes!("../assets/icons/info.svg"),
            "icons/reset.svg" => include_bytes!("../assets/icons/reset.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(asset)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if Path::new(path).extension().is_none() {
            return Ok(Vec::new());
        }
        Ok(Vec::new())
    }
}

fn main() {
    run();
}

fn run() {
    application()
        .with_assets(Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(|cx: &mut App| {
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("ctrl-q", Quit, None),
            ]);
            // Stable process identity for the Wayland app_id/X11 WM_CLASS and
            // notification attribution. Call once, before opening windows.
            cx.set_app_identity(APP_ID, APP_NAME);
            Theme::init(cx);
            input::init(cx);
            register_key_bindings(cx);
            set_app_menus(cx);
            open_main_window(cx);
            cx.activate(true);
        });
}

/// Decode the embedded desktop icon once. X11 consumes the RGBA pixels from
/// `WindowOptions`; Wayland associates the window through `app_id` and its
/// installed desktop entry.
#[cfg(target_os = "linux")]
fn linux_app_icon() -> Option<Arc<image::RgbaImage>> {
    static ICON: std::sync::LazyLock<Option<Arc<image::RgbaImage>>> =
        std::sync::LazyLock::new(|| {
            image::load_from_memory(include_bytes!("../assets/app-icon.png"))
                .ok()
                .map(|image| Arc::new(image.into_rgba8()))
        });
    ICON.clone()
}

fn set_app_menus(cx: &mut App) {
    cx.set_menus([
        Menu::new("Nori").items([MenuItem::action("Quit Nori", Quit)]),
        Menu::new("Edit").items([
            MenuItem::action("Undo", input::Undo),
            MenuItem::action("Redo", input::Redo),
            MenuItem::separator(),
            MenuItem::action("Cut", input::Cut),
            MenuItem::action("Copy", input::Copy),
            MenuItem::action("Paste", input::Paste),
            MenuItem::action("Select All", input::SelectAll),
        ]),
        Menu::new("View").items([MenuItem::action("Toggle Sidebar", ToggleSidebar)]),
    ]);
}

fn open_main_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);

    cx.open_window(
        WindowOptions {
            focus: true,
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(760.), px(520.))),
            window_background: WindowBackgroundAppearance::Opaque,
            app_id: Some(APP_ID.into()),
            #[cfg(target_os = "linux")]
            icon: linux_app_icon(),
            titlebar: Some(TitlebarOptions {
                title: Some(APP_NAME.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| MailApp::new(window, cx)),
    )
    .expect("failed to open Nori window");
}
