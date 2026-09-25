use std::rc::Rc;

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement, Render, Role,
    ScrollHandle, SharedString, Window, div, prelude::*, px, transparent_black,
};

use crate::actions::{Dismiss, SETTINGS_NAV_CONTEXT, SelectNextSection, SelectPreviousSection};
use crate::components::{Icon, TextField, ToggleSwitch};
use crate::model::{Setting, SettingsPage, SettingsState};
use crate::theme::Theme;

/// Reports one setting's intended value to the app that owns the state.
type SetSettingHandler = Rc<dyn Fn(Setting, bool, &mut App) + 'static>;
/// Reports the page the user picked to the app that owns the state, which is
/// also the app that names it in the top bar.
type SetPageHandler = Rc<dyn Fn(SettingsPage, &mut App) + 'static>;

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
    theme: Theme,
    page: SettingsPage,
    /// A copy of the app's settings. Edits are reported upward through
    /// `on_set` rather than applied here, so the mail list behind this view
    /// sees the change and this view never becomes a second source of truth.
    state: SettingsState,
    on_set: SetSettingHandler,
    on_page: SetPageHandler,
    search: Entity<TextField>,
    nav_focus: FocusHandle,
    scroll: ScrollHandle,
}

impl SettingsView {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        state: SettingsState,
        page: SettingsPage,
        on_set: impl Fn(Setting, bool, &mut App) + 'static,
        on_page: impl Fn(SettingsPage, &mut App) + 'static,
    ) -> Self {
        let on_set: SetSettingHandler = Rc::new(on_set);
        let on_page: SetPageHandler = Rc::new(on_page);
        let search = cx.new(|cx| TextField::new("settings-search", "Settings", "", true, 1, cx));
        let nav_focus = cx.focus_handle().tab_index(0).tab_stop(true);
        window.focus(&search.read(cx).focus_handle(), cx);
        Self {
            theme: Theme::dark(),
            page,
            state,
            on_set,
            on_page,
            search,
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

    /// The page list narrowed by the nav search, trimmed and lowercased.
    fn query(&self, cx: &App) -> String {
        self.search.read(cx).content().trim().to_lowercase()
    }

    fn visible_pages(&self, cx: &App) -> Vec<SettingsPage> {
        SettingsPage::visible(&self.query(cx))
    }

    /// Step the selected page through the rows the search leaves visible,
    /// wrapping at both ends. A page filtered out by the query re-enters the
    /// list from whichever end the key came from.
    fn cycle_page(&mut self, direction: i32, cx: &mut Context<Self>) {
        let pages = self.visible_pages(cx);
        if pages.is_empty() {
            return;
        }
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
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> gpui::AnyElement {
        let theme = self.theme;
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
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        is_last: bool,
    ) -> gpui::AnyElement {
        let theme = self.theme;
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
            .child(self.render_row_text(title, description))
            .into_any_element()
    }

    /// A label/description row with a switch parked on the trailing edge.
    fn render_toggle_row(
        &mut self,
        setting: Setting,
        is_last: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = self.theme;
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
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(self.render_row_text(setting.label(), setting.description())),
            )
            .child(
                ToggleSwitch::new(id, label, on)
                    .theme(theme)
                    .on_toggle(move |_window, cx| {
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
    fn render_value_row(&self, key: &'static str, value: &str, is_last: bool) -> gpui::AnyElement {
        let theme = self.theme;
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
    fn render_page_heading(&self) -> gpui::AnyElement {
        let theme = self.theme;
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
        div()
            .child(self.render_page_heading())
            .child(self.render_note_row(
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
        div()
            .child(self.render_page_heading())
            .child(self.render_note_row(
                "One palette today",
                "Nori ships a single dark palette. The tokens live in theme.rs, so a \
                 second palette is a matter of adding one alongside it.",
                false,
            ))
            .child(self.render_toggle_row(Setting::CompactRows, false, cx))
            .child(self.render_toggle_row(Setting::ShowSender, true, cx))
            .into_any_element()
    }

    fn render_mail(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .child(self.render_page_heading())
            .child(self.render_toggle_row(Setting::OpenInTab, false, cx))
            .child(self.render_toggle_row(Setting::GroupConversations, false, cx))
            .child(self.render_toggle_row(Setting::ShowAttachments, true, cx))
            .into_any_element()
    }

    fn render_account(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .child(self.render_page_heading())
            .child(self.render_note_row(
                "No account connected",
                "The prototype reads its sample messages from memory, so there is no \
                 sign-in to configure yet.",
                false,
            ))
            .child(self.render_value_row("Address", "me@example.com", false))
            .child(self.render_value_row("Storage", "In memory, resets on quit", false))
            .child(self.render_toggle_row(Setting::CheckForMail, false, cx))
            .child(self.render_toggle_row(Setting::ReadReceipts, true, cx))
            .into_any_element()
    }

    fn render_about(&mut self) -> gpui::AnyElement {
        div()
            .child(self.render_page_heading())
            .child(self.render_note_row(
                "Nori",
                "A native mail client prototype built with GPUI. The sample mail, the \
                 settings above, and these facts are all part of the prototype.",
                false,
            ))
            .child(self.render_value_row("Version", env!("CARGO_PKG_VERSION"), false))
            .child(self.render_value_row("Interface", "GPUI (Rust)", false))
            .child(self.render_value_row("Source", "crates/nori-ui, crates/nori-desktop", true))
            .into_any_element()
    }

    // ----- columns --------------------------------------------------------

    fn render_nav(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = self.theme;
        let pages = self.visible_pages(cx);
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
            .child(
                // No back row and no heading: the mail sidebar sits right next
                // to this column and any mailbox is a way out, as is Escape.
                // The field's own placeholder names what the column holds.
                div().px(px(10.)).pt(px(10.)).child(self.search.clone()),
            )
            .child(div().h(px(12.)).flex_none())
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
                    .pb(px(10.))
                    .when(pages.is_empty(), |this| {
                        this.child(
                            div()
                                .px(px(11.))
                                .py(px(8.))
                                .text_size(px(12.5))
                                .text_color(theme.faint)
                                .child("No settings match"),
                        )
                    })
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
        let theme = self.theme;
        let body: gpui::AnyElement = match self.page {
            SettingsPage::General => self.render_general(cx),
            SettingsPage::Appearance => self.render_appearance(cx),
            SettingsPage::Mail => self.render_mail(cx),
            SettingsPage::Account => self.render_account(cx),
            SettingsPage::About => self.render_about(),
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
        let theme = self.theme;
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
