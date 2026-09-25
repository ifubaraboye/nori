use crate::actions::{
    CloseTab, Compose, Dismiss, MoveSelectionDown, MoveSelectionUp, NextTab, OpenSearch,
    OpenSelected, OpenSettings, PreviousTab, ToggleSidebar, ToggleTabStrip,
};
use gpui::{
    Context, CursorStyle, Entity, FocusHandle, IntoElement, MouseButton, MouseMoveEvent,
    MouseUpEvent, ParentElement, Render, Role, ScrollHandle, SharedString, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, transparent_black,
};

use super::compose_view::{ComposeEvent, ComposeView};
use super::email_view::EmailView;
use super::inbox::{Inbox, scroll_selected_into_view};
use super::search_view::{SearchEvent, SearchView};
use super::settings_view::{SettingsEvent, SettingsView};
use crate::components::{
    EmailTabs, SIDEBAR_DEFAULT_WIDTH, Sidebar, TextField, TopBar, clamp_sidebar_width,
};
use crate::model::{
    Density, DraftSeed, Email, EmailId, LabelId, LabelStore, MailStore, Mailbox, Overlay, Setting,
    SettingsPage, SettingsState, WorkspaceView, mock::mock_emails,
};
use crate::theme::Theme;

struct SidebarResize {
    start_x: f32,
    start_width: f32,
}

/// A drag of the compose divider. `start_x` is where the pointer went down,
/// and the width is recomputed from the delta so the divider tracks the
/// pointer exactly instead of jumping to the cursor.
struct ComposeResize {
    start_x: f32,
    start_width: f32,
}

/// How far below the top of the window an overlay panel sits.
const OVERLAY_TOP_OFFSET: f32 = 52.;

/// Width of the compose column. Wide enough to write a mail in, narrow enough
/// that the list beside it stays scannable.
const COMPOSE_PANE_WIDTH: f32 = 520.;

/// The compose column is user-resizable, so it needs ends. Below the minimum
/// the fields stop being usable; above the maximum the list beside it is gone.
const COMPOSE_PANE_MIN: f32 = 340.;
const COMPOSE_PANE_MAX: f32 = 960.;

pub fn clamp_compose_pane_width(width: f32) -> f32 {
    width.clamp(COMPOSE_PANE_MIN, COMPOSE_PANE_MAX)
}

pub struct MailApp {
    store: MailStore,
    /// User-defined labels, kept beside the store so mailbox state and label
    /// state never disturb each other.
    labels: LabelStore,
    /// The label the list is filtered by, or `None` for no label filter.
    selected_label: Option<LabelId>,
    /// The inline "new label" field in the sidebar.
    new_label_field: Entity<TextField>,
    theme: Theme,
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
    sidebar_resize: Option<SidebarResize>,
    /// Width of the compose column, remembered between openings so a resize
    /// is not undone by closing the pane.
    compose_pane_width: f32,
    compose_resize: Option<ComposeResize>,
    compose: Option<Entity<ComposeView>>,
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
    subscriptions: Vec<Subscription>,
    previous_focus: Option<FocusHandle>,
}

impl MailApp {
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
            labels.set_for(EmailId(1), &[work.id]);
            labels.set_for(EmailId(4), &[work.id]);
        }
        if let Some(personal) = &personal {
            labels.set_for(EmailId(7), &[personal.id]);
        }
        let new_label_field =
            cx.new(|cx| TextField::new("new-label", "New label", "", true, 3, cx));
        Self {
            store,
            labels,
            selected_label: None,
            new_label_field,
            theme: Theme::dark(),
            inbox_focus,
            workspace_focus,
            tab_scroll: ScrollHandle::new(),
            inbox_scroll: UniformListScrollHandle::new(),
            sidebar_visible: true,
            tab_strip_visible: true,
            sidebar_width: SIDEBAR_DEFAULT_WIDTH,
            mailboxes_collapsed: false,
            sidebar_resize: None,
            compose_pane_width: COMPOSE_PANE_WIDTH,
            compose_resize: None,
            compose: None,
            search: None,
            settings: None,
            settings_page: SettingsPage::General,
            settings_state: SettingsState::new(),
            subscriptions: Vec::new(),
            previous_focus: None,
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
        if self.settings.is_some() {
            self.close_settings(window, cx);
        } else if self.store.overlay().is_some() {
            self.close_overlay(window, cx);
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
        if self.store.overlay().is_some() {
            self.close_overlay(window, cx);
        }
        self.previous_focus = window.focused(cx);
        self.settings_page = SettingsPage::General;
        let settings_entity = cx.entity();
        let page_entity = cx.entity();
        let state = self.settings_state;
        let page = self.settings_page;
        let settings = cx.new(|cx| {
            SettingsView::new(
                window,
                cx,
                state,
                page,
                move |setting, enabled, cx| {
                    settings_entity.update(cx, |this, cx| this.set_setting(setting, enabled, cx));
                },
                move |page, cx| {
                    page_entity.update(cx, |this, cx| this.set_settings_page(page, cx));
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
        self.subscriptions.push(subscription);
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = None;
        self.subscriptions.clear();
        if let Some(previous) = self.previous_focus.take() {
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
            cx.notify();
        }
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
        if self.store.overlay().is_some() {
            self.close_overlay(window, cx);
        }
        self.store.select_mailbox(mailbox);
        window.focus(&self.inbox_focus, cx);
        cx.notify();
    }

    fn open_email(&mut self, id: EmailId, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.open_email(id) {
            window.focus(&self.workspace_focus, cx);
            cx.notify();
        }
    }

    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(email) = self.store.selected_email().cloned() {
            self.open_email(email.id, window, cx);
        }
    }

    fn toggle_star(&mut self, id: EmailId, cx: &mut Context<Self>) {
        self.store.toggle_star(id);
        cx.notify();
    }

    fn open_compose(&mut self, seed: DraftSeed, window: &mut Window, cx: &mut Context<Self>) {
        if self.compose.is_some() {
            return;
        }
        self.previous_focus = window.focused(cx);
        let compose = cx.new(|cx| ComposeView::new(seed, window, cx));
        let subscription =
            cx.subscribe_in(&compose, window, |this, _, event, window, cx| match event {
                ComposeEvent::Dismiss => this.close_overlay(window, cx),
            });
        self.store.set_overlay(Some(Overlay::Compose));
        self.compose = Some(compose);
        self.subscriptions.push(subscription);
        cx.notify();
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_some() {
            return;
        }
        self.previous_focus = window.focused(cx);
        let emails = self.store.emails().to_vec();
        let search = cx.new(|cx| SearchView::new(emails, window, cx));
        let subscription =
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                SearchEvent::Open(id) => {
                    this.close_overlay(window, cx);
                    this.open_email(*id, window, cx);
                }
                SearchEvent::Dismiss => this.close_overlay(window, cx),
            });
        self.store.set_overlay(Some(Overlay::Search));
        self.search = Some(search);
        self.subscriptions.push(subscription);
        cx.notify();
    }

    fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.compose = None;
        self.search = None;
        self.subscriptions.clear();
        self.store.set_overlay(None);
        if let Some(previous) = self.previous_focus.take() {
            window.focus(&previous, cx);
        }
        cx.notify();
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
                email.body.join("\n\n")
            )
        } else {
            format!("\n\nOn {}:\n{}", email.full_date, email.body.join("\n\n"))
        };
        DraftSeed { to, subject, body }
    }

    /// Search only. Compose is not an overlay: it is a second column in the
    /// workspace, so the mail list stays visible while you write.
    fn render_overlay(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let theme = self.theme;
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
                        this.close_overlay(window, cx);
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

    fn toggle_mailboxes(&mut self, cx: &mut Context<Self>) {
        self.mailboxes_collapsed = !self.mailboxes_collapsed;
        cx.notify();
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

    fn set_compose_pane_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let next = clamp_compose_pane_width(width);
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

    /// The divider sits on the pane's left edge, so dragging right *widens*
    /// the pane: the width moves opposite to the pointer's x.
    fn update_compose_resize(&mut self, cursor_x: f32, cx: &mut Context<Self>) {
        if let Some(resize) = &self.compose_resize {
            let width = resize.start_width - (cursor_x - resize.start_x);
            self.set_compose_pane_width(width, cx);
        }
    }

    fn end_compose_resize(&mut self, cx: &mut Context<Self>) {
        if self.compose_resize.take().is_some() {
            cx.notify();
        }
    }

    /// Keyboard equivalent of the drag, so the divider is not pointer-only.
    fn nudge_compose_pane(&mut self, step: f32, cx: &mut Context<Self>) {
        self.set_compose_pane_width(self.compose_pane_width + step, cx);
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

    fn compose_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.overlay().is_none() {
            self.open_compose(DraftSeed::default(), window, cx);
        }
    }

    fn search_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.overlay().is_none() {
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
    fn toggle_pin(&mut self, id: EmailId, cx: &mut Context<Self>) {
        self.store.toggle_pin(id);
        cx.notify();
    }

    /// Retrace the previous view, the way the sidebar's back chevron does.
    fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            // Blank or duplicate: leave the text so it can be corrected.
            return;
        };
        let field = self.new_label_field.clone();
        self.new_label_field
            .update(cx, |this, cx| this.set_content("", cx));
        window.focus(&field.read(cx).focus_handle(), cx);
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

    /// Add or remove a label on the open mail.
    fn toggle_label(&mut self, id: LabelId, email: EmailId, cx: &mut Context<Self>) {
        self.labels.toggle(email, id);
        cx.notify();
    }

    fn render_inbox(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let open_entity = entity.clone();
        let star_entity = entity.clone();
        // A selected label narrows the list to the mails carrying it, on top of
        // whatever mailbox is showing.
        let mut rows = self.store.visible_summaries();
        if let Some(label) = self.selected_label {
            rows.retain(|row| self.labels.labels_for(row.id).contains(&label));
        }
        let selected_index = self.store.selected_index();
        Inbox::new(
            rows,
            selected_index,
            self.inbox_focus.clone(),
            self.inbox_scroll.clone(),
            self.density(),
            self.theme,
            move |id, window, cx| {
                open_entity.update(cx, |this, cx| this.open_email(id, window, cx));
            },
            move |id, _window, cx| {
                star_entity.update(cx, |this, cx| this.toggle_star(id, cx));
            },
        )
    }
}

impl Render for MailApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
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
        let create_label_entity = entity.clone();
        let delete_label_entity = entity.clone();
        let rename_label_entity = entity.clone();
        let counts = [
            self.store.count(Mailbox::Inbox),
            self.store.count(Mailbox::Starred),
            self.store.count(Mailbox::Sent),
            self.store.count(Mailbox::Drafts),
            self.store.count(Mailbox::Archive),
            self.store.count(Mailbox::Trash),
        ];
        let sidebar = Sidebar::new(
            self.store.selected_mailbox(),
            counts,
            theme,
            self.sidebar_width,
            self.sidebar_visible,
            self.mailboxes_collapsed,
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
            self.new_label_field.clone(),
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
                WorkspaceView::Email(id) => match self.store.email(id) {
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
                    .email(*id)
                    .map(|email| (*id, email.subject.clone().into()))
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
                theme,
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
        let top_bar = TopBar::new(
            theme,
            self.sidebar_visible,
            prefix,
            title,
            move |window, cx| {
                top_toggle_entity.update(cx, |this, cx| this.toggle_sidebar(window, cx));
            },
        );
        let workspace: gpui::AnyElement = match self.settings.clone() {
            // Settings holds the workspace beside the mail sidebar. The mail
            // views are not built at all while it is open, so the list behind
            // it costs nothing.
            Some(settings) => settings.into_any_element(),
            None => match self.store.workspace_view() {
                WorkspaceView::Mailbox => self.render_inbox(cx).into_any_element(),
                WorkspaceView::Email(id) => {
                    if let Some(email) = self.store.email(id).cloned() {
                        let reply = Self::reply_seed(&email, false, false);
                        let reply_all = Self::reply_seed(&email, true, false);
                        let forward = Self::reply_seed(&email, false, true);
                        let view_entity = cx.entity();
                        let reply_entity = view_entity.clone();
                        let reply_all_entity = view_entity.clone();
                        let forward_entity = view_entity.clone();
                        let pin_entity = view_entity.clone();
                        let label_assign_entity = view_entity.clone();
                        let email_id = email.id;
                        EmailView::new(
                            email,
                            self.workspace_focus.clone(),
                            theme,
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
                                pin_entity.update(cx, |this, cx| this.toggle_pin(email_id, cx));
                            },
                            self.labels.labels().to_vec(),
                            self.labels.labels_for(email_id).to_vec(),
                            move |label, _window, cx| {
                                label_assign_entity
                                    .update(cx, |this, cx| this.toggle_label(label, email_id, cx));
                            },
                        )
                        .into_any_element()
                    } else {
                        self.render_inbox(cx).into_any_element()
                    }
                }
            },
        };
        let overlay = self.render_overlay(cx);
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
                .left(px(-5.))
                .w(px(10.))
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
                    move |event, _window, cx| {
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
                                resize_entity
                                    .update(cx, |this, cx| this.nudge_compose_pane(delta, cx));
                                cx.stop_propagation();
                            }
                            "home" => {
                                resize_entity.update(cx, |this, cx| {
                                    this.set_compose_pane_width(COMPOSE_PANE_MIN, cx)
                                });
                                cx.stop_propagation();
                            }
                            "end" => {
                                resize_entity.update(cx, |this, cx| {
                                    this.set_compose_pane_width(COMPOSE_PANE_MAX, cx)
                                });
                                cx.stop_propagation();
                            }
                            _ => {}
                        }
                    }
                })
                .child(
                    // Only lights up on hover, matching the sidebar's handle.
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(4.))
                        .w(px(2.))
                        .bg(transparent_black())
                        .group_hover("compose-resize", |style| style.bg(theme.focus)),
                );

            div()
                .id("compose-pane-shell")
                .debug_selector(|| "compose-pane-shell".into())
                .relative()
                .flex_none()
                .w(px(self.compose_pane_width))
                .h_full()
                .flex()
                .border_l_1()
                .border_color(theme.hairline_strong)
                .child(compose)
                .child(handle)
        });

        div()
            .id("mail-app")
            .relative()
            .size_full()
            .flex()
            .bg(theme.canvas)
            .on_action(cx.listener(Self::open_settings_action))
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
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                this.update_sidebar_resize(event.position.x.as_f32(), cx);
                this.update_compose_resize(event.position.x.as_f32(), cx);
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
                                .child(
                                    div()
                                        .id("workspace-main")
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .child(workspace),
                                )
                                .when_some(
                                    compose_pane.filter(|_| !settings_open),
                                    |this, pane| this.child(pane),
                                ),
                        ),
                ),
            )
            .when(!settings_open, |this| {
                this.when_some(overlay, |this, overlay| this.child(overlay))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{CloseTab, Compose, OpenSelected};
    use gpui::{AppContext, TestAppContext, VisualTestContext};

    const COMPOSE_PANE_MAX: f32 = super::COMPOSE_PANE_MAX;
    const COMPOSE_PANE_MIN: f32 = super::COMPOSE_PANE_MIN;

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
        app.update(&mut cx, |app, cx| app.toggle_pin(id, cx));
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
        app.update(&mut cx, |app, cx| app.toggle_pin(id, cx));
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
        app.update(&mut cx, |app, cx| app.toggle_pin(id, cx));
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
            app.read_with(&cx, |app, _| app.store.email(id).is_some_and(|e| e.pinned)),
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
            WorkspaceView::Email(EmailId(1)),
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

        // And the clamp holds at the ends.
        cx.update(|_, cx| {
            app.update(cx, |this, cx| this.set_compose_pane_width(10_000., cx));
        });
        assert_eq!(
            app.read_with(&cx, |app, _| app.compose_pane_width),
            COMPOSE_PANE_MAX
        );
        cx.update(|_, cx| {
            app.update(cx, |this, cx| this.set_compose_pane_width(0., cx));
        });
        assert_eq!(
            app.read_with(&cx, |app, _| app.compose_pane_width),
            COMPOSE_PANE_MIN
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

        // The sidebar renders the seeded labels and the create row.
        assert!(
            cx.debug_bounds("sidebar-labels").is_some(),
            "the sidebar renders a labels section"
        );
        assert!(cx.debug_bounds("sidebar-label-new").is_some());

        // Create a label through the app, the way the sidebar row commits it.
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
        let email_id = EmailId(1);
        let work = app
            .read_with(&cx, |app, _| {
                app.labels
                    .labels()
                    .iter()
                    .find(|l| l.name == "Work")
                    .map(|l| l.id)
            })
            .expect("Work is seeded");
        app.update(&mut cx, |app, cx| app.toggle_label(follow_up, email_id, cx));
        assert_eq!(
            app.read_with(&cx, |app, _| app.labels.labels_for(email_id).to_vec()),
            vec![work, follow_up],
            "the new label joins the one the mail already had"
        );
        // Open the mail so the reading view, and its label picker, render.
        app.update(&mut cx, |app, cx| {
            app.store.open_email(email_id);
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("email-labels").is_some(),
            "the reading view renders the label picker"
        );

        // Deleting it clears the assignment and the filter.
        app.update(&mut cx, |app, cx| app.delete_label(follow_up, cx));
        assert_eq!(
            app.read_with(&cx, |app, _| app.labels.labels_for(email_id).to_vec()),
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
    fn opening_settings_while_compose_is_open_dismisses_the_overlay(cx: &mut TestAppContext) {
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
        assert!(app.read_with(&cx, |app, _| app.store.overlay().is_some()));
        cx.update(|window, cx| focus.dispatch_action(&OpenSettings, window, cx));
        assert!(app.read_with(&cx, |app, _| app.store.overlay().is_none()));
        assert!(app.read_with(&cx, |app, _| app.settings.is_some()));
    }
}
