//! A case's replay bundle as the host supplies it (spec 015, 3.1).
//!
//! The library reads no storage: the host either parsed a bundle or
//! observed why the bytes it holds are unusable, and says which. A case
//! with no entry at all is `bundle-missing`, a different statement: the
//! host holds nothing for it.

use rustev_contract::replay::ReplayBundle;

/// The most bytes of an operator note a load failure keeps.
pub const NOTE_MAX_BYTES: usize = 256;

/// Why a bundle the host holds could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BundleLoad {
    /// The host could not read bytes it holds: an I/O error, a refusal, a
    /// file that is not a regular file.
    Inaccessible,
    /// The bytes exceed `REPLAY_V1`'s document limit.
    Oversized,
    /// The bytes are not a valid `rustev.replay/1` document.
    Corrupt,
}

impl BundleLoad {
    /// The case's incomparable reason (spec 015, 3.2.1).
    pub fn code(self) -> &'static str {
        match self {
            BundleLoad::Inaccessible => "bundle-inaccessible",
            BundleLoad::Oversized => "bundle-oversized",
            BundleLoad::Corrupt => "bundle-corrupt",
        }
    }
}

/// A load failure the host observed, with a bounded note for the operator.
/// The note is not authoritative and never enters a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadFailure {
    pub kind: BundleLoad,
    note: String,
}

impl LoadFailure {
    /// Keeps at most [`NOTE_MAX_BYTES`] of `note`, cut at a character
    /// boundary.
    pub fn new(kind: BundleLoad, note: &str) -> LoadFailure {
        let mut end = note.len().min(NOTE_MAX_BYTES);
        while !note.is_char_boundary(end) {
            end -= 1;
        }
        LoadFailure {
            kind,
            note: note[..end].to_string(),
        }
    }

    pub fn note(&self) -> &str {
        &self.note
    }
}

/// One case's entry in the evaluation inputs.
#[derive(Debug, Clone)]
pub enum BundleInput {
    Loaded(Box<ReplayBundle>),
    Failed(LoadFailure),
}

impl From<ReplayBundle> for BundleInput {
    fn from(b: ReplayBundle) -> BundleInput {
        BundleInput::Loaded(Box::new(b))
    }
}

impl From<LoadFailure> for BundleInput {
    fn from(f: LoadFailure) -> BundleInput {
        BundleInput::Failed(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_cut_at_a_character_boundary() {
        let long = "é".repeat(200);
        let f = LoadFailure::new(BundleLoad::Corrupt, &long);
        assert_eq!(f.note().len(), NOTE_MAX_BYTES);
        let odd = format!("x{long}");
        let f = LoadFailure::new(BundleLoad::Corrupt, &odd);
        assert_eq!(f.note().len(), NOTE_MAX_BYTES - 1);
        assert_eq!(
            LoadFailure::new(BundleLoad::Oversized, "short").note(),
            "short"
        );
    }
}
