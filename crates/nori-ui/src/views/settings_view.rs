use std::rc::Rc;

use gpui::{
    App, Context, EventEmitter, FocusHandle, IntoElement, ParentElement, Render, Role,
    ScrollHandle, SharedString, Subscription, Window, div, prelude::*, px, transparent_black,
};

use crate::actions::{Dismiss, SETTINGS_NAV_CONTEXT, SelectNextSection, SelectPreviousSection};
use crate::components::{Button, Icon, ToggleSwitch};
use crate::model::{AccountState, Setting, SettingsPage, SettingsState};
use crate::theme::Theme;

/// Reports one setting's intended value to the app that owns the state.
type SetSettingHandler = Rc<dyn Fn(Setting, bool, &mut App) + 'static>;
/// Reports the page the user picked to the app that owns the state, which is
/// also the app that names it in the top bar.
type SetPageHandler = Rc<dyn Fn(SettingsPage, &mut App) + 'static>;
/// Asks the app to begin an OAuth sign-in. The app owns the browser, the
/// loopback listener and the token store; this view only reports the intent,
/// the same way a setting reports its value rather than applying it.
type SignInHandler = Rc<dyn Fn(&mut App) + 'static>;
/// Asks the app to forget the account.
type SignOutHandler = Rc<dyn Fn(&mut App) + 'static>;

/// The page column sits beside the mail sidebar rather than replacing it, so
/// it is narrower than a sidebar and carries no back row.
const PAGES_COLUMN_WIDTH: f32 = 200.;

/// Descriptions stop here even though the rows themselves run the full width
/// of the pane, so a one-line description does not stretch into a 1500px
/// measure on a wide window. The switch stays pinned to the trailing edge.
const DESCRIPTION_MAX_WIDTH: f32 = 560.;

pub enum SettingsEvent {
    Dismiss,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

/// The settings workspace: a page column beside a full-width content column.
/// It renders inside the mail shell, so the mail sidebar and top bar are its
/// neighbours rather than things it hides. The pages, rows, and defaults are
/// Nori's own.
pub struct SettingsView {
    /// Kept so a change to the theme global repaints this view. The light mode
    /// switch lives on this page, so without it the row that was just flipped
    /// would keep drawing the old palette.
    _theme_sub: Subscription,
    page: SettingsPage,
    /// A copy of the app's settings. Edits are reported upward through
    /// `on_set` rather than applied here, so the mail list behind this view
    /// sees the change and this view never becomes a second source of truth.
    state: SettingsState,
    /// How the connected account reads. Held here rather than in `MailApp` so
    /// the page can be rendered and tested without an account behind it.
    account: AccountState,
    on_set: SetSettingHandler,
    on_page: SetPageHandler,
    on_sign_in: SignInHandler,
    on_sign_out: SignOutHandler,
    nav_focus: FocusHandle,
    scroll: ScrollHandle,
}

impl SettingsView {
    // The view reports intent upward rather than owning it, so the app stays
    // the single writer. That means a callback per concern, and this is the
    // fourth; grouping them would only move the list somewhere else.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        state: SettingsState,
        page: SettingsPage,
        on_set: impl Fn(Setting, bool, &mut App) + 'static,
        on_page: impl Fn(SettingsPage, &mut App) + 'static,
        account: AccountState,
        on_sign_in: impl Fn(&mut App) + 'static,
        on_sign_out: impl Fn(&mut App) + 'static,
    ) -> Self {
        let on_set: SetSettingHandler = Rc::new(on_set);
        let on_page: SetPageHandler = Rc::new(on_page);
        let on_sign_in: SignInHandler = Rc::new(on_sign_in);
        let on_sign_out: SignOutHandler = Rc::new(on_sign_out);
        let nav_focus = cx.focus_handle().tab_index(0).tab_stop(true);
        // The page column takes focus, not a text field: with no search to type
        // into, the column itself is the only thing here that wants keyboard
        // focus, and it is what the arrow keys drive.
        window.focus(&nav_focus, cx);
        let theme_sub = cx.observe_global::<Theme>(|_, cx| cx.notify());
        Self {
            _theme_sub: theme_sub,
            page,
            state,
            account,
            on_set,
            on_page,
            on_sign_in,
            on_sign_out,
            nav_focus,
            scroll: ScrollHandle::new(),
        }
    }

    /// The handle that owns this view's action handling. `MailApp` needs it
    /// to dispatch a dismiss against whichever element currently has focus.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn focus_handle(&self) -> FocusHandle {
        self.nav_focus.clone()
    }

    /// Step the selected page through the list, wrapping at both ends.
    fn cycle_page(&mut self, direction: i32, cx: &mut Context<Self>) {
        let pages = SettingsPage::ALL;
        let current = pages.iter().position(|page| *page == self.page);
        let next = match current {
            Some(index) => (index as i32 + direction).rem_euclid(pages.len() as i32) as usize,
            None if direction > 0 => 0,
            None => pages.len() - 1,
        };
        self.page = pages[next];
        self.report_page(cx);
    }

    fn open_page(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        self.page = page;
        self.report_page(cx);
    }

    /// Hand the new page to the app. The app owns the page for the same
    /// reason it owns the values: the top bar names it, and the top bar is
    /// the app's. Reporting rather than assuming keeps the two in step.
    fn report_page(&mut self, cx: &mut Context<Self>) {
        (self.on_page)(self.page, cx);
        cx.notify();
    }

    // ----- content pieces -------------------------------------------------

    /// The shared left half of a row: a label over a description that stops
    /// at a readable measure. The row itself still spans the pane, so the
    /// switch on the trailing edge stays put however wide the window gets.
    fn render_row_text(
        &self,
        theme: Theme,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> gpui::AnyElement {
        div()
            .max_w(px(DESCRIPTION_MAX_WIDTH))
            .min_w_0()
            .text_size(px(13.5))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.text)
            .child(title.into())
            .child(
                div()
                    .mt(px(5.))
                    .text_size(px(12.5))
                    .line_height(px(18.))
                    .text_color(theme.muted)
                    .child(description.into()),
            )
            .into_any_element()
    }

    /// A note-only row: label and description, no control.
    fn render_note_row(
        &self,
        theme: Theme,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        is_last: bool,
    ) -> gpui::AnyElement {
        div()
            .w_full()
            .flex_none()
            .px(px(24.))
            .py(px(16.))
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(if is_last {
                // A transparent border keeps the row the same height as the
                // others, so the last one does not shift the page.
                transparent_black()
            } else {
                theme.hairline
            })
            .child(self.render_row_text(theme, title, description))
            .into_any_element()
    }

    /// A label/description row with a switch parked on the trailing edge.
    fn render_toggle_row(
        &mut self,
        setting: Setting,
        is_last: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::current(cx);
        let on = self.state.get(setting);
        let id = toggle_id(setting);
        let label = SharedString::from(setting.label());
        let entity = cx.entity();
        let on_set = self.on_set.clone();
        let next = !on;

        div()
            .w_full()
            .flex_none()
            .min_h(px(60.))
            .px(px(24.))
            .py(px(14.))
            .flex()
            .items_center()
            .gap(px(24.))
            .border_b_1()
            .border_color(if is_last {
                transparent_black()
            } else {
                theme.hairline
            })
            .child(div().flex_1().min_w_0().child(self.render_row_text(
                theme,
                setting.label(),
                setting.description(),
            )))
            .child(
                ToggleSwitch::new(id, label, on).on_toggle(move |_window, cx| {
                    // Report the value the switch is moving to, and keep
                    // the local copy in step so the switch redraws from
                    // the same state the app now holds.
                    (on_set)(setting, next, cx);
                    entity.update(cx, |this, cx| {
                        this.state.set(setting, next);
                        cx.notify();
                    })
                }),
            )
            .into_any_element()
    }

    /// A read-only key/value line, for facts rather than controls.
    fn render_value_row(
        &self,
        theme: Theme,
        key: &'static str,
        value: &str,
        is_last: bool,
    ) -> gpui::AnyElement {
        div()
            .px(px(24.))
            .py(px(10.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(if is_last {
                transparent_black()
            } else {
                theme.hairline
            })
            .child(
                div()
                    .w(px(120.))
                    .flex_none()
                    .text_size(px(12.5))
                    .text_color(theme.faint)
                    .child(key),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(theme.text)
                    .child(SharedString::from(value.to_string())),
            )
            .into_any_element()
    }

    /// A page heading, so the content still names itself now that its own
    /// header bar is gone. The page column already shows which page is
    /// selected; this is the anchor for the eye when reading the rows.
    fn render_page_heading(&self, theme: Theme) -> gpui::AnyElement {
        div()
            .w_full()
            .flex_none()
            .px(px(24.))
            .pt(px(22.))
            .pb(px(14.))
            .text_size(px(19.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.text)
            .child(self.page.label())
            .into_any_element()
    }

    fn render_general(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::current(cx);
        div()
            .child(self.render_page_heading(theme))
            .child(self.render_note_row(
                theme,
                "Local by default",
                "Nori keeps your mail on this machine. Nothing is uploaded, and this \
                 prototype ships with a bundled set of sample messages rather than a \
                 connected account.",
                false,
            ))
            .child(self.render_toggle_row(Setting::MarkReadOnOpen, false, cx))
            .child(self.render_toggle_row(Setting::UnreadBadges, false, cx))
            .child(self.render_toggle_row(Setting::ConfirmBeforeArchive, true, cx))
            .into_any_element()
    }

    fn render_appearance(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::current(cx);
        div()
            .child(self.render_page_heading(theme))
            // First on the page because it is the one setting here that
            // changes every other row on screen.
            .child(self.render_toggle_row(Setting::LightMode, false, cx))
            .child(self.render_note_row(
                theme,
                "Two palettes",
                "Both palettes live side by side in theme.rs. The switch republishes \
                 the theme global, so every open view redraws from the new tokens \
                 rather than from a copy it was handed earlier.",
                false,
            ))
            .child(self.render_toggle_row(Setting::CompactRows, false, cx))
            .child(self.render_toggle_row(Setting::ShowSender, true, cx))
            .into_any_element()
    }

    fn render_mail(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::current(cx);
        div()
            .child(self.render_page_heading(theme))
            .child(self.render_toggle_row(Setting::OpenInTab, false, cx))
            .child(self.render_toggle_row(Setting::GroupConversations, false, cx))
            .child(self.render_toggle_row(Setting::ShowAttachments, true, cx))
            .into_any_element()
    }

    /// The account page.
    ///
    /// The rows say what is actually connected and where the token lives,
    /// rather than describing a sign-in that does not exist. `Storage` naming
    /// "in memory, resets on quit" was true only while the prototype read
    /// sample mail, and leaving it there next to a real account would be a lie
    /// about where a refresh token is kept.
    fn render_account(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::current(cx);
        let mut rows: Vec<gpui::AnyElement> = vec![self.render_page_heading(theme)];

        match &self.account {
            AccountState::Disconnected => {
                rows.push(self.render_note_row(
                    theme,
                    "No account connected",
                    "Sign in to sync a Gmail mailbox. Nori reads mail on your own \
                     machine and sends nothing to any server of ours.",
                    false,
                ));
                rows.push(self.render_value_row(theme, "Address", "Not connected", false));
                rows.push(self.render_value_row(
                    theme,
                    "Storage",
                    "Token file, created on first sign-in",
                    false,
                ));
                rows.push(self.render_sign_in_button("Sign in with Gmail"));
            }
            AccountState::Connecting => {
                rows.push(self.render_note_row(
                    theme,
                    "Signing in…",
                    "Finish signing in the tab that just opened. Nori is \
                     listening for the redirect, then it will start fetching \
                     your mail.",
                    false,
                ));
            }
            AccountState::Fetching { address } => {
                // The long part of a sign-in. Saying "waiting for the browser"
                // here was true for a few seconds and a lie for the next
                // minute, over an empty list that looks broken.
                rows.push(self.render_note_row(
                    theme,
                    "Fetching your mail…",
                    format!(
                        "Signed in as {address}. Reading your mailbox now — this \
                         takes a minute on a large one, and your mail appears as \
                         it arrives."
                    ),
                    false,
                ));
            }
            AccountState::Connected {
                address,
                mail,
                labels,
                ..
            } => {
                rows.push(self.render_note_row(
                    theme,
                    address.clone(),
                    "Synced. Mail and labels are read on demand, and read state \
                     and stars are written back.",
                    false,
                ));
                rows.push(self.render_value_row(
                    theme,
                    "Synced",
                    &format!("{mail} messages, {labels} labels"),
                    false,
                ));
                rows.push(self.render_value_row(
                    theme,
                    "Refresh",
                    if self.account.is_usable() {
                        "On open, and every few minutes"
                    } else {
                        "Paused until the account is usable"
                    },
                    false,
                ));
                rows.push(self.render_sign_out_button());
            }
            AccountState::NeedsReauth { address } => {
                // Expected, not exceptional: an app in Google's "Testing"
                // publishing status has its refresh tokens expire after seven
                // days by design.
                rows.push(self.render_note_row(
                    theme,
                    format!("{address} needs to sign in again"),
                    "Google expires the token after about a week while the app is \
                     unverified. Nothing was lost — signing in again picks up where \
                     it left off.",
                    false,
                ));
                rows.push(self.render_sign_in_button("Sign in again"));
            }
            AccountState::Failed { reason } => {
                rows.push(self.render_note_row(theme, "Could not connect", reason.clone(), false));
                rows.push(self.render_sign_in_button("Try again"));
            }
        }

        rows.push(self.render_toggle_row(Setting::CheckForMail, false, cx));
        rows.push(self.render_toggle_row(Setting::ReadReceipts, true, cx));
        div().children(rows).into_any_element()
    }

    /// Square and flush left, matching the rows above it rather than sitting
    /// in a card of its own.
    fn render_sign_in_button(&self, label: &'static str) -> gpui::AnyElement {
        let on_sign_in = self.on_sign_in.clone();
        div()
            .px(px(24.))
            // The same height as a toggle row, with the button centred in it.
            // At its own padding the row was 48px next to a 74px toggle row
            // whose label sits centred, so the button looked pinned to the row
            // above with a void under it. Matching the height puts equal space
            // above and below the button and keeps the page's rhythm even.
            .min_h(px(60.))
            .flex()
            .items_center()
            .child(
                div()
                    .debug_selector(|| "account-sign-in".to_string())
                    // A flex row, so the button becomes a flex *item* and
                    // shrinks to its label. Left as a block box the button is
                    // block-level too and fills the whole content pane, which
                    // is what turned a primary action into a full-width slab.
                    .flex()
                    .child(
                        Button::new("account-sign-in", label)
                            .style(crate::components::ButtonStyle::Accent)
                            .on_click(move |_event, _window, cx| on_sign_in(cx)),
                    )
                    .id("account-sign-in-wrap"),
            )
            .id("account-sign-in-row")
            .into_any_element()
    }

    fn render_sign_out_button(&self) -> gpui::AnyElement {
        let on_sign_out = self.on_sign_out.clone();
        div()
            .px(px(24.))
            .min_h(px(60.))
            .flex()
            .items_center()
            .child(
                div()
                    .debug_selector(|| "account-sign-out".to_string())
                    .flex()
                    .child(
                        Button::new("account-sign-out", "Disconnect")
                            // `Subtle`, not the `Ghost` default. A ghost button
                            // is a transparent background, a transparent border
                            // and muted text, which reads as a label rather than
                            // something you can press. `Subtle` gives it a
                            // surface and a real border, and keeps the sign-in
                            // button as the only filled `Accent` on the page.
                            .style(crate::components::ButtonStyle::Subtle)
                            .on_click(move |_event, _window, cx| on_sign_out(cx)),
                    )
                    .id("account-sign-out-wrap"),
            )
            .id("account-sign-out-row")
            .into_any_element()
    }

    fn render_about(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::current(cx);
        div()
            .child(self.render_page_heading(theme))
            .child(self.render_note_row(
                theme,
                "Nori",
                "A native mail client prototype built with GPUI. The sample mail, the \
                 settings above, and these facts are all part of the prototype.",
                false,
            ))
            .child(self.render_value_row(theme, "Version", env!("CARGO_PKG_VERSION"), false))
            .child(self.render_value_row(theme, "Interface", "GPUI (Rust)", false))
            .child(self.render_value_row(
                theme,
                "Source",
                "crates/nori-ui, crates/nori-desktop",
                true,
            ))
            .into_any_element()
    }

    // ----- columns --------------------------------------------------------

    fn render_nav(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::current(cx);
        let pages = SettingsPage::ALL;
        let current = self.page;

        div()
            .id("settings-nav")
            .key_context(SETTINGS_NAV_CONTEXT)
            .track_focus(&self.nav_focus)
            .w(px(PAGES_COLUMN_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.chrome)
            .border_r_1()
            .border_color(theme.border)
            // Just the page list. There is no search field and no heading: the
            // mail sidebar sits right next to this column, so any mailbox is a
            // way out, as is Escape, and the column needs no label of its own.
            .child(
                div()
                    .id("settings-nav-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    // No horizontal inset: the selected row's fill and its
                    // hover wash run the full width of the column, edge to
                    // edge. The rows carry their own inner padding, so the
                    // labels stay inset while the highlight does not.
                    //
                    // No top pad either: the column has no header of its own,
                    // so a pad here would just push the first page down from
                    // the top bar.
                    .pb(px(10.))
                    .children(pages.into_iter().map(|page| {
                        let selected = page == current;
                        div()
                            .id(page.nav_id())
                            .debug_selector(move || page.nav_id().to_string())
                            .h(px(36.))
                            .w_full()
                            .px(px(11.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .cursor_pointer()
                            .role(Role::Button)
                            .aria_label(page.label())
                            .aria_selected(selected)
                            .focusable()
                            .tab_stop(true)
                            .focus_visible(|style| style.border_color(theme.focus))
                            .bg(if selected {
                                theme.selected
                            } else {
                                transparent_black()
                            })
                            .hover(|style| {
                                style.bg(if selected {
                                    theme.selected
                                } else {
                                    theme.hover_subtle
                                })
                            })
                            .text_size(px(13.))
                            .text_color(if selected { theme.text } else { theme.muted })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_page(page, cx);
                            }))
                            .child(Icon::new(
                                page.icon(),
                                15.,
                                if selected { theme.text } else { theme.faint },
                            ))
                            .child(page.label())
                    })),
            )
            .into_any_element()
    }

    fn render_content(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::current(cx);
        let body: gpui::AnyElement = match self.page {
            SettingsPage::General => self.render_general(cx),
            SettingsPage::Appearance => self.render_appearance(cx),
            SettingsPage::Mail => self.render_mail(cx),
            SettingsPage::Account => self.render_account(cx),
            SettingsPage::About => self.render_about(cx),
        };

        div()
            .id("settings-content")
            .flex_1()
            .h_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(theme.canvas)
            // No header bar of its own. The mail top bar above already names
            // the page, and a second bar under it was the chrome the page
            // heading replaced.
            .child(
                div()
                    .id("settings-content-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .pb(px(48.))
                    // Full bleed and left aligned: no cap, no centring. Row
                    // padding supplies the only inset.
                    .child(div().w_full().child(body)),
            )
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let entity = cx.entity();
        div()
            .id("settings")
            .debug_selector(|| "settings".into())
            .size_full()
            .flex()
            .bg(theme.canvas)
            .text_color(theme.text)
            // Escape belongs to whatever is on top, so settings answers it
            // here rather than letting the mail layout underneath try.
            .on_action(move |_: &Dismiss, _window, cx| {
                entity.update(cx, |_, cx| cx.emit(SettingsEvent::Dismiss));
            })
            .on_action(cx.listener(|this, _: &SelectNextSection, _, cx| {
                this.cycle_page(1, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousSection, _, cx| {
                this.cycle_page(-1, cx);
            }))
            .child(self.render_nav(cx))
            .child(self.render_content(cx))
    }
}

/// A switch's element id.
fn toggle_id(setting: Setting) -> SharedString {
    SharedString::from(setting.element_id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, TestAppContext, VisualTestContext, WindowHandle};
    use std::cell::RefCell;

    fn open_settings(cx: &mut TestAppContext) -> (WindowHandle<SettingsView>, VisualTestContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| {
                    SettingsView::new(
                        window,
                        cx,
                        SettingsState::new(),
                        SettingsPage::General,
                        |_, _, _| {},
                        |_, _| {},
                        AccountState::default(),
                        |_| {},
                        |_| {},
                    )
                })
            })
            .unwrap()
        });
        let cx = VisualTestContext::from_window(window.into(), cx);
        // A window with no size lays nothing out, so nothing reports bounds.
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        (window, cx)
    }

    #[gpui::test]
    fn nav_lists_every_page_by_default(cx: &mut TestAppContext) {
        let (window, mut cx) = open_settings(cx);
        window.root(&mut cx).unwrap();
        cx.run_until_parked();
        for page in SettingsPage::ALL {
            let found = cx.debug_bounds(page.nav_id()).is_some();
            assert!(found, "{page:?} should be listed in the nav");
        }
    }

    #[gpui::test]
    fn cycling_pages_wraps_in_both_directions(cx: &mut TestAppContext) {
        let (window, mut cx) = open_settings(cx);
        let view = window.root(&mut cx).unwrap();
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, cx| this.cycle_page(-1, cx));
        });
        assert_eq!(
            view.read_with(&cx, |this, _| this.page),
            SettingsPage::About,
            "stepping up from the first page wraps to the last"
        );
        cx.update(|_, cx| {
            view.update(cx, |this, cx| this.cycle_page(1, cx));
        });
        assert_eq!(
            view.read_with(&cx, |this, _| this.page),
            SettingsPage::General
        );
    }

    #[gpui::test]
    fn toggling_a_setting_moves_the_local_copy(cx: &mut TestAppContext) {
        let (window, mut cx) = open_settings(cx);
        let view = window.root(&mut cx).unwrap();
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, _| {
                assert!(this.state.mark_read_on_open);
                this.state.toggle(Setting::MarkReadOnOpen);
            });
        });
        assert!(!view.read_with(&cx, |this, _| this.state.mark_read_on_open));
    }

    #[gpui::test]
    fn changing_page_reports_it_upward(cx: &mut TestAppContext) {
        // The app owns the page so the top bar can name it, so picking a row
        // has to reach it rather than only repainting this view.
        let reported: Rc<RefCell<Vec<SettingsPage>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = reported.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| {
                    let sink = sink.clone();
                    SettingsView::new(
                        window,
                        cx,
                        SettingsState::new(),
                        SettingsPage::General,
                        |_, _, _| {},
                        move |page, _cx| sink.borrow_mut().push(page),
                        AccountState::default(),
                        |_| {},
                        |_| {},
                    )
                })
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let view = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        cx.update(|_, cx| {
            view.update(cx, |this, cx| this.open_page(SettingsPage::About, cx));
        });
        assert_eq!(
            reported.borrow().as_slice(),
            &[SettingsPage::About],
            "the new page should be reported, not just drawn"
        );
    }

    #[gpui::test]
    fn a_switch_reports_the_value_it_moves_to(cx: &mut TestAppContext) {
        // The view does not own settings state, so the report is the whole
        // contract: the app has to be told what the switch is asking for.
        let reported: Rc<RefCell<Vec<(Setting, bool)>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = reported.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| {
                    let sink = sink.clone();
                    SettingsView::new(
                        window,
                        cx,
                        SettingsState::new(),
                        SettingsPage::General,
                        move |setting, enabled, _cx| {
                            sink.borrow_mut().push((setting, enabled));
                        },
                        |_, _| {},
                        AccountState::default(),
                        |_| {},
                        |_| {},
                    )
                })
            })
            .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(760.)));
        let view = window.root(&mut cx).unwrap();
        cx.run_until_parked();

        // Click the switch itself, not a guessed point in the page.
        let setting = Setting::MarkReadOnOpen;
        let bounds = cx
            .debug_bounds(setting.element_id())
            .expect("the General page's first switch should be rendered");
        let center = bounds.center();
        cx.simulate_click(
            gpui::Point::new(center.x, center.y),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();

        assert_eq!(
            reported.borrow().as_slice(),
            &[(setting, false)],
            "a switch that starts on must report the value it moves to"
        );
        assert!(
            !view.read_with(&cx, |this, _| this.state.get(setting)),
            "the view mirrors what it reported, so it cannot drift from the app"
        );
    }

    #[gpui::test]
    fn a_switch_is_rendered_for_the_selected_page(cx: &mut TestAppContext) {
        let (window, mut cx) = open_settings(cx);
        let view = window.root(&mut cx).unwrap();
        cx.run_until_parked();
        let first = Setting::MarkReadOnOpen.element_id();
        assert!(
            cx.debug_bounds(first).is_some(),
            "the General page leads with its first switch"
        );
        // A different page shows its own rows, not the previous page's.
        cx.update(|_, cx| {
            view.update(cx, |this, cx| this.open_page(SettingsPage::Mail, cx));
        });
        cx.run_until_parked();
        let mail_row = Setting::OpenInTab.element_id();
        assert!(
            cx.debug_bounds(mail_row).is_some(),
            "the Mail page shows its own switches"
        );
    }
}
