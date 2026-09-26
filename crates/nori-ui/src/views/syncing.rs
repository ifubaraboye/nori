//! The counter shown while mail is arriving.
//!
//! There is no skeleton here, and that is the point. The list fills with real
//! messages as each one comes back from Gmail, so the only thing left to say is
//! how far along it is — and that belongs at the foot of the list, out of the
//! way, rather than across the top where a loading bar competes with the
//! folder name for the same edge.
//!
//! An earlier version drew a progress line under the top bar and twelve
//! placeholder rows. Both were wrong: the line read as part of the header, and
//! thirty seconds of invented content is worse than an empty list, because it
//! promises a shape the real mail will not keep.

use gpui::{SharedString, div, prelude::*, px};

use crate::theme::Theme;

/// A quiet line at the foot of the list: "34 of 80", or nothing before the
/// total is known.
pub fn counter(theme: Theme, label: Option<String>) -> gpui::AnyElement {
    let mut line = div().flex_none().px(px(16.)).py(px(9.));
    if let Some(label) = label {
        line = line.child(
            div()
                .text_size(px(11.))
                .text_color(theme.faint)
                .child(SharedString::from(label)),
        );
    }
    line.into_any_element()
}
