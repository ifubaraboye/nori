use gpui::{App, KeyBinding, actions};

actions!(nori_edit, [Undo, Redo, Cut, Copy, Paste, SelectAll]);

/// Global text-editing actions shared by every text input, mirroring Waku's
/// `input::init`. Bindings are intentionally context-free so the native menu
/// bar's Edit items reach the focused field on every platform; each
/// `TextField` handles the actions it supports and ignores the rest.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-z", Undo, None),
        KeyBinding::new("secondary-shift-z", Redo, None),
        KeyBinding::new("secondary-x", Cut, None),
        KeyBinding::new("secondary-c", Copy, None),
        KeyBinding::new("secondary-v", Paste, None),
        KeyBinding::new("secondary-a", SelectAll, None),
    ]);
}
