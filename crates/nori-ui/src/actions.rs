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
        SelectPreviousSection
    ]
);

/// Key context the settings nav column declares around its search field.
pub const SETTINGS_NAV_CONTEXT: &str = "SettingsNav";

/// The nav search field holds focus while the user types, so `up`/`down` have
/// to be claimed from under it by a binding scoped to the field's own context.
pub const SETTINGS_SEARCH_CONTEXT: &str = "SettingsNav > NoriTextField";

pub fn register_key_bindings(cx: &mut gpui::App) {
    use gpui::KeyBinding;

    crate::components::register_text_field_bindings(cx);

    cx.bind_keys([
        KeyBinding::new("j", MoveSelectionDown, Some("Inbox")),
        KeyBinding::new("k", MoveSelectionUp, Some("Inbox")),
        KeyBinding::new("enter", OpenSelected, Some("Inbox")),
        KeyBinding::new("c", Compose, Some("Inbox")),
        KeyBinding::new("/", OpenSearch, Some("Inbox")),
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
        KeyBinding::new("ctrl-b", ToggleSidebar, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("ctrl-t", ToggleTabStrip, None),
        KeyBinding::new("cmd-t", ToggleTabStrip, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("down", SelectNextSection, Some(SETTINGS_SEARCH_CONTEXT)),
        KeyBinding::new("up", SelectPreviousSection, Some(SETTINGS_SEARCH_CONTEXT)),
    ]);
}
