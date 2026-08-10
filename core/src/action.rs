use serde::{Deserialize, Serialize};

use crate::state::Position;

/// A single reversible edit.
///
/// Every mutation of the document goes through one of these, which is what makes
/// undo/redo a core concern rather than something each shell reimplements. Both
/// variants carry the affected text so the inverse can be derived without
/// re-reading the document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Insert { at: Position, text: String },
    Delete { at: Position, text: String },
}

impl Action {
    /// The edit that undoes this one.
    pub fn inverse(&self) -> Action {
        match self {
            Action::Insert { at, text } => Action::Delete {
                at: *at,
                text: text.clone(),
            },
            Action::Delete { at, text } => Action::Insert {
                at: *at,
                text: text.clone(),
            },
        }
    }
}
