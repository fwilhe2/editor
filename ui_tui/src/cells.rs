// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! This shell's whole contribution to layout: how many terminal cells a character takes.
//!
//! The core wraps lines, moves carets by rows and hit-tests clicks (`quill_core::layout`); it
//! asks only how wide text is. Here the answer is `unicode-width`'s — two cells for a CJK
//! ideograph, none for a combining mark — plus two decisions about characters a terminal would
//! otherwise draw as nothing or as a cursor jump — a tab and a control character — which are the
//! core's (`quill_core::layout::shown`), so every shell draws them alike.
//!
//! [`shown`] and [`width`] are one decision in two halves, and that is the rule every shell
//! keeps: **measure with exactly what you draw**, or the caret lands where the ink is not.

use quill_core::layout::{self, Metrics, TAB_SPACES};
use unicode_width::UnicodeWidthChar;

/// What is drawn for `c` — the core's answer, shared by every shell.
pub fn shown(c: char) -> String {
    layout::shown(c).into_owned()
}

/// How many cells [`shown`]`(c)` takes.
pub fn width(c: char) -> usize {
    match c {
        '\t' => TAB_SPACES,
        c if (c as u32) < 0x20 || c == '\u{7f}' => 1,
        c => c.width().unwrap_or(0),
    }
}

/// [`Metrics`] in terminal cells.
pub struct Cells;

impl Metrics for Cells {
    fn advances(&self, text: &str, out: &mut Vec<f32>) {
        let mut x = 0;
        for c in text.chars() {
            x += width(c);
            out.push(x as f32);
        }
    }

    fn line_height(&self) -> f32 {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_and_zero_width_characters_take_their_cells() {
        let mut out = Vec::new();
        Cells.advances("a漢e\u{301}\t", &mut out);
        assert_eq!(out, [1.0, 3.0, 4.0, 4.0, 8.0]);
    }

    #[test]
    fn what_is_drawn_is_as_wide_as_what_was_measured() {
        for c in ['a', '漢', '\t', '\r', '\u{7f}', 'é'] {
            let drawn: usize = shown(c).chars().map(|d| d.width().unwrap_or(0)).sum();
            assert_eq!(drawn, width(c), "{c:?}");
        }
    }
}
