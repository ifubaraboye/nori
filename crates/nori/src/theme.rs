//! The waku palette, translated from its design tokens.
//!
//! waku is a design kit, not a Rust library, so the surfaces, borders and
//! type ramp are reproduced here by hand. Colour is used for meaning only:
//! graphite carries the chrome, the warm accent marks the one action on a
//! screen, and green/amber/red describe a download's state.

use gpui::{Hsla, WindowAppearance, hsla, rgb};
use nori_core::config::ThemePreference;

/// One resolved palette. Cheap to copy, so views can take it by value.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub is_dark: bool,

    pub canvas: Hsla,
    pub sidebar: Hsla,
    pub raised: Hsla,
    pub inset: Hsla,
    pub composer: Hsla,
    pub overlay: Hsla,
    pub overlay_strong: Hsla,

    pub border: Hsla,
    pub border_strong: Hsla,
    pub sidebar_border: Hsla,

    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_tertiary: Hsla,
    pub text_ghost: Hsla,

    pub accent: Hsla,
    pub selection: Hsla,
    pub code_text: Hsla,
    pub inverse: Hsla,
    pub on_inverse: Hsla,

    pub warning: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
}

impl Theme {
    pub fn resolve(preference: ThemePreference, appearance: WindowAppearance) -> Self {
        let dark = match preference {
            ThemePreference::System => matches!(
                appearance,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            ThemePreference::Light => false,
            ThemePreference::Dark => true,
        };
        if dark { Self::dark() } else { Self::light() }
    }

    pub fn dark() -> Self {
        Self {
            is_dark: true,
            canvas: rgb(0x1A1A1A).into(),
            sidebar: rgb(0x181818).into(),
            raised: rgb(0x232323).into(),
            inset: rgb(0x151515).into(),
            composer: rgb(0x212121).into(),
            overlay: hsla(220.0 / 360.0, 0.10, 0.90, 0.05),
            overlay_strong: hsla(220.0 / 360.0, 0.10, 0.90, 0.09),

            border: hsla(220.0 / 360.0, 0.10, 0.90, 0.07),
            border_strong: hsla(220.0 / 360.0, 0.10, 0.90, 0.14),
            sidebar_border: hsla(126.93 / 360.0, 0.000_000_1, 0.16077, 1.0),

            text: rgb(0xE2E2E2).into(),
            text_secondary: rgb(0xA3A3A3).into(),
            text_tertiary: rgb(0x7D7D7D).into(),
            text_ghost: rgb(0x575757).into(),

            accent: rgb(0xE2795B).into(),
            selection: hsla(211.0 / 360.0, 1.0, 0.50, 0.55),
            code_text: rgb(0xE0A882).into(),
            inverse: rgb(0xE7E9EC).into(),
            on_inverse: rgb(0x17181C).into(),

            warning: rgb(0xE0B36A).into(),
            success: rgb(0x62C987).into(),
            danger: rgb(0xE2726A).into(),
            danger_soft: hsla(4.0 / 360.0, 0.55, 0.63, 0.10),
        }
    }

    pub fn light() -> Self {
        Self {
            is_dark: false,
            canvas: rgb(0xF6F5F6).into(),
            sidebar: rgb(0xF3F3F3).into(),
            raised: rgb(0xECECEC).into(),
            inset: rgb(0xE6E6E6).into(),
            composer: rgb(0xFFFFFF).into(),
            overlay: hsla(220.0 / 360.0, 0.10, 0.12, 0.05),
            overlay_strong: hsla(220.0 / 360.0, 0.10, 0.12, 0.09),

            border: hsla(220.0 / 360.0, 0.10, 0.12, 0.08),
            border_strong: hsla(220.0 / 360.0, 0.10, 0.12, 0.15),
            sidebar_border: hsla(0.0, 0.0, 0.078, 0.12),

            text: rgb(0x242424).into(),
            text_secondary: rgb(0x666666).into(),
            text_tertiary: rgb(0x858585).into(),
            text_ghost: rgb(0xA4A4A4).into(),

            accent: rgb(0xC85F44).into(),
            selection: hsla(211.0 / 360.0, 1.0, 0.50, 0.35),
            code_text: rgb(0x9A5528).into(),
            inverse: rgb(0x202227).into(),
            on_inverse: rgb(0xF8F8F9).into(),

            warning: rgb(0xA66B20).into(),
            success: rgb(0x2F8F52).into(),
            danger: rgb(0xC64A42).into(),
            danger_soft: hsla(4.0 / 360.0, 0.55, 0.52, 0.10),
        }
    }

    /// The persistent hover wash for rows.
    pub fn row_hover(self) -> Hsla {
        self.overlay_strong
    }

    pub fn row_selected(self) -> Hsla {
        if self.is_dark {
            rgb(0x202020).into()
        } else {
            rgb(0xEEEDEE).into()
        }
    }

    pub fn control(self) -> Hsla {
        self.inset
    }
}

/// The short label a source wears on a result row.
pub fn source_tag(id: &str) -> &'static str {
    nori_core::search::model::source_tag(id)
}

/// Distinct, muted tints for the source badges. They sit far from the accent
/// so a colourful list still keeps the accent as the one loud thing.
pub fn source_tint(id: &str, theme: &Theme) -> Hsla {
    match id {
        "fitgirl" => rgb(0xA78BFA).into(),
        "yts" => rgb(0x86D6A2).into(),
        "eztv" => rgb(0xF0C560).into(),
        "nyaa" => rgb(0xC4B5FD).into(),
        "subsplease" => rgb(0xB9A7E6).into(),
        "tpb-movies" | "tpb-tv" => rgb(0x5FD0C5).into(),
        "x1337-movies" | "x1337-tv" => rgb(0xF6A55C).into(),
        "bittorrented" => rgb(0x7DB8F0).into(),
        _ => theme.text_tertiary,
    }
}
