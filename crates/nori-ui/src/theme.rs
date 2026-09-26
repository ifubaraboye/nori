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
    /// Foreground for text and glyphs sitting *on* `accent`, for a filled
    /// accent button.
    ///
    /// Not `text` and not `canvas` unconditionally: the accent is a mid-tone in
    /// both palettes, so it needs the darker of the two as its foreground.
    /// Inverting that choice in either theme drops the button label to about
    /// 3.5:1, under the 4.5:1 a 12px label needs.
    pub on_accent: Hsla,
    pub selected: Hsla,
    /// Neutral wash for selected rows/cards on workspace surfaces, in the
    /// spirit of Waku's `sidebar_item_background`. The solid `selected` stays
    /// for the sidebar so its pixels do not change.
    ///
    /// 9% rather than the 6% it started at: at 6% this was byte-identical to
    /// `hover` over the same canvas, so a selected mail row and a merely
    /// hovered one rendered the same shade. It has to clear `hover` to be
    /// seen at all.
    pub selected_layer: Hsla,
    /// Whisper-thin translucent hairlines for workspace surfaces. The solid
    /// `border`/`strong_border` stay for the sidebar.
    pub hairline: Hsla,
    pub hairline_strong: Hsla,
    pub hover: Hsla,
    /// Second tier of `hover`, for rows that sit next to a *solid*
    /// `selected` background: the sidebar and settings nav.
    ///
    /// At `hover`'s strength these two nearly collide — white 6% over the
    /// `#181818` chrome composites to `#262626`, just 4/255 under the `#2a2a2a`
    /// selected pill, so hovering an unselected row reads as selecting it.
    /// The weaker alpha puts hover roughly a third of the way from chrome to
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

    /// The one place a light/dark flag becomes a palette, so nothing else has
    /// to know which setting drives the theme.
    pub fn for_light_mode(light: bool) -> Self {
        if light { Self::light() } else { Self::dark() }
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
            on_accent: rgba(0x1a1a1aff).into(),
            selected: rgba(0x2a2a2aff).into(),
            selected_layer: hsla(0., 0., 0.941, 0.09),
            hairline: hsla(220. / 360., 0.10, 0.90, 0.07),
            hairline_strong: hsla(220. / 360., 0.10, 0.90, 0.14),
            hover: rgba(0xffffff0f).into(),
            hover_subtle: rgba(0xffffff0a).into(),
            active: rgba(0xffffff1a).into(),
            focus: rgba(0xe2795bff).into(),
        }
    }

    /// The light counterpart to `dark`.
    ///
    /// Every wash token is black-based here rather than white, because on a
    /// light surface "more prominent" means darker. That inversion is also why
    /// the alphas are not a mirror of the dark ones: `hover`/`hover_subtle` at
    /// 4%/2% land within one step of each other over `#f2f2f2` chrome, so the
    /// light `selected` is pushed down to `#e4e4e4` to keep a 14-step gap
    /// from the surface and the two hover tiers room to sit inside it.
    pub fn light() -> Self {
        Self {
            canvas: rgba(0xfcfcfcff).into(),
            chrome: rgba(0xf2f2f2ff).into(),
            surface: rgba(0xfdfdfdff).into(),
            raised: rgba(0xffffffff).into(),
            // Still a dark scrim: it dims whatever it sits over, in both modes.
            overlay: rgba(0x00000042).into(),
            border: rgba(0xe3e3e3ff).into(),
            strong_border: rgba(0xd2d2d2ff).into(),
            text: rgba(0x1c1c1cff).into(),
            text_dim: rgba(0x3f3f3fff).into(),
            muted: rgba(0x5e5e5eff).into(),
            faint: rgba(0x8c8c8cff).into(),
            ghost: rgba(0xb8b8b8ff).into(),
            inset: rgba(0xf4f4f4ff).into(),
            // Deeper than the dark palette's accent: the same orange is only
            // ~2.9:1 on white, and this doubles as the focus ring colour.
            accent: rgba(0xcf5f38ff).into(),
            on_accent: rgba(0x1c1c1cff).into(),
            selected: rgba(0xe4e4e4ff).into(),
            selected_layer: hsla(0., 0., 0., 0.09),
            hairline: hsla(220. / 360., 0.10, 0.10, 0.07),
            hairline_strong: hsla(220. / 360., 0.10, 0.10, 0.14),
            hover: rgba(0x0000000a).into(),
            hover_subtle: rgba(0x00000005).into(),
            active: rgba(0x0000000f).into(),
            focus: rgba(0xcf5f38ff).into(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Composite a translucent token over a solid one and return the
    /// perceptual lightness of the result, so the tests below compare what the
    /// user actually sees rather than the token's own `l`. `Rgba` components
    /// are already 0..1, so luma needs no rescaling.
    fn composited_l(wash: Hsla, over: Hsla) -> f32 {
        let base = over.to_rgb();
        let wash = wash.to_rgb();
        let mix = |w: f32, b: f32| w * wash.r + (1. - w) * b;
        let a = wash.a;
        let r = mix(a, base.r);
        let g = mix(a, base.g);
        let b = mix(a, base.b);
        // Rec. 709 luma.
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// The two list surfaces, plus the two thresholds that sit between the
    /// resting row and the selected one.
    fn tiers(theme: Theme, sidebar_surface: Hsla) -> (f32, f32, f32) {
        (
            composited_l(sidebar_surface, sidebar_surface),
            composited_l(theme.hover_subtle, sidebar_surface),
            composited_l(theme.selected, sidebar_surface),
        )
    }

    /// Every palette has to keep three separable steps in the same direction:
    /// resting, hovered, selected. On dark "more prominent" is lighter; on
    /// light it is darker. A palette that gets this backwards, or that spaces
    /// two of the three too tightly, makes hovering a row look like selecting
    /// it — which is the exact bug `hover_subtle` exists to prevent.
    #[test]
    fn the_three_sidebar_tiers_stay_ordered_in_both_palettes() {
        for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
            let light = name == "light";
            let (resting, hovered, selected) = tiers(theme, theme.chrome);
            if light {
                assert!(
                    resting > hovered && hovered > selected,
                    "{name}: expected resting({resting:.3}) > hovered({hovered:.3}) > \
                     selected({selected:.3})"
                );
            } else {
                assert!(
                    resting < hovered && hovered < selected,
                    "{name}: expected resting({resting:.3}) < hovered({hovered:.3}) < \
                     selected({selected:.3})"
                );
            }
            // A step too small to see is the same as no step at all.
            let gap = (resting - hovered).abs();
            assert!(
                gap > 0.01,
                "{name}: resting and hovered are only {gap:.4} apart, too close to read"
            );
            let gap = (hovered - selected).abs();
            assert!(
                gap > 0.01,
                "{name}: hovered and selected are only {gap:.4} apart, too close to read"
            );
        }
    }

    /// The mail list selects with `selected_layer` rather than the solid
    /// `selected`, so this tier needs its own check. At 6% it was byte
    /// identical to `hover` over the same canvas, which is why it is 9% now.
    #[test]
    fn the_mail_list_tiers_stay_separate_in_both_palettes() {
        for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
            let light = name == "light";
            let resting = composited_l(theme.canvas, theme.canvas);
            let hovered = composited_l(theme.hover, theme.canvas);
            let selected = composited_l(theme.selected_layer, theme.canvas);
            if light {
                assert!(
                    resting > hovered && hovered > selected,
                    "{name}: expected resting({resting:.3}) > hovered({hovered:.3}) > \
                     selected({selected:.3})"
                );
            } else {
                assert!(
                    resting < hovered && hovered < selected,
                    "{name}: expected resting({resting:.3}) < hovered({hovered:.3}) < \
                     selected({selected:.3})"
                );
            }
            assert!(
                (hovered - selected).abs() > 0.01,
                "{name}: a hovered row and a selected row are only {:.4} apart",
                (hovered - selected).abs()
            );
        }
    }

    #[test]
    fn the_light_flag_picks_the_palette() {
        // The one mapping from setting to palette. If this ever inverts, the
        // switch would show the wrong state, so it is worth pinning — the
        // palettes themselves are covered by the tier tests above.
        assert_eq!(
            Theme::for_light_mode(false).canvas.to_rgb(),
            Theme::dark().canvas.to_rgb()
        );
        assert_eq!(
            Theme::for_light_mode(true).canvas.to_rgb(),
            Theme::light().canvas.to_rgb()
        );
    }
}
