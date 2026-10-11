// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The document: a rope, and the one operation that changes it.

use ropey::Rope;
use serde::{Deserialize, Serialize};

/// A 0-based (line, column) address. Columns count `char`s, not bytes and not cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: u64,
    pub column: u64,
}

impl Position {
    pub fn new(line: u64, column: u64) -> Self {
        Position { line, column }
    }
}

/// One reversible change: at `at` (a char offset), `removed` was replaced by `inserted`.
///
/// One variant rather than `Insert`/`Delete`, because typing over a selection is a single
/// change — two actions would be two Ctrl+Z presses for one keystroke. The inverse is the same
/// shape with the two strings swapped, which is what makes undo a property of the data rather
/// than a second code path ([`Document::apply`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub at: usize,
    pub removed: String,
    pub inserted: String,
}

impl Edit {
    pub fn inverse(&self) -> Edit {
        Edit {
            at: self.at,
            removed: self.inserted.clone(),
            inserted: self.removed.clone(),
        }
    }

    /// Where a caret belongs after this edit: just past what it inserted.
    pub fn end(&self) -> usize {
        self.at + self.inserted.chars().count()
    }
}

/// The text. LF-separated, always — [`crate::Encoding`] is what remembers the file said CRLF.
#[derive(Clone, Debug, Default)]
pub struct Document {
    rope: Rope,
}

impl Document {
    pub fn new(text: &str) -> Self {
        Document {
            rope: Rope::from_str(text),
        }
    }

    /// Apply `edit` and return its inverse — **the whole of undo**.
    ///
    /// The caller pushes the inverse on a stack; applying it later restores exactly what this
    /// changed. No special cases, no second implementation of "un-delete".
    pub fn apply(&mut self, edit: &Edit) -> Edit {
        let at = edit.at.min(self.len());
        let end = (at + edit.removed.chars().count()).min(self.len());
        debug_assert_eq!(self.rope.slice(at..end).to_string(), edit.removed);
        self.rope.remove(at..end);
        self.rope.insert(at, &edit.inserted);
        edit.inverse()
    }

    /// Characters in the document, line breaks included.
    pub fn len(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lines in the document. Never zero: an empty document is one empty line, and a trailing
    /// newline starts a last, empty line — which is where a caret after it sits.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// The char offset at which `line` starts. Clamped to the last line.
    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_char(line.min(self.line_count() - 1))
    }

    /// Characters on `line`, without its line break.
    pub fn line_len(&self, line: usize) -> usize {
        let slice = self.rope.line(line.min(self.line_count() - 1));
        let len = slice.len_chars();
        if len > 0 && slice.char(len - 1) == '\n' {
            len - 1
        } else {
            len
        }
    }

    /// The char offset just before `line`'s break (or the document's end).
    pub fn line_end(&self, line: usize) -> usize {
        self.line_start(line) + self.line_len(line)
    }

    /// `line`'s text, without its line break.
    pub fn line_text(&self, line: usize) -> String {
        let start = self.line_start(line);
        self.rope
            .slice(start..start + self.line_len(line))
            .to_string()
    }

    /// The line a char offset is on.
    pub fn line_of(&self, offset: usize) -> usize {
        self.rope.char_to_line(offset.min(self.len()))
    }

    pub fn char_at(&self, offset: usize) -> Option<char> {
        (offset < self.len()).then(|| self.rope.char(offset))
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        let end = end.min(self.len());
        self.rope.slice(start.min(end)..end).to_string()
    }

    /// The char offset of `position`, after clamping it into the document — so a caller may pass
    /// a sloppy coordinate (line 99, column 99) and get the nearest real one.
    pub fn offset(&self, position: Position) -> usize {
        let line = (position.line as usize).min(self.line_count() - 1);
        self.line_start(line) + (position.column as usize).min(self.line_len(line))
    }

    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.len());
        let line = self.rope.char_to_line(offset);
        Position::new(line as u64, (offset - self.rope.line_to_char(line)) as u64)
    }

    pub fn clamp(&self, position: Position) -> Position {
        self.position(self.offset(position))
    }

    /// The whole text. For saving and for tests — rendering goes through `App::view`.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Every char offset where `needle` starts, from `from` onwards. A plain substring search;
    /// the rope is walked a line at a time so a match never has to be assembled from chunks
    /// unless it spans a line break.
    pub(crate) fn find(&self, needle: &str, from: usize) -> Option<usize> {
        if needle.is_empty() {
            return None;
        }
        let haystack = self.slice(from, self.len());
        haystack
            .find(needle)
            .map(|byte| from + haystack[..byte].chars().count())
    }

    /// The last char offset before `before` where `needle` starts.
    pub(crate) fn rfind(&self, needle: &str, before: usize) -> Option<usize> {
        if needle.is_empty() {
            return None;
        }
        let haystack = self.slice(0, before + needle.chars().count() - 1);
        haystack
            .rfind(needle)
            .map(|byte| haystack[..byte].chars().count())
            .filter(|&at| at < before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applying_an_inverse_restores_the_text() {
        let mut doc = Document::new("hello world");
        let edit = Edit {
            at: 6,
            removed: "world".into(),
            inserted: "there".into(),
        };
        let inverse = doc.apply(&edit);
        assert_eq!(doc.text(), "hello there");
        doc.apply(&inverse);
        assert_eq!(doc.text(), "hello world");
    }

    #[test]
    fn lines_exclude_their_break_and_a_trailing_newline_starts_one() {
        let doc = Document::new("ab\ncd\n");
        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.line_text(0), "ab");
        assert_eq!(doc.line_text(2), "");
        assert_eq!(doc.line_end(0), 2);
    }

    #[test]
    fn positions_clamp_into_the_document() {
        let doc = Document::new("ab\ncd");
        assert_eq!(doc.clamp(Position::new(9, 9)), Position::new(1, 2));
        assert_eq!(doc.offset(Position::new(0, 9)), 2);
    }

    #[test]
    fn find_walks_forwards_and_backwards() {
        let doc = Document::new("one two one");
        assert_eq!(doc.find("one", 0), Some(0));
        assert_eq!(doc.find("one", 1), Some(8));
        assert_eq!(doc.find("one", 9), None);
        assert_eq!(doc.rfind("one", 8), Some(0));
        assert_eq!(doc.rfind("one", 11), Some(8));
    }
}
