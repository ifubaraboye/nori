use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::actions::{
    CloseTab, Compose, Dismiss, GoArchive, GoBack, GoDrafts, GoForward, GoInbox, GoSent, GoStarred,
    GoTrash, MoveSelectionDown, MoveSelectionUp, NextTab, OpenSearch, OpenSelected, OpenSettings,
    PreviousTab, ToggleSidebar, ToggleTabStrip,
};
use gpui::{
    Context, CursorStyle, Entity, FocusHandle, ImageFormat, IntoElement, MouseButton,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render, Role, ScrollHandle,
    SharedString, Subscription, UniformListScrollHandle, Window, div, prelude::*, px,
    transparent_black,
};

use super::compose_view::{ComposeEvent, ComposeView};
use super::email_view::{EmailView, ImageSlot};
use super::inbox::{Inbox, scroll_selected_into_view};
use super::search_view::{SearchEvent, SearchView};
use super::settings_view::{SettingsEvent, SettingsView};
use crate::components::{
    EmailTabs, LabelsAction, SIDEBAR_DEFAULT_WIDTH, Sidebar, TextField, TopBar, clamp_sidebar_width,
};
use crate::model::{
    AccountState, Density, DraftSeed, Email, EmailId, Index, IndexCache, Label, LabelId,
    LabelStore, MailStore, Mailbox, Overlay, Setting, SettingsPage, SettingsState, SettingsStore,
    WorkspaceView, gmail_query, may_replace_index, mock::mock_emails, to_email,
};
use crate::theme::Theme;

struct SidebarResize {
    start_x: f32,
    start_width: f32,
}

/// A drag of the compose divider. `start_x` is where the pointer went down,
/// and the width is recomputed from the delta so the divider tracks the
/// pointer exactly instead of jumping to the cursor.
/// How often to ask Gmail what has changed.
///
/// Long on purpose. Each poll is one `history.list` at 2 units, so this costs
/// almost nothing, and a mail client that syncs on a timer far more often than
/// a person reads mail is just burning battery to reassure them.
const POLL_INTERVAL_SECONDS: u64 = 180;

/// Gmail's names for the two labels Nori writes back.
const STARRED: &str = "STARRED";
const UNREAD: &str = "UNREAD";

/// One label change queued for the server.
///
/// `Send` because it crosses onto a background thread. It is a struct rather
/// than a `Fn` closure so the work has a name in a stack trace and the
/// credential never has to be captured.
struct WriteBack {
    id: String,
    add: Vec<String>,
    remove: Vec<String>,
}

struct ComposeResize {
    start_x: f32,
    start_width: f32,
}

/// Width of the row overflow menu.
const LABEL_MENU_WIDTH: f32 = 200.;

/// Assumed window width for the flip test. The menu flips to the button's left
/// past this point rather than hanging off the right edge.
const LABEL_MENU_EDGE_GUARD: f32 = 700.;

/// How far below the top of the window an overlay panel sits.
const OVERLAY_TOP_OFFSET: f32 = 52.;

/// Width of the compose column. Wide enough to write a mail in, narrow enough
/// that the list beside it stays scannable.
const COMPOSE_PANE_WIDTH: f32 = 520.;

/// How many remote pictures one mail may trigger. Marketing mail sprinkles
/// dozens of tiny icons; the hero and the product shots all sit in the
/// first handful, so the tail never earns a connection.
const MAX_IMAGES_PER_MAIL: usize = 24;

/// Largest single picture Nori will hold: an 8MB hero is already generous,
/// and anything bigger is a hostile or broken endpoint, not photography.
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// Entries across all mails before the cache restarts. Pictures are small
/// against a mailbox, but unbounded is unbounded.
const MAX_CACHED_IMAGES: usize = 512;

/// A picture URL Nori will actually fetch: absolute `https` with something
/// after the scheme. The parser already enforces this; the fetch re-checks,
/// because a URL that arrives by any other road gets the same answer.
fn is_fetchable_image_url(url: &str) -> bool {
    url.len() > "https://".len() && url.starts_with("https://")
}

/// The image format from its magic bytes, so fetching never trusts a
/// `Content-Type` header or a file extension. Anything unrecognised —
/// SVG included, which is XML that can carry scripts — refuses to load.
fn sniff_image_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
    }
}

/// Download one picture off the UI thread: capped bytes, settled status,
/// recognised bytes. Anything else — wrong status, wrong bytes, wrong
/// format — is a `None`, and the reading view shows alt text instead.
///
/// Note what this request deliberately is not: no cookies, no auth, no
/// referrer beyond what the transport sets. Loading the picture still tells
/// the sender's server that somebody looked, which is inherent to remote
/// images and exactly what the future per-sender content policy will govern.
fn fetch_image_bytes(url: &str) -> Option<(ImageFormat, Vec<u8>)> {
    if !is_fetchable_image_url(url) {
        return None;
    }
    let mut response = nori_gmail::agent().get(url).call().ok()?;
    if response.status() != 200 {
        return None;
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_IMAGE_BYTES + 1)
        .read_to_vec()
        .ok()?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return None;
    }
    let format = sniff_image_format(&bytes)?;
    Some((format, bytes))
}

/// The compose column is user-resizable, so it needs ends. Below the minimum
/// the fields stop being usable.
const COMPOSE_PANE_MIN: f32 = 340.;

/// The mail pane has to survive beside the composer, or there is nothing left
/// to read while writing.
const MIN_READING_PANE_WIDTH: f32 = 420.;

/// Absolute backstop, so an unusually wide display cannot let the composer
/// swallow the whole window.
const COMPOSE_PANE_CEILING: f32 = 1600.;

/// Widest the compose column may get, given the window it shares with the
/// sidebar and the reading pane.
///
/// This has to be derived from the window rather than fixed. A flat pixel cap
/// is the reason the divider used to feel stuck in one direction: on a wide
/// display 960px is already most of the workspace, so the composer sat pinned
/// against its maximum and dragging the seam further left did nothing at all.
pub fn compose_pane_max(window_width: f32, sidebar_width: f32) -> f32 {
    (window_width - sidebar_width - MIN_READING_PANE_WIDTH)
        .clamp(COMPOSE_PANE_MIN, COMPOSE_PANE_CEILING)
}

/// Clamp to a maximum the caller supplies, so the bound can follow the window.
pub fn clamp_compose_pane_width(width: f32, max: f32) -> f32 {
    width.clamp(COMPOSE_PANE_MIN, max.max(COMPOSE_PANE_MIN))
}

pub struct MailApp {
    store: MailStore,
    /// The row overflow menu: which mail it belongs to, and where the button
    /// was clicked. The position travels with the click because the list is
    /// virtualized, so a row's bounds are only valid on the frame it was drawn.
    label_menu: Option<(EmailId, Point<Pixels>)>,
    /// User-defined labels, kept beside the store so mailbox state and label
    /// state never disturb each other.
    labels: LabelStore,
    /// The label the list is filtered by, or `None` for no label filter.
    selected_label: Option<LabelId>,
    /// The "new label" field in the sidebar. It is only mounted while
    /// `label_composer_open`, and the `+` that opens it has its own focus
    /// handle so the keyboard can reach it and so focus has somewhere to land
    /// when the composer closes.
    new_label_field: Entity<TextField>,
    new_label_focus: FocusHandle,
    /// Whether the Labels group is collapsed, independent of the Mailboxes
    /// group above it.
    labels_collapsed: bool,
    /// Whether the "new label" field is showing. The sidebar is `RenderOnce`
    /// and holds no state, so this lives here.
    label_composer_open: bool,
    inbox_focus: FocusHandle,
    workspace_focus: FocusHandle,
    tab_scroll: ScrollHandle,
    inbox_scroll: UniformListScrollHandle,
    sidebar_visible: bool,
    /// Whether the pinned-mail strip is shown. Hiding it does not unpin
    /// anything: `ctrl-tab` still cycles the pinned mails, so the strip is a
    /// view, not the only route to a tab.
    tab_strip_visible: bool,
    sidebar_width: f32,
    mailboxes_collapsed: bool,
    /// Whether Ctrl or Cmd is held, in which case the sidebar swaps each
    /// mailbox's count for its shortcut. Held-state rather than a toggle: the
    /// hint has to come back by itself the moment the key comes up.
    shortcuts_visible: bool,
    sidebar_resize: Option<SidebarResize>,
    /// Width of the compose column, remembered between openings so a resize
    /// is not undone by closing the pane.
    compose_pane_width: f32,
    compose_resize: Option<ComposeResize>,
    compose: Option<Entity<ComposeView>>,
    /// Remote images by source URL, across every open mail: a logo repeated
    /// in ten mails downloads once. Entries arrive as `Loading` and settle
    /// to `Loaded` or `Failed`; the reading view renders alt text until
    /// then. Bounded, because a mailbox that never forgets a picture is a
    /// slow leak wearing a cache costume.
    images: HashMap<String, ImageSlot>,
    search: Option<Entity<SearchView>>,
    /// Settings sits inside the mail shell rather than over it, so the mail
    /// sidebar and top bar stay on screen around it.
    settings: Option<Entity<SettingsView>>,
    /// Which settings page is showing. It lives here, not in `SettingsView`,
    /// because the top bar names the page and the top bar is this view's.
    /// `SettingsView` renders whatever it is handed and reports changes
    /// upward, the same way it reports toggles through `on_set`.
    settings_page: SettingsPage,
    /// Settings live here, not in `SettingsView`: the mail list needs them
    /// while settings is closed, and the view that edits them is discarded
    /// on close. Copying ten bools per frame is cheaper than a global.
    settings_state: SettingsState,
    /// Where the settings are written. A field rather than a path looked up on
    /// every save, so the destination is part of the app's state and a test can
    /// put it somewhere of its own instead of over the user's real settings.
    settings_store: SettingsStore,
    /// Sends index writes somewhere else, for a test. `None` in the app, which
    /// resolves the real config directory per account.
    ///
    /// This exists because "pinning saves the index" is otherwise untestable
    /// without writing over the index of whichever account the machine running
    /// the suite happens to be signed in to. The first version of that test
    /// sidestepped the problem by calling the write itself rather than the pin
    /// that triggers it, which meant it passed whether or not pinning saved
    /// anything — the exact bug it was written to catch.
    index_cache_override: Option<IndexCache>,
    /// Where the "which account did we last sign in to" pointer lives, when
    /// that should not be the real config directory.
    ///
    /// Signing out deletes that file, and it is named after nobody — not the
    /// account, not the test — so a test pressing the real Disconnect button
    /// deletes the pointer belonging to whoever is running the suite. That is
    /// not hypothetical: it happened, and the account had to be put back by
    /// hand. Production leaves this `None` and resolves the config directory.
    account_pointer: Option<std::path::PathBuf>,
    /// How the connected account reads, for the settings page and the top bar.
    /// `MailApp` owns it because the token store and the sync both live here,
    /// and the settings view is discarded on close.
    account: AccountState,
    /// Which mailboxes have had their mail fetched, so a folder is indexed once
    /// rather than on every visit.
    loaded_mailboxes: HashSet<Mailbox>,
    /// Folders a fetch came back empty from, so the control asking for more
    /// has nothing left to offer and stops being drawn.
    exhausted_mailboxes: HashSet<Mailbox>,
    /// Whether the scroll watcher is already running, so `render` can ask for one
    /// on every frame without starting a second.
    watching_more: bool,
    /// Whether a sync is in flight. A first sync takes about a minute — the
    /// quota allows only 300 mails a minute — so without this the list is just
    /// empty for a while, which reads as a broken app rather than a slow one.
    syncing: bool,
    /// The current fetch's stream. Mail arrives here from the worker threads
    /// and is moved into the list by `pump_incoming`, so the inbox fills in
    /// while the fetch is still running rather than appearing all at once.
    sync_stream: Option<Arc<nori_gmail::MailStream>>,
    /// The open search's own stream. Kept apart from the inbox's because its
    /// mail is an answer to a question, not something the folder views should
    /// start showing: a search hit is not necessarily in the Inbox.
    search_stream: Option<Arc<nori_gmail::MailStream>>,
    /// Bumped on every keystroke. A request that finds its number stale has
    /// been superseded and drops its results instead of answering a question
    /// the user has already typed past.
    search_generation: u64,
    /// Kept for the life of the view, so it lives in its own field: clearing a
    /// bag of subscriptions to drop one view's used to take the theme observer
    /// with it, and the app stopped repainting on a light-mode switch.
    _theme_subscription: Subscription,
    /// The settings page's `Dismiss`, dropped when settings closes.
    settings_subscription: Option<Subscription>,
    /// The compose and search subscriptions. They are mutually exclusive — only
    /// one overlay is open at a time — so they share a slot, and `close_overlay`
    /// drops it. Keeping it separate from the theme observer is what stops a
    /// closed composer from taking the others' listeners with it.
    /// The compose and search layers can be open at the same time, and each has
    /// to hand focus back to whatever was focused before *it* opened. One
    /// shared slot could only remember the last, so a popup opened over the
    /// compose pane would have sent focus to the wrong place on close.
    compose_subscription: Option<Subscription>,
    search_subscription: Option<Subscription>,
    settings_previous_focus: Option<FocusHandle>,
    compose_previous_focus: Option<FocusHandle>,
    search_previous_focus: Option<FocusHandle>,
    /// The draft the compose pane was holding when it closed. Dropping the
    /// pane drops its fields, so the text is lifted out on the way and put
    /// back the next time a blank compose is asked for.
    compose_draft: Option<DraftSeed>,
}

impl MailApp {
    /// Build the app over the sample mail, touching nothing on disk.
    ///
    /// Deliberately free of I/O. Reading the saved account here would make a
    /// test run depend on what happens to be signed in on the developer's
    /// machine, and would load a real mailbox over the sample data those tests
    /// assert against. The real app calls [`Self::resume`] once the window
    /// exists; a test that wants the cache can call it too.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let inbox_focus = cx.focus_handle().tab_index(1).tab_stop(true);
        let workspace_focus = cx.focus_handle().tab_index(2).tab_stop(true);
        window.focus(&inbox_focus, cx);
        let store = MailStore::new(mock_emails());
        let mut labels = LabelStore::new();
        // Seed a couple of labels so the section and the chips are not empty
        // on a cold start; the mock mails get them assigned below.
        let work = labels.create("Work");
        let personal = labels.create("Personal");
        if let Some(work) = &work {
            labels.set_for(EmailId::from(1), &[work.id]);
            labels.set_for(EmailId::from(4), &[work.id]);
        }
        if let Some(personal) = &personal {
            labels.set_for(EmailId::from(7), &[personal.id]);
        }
        let new_label_field =
            cx.new(|cx| TextField::new("new-label", "New label", "", true, 3, cx));
        let new_label_focus = cx.focus_handle().tab_index(4).tab_stop(true);
        // The theme is published as a global, so subscribing is what repaints
        // this view when the light mode switch flips it.
        let theme_subscription = cx.observe_global::<Theme>(|_, cx| cx.notify());
        // Settings come off disk before the first frame, not during it: a
        // remembered light mode has to be the palette the window opens with,
        // or the app flashes dark and then corrects itself.
        // Where settings live. Production resolves the real config directory.
        // A test gets a throwaway file of its own, because `set_setting` saves
        // on every change and a test that flipped a switch would otherwise
        // write over the settings of whoever is running the suite — which is
        // exactly what happened the first time this was wired up.
        #[cfg(not(test))]
        let settings_store = SettingsStore::with_config_dir()
            // No config directory means nowhere to keep preferences. The temp
            // directory is worse than the config directory and better than
            // dropping them on every change, and it only happens when the
            // environment has no home to speak of.
            .unwrap_or_else(|_| {
                SettingsStore::new(std::env::temp_dir().join("nori").join("settings.json"))
            });
        #[cfg(test)]
        let settings_store = SettingsStore::new(std::env::temp_dir().join(format!(
            "nori-settings-test-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        )));
        let settings_state = settings_store.load().unwrap_or_default();
        cx.set_global(Theme::for_light_mode(settings_state.light_mode));
        // Production resolves the real pointer. A test gets a throwaway file,
        // because signing out deletes the pointer and no test should be able
        // to delete the one the person running the suite signs in with.
        #[cfg(not(test))]
        let account_pointer: Option<std::path::PathBuf> = None;
        #[cfg(test)]
        let account_pointer = Some(std::env::temp_dir().join(format!(
            "nori-pointer-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        )));
        Self {
            store,
            labels,
            label_menu: None,
            selected_label: None,
            new_label_field,
            new_label_focus,
            labels_collapsed: false,
            label_composer_open: false,
            inbox_focus,
            workspace_focus,
            tab_scroll: ScrollHandle::new(),
            inbox_scroll: UniformListScrollHandle::new(),
            sidebar_visible: true,
            tab_strip_visible: true,
            sidebar_width: SIDEBAR_DEFAULT_WIDTH,
            mailboxes_collapsed: false,
            shortcuts_visible: false,
            sidebar_resize: None,
            compose_pane_width: COMPOSE_PANE_WIDTH,
            compose_resize: None,
            compose: None,
            images: HashMap::new(),
            search: None,
            settings: None,
            settings_page: SettingsPage::General,
            settings_state,
            settings_store,
            index_cache_override: None,
            account_pointer,
            account: AccountState::default(),
            loaded_mailboxes: HashSet::new(),
            exhausted_mailboxes: HashSet::new(),
            watching_more: false,
            syncing: false,
            sync_stream: None,
            search_stream: None,
            search_generation: 0,
            _theme_subscription: theme_subscription,
            settings_subscription: None,
            compose_subscription: None,
            search_subscription: None,
            settings_previous_focus: None,
            compose_previous_focus: None,
            search_previous_focus: None,
            compose_draft: None,
        }
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.overlay().is_some() {
            return;
        }
        self.store.close_active_tab();
        self.focus_after_tab_change(window, cx);
        cx.notify();
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.overlay().is_some() {
            return;
        }
        self.store.cycle_tab(1);
        self.focus_workspace(window, cx);
        cx.notify();
    }

    fn previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.overlay().is_some() {
            return;
        }
        self.store.cycle_tab(-1);
        self.focus_workspace(window, cx);
        cx.notify();
    }

    fn dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        if self.label_menu.is_some() {
            self.close_label_menu(cx);
        } else if self.search.is_some() {
            // The popup is the topmost thing on screen, so it goes first and
            // whatever opened it — the compose pane, or settings — stays.
            self.close_search(window, cx);
        } else if self.compose.is_some() {
            self.close_compose(window, cx);
        } else if self.settings.is_some() {
            self.close_settings(window, cx);
        } else if self.store.workspace_view() == WorkspaceView::Mailbox {
            return;
        } else {
            self.store.dismiss();
            window.focus(&self.inbox_focus, cx);
        }
        cx.notify();
    }

    /// Settings takes over the workspace beside the mail sidebar, so it also
    /// parks the mail overlays and remembers where focus came from.
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }
        // The composer is only hidden while settings is open, never destroyed.
        // Settings is a detour: coming back out of it — a mailbox, or the back
        // chevron — must return the user to the draft they left, not to an empty
        // composer. The pane is already filtered out of the layout while
        // `settings` is set, so nothing needs tearing down here.
        //
        // Search is modal and would sit on top of settings, so it is the one
        // thing that actually goes.
        if self.search.is_some() {
            self.close_search(window, cx);
        }
        self.settings_previous_focus = window.focused(cx);
        self.settings_page = SettingsPage::General;
        let settings_entity = cx.entity();
        let page_entity = cx.entity();
        let sign_in_entity = cx.entity();
        let sign_out_entity = cx.entity();
        let state_entity = cx.entity();
        let page = self.settings_page;
        // A reader, not a snapshot: the account page has to follow the app
        // through signing in, a sync landing and Disconnect. Handing it a copy
        // left it rendering a Disconnect button over an account that was
        // already gone.
        let account_entity = cx.entity();
        let settings = cx.new(|cx| {
            SettingsView::new(
                window,
                cx,
                move |cx| state_entity.read_with(cx, |this, _| this.settings_state),
                page,
                move |setting, enabled, cx| {
                    settings_entity.update(cx, |this, cx| this.set_setting(setting, enabled, cx));
                },
                move |page, cx| {
                    page_entity.update(cx, |this, cx| this.set_settings_page(page, cx));
                },
                move |cx| account_entity.read_with(cx, |this, _| this.account.clone()),
                move |cx| {
                    sign_in_entity.update(cx, |this, cx| this.start_sign_in(cx));
                },
                move |cx| {
                    sign_out_entity.update(cx, |this, cx| this.sign_out(cx));
                },
            )
        });
        let subscription =
            cx.subscribe_in(
                &settings,
                window,
                |this, _, event, window, cx| match event {
                    SettingsEvent::Dismiss => this.close_settings(window, cx),
                },
            );
        self.settings = Some(settings);
        self.settings_subscription = Some(subscription);
        cx.notify();
    }

    /// Drop the settings view and its subscription, without touching focus.
    ///
    /// Focus is the caller's problem because there are two ways out of
    /// settings and they want different things: closing it restores where the
    /// user was, whereas making room for a compose pane must leave focus
    /// alone or it would be yanked back to the mailbox the pane is opening
    /// over.
    fn take_settings(&mut self) -> bool {
        if self.settings.take().is_none() {
            return false;
        }
        // Only this view's listener. The composer's lives in its own slot and
        // has to outlive a settings visit, or the composer's own close button
        // goes dead while it is still on screen.
        //
        // The focus slot is deliberately left alone: `close_settings` still
        // reads it. A caller that is *not* returning focus to where settings
        // was opened clears it itself, so the two exits differ.
        self.settings_subscription = None;
        true
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_settings();
        if let Some(previous) = self.settings_previous_focus.take() {
            window.focus(&previous, cx);
        } else {
            window.focus(&self.inbox_focus, cx);
        }
        cx.notify();
    }

    /// Show or hide the pinned-mail strip. Nothing is unpinned, so the tabs
    /// come back intact and `ctrl-tab` keeps working while it is hidden.
    fn toggle_tab_strip(
        &mut self,
        _: &ToggleTabStrip,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab_strip_visible = !self.tab_strip_visible;
        cx.notify();
    }

    fn open_settings_action(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.is_some() {
            self.close_settings(window, cx);
        } else {
            self.open_settings(window, cx);
        }
    }

    /// The single writer for settings state. The settings view reports an
    /// intended value rather than flipping it itself, so this stays the only
    /// place the app decides what a setting means.
    fn set_setting(&mut self, setting: Setting, enabled: bool, cx: &mut Context<Self>) {
        if self.settings_state.set(setting, enabled) {
            // The theme is the one setting that lives outside this struct: it
            // is published as a global so every view repaints from one place
            // instead of each one being handed a copy. This is the only writer.
            if setting == Setting::LightMode {
                cx.set_global(Theme::for_light_mode(self.settings_state.light_mode));
            }
            // The switch the account page carries, so a sync can be asked for
            // without waiting for a timer that has no visible state.
            if setting == Setting::CheckForMail && enabled {
                self.sync(cx);
            }
            // Written on every change, not on the way out: there is no way out.
            // A switch that only survived a clean exit is a switch the user
            // cannot rely on.
            self.persist_settings();
            cx.notify();
        }
    }

    /// Write the settings out. Best effort — a read-only config directory
    /// should not stop someone flipping a switch, it should only mean the
    /// choice does not outlive the session.
    fn persist_settings(&self) {
        self.write_settings(&self.settings_store);
    }

    /// The write itself, given a store, so a test can point it somewhere of
    /// its own instead of over the user's real settings.
    fn write_settings(&self, store: &SettingsStore) {
        let _ = store.save(&self.settings_state);
    }

    /// The single writer for the open settings page, for the same reason
    /// `set_setting` owns the values: the top bar reads this to name the page,
    /// so it has to agree with whatever the settings view is drawing.
    fn set_settings_page(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        if self.settings_page != page {
            self.settings_page = page;
            cx.notify();
        }
    }

    /// The only writer of the account state.
    ///
    /// Ten-odd places set it — signing in, a sync landing, a sync failing,
    /// signing out — and the settings page renders it, so "changed the state"
    /// and "the page must be told" are the same act here rather than two that
    /// have to be remembered together at every site.
    fn set_account(&mut self, account: AccountState, cx: &mut Context<Self>) {
        self.account = account;
        // The settings page is its own entity, so notifying the app does not
        // reach it. It renders this state, so it is told here rather than at
        // each of the ten-odd places that could have forgotten.
        if let Some(settings) = self.settings.clone() {
            settings.update(cx, |_, cx| cx.notify());
        }
    }

    /// Begin an OAuth sign-in.
    ///
    /// Split deliberately. The listener is bound and the browser opened on the
    /// main thread, because opening a URL needs an `App`; everything that can
    /// block — waiting on the redirect, the token exchange, the first sync —
    /// runs on a background thread, because all of it is network I/O and the
    /// UI thread is where a dropped frame is visible.
    fn start_sign_in(&mut self, cx: &mut Context<Self>) {
        if matches!(self.account, AccountState::Connecting) {
            return;
        }
        let credentials = match nori_gmail::credentials() {
            Ok(credentials) => credentials,
            Err(error) => {
                self.set_account(
                    AccountState::Failed {
                        reason: format!("{error}"),
                    },
                    cx,
                );
                cx.notify();
                return;
            }
        };
        let request = match nori_gmail::oauth::begin(&credentials) {
            Ok(request) => request,
            Err(error) => {
                self.set_account(
                    AccountState::Failed {
                        reason: format!("could not start the sign-in: {error}"),
                    },
                    cx,
                );
                cx.notify();
                return;
            }
        };

        // The user has to see this, so it is a state change rather than a
        // silent wait on a browser that may never have opened.
        self.set_account(AccountState::Connecting, cx);
        cx.notify();
        cx.open_url(&request.url);

        let (redirect_uri, verifier) =
            (request.redirect_uri.clone(), request.verifier().to_owned());
        // First half: the browser round trip and the token. Nothing here reads
        // mail, so the account has no mailbox to talk about until it lands.
        let task = cx.background_executor().spawn(async move {
            let code = request.await_callback()?;
            let agent = nori_gmail::agent();
            let token =
                nori_gmail::oauth::exchange(&agent, &credentials, &redirect_uri, &code, &verifier)?;

            // The account key is the address, which is only known now.
            let profile = nori_gmail::gmail::profile(&agent, &token)?;
            let store = nori_gmail::FileTokenStore::with_account(&profile.email)?;
            nori_gmail::TokenStore::save(&store, &token)?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(profile.email)
        });

        cx.spawn(async move |this, cx| {
            // Every branch lands back on the main thread, so app state is only
            // ever written here and never from the worker.
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(address) => {
                    // Record which account was connected. Without this the
                    // token file is written and then never found again: the
                    // next launch asks the pointer which account to load, finds
                    // nothing, and reports "No account connected" with a
                    // working grant sitting on disk.
                    if let Some(pointer) = this.account_pointer() {
                        this.remember_account(&pointer, &address);
                    }
                    this.fetch_after_sign_in(address, cx);
                }
                Err(error) => {
                    this.set_account(
                        AccountState::Failed {
                            reason: format!("{error}"),
                        },
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    /// Sync an already-connected account.
    ///
    /// Incremental when the stored cursor is still good, full when Gmail has
    /// aged it out. That distinction is not an optimisation: a stale
    /// `historyId` makes Gmail answer 404, and treating that as an error would
    /// leave a mailbox that silently stopped updating, since the id never
    /// becomes fresh again on its own.
    fn sync(&mut self, cx: &mut Context<Self>) {
        if !self.account.is_usable() {
            return;
        }
        // One *indexing* fetch at a time. The rate limiter is built per fetch,
        // so two live fetches each pace at the self-limit and together exceed
        // what Google allows — the budgets on their own do not prevent that.
        // The flag doubles as the "fetching" state the empty list shows.
        if self.syncing {
            return;
        }
        let Some(address) = self.account.address().map(str::to_string) else {
            return;
        };
        let Ok(credentials) = nori_gmail::credentials() else {
            return;
        };
        let Ok(store) = nori_gmail::FileTokenStore::with_account(&address) else {
            return;
        };
        let since = self.store.synced_history_id().map(str::to_string);
        let account = address.clone();
        self.syncing = true;
        let (stream, incoming) = nori_gmail::mail_stream();
        self.sync_stream = Some(stream.clone());
        self.pump_incoming(incoming, cx);
        let task = cx.background_executor().spawn(async move {
            let mut sync = nori_gmail::Sync::new(nori_gmail::agent(), &credentials, &store)?;
            let counts = sync.folder_counts().ok();
            let outcome = match since.as_deref() {
                // `None` here means the cursor aged out, and a full sync is
                // the documented recovery rather than a failure to report.
                Some(since) => match sync.incremental(since)? {
                    Some(delta) => nori_gmail::SyncOutcome::Changes(delta),
                    None => nori_gmail::SyncOutcome::Full(sync.full_with(Some(&stream))?),
                },
                None => nori_gmail::SyncOutcome::Full(sync.full_with(Some(&stream))?),
            };
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((account, outcome, counts))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                // The in-flight guard's own flag, cleared on every exit. Leaving
                // it set on the error path is a trap for whatever reads it next:
                // a failed sync would wedge the app into refusing to fetch
                // anything again, and the guard would be reporting a fetch that
                // finished minutes ago as though it were still running.
                this.syncing = false;
                let Ok((account, outcome, counts)) = result else {
                    cx.notify();
                    return;
                };
                if let AccountState::Connected { counts: slot, .. } = &mut this.account {
                    *slot = counts;
                }
                match outcome {
                    nori_gmail::SyncOutcome::Full(snapshot) => {
                        this.store
                            .set_synced_history_id(snapshot.history_id.clone());
                        this.absorb(snapshot, cx);
                        this.save_index(&account);
                    }
                    nori_gmail::SyncOutcome::Changes(delta) => {
                        this.store.set_synced_history_id(delta.history_id.clone());
                        this.apply_delta(&delta, &account, cx);
                        this.save_index(&account);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Apply an incremental pass. Each mail is merged rather than swapped, so
    /// a pin survives, and a body already fetched is not thrown away and
    /// re-downloaded.
    fn apply_delta(
        &mut self,
        delta: &nori_gmail::Incremental,
        account: &str,
        cx: &mut Context<Self>,
    ) {
        for remote in &delta.changed {
            let incoming = to_email(remote, account);
            // A mail whose body is already loaded keeps it: `upsert` replaces
            // the record wholesale, so the body has to be carried over or the
            // reading pane would blank and re-fetch.
            let body = self
                .store
                .email(&incoming.id)
                .filter(|email| email.body_loaded)
                .map(|email| (email.body.clone(), email.pinned));
            let mut incoming = incoming;
            if let Some((paragraphs, _)) = body {
                incoming.body = paragraphs;
                incoming.body_loaded = true;
            }
            self.store.upsert(incoming);
        }
        for id in &delta.deleted {
            self.store.remove(&EmailId::from(id.as_str()));
        }
        cx.notify();
    }

    /// Fold a synced snapshot into the store.
    ///
    /// `replace_emails` rather than `upsert` per mail: a first sync has no
    /// local mail worth keeping, and the two differ in what they preserve, so
    /// the choice is made once, here.
    fn absorb(&mut self, snapshot: nori_gmail::Snapshot, cx: &mut Context<Self>) {
        let account = snapshot.account.clone();
        for remote in &snapshot.labels {
            self.seed_label(&crate::model::to_label_seed(remote));
        }
        for remote in &snapshot.mail {
            self.store.upsert(crate::model::to_email(remote, &account));
        }
        cx.notify();
    }

    /// Fetch a mailbox's mail the first time it is opened.
    ///
    /// The quota is per minute per user, so the mailbox cannot be indexed in
    /// advance — 6,000 units a minute is 300 mails, and a real mailbox holds
    /// thousands. Fetching the folder the user actually opened turns that from
    /// a limitation into the right behaviour: a folder nobody opens costs
    /// nothing, and the one they open is populated within a second or so.
    ///
    /// Populate a folder the first time it is opened.
    fn ensure_mailbox_fetched(&mut self, mailbox: Mailbox, cx: &mut Context<Self>) {
        if self.loaded_mailboxes.contains(&mailbox) {
            return;
        }
        // Already in the index from an earlier session. Having rows on disk is
        // not the same as having nothing, so opening the folder should draw
        // them rather than spend a fetch to redraw the same mail. Mail still
        // arrives: the poll sync upserts changes whichever folder they land in,
        // and `fetch_more` pages deeper once the list runs out.
        if self.store.count(mailbox) > 0 {
            self.loaded_mailboxes.insert(mailbox);
            return;
        }
        self.fetch_folder(mailbox, false, cx);
    }

    /// Fetch the next page of the folder on screen, because the user has
    /// scrolled to the end of it.
    ///
    /// Nori holds a window of each folder rather than all of it, and the window
    /// follows the user down the list rather than sitting behind a control they
    /// have to find. A row of text at the foot of the list was the alternative
    /// and it was worse on both counts: it told the user their mail was
    /// incomplete before they had scrolled far enough to notice, and it put a
    /// permanent bar under a list that is otherwise just mail.
    ///
    /// Two things keep this from spending quota on a runaway. Only one fetch runs
    /// at a time, and a folder that gives up nothing new is marked exhausted and
    /// never asked again.
    fn fetch_more(&mut self, cx: &mut Context<Self>) {
        let mailbox = self.store.selected_mailbox();
        if self.syncing || self.exhausted_mailboxes.contains(&mailbox) {
            return;
        }
        self.fetch_folder(mailbox, true, cx);
    }

    /// Watch the list's scroll position and fetch when it runs out of rows.
    ///
    /// Started from `render` rather than from a folder switch, so it comes back
    /// on its own after the reading view has been opened and closed, and it ends
    /// when the list is no longer what is on screen. A watcher that outlived the
    /// list would be a timer running for the rest of the session to poll a scroll
    /// position nobody is looking at.
    ///
    /// This GPUI revision's scroll handle cannot be observed, only read, so it is
    /// polled. At a quarter of a second that is under the threshold where reading
    /// a number feels like lag, and it costs nothing when the answer is the same
    /// one it was a quarter of a second ago.
    fn watch_for_more(&mut self, cx: &mut Context<Self>) {
        if self.watching_more || !self.account.is_usable() {
            return;
        }
        self.watching_more = true;
        let handle = self.inbox_scroll.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
                // `true` means this watcher is done and the flag is released, so
                // the next `render` starts another if the list is still on
                // screen. The flag is deliberately left *set* on every other
                // path: clearing it in passing would let a frame start a second
                // watcher beside this one, and two watchers polling the same
                // scroll position is how a page gets fetched twice.
                let stop = this
                    .update(cx, |this, cx| {
                        if !matches!(this.store.workspace_view(), WorkspaceView::Mailbox) {
                            return true;
                        }
                        let mailbox = this.store.selected_mailbox();
                        if this.syncing || this.exhausted_mailboxes.contains(&mailbox) {
                            return false;
                        }
                        // Four rows of slack, so the next page is already on its
                        // way by the time the last of this one is in view. Waiting
                        // for the exact final row means the fetch always begins
                        // after the user has hit the end and stopped.
                        let held = this.store.count(mailbox);
                        let last = handle.0.borrow().base_handle.bottom_item();
                        if held == 0 || last + 4 < held {
                            return false;
                        }
                        this.fetch_more(cx);
                        false
                    })
                    .unwrap_or(true);
                if stop {
                    let _ = this.update(cx, |this, _| this.watching_more = false);
                    break;
                }
            }
        })
        .detach();
    }

    /// Fetch a folder's mail: its first page when it is opened, the next one
    /// when the user asks for more.
    ///
    /// The two are the same work. What changes is only whether it happens by
    /// itself, and the skip-what-is-held rule inside `fetch_with` is what makes
    /// asking twice return mail that is new rather than the same hundred
    /// again — which is why no page cursor has to be kept, and why this keeps
    /// working after a restart.
    ///
    /// Starred is fetched like any other folder. It used to be skipped on the
    /// reasoning that it is a filter over mail indexed elsewhere, but with only
    /// the newest two hundred held that reasoning is wrong: a star put on a
    /// three-year-old mail is in a folder Nori never fetched, so Starred came
    /// up empty. Gmail answers `is:starred` the same as any other query, at the
    /// same cost, so there is nothing to be saved by not asking.
    fn fetch_folder(&mut self, mailbox: Mailbox, more: bool, cx: &mut Context<Self>) {
        // Same reason as `sync`: two indexers at once overspend the quota
        // minute.
        if self.syncing {
            return;
        }
        let Some(address) = self.account.address().map(str::to_string) else {
            return;
        };
        let Ok(credentials) = nori_gmail::credentials() else {
            return;
        };
        // Marked only now that a fetch is genuinely on its way. Doing it before
        // these checks left a folder looking loaded when nothing was ever
        // requested, so it was never retried.
        if !more {
            self.loaded_mailboxes.insert(mailbox);
        }
        let Ok(store) = nori_gmail::FileTokenStore::with_account(&address) else {
            return;
        };
        let query = gmail_query(mailbox);
        // Everything Nori holds, so the fetch walks past it to mail that is not
        // here yet. Skipping on id rather than on folder is deliberate: a mail
        // that moved folders is still held, and asking for it again would only
        // cost quota.
        let held: std::collections::HashSet<String> = self
            .store
            .emails()
            .iter()
            .map(|email| email.id.0.clone())
            .collect();
        let (stream, incoming) = nori_gmail::mail_stream();
        self.sync_stream = Some(stream.clone());
        self.syncing = true;
        self.pump_incoming(incoming, cx);
        let task = cx.background_executor().spawn(async move {
            let mut sync = nori_gmail::Sync::new(nori_gmail::agent(), &credentials, &store)?;
            sync.fetch_with(
                query,
                nori_gmail::sync::MAILBOX_FETCH_BUDGET,
                &held,
                Some(&stream),
            )
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.syncing = false;
                let Ok(mail) = result else {
                    // Let a later attempt retry rather than remembering a
                    // folder that failed as though it had been loaded.
                    this.loaded_mailboxes.remove(&mailbox);
                    cx.notify();
                    return;
                };
                // Nothing came back, so this folder has given up everything it
                // has. Saying so by hiding the control is the honest end of it:
                // there is no page left to ask for. This holds for a first fetch
                // too, which is how an empty folder stops offering more.
                if mail.is_empty() {
                    this.exhausted_mailboxes.insert(mailbox);
                } else {
                    this.exhausted_mailboxes.remove(&mailbox);
                }
                for remote in &mail {
                    this.store.upsert(to_email(remote, &address));
                }
                // Persist the folder, same as a body and a sync. `loaded_mailboxes`
                // is in-memory only, so a folder that is never written out is
                // refetched from scratch on every launch — which is why opening
                // Drafts cost a full ~20s fetch each time instead of nothing.
                this.save_index(&address);
                cx.notify();
            });
        })
        .detach();
    }

    /// Second half of a sign-in: read the mailbox for an account that is
    /// already authenticated.
    ///
    /// Split from the browser round trip so the app can say what it is doing.
    /// Doing both in one task left the whole thing labelled "waiting for the
    /// browser", which is only true for the first few seconds — the rest is
    /// Gmail paging a mailbox, and the user was watching an apparently idle app
    /// for the better part of a minute.
    fn fetch_after_sign_in(&mut self, address: String, cx: &mut Context<Self>) {
        let Ok(credentials) = nori_gmail::credentials() else {
            self.set_account(
                AccountState::Failed {
                    reason: "the Gmail client credentials went missing between the two \
                         halves of the sign-in. Set them and try again."
                        .to_string(),
                },
                cx,
            );
            cx.notify();
            return;
        };
        let Ok(store) = nori_gmail::FileTokenStore::with_account(&address) else {
            self.set_account(
                AccountState::Failed {
                    reason: format!("could not open the token store for {address}."),
                },
                cx,
            );
            cx.notify();
            return;
        };

        self.set_account(
            AccountState::Fetching {
                address: address.clone(),
            },
            cx,
        );
        self.syncing = true;
        cx.notify();

        let task = cx.background_executor().spawn(async move {
            let mut sync = nori_gmail::Sync::new(nori_gmail::agent(), &credentials, &store)?;
            // One label read, so the sidebar's badges are right the moment the
            // account appears. Cheap next to the mail it sits beside.
            let counts = sync.folder_counts().ok();
            let snapshot = sync.full()?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((snapshot, counts))
        });

        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.syncing = false;
                match result {
                    Ok((snapshot, counts)) => {
                        this.install_account(address, snapshot, counts, cx);
                        // The store is the account's now, so it is worth
                        // writing down: a launch that finds the token but no
                        // index pays for the whole sync again.
                        if let Some(account) = this.account.address().map(str::to_string) {
                            this.save_index(&account);
                        }
                    }
                    Err(error) => {
                        this.set_account(
                            AccountState::Failed {
                                reason: format!("{error}"),
                            },
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Put a freshly connected account into the app.
    ///
    /// The prototype's sample mail and its seeded labels go first, in one go and
    /// unconditionally: the only way to reach this is by connecting an account,
    /// so whatever is on screen belongs to the prototype rather than to the
    /// account. `absorb` merges rather than replaces, so anything left behind
    /// would sit in the user's inbox beside their real mail — which is exactly
    /// what it did before this was its own step.
    ///
    /// The cursor comes from the snapshot, so the next launch is incremental
    /// rather than paying for another full sync.
    fn install_account(
        &mut self,
        address: String,
        snapshot: nori_gmail::Snapshot,
        counts: Option<nori_gmail::FolderCounts>,
        cx: &mut Context<Self>,
    ) {
        let mail = snapshot.mail.len();
        let labels = snapshot.labels.len();
        let history_id = snapshot.history_id.clone();
        self.store.clear();
        self.labels.clear();
        self.store.set_synced_history_id(history_id);
        self.absorb(snapshot, cx);
        self.set_account(
            AccountState::Connected {
                address,
                mail,
                labels,
                counts,
            },
            cx,
        );
    }

    /// Record which account Nori is signed in as.
    ///
    /// The token file is named after the address, so on its own it says nothing
    /// about which account to load. This pointer is the only record of that, and
    /// `resume_account` reads it at every launch. A sign-in that does not write
    /// it looks, on the next start, exactly like a sign-in that never happened.
    fn remember_account(&self, pointer: &nori_gmail::LastAccount, address: &str) {
        let _ = pointer.save(address);
    }

    /// Create a Nori label for a Gmail one, without duplicating it on every
    /// sync.
    fn seed_label(&mut self, seed: &crate::model::account::LabelSeed) {
        if self
            .labels
            .labels()
            .iter()
            .any(|label| label.name == seed.name)
        {
            return;
        }
        self.labels.create_with_colour(&seed.name, seed.colour);
    }

    /// The pointer to the last signed-in account, wherever that is.
    ///
    /// One function so no caller can quietly reach the real config directory
    /// instead: this file is deleted on sign-out, and a test that signs out
    /// should be deleting its own.
    fn account_pointer(&self) -> Option<nori_gmail::LastAccount> {
        match &self.account_pointer {
            Some(path) => Some(nori_gmail::LastAccount::new(path.clone())),
            None => nori_gmail::LastAccount::with_config_dir().ok(),
        }
    }

    /// Forget the account: the token, the synced mail, and the state.
    fn sign_out(&mut self, cx: &mut Context<Self>) {
        let address = self.account.address().map(str::to_string);
        if let Some(address) = &address
            && let Ok(store) = nori_gmail::FileTokenStore::with_account(address)
        {
            let _ = nori_gmail::TokenStore::clear(&store);
        }
        // The synced mail goes with the token: leaving it behind would show a
        // mailbox belonging to an account that is no longer connected.
        self.store.clear();
        if let Some(pointer) = self.account_pointer() {
            pointer.clear();
        }
        if let Some(address) = &address
            && let Ok(cache) = IndexCache::with_account(address)
        {
            cache.clear();
        }
        self.set_account(AccountState::Disconnected, cx);
        cx.notify();
    }

    /// Poll for changes on a timer, for as long as the app is open.
    ///
    /// Cheap enough to be uninteresting: an incremental pass is one
    /// `history.list` at 2 quota units, so a poll every few minutes costs
    /// under a hundred units an hour against an allowance of 6,000 a minute.
    /// The full sync is the expensive part and happens once, on launch or when
    /// the stored cursor has aged out.
    ///
    /// Not a focus-based trigger, which would be the obvious choice, because
    /// this GPUI revision has no window focus event to hang it on.
    fn poll_for_changes(&self, cx: &mut Context<Self>) {
        let interval = std::time::Duration::from_secs(POLL_INTERVAL_SECONDS);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let _ = this.update(cx, |this, cx| {
                    // A disconnected or half-signed-in account has nothing to
                    // poll, and `sync` is a no-op there anyway; skipping it keeps
                    // the timer from waking the UI for no reason.
                    if this.account.is_usable() {
                        this.sync(cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Resume the account that was connected last. Called by the binary, not by
    /// [`Self::new`], because it reads `~/.config/nori`.
    ///
    /// The order here is the whole point. The local index is loaded and drawn
    /// *first*, so the list is on screen in milliseconds, and only then does the
    /// network get involved — and because the index carries a sync cursor, that
    /// network call is an incremental pass costing a couple of quota units for
    /// the handful of mails that arrived since, rather than a full re-read of
    /// the mailbox at 300 mails a minute.
    ///
    /// With no index on disk this is a first run, and the prototype's sample
    /// mail is discarded before the full fetch begins.
    pub fn resume(&mut self, cx: &mut Context<Self>) {
        self.poll_for_changes(cx);
        // Nori starts with a handful of invented messages in the store so the
        // tests have something to lay out. They are a fixture, and a fixture
        // that reaches a person is indistinguishable from real mail until you
        // already know it is not — so they are cleared here, before any of the
        // paths below, and each one puts real mail back if there is any. Every
        // exit from here leaves the list either holding the user's own mail or
        // empty on purpose.
        self.store.clear();
        let Some(pointer) = self.account_pointer() else {
            cx.notify();
            return;
        };
        let Some(address) = pointer.load() else {
            cx.notify();
            return;
        };
        let Ok(store) = nori_gmail::FileTokenStore::with_account(&address) else {
            cx.notify();
            return;
        };
        match nori_gmail::TokenStore::load(&store) {
            Ok(Some(_)) => self.resume_with_index(&address, cx),
            Ok(None) => {
                // The token is gone but the pointer was not; the next sign-in
                // will rewrite both, so this is not worth reporting.
                pointer.clear();
                cx.notify();
            }
            Err(error) => {
                self.set_account(
                    AccountState::Failed {
                        reason: format!("could not read the saved token: {error}"),
                    },
                    cx,
                );
                cx.notify();
            }
        }
    }

    fn resume_with_index(&mut self, address: &str, cx: &mut Context<Self>) {
        let cached = IndexCache::with_account(address)
            .ok()
            .and_then(|cache| cache.load(address))
            .filter(|index| !index.is_empty());

        let (mail, labels) = match &cached {
            Some(index) => {
                self.store
                    .restore(index.emails.clone(), index.history_id.clone());
                self.labels
                    .restore(index.labels.clone(), index.assignments.clone());
                (index.emails.len(), index.labels.len())
            }
            None => {
                // First run: what is on screen is the prototype's sample data,
                // which belongs to no account and should not sit in a list the
                // user is trying to read.
                self.store.clear();
                self.labels.clear();
                (0, 0)
            }
        };

        self.set_account(
            AccountState::Connected {
                address: address.to_string(),
                mail,
                labels,
                counts: None,
            },
            cx,
        );
        // The Inbox is on screen without anyone having opened it, and both
        // branches above put its mail in the store: either from the index on
        // disk, or a moment from now when the sync lands. Marking it loaded
        // here is what keeps the first visit to the Inbox from quietly fetching
        // another page the user never asked for — the control at the foot of the
        // list is for asking, not for arriving.
        self.loaded_mailboxes.insert(Mailbox::Inbox);
        cx.notify();
        self.sync(cx);
    }

    /// Write the index out after a sync, so the next launch has something to
    /// draw before it makes a request. Refuses to let a thin store clobber a
    /// fuller file — see [`may_replace_index`] — so a partial state can never
    /// strand the next launch on an empty list.
    fn save_index(&self, account: &str) {
        let cache = match self.index_cache_override.clone() {
            Some(cache) => cache,
            None => match IndexCache::with_account(account) {
                Ok(cache) => cache,
                Err(_) => return,
            },
        };
        self.write_index(account, &cache);
    }

    /// The write itself, given a cache, so a test can point it somewhere of its
    /// own instead of over the index of the account it is really signed in to.
    fn write_index(&self, account: &str, cache: &IndexCache) {
        let snapshot = self.store.snapshot();
        if let Some(current) = cache.load(account)
            && !may_replace_index(&current, snapshot.len())
        {
            return;
        }
        let (labels, assignments) = self.labels.snapshot();
        let _ = cache.save(&Index {
            account: account.to_string(),
            emails: snapshot,
            labels,
            assignments,
            history_id: self.store.synced_history_id().map(str::to_string),
        });
    }

    /// How tall the mail list rows are right now.
    fn density(&self) -> Density {
        Density::from_compact_rows(self.settings_state.compact_rows)
    }

    fn focus_after_tab_change(&self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.store.workspace_view(), WorkspaceView::Email(_)) {
            window.focus(&self.workspace_focus, cx);
        } else {
            window.focus(&self.inbox_focus, cx);
        }
    }

    fn focus_workspace(&self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.store.workspace_view(), WorkspaceView::Email(_)) {
            window.focus(&self.workspace_focus, cx);
        }
    }

    fn select_mailbox(&mut self, mailbox: Mailbox, window: &mut Window, cx: &mut Context<Self>) {
        // The mail sidebar stays live behind settings, so picking a mailbox is
        // a way out of settings as well as a way to change folder.
        if self.settings.is_some() {
            self.close_settings(window, cx);
        }
        // Switching folders must not cost the user their draft. The composer is
        // a second column, not a modal, so it stays put beside whatever the
        // workspace shows: glancing at another mailbox mid-sentence keeps the
        // text. `close_overlay` is deliberately not called here — it drops the
        // `ComposeView` entity, and with it every field the user had filled in.
        //
        // Search is the one thing that must go: it is modal and occludes the
        // sidebar, so it should not be open here at all, but if it is, it is
        // dismissed rather than left stranded.
        if self.search.take().is_some() {
            self.sync_overlay();
        }
        self.store.select_mailbox(mailbox);
        self.ensure_mailbox_fetched(mailbox, cx);
        window.focus(&self.inbox_focus, cx);
        cx.notify();
    }

    /// Open a mail, and fetch its body if the list only ever had metadata.
    ///
    /// The reading pane opens immediately with the snippet and then fills in.
    /// Waiting for the body first would put a network round trip between the
    /// click and the pane appearing, which is the one thing that makes a mail
    /// client feel slow.
    fn open_email(&mut self, id: EmailId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(email) = self.store.email(&id).cloned() else {
            return;
        };
        if self.store.open_email(id.clone()) {
            window.focus(&self.workspace_focus, cx);
            cx.notify();
        }
        // Read state has to round-trip. This is the reason the account asks for
        // `gmail.modify` rather than `gmail.readonly`: without the write-back,
        // Nori marks a mail read locally and Gmail still lists it as unread,
        // so the two disagree and the next sync undoes the read. The gate is
        // that it was unread *before*, so re-opening a read mail costs nothing
        // — and the state was already in hand for the body check.
        let was_unread = email.unread;
        let account = email.write_back_account().map(str::to_string);
        if was_unread && let Some(account) = account.clone() {
            self.write_back(
                account,
                WriteBack {
                    id: id.0.clone(),
                    add: Vec::new(),
                    remove: vec![UNREAD.to_string()],
                },
                cx,
            );
        }
        if email.body_loaded {
            // Pictures are not account-bound, so they load for sample mail
            // too — or rather, they would, if sample mail had any.
            self.fetch_images_for(&id, cx);
            return;
        }
        let Some(account) = account else {
            return;
        };
        self.fetch_body(account, id.0, cx);
    }

    /// Start fetching a mail's remote pictures, once each: repeats across
    /// mails share the one entry, and settled entries are never re-fetched.
    /// Runs both when a mail opens with its body already in hand and when a
    /// fetched body lands, so neither path shows placeholders forever.
    fn fetch_images_for(&mut self, id: &EmailId, cx: &mut Context<Self>) {
        let Some(email) = self.store.email(id) else {
            return;
        };
        let mut sources = nori_gmail::image_sources(&email.body);
        sources.truncate(MAX_IMAGES_PER_MAIL);
        let mut started = false;
        for src in sources {
            if self.images.contains_key(&src) || !is_fetchable_image_url(&src) {
                continue;
            }
            self.insert_image_slot(src.clone(), ImageSlot::Loading);
            started = true;
            let for_fetch = src.clone();
            let task = cx.background_executor().spawn(async move {
                fetch_image_bytes(&for_fetch).map(|(format, bytes)| {
                    (
                        format,
                        std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)),
                    )
                })
            });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.insert_image_slot(
                        src,
                        match result {
                            Some((_, image)) => ImageSlot::Loaded(image),
                            None => ImageSlot::Failed,
                        },
                    );
                    cx.notify();
                });
            })
            .detach();
        }
        if started {
            cx.notify();
        }
    }

    /// Insert a slot, keeping the cache bounded: finished pictures survive
    /// eviction first, and when even those overflow the whole cache restarts
    /// rather than growing without end. A cleared picture simply loads again
    /// if its mail is still open.
    fn insert_image_slot(&mut self, url: String, slot: ImageSlot) {
        if self.images.len() >= MAX_CACHED_IMAGES {
            self.images
                .retain(|_, slot| matches!(slot, ImageSlot::Loaded(_)));
            if self.images.len() >= MAX_CACHED_IMAGES {
                self.images.clear();
            }
        }
        self.images.insert(url, slot);
    }

    /// Pull one mail's body off the thread and hand it to the store.
    fn fetch_body(&mut self, account: String, id: String, cx: &mut Context<Self>) {
        let Ok(credentials) = nori_gmail::credentials() else {
            return;
        };
        let Ok(store) = nori_gmail::FileTokenStore::with_account(&account) else {
            return;
        };
        // The worker only needs the id; this copy is for writing the index back
        // once the bodies land.
        let account_for_save = account.clone();
        let task = cx.background_executor().spawn(async move {
            let mut sync = nori_gmail::Sync::new(nori_gmail::agent(), &credentials, &store)?;
            let bodies = sync.bodies(&[id])?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(bodies)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                // A body that never arrives leaves `body_loaded` false, so the
                // next open tries again rather than treating the mail as empty.
                if let Ok(bodies) = result {
                    for (id, body) in bodies {
                        this.store.set_body(&EmailId::from(id.as_str()), body);
                        this.fetch_images_for(&EmailId::from(id.as_str()), cx);
                    }
                    // Write the index here too. Bodies are part of the cached
                    // record, so without this the mail you just read is fetched
                    // again on every launch — the store had it, the file never
                    // learned that.
                    this.save_index(&account_for_save);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(email) = self.store.selected_email().cloned() {
            self.open_email(email.id, window, cx);
        }
    }

    /// Open the row's overflow menu under the button, or close it if this row
    /// already has it open. Toggling on the same row keeps the menu from
    /// stranding itself open with no way to dismiss.
    fn open_label_menu(&mut self, id: EmailId, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.label_menu = match self.label_menu.take() {
            Some((open, _)) if open == id => None,
            _ => Some((id, position)),
        };
        cx.notify();
    }

    fn close_label_menu(&mut self, cx: &mut Context<Self>) {
        if self.label_menu.take().is_some() {
            cx.notify();
        }
    }

    /// One label in the menu, filled when the mail carries it.
    fn render_label_menu_item(
        &self,
        entity: Entity<Self>,
        email: EmailId,
        label: &Label,
        theme: Theme,
    ) -> impl IntoElement {
        let (text, border, fill) = label.chip();
        let is_on = self.labels.labels_for(&email).contains(&label.id);
        let label_id = label.id;
        div()
            .id(format!("label-menu-item-{}", label.id))
            .debug_selector(move || format!("label-menu-item-{}", label_id))
            .w_full()
            .h(px(30.))
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .cursor_pointer()
            .role(Role::MenuItem)
            .aria_label(format!(
                "{} {}",
                if is_on { "Remove" } else { "Add" },
                label.name
            ))
            .aria_selected(is_on)
            .focusable()
            .tab_stop(true)
            .focus_visible(|style| style.border_color(theme.focus))
            .hover(|style| style.bg(theme.hover))
            // The menu stays open across a toggle: assigning one label is
            // rarely the whole job, and closing under the pointer each time
            // would make applying two feel like a chore.
            .on_click({
                let entity = entity.clone();
                move |_event, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.toggle_label(label_id, email.clone(), cx)
                    })
                }
            })
            // A filled swatch rather than a tick: the label's own hue is what
            // identifies it here and in the sidebar, and a tick would compete
            // with it for the same slot.
            .child(
                div()
                    .size(px(10.))
                    .flex_none()
                    .bg(if is_on { fill } else { transparent_black() })
                    .border_1()
                    .border_color(if is_on { border } else { theme.hairline_strong }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(if is_on { text } else { theme.muted })
                    .child(label.name.clone()),
            )
    }

    /// Star or unstar a mail, and write the change back when the mail is real.
    ///
    /// The local change lands first so the star moves under the pointer, and
    /// the request follows. A write-back that fails is reported rather than
    /// retried silently: a star that disagrees between Nori and Gmail is worse
    /// than one the user was told did not save.
    fn toggle_star(&mut self, id: EmailId, cx: &mut Context<Self>) {
        let Some(email) = self.store.email(&id).cloned() else {
            return;
        };
        self.store.toggle_star(id.clone());
        let starred = self.store.email(&id).is_some_and(|email| email.starred);

        // `None` for sample mail, which has no server to tell.
        let Some(account) = email.write_back_account().map(str::to_string) else {
            return;
        };
        self.save_index(&account);
        self.write_back(
            account,
            WriteBack {
                id: id.0,
                add: starred.then(|| STARRED.to_string()).into_iter().collect(),
                remove: (!starred)
                    .then(|| STARRED.to_string())
                    .into_iter()
                    .collect(),
            },
            cx,
        );
    }

    /// Send a label change to Gmail, off the main thread.
    ///
    /// Taking the account by value keeps the error path able to name it: a
    /// failed write-back has to say *which* account needs re-authorising.
    fn write_back(&mut self, account: String, change: WriteBack, cx: &mut Context<Self>) {
        let Ok(credentials) = nori_gmail::credentials() else {
            return;
        };
        let Ok(store) = nori_gmail::FileTokenStore::with_account(&account) else {
            return;
        };
        let task = cx.background_executor().spawn(async move {
            let mut sync = nori_gmail::Sync::new(nori_gmail::agent(), &credentials, &store)?;
            let add: Vec<&str> = change.add.iter().map(String::as_str).collect();
            let remove: Vec<&str> = change.remove.iter().map(String::as_str).collect();
            sync.modify(&change.id, &add, &remove)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Err(error) = result else { return };
                let text = format!("{error}");
                // `invalid_grant` is the expected weekly end of a Testing-mode
                // app's grant, so it is a state and not an error toast.
                if text.contains("no longer valid") || text.contains("sign in again") {
                    this.set_account(AccountState::NeedsReauth { address: account }, cx);
                } else if !matches!(this.account, AccountState::Disconnected) {
                    this.set_account(AccountState::Failed { reason: text }, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_compose(&mut self, seed: DraftSeed, window: &mut Window, cx: &mut Context<Self>) {
        if self.compose.is_some() {
            return;
        }
        // The compose pane is a workspace sibling, so it cannot appear while
        // settings is holding the workspace. Asking to compose from settings
        // therefore means leaving settings, showing the mailbox, and opening
        // there — otherwise it is a no-op behind a pane the user cannot see.
        //
        // Only that case moves the workspace. Composing from an open message
        // deliberately keeps the message on screen beside the pane, which is
        // the whole point of the divider.
        let from_settings = self.take_settings();
        if from_settings {
            // Focus is going to the compose pane, not back to wherever settings
            // was opened from.
            self.settings_previous_focus = None;
            if !matches!(self.store.workspace_view(), WorkspaceView::Mailbox) {
                // Same mailbox, list view: leaving settings for a compose pane
                // should not also move the user to a different folder.
                let mailbox = self.store.selected_mailbox();
                self.select_mailbox(mailbox, window, cx);
            }
        }
        // A blank seed is just "open a compose pane", so a draft parked by a
        // previous close comes back. A real seed — a reply, a forward — always
        // wins and retires the parked draft, because the user asked for that
        // mail specifically.
        let seed = if seed.is_blank() && self.compose_draft.is_some() {
            self.compose_draft.take().unwrap_or_default()
        } else {
            self.compose_draft = None;
            seed
        };
        // Where closing the pane should send focus. Normally whatever held it
        // beforehand — but coming from settings that handle belongs to a view
        // we just discarded, so restoring it would focus a dead element and
        // leave the pane's close button unreachable by keyboard. The list is
        // the only sensible exit from that route.
        self.compose_previous_focus = if from_settings {
            Some(self.inbox_focus.clone())
        } else {
            window.focused(cx)
        };
        let compose = cx.new(|cx| ComposeView::new(seed, window, cx));
        let subscription =
            cx.subscribe_in(&compose, window, |this, _, event, window, cx| match event {
                ComposeEvent::Dismiss => this.close_compose(window, cx),
            });
        self.compose = Some(compose);
        self.compose_subscription = Some(subscription);
        self.sync_overlay();
        cx.notify();
    }

    /// Put the compose pane away, keeping whatever was in it.
    ///
    /// Only the pane goes. The draft is lifted out first, because the fields
    /// live in the entity and the entity is what is being dropped — closing the
    /// pane used to take the half-written mail with it.
    fn close_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(compose) = self.compose.take() else {
            return;
        };
        let draft = compose.read(cx).draft(cx);
        if !draft.is_blank() {
            self.compose_draft = Some(draft);
        }
        self.compose_subscription = None;
        self.sync_overlay();
        if let Some(previous) = self.compose_previous_focus.take() {
            window.focus(&previous, cx);
        }
        cx.notify();
    }

    /// Re-derive the store's single overlay flag from what is actually open.
    /// Search wins the slot because it is the popup and sits on top.
    fn sync_overlay(&mut self) {
        self.store.set_overlay(
            self.search
                .as_ref()
                .map(|_| Overlay::Search)
                .or(self.compose.as_ref().map(|_| Overlay::Compose)),
        );
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_some() {
            return;
        }
        self.search_previous_focus = window.focused(cx);
        let emails = self.store.emails().to_vec();
        // With an account connected, the answer comes from Gmail, which knows
        // about all of it. Nori only holds the tail it has synced, and
        // searching that alone would quietly report "no match" for mail that
        // exists.
        let remote = self.account.address().is_some();
        let search = cx.new(|cx| SearchView::new(emails, remote, window, cx));
        let subscription =
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                SearchEvent::Open(id) => {
                    // A hit from Gmail is not in the store — only the folder
                    // fetches were written there — so bring it across before
                    // opening, or the reading view has nothing to show.
                    if this.store.email(id).is_none()
                        && let Some(view) = &this.search
                        && let Some(email) = view.read(cx).email(id)
                    {
                        this.store.upsert(email);
                    }
                    this.close_search(window, cx);
                    this.open_email(id.clone(), window, cx);
                }
                SearchEvent::Dismiss => this.close_search(window, cx),
                SearchEvent::QueryChanged(query) => this.search_gmail(query.clone(), cx),
            });
        self.search = Some(search);
        self.search_subscription = Some(subscription);
        self.sync_overlay();
        cx.notify();
    }

    /// Put the search popup away. The compose pane underneath is untouched —
    /// they are separate layers, and dismissing a popup that was opened over it
    /// used to take the draft with it.
    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.take().is_none() {
            return;
        }
        self.search_subscription = None;
        self.sync_overlay();
        if let Some(previous) = self.search_previous_focus.take() {
            window.focus(&previous, cx);
        }
        cx.notify();
    }

    /// Ask Gmail what matches, once the typing has settled.
    ///
    /// The pause is the point: a search costs 20 units per message, and one per
    /// keystroke would burn the quota on prefixes nobody meant to search for.
    fn search_gmail(&mut self, query: String, cx: &mut Context<Self>) {
        self.search_generation += 1;
        let generation = self.search_generation;
        let Some(address) = self.account.address().map(str::to_string) else {
            return;
        };
        let Ok(credentials) = nori_gmail::credentials() else {
            return;
        };
        let Ok(store) = nori_gmail::FileTokenStore::with_account(&address) else {
            return;
        };
        let (stream, incoming) = nori_gmail::mail_stream();
        self.search_stream = Some(stream.clone());
        // The raw text goes straight through: Gmail's own search operators
        // (`from:`, `after:`, `has:attachment`) are worth having, and Nori has
        // no business second-guessing a query the user can see.
        let trimmed = query.trim().to_string();
        self.pump_search_results(incoming, query, generation, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(350))
                .await;
            // The wait is over; if the user typed on, this is an answer to a
            // question they have already moved past.
            let superseded = this
                .update(cx, |this, _| this.search_generation != generation)
                .unwrap_or(true);
            if superseded {
                return;
            }
            let task = cx.background_executor().spawn(async move {
                let mut sync = nori_gmail::Sync::new(nori_gmail::agent(), &credentials, &store)?;
                sync.fetch_with(
                    &trimmed,
                    nori_gmail::sync::SEARCH_BUDGET,
                    &std::collections::HashSet::new(),
                    Some(&stream),
                )
            });
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                // Only the newest search is allowed to call it finished; an
                // older one landing now would switch the line off while a newer
                // request is still out.
                if this.search_generation != generation {
                    return;
                }
                this.search_stream = None;
                if let Some(view) = &this.search {
                    view.update(cx, |view, cx| match result {
                        // A failed search says so. Falling through to "nothing
                        // matches" would report a confident lie about the
                        // user's own mail.
                        Err(error) => view.set_error(error.to_string(), cx),
                        Ok(_) => view.set_searching(false, cx),
                    });
                }
            });
        })
        .detach();
    }

    fn reply_seed(email: &Email, reply_all: bool, forward: bool) -> DraftSeed {
        let subject = if forward {
            format!("Fwd: {}", email.subject)
        } else {
            format!("Re: {}", email.subject)
        };
        let to = if forward {
            String::new()
        } else if reply_all {
            email
                .recipients
                .iter()
                .filter(|recipient| *recipient != "me@example.com")
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            email.address.clone()
        };
        let body = if forward {
            format!(
                "\n\n--- Forwarded message ---\nFrom: {} <{}>\n\n{}",
                email.sender,
                email.address,
                nori_gmail::plain_text(&email.body)
            )
        } else {
            format!(
                "\n\nOn {}:\n{}",
                email.full_date,
                nori_gmail::plain_text(&email.body)
            )
        };
        DraftSeed { to, subject, body }
    }

    /// The row overflow menu, drawn as a layer above the mail shell.
    ///
    /// Absolutely positioned from the click point rather than from the row's
    /// own box, for two reasons: the list is virtualized, so a row's y is only
    /// valid on the frame it was drawn and the menu would drift if the list
    /// scrolled; and a menu inside a list row would be clipped by the list's
    /// overflow, cutting off the bottom of the menu on the last visible row.
    fn render_label_menu(
        &mut self,
        entity: Entity<Self>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let theme = Theme::current(cx);
        let (email, position) = self.label_menu.clone()?;
        let labels = self.labels.labels().to_vec();
        let width = px(LABEL_MENU_WIDTH);
        // Flip to the other side of the button rather than running off the
        // right edge of a narrow window.
        let width_offset = if position.x + width + px(16.) > px(LABEL_MENU_EDGE_GUARD) {
            -width - px(8.)
        } else {
            px(8.)
        };

        Some(
            div()
                .id("label-menu-layer")
                .absolute()
                .inset_0()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_label_menu(cx)),
                )
                .child(
                    div()
                        .id("label-menu")
                        .debug_selector(|| "label-menu".into())
                        .absolute()
                        .left(position.x + width_offset)
                        .top(position.y)
                        .w(width)
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .p(px(5.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .bg(theme.raised)
                        .border_1()
                        .border_color(theme.strong_border)
                        .shadow_lg()
                        // Clicks inside must not reach the dismiss layer.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(labels.iter().map(|label| {
                            self.render_label_menu_item(entity.clone(), email.clone(), label, theme)
                        }))
                        .when(labels.is_empty(), |this| {
                            this.child(
                                div()
                                    .px(px(10.))
                                    .py(px(8.))
                                    .text_size(px(12.))
                                    .text_color(theme.faint)
                                    .child("No labels yet — create one in the sidebar"),
                            )
                        })
                        .into_any_element(),
                )
                .into_any_element(),
        )
    }

    /// Search only. Compose is not an overlay: it is a second column in the
    /// workspace, so the mail list stays visible while you write.
    fn render_overlay(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let theme = Theme::current(cx);
        let child: gpui::AnyElement = if let Some(search) = &self.search {
            search.clone().into_any_element()
        } else {
            return None;
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .bg(theme.overlay)
                .occlude()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.close_search(window, cx);
                    }),
                )
                .child(
                    // Plain flex centring, not `anchored()`. Anchored in Window
                    // mode anchors to its own layout origin unless given an
                    // explicit position, so `TopCenter` resolved against
                    // (0, 0), put the panel at a negative x, and the edge
                    // clamp then pinned it to the left margin instead of the
                    // centre. It also displaced the panel's hit region from
                    // its layout bounds, so clicks on the contents missed.
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .justify_center()
                        .pt(px(OVERLAY_TOP_OFFSET))
                        .child(
                            // Stops mouse events so a click inside the panel
                            // is not read as a click on the backdrop.
                            div()
                                .id("overlay-panel")
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .child(child),
                        ),
                )
                .into_any_element(),
        )
    }

    fn move_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self.store.overlay().is_none() {
            self.store.move_selection(delta);
            scroll_selected_into_view(&self.inbox_scroll, self.store.selected_index());
            cx.notify();
        }
    }

    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        self.sidebar_resize = None;
        window.focus(&self.inbox_focus, cx);
        cx.notify();
    }

    /// Show or hide the sidebar's shortcut hints, following the modifier.
    /// Only repaints on a change, so sliding other modifiers around does not
    /// churn the sidebar.
    fn set_shortcuts_visible(&mut self, showing: bool, cx: &mut Context<Self>) {
        if self.shortcuts_visible != showing {
            self.shortcuts_visible = showing;
            cx.notify();
        }
    }

    /// The `Ctrl+1`..`Ctrl+6` shortcuts. Each names its mailbox at compile
    /// time because a gpui action carries no payload, so this is six
    /// one-liners rather than one handler with an argument.
    fn go_mailbox(&mut self, mailbox: Mailbox, window: &mut Window, cx: &mut Context<Self>) {
        self.select_mailbox(mailbox, window, cx);
    }

    fn toggle_mailboxes(&mut self, cx: &mut Context<Self>) {
        self.mailboxes_collapsed = !self.mailboxes_collapsed;
        cx.notify();
    }

    /// Everything the sidebar's Labels section can ask for. The section is
    /// `RenderOnce`, so the group state and the composer state both live here.
    fn labels_action(&mut self, action: LabelsAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            LabelsAction::ToggleCollapsed => {
                self.labels_collapsed = !self.labels_collapsed;
                // Collapsing out from under a live field would leave the
                // composer open and invisible, so expanding again would bring
                // back a field the user thought they had dismissed.
                if self.labels_collapsed {
                    self.label_composer_open = false;
                }
            }
            LabelsAction::ToggleComposer => {
                if self.label_composer_open {
                    self.close_label_composer(window, cx);
                    return;
                }
                // Opening the composer from a collapsed group has to expand it,
                // or the `+` would appear to do nothing.
                self.labels_collapsed = false;
                self.label_composer_open = true;
                let field = self.new_label_field.clone();
                window.focus(&field.read(cx).focus_handle(), cx);
            }
            LabelsAction::CloseComposer => {
                if !self.label_composer_open {
                    return;
                }
                self.close_label_composer(window, cx);
            }
        }
        cx.notify();
    }

    /// Put the composer away and hand focus back to the `+`, so the keyboard
    /// can open it again without reaching for the mouse.
    fn close_label_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.label_composer_open = false;
        self.new_label_field
            .update(cx, |this, cx| this.set_content("", cx));
        window.focus(&self.new_label_focus, cx);
    }

    fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let next = clamp_sidebar_width(width);
        if next != self.sidebar_width {
            self.sidebar_width = next;
            cx.notify();
        }
    }

    fn begin_sidebar_resize(&mut self, start_x: f32, cx: &mut Context<Self>) {
        self.sidebar_resize = Some(SidebarResize {
            start_x,
            start_width: self.sidebar_width,
        });
        cx.notify();
    }

    fn update_sidebar_resize(&mut self, cursor_x: f32, cx: &mut Context<Self>) {
        if let Some(resize) = &self.sidebar_resize {
            let width = resize.start_width + (cursor_x - resize.start_x);
            self.set_sidebar_width(width, cx);
        }
    }

    /// The bound comes from the window, so widening the composer never depends
    /// on a constant that a large display has already exhausted.
    fn set_compose_pane_width(&mut self, width: f32, window: &Window, cx: &mut Context<Self>) {
        let max = compose_pane_max(f32::from(window.viewport_size().width), self.sidebar_width);
        let next = clamp_compose_pane_width(width, max);
        if next != self.compose_pane_width {
            self.compose_pane_width = next;
            cx.notify();
        }
    }

    fn begin_compose_resize(&mut self, start_x: f32, cx: &mut Context<Self>) {
        self.compose_resize = Some(ComposeResize {
            start_x,
            start_width: self.compose_pane_width,
        });
        cx.notify();
    }

    /// The divider sits on the pane's left edge, so the width moves opposite
    /// to the pointer: dragging left widens the pane, dragging right narrows
    /// it. Both ends are clamped, and the upper clamp scales with the window.
    fn update_compose_resize(&mut self, cursor_x: f32, window: &Window, cx: &mut Context<Self>) {
        if let Some(resize) = &self.compose_resize {
            let width = resize.start_width - (cursor_x - resize.start_x);
            self.set_compose_pane_width(width, window, cx);
        }
    }

    fn end_compose_resize(&mut self, cx: &mut Context<Self>) {
        if self.compose_resize.take().is_some() {
            cx.notify();
        }
    }

    /// Keyboard equivalent of the drag, so the divider is not pointer-only.
    fn nudge_compose_pane(&mut self, step: f32, window: &Window, cx: &mut Context<Self>) {
        self.set_compose_pane_width(self.compose_pane_width + step, window, cx);
    }

    fn end_sidebar_resize(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_resize.take().is_some() {
            cx.notify();
        }
    }

    fn open_selected_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.overlay().is_none() {
            self.open_selected(window, cx);
        }
    }

    /// `Ctrl+N` toggles the compose pane. Reopening it brings back whatever
    /// was last in it, so a half-written mail survives being put away.
    fn compose_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.compose.is_some() {
            self.close_compose(window, cx);
        } else if self.search.is_none() {
            self.open_compose(DraftSeed::default(), window, cx);
        }
    }

    /// `Ctrl+S` toggles the search popup. It is a popup, so it opens over
    /// whatever else is showing — the compose pane, or the settings pane — and
    /// closing it hands focus back to the layer underneath.
    fn search_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_some() {
            self.close_search(window, cx);
        } else {
            self.open_search(window, cx);
        }
    }

    fn move_selection_down(
        &mut self,
        _: &MoveSelectionDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selection(1, cx);
    }

    fn move_selection_up(
        &mut self,
        _: &MoveSelectionUp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selection(-1, cx);
    }

    fn open_selected_shortcut_action(
        &mut self,
        _: &OpenSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_selected_shortcut(window, cx);
    }

    fn compose_shortcut_action(
        &mut self,
        _: &Compose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.compose_shortcut(window, cx);
    }

    fn search_shortcut_action(
        &mut self,
        _: &OpenSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_shortcut(window, cx);
    }

    /// Pin or unpin the open mail. Unpinning also closes its tab.
    ///
    /// Written to the index immediately, the way a star is. A pin is Nori's own
    /// state with no server equivalent, so there is nothing to write back and
    /// the index file is the only place it can live — which means a pin that
    /// was not saved was not kept. It was previously saved only as a side
    /// effect of some later sync or body fetch happening to run, so pinning a
    /// mail and quitting lost it.
    fn toggle_pin(&mut self, id: EmailId, cx: &mut Context<Self>) {
        if !self.store.toggle_pin(id.clone()) {
            return;
        }
        cx.notify();

        // `None` for sample mail, which has no index file to be written to.
        let Some(account) = self
            .store
            .email(&id)
            .and_then(|email| email.write_back_account())
            .map(str::to_string)
        else {
            return;
        };
        self.save_index(&account);
    }

    /// Retrace the previous view, the way the sidebar's back chevron does.
    fn go_back_action(&mut self, _: &GoBack, window: &mut Window, cx: &mut Context<Self>) {
        self.go_back(window, cx);
    }

    fn go_forward_action(&mut self, _: &GoForward, window: &mut Window, cx: &mut Context<Self>) {
        self.go_forward(window, cx);
    }

    fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Settings first: it is the most recent thing the user did, and closing
        // it is what brings the composer back into view. The chevron is the
        // affordance for "undo that", so it takes precedence over mail history.
        if self.settings.is_some() {
            self.close_settings(window, cx);
            return;
        }
        self.store.go_back();
        self.focus_after_history_change(window, cx);
    }

    fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.store.go_forward();
        self.focus_after_history_change(window, cx);
    }

    fn focus_after_history_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.workspace_view() {
            WorkspaceView::Email(_) => window.focus(&self.workspace_focus, cx),
            WorkspaceView::Mailbox => window.focus(&self.inbox_focus, cx),
        }
        cx.notify();
    }

    fn toggle_sidebar_action(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.store.overlay().is_none() {
            self.toggle_sidebar(window, cx);
        }
    }

    /// Create a label from the sidebar's inline field, then clear the field.
    fn create_label(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(label) = self.labels.create(&name) else {
            // Blank or duplicate: leave the text and the composer open so it
            // can be corrected.
            return;
        };
        // One label per visit: the composer closes and the `+` takes focus
        // back, rather than leaving a cleared field waiting for the next one.
        self.close_label_composer(window, cx);
        self.selected_label = Some(label.id);
        cx.notify();
    }

    /// Filter the list by a label, or clear the filter when it is re-picked.
    fn select_label(&mut self, label: Option<LabelId>, cx: &mut Context<Self>) {
        self.selected_label = label;
        cx.notify();
    }

    /// Delete a label; it is removed from every mail that carried it, and the
    /// filter clears if that label was the one in use.
    fn delete_label(&mut self, id: LabelId, cx: &mut Context<Self>) {
        self.labels.remove(id);
        if self.selected_label == Some(id) {
            self.selected_label = None;
        }
        cx.notify();
    }

    /// Rename a label, keeping its colour. A blank or duplicate name is
    /// refused and the old name stays, which is why nothing is cleared here.
    fn rename_label(&mut self, id: LabelId, name: String, cx: &mut Context<Self>) {
        self.labels.rename(id, &name);
        cx.notify();
    }

    /// The single writer for label assignment. The list row's overflow menu
    /// routes through here so the reading view and the menu cannot drift.
    fn toggle_label(&mut self, id: LabelId, email: EmailId, cx: &mut Context<Self>) {
        self.labels.toggle(email, id);
        cx.notify();
    }

    fn render_inbox(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entity = cx.entity();
        let open_entity = entity.clone();
        let star_entity = entity.clone();
        let menu_entity = entity.clone();
        // A selected label narrows the list to the mails carrying it, on top of
        // whatever mailbox is showing.
        let mut rows = self.store.visible_summaries();
        if let Some(label) = self.selected_label {
            rows.retain(|row| self.labels.labels_for(&row.id).contains(&label));
        }
        // Resolve each row's chips in row order, so the list can draw what
        // a mail carries without reaching back into the store per row.
        let labels: Vec<Vec<Label>> = rows
            .iter()
            .map(|row| {
                self.labels
                    .labels_for(&row.id)
                    .iter()
                    .filter_map(|id| self.labels.label(*id).cloned())
                    .collect()
            })
            .collect();
        // An empty list says why it is empty. Which reason matters: with no
        // account there is a thing to do about it, and without one the honest
        // answer is that the folder is simply empty, and offering a button that
        // cannot help would be worse than saying nothing.
        if rows.is_empty() {
            let sign_in_entity = cx.entity();
            let empty = match &self.account {
                // An account is on its way, so the list being empty is a fact
                // about progress, not about mail. Saying so beats an empty pane
                // the user cannot interpret.
                AccountState::Connecting => crate::views::empty_state::Empty::SigningIn,
                AccountState::Fetching { address } => {
                    crate::views::empty_state::Empty::FetchingMail(
                        // Leaked to the element's own lifetime, which is the
                        // frame; the variant is only read while drawing.
                        address.clone(),
                    )
                }
                _ if self.account.address().is_some() => {
                    crate::views::empty_state::Empty::Folder(self.store.selected_mailbox().label())
                }
                _ => crate::views::empty_state::Empty::NotSignedIn,
            };
            return crate::views::empty_state::render(
                Theme::current(cx),
                &empty,
                move |_window, cx| {
                    sign_in_entity.update(cx, |this, cx| this.start_sign_in(cx));
                },
            );
        }

        let selected_index = self.store.selected_index();
        Inbox::new(
            rows,
            labels,
            selected_index,
            self.inbox_focus.clone(),
            self.inbox_scroll.clone(),
            self.density(),
            move |id, window, cx| {
                open_entity.update(cx, |this, cx| this.open_email(id, window, cx));
            },
            move |id, _window, cx| {
                star_entity.update(cx, |this, cx| this.toggle_star(id, cx));
            },
            move |id, position, _window, cx| {
                menu_entity.update(cx, |this, cx| this.open_label_menu(id, position, cx));
            },
        )
        .into_any_element()
    }

    /// The list.
    ///
    /// Nothing is drawn in place of the list while mail loads, and nothing
    /// is wrapped around it either. Each message goes into the store the
    /// moment it comes back from Gmail, so the inbox fills in with real rows
    /// as the fetch runs — the fetching is visible without a thing drawn
    /// to say so. An earlier version wrapped the list in a column to carry a
    /// progress counter, and the wrapper collapsed the list to zero height for
    /// the whole length of every sync, blanking the inbox at exactly the
    /// moment it was being waited on.
    /// The list.
    ///
    /// Nothing is drawn in place of the list while mail loads, and — just as
    /// importantly — nothing is wrapped around it. Each message goes into the
    /// store the moment it comes back from Gmail, so the inbox fills in with
    /// real rows as the fetch runs; the fetching is visible without a thing
    /// drawn to say so.
    ///
    /// A wrapper here is a trap worth recording, because it has been built
    /// twice. Putting the list inside a column to carry something beneath it
    /// collapses the list to zero height for the whole length of every sync, so
    /// the affordance is bought with a blank inbox at the exact moment the
    /// inbox is being waited on. Whatever belongs below the list belongs below
    /// the split that contains it; see the call in `render`.
    fn render_list(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        self.watch_for_more(cx);
        self.render_inbox(cx)
    }

    /// Move mail from the fetch into the list, as it arrives.
    ///
    /// The workers hand each message over a channel; this drains that channel
    /// onto the main thread and repaints. Drained with `try_recv` on a timer
    /// rather than a blocking receive, because this runs on the foreground
    /// executor and blocking there would freeze the window — which is the one
    /// thing the whole exercise is meant to avoid.
    ///
    /// The loop ends by itself: the fetch closes the stream when it is done, so
    /// the channel reports disconnected and this is not a permanent timer.
    fn pump_incoming(&self, mut incoming: nori_gmail::IncomingMail, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut batch = Vec::new();
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(60))
                    .await;
                batch.clear();
                let open = nori_gmail::drain(&mut incoming, &mut batch);
                let arrived = !batch.is_empty();
                // Two ways out, and both are needed. A full fetch closes the
                // stream, so the channel reports disconnected. An incremental one
                // never does — it is a handful of ids already known, and there
                // is nothing to hand over as it arrives — so the only signal
                // that it has finished is `syncing` going false. Watching the
                // channel alone leaves this loop waking sixteen times a second
                // for the rest of the session, which is not a crash but is a
                // background timer that can never be idle and never stops.
                let finished = this
                    .update(cx, |this, cx| {
                        if arrived {
                            for remote in batch.drain(..) {
                                this.store
                                    .upsert(crate::model::to_email(&remote, this.fetch_account()));
                            }
                            cx.notify();
                        }
                        !this.syncing
                    })
                    .unwrap_or(true);
                if finished || (!open && !arrived) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The account newly fetched mail belongs to. Falls back to the connected
    /// account; the stream is only ever created for a real one.
    fn fetch_account(&self) -> &str {
        self.account.address().unwrap_or_default()
    }

    /// Move search hits from the request into the dialog, as they arrive.
    ///
    /// Separate from [`Self::pump_incoming`] because search mail must not land
    /// in the store: a hit may be from a folder Nori has never opened, and
    /// writing it in would have the sidebar claim a message the Inbox does not
    /// hold. So the results go to the dialog and nowhere else, and the reading
    /// view is handed the one that gets opened.
    fn pump_search_results(
        &self,
        mut incoming: nori_gmail::IncomingMail,
        query: String,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let account = self.fetch_account().to_string();
        cx.spawn(async move |this, cx| {
            let mut batch = Vec::new();
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(60))
                    .await;
                batch.clear();
                let open = nori_gmail::drain(&mut incoming, &mut batch);
                let arrived = !batch.is_empty();
                let account = account.clone();
                let query = query.clone();
                let _ = this.update(cx, |this, cx| {
                    if this.search_generation != generation {
                        return;
                    }
                    if !arrived {
                        return;
                    }
                    let found: Vec<_> = batch
                        .drain(..)
                        .map(|remote| crate::model::to_email(&remote, &account))
                        .collect();
                    if let Some(view) = &this.search {
                        view.update(cx, |view, cx| view.add_results(&query, found, cx));
                    }
                });
                if !open && !arrived {
                    break;
                }
            }
        })
        .detach();
    }
}

impl Render for MailApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let entity = cx.entity();
        let sidebar_entity = entity.clone();
        let sidebar_search_entity = entity.clone();
        let compose_entity = entity.clone();
        let settings_entity = entity.clone();
        let group_entity = entity.clone();
        let toggle_entity = entity.clone();
        let back_entity = entity.clone();
        let forward_entity = entity.clone();
        let resize_start_entity = entity.clone();
        let resize_step_entity = entity.clone();
        let label_entity = entity.clone();
        let labels_action_entity = entity.clone();
        let create_label_entity = entity.clone();
        let delete_label_entity = entity.clone();
        let rename_label_entity = entity.clone();
        let counts: [usize; 6] = std::array::from_fn(|index| {
            let mailbox = Mailbox::NAV_ITEMS[index];
            self.account
                .count_of(mailbox, self.store.unread_count(mailbox))
        });
        let sidebar = Sidebar::new(
            self.store.selected_mailbox(),
            counts,
            self.sidebar_width,
            self.sidebar_visible,
            self.mailboxes_collapsed,
            self.shortcuts_visible,
            move |mailbox, window, cx| {
                sidebar_entity.update(cx, |this, cx| this.select_mailbox(mailbox, window, cx));
            },
            move |window, cx| {
                sidebar_search_entity.update(cx, |this, cx| this.open_search(window, cx));
            },
            move |window, cx| {
                compose_entity.update(cx, |this, cx| {
                    this.open_compose(DraftSeed::default(), window, cx)
                });
            },
            move |window, cx| {
                settings_entity.update(cx, |this, cx| this.open_settings(window, cx));
            },
            move |window, cx| {
                toggle_entity.update(cx, |this, cx| this.toggle_sidebar(window, cx));
            },
            move |window, cx| {
                back_entity.update(cx, |this, cx| this.go_back(window, cx));
            },
            move |window, cx| {
                forward_entity.update(cx, |this, cx| this.go_forward(window, cx));
            },
            self.store.can_go_back(),
            self.store.can_go_forward(),
            move |_window, cx| {
                group_entity.update(cx, |this, cx| this.toggle_mailboxes(cx));
            },
            self.labels.labels().to_vec(),
            self.selected_label,
            self.labels_collapsed,
            self.label_composer_open,
            self.new_label_field.clone(),
            self.new_label_focus.clone(),
            move |action, window, cx| {
                labels_action_entity.update(cx, |this, cx| this.labels_action(action, window, cx));
            },
            move |label, _window, cx| {
                label_entity.update(cx, |this, cx| this.select_label(label, cx));
            },
            move |name, window, cx| {
                create_label_entity.update(cx, |this, cx| this.create_label(name, window, cx));
            },
            move |id, _window, cx| {
                delete_label_entity.update(cx, |this, cx| this.delete_label(id, cx));
            },
            move |id, name, _window, cx| {
                rename_label_entity.update(cx, |this, cx| this.rename_label(id, name, cx));
            },
            move |start_x, _window, cx| {
                resize_start_entity.update(cx, |this, cx| this.begin_sidebar_resize(start_x, cx));
            },
            move |delta, _window, cx| {
                resize_step_entity.update(cx, |this, cx| {
                    let width = this.sidebar_width + delta;
                    this.set_sidebar_width(width, cx);
                });
            },
        );
        let top_entity = cx.entity();
        let modifiers_entity = top_entity.clone();
        let top_toggle_entity = top_entity.clone();
        // An open email reads as `Mailbox / Subject`; the mailbox view is the
        // bare label. Settings borrows the same bar, so it reads as
        // `Settings / Page` while it holds the workspace.
        let (prefix, title): (Option<SharedString>, SharedString) = if self.settings.is_some() {
            (
                Some("Settings".into()),
                self.settings_page.label().to_string().into(),
            )
        } else {
            match self.store.workspace_view() {
                WorkspaceView::Email(id) => match self.store.email(&id) {
                    Some(email) => (
                        Some(email.mailbox.label().into()),
                        email.subject.clone().into(),
                    ),
                    None => (None, self.store.selected_mailbox().label().into()),
                },
                WorkspaceView::Mailbox => (None, self.store.selected_mailbox().label().into()),
            }
        };
        let tabs: Vec<(EmailId, SharedString)> = self
            .store
            .tabs()
            .iter()
            .filter_map(|id| {
                self.store
                    .email(id)
                    .map(|email| (id.clone(), email.subject.clone().into()))
            })
            .collect();
        let tab_entity = cx.entity();
        let select_tab_entity = tab_entity.clone();
        let close_tab_entity = tab_entity.clone();
        let tabs_component = if tabs.is_empty() || !self.tab_strip_visible {
            None
        } else {
            Some(EmailTabs::new(
                tabs,
                self.store.active_tab(),
                self.tab_scroll.clone(),
                move |id, window, cx| {
                    select_tab_entity.update(cx, |this, cx| this.open_email(id, window, cx));
                },
                move |id, window, cx| {
                    close_tab_entity.update(cx, |this, cx| {
                        this.store.close_tab(id);
                        this.focus_after_tab_change(window, cx);
                        cx.notify();
                    });
                },
            ))
        };
        let top_bar = TopBar::new(self.sidebar_visible, prefix, title, move |window, cx| {
            top_toggle_entity.update(cx, |this, cx| this.toggle_sidebar(window, cx));
        });
        let workspace: gpui::AnyElement = match self.settings.clone() {
            // Settings holds the workspace beside the mail sidebar. The mail
            // views are not built at all while it is open, so the list behind
            // it costs nothing.
            Some(settings) => settings.into_any_element(),
            None => match self.store.workspace_view() {
                WorkspaceView::Mailbox => self.render_list(cx),
                WorkspaceView::Email(id) => {
                    if let Some(email) = self.store.email(&id).cloned() {
                        let reply = Self::reply_seed(&email, false, false);
                        let reply_all = Self::reply_seed(&email, true, false);
                        let forward = Self::reply_seed(&email, false, true);
                        let view_entity = cx.entity();
                        let reply_entity = view_entity.clone();
                        let reply_all_entity = view_entity.clone();
                        let forward_entity = view_entity.clone();
                        let pin_entity = view_entity.clone();
                        let email_id = email.id.clone();
                        EmailView::new(
                            email,
                            self.workspace_focus.clone(),
                            self.images.clone(),
                            move |window, cx| {
                                reply_entity.update(cx, |this, cx| {
                                    this.open_compose(reply.clone(), window, cx)
                                });
                            },
                            move |window, cx| {
                                reply_all_entity.update(cx, |this, cx| {
                                    this.open_compose(reply_all.clone(), window, cx)
                                });
                            },
                            move |window, cx| {
                                forward_entity.update(cx, |this, cx| {
                                    this.open_compose(forward.clone(), window, cx)
                                });
                            },
                            move |_window, cx| {
                                pin_entity
                                    .update(cx, |this, cx| this.toggle_pin(email_id.clone(), cx));
                            },
                        )
                        .into_any_element()
                    } else {
                        self.render_list(cx)
                    }
                }
            },
        };
        let overlay = self.render_overlay(cx);
        let label_menu = self.render_label_menu(entity, cx);
        let settings_open = self.settings.is_some();
        // Compose's column. Fixed width rather than a percentage so the mail
        // list keeps a readable measure instead of being squeezed by however
        // wide the window happens to be.
        let compose_pane = self.compose.clone().map(|compose| {
            let resize_entity = cx.entity();
            let handle = div()
                .id("compose-pane-resize")
                .debug_selector(|| "compose-pane-resize".into())
                .absolute()
                .top_0()
                .bottom_0()
                // The strip is invisible, so it has to be generous, and it must
                // straddle the seam: the visible edge users aim at is the
                // shell's left border, so a press a few pixels into the mail
                // side has to start the drag too. An inside-only strip made
                // every grab from the mail side a miss, which read as "drag
                // left does nothing" because growing the composer is exactly
                // the gesture that starts from that side.
                .left(px(-8.))
                .w(px(24.))
                .cursor(CursorStyle::ResizeLeftRight)
                .role(Role::Splitter)
                .aria_label("Resize compose")
                .focusable()
                .tab_stop(true)
                .focus_visible(|style| style.bg(theme.focus))
                .group("compose-resize")
                .on_mouse_down(MouseButton::Left, {
                    let resize_entity = resize_entity.clone();
                    move |event, _window, cx| {
                        cx.stop_propagation();
                        resize_entity.update(cx, |this, cx| {
                            this.begin_compose_resize(event.position.x.as_f32(), cx)
                        });
                    }
                })
                .on_key_down({
                    let resize_entity = resize_entity.clone();
                    move |event, window, cx| {
                        let step = if event.keystroke.modifiers.shift {
                            40.
                        } else {
                            12.
                        };
                        match event.keystroke.key.as_str() {
                            "left" | "right" => {
                                let delta = if event.keystroke.key.as_str() == "left" {
                                    step
                                } else {
                                    -step
                                };
                                resize_entity.update(cx, |this, cx| {
                                    this.nudge_compose_pane(delta, window, cx)
                                });
                                cx.stop_propagation();
                            }
                            "home" => {
                                resize_entity.update(cx, |this, cx| {
                                    this.set_compose_pane_width(COMPOSE_PANE_MIN, window, cx)
                                });
                                cx.stop_propagation();
                            }
                            "end" => {
                                // As wide as this window allows, which is not a
                                // fixed number.
                                resize_entity.update(cx, |this, cx| {
                                    let max = compose_pane_max(
                                        f32::from(window.viewport_size().width),
                                        this.sidebar_width,
                                    );
                                    this.set_compose_pane_width(max, window, cx)
                                });
                                cx.stop_propagation();
                            }
                            _ => {}
                        }
                    }
                });

            div()
                .id("compose-pane-shell")
                .debug_selector(|| "compose-pane-shell".into())
                .relative()
                .flex_none()
                .w(px(self.compose_pane_width))
                .h_full()
                .flex()
                .child(compose)
                .child(handle)
        });
        // Explicit width for the mail column while compose is open. Flex
        // minimums follow content width here, so a flex_1 main refuses to
        // shrink below the reading view's measure: the composer then grows
        // past the window edge (Send slides off-screen) instead of the seam
        // travelling. Deriving the width from the window makes the seam
        // track the divider 1:1 by construction.
        let main_width = match (&compose_pane, settings_open) {
            (Some(_), false) => {
                let sidebar = if self.sidebar_visible {
                    self.sidebar_width
                } else {
                    0.
                };
                Some(
                    (f32::from(window.viewport_size().width) - sidebar - self.compose_pane_width)
                        .max(0.),
                )
            }
            _ => None,
        };

        div()
            .id("mail-app")
            .relative()
            .size_full()
            .flex()
            // Fires on the modifier going down as well as coming up, so the
            // sidebar hint needs no timer and no polling. `control` and
            // `platform` are both watched because the shortcuts are bound
            // against Ctrl and Cmd alike.
            .on_modifiers_changed(move |event, _window, cx| {
                modifiers_entity.update(cx, |this, cx| {
                    this.set_shortcuts_visible(
                        event.modifiers.control || event.modifiers.platform,
                        cx,
                    )
                });
            })
            .bg(theme.canvas)
            .on_action(cx.listener(Self::open_settings_action))
            .on_action(cx.listener(|this, _: &GoInbox, window, cx| {
                this.go_mailbox(Mailbox::Inbox, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GoStarred, window, cx| {
                this.go_mailbox(Mailbox::Starred, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GoSent, window, cx| {
                this.go_mailbox(Mailbox::Sent, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GoDrafts, window, cx| {
                this.go_mailbox(Mailbox::Drafts, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GoArchive, window, cx| {
                this.go_mailbox(Mailbox::Archive, window, cx)
            }))
            .on_action(cx.listener(|this, _: &GoTrash, window, cx| {
                this.go_mailbox(Mailbox::Trash, window, cx)
            }))
            .on_action(cx.listener(Self::go_back_action))
            .on_action(cx.listener(Self::go_forward_action))
            .on_action(cx.listener(Self::move_selection_down))
            .on_action(cx.listener(Self::move_selection_up))
            .on_action(cx.listener(Self::open_selected_shortcut_action))
            .on_action(cx.listener(Self::compose_shortcut_action))
            .on_action(cx.listener(Self::search_shortcut_action))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::toggle_sidebar_action))
            .on_action(cx.listener(Self::toggle_tab_strip))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.update_sidebar_resize(event.position.x.as_f32(), cx);
                this.update_compose_resize(event.position.x.as_f32(), window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    this.end_sidebar_resize(cx);
                    this.end_compose_resize(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    this.end_sidebar_resize(cx);
                    this.end_compose_resize(cx);
                }),
            )
            .child(
                // The mail shell is never torn down for settings. Sidebar and
                // top bar stay put; only the workspace and the tab strip swap,
                // and the tabs are hidden because they name open mail, which
                // settings is not.
                div().flex_1().min_h_0().flex().child(sidebar).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .relative()
                        .flex()
                        .flex_col()
                        .child(top_bar)
                        .when_some(tabs_component.filter(|_| !settings_open), |this, tabs| {
                            this.child(tabs)
                        })
                        // Compose splits the workspace rather than covering
                        // it, so the mail list stays readable while writing.
                        .child(
                            div()
                                .id("workspace-split")
                                .flex_1()
                                .min_h_0()
                                .flex()
                                // Stretch the children to the row's height. Without
                                // it `workspace-main` takes its content height, and
                                // since the list inside it sizes with `flex_1` that
                                // is circular: a short list looks fine, and a long
                                // one — more rows than the viewport — collapses to
                                // nothing because nothing tells the list how tall it
                                // is allowed to be.
                                .items_stretch()
                                .child(
                                    div()
                                        .id("workspace-main")
                                        .flex()
                                        .h_full()
                                        .min_h_0()
                                        // Clip, never overflow: without this a
                                        // wide reading view pushes the row
                                        // past the window edge.
                                        .overflow_hidden()
                                        .when_some(main_width, |this, w| this.w(px(w)).flex_none())
                                        .when(main_width.is_none(), |this| this.flex_1().min_w_0())
                                        .child(workspace),
                                )
                                .when_some(
                                    compose_pane.filter(|_| !settings_open),
                                    |this, pane| this.child(pane),
                                ),
                        ),
                ),
            )
            // Not gated on settings. The popup is `absolute().inset_0()` and
            // mounts above everything, which is the whole reason it is a popup
            // rather than a workspace pane: it can sit over the settings
            // without settings having to close first. Gating it here meant
            // `Ctrl+S` in settings set the state and drew nothing at all.
            .when_some(overlay, |this, overlay| this.child(overlay))
            .when_some(label_menu, |this, menu| this.child(menu))
            // Mounted last, so it sits above every other surface, and only
            // while a divider is actually being dragged. The root's own
            // `on_mouse_move` has to rely on the event bubbling up through
            // whatever the pointer is currently over, and a drag that crosses
            // from the compose pane onto the scrolling reading view (or the
            // other way round) is exactly the case where that path is
            // unreliable. A full-window overlay removes the question: the
            // pointer is always over the one element that wants the event.
            .when(
                self.compose_resize.is_some() || self.sidebar_resize.is_some(),
                |this| {
                    this.child(
                        div()
                            .id("divider-drag-overlay")
                            .debug_selector(|| "divider-drag-overlay".into())
                            .absolute()
                            .inset_0()
                            .cursor(CursorStyle::ResizeLeftRight)
                            .on_mouse_move(cx.listener(
                                |this, event: &MouseMoveEvent, window, cx| {
                                    this.update_sidebar_resize(event.position.x.as_f32(), cx);
                                    this.update_compose_resize(
                                        event.position.x.as_f32(),
                                        window,
                                        cx,
                                    );
                                },
                            ))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                                    this.end_sidebar_resize(cx);
                                    this.end_compose_resize(cx);
                                }),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                                    this.end_sidebar_resize(cx);
                                    this.end_compose_resize(cx);
                                }),
                            ),
                    )
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{CloseTab, Compose, OpenSelected};
    use gpui::{AppContext, TestAppContext, VisualTestContext, WindowHandle};

    const COMPOSE_PANE_MIN: f32 = super::COMPOSE_PANE_MIN;
    const MIN_READING_PANE_WIDTH: f32 = super::MIN_READING_PANE_WIDTH;

    #[gpui::test]
    fn starts_in_inbox_and_opens_selected_email(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());

        assert_eq!(
            app.read_with(&cx, |app, _| app.store.selected_mailbox()),
            Mailbox::Inbox
        );
        cx.update(|window, cx| focus.dispatch_action(&OpenSelected, window, cx));
        // Unpinned by default, so it opens without taking a tab.
        assert_eq!(app.read_with(&cx, |app, _| app.store.tabs().len()), 0);
        assert!(matches!(
            app.read_with(&cx, |app, _| app.store.workspace_view()),
            WorkspaceView::Email(_)
        ));
    }

    #[gpui::test]
    fn close_tab_returns_to_mailbox(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.update(|window, cx| focus.dispatch_action(&OpenSelected, window, cx));
        let id = match app.read_with(&cx, |app, _| app.store.workspace_view()) {
            WorkspaceView::Email(id) => id,
            WorkspaceView::Mailbox => panic!("opening a mail should show it"),
        };
        app.update(&mut cx, |app, cx| app.toggle_pin(id.clone(), cx));
        assert_eq!(app.read_with(&cx, |app, _| app.store.tabs().len()), 1);
        let workspace_focus = app.read_with(&cx, |app, _| app.workspace_focus.clone());
        cx.update(|window, cx| workspace_focus.dispatch_action(&CloseTab, window, cx));
        assert_eq!(app.read_with(&cx, |app, _| app.store.tabs().len()), 0);
        assert_eq!(
            app.read_with(&cx, |app, _| app.store.workspace_view()),
            WorkspaceView::Mailbox
        );
    }

    #[gpui::test]
    fn inbox_list_fills_available_height(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        cx.run_until_parked();
        let bounds = cx
            .debug_bounds("email-list")
            .expect("email list should be rendered");
        assert!(
            bounds.size.height > gpui::px(200.),
            "email list height should fill the workspace, got {:?}",
            bounds.size.height
        );
    }

    #[gpui::test]
    fn compact_rows_setting_shrinks_the_list_rows(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        let row_height = |cx: &mut VisualTestContext| {
            cx.debug_bounds("email-row-1")
                .expect("the first row should be rendered")
                .size
                .height
        };
        assert_eq!(row_height(&mut cx), gpui::px(40.), "compact by default");

        // The setting is what the compact layout hangs off, so flip it the
        // way the settings view would and the list has to follow.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.set_setting(Setting::CompactRows, false, cx);
            });
        });
        cx.run_until_parked();
        assert_eq!(
            row_height(&mut cx),
            gpui::px(70.),
            "turning compact rows off should give the three-line row back"
        );

        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.set_setting(Setting::CompactRows, true, cx);
            });
        });
        cx.run_until_parked();
        assert_eq!(
            row_height(&mut cx),
            gpui::px(40.),
            "turning compact rows back on should shrink them again"
        );
    }

    #[gpui::test]
    fn the_sender_column_does_not_shift_with_the_unread_bar(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();
        // Compact is the default, and this is the layout with a fixed sender
        // column, so there is nothing to set up here.
        assert!(
            app.read_with(&cx, |app, _| app.density()) == Density::Compact,
            "this test only means something while rows are compact"
        );

        // Mail 1 is unread, mail 4 is read, so their rows differ only in the
        // bar. The sender must start at the same x on both.
        let sender_x = |cx: &mut VisualTestContext, id: u16| {
            cx.debug_bounds(format!("email-row-{id}-sender").leak())
                .unwrap_or_else(|| panic!("row {id} should be rendered"))
                .origin
                .x
        };
        let unread_x = sender_x(&mut cx, 1);
        let read_x = sender_x(&mut cx, 4);
        assert_eq!(
            unread_x, read_x,
            "the unread bar must not push the sender column across"
        );
    }

    #[gpui::test]
    fn toggle_sidebar_action_flips_visibility(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        assert!(app.read_with(&cx, |app, _| app.sidebar_visible));
        cx.update(|window, cx| focus.dispatch_action(&crate::actions::ToggleSidebar, window, cx));
        assert!(!app.read_with(&cx, |app, _| app.sidebar_visible));
    }

    #[gpui::test]
    fn top_bar_stays_visible_with_toggle_only_when_hidden(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();
        let bar = cx
            .debug_bounds("top-bar")
            .expect("top bar stays above the mails");
        assert_eq!(
            bar.size.height,
            gpui::px(42.),
            "top bar keeps its height even with the toggle hidden"
        );
        assert!(
            cx.debug_bounds("top-bar-sidebar").is_none(),
            "no top bar toggle while the sidebar toggle is visible"
        );
        assert!(
            cx.debug_bounds("sidebar-toggle").is_some(),
            "sidebar header holds the shared toggle while visible"
        );
        cx.update(|window, cx| focus.dispatch_action(&crate::actions::ToggleSidebar, window, cx));
        assert!(
            !app.read_with(&cx, |app, _| app.sidebar_visible),
            "sidebar should be hidden after toggle"
        );
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("top-bar").is_some(),
            "top bar stays above the mails while hidden"
        );
        assert!(
            cx.debug_bounds("top-bar-sidebar").is_some(),
            "hidden sidebar must leave a way back"
        );
    }

    #[gpui::test]
    fn tab_strip_appears_only_for_pinned_mail(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("email-tabs").is_none(),
            "no mails are pinned at startup"
        );

        // Open a mail: it shows, but takes no tab while unpinned.
        cx.update(|window, cx| focus.dispatch_action(&OpenSelected, window, cx));
        let id = match app.read_with(&cx, |app, _| app.store.workspace_view()) {
            WorkspaceView::Email(id) => id,
            WorkspaceView::Mailbox => panic!("opening a mail should show it"),
        };
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("email-tabs").is_none(),
            "an unpinned mail must not get a tab"
        );
        assert!(
            cx.debug_bounds("email-pin").is_some(),
            "the reading view offers a pin control"
        );
        assert!(
            cx.debug_bounds("email-back").is_none(),
            "the reading view has no back button; the sidebar chevrons own that"
        );

        // Pinning it earns the tab.
        app.update(&mut cx, |app, cx| app.toggle_pin(id.clone(), cx));
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("email-tabs").is_some(),
            "a pinned mail earns a tab"
        );

        // The sidebar's back chevron returns to the mailbox, keeping the tab.
        cx.update(|window, cx| app.update(cx, |app, cx| app.go_back(window, cx)));
        cx.run_until_parked();
        assert_eq!(
            app.read_with(&cx, |app, _| app.store.workspace_view()),
            WorkspaceView::Mailbox
        );
        assert!(
            cx.debug_bounds("email-tabs").is_some(),
            "the pinned tab survives going back to the mailbox"
        );
    }

    #[gpui::test]
    fn the_tab_strip_can_be_hidden_without_unpinning_anything(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        // Pin a mail so there is a strip to hide.
        cx.update(|window, cx| focus.dispatch_action(&OpenSelected, window, cx));
        let id = match app.read_with(&cx, |app, _| app.store.workspace_view()) {
            WorkspaceView::Email(id) => id,
            WorkspaceView::Mailbox => panic!("opening a mail should show it"),
        };
        app.update(&mut cx, |app, cx| app.toggle_pin(id.clone(), cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("email-tabs").is_some());

        // The reading view is on screen now, so the inbox focus handle is
        // tracked by nothing; the workspace handle is the live one.
        let workspace_focus = app.read_with(&cx, |app, _| app.workspace_focus.clone());
        cx.update(|window, cx| workspace_focus.dispatch_action(&ToggleTabStrip, window, cx));
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("email-tabs").is_none(),
            "the keybind hides the strip"
        );
        assert!(
            app.read_with(&cx, |app, _| app.store.email(&id).is_some_and(|e| e.pinned)),
            "hiding the strip must not unpin the mail"
        );

        // And it comes back.
        cx.update(|window, cx| workspace_focus.dispatch_action(&ToggleTabStrip, window, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("email-tabs").is_some());
    }

    #[gpui::test]
    fn clicking_a_search_result_opens_that_mail(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        cx.update(|window, cx| focus.dispatch_action(&OpenSearch, window, cx));
        cx.run_until_parked();
        let row = cx
            .debug_bounds("search-result-1")
            .expect("the search dialog should list results");
        let center = row.center();
        cx.simulate_click(
            gpui::Point::new(center.x, center.y),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();

        assert!(
            app.read_with(&cx, |app, _| app.store.overlay().is_none()),
            "choosing a result should close the search"
        );
        assert_eq!(
            app.read_with(&cx, |app, _| app.store.workspace_view()),
            WorkspaceView::Email(EmailId::from(1)),
            "clicking a result should open that mail"
        );
    }

    #[gpui::test]
    fn the_search_dialog_is_centred_on_the_window(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        cx.update(|window, cx| focus.dispatch_action(&OpenSearch, window, cx));
        cx.run_until_parked();
        let dialog = cx
            .debug_bounds("search-dialog")
            .expect("the search dialog should be rendered");
        let window_width = 1200.;
        let dialog_center = dialog.origin.x + dialog.size.width / 2.;
        let window_center = gpui::px(window_width / 2.);
        assert!(
            (dialog_center - window_center).abs() < gpui::px(1.),
            "the dialog should sit on the window's centre line, got {dialog_center:?} in a \
             {window_width} wide window"
        );
    }

    #[gpui::test]
    fn compose_splits_beside_the_mail_list_instead_of_covering_it(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        let list_before = cx
            .debug_bounds("email-list")
            .expect("the mail list should be rendered");

        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();

        let pane = cx
            .debug_bounds("compose-pane-shell")
            .expect("compose should open as a column, not an overlay");
        let list_after = cx
            .debug_bounds("email-list")
            .expect("the mail list must survive compose opening");

        assert!(
            pane.origin.x >= list_after.right(),
            "compose must sit to the right of the list, not on top of it: pane starts at \
             {:?}, list ends at {:?}",
            pane.origin,
            list_after.right()
        );
        assert!(
            list_after.size.width < list_before.size.width,
            "the list should give up width to the compose column"
        );
        assert!(
            cx.debug_bounds("search-dialog").is_none(),
            "compose is not a floating overlay"
        );
    }

    /// Widening the pane was reported dead while narrowing it worked. The
    /// delta math is symmetric, so the useful guard is that a leftward drag
    /// moves the seam by exactly the distance the pointer travelled, and that
    /// a rightward drag mirrors it.
    #[gpui::test]
    fn the_compose_divider_moves_both_ways_by_the_same_distance(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();
        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();

        let seam_x = |cx: &mut VisualTestContext| {
            f32::from(cx.debug_bounds("compose-pane-shell").unwrap().origin.x)
        };
        let y = gpui::px(300.);

        let drag = |cx: &mut VisualTestContext, dx: f32| {
            let seam = seam_x(cx);
            let grab = gpui::Point::new(gpui::px(seam + 6.), y);
            cx.simulate_mouse_down(grab, gpui::MouseButton::Left, gpui::Modifiers::default());
            let to = gpui::Point::new(grab.x + gpui::px(dx), y);
            cx.simulate_mouse_move(
                to,
                Some(gpui::MouseButton::Left),
                gpui::Modifiers::default(),
            );
            cx.run_until_parked();
            let after = seam_x(cx);
            cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::default());
            cx.run_until_parked();
            (seam, after)
        };

        let (start, after_left) = drag(&mut cx, -120.);
        assert!(
            after_left < start,
            "dragging left must widen the pane: seam {start} -> {after_left}"
        );
        assert!(
            (start - after_left - 120.).abs() <= 1.,
            "the seam should track the pointer 1:1, moved {} for a 120px drag",
            start - after_left
        );

        let (_, after_right) = drag(&mut cx, 120.);
        assert!(
            (after_right - after_left - 120.).abs() <= 1.,
            "dragging right must mirror it exactly: {after_left} -> {after_right}"
        );
    }

    /// The reported bug: dragging the seam left did nothing, because the
    /// composer was already pinned against a flat 960px ceiling. On a wide
    /// window that ceiling is reached long before the pointer runs out of
    /// room, so the divider felt welded in one direction.
    #[gpui::test]
    fn a_wide_window_lets_the_composer_grow_past_the_old_flat_ceiling(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        // A large display, where the flat ceiling used to bite.
        cx.simulate_resize(gpui::size(gpui::px(2832.), gpui::px(1500.)));
        let app = window.root(&mut cx).unwrap();
        let inbox = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();
        cx.update(|window, cx| inbox.dispatch_action(&OpenSelected, window, cx));
        cx.run_until_parked();
        let workspace = app.read_with(&cx, |app, _| app.workspace_focus.clone());
        cx.update(|window, cx| workspace.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();

        let seam_x = |cx: &mut VisualTestContext| {
            f32::from(cx.debug_bounds("compose-pane-shell").unwrap().origin.x)
        };
        let y = gpui::px(600.);

        let grab = gpui::Point::new(gpui::px(seam_x(&mut cx) + 6.), y);
        cx.simulate_mouse_down(grab, gpui::MouseButton::Left, gpui::Modifiers::default());
        // Drag a long way left, well past the old 960px ceiling.
        let far = gpui::Point::new(gpui::px(600.), y);
        cx.simulate_mouse_move(
            far,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
        let grown = app.read_with(&cx, |app, _| app.compose_pane_width);
        let seam_after = seam_x(&mut cx);
        cx.simulate_mouse_up(far, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();

        assert!(
            grown > 960.,
            "a wide window must let the composer pass the old flat ceiling, got {grown}"
        );
        // It must stop at the window-derived bound, not run away with the window.
        let expected = compose_pane_max(2832., app.read_with(&cx, |app, _| app.sidebar_width));
        assert!(
            grown <= expected + 0.5,
            "the composer must respect the window-derived maximum: {grown} > {expected}"
        );
        // And the mail pane must survive beside it.
        let reading = seam_after - app.read_with(&cx, |app, _| app.sidebar_width);
        assert!(
            reading >= MIN_READING_PANE_WIDTH,
            "the reading pane must keep a usable width, got {reading}"
        );
    }

    /// The bound has to shrink on a narrow window too, or a small screen would
    /// let the composer crowd the mail out entirely.
    #[gpui::test]
    fn a_narrow_window_shrinks_the_composers_ceiling(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(700.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();
        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();

        cx.update(|window, cx| {
            app.update(cx, |this, cx| {
                this.set_compose_pane_width(10_000., window, cx)
            });
        });
        let wide_max = compose_pane_max(2832., app.read_with(&cx, |app, _| app.sidebar_width));
        let narrow_max = compose_pane_max(900., app.read_with(&cx, |app, _| app.sidebar_width));
        assert!(
            narrow_max < wide_max,
            "a narrow window must cap the composer lower: {narrow_max} vs {wide_max}"
        );
        assert_eq!(
            app.read_with(&cx, |app, _| app.compose_pane_width),
            narrow_max
        );
        // Never below the minimum, or the fields become unusable.
        assert!(
            narrow_max >= COMPOSE_PANE_MIN,
            "the ceiling must never fall under the minimum: {narrow_max}"
        );
    }

    /// The keyboard nudges run through the same clamp as the drag but never
    /// touch a mouse event, so they isolate the arithmetic. `nudge` is what
    /// the divider's Left/Right keys call.
    #[gpui::test]
    fn the_divider_nudges_both_ways_by_the_same_distance(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();
        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();

        let start = app.read_with(&cx, |app, _| app.compose_pane_width);
        let nudge = |cx: &mut VisualTestContext, step: f32| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.nudge_compose_pane(step, window, cx));
            });
            cx.run_until_parked();
            app.read_with(cx, |app, _| app.compose_pane_width)
        };

        // The divider's Left key nudges by +12, Right by -12.
        let widened = nudge(&mut cx, 12.);
        let narrowed = nudge(&mut cx, -12.);

        assert!(
            widened > start,
            "a positive nudge must widen the pane: {start} -> {widened}"
        );
        assert!(
            (widened - narrowed - 12.).abs() <= 0.5,
            "the reverse nudge must undo the first exactly: {widened} vs {narrowed}"
        );
        assert!(
            (narrowed - start).abs() <= 0.5,
            "a left/right pair must be a no-op, not a drift: {start} -> {narrowed}"
        );

        // And the clamps are reachable in both directions, so neither end is
        // a wall that only opens one way.
        let floored = nudge(&mut cx, -10_000.);
        assert_eq!(floored, COMPOSE_PANE_MIN);
        let ceiled = nudge(&mut cx, 20_000.);
        assert_eq!(
            ceiled,
            compose_pane_max(1400., app.read_with(&cx, |app, _| app.sidebar_width)),
            "the ceiling must be the window-derived maximum, not a flat constant"
        );
        assert!(
            ceiled > floored,
            "the ceiling must be above the floor, or one direction is dead: {floored} vs {ceiled}"
        );
    }

    /// A drag that crosses from the compose pane onto the scrolling reading
    /// view used to die there, so only one direction worked. While a drag is
    /// live a full-window overlay must exist, and it must keep receiving
    /// moves even when the pointer is far outside the compose pane.
    #[gpui::test]
    fn a_live_drag_is_carried_by_a_full_window_overlay(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1800.), gpui::px(900.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();
        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();

        assert!(
            cx.debug_bounds("divider-drag-overlay").is_none(),
            "no overlay before a drag starts"
        );

        let seam = cx.debug_bounds("compose-pane-shell").unwrap().origin.x;
        let y = gpui::px(300.);
        let grab = gpui::Point::new(seam + gpui::px(6.), y);
        cx.simulate_mouse_down(grab, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();

        let overlay = cx
            .debug_bounds("divider-drag-overlay")
            .expect("a live drag must mount the overlay");
        assert!(
            overlay.origin.x <= gpui::px(0.) && overlay.size.width >= gpui::px(1800.),
            "the overlay must cover the window, not just the pane: {overlay:?}"
        );

        // Now drag well past the reading pane's own content, the way a hand
        // travelling left would, and check the pane grew by the same amount.
        let start = app.read_with(&cx, |app, _| app.compose_pane_width);
        let far = gpui::Point::new(grab.x - gpui::px(300.), y);
        cx.simulate_mouse_move(
            far,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
        let grown = app.read_with(&cx, |app, _| app.compose_pane_width);
        cx.simulate_mouse_up(far, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();

        assert!(
            (grown - start - 300.).abs() <= 1.,
            "dragging 300px left over the reading view must widen the pane by 300: {start} -> {grown}"
        );
        assert!(
            cx.debug_bounds("divider-drag-overlay").is_none(),
            "releasing must tear the overlay down"
        );
    }

    /// The reported failure: with an email open *and* compose showing, the
    /// divider would not budge. The reading pane fills the space the handle
    /// used to straddle, so the grab strip has to live inside the compose
    /// pane and be wide enough to hit by hand.
    #[gpui::test]
    fn the_compose_divider_drags_while_reading_an_email(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let inbox_focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        // Read a mail, then open compose beside it.
        cx.update(|window, cx| inbox_focus.dispatch_action(&OpenSelected, window, cx));
        cx.run_until_parked();
        let workspace_focus = app.read_with(&cx, |app, _| app.workspace_focus.clone());
        cx.update(|window, cx| workspace_focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();
        assert!(
            matches!(
                app.read_with(&cx, |app, _| app.store.workspace_view()),
                WorkspaceView::Email(_)
            ),
            "this test is only meaningful while an email is open"
        );

        let shell = cx
            .debug_bounds("compose-pane-shell")
            .expect("compose renders beside the reading view");
        let handle = cx
            .debug_bounds("compose-pane-resize")
            .expect("the divider renders");
        assert!(
            handle.origin.x < shell.origin.x,
            "the grab strip must reach across the seam onto the mail side: handle {:?} shell {:?}",
            handle,
            shell
        );
        assert!(
            handle.size.width >= gpui::px(20.),
            "an invisible grab strip narrower than 20px is a coin flip: {:?}",
            handle.size.width
        );

        let pane_width =
            |cx: &mut VisualTestContext| cx.debug_bounds("compose-pane-shell").unwrap().size.width;
        let start = pane_width(&mut cx);

        // Grab just onto the *mail* side of the seam, the way a hand aiming
        // at the visible edge does, and drag left to widen the pane. This is
        // the gesture that used to be a clean miss.
        let grab = gpui::Point::new(
            shell.origin.x - gpui::px(4.),
            shell.origin.y + gpui::px(300.),
        );
        cx.simulate_mouse_down(grab, gpui::MouseButton::Left, gpui::Modifiers::default());
        let moved = gpui::Point::new(grab.x - gpui::px(80.), grab.y);
        cx.simulate_mouse_move(
            moved,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
        let widened = pane_width(&mut cx);
        cx.simulate_mouse_up(moved, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();

        assert!(
            widened > start,
            "dragging the divider left while reading should widen the pane, {start:?} -> {widened:?}"
        );

        // The width state growing is not enough: the seam itself must travel
        // left by the same distance, and the pane's right edge must stay
        // inside the window. Without that, the composer grows past the
        // window edge instead — the Send button slides off-screen right
        // while the mail view keeps every pixel.
        let shell_after = cx
            .debug_bounds("compose-pane-shell")
            .expect("compose still renders after the drag");
        let seam_travelled = f32::from(shell.origin.x) - f32::from(shell_after.origin.x);
        assert!(
            (seam_travelled - 80.).abs() <= 2.,
            "the seam must follow the pointer 1:1, travelled {seam_travelled} for an 80px drag"
        );
        let right_edge = f32::from(shell_after.origin.x) + f32::from(shell_after.size.width);
        assert!(
            right_edge <= 1400. + 1.,
            "the composer must not run past the window edge, right edge at {right_edge}"
        );
    }

    #[gpui::test]
    fn the_compose_divider_resizes_the_pane(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();
        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();

        let pane_width = |cx: &mut VisualTestContext| {
            cx.debug_bounds("compose-pane-shell")
                .expect("the compose pane should be rendered")
                .size
                .width
        };
        let start = pane_width(&mut cx);
        let handle = cx
            .debug_bounds("compose-pane-resize")
            .expect("the divider should be rendered");

        // Press on the divider, then move left, which should widen the pane.
        cx.simulate_mouse_down(
            handle.center(),
            gpui::MouseButton::Left,
            gpui::Modifiers::default(),
        );
        let moved = gpui::Point::new(handle.center().x - gpui::px(80.), handle.center().y);
        cx.simulate_mouse_move(
            moved,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.simulate_mouse_up(moved, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        let widened = pane_width(&mut cx);
        assert!(
            widened > start,
            "dragging the divider left should widen the pane, {start:?} -> {widened:?}"
        );

        // And the clamps hold at the ends, with the upper one derived from
        // the window rather than a flat constant.
        cx.update(|window, cx| {
            app.update(cx, |this, cx| {
                this.set_compose_pane_width(10_000., window, cx)
            });
        });
        assert_eq!(
            app.read_with(&cx, |app, _| app.compose_pane_width),
            compose_pane_max(1400., app.read_with(&cx, |app, _| app.sidebar_width))
        );
        cx.update(|window, cx| {
            app.update(cx, |this, cx| this.set_compose_pane_width(0., window, cx));
        });
        assert_eq!(
            app.read_with(&cx, |app, _| app.compose_pane_width),
            COMPOSE_PANE_MIN
        );
    }

    #[gpui::test]
    fn the_row_menu_assigns_labels_and_the_list_shows_chips(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        // Mail 1 is seeded with the "Work" label. The row must draw it as a
        // chip immediately left of the subject.
        let work = app
            .read_with(&cx, |app, _| {
                app.labels
                    .labels()
                    .iter()
                    .find(|label| label.name == "Work")
                    .map(|label| label.id)
            })
            .expect("the Work label should be seeded");
        assert_eq!(
            app.read_with(&cx, |app, _| app
                .labels
                .labels_for(&EmailId::from(1))
                .to_vec()),
            vec![work]
        );
        assert!(
            cx.debug_bounds(format!("row-label-1-{work}").leak())
                .is_some(),
            "an assigned label must show left of the row's subject"
        );

        // Open the menu from the row's overflow button.
        let button = cx
            .debug_bounds("row-menu-1")
            .expect("every row should carry an overflow button");
        let mods = gpui::Modifiers::default();
        cx.simulate_mouse_down(button.center(), gpui::MouseButton::Left, mods);
        cx.simulate_mouse_up(button.center(), gpui::MouseButton::Left, mods);
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("label-menu").is_some(),
            "the overflow button should open the label menu"
        );

        // Toggling a label from the menu assigns it, and leaves the menu open
        // so a second label is one click away.
        let personal = app
            .read_with(&cx, |app, _| {
                app.labels
                    .labels()
                    .iter()
                    .find(|label| label.name == "Personal")
                    .map(|label| label.id)
            })
            .expect("the Personal label should be seeded");
        assert!(!app.read_with(&cx, |app, _| {
            app.labels.labels_for(&EmailId::from(1)).contains(&personal)
        }));
        let item = cx
            .debug_bounds(format!("label-menu-item-{personal}").leak())
            .expect("the menu should list the Personal label");
        cx.simulate_click(item.center(), mods);
        cx.run_until_parked();
        assert!(
            app.read_with(&cx, |app, _| app
                .labels
                .labels_for(&EmailId::from(1))
                .contains(&personal)),
            "picking a label in the menu should assign it"
        );
        assert!(
            cx.debug_bounds("label-menu").is_some(),
            "the menu stays open so a second label is one click away"
        );

        // Escape closes it.
        cx.update(|window, cx| focus.dispatch_action(&Dismiss, window, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("label-menu").is_none());

        // The newly assigned label now rides on the row beside the first one.
        assert!(
            cx.debug_bounds(format!("row-label-1-{personal}").leak())
                .is_some(),
            "assigning a label from the menu must add its chip to the row"
        );
    }

    /// The three-line layout carries the same chips on the subject line, so
    /// switching density never hides what a mail is tagged with.
    #[gpui::test]
    fn comfortable_rows_show_label_chips_left_of_the_subject(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.set_setting(Setting::CompactRows, false, cx);
            });
        });
        cx.run_until_parked();

        // Mail 1 is seeded with the Work label.
        let work = app
            .read_with(&cx, |app, _| {
                app.labels
                    .labels()
                    .iter()
                    .find(|label| label.name == "Work")
                    .map(|label| label.id)
            })
            .expect("the Work label should be seeded");
        let chip = cx
            .debug_bounds(format!("row-label-1-{work}").leak())
            .expect("the comfortable row must draw the assigned chip");
        let row = cx
            .debug_bounds("email-row-1")
            .expect("the row should be rendered");
        assert!(
            chip.origin.x > row.origin.x,
            "the chip must sit inside the row, not on its edge: {chip:?} vs {row:?}"
        );
        assert!(
            chip.size.width > gpui::px(0.),
            "the chip must have a real width: {chip:?}"
        );
    }

    /// A folder that is already in the index must not be fetched again.
    ///
    /// `loaded_mailboxes` lives in memory, so after a restart every folder looks
    /// unvisited and costs a full fetch. Having rows on disk is the difference
    /// between opening Drafts instantly and waiting out a page of Gmail.
    #[gpui::test]
    fn a_folder_already_in_the_index_is_not_refetched(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        // A cold start: nothing fetched, no account, so any fetch would be a no-op
        // we can still observe via the loaded set.
        assert!(!app.read_with(&cx, |app, _| {
            app.loaded_mailboxes.contains(&Mailbox::Drafts)
        }));

        // Launch state: the index handed us five drafts.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                let drafts: Vec<crate::model::Email> = (0..5)
                    .map(|n| crate::model::Email {
                        id: EmailId(format!("d{n}").leak().to_string()),
                        sender: "Me".to_string(),
                        subject: format!("Draft {n}"),
                        mailbox: Mailbox::Drafts,
                        ..Default::default()
                    })
                    .collect();
                this.store.restore(drafts, None);
                cx.notify();
            });
        });
        cx.run_until_parked();

        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.ensure_mailbox_fetched(Mailbox::Drafts, cx)
            });
        });
        cx.run_until_parked();

        assert!(
            app.read_with(&cx, |app, _| app
                .loaded_mailboxes
                .contains(&Mailbox::Drafts)),
            "a folder with mail on disk counts as loaded"
        );
        assert!(
            !app.read_with(&cx, |app, _| app.syncing),
            "no fetch should have been started for a folder already held"
        );
    }

    /// An empty folder still fetches: nothing on disk means nothing to draw.
    #[gpui::test]
    fn an_empty_folder_still_fetches(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        // Sample mail ships drafts, so start from a genuinely empty store.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.store.restore(Vec::new(), None);
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert_eq!(
            app.read_with(&cx, |app, _| app.store.count(Mailbox::Drafts)),
            0,
            "the store must be empty for this test to mean anything"
        );

        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.ensure_mailbox_fetched(Mailbox::Drafts, cx)
            });
        });
        cx.run_until_parked();

        assert!(
            !app.read_with(&cx, |app, _| app
                .loaded_mailboxes
                .contains(&Mailbox::Drafts)),
            "a folder with nothing cached must still be fetched"
        );
    }

    /// The composer's own close button must work after a settings visit.
    ///
    /// Settings and the overlays used to share one bag of subscriptions, and
    /// closing either cleared the lot. A composer that had been open across a
    /// settings visit therefore had no listener left: the pane was still on
    /// screen, its text intact, and its close button did nothing at all.
    #[gpui::test]
    fn the_compose_close_button_still_works_after_a_settings_visit(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("compose-pane-shell").is_some());

        // Into settings and back out again.
        cx.update(|window, cx| app.update(cx, |this, cx| this.open_settings(window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| app.update(cx, |this, cx| this.go_back(window, cx)));
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("compose-pane-shell").is_some(),
            "the composer came back from settings"
        );

        // Now the close button in its header.
        let close = cx
            .debug_bounds("compose-close")
            .expect("the composer's close button is rendered");
        cx.simulate_click(close.center(), gpui::Modifiers::default());
        cx.run_until_parked();

        assert!(
            !app.read_with(&cx, |app, _| app.compose.is_some()),
            "clicking the composer's own x must close it"
        );
        assert!(
            cx.debug_bounds("compose-pane-shell").is_none(),
            "and the pane must be gone from the layout"
        );
    }

    /// Connecting an account must not leave the prototype's sample mail in the
    /// list beside the user's own.
    ///
    /// This drives `install_account` itself, not a helper it happens to call,
    /// because the bug lived at the call site: the sign-in path cleared the
    /// sample labels only when the store happened to be empty, and never
    /// cleared the sample mail, so `absorb` merged the real account into it.
    #[gpui::test]
    fn connecting_an_account_clears_the_sample_mail_and_labels(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        // A cold start is all sample data: the prototype's mail and its labels.
        let sample_mail = app.read_with(&cx, |app, _| app.store.snapshot().len());
        assert!(
            sample_mail > 1,
            "the app must start holding sample mail for this test to mean anything, got {sample_mail}"
        );
        assert!(
            !app.read_with(&cx, |app, _| app.labels.labels().is_empty()),
            "and the sample labels seeded beside it"
        );

        // What a completed sign-in hands over: one real mail, no labels.
        fn remote_mail(id: &str) -> nori_gmail::RemoteMail {
            nori_gmail::RemoteMail {
                id: id.to_string(),
                sender: "A Real Sender".to_string(),
                address: "real@example.com".to_string(),
                recipients: vec!["me@example.com".to_string()],
                subject: "A real subject".to_string(),
                preview: "Real mail".to_string(),
                body: None,
                timestamp: "Sep 26".to_string(),
                full_date: "Sep 26".to_string(),
                label_ids: vec!["INBOX".to_string()],
            }
        }

        let snapshot = nori_gmail::Snapshot {
            account: "me@example.com".to_string(),
            labels: Vec::new(),
            mail: vec![remote_mail("real-1")],
            history_id: Some("cursor-1".to_string()),
        };
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.install_account("me@example.com".to_string(), snapshot, None, cx);
            });
        });
        cx.run_until_parked();

        let held = app.read_with(&cx, |app, _| {
            app.store
                .snapshot()
                .iter()
                .map(|email| email.id.0.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            held,
            vec!["real-1".to_string()],
            "only the account's own mail may remain, sample subjects are gone"
        );
        assert!(
            app.read_with(&cx, |app, _| app.labels.labels().is_empty()),
            "and the sample labels went with it"
        );
        assert_eq!(
            app.read_with(&cx, |app, _| app
                .store
                .synced_history_id()
                .map(str::to_string)),
            Some("cursor-1".to_string()),
            "the cursor carries over, so the next launch is not another full sync"
        );
        assert!(
            app.read_with(&cx, |app, _| app.account.is_usable()),
            "and the app reports the account as connected"
        );
    }

    /// An empty list during a sign-in must say which of the two long waits the
    /// user is in, and must not offer a button they have already pressed.
    ///
    /// The two phases are told apart deliberately. "Waiting for the browser" is
    /// true for a few seconds and a lie for the minute that follows, and a
    /// sign-in button still on screen during it invites a second press.
    #[gpui::test]
    fn the_empty_list_says_what_the_sign_in_is_doing(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        // Empty, so the placeholder is the thing on screen.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.store.clear();
                this.account = AccountState::Disconnected;
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("empty-sign-in").is_some(),
            "with no account, the way in is offered"
        );
        assert!(
            cx.debug_bounds("empty-state-not-signed-in").is_some(),
            "and the list says it has no account rather than no mail"
        );

        // Phase one: the browser round trip.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.account = AccountState::Connecting;
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("empty-state-signing-in").is_some(),
            "the list must name the browser round trip it is in"
        );
        assert!(
            cx.debug_bounds("empty-sign-in").is_none(),
            "the sign-in button must not still be offered while signing in"
        );

        // Phase two: the mailbox read, which is the long one.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.account = AccountState::Fetching {
                    address: "me@example.com".to_string(),
                };
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("empty-state-fetching").is_some(),
            "and the long wait must be named as fetching, not signing in"
        );
        assert!(
            cx.debug_bounds("empty-state-signing-in").is_none(),
            "the wording must move on once the grant has landed"
        );
        assert!(
            cx.debug_bounds("empty-sign-in").is_none(),
            "nor after the grant lands, while mail is still arriving"
        );
    }

    /// A test must not be able to reach the real account pointer.
    ///
    /// Signing out deletes that file, and the disconnect test presses the real
    /// Disconnect button — so with the pointer left pointing at the config
    /// directory, running the suite deleted the account the person running it
    /// was signed in to. It happened twice before this was noticed.
    ///
    /// The assertion is on where the pointer resolves, not on what sign-out
    /// does, because that is the part that can silently regress: remove the
    /// test-only path and this fails, rather than the suite quietly eating
    /// someone's account again.
    #[gpui::test]
    fn a_test_never_resolves_the_real_account_pointer(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        let resolved = app.read_with(&cx, |app, _| {
            app.account_pointer()
                .map(|pointer| pointer.path().to_path_buf())
        });
        let resolved = resolved.expect("the pointer always resolves to something");
        let real = std::env::var_os("HOME").map(std::path::PathBuf::from);
        if let Some(home) = real {
            assert!(
                !resolved.starts_with(home.join(".config")),
                "a test resolved the pointer to {} — inside the real config \
                 directory, so signing out in a test would delete it",
                resolved.display()
            );
        }
    }

    /// The account page must follow the app, not a snapshot of it.
    ///
    /// It used to hold the account state as it was when Settings opened, so
    /// Disconnect left the page still offering Disconnect over an account that
    /// had already been forgotten. Driving the real sign-out through the real
    /// button is the only way to catch that: a test on the page alone would
    /// never notice it was reading a copy.
    #[gpui::test]
    fn disconnecting_turns_the_account_page_into_a_sign_in_prompt(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        // Connected, with the settings page open on Account.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.account = AccountState::Connected {
                    address: "me@example.com".to_string(),
                    mail: 335,
                    labels: 2,
                    counts: None,
                };
                cx.notify();
            });
        });
        cx.update(|window, cx| focus.dispatch_action(&OpenSettings, window, cx));
        cx.run_until_parked();
        // Settings opens on General, so walk to Account the way a person would.
        let account_nav = cx
            .debug_bounds(SettingsPage::Account.nav_id())
            .expect("the Account page is in the nav");
        cx.simulate_click(account_nav.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        assert_eq!(
            app.read_with(&cx, |app, _| app.settings_page),
            SettingsPage::Account,
            "and the account page is what carries this"
        );
        assert!(
            cx.debug_bounds("account-sign-out").is_some(),
            "a connected account offers Disconnect"
        );

        // Press it, the way a person would.
        let out = cx
            .debug_bounds("account-sign-out")
            .expect("the Disconnect button");
        cx.simulate_click(out.center(), gpui::Modifiers::default());
        cx.run_until_parked();

        assert!(
            !app.read_with(&cx, |app, _| app.account.is_usable()),
            "the app must have forgotten the account"
        );
        assert!(
            cx.debug_bounds("account-sign-out").is_none(),
            "and the page must stop offering Disconnect, not keep it on screen"
        );
        assert!(
            cx.debug_bounds("account-sign-in").is_some(),
            "offering the way back in instead"
        );
    }

    /// The account page repaints when the account changes on its own.
    ///
    /// The click test next door passes with or without the repaint, because
    /// clicking a button redraws that button's row anyway — which is exactly
    /// why it cannot be trusted to cover this. Nothing here is clicked: the
    /// account is moved underneath the open page, the way a background sync
    /// landing an expired grant does. Before, the page sat on "Disconnect"
    /// over an account that needed signing in again.
    #[gpui::test]
    fn the_account_page_follows_a_change_nobody_clicked(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.set_account(
                    AccountState::Connected {
                        address: "me@example.com".to_string(),
                        mail: 335,
                        labels: 2,
                        counts: None,
                    },
                    cx,
                );
            });
        });
        cx.update(|window, cx| focus.dispatch_action(&OpenSettings, window, cx));
        cx.run_until_parked();
        let nav = cx
            .debug_bounds(SettingsPage::Account.nav_id())
            .expect("the Account page is in the nav");
        cx.simulate_click(nav.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("account-sign-out").is_some(),
            "premise: a live account offers Disconnect"
        );

        // The grant expired mid-sync. No click, no focus change, no settings
        // navigation: just the state moving underneath the open page.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.set_account(
                    AccountState::NeedsReauth {
                        address: "me@example.com".to_string(),
                    },
                    cx,
                );
            });
        });
        cx.run_until_parked();

        assert!(
            cx.debug_bounds("account-sign-out").is_none(),
            "an account that needs signing in again must not still offer Disconnect"
        );
        assert!(
            cx.debug_bounds("account-sign-in").is_some(),
            "and must offer the way back in"
        );
    }

    /// Flipping a switch writes it out, which is the only reason persistence is
    /// worth having.
    ///
    /// Driven through `write_settings` with a store of the test's own: the
    /// failure being ruled out is a switch that flips on screen and is quietly
    /// not saved, and the last version of this test proved nothing except that
    /// it could overwrite the user's real settings.
    #[gpui::test]
    fn flipping_a_switch_writes_the_settings_file(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        let dir = std::env::temp_dir().join(format!("nori-settings-app-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let store = SettingsStore::new(dir.join("settings.json"));

        // Point the app at a store of this test's own first: `set_setting`
        // saves on every change, so without this the test writes over the
        // settings of whoever is running it.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.settings_store = SettingsStore::new(dir.join("settings.json"));
                this.set_setting(Setting::LightMode, true, cx);
                this.set_setting(Setting::CompactRows, false, cx);
            });
        });
        cx.run_until_parked();

        assert!(
            app.read_with(&cx, |app, _| app.settings_state.get(Setting::LightMode)),
            "the switch reads on"
        );
        let reloaded = store.load().expect("the file was written");
        assert!(
            reloaded.get(Setting::LightMode),
            "and it was written out, not just held in memory"
        );
        assert!(!reloaded.get(Setting::CompactRows));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A successful sign-in must record which account it connected.
    ///
    /// The token file is named after the address, so on its own it says nothing
    /// about which account Nori should load. The pointer is the only record of
    /// that, and it is what the next launch reads. Skipping the write made a
    /// working sign-in look like it had never happened: the app came back
    /// reporting no account, with a valid token on disk.
    #[gpui::test]
    fn a_signed_in_account_is_remembered_for_the_next_launch(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        // An explicit path rather than the config dir: the pointer is what is
        // under test, and writing to the real one would be rude.
        let dir = std::env::temp_dir().join(format!("nori-pointer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let pointer = nori_gmail::LastAccount::new(dir.join("last-account"));

        // What the sign-in completion does once Gmail has handed back a token.
        cx.update(|_, cx| {
            app.update(cx, |this, _| {
                this.remember_account(&pointer, "me@example.com");
            });
        });

        assert_eq!(
            pointer.load().as_deref(),
            Some("me@example.com"),
            "the address must be readable on the next launch"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Settings is a detour, not a dismissal.
    ///
    /// The composer is hidden while settings is open, but it is not destroyed:
    /// leaving by a mailbox or by the back chevron must return the user to the
    /// draft they left, exactly as typed.
    #[gpui::test]
    fn settings_hides_the_composer_without_destroying_it(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();
        let compose = app
            .read_with(&cx, |app, _| app.compose.clone())
            .expect("compose is open");
        let typed = "draft that must survive settings";
        cx.update(|_, cx| {
            compose.update(cx, |this, cx| this.set_body_text(typed, cx));
        });
        cx.run_until_parked();

        // Into settings.
        cx.update(|window, cx| app.update(cx, |this, cx| this.open_settings(window, cx)));
        cx.run_until_parked();
        assert!(app.read_with(&cx, |app, _| app.settings.is_some()));
        assert!(
            cx.debug_bounds("compose-pane-shell").is_none(),
            "settings takes the workspace, so the composer is hidden"
        );
        assert!(
            app.read_with(&cx, |app, _| app.compose.is_some()),
            "but it must still be alive behind settings"
        );

        // Back out with the chevron.
        cx.update(|window, cx| app.update(cx, |this, cx| this.go_back(window, cx)));
        cx.run_until_parked();
        assert!(
            !app.read_with(&cx, |app, _| app.settings.is_some()),
            "the back chevron must close settings"
        );
        assert!(
            cx.debug_bounds("compose-pane-shell").is_some(),
            "and the composer must come back"
        );
        let kept = app.read_with(&cx, |app, _| {
            app.compose
                .as_ref()
                .map(|compose| compose.read_with(&cx, |view, cx| view.body_text(cx)))
        });
        assert_eq!(
            kept.as_deref().map(str::to_string),
            Some(typed.to_string()),
            "with the text exactly as it was"
        );
    }

    /// The same round trip, leaving by a mailbox instead of the chevron.
    #[gpui::test]
    fn leaving_settings_by_mailbox_restores_the_draft(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();
        let compose = app
            .read_with(&cx, |app, _| app.compose.clone())
            .expect("compose is open");
        cx.update(|_, cx| {
            compose.update(cx, |this, cx| this.set_body_text("kept", cx));
        });
        cx.update(|window, cx| app.update(cx, |this, cx| this.open_settings(window, cx)));
        cx.run_until_parked();

        cx.update(|window, cx| {
            app.update(cx, |this, cx| {
                this.select_mailbox(Mailbox::Sent, window, cx)
            })
        });
        cx.run_until_parked();

        assert_eq!(
            app.read_with(&cx, |app, _| app.store.selected_mailbox()),
            Mailbox::Sent
        );
        assert!(cx.debug_bounds("compose-pane-shell").is_some());
        let kept = app.read_with(&cx, |app, _| {
            app.compose
                .as_ref()
                .map(|compose| compose.read_with(&cx, |view, cx| view.body_text(cx)))
        });
        assert_eq!(
            kept.as_deref().map(str::to_string),
            Some("kept".to_string())
        );
    }

    /// Switching folders must not cost the user their draft.
    ///
    /// The composer is a second column, not a modal: glancing at another
    /// mailbox mid-sentence is an ordinary thing to do, and it used to destroy
    /// the whole `ComposeView` — every field the user had filled in went with
    /// it.
    #[gpui::test]
    fn switching_mailboxes_keeps_the_draft(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        // Compose, and type into it.
        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        cx.run_until_parked();
        let compose = app
            .read_with(&cx, |app, _| app.compose.clone())
            .expect("compose should be open");
        let typed = "half a sentence about the invoice";
        cx.update(|_, cx| {
            compose.update(cx, |this, cx| {
                this.set_body_text(typed, cx);
            });
        });
        cx.run_until_parked();

        // Switch to another mailbox.
        cx.update(|window, cx| {
            app.update(cx, |this, cx| {
                this.select_mailbox(Mailbox::Drafts, window, cx)
            })
        });
        cx.run_until_parked();

        assert!(
            app.read_with(&cx, |app, _| app.compose.is_some()),
            "the composer must survive a folder switch"
        );
        assert_eq!(
            app.read_with(&cx, |app, _| app.store.selected_mailbox()),
            Mailbox::Drafts,
            "and the folder must actually have changed"
        );
        let kept = app.read_with(&cx, |app, _| {
            app.compose
                .as_ref()
                .map(|compose| compose.read_with(&cx, |view, cx| view.body_text(cx)))
        });
        assert_eq!(
            kept.as_deref().map(str::to_string),
            Some(typed.to_string()),
            "the draft text must be exactly what was typed"
        );
        assert!(
            cx.debug_bounds("compose-pane-shell").is_some(),
            "the composer must still be on screen beside the new folder"
        );
    }

    /// A body that has been read must reach the index file, or the next launch
    /// fetches it again. The store had it; the cache is the part that was
    /// being dropped.
    #[gpui::test]
    fn a_fetched_body_is_written_to_the_index(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        let dir = std::env::temp_dir().join(format!("nori-index-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("account.index.json");
        let cache = IndexCache::new(&path);

        let blocks = nori_gmail::text_blocks(vec!["A body that was fetched.".to_string()]);

        // What the fetch path does: attach the body, then persist.
        cx.update(|_, cx| {
            app.update(cx, |this, _| {
                this.store.restore(
                    vec![crate::model::Email {
                        id: EmailId("cached-1".to_string()),
                        sender: "Sender".to_string(),
                        subject: "Subject".to_string(),
                        mailbox: Mailbox::Inbox,
                        ..Default::default()
                    }],
                    Some("cursor".to_string()),
                );
                this.store
                    .set_body(&EmailId::from("cached-1"), blocks.clone());
                let _ = cache.save(&crate::model::Index {
                    account: "me@example.com".to_string(),
                    emails: this.store.snapshot(),
                    labels: Vec::new(),
                    assignments: Vec::new(),
                    history_id: this.store.synced_history_id().map(str::to_string),
                });
            });
        });

        let written = std::fs::read_to_string(&path).expect("index written");
        assert!(
            written.contains("A body that was fetched."),
            "the fetched body must be in the file, not just in memory: {written}"
        );

        // And it must read back as a loaded body, so no fetch is needed.
        let reloaded = cache
            .load("me@example.com")
            .expect("index loads for this account");
        let mail = &reloaded.emails[0];
        assert!(mail.body_loaded, "a cached body counts as loaded");
        assert_eq!(mail.body, blocks);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pinning has to reach the file, not just the store.
    ///
    /// `pinned` was already a serialised field, so the data reached disk
    /// eventually — but only as a side effect of some later sync or body fetch
    /// happening to run. Pin a mail and quit, and the file still said unpinned.
    ///
    /// Nothing here calls the write itself: the only thing driven is
    /// `toggle_pin`, so if pinning stops saving, this fails. An earlier version
    /// of this test called `write_index` by hand and passed with the save
    /// deleted, having proved only that a write writes.
    #[gpui::test]
    fn pinning_writes_the_pin_to_the_index(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        let dir = std::env::temp_dir().join(format!("nori-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let cache = IndexCache::new(dir.join("account.index.json"));
        let account = "me@example.com";

        // Real mail with a real origin, so `toggle_pin` takes the save path
        // rather than stopping at the sample-mail early return, and its writes
        // land in this test's file rather than the account's real index.
        let pinned = EmailId("pin-1".to_string());
        cx.update(|_, cx| {
            app.update(cx, |this, _| {
                this.index_cache_override = Some(IndexCache::new(dir.join("account.index.json")));
                this.store.restore(
                    vec![crate::model::Email {
                        id: pinned.clone(),
                        sender: "Sender".to_string(),
                        subject: "Worth keeping".to_string(),
                        mailbox: Mailbox::Inbox,
                        origin: crate::model::Origin::Remote {
                            account: account.to_string(),
                        },
                        ..Default::default()
                    }],
                    None,
                );
            });
        });

        // The pin, and nothing else.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| this.toggle_pin(pinned.clone(), cx));
        });
        cx.run_until_parked();

        let written = cache
            .load(account)
            .expect("pinning writes the index on its own");
        assert!(
            written.emails[0].pinned,
            "the pin must be in the file: quitting right after a pin must not lose it"
        );

        // And a launch reading that file gets the tab back.
        let mut reopened = crate::model::MailStore::new(Vec::new());
        reopened.restore(written.emails, written.history_id);
        assert_eq!(
            reopened.tabs(),
            std::slice::from_ref(&pinned),
            "a pinned mail has its tab again on the next launch"
        );

        // Unpinning is as durable as pinning.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| this.toggle_pin(pinned.clone(), cx));
        });
        cx.run_until_parked();
        let cleared = cache.load(account).expect("unpinning writes the index");
        assert!(
            !cleared.emails[0].pinned,
            "an unpin has to be written too, or quitting resurrects the tab"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The list must draw rows both when idle and while a sync is in flight.
    ///
    /// The second case is the one that matters: the fetch counter used to
    /// arrive wrapped around the list, and that wrapper collapsed the list to
    /// zero height, so the inbox went blank for the whole length of every
    /// sync — precisely when it was being waited on.
    #[gpui::test]
    fn rows_draw_while_a_sync_is_in_flight(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        let load = |cx: &mut VisualTestContext, syncing: bool| {
            cx.update(|_, cx| {
                app.update(cx, |this, cx| {
                    let mail: Vec<crate::model::Email> = (0..20)
                        .map(|n| crate::model::Email {
                            id: EmailId(format!("s{n}").leak().to_string()),
                            sender: format!("Sender {n}"),
                            subject: format!("Subject {n}"),
                            preview: format!("Preview {n}"),
                            timestamp: "Sep 26".to_string(),
                            mailbox: Mailbox::Inbox,
                            ..Default::default()
                        })
                        .collect();
                    this.store.restore(mail, None);
                    this.syncing = syncing;
                    cx.notify();
                });
            });
            cx.run_until_parked();
        };

        for syncing in [false, true] {
            load(&mut cx, syncing);
            let list = cx.debug_bounds("email-list").expect("the list is rendered");
            assert!(
                list.size.height > gpui::px(400.),
                "the list must keep its height while syncing={syncing}, got {list:?}"
            );
            let row = cx
                .debug_bounds("email-row-s0")
                .unwrap_or_else(|| panic!("a row must draw while syncing={syncing}"));
            assert!(
                row.size.height > gpui::px(0.),
                "the row must have height while syncing={syncing}, got {row:?}"
            );
        }
    }

    #[test]
    fn image_formats_sniff_from_magic_bytes() {
        assert_eq!(
            sniff_image_format(&[0x89, b'P', b'N', b'G', 0]),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            sniff_image_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            sniff_image_format(b"GIF89a\x01\x00"),
            Some(ImageFormat::Gif)
        );
        assert_eq!(
            sniff_image_format(b"RIFF\x00\x00\x00\x00WEBPVP8\x00"),
            Some(ImageFormat::Webp)
        );
        assert_eq!(sniff_image_format(b"<svg"), None, "XML is not a picture");
        assert_eq!(sniff_image_format(&[]), None);
        assert_eq!(sniff_image_format(b"hello"), None);
    }

    #[test]
    fn only_absolute_https_pictures_fetch() {
        assert!(is_fetchable_image_url("https://img.example/a.jpg"));
        assert!(!is_fetchable_image_url("http://img.example/a.jpg"));
        assert!(!is_fetchable_image_url("https://"));
        assert!(!is_fetchable_image_url("/relative/a.jpg"));
        assert!(!is_fetchable_image_url(""));
    }

    #[gpui::test]
    fn the_image_cache_restarts_instead_of_growing_forever(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();
        cx.update(|_, cx| {
            app.update(cx, |this, _| {
                for index in 0..MAX_CACHED_IMAGES + 100 {
                    this.insert_image_slot(
                        format!("https://img.example/{index}.jpg"),
                        ImageSlot::Failed,
                    );
                }
            });
        });
        assert!(
            app.read_with(&cx, |app, _| app.images.len()) <= MAX_CACHED_IMAGES,
            "hundreds of failed pictures must not accumulate without bound"
        );
    }

    #[gpui::test]
    fn opening_a_mail_with_pictures_queues_downloads(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        // Mail 1 gets a rich body with a picture before it opens.
        cx.update(|_, cx| {
            app.update(cx, |this, _| {
                this.store.set_body(
                    &EmailId::from(1),
                    vec![nori_gmail::RichBlock::Image {
                        src: "https://img.example/a.jpg".to_string(),
                        alt: "A".to_string(),
                        link: None,
                    }],
                );
            });
        });
        cx.update(|window, cx| {
            app.update(cx, |this, cx| this.open_email(EmailId::from(1), window, cx));
        });
        cx.run_until_parked();

        // img.example never resolves, so the slot settles failed — but the
        // point is the open queued it at all.
        assert!(
            app.read_with(&cx, |app, _| app
                .images
                .contains_key("https://img.example/a.jpg")),
            "opening a mail with pictures must queue their downloads"
        );
    }

    #[gpui::test]
    fn sidebar_owns_the_back_and_forward_chevrons(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("sidebar-back").is_some(),
            "the sidebar carries the back chevron"
        );
        assert!(
            cx.debug_bounds("sidebar-forward").is_some(),
            "the sidebar carries the forward chevron"
        );

        // Opening a mail makes back live, and going back re-enables forward.
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.update(|window, cx| focus.dispatch_action(&OpenSelected, window, cx));
        assert!(app.read_with(&cx, |app, _| app.store.can_go_back()));
        cx.update(|window, cx| app.update(cx, |app, cx| app.go_back(window, cx)));
        assert!(app.read_with(&cx, |app, _| app.store.can_go_forward()));
        assert_eq!(
            app.read_with(&cx, |app, _| app.store.workspace_view()),
            WorkspaceView::Mailbox
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.go_forward(window, cx)));
        assert!(matches!(
            app.read_with(&cx, |app, _| app.store.workspace_view()),
            WorkspaceView::Email(_)
        ));
    }

    /// Open the app tall enough that the sidebar's scrolled nav lays out the
    /// labels section, which sits below the mailboxes.
    fn open_app_with_labels(cx: &mut TestAppContext) -> (WindowHandle<MailApp>, VisualTestContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
        (window, visual)
    }

    /// Click whatever the debug selector names, by its own centre. The
    /// selector is `&'static str` because that is what gpui's `debug_bounds`
    /// takes, so these helpers only ever name a literal.
    fn click_selector(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} should be on screen"));
        let center = bounds.center();
        cx.simulate_click(
            gpui::Point::new(center.x, center.y),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
    }

    /// Both sidebar groups read as one block: a header, then its rows flush
    /// underneath, then a clear gap before the next group. This pins the
    /// vertical rhythm so a change to one group's padding cannot quietly make
    /// the two look different.
    #[gpui::test]
    fn the_sidebar_groups_are_spaced_like_each_other(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let _ = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        let bottom = |cx: &mut VisualTestContext, selector: &'static str| {
            let bounds = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} should be laid out"));
            bounds.origin.y + bounds.size.height
        };
        let top = |cx: &mut VisualTestContext, selector: &'static str| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} should be laid out"))
                .origin
                .y
        };
        let gap = |cx: &mut VisualTestContext, above: &'static str, below: &'static str| {
            top(cx, below) - bottom(cx, above)
        };

        // Rows sit flush under their own header, in both groups.
        let under_mailboxes = gap(&mut cx, "sidebar-mailboxes-toggle", "sidebar-mailbox-0");
        let under_labels = gap(&mut cx, "sidebar-labels-toggle", "sidebar-labels");
        assert_eq!(
            under_mailboxes, under_labels,
            "a group's rows should hug its own header the same way in both groups"
        );

        // And the two groups are clearly separated from each other.
        let between = gap(&mut cx, "sidebar-mailbox-5", "sidebar-labels-toggle");
        assert!(
            between > under_labels * 2,
            "the Labels header needs clear air above it, got {between} against \
             {under_labels} inside a group"
        );
    }

    #[gpui::test]
    fn the_labels_group_collapses_independently_of_the_mailboxes(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        // Both groups start open, so a seeded label row is on screen.
        assert!(
            app.read_with(&cx, |app, _| !app.labels_collapsed),
            "the labels group starts expanded"
        );
        assert!(!app.read_with(&cx, |app, _| app.mailboxes_collapsed));
        assert!(
            cx.debug_bounds("sidebar-labels").is_some(),
            "the label rows are on screen while the group is open"
        );

        click_selector(&mut cx, "sidebar-labels-toggle");
        assert!(
            app.read_with(&cx, |app, _| app.labels_collapsed),
            "the header should collapse the labels group"
        );
        assert!(
            cx.debug_bounds("sidebar-labels").is_none(),
            "a collapsed group shows its header and nothing else"
        );
        assert!(
            cx.debug_bounds("sidebar-mailbox-0").is_some(),
            "collapsing labels must not touch the mailboxes group"
        );
        assert!(!app.read_with(&cx, |app, _| app.mailboxes_collapsed));

        click_selector(&mut cx, "sidebar-labels-toggle");
        assert!(!app.read_with(&cx, |app, _| app.labels_collapsed));
        assert!(cx.debug_bounds("sidebar-labels").is_some());
    }

    /// The composer's open/close mechanics in one walk: the `+` opens it, the
    /// `+` again closes it, opening on a collapsed group expands that group,
    /// and collapsing the group takes the composer with it.
    #[gpui::test]
    fn the_label_composer_opens_and_closes(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();
        let composer_open =
            |cx: &VisualTestContext| app.read_with(cx, |app, _| app.label_composer_open);
        let labels_collapsed =
            |cx: &VisualTestContext| app.read_with(cx, |app, _| app.labels_collapsed);
        let act = |cx: &mut VisualTestContext, action: LabelsAction| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.labels_action(action, window, cx)));
            cx.run_until_parked();
        };

        assert!(cx.debug_bounds("sidebar-label-new").is_none());
        assert!(!composer_open(&cx));

        // The `+` opens it, and the `+` is also the pointer way back out.
        click_selector(&mut cx, "sidebar-labels-add");
        assert!(composer_open(&cx));
        assert!(cx.debug_bounds("sidebar-label-new").is_some());
        click_selector(&mut cx, "sidebar-labels-add");
        assert!(!composer_open(&cx));

        // Opening it on a collapsed group has to expand that group, or the `+`
        // would appear to do nothing.
        act(&mut cx, LabelsAction::ToggleCollapsed);
        assert!(labels_collapsed(&cx));
        act(&mut cx, LabelsAction::ToggleComposer);
        assert!(composer_open(&cx));
        assert!(
            !labels_collapsed(&cx),
            "the + must not appear to do nothing on a collapsed group"
        );

        // Collapsing takes the composer with it rather than hiding it.
        act(&mut cx, LabelsAction::ToggleCollapsed);
        assert!(
            !composer_open(&cx),
            "collapsing out from under a live field would hide it without \
             dismissing it"
        );
    }

    /// A successful create ends the visit; a refused one must not, or the name
    /// could not be corrected.
    #[gpui::test]
    fn creating_a_label_ends_the_composer_but_a_refused_one_does_not(cx: &mut TestAppContext) {
        let (window, mut cx) = open_app_with_labels(cx);
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();
        let named = |cx: &VisualTestContext, name: &str| {
            app.read_with(cx, |app, _| {
                app.labels.labels().iter().any(|label| label.name == name)
            })
        };

        // Deliberately not a seeded name: creating the label first makes the
        // second attempt a duplicate whatever the app starts with.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.labels_action(LabelsAction::ToggleComposer, window, cx);
                app.create_label("Receipts".to_string(), window, cx);
            })
        });
        cx.run_until_parked();
        assert!(named(&cx, "Receipts"));
        assert!(
            !app.read_with(&cx, |app, _| app.label_composer_open),
            "one label per visit: the composer closes after a successful create"
        );

        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.labels_action(LabelsAction::ToggleComposer, window, cx);
                app.create_label("Receipts".to_string(), window, cx);
            })
        });
        cx.run_until_parked();
        assert!(
            app.read_with(&cx, |app, _| app.label_composer_open),
            "a duplicate has to stay open so the name can be corrected"
        );
    }

    #[gpui::test]
    fn labels_can_be_created_assigned_filtered_and_deleted(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        // Tall enough that the sidebar's scrolled nav actually lays out the
        // labels section, which sits below the mailboxes.
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        // The sidebar renders the seeded labels under a collapsible header,
        // and the composer is closed until the header's `+` opens it.
        assert!(
            cx.debug_bounds("sidebar-labels").is_some(),
            "the sidebar renders a labels section"
        );
        assert!(
            cx.debug_bounds("sidebar-labels-toggle").is_some(),
            "the labels group has a header of its own"
        );
        assert!(
            cx.debug_bounds("sidebar-labels-add").is_some(),
            "the header carries the new-label button"
        );
        assert!(
            cx.debug_bounds("sidebar-label-new").is_none(),
            "the composer must not be on screen before the + is pressed"
        );

        // Create a label through the app, the way the composer commits it.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.create_label("Follow up".to_string(), window, cx)
            })
        });
        let follow_up = app
            .read_with(&cx, |app, _| {
                app.labels
                    .labels()
                    .iter()
                    .find(|l| l.name == "Follow up")
                    .map(|l| l.id)
            })
            .expect("the label should have been created");
        // Creating selects it, so the list is filtered to its (empty) set.
        assert_eq!(
            app.read_with(&cx, |app, _| app.selected_label),
            Some(follow_up)
        );

        // A duplicate name is refused and changes nothing.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.create_label("follow up".to_string(), window, cx)
            })
        });
        assert_eq!(app.read_with(&cx, |app, _| app.labels.labels().len()), 3);

        // Assign it to a mail that already carries a seeded label, so the two
        // compose rather than replacing each other.
        let email_id = EmailId::from(1);
        let work = app
            .read_with(&cx, |app, _| {
                app.labels
                    .labels()
                    .iter()
                    .find(|l| l.name == "Work")
                    .map(|l| l.id)
            })
            .expect("Work is seeded");
        app.update(&mut cx, |app, cx| {
            app.toggle_label(follow_up, email_id.clone(), cx)
        });
        assert_eq!(
            app.read_with(&cx, |app, _| app.labels.labels_for(&email_id).to_vec()),
            vec![work, follow_up],
            "the new label joins the one the mail already had"
        );
        // In the list, the overflow menu is where labels are assigned.
        assert!(
            cx.debug_bounds("row-menu-1").is_some(),
            "the list row carries the label menu"
        );

        // The reading view no longer carries a label row: opening a mail must
        // not put labels back under the body.
        app.update(&mut cx, |app, cx| {
            app.store.open_email(email_id.clone());
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("email-labels").is_none(),
            "labels are assigned from the list, not under the message body"
        );

        // Deleting it clears the assignment and the filter.
        app.update(&mut cx, |app, cx| app.delete_label(follow_up, cx));
        assert_eq!(
            app.read_with(&cx, |app, _| app.labels.labels_for(&email_id).to_vec()),
            vec![work],
            "deleting a label leaves the others the mail carried"
        );
        assert_eq!(app.read_with(&cx, |app, _| app.selected_label), None);
        assert!(app.read_with(&cx, |app, _| app.labels.label(follow_up).is_none()));
    }

    #[gpui::test]
    fn compose_action_opens_overlay(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        assert!(app.read_with(&cx, |app, _| matches!(
            app.store.overlay(),
            Some(Overlay::Compose)
        )));
    }

    #[gpui::test]
    #[gpui::test]
    fn the_light_mode_switch_republishes_the_theme(cx: &mut TestAppContext) {
        // The theme is the one setting that lives outside the app struct, so
        // the contract is the global itself: flipping the switch has to change
        // what every view reads at paint time.
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        let published_canvas = |cx: &VisualTestContext| {
            cx.try_read_global::<Theme, _>(|theme, _| theme.canvas.to_rgb())
                .unwrap_or(Theme::dark().canvas.to_rgb())
        };
        assert_eq!(
            published_canvas(&mut cx),
            Theme::dark().canvas.to_rgb(),
            "a cold start is dark"
        );

        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.set_setting(Setting::LightMode, true, cx);
            });
        });
        cx.run_until_parked();
        assert_eq!(
            published_canvas(&mut cx),
            Theme::light().canvas.to_rgb(),
            "flipping the setting should publish the light palette"
        );

        cx.update(|_, cx| {
            app.update(cx, |this, cx| {
                this.set_setting(Setting::LightMode, false, cx);
            });
        });
        cx.run_until_parked();
        assert_eq!(
            published_canvas(&mut cx),
            Theme::dark().canvas.to_rgb(),
            "flipping it back should publish dark again"
        );
    }

    #[gpui::test]
    fn the_light_mode_switch_is_on_the_appearance_page(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        cx.update(|window, cx| focus.dispatch_action(&OpenSettings, window, cx));
        cx.run_until_parked();

        // Walk to Appearance by clicking the page row, the way a user does.
        let row = cx
            .debug_bounds(SettingsPage::Appearance.nav_id())
            .expect("the page column should list Appearance");
        let center = row.center();
        cx.simulate_click(
            gpui::Point::new(center.x, center.y),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();

        let toggle = cx
            .debug_bounds(Setting::LightMode.element_id())
            .expect("Appearance should offer the light mode switch");
        let center = toggle.center();
        cx.simulate_click(
            gpui::Point::new(center.x, center.y),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();

        assert!(
            app.read_with(&cx, |app, _| app.settings_state.light_mode),
            "clicking the switch should turn light mode on"
        );
        assert_eq!(
            cx.try_read_global::<Theme, _>(|theme, _| theme.canvas.to_rgb()),
            Some(Theme::light().canvas.to_rgb()),
            "and the app should be painting from the light palette"
        );
    }

    #[gpui::test]
    fn settings_shares_the_mail_shell_and_escape_returns(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        cx.update(|window, cx| focus.dispatch_action(&OpenSettings, window, cx));
        cx.run_until_parked();
        // Settings takes focus for its own nav, so the mail app's action
        // handlers are only reachable through the settings view itself.
        let settings = app
            .read_with(&cx, |app, _| app.settings.clone())
            .expect("settings should be open");
        let settings_focus = settings.read_with(&cx, |settings, _| settings.focus_handle());
        assert!(app.read_with(&cx, |app, _| app.settings.is_some()));
        assert!(
            cx.debug_bounds("settings").is_some(),
            "settings should hold the workspace"
        );
        assert!(
            cx.debug_bounds("email-list").is_none(),
            "the mail list must not sit behind settings"
        );
        // The mail shell stays put around settings rather than being replaced
        // by it, so the sidebar and the shared top bar are both still on screen.
        assert!(
            cx.debug_bounds("sidebar-toggle").is_some(),
            "the mail sidebar stays visible behind settings"
        );
        assert!(
            cx.debug_bounds("top-bar").is_some(),
            "settings borrows the mail top bar instead of bringing its own"
        );
        assert!(
            cx.debug_bounds("email-tabs").is_none(),
            "the tab strip names open mail, which settings is not"
        );
        // The top bar names the settings page, using the same breadcrumb shape
        // an open email uses.
        assert_eq!(
            app.read_with(&cx, |app, _| app.settings_page),
            SettingsPage::General
        );
        assert!(
            cx.debug_bounds("top-bar-prefix").is_some(),
            "the top bar shows the settings prefix"
        );
        assert!(
            cx.debug_bounds("top-bar-separator").is_some(),
            "the breadcrumb draws its separator"
        );
        let title = cx
            .debug_bounds("top-bar-title")
            .expect("the top bar still has a title");
        assert!(
            title.size.width > gpui::px(0.),
            "the top bar title is laid out"
        );

        // The same key that closes an overlay closes settings again. It has to
        // be dispatched from the element that currently holds focus, which is
        // inside settings once it is open.
        cx.update(|window, cx| settings_focus.dispatch_action(&Dismiss, window, cx));
        cx.run_until_parked();
        assert!(app.read_with(&cx, |app, _| app.settings.is_none()));
        assert!(cx.debug_bounds("email-list").is_some());
    }

    #[gpui::test]
    fn clicking_a_mailbox_in_the_sidebar_leaves_settings(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.run_until_parked();

        cx.update(|window, cx| focus.dispatch_action(&OpenSettings, window, cx));
        cx.run_until_parked();
        assert!(app.read_with(&cx, |app, _| app.settings.is_some()));
        assert!(
            cx.debug_bounds("settings").is_some(),
            "settings should hold the workspace"
        );

        // The sidebar stays live, so picking a mailbox is a way out of
        // settings as well as a way to change folder.
        cx.update(|window, cx| {
            app.update(cx, |this, cx| {
                this.select_mailbox(Mailbox::Starred, window, cx)
            });
        });
        cx.run_until_parked();
        assert!(
            app.read_with(&cx, |app, _| app.settings.is_none()),
            "picking a mailbox should close settings"
        );
        assert_eq!(
            app.read_with(&cx, |app, _| app.store.selected_mailbox()),
            Mailbox::Starred
        );
        assert!(cx.debug_bounds("email-list").is_some());
    }

    #[gpui::test]
    /// Settings used to dismiss the composer outright. It now only takes the
    /// workspace, because settings is a detour and the draft has to be there
    /// when the user comes back out of it.
    fn opening_settings_while_compose_is_open_hides_but_keeps_it(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| MailApp::new(window, cx))
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let app = window.root(&mut cx).unwrap();
        let focus = app.read_with(&cx, |app, _| app.inbox_focus.clone());
        cx.update(|window, cx| focus.dispatch_action(&Compose, window, cx));
        assert!(app.read_with(&cx, |app, _| app.compose.is_some()));
        cx.update(|window, cx| focus.dispatch_action(&OpenSettings, window, cx));
        assert!(app.read_with(&cx, |app, _| app.settings.is_some()));
        assert!(
            app.read_with(&cx, |app, _| app.compose.is_some()),
            "settings must not destroy the composer"
        );
    }
}
