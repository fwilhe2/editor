// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The vocabulary of caret movement and deletion — names, not keys.
//!
//! A shell maps *its* keys onto these (Ctrl+Right on Linux and Windows, ⌥→ on macOS, `w` in a
//! vi-flavoured terminal) and the core decides what each one means. That split is the point:
//! the key is a platform convention, the meaning is the program's.

use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

use crate::document::Document;

/// Where a caret can be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Motion {
    /// One grapheme cluster — so an emoji with a skin tone, or `e` + a combining accent, is one
    /// press of an arrow key, not two.
    Left,
    Right,
    /// To the start of the previous word / past the end of the next one (UAX #29).
    WordLeft,
    WordRight,
    /// One *row* on screen, keeping the column the run of vertical moves started in.
    Up,
    Down,
    /// The start and end of the row on screen — of a wrapped line's visible piece, not of the
    /// whole logical line.
    RowStart,
    RowEnd,
    /// One screenful.
    PageUp,
    PageDown,
    DocumentStart,
    DocumentEnd,
}

impl Motion {
    /// Every motion, in the order a reference lists them.
    pub const ALL: [Motion; 12] = [
        Motion::Left,
        Motion::Right,
        Motion::WordLeft,
        Motion::WordRight,
        Motion::Up,
        Motion::Down,
        Motion::RowStart,
        Motion::RowEnd,
        Motion::PageUp,
        Motion::PageDown,
        Motion::DocumentStart,
        Motion::DocumentEnd,
    ];

    /// The name the CLI takes and prints (`quill move FILE word-right`).
    pub fn name(self) -> &'static str {
        match self {
            Motion::Left => "left",
            Motion::Right => "right",
            Motion::WordLeft => "word-left",
            Motion::WordRight => "word-right",
            Motion::Up => "up",
            Motion::Down => "down",
            Motion::RowStart => "row-start",
            Motion::RowEnd => "row-end",
            Motion::PageUp => "page-up",
            Motion::PageDown => "page-down",
            Motion::DocumentStart => "document-start",
            Motion::DocumentEnd => "document-end",
        }
    }

    pub fn from_name(name: &str) -> Option<Motion> {
        Motion::ALL.into_iter().find(|m| m.name() == name)
    }

    /// Whether the motion is defined by rows on screen, and so by layout. The rest move through
    /// the text alone, and keep no goal column.
    pub(crate) fn is_vertical(self) -> bool {
        matches!(
            self,
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown
        )
    }
}

/// What a Backspace or Delete key removes when nothing is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Delete {
    /// One grapheme before the caret — Backspace.
    Backward,
    /// One grapheme after it — Delete.
    Forward,
    /// Back to the start of the word — Ctrl+Backspace, ⌥⌫.
    WordBackward,
    /// On to the end of the word — Ctrl+Delete, ⌥⌦.
    WordForward,
}

impl Delete {
    pub(crate) fn motion(self) -> Motion {
        match self {
            Delete::Backward => Motion::Left,
            Delete::Forward => Motion::Right,
            Delete::WordBackward => Motion::WordLeft,
            Delete::WordForward => Motion::WordRight,
        }
    }
}

/// Which way [`crate::App::find`] looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Search {
    Forward,
    Backward,
}

/// The offset one grapheme cluster left of `offset`, crossing a line break as one step.
pub(crate) fn grapheme_left(doc: &Document, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let line = doc.line_of(offset);
    let start = doc.line_start(line);
    if offset == start {
        return offset - 1;
    }
    let text = doc.slice(start, offset);
    let last = text
        .graphemes(true)
        .next_back()
        .map_or(1, |g| g.chars().count());
    offset - last
}

/// The offset one grapheme cluster right of `offset`.
pub(crate) fn grapheme_right(doc: &Document, offset: usize) -> usize {
    if offset >= doc.len() {
        return doc.len();
    }
    let line = doc.line_of(offset);
    let end = doc.line_end(line);
    if offset == end {
        return offset + 1;
    }
    let text = doc.slice(offset, end);
    let first = text.graphemes(true).next().map_or(1, |g| g.chars().count());
    offset + first
}

/// The start of the word before `offset`, skipping whitespace and punctuation first; at a line's
/// start, the end of the line above.
pub(crate) fn word_left(doc: &Document, offset: usize) -> usize {
    let line = doc.line_of(offset);
    let start = doc.line_start(line);
    if offset == start {
        return offset.saturating_sub(1);
    }
    let text = doc.slice(start, offset);
    let column = text
        .split_word_bound_indices()
        .rfind(|(_, word)| is_word(word))
        .map_or(0, |(byte, _)| text[..byte].chars().count());
    start + column
}

/// The end of the word after `offset`, skipping whitespace and punctuation first; at a line's
/// end, the start of the line below.
pub(crate) fn word_right(doc: &Document, offset: usize) -> usize {
    let line = doc.line_of(offset);
    let end = doc.line_end(line);
    if offset >= end {
        return (offset + 1).min(doc.len());
    }
    let text = doc.slice(offset, end);
    for (byte, word) in text.split_word_bound_indices() {
        if is_word(word) {
            return offset + text[..byte].chars().count() + word.chars().count();
        }
    }
    end
}

fn is_word(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphemes_move_as_one() {
        // `e` + COMBINING ACUTE ACCENT, then a family emoji joined with ZWJs.
        let doc = Document::new("e\u{301}x👩\u{200d}👩\u{200d}👦y");
        assert_eq!(grapheme_right(&doc, 0), 2);
        assert_eq!(grapheme_left(&doc, 2), 0);
        assert_eq!(grapheme_right(&doc, 3), 8);
        assert_eq!(grapheme_left(&doc, 8), 3);
    }

    #[test]
    fn a_line_break_is_one_step() {
        let doc = Document::new("ab\ncd");
        assert_eq!(grapheme_right(&doc, 2), 3);
        assert_eq!(grapheme_left(&doc, 3), 2);
    }

    #[test]
    fn words_skip_punctuation_and_space() {
        let doc = Document::new("one, two  three");
        assert_eq!(word_right(&doc, 0), 3);
        assert_eq!(word_right(&doc, 3), 8);
        assert_eq!(word_left(&doc, 15), 10);
        assert_eq!(word_left(&doc, 10), 5);
        assert_eq!(word_left(&doc, 5), 0);
    }
}
