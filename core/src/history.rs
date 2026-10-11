// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Undo and redo — in the core, so every shell has the same history.
//!
//! Two stacks of [`Edit`]s and nothing else: an entry *is* the change, and undoing it is applying
//! its inverse. The two decisions that are not mechanical are both here, because a shell that
//! made them would make them differently:
//!
//! - **Typing coalesces.** One Ctrl+Z after typing a sentence removes a word, not a letter. A
//!   run of typed characters at consecutive offsets merges into one entry until a word ends (the
//!   first whitespace after a non-whitespace character), the caret moves, or the document is
//!   saved ([`History::seal`]).
//! - **"Modified" is a position in the history**, not a flag: the document is clean exactly when
//!   the undo stack is as deep as it was at the last save. Undoing back to that point makes it
//!   clean again, which a dirty *flag* cannot express.

use serde::{Deserialize, Serialize};

use crate::document::Edit;

/// The undo and redo stacks, and where the last save was.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// The undo depth at the last save, or `None` once that state is unreachable (it was on the
    /// redo stack and a new edit discarded it).
    saved: Option<usize>,
    /// Whether the next typed character may merge into the top entry.
    #[serde(skip)]
    open: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    edit: Edit,
    typed: bool,
}

impl History {
    /// A fresh history whose current state counts as saved — a document just read from a file.
    pub fn saved() -> Self {
        History {
            saved: Some(0),
            ..History::default()
        }
    }

    /// Record an edit already applied to the document. `typed` marks a keystroke that may
    /// coalesce with the one before it.
    pub fn record(&mut self, edit: Edit, typed: bool) {
        if self.saved.is_some_and(|depth| depth > self.undo.len()) {
            self.saved = None;
        }
        self.redo.clear();

        if typed
            && self.open
            && let Some(top) = self.undo.last_mut()
            && top.typed
            && coalesces(&top.edit, &edit)
        {
            top.edit.inserted.push_str(&edit.inserted);
            return;
        }
        self.undo.push(Entry { edit, typed });
        self.open = typed;
    }

    /// Stop the next keystroke from merging into the last one: the caret moved, or the document
    /// was saved. Without this, "type, save, type" would be one undo entry whose first half is
    /// on disk, and the document would read as clean after the second half.
    pub fn seal(&mut self) {
        self.open = false;
    }

    /// The edit that undoes the most recent one, moved onto the redo stack.
    pub fn undo(&mut self) -> Option<Edit> {
        self.open = false;
        let entry = self.undo.pop()?;
        let inverse = entry.edit.inverse();
        self.redo.push(entry);
        Some(inverse)
    }

    /// The edit to redo, moved back onto the undo stack.
    pub fn redo(&mut self) -> Option<Edit> {
        self.open = false;
        let entry = self.redo.pop()?;
        let edit = entry.edit.clone();
        self.undo.push(entry);
        Some(edit)
    }

    pub fn mark_saved(&mut self) {
        self.open = false;
        self.saved = Some(self.undo.len());
    }

    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

/// Whether `next` continues the typing run that ended in `top`.
fn coalesces(top: &Edit, next: &Edit) -> bool {
    if !next.removed.is_empty() || top.end() != next.at {
        return false;
    }
    let Some(new) = next.inserted.chars().next() else {
        return false;
    };
    let last = top.inserted.chars().last();
    // A word ends at its first whitespace: "hello world" is two entries, "hello" and " world".
    !(new.is_whitespace() && last.is_some_and(|c| !c.is_whitespace())) && new != '\n'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(at: usize, text: &str) -> Edit {
        Edit {
            at,
            removed: String::new(),
            inserted: text.into(),
        }
    }

    #[test]
    fn a_word_typed_a_letter_at_a_time_is_one_entry() {
        let mut history = History::saved();
        for (i, c) in "hello".chars().enumerate() {
            history.record(typed(i, &c.to_string()), true);
        }
        assert_eq!(history.undo().unwrap().removed, "hello");
        assert!(!history.can_undo());
    }

    #[test]
    fn whitespace_after_a_word_starts_the_next_entry() {
        let mut history = History::saved();
        for (i, c) in "hi you".chars().enumerate() {
            history.record(typed(i, &c.to_string()), true);
        }
        assert_eq!(history.undo().unwrap().removed, " you");
        assert_eq!(history.undo().unwrap().removed, "hi");
    }

    #[test]
    fn a_seal_stops_coalescing() {
        let mut history = History::saved();
        history.record(typed(0, "a"), true);
        history.seal();
        history.record(typed(1, "b"), true);
        assert_eq!(history.undo().unwrap().removed, "b");
    }

    #[test]
    fn undoing_back_to_the_save_point_is_clean_again() {
        let mut history = History::saved();
        history.record(typed(0, "a"), false);
        assert!(history.is_dirty());
        history.undo();
        assert!(!history.is_dirty());
        history.redo();
        assert!(history.is_dirty());
    }

    #[test]
    fn a_save_point_on_the_discarded_redo_stack_can_never_be_clean() {
        let mut history = History::saved();
        history.record(typed(0, "a"), false);
        history.mark_saved();
        history.undo();
        history.record(typed(0, "b"), false);
        history.undo();
        assert!(
            history.is_dirty(),
            "the saved state was discarded with the redo stack"
        );
    }
}
