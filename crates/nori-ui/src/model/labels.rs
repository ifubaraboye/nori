//! User-defined labels.
//!
//! Labels are separate from mailboxes: a mail lives in exactly one mailbox but
//! can carry any number of labels. Colours are chosen by the user and stored
//! with the label, so a rename never changes a chip's colour and two labels
//! never collapse onto one.

use gpui::Hsla;

use crate::model::EmailId;

pub type LabelId = u32;

/// The fixed palette a new label draws its colour from, as packed 0xRRGGBB.
/// Chosen so any of these stays legible as solid text over a low-alpha wash on
/// the dark canvas, which is how the list renders a chip.
pub const LABEL_COLOURS: [u32; 8] = [
    0xE2_65_8A, // rose
    0xE0_A8_5B, // amber
    0x62_C9_87, // green
    0x5B_C7_B5, // teal
    0x62_A8_E2, // blue
    0xA0_8C_E0, // violet
    0xD0_7A_C8, // magenta
    0x9A_A5_B5, // slate
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub id: LabelId,
    pub name: String,
    /// Packed 0xRRGGBB, drawn from [`LABEL_COLOURS`] at creation.
    pub colour: u32,
}

impl Label {
    /// The chip style: solid text over a translucent wash of the same hue.
    /// The hue comes from the green channel because every palette entry is
    /// already a plausible hue, which keeps the store free of colour maths.
    pub fn chip(&self) -> (Hsla, Hsla, Hsla) {
        let base = Hsla {
            h: f32::from(((self.colour >> 8) & 0xff) as u8) / 255.,
            s: 0.55,
            l: 0.70,
            a: 1.,
        };
        let washed = |a: f32| Hsla {
            h: base.h,
            s: base.s,
            l: base.l,
            a,
        };
        (base, washed(0.45), washed(0.16))
    }
}

/// The label table plus the assignment of labels to mails.
///
/// Kept beside [`MailStore`](crate::model::MailStore) rather than inside it so
/// label identity is independent of mailbox state: switching mailboxes or
/// moving a mail to Trash never disturbs a label.
#[derive(Debug)]
pub struct LabelStore {
    labels: Vec<Label>,
    next_id: LabelId,
    /// Mail id -> label ids, kept sorted by label id so rendering is stable.
    assignments: Vec<(EmailId, Vec<LabelId>)>,
}

impl Default for LabelStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LabelStore {
    pub fn new() -> Self {
        Self {
            labels: Vec::new(),
            next_id: 1,
            assignments: Vec::new(),
        }
    }

    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    pub fn label(&self, id: LabelId) -> Option<&Label> {
        self.labels.iter().find(|label| label.id == id)
    }

    /// Create a label. Names are unique and case-insensitively matched, since
    /// two labels differing only in case would be indistinguishable in the
    /// sidebar. Returns the new label, or `None` if the name is blank or taken.
    pub fn create(&mut self, name: &str) -> Option<Label> {
        let name = name.trim();
        if name.is_empty() || self.name_taken(name) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        // Spread new labels around the palette by id so consecutive creations
        // do not come out the same colour.
        let colour = LABEL_COLOURS[(id as usize - 1) % LABEL_COLOURS.len()];
        let label = Label {
            id,
            name: name.to_string(),
            colour,
        };
        self.labels.push(label.clone());
        Some(label)
    }

    fn name_taken(&self, name: &str) -> bool {
        self.labels
            .iter()
            .any(|label| label.name.eq_ignore_ascii_case(name))
    }

    /// Rename a label, keeping its colour. Fails on a blank or duplicate name.
    pub fn rename(&mut self, id: LabelId, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let taken = self
            .labels
            .iter()
            .any(|label| label.id != id && label.name.eq_ignore_ascii_case(name));
        if taken {
            return false;
        }
        let Some(label) = self.labels.iter_mut().find(|label| label.id == id) else {
            return false;
        };
        label.name = name.to_string();
        true
    }

    /// Delete a label and drop it from every mail that carried it.
    pub fn remove(&mut self, id: LabelId) -> bool {
        let Some(index) = self.labels.iter().position(|label| label.id == id) else {
            return false;
        };
        self.labels.remove(index);
        for (_, assigned) in self.assignments.iter_mut() {
            assigned.retain(|assigned| *assigned != id);
        }
        self.assignments
            .retain(|(_, assigned)| !assigned.is_empty());
        true
    }

    /// The label ids on one mail, in stable order.
    pub fn labels_for(&self, email: EmailId) -> &[LabelId] {
        self.assignments
            .iter()
            .find(|(id, _)| *id == email)
            .map(|(_, assigned)| assigned.as_slice())
            .unwrap_or(&[])
    }

    /// Add or remove one label on one mail. Assigning a second label to a mail
    /// that already has one keeps both, so labels compose.
    pub fn toggle(&mut self, email: EmailId, label: LabelId) -> bool {
        if self.label(label).is_none() {
            return false;
        }
        match self.assignments.iter_mut().find(|(id, _)| *id == email) {
            Some((_, assigned)) => {
                if let Some(index) = assigned.iter().position(|id| *id == label) {
                    assigned.remove(index);
                } else {
                    assigned.push(label);
                    assigned.sort_unstable();
                }
                if assigned.is_empty() {
                    self.assignments.retain(|(id, _)| *id != email);
                }
            }
            None => self.assignments.push((email, vec![label])),
        }
        true
    }

    /// Replace a mail's labels outright, used to seed from a draft or test.
    pub fn set_for(&mut self, email: EmailId, labels: &[LabelId]) {
        let mut assigned: Vec<LabelId> = labels
            .iter()
            .copied()
            .filter(|id| self.label(*id).is_some())
            .collect();
        assigned.sort_unstable();
        assigned.dedup();
        if assigned.is_empty() {
            self.assignments.retain(|(id, _)| *id != email);
        } else {
            match self.assignments.iter_mut().find(|(id, _)| *id == email) {
                Some((_, slot)) => *slot = assigned,
                None => self.assignments.push((email, assigned)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::model::EmailId;

    const MAIL: EmailId = EmailId(7);

    #[test]
    fn creating_rejects_blank_and_duplicate_names() {
        let mut store = LabelStore::new();
        let label = store.create("Work").expect("a fresh name is accepted");
        assert_eq!(label.name, "Work");
        assert!(store.create("   ").is_none(), "blank is rejected");
        assert!(
            store.create("work").is_none(),
            "case-insensitive dupe rejected"
        );
        assert!(store.create("Personal").is_some());
        assert_eq!(store.labels().len(), 2);
    }

    #[test]
    fn new_labels_get_distinct_colours() {
        let mut store = LabelStore::new();
        let a = store.create("A").unwrap();
        let b = store.create("B").unwrap();
        assert_ne!(a.colour, b.colour, "consecutive labels must not collide");
    }

    #[test]
    fn renaming_keeps_the_colour_and_rejects_duplicates() {
        let mut store = LabelStore::new();
        let a = store.create("Work").unwrap();
        store.create("Home").unwrap();
        assert!(store.rename(a.id, "Office"));
        assert!(!store.rename(a.id, "home"), "duplicate rename is refused");
        assert!(!store.rename(a.id, "  "), "blank rename is refused");
        let renamed = store.label(a.id).unwrap();
        assert_eq!(renamed.name, "Office");
        assert_eq!(renamed.colour, a.colour, "a rename must not recolour");
    }

    #[test]
    fn toggling_adds_then_removes_and_keeps_multiples() {
        let mut store = LabelStore::new();
        let work = store.create("Work").unwrap();
        let urgent = store.create("Urgent").unwrap();
        let mail = MAIL;

        assert!(store.toggle(mail, work.id));
        assert_eq!(store.labels_for(mail), &[work.id]);
        // A second label composes with the first.
        assert!(store.toggle(mail, urgent.id));
        assert_eq!(store.labels_for(mail), &[work.id, urgent.id]);
        // Toggling off leaves the other alone.
        assert!(store.toggle(mail, work.id));
        assert_eq!(store.labels_for(mail), &[urgent.id]);
        assert!(store.toggle(mail, urgent.id));
        assert!(store.labels_for(mail).is_empty());
    }

    #[test]
    fn toggling_an_unknown_label_is_refused() {
        let mut store = LabelStore::new();
        assert!(
            !store.toggle(EmailId(1), 999),
            "cannot assign a label that does not exist"
        );
        assert!(store.labels_for(EmailId(1)).is_empty());
    }

    #[test]
    fn removing_a_label_clears_it_from_mails() {
        let mut store = LabelStore::new();
        let work = store.create("Work").unwrap();
        let keep = store.create("Keep").unwrap();
        store.toggle(EmailId(3), work.id);
        store.toggle(EmailId(3), keep.id);
        store.toggle(EmailId(4), work.id);

        assert!(store.remove(work.id));
        assert!(store.label(work.id).is_none());
        assert_eq!(
            store.labels_for(EmailId(3)),
            &[keep.id],
            "removing one label leaves the others the mail carried"
        );
        assert!(
            store.labels_for(EmailId(4)).is_empty(),
            "a mail that only had the removed label is now unlabelled"
        );
        assert!(
            !store.remove(work.id),
            "removing twice reports nothing to do"
        );
    }
}
