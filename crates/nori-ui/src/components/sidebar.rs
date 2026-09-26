use std::rc::Rc;

use gpui::{
    AnimationExt as _, App, CursorStyle, Entity, FocusHandle, IntoElement, MouseButton, RenderOnce,
    Role, SpringAnimation, SpringConfig, Window, div, prelude::*, px, transparent_black,
};

use super::{Button, ButtonStyle, Icon, SidebarToggle, TextField};
use crate::model::{Label, LabelId, Mailbox};
use crate::theme::Theme;

pub const SIDEBAR_DEFAULT_WIDTH: f32 = 252.;
pub const SIDEBAR_MIN_WIDTH: f32 = 180.;
pub const SIDEBAR_MAX_WIDTH: f32 = 420.;

pub fn clamp_sidebar_width(width: f32) -> f32 {
    width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH)
}

type MailboxHandler = Rc<dyn Fn(Mailbox, &mut Window, &mut App) + 'static>;
type SidebarActionHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;
type ResizeStartHandler = Rc<dyn Fn(f32, &mut Window, &mut App) + 'static>;
type ResizeStepHandler = Rc<dyn Fn(f32, &mut Window, &mut App) + 'static>;
type LabelHandler = Rc<dyn Fn(Option<LabelId>, &mut Window, &mut App) + 'static>;
type LabelIdHandler = Rc<dyn Fn(LabelId, &mut Window, &mut App) + 'static>;
type CreateLabelHandler = Rc<dyn Fn(String, &mut Window, &mut App) + 'static>;
type RenameLabelHandler = Rc<dyn Fn(LabelId, String, &mut Window, &mut App) + 'static>;

/// What the labels section can ask the app to do.
///
/// The section is `RenderOnce`, so it holds no state of its own: collapsing
/// the group, opening the composer and closing it are all decisions the app
/// owns. They arrive through one handler rather than three so `Sidebar::new`
/// does not grow three more positional parameters on top of the two dozen it
/// already has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelsAction {
    ToggleCollapsed,
    /// Opens the composer, or closes it if it is already showing. The `+` is
    /// the only pointer affordance for dismissing the composer — this gpui
    /// version has no `on_blur`, so there is nothing to click-away into — so
    /// pressing it twice has to be a way back out.
    ToggleComposer,
    CloseComposer,
}

type LabelsHandler = Rc<dyn Fn(LabelsAction, &mut Window, &mut App) + 'static>;

/// A collapsible group header: a title with a chevron, plus an optional
/// trailing button on the far right.
///
/// Shared by the mailboxes and labels sections. The two headers look and
/// behave identically today, and the labels one additionally has to leave room
/// for a `+`, so they are written once here rather than kept in step by hand.
///
/// The trailing button is a *sibling* of the toggle inside the row, never a
/// child of it: the toggle is a `role="button"`, and a button nested inside
/// one is both a hit-testing problem and an accessibility one.
fn group_header(
    toggle_id: &'static str,
    title: &'static str,
    collapsed: bool,
    theme: Theme,
    on_toggle: SidebarActionHandler,
    trailing: Option<gpui::AnyElement>,
) -> impl IntoElement {
    let on_toggle_key = on_toggle.clone();
    div().px(px(10.)).child(
        div()
            .h(px(28.))
            .flex()
            .items_center()
            .justify_between()
            .px(px(8.))
            .child(
                div()
                    .id(toggle_id)
                    .debug_selector(move || toggle_id.to_string())
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .h(px(22.))
                    .px(px(4.))
                    .text_size(px(12.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.faint)
                    .cursor_pointer()
                    .role(Role::Button)
                    .aria_label(format!("{title} group"))
                    .focusable()
                    .tab_stop(true)
                    .focus_visible(|style| style.border_color(theme.focus))
                    .hover(|style| style.text_color(theme.text))
                    .on_click({
                        let on_toggle = on_toggle.clone();
                        move |_event, window, cx| on_toggle(window, cx)
                    })
                    .on_key_down(move |event, window, cx| {
                        let key = event.keystroke.key.as_str();
                        if (key == "left" && !collapsed) || (key == "right" && collapsed) {
                            on_toggle_key(window, cx);
                        }
                    })
                    .child(title)
                    .child(Icon::new(
                        if collapsed {
                            "icons/chevron-right.svg"
                        } else {
                            "icons/chevron-down.svg"
                        },
                        12.,
                        theme.faint,
                    )),
            )
            .when_some(trailing, |this, trailing| this.child(trailing)),
    )
}

/// The sidebar's Labels section: one selectable row per label, each with a
/// colour dot, and a delete affordance that only appears on hover.
#[allow(clippy::too_many_arguments)]
fn labels_section(
    labels: &[Label],
    selected: Option<LabelId>,
    theme: Theme,
    on_select: &LabelHandler,
    on_delete: &LabelIdHandler,
    on_rename: &RenameLabelHandler,
) -> impl IntoElement {
    div()
        .id("sidebar-labels")
        .debug_selector(|| "sidebar-labels".into())
        .flex_none()
        .children(labels.iter().map(|label| {
            let id = label.id;
            let is_selected = selected == Some(id);
            let on_select_row = on_select.clone();
            let on_delete_row = on_delete.clone();
            let on_rename_row = on_rename.clone();
            div().px(px(10.)).pb(px(1.)).child(
                div()
                    .id(("sidebar-label", id as usize))
                    .group("sidebar-label")
                    .h(px(30.))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(8.))
                    .cursor_pointer()
                    .role(Role::Button)
                    .aria_label(format!("Label {}", label.name))
                    .aria_selected(is_selected)
                    .focusable()
                    .tab_stop(true)
                    .focus_visible(|style| style.border_color(theme.focus))
                    .bg(if is_selected {
                        theme.selected
                    } else {
                        transparent_black()
                    })
                    .hover(|style| {
                        style.bg(if is_selected {
                            theme.selected
                        } else {
                            theme.hover_subtle
                        })
                    })
                    .on_click(move |_event, window, cx| {
                        on_select_row(if is_selected { None } else { Some(id) }, window, cx);
                    })
                    .child(div().size(px(10.)).flex_none().child(label_swatch(label)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .text_color(if is_selected { theme.text } else { theme.muted })
                            .truncate()
                            .child(label.name.clone()),
                    )
                    .child(
                        div()
                            .id(("sidebar-label-rename", id as usize))
                            .size(px(18.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .role(Role::Button)
                            .aria_label(format!("Rename label {}", label.name))
                            .opacity(0.)
                            .group_hover("sidebar-label", |el| el.opacity(1.))
                            .hover(|el| el.bg(theme.hover))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click({
                                let name = label.name.clone();
                                move |_event, window, cx| {
                                    cx.stop_propagation();
                                    on_rename_row(id, name.clone(), window, cx);
                                }
                            })
                            .child(Icon::new("icons/pencil.svg", 10., theme.faint)),
                    )
                    .child(
                        div()
                            .id(("sidebar-label-delete", id as usize))
                            .size(px(18.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .role(Role::Button)
                            .aria_label(format!("Delete label {}", label.name))
                            .opacity(0.)
                            .group_hover("sidebar-label", |el| el.opacity(1.))
                            .hover(|el| el.bg(theme.hover))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_event, window, cx| {
                                cx.stop_propagation();
                                on_delete_row(id, window, cx);
                            })
                            .child(Icon::new("icons/close.svg", 10., theme.faint)),
                    ),
            )
        }))
}

/// A label's colour as a filled dot, matching the row chips.
fn label_swatch(label: &Label) -> impl IntoElement {
    div().size_full().bg(label.chip().0)
}

/// The inline "new label" composer, opened by the header's `+`.
///
/// Typing goes through the shared `TextField`; Enter hands the name to
/// `on_create`, which is where the duplicate and blank rules in
/// `LabelStore::create` decide whether it sticks. Escape backs out without
/// creating anything, and stops there: the app-wide `Dismiss` would otherwise
/// see the same keystroke and close whatever else is open.
fn label_composer(
    field: Entity<TextField>,
    on_create: &CreateLabelHandler,
    on_close: &LabelsHandler,
) -> impl IntoElement {
    let on_submit_key = on_create.clone();
    let on_cancel_key = on_close.clone();
    let field_for_key = field.clone();
    div().px(px(10.)).pt(px(4.)).child(
        div()
            .id("sidebar-label-new")
            .debug_selector(|| "sidebar-label-new".into())
            .h(px(30.))
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .on_key_down(move |event, window, cx| {
                let key = event.keystroke.key.as_str();
                if key == "escape" {
                    cx.stop_propagation();
                    on_cancel_key(LabelsAction::CloseComposer, window, cx);
                    return;
                }
                if key != "enter" {
                    return;
                }
                let name = field_for_key.read(cx).content().trim().to_string();
                if !name.is_empty() {
                    on_submit_key(name, window, cx);
                }
            })
            .child(field),
    )
}

/// The `+` that opens the label composer, shown at the trailing edge of the
/// Labels header. Focusable in its own right so the keyboard can reach the
/// composer without a mouse, and so focus has somewhere to return to when the
/// composer closes.
fn label_add_button(focus: FocusHandle, theme: Theme, on_open: &LabelsHandler) -> impl IntoElement {
    let on_click = on_open.clone();
    let on_key = on_open.clone();
    div()
        .id("sidebar-labels-add")
        .debug_selector(|| "sidebar-labels-add".into())
        .size(px(22.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .role(Role::Button)
        .aria_label("New label")
        .track_focus(&focus)
        .focusable()
        .tab_stop(true)
        .focus_visible(|style| style.border_color(theme.focus))
        .hover(|style| style.bg(theme.hover_subtle))
        .on_click(move |_event, window, cx| on_click(LabelsAction::ToggleComposer, window, cx))
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | " ") {
                cx.stop_propagation();
                on_key(LabelsAction::ToggleComposer, window, cx);
            }
        })
        .child(Icon::new("icons/plus.svg", 12., theme.faint))
}

#[derive(IntoElement)]
pub struct Sidebar {
    selected: Mailbox,
    counts: [usize; 6],
    width: f32,
    visible: bool,
    mailboxes_collapsed: bool,
    on_mailbox: MailboxHandler,
    on_search: SidebarActionHandler,
    on_compose: SidebarActionHandler,
    on_settings: SidebarActionHandler,
    on_toggle: SidebarActionHandler,
    on_back: SidebarActionHandler,
    on_forward: SidebarActionHandler,
    can_back: bool,
    can_forward: bool,
    on_toggle_group: SidebarActionHandler,
    labels: Vec<Label>,
    selected_label: Option<LabelId>,
    labels_collapsed: bool,
    label_composer_open: bool,
    new_label_field: Entity<TextField>,
    new_label_focus: FocusHandle,
    on_labels_action: LabelsHandler,
    on_select_label: LabelHandler,
    on_create_label: CreateLabelHandler,
    on_delete_label: LabelIdHandler,
    on_rename_label: RenameLabelHandler,
    on_begin_resize: ResizeStartHandler,
    on_resize_step: ResizeStepHandler,
}

impl Sidebar {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        selected: Mailbox,
        counts: [usize; 6],
        width: f32,
        visible: bool,
        mailboxes_collapsed: bool,
        on_mailbox: impl Fn(Mailbox, &mut Window, &mut App) + 'static,
        on_search: impl Fn(&mut Window, &mut App) + 'static,
        on_compose: impl Fn(&mut Window, &mut App) + 'static,
        on_settings: impl Fn(&mut Window, &mut App) + 'static,
        on_toggle: impl Fn(&mut Window, &mut App) + 'static,
        on_back: impl Fn(&mut Window, &mut App) + 'static,
        on_forward: impl Fn(&mut Window, &mut App) + 'static,
        can_back: bool,
        can_forward: bool,
        on_toggle_group: impl Fn(&mut Window, &mut App) + 'static,
        labels: Vec<Label>,
        selected_label: Option<LabelId>,
        labels_collapsed: bool,
        label_composer_open: bool,
        new_label_field: Entity<TextField>,
        new_label_focus: FocusHandle,
        on_labels_action: impl Fn(LabelsAction, &mut Window, &mut App) + 'static,
        on_select_label: impl Fn(Option<LabelId>, &mut Window, &mut App) + 'static,
        on_create_label: impl Fn(String, &mut Window, &mut App) + 'static,
        on_delete_label: impl Fn(LabelId, &mut Window, &mut App) + 'static,
        on_rename_label: impl Fn(LabelId, String, &mut Window, &mut App) + 'static,
        on_begin_resize: impl Fn(f32, &mut Window, &mut App) + 'static,
        on_resize_step: impl Fn(f32, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            selected,
            counts,
            width,
            visible,
            mailboxes_collapsed,
            on_mailbox: Rc::new(on_mailbox),
            on_search: Rc::new(on_search),
            on_compose: Rc::new(on_compose),
            on_settings: Rc::new(on_settings),
            on_toggle: Rc::new(on_toggle),
            on_back: Rc::new(on_back),
            on_forward: Rc::new(on_forward),
            can_back,
            can_forward,
            on_toggle_group: Rc::new(on_toggle_group),
            labels,
            selected_label,
            labels_collapsed,
            label_composer_open,
            new_label_field,
            new_label_focus,
            on_labels_action: Rc::new(on_labels_action),
            on_select_label: Rc::new(on_select_label),
            on_create_label: Rc::new(on_create_label),
            on_delete_label: Rc::new(on_delete_label),
            on_rename_label: Rc::new(on_rename_label),
            on_begin_resize: Rc::new(on_begin_resize),
            on_resize_step: Rc::new(on_resize_step),
        }
    }
}

impl RenderOnce for Sidebar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::current(cx);
        let width = clamp_sidebar_width(self.width);
        let target = if self.visible { width } else { 0. };
        let spring = SpringAnimation::new(SpringConfig::new(210., 29., 1.))
            .to(px(target))
            .with_epsilon(0.5);

        let on_compose = self.on_compose.clone();
        let on_settings = self.on_settings.clone();
        let on_toggle = self.on_toggle.clone();
        let on_back = self.on_back.clone();
        let on_forward = self.on_forward.clone();
        let (can_back, can_forward) = (self.can_back, self.can_forward);
        let header = div()
            .h(px(48.))
            .flex_none()
            .flex()
            .items_center()
            .px(px(10.))
            .child(SidebarToggle::new("sidebar-toggle", move |window, cx| {
                on_toggle(window, cx);
            }))
            // Waku-style history arrows, grouped beside the sidebar toggle.
            .child(
                div()
                    .id("sidebar-back")
                    .debug_selector(|| "sidebar-back".into())
                    .ml(px(6.))
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor(if can_back {
                        gpui::CursorStyle::PointingHand
                    } else {
                        gpui::CursorStyle::Arrow
                    })
                    .when(can_back, |this| {
                        this.hover(|style| style.bg(theme.hover))
                            .on_click(move |_event, window, cx| on_back(window, cx))
                    })
                    .when(!can_back, |this| this.opacity(0.35))
                    .child(Icon::new("icons/arrow-left.svg", 14., theme.muted)),
            )
            .child(
                div()
                    .id("sidebar-forward")
                    .debug_selector(|| "sidebar-forward".into())
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor(if can_forward {
                        gpui::CursorStyle::PointingHand
                    } else {
                        gpui::CursorStyle::Arrow
                    })
                    .when(can_forward, |this| {
                        this.hover(|style| style.bg(theme.hover))
                            .on_click(move |_event, window, cx| on_forward(window, cx))
                    })
                    .when(!can_forward, |this| this.opacity(0.35))
                    .child(Icon::new("icons/arrow-right.svg", 14., theme.muted)),
            )
            .child(div().flex_1());

        let compose = div().px(px(10.)).pt(px(10.)).pb(px(1.)).child(
            div()
                .id("sidebar-compose")
                .h(px(32.))
                .w_full()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(8.))
                .cursor_pointer()
                .role(Role::Button)
                .aria_label("Compose")
                .focusable()
                .tab_stop(true)
                .focus_visible(|style| style.border_color(theme.focus))
                .hover(|style| style.bg(theme.hover_subtle))
                .text_color(theme.muted)
                .on_click(move |_event, window, cx| on_compose(window, cx))
                .child(
                    div()
                        .size(px(20.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new("icons/compose.svg", 16., theme.muted)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.))
                        .truncate()
                        .child("Compose"),
                ),
        );

        let on_search = self.on_search.clone();
        let on_toggle_group = self.on_toggle_group.clone();
        let on_mailbox = self.on_mailbox.clone();
        let selected = self.selected;
        let counts = self.counts;
        let collapsed = self.mailboxes_collapsed;
        let nav = div()
            .id("mail-sidebar-nav")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div().px(px(10.)).pb(px(1.)).child(
                    div()
                        .id("sidebar-search")
                        .h(px(32.))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(8.))
                        .cursor_pointer()
                        .role(Role::Button)
                        .aria_label("Search")
                        .focusable()
                        .tab_stop(true)
                        .focus_visible(|style| style.border_color(theme.focus))
                        .hover(|style| style.bg(theme.hover_subtle))
                        .text_color(theme.muted)
                        .on_click(move |_event, window, cx| on_search(window, cx))
                        .child(
                            div()
                                .size(px(20.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new("icons/search.svg", 16., theme.muted)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.))
                                .truncate()
                                .child("Search"),
                        ),
                ),
            )
            .child(div().h(px(10.)).flex_none())
            .child(group_header(
                "sidebar-mailboxes-toggle",
                "Mailboxes",
                collapsed,
                theme,
                on_toggle_group.clone(),
                None,
            ))
            .when(!collapsed, |this| {
                this.children(counts.into_iter().enumerate().map(|(index, count)| {
                    let mailbox = Mailbox::NAV_ITEMS[index];
                    let is_selected = selected == mailbox;
                    let icon = match mailbox {
                        Mailbox::Inbox => "icons/inbox.svg",
                        Mailbox::Starred => "icons/star.svg",
                        Mailbox::Sent => "icons/sent.svg",
                        Mailbox::Drafts => "icons/drafts.svg",
                        Mailbox::Archive => "icons/archive.svg",
                        Mailbox::Trash => "icons/trash.svg",
                    };
                    let on_mailbox = on_mailbox.clone();
                    div().px(px(10.)).pb(px(1.)).child(
                        div()
                            .id(("sidebar-mailbox", index))
                            .debug_selector(move || format!("sidebar-mailbox-{index}"))
                            .h(px(32.))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .px(px(8.))
                            .cursor_pointer()
                            .role(Role::Button)
                            .aria_label(format!("{} ({} messages)", mailbox.label(), count))
                            .aria_selected(is_selected)
                            .focusable()
                            .tab_stop(true)
                            .focus_visible(|style| style.border_color(theme.focus))
                            .bg(if is_selected {
                                theme.selected
                            } else {
                                transparent_black()
                            })
                            .hover(|style| {
                                style.bg(if is_selected {
                                    theme.selected
                                } else {
                                    theme.hover_subtle
                                })
                            })
                            .text_color(if is_selected { theme.text } else { theme.muted })
                            .on_click(move |_event, window, cx| {
                                on_mailbox(mailbox, window, cx);
                            })
                            .child(
                                div()
                                    .size(px(20.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::new(
                                        icon,
                                        16.,
                                        if is_selected { theme.text } else { theme.muted },
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(13.))
                                    .truncate()
                                    .child(mailbox.label()),
                            )
                            .when(count > 0, |this| {
                                this.child(
                                    div()
                                        .flex_none()
                                        .text_size(px(11.5))
                                        .text_color(theme.faint)
                                        .child(count.to_string()),
                                )
                            }),
                    )
                }))
            })
            // A group's rows sit flush under their own header and the header
            // is preceded by a spacer, so each group reads as one block. The
            // Labels group is separated from the mailboxes the same way the
            // mailboxes are separated from Search.
            .child(div().h(px(10.)).flex_none())
            // Labels: a collapsible group, mirroring Mailboxes, with the
            // composer opened from the header's `+` rather than living in the
            // list permanently.
            .child(group_header(
                "sidebar-labels-toggle",
                "Labels",
                self.labels_collapsed,
                theme,
                // The Labels group collapses independently of Mailboxes, so
                // this goes through the labels dispatcher rather than reusing
                // the mailboxes group toggle.
                Rc::new({
                    let on_labels_action = self.on_labels_action.clone();
                    move |window: &mut Window, cx: &mut App| {
                        on_labels_action(LabelsAction::ToggleCollapsed, window, cx);
                    }
                }),
                Some(
                    label_add_button(self.new_label_focus.clone(), theme, &self.on_labels_action)
                        .into_any_element(),
                ),
            ))
            .when(!self.labels_collapsed, |this| {
                this.child(labels_section(
                    &self.labels,
                    self.selected_label,
                    theme,
                    &self.on_select_label,
                    &self.on_delete_label,
                    &self.on_rename_label,
                ))
                .when(self.label_composer_open, |this| {
                    this.child(label_composer(
                        self.new_label_field.clone(),
                        &self.on_create_label,
                        &self.on_labels_action,
                    ))
                })
            });

        let footer = div()
            .h(px(40.))
            .flex_none()
            .flex()
            .items_center()
            .px(px(10.))
            .child(
                Button::new("sidebar-settings", "")
                    .dense()
                    .icon(Icon::new("icons/gear.svg", 14., theme.faint))
                    .style(ButtonStyle::Ghost)
                    .without_focus_ring()
                    .without_hover_fill()
                    .aria_label("Settings")
                    .on_click(move |_event, window, cx| on_settings(window, cx)),
            );

        let on_begin_resize = self.on_begin_resize.clone();
        let on_resize_step = self.on_resize_step.clone();
        let resize_width = width;
        let resize_handle = div()
            .id("mail-sidebar-resize")
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(0.))
            .w(px(9.))
            .cursor(CursorStyle::ResizeLeftRight)
            .role(Role::Splitter)
            .aria_label("Resize sidebar")
            .focusable()
            .tab_stop(true)
            .focus_visible(|style| style.border_color(theme.focus))
            .group("sidebar-resize")
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                cx.stop_propagation();
                on_begin_resize(event.position.x.as_f32(), window, cx);
            })
            .on_key_down(move |event, window, cx| {
                let step = if event.keystroke.modifiers.shift {
                    20.
                } else {
                    8.
                };
                match event.keystroke.key.as_str() {
                    "left" => on_resize_step(-step, window, cx),
                    "right" => on_resize_step(step, window, cx),
                    "home" => on_resize_step(SIDEBAR_MIN_WIDTH - resize_width, window, cx),
                    "end" => on_resize_step(SIDEBAR_MAX_WIDTH - resize_width, window, cx),
                    _ => {}
                }
            });

        let content = div()
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .child(header)
            .child(compose)
            .child(nav)
            .child(footer);

        div()
            .id("mail-sidebar-shell")
            .relative()
            .h_full()
            .flex_none()
            .overflow_hidden()
            .bg(theme.chrome)
            .border_r_1()
            .border_color(if self.visible {
                theme.border
            } else {
                transparent_black()
            })
            .child(content)
            .when(self.visible, |this| this.child(resize_handle))
            .with_spring("mail-sidebar-width", spring, |this, value| this.w(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_width_is_clamped_to_waku_range() {
        assert_eq!(clamp_sidebar_width(0.), SIDEBAR_MIN_WIDTH);
        assert_eq!(clamp_sidebar_width(10_000.), SIDEBAR_MAX_WIDTH);
        assert_eq!(clamp_sidebar_width(252.), 252.);
    }
}
