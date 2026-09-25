/// How tall a mail list row is, and how much of the mail it shows.
///
/// The row heights live here rather than in `EmailRow` so the comfortable and
/// compact layouts cannot drift apart, and so a third density is one enum
/// variant rather than another pile of literals in the row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Density {
    /// Three lines: sender and time, subject, preview. Today's default.
    #[default]
    Comfortable,
    /// One line: sender, label chip, subject, preview. Fits roughly twice as
    /// many rows in the same space.
    Compact,
}

impl Density {
    pub fn from_compact_rows(compact_rows: bool) -> Self {
        if compact_rows {
            Self::Compact
        } else {
            Self::Comfortable
        }
    }

    pub fn row_height(self) -> f32 {
        match self {
            Self::Comfortable => 70.,
            Self::Compact => 40.,
        }
    }

    /// Width of the sender column in the compact layout, where the row is a
    /// single line and the sender needs a fixed edge to line up against.
    pub fn sender_column_width(self) -> Option<f32> {
        match self {
            Self::Comfortable => None,
            Self::Compact => Some(200.),
        }
    }

    pub fn shows_preview(self) -> bool {
        // Comfortable gives the preview its own line; compact trails it
        // behind the subject, where it is the first thing to truncate away.
        matches!(self, Self::Comfortable | Self::Compact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_rows_setting_picks_the_density() {
        assert_eq!(Density::from_compact_rows(false), Density::Comfortable);
        assert_eq!(Density::from_compact_rows(true), Density::Compact);
    }

    #[test]
    fn compact_rows_are_much_shorter() {
        assert_eq!(Density::Comfortable.row_height(), 70.);
        assert_eq!(Density::Compact.row_height(), 40.);
        assert!(Density::Compact.row_height() < Density::Comfortable.row_height());
    }

    #[test]
    fn only_the_single_line_layout_fixes_the_sender_column() {
        assert_eq!(Density::Comfortable.sender_column_width(), None);
        assert_eq!(Density::Compact.sender_column_width(), Some(200.));
    }
}
