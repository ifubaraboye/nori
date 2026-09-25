use gpui::{App, Global, Hsla, hsla, rgba};

impl Global for Theme {}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub canvas: Hsla,
    pub chrome: Hsla,
    pub surface: Hsla,
    pub raised: Hsla,
    pub overlay: Hsla,
    pub border: Hsla,
    pub strong_border: Hsla,
    pub text: Hsla,
    /// Sits between `text` and `muted`, for prefixes that share a line with
    /// `text` rather than standing alone. A `muted` breadcrumb prefix next to
    /// a `text` subject reads as two different sizes even at an identical
    /// 13px, because brighter text blooms on a dark surface. Closing that
    /// brightness gap keeps the pair optically matched.
    pub text_dim: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub ghost: Hsla,
    pub inset: Hsla,
    pub accent: Hsla,
    pub selected: Hsla,
    /// 6% neutral wash for selected rows/cards on workspace surfaces, in the
    /// spirit of Waku's `sidebar_item_background`. The solid `selected` stays
    /// for the sidebar so its pixels do not change.
    pub selected_layer: Hsla,
    /// Whisper-thin translucent hairlines (white 7% / 14%) for workspace
    /// surfaces. The solid `border`/`strong_border` stay for the sidebar.
    pub hairline: Hsla,
    pub hairline_strong: Hsla,
    pub hover: Hsla,
    /// Lighter tier of `hover`, for rows that sit next to a *solid*
    /// `selected` background: the sidebar and settings nav.
    ///
    /// At `hover`'s 6% these two nearly collide — white 6% over the `#181818`
    /// chrome composites to `#262626`, just 4/255 under the `#2a2a2a` selected
    /// pill, so hovering an unselected row reads as selecting it. Dropping to
    /// ~4% puts hover at `#212121`, roughly a third of the way from chrome to
    /// selected, so the three steps stay ordered and separable.
    pub hover_subtle: Hsla,
    pub active: Hsla,
    pub focus: Hsla,
}

impl Theme {
    /// Publish the theme as application-global state, mirroring Waku's
    /// `theme::init`. Call once, early in startup, before opening windows.
    pub fn init(cx: &mut App) {
        cx.set_global(Theme::dark());
    }

    /// Read the published theme. Falls back to the default dark theme when
    /// `init` has not run (notably in unit tests).
    pub fn current(cx: &App) -> Theme {
        cx.try_global::<Theme>().copied().unwrap_or_default()
    }

    pub fn dark() -> Self {
        Self {
            canvas: rgba(0x1a1a1aff).into(),
            chrome: rgba(0x181818ff).into(),
            surface: rgba(0x212121ff).into(),
            raised: rgba(0x232323ff).into(),
            overlay: rgba(0x00000042).into(),
            border: rgba(0x2d2d2dff).into(),
            strong_border: rgba(0x3b3b3bff).into(),
            text: rgba(0xe2e2e2ff).into(),
            text_dim: rgba(0xc4c4c4ff).into(),
            muted: rgba(0xa3a3a3ff).into(),
            faint: rgba(0x7d7d7dff).into(),
            ghost: rgba(0x575757ff).into(),
            inset: rgba(0x151515ff).into(),
            accent: rgba(0xe2795bff).into(),
            selected: rgba(0x2a2a2aff).into(),
            selected_layer: hsla(0., 0., 0.941, 0.06),
            hairline: hsla(220. / 360., 0.10, 0.90, 0.07),
            hairline_strong: hsla(220. / 360., 0.10, 0.90, 0.14),
            hover: rgba(0xffffff0f).into(),
            hover_subtle: rgba(0xffffff0a).into(),
            active: rgba(0xffffff1a).into(),
            focus: rgba(0xe2795bff).into(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
