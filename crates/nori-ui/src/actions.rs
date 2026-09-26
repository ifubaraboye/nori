use gpui::actions;

actions!(
    nori,
    [
        MoveSelectionDown,
        MoveSelectionUp,
        OpenSelected,
        Compose,
        OpenSearch,
        CloseTab,
        NextTab,
        PreviousTab,
        Dismiss,
        SearchMoveDown,
        SearchMoveUp,
        SearchOpenSelected,
        ToggleSidebar,
        ToggleTabStrip,
        OpenSettings,
        SelectNextSection,
        SelectPreviousSection,
        // One action per mailbox, in `Mailbox::NAV_ITEMS` order, because a
        // gpui action carries no payload: `Ctrl+1` has to name a folder at
        // compile time rather than pass one in.
        GoInbox,
        GoStarred,
        GoSent,
        GoDrafts,
        GoArchive,
        GoTrash,
        // The sidebar's back and forward chevrons.
        GoBack,
        GoForward,
    ]
);

/// Key context the settings nav column declares around its page list.
pub const SETTINGS_NAV_CONTEXT: &str = "SettingsNav";

pub fn register_key_bindings(cx: &mut gpui::App) {
    use gpui::KeyBinding;

    crate::components::register_text_field_bindings(cx);

    cx.bind_keys([
        KeyBinding::new("j", MoveSelectionDown, Some("Inbox")),
        KeyBinding::new("k", MoveSelectionUp, Some("Inbox")),
        KeyBinding::new("enter", OpenSelected, Some("Inbox")),
        KeyBinding::new("escape", Dismiss, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("cmd-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
        KeyBinding::new("cmd-shift-tab", PreviousTab, None),
        KeyBinding::new("down", SearchMoveDown, Some("Search")),
        KeyBinding::new("up", SearchMoveUp, Some("Search")),
        KeyBinding::new("enter", SearchOpenSelected, Some("Search")),
        // Every lettered shortcut is registered twice, once per case, and the
        // pairs are not duplicates. gpui parses a single uppercase letter in a
        // binding as *shift* plus the lowercase key, and matches the key and
        // the modifiers exactly, so `ctrl-n` is `Ctrl+n` and `ctrl-N` is
        // `Ctrl+Shift+n`. Registering both is what makes the command answer to
        // either case while the sidebar only ever shows the capital one.
        // `ctrl-shift-p` needs no such pair: the explicit `shift-` already
        // produces the same keystroke either way.
        KeyBinding::new("ctrl-n", Compose, None),
        KeyBinding::new("ctrl-N", Compose, None),
        KeyBinding::new("cmd-n", Compose, None),
        KeyBinding::new("cmd-N", Compose, None),
        KeyBinding::new("ctrl-s", OpenSearch, None),
        KeyBinding::new("ctrl-S", OpenSearch, None),
        KeyBinding::new("cmd-s", OpenSearch, None),
        KeyBinding::new("cmd-S", OpenSearch, None),
        KeyBinding::new("ctrl-alt-b", ToggleSidebar, None),
        KeyBinding::new("ctrl-alt-B", ToggleSidebar, None),
        KeyBinding::new("cmd-alt-b", ToggleSidebar, None),
        KeyBinding::new("cmd-alt-B", ToggleSidebar, None),
        KeyBinding::new("ctrl-shift-p", OpenSettings, None),
        KeyBinding::new("cmd-shift-p", OpenSettings, None),
        // Mailboxes, in `Mailbox::NAV_ITEMS` order so the digits match the
        // order the sidebar shows them in.
        KeyBinding::new("ctrl-1", GoInbox, None),
        KeyBinding::new("cmd-1", GoInbox, None),
        KeyBinding::new("ctrl-2", GoStarred, None),
        KeyBinding::new("cmd-2", GoStarred, None),
        KeyBinding::new("ctrl-3", GoSent, None),
        KeyBinding::new("cmd-3", GoSent, None),
        KeyBinding::new("ctrl-4", GoDrafts, None),
        KeyBinding::new("cmd-4", GoDrafts, None),
        KeyBinding::new("ctrl-5", GoArchive, None),
        KeyBinding::new("cmd-5", GoArchive, None),
        KeyBinding::new("ctrl-6", GoTrash, None),
        KeyBinding::new("cmd-6", GoTrash, None),
        // History. No command variant: the chevrons are not the way into this,
        // so nothing is displayed for them.
        KeyBinding::new("alt-left", GoBack, None),
        KeyBinding::new("alt-right", GoForward, None),
        KeyBinding::new("ctrl-t", ToggleTabStrip, None),
        KeyBinding::new("cmd-t", ToggleTabStrip, None),
        KeyBinding::new("down", SelectNextSection, Some(SETTINGS_NAV_CONTEXT)),
        KeyBinding::new("up", SelectPreviousSection, Some(SETTINGS_NAV_CONTEXT)),
    ]);
}
