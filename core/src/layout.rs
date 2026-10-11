// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Breaking one line of text into rows that fit a width — in the core, not in a shell.
//!
//! Down-arrow, Home, End, Page Down, a click and a selection's extent are every one of them
//! defined in terms of a *row on screen*, and a row is not a thing in the document: it is an
//! output of layout. If each shell wrapped text itself, each would own that half of the editing
//! model, and six shells would disagree about where the caret goes after Down. So the core owns
//! everything about layout that is **not a font question** — where a line may break (UAX #14),
//! how rows are filled, which offset a click lands on — and asks the shell the font question
//! through [`Metrics`].
//!
//! **The unit is the shell's own, and the core never converts.** Terminal cells, Pango units,
//! CSS pixels, device pixels — it does not matter, as long as the width a shell hands in is in
//! the same unit its [`Metrics`] answers in. A core that invented a unit would need a DPI.
//!
//! Left-to-right only. Bidirectional layout is a named gap (`doc/architecture.md`, "What the
//! template does not do"), not an oversight.

use unicode_linebreak::{BreakOpportunity, linebreaks};

/// How wide is a piece of text, and how tall is a row of it?
///
/// The two questions the core cannot answer, and so the entire surface between layout and the
/// shell that draws it. One implementation per shell: `unicode-width` cells in the terminal,
/// Pango in GTK, a canvas in the browser, GDI on Windows, CoreText on macOS — and [`Fixed`] in
/// the CLI and every test.
pub trait Metrics {
    /// The cumulative advance after each `char` of `text`, appended to `out`.
    ///
    /// Exactly `text.chars().count()` values, never decreasing; the last is the width of the
    /// whole string. **Cumulative and in one call on purpose**: `width("a") + width("b")` is not
    /// `width("ab")` once kerning and ligatures exist, so a per-character question would be
    /// quietly wrong. Handing over the whole line lets the shell shape it once, and leaves
    /// every caret x in the resulting [`LineLayout`] — so hit-testing needs no font afterwards.
    ///
    /// **Measure with the engine you draw with.** A shell that measured with one font engine
    /// and painted with another would put the caret where the ink is not.
    fn advances(&self, text: &str, out: &mut Vec<f32>);

    /// The height of one row, in the same unit.
    fn line_height(&self) -> f32;
}

/// One unit per `char`, one unit per row.
///
/// What the CLI measures with, so `quill view --width 40` and `quill move down --width 40`
/// are real answers, reachable with no display at all. Also what every test uses, because a
/// synthetic provider makes wrapping exactly assertable.
///
/// Not good enough for a terminal, and not trying to be: a CJK ideograph takes two cells and a
/// combining mark none. That is `unicode-width`'s job, in `ui_tui`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fixed;

impl Metrics for Fixed {
    fn advances(&self, text: &str, out: &mut Vec<f32>) {
        out.extend((1..=text.chars().count()).map(|n| n as f32));
    }

    fn line_height(&self) -> f32 {
        1.0
    }
}

/// How many spaces wide a tab is drawn, in every shell.
///
/// ponytail: a fixed width rather than a tab *stop*. A stop depends on where the tab starts,
/// which [`Metrics::advances`] could answer (it sees the whole line) but a row drawn on its own
/// could not without being told its line's x; the shells all use this fixed width meanwhile, so
/// they agree with each other.
pub const TAB_SPACES: usize = 4;

/// What a shell draws for one character of the document.
///
/// Tabs become [`TAB_SPACES`] spaces and C0 control characters their Unicode Control Pictures
/// (U+2400 + code), so a lone `\r` in a file with mixed line endings is visible rather than a
/// cursor jump or nothing. Every other character is itself.
///
/// In the core because it is a decision every shell must make *identically* to the measuring
/// half: a shell measures [`printable`] text and draws the same string, which is the rule that
/// keeps the caret where the ink is.
pub fn shown(c: char) -> std::borrow::Cow<'static, str> {
    match c {
        '\t' => " ".repeat(TAB_SPACES).into(),
        c if (c as u32) < 0x20 => char::from_u32(0x2400 + c as u32)
            .unwrap()
            .to_string()
            .into(),
        '\u{7f}' => "\u{2421}".into(),
        c => c.to_string().into(),
    }
}

/// `text` as drawn ([`shown`] for every character), with the byte offset in the result at which
/// each original character starts — plus one more, the result's length. So character `i` of
/// `text` is drawn as `printed[starts[i]..starts[i + 1]]`, which is how a shell measuring the
/// printed string maps its advances back onto the document's characters.
pub fn printable(text: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut starts = Vec::with_capacity(text.len() + 1);
    for c in text.chars() {
        starts.push(out.len());
        out.push_str(&shown(c));
    }
    starts.push(out.len());
    (out, starts)
}

/// One row: a range of char offsets into its line, `start..end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    /// One past the last char, *including* trailing spaces the break ate.
    pub end: usize,
}

/// One logical line, broken into rows at a width.
///
/// A plain value carrying the x of every caret position, so nothing that reads it needs
/// [`Metrics`] again.
#[derive(Clone, Debug, PartialEq)]
pub struct LineLayout {
    /// Cumulative advance at every caret offset `0..=len`.
    xs: Vec<f32>,
    rows: Vec<Span>,
    row_height: f32,
}

impl LineLayout {
    pub fn rows(&self) -> &[Span] {
        &self.rows
    }

    pub fn row_height(&self) -> f32 {
        self.row_height
    }

    /// Characters in the line.
    pub fn len(&self) -> usize {
        self.xs.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Which row a caret offset is drawn on.
    ///
    /// At a soft break an offset is both the end of one row and the start of the next; it is
    /// drawn on **the later row**, where a caret walking off the end of a wrapped row is expected
    /// to reappear. The line's last offset has no later row and stays on the last.
    pub fn row_of(&self, offset: usize) -> usize {
        let offset = offset.min(self.len());
        self.rows
            .iter()
            .position(|row| offset < row.end)
            .unwrap_or(self.rows.len() - 1)
    }

    /// A caret offset's x, relative to the start of its row.
    pub fn x_of(&self, offset: usize) -> f32 {
        let offset = offset.min(self.len());
        let row = self.rows[self.row_of(offset)];
        self.xs[offset] - self.xs[row.start]
    }

    /// The x of `offset` measured from the start of `row`, whichever row the offset is drawn on —
    /// what a selection's edge needs at a soft break, where [`LineLayout::x_of`] would answer
    /// for the later row.
    pub fn x_in_row(&self, row: usize, offset: usize) -> f32 {
        let span = self.rows[row];
        self.xs[offset.clamp(span.start, span.end)] - self.xs[span.start]
    }

    /// The caret offset nearest to `x` on `row` — hit-testing a click, and the landing spot of
    /// an Up or Down that remembers its column.
    ///
    /// Nearest rather than "the character under x", because a caret goes *between* characters.
    /// On a row that ends in a soft break the row's own end is excluded: that offset is drawn on
    /// the next row, so clicking far right of a wrapped row lands before its trailing space.
    pub fn offset_at(&self, row: usize, x: f32) -> usize {
        let span = self.rows[row.min(self.rows.len() - 1)];
        let last = if row + 1 < self.rows.len() && span.end > span.start {
            span.end - 1
        } else {
            span.end
        };
        let origin = self.xs[span.start];
        (span.start..=last)
            .min_by(|&a, &b| {
                let da = (self.xs[a] - origin - x).abs();
                let db = (self.xs[b] - origin - x).abs();
                da.total_cmp(&db)
            })
            .unwrap_or(span.start)
    }

    /// The width of a row, trailing spaces included.
    pub fn row_width(&self, row: usize) -> f32 {
        let span = self.rows[row];
        self.xs[span.end] - self.xs[span.start]
    }
}

/// Break `text` (one line, no `\n`) into rows no wider than `width`.
///
/// Greedy: fill a row until the next break opportunity would overflow it. Trailing spaces hang
/// past the edge rather than starting the next row, as in every editor. A word wider than the
/// whole width is broken between characters, since the alternative is a row that never ends.
///
/// `width <= 0` means **do not wrap**: one row, which is what the CLI wants without `--width`.
pub fn wrap(text: &str, width: f32, metrics: &dyn Metrics) -> LineLayout {
    let mut xs = Vec::with_capacity(text.len() + 1);
    xs.push(0.0);
    metrics.advances(text, &mut xs);
    let len = xs.len() - 1;
    let mut rows = Vec::new();
    let mut start = 0;

    if width > 0.0 && len > 0 {
        let chars: Vec<char> = text.chars().collect();
        // Break opportunities as char offsets; UAX #14 hands out byte offsets.
        let mut byte_to_char = Vec::with_capacity(text.len() + 1);
        for (i, c) in chars.iter().enumerate() {
            byte_to_char.extend(std::iter::repeat_n(i, c.len_utf8()));
        }
        byte_to_char.push(len);
        let opportunities: Vec<usize> = linebreaks(text)
            .filter(|&(_, kind)| {
                matches!(
                    kind,
                    BreakOpportunity::Allowed | BreakOpportunity::Mandatory
                )
            })
            .map(|(byte, _)| byte_to_char[byte])
            .collect();

        let mut candidate: Option<usize> = None;
        let mut i = 0;
        while i < opportunities.len() {
            let at = opportunities[i];
            // What has to fit is the row's ink: trailing whitespace may hang past the edge.
            let mut ink = at;
            while ink > start && chars[ink - 1].is_whitespace() {
                ink -= 1;
            }
            if xs[ink] - xs[start] <= width {
                candidate = Some(at);
                i += 1;
                continue;
            }
            match candidate.take() {
                Some(at) if at > start => {
                    rows.push(Span { start, end: at });
                    start = at;
                }
                _ => {
                    // No opportunity fits: break inside the word, as late as it still fits.
                    let mut end = start + 1;
                    while end < ink && xs[end + 1] - xs[start] <= width {
                        end += 1;
                    }
                    rows.push(Span { start, end });
                    start = end;
                }
            }
        }
    }
    rows.push(Span { start, end: len });

    LineLayout {
        xs,
        rows,
        row_height: metrics.line_height(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str, width: f32) -> Vec<String> {
        let layout = wrap(text, width, &Fixed);
        let chars: Vec<char> = text.chars().collect();
        layout
            .rows()
            .iter()
            .map(|span| chars[span.start..span.end].iter().collect())
            .collect()
    }

    #[test]
    fn printable_text_maps_back_onto_characters() {
        let (printed, starts) = printable("a\tb\r");
        assert_eq!(printed, "a    b\u{240d}");
        assert_eq!(starts, [0, 1, 5, 6, 9]);
    }

    #[test]
    fn a_line_that_fits_is_one_row() {
        assert_eq!(rows("hello world", 20.0), ["hello world"]);
        assert_eq!(rows("", 20.0), [""]);
    }

    #[test]
    fn rows_break_after_spaces_which_hang_past_the_edge() {
        assert_eq!(rows("hello world again", 11.0), ["hello world ", "again"]);
        assert_eq!(rows("hello world", 5.0), ["hello ", "world"]);
    }

    #[test]
    fn a_word_wider_than_the_row_is_broken_between_characters() {
        assert_eq!(rows("abcdefgh", 3.0), ["abc", "def", "gh"]);
        assert_eq!(rows("ab abcdefgh", 3.0), ["ab ", "abc", "def", "gh"]);
    }

    #[test]
    fn zero_width_means_no_wrapping() {
        assert_eq!(rows("a b c d e f", 0.0), ["a b c d e f"]);
    }

    #[test]
    fn a_caret_at_a_soft_break_is_drawn_on_the_later_row() {
        let layout = wrap("hello world", 6.0, &Fixed);
        assert_eq!(layout.row_of(6), 1);
        assert_eq!(layout.x_of(6), 0.0);
        assert_eq!(
            layout.row_of(11),
            1,
            "the line's last offset stays on the last row"
        );
    }

    #[test]
    fn a_click_lands_on_the_nearest_caret_position() {
        let layout = wrap("hello world", 6.0, &Fixed);
        assert_eq!(layout.offset_at(0, 1.4), 1);
        assert_eq!(layout.offset_at(0, 1.6), 2);
        assert_eq!(layout.offset_at(0, 99.0), 5, "not past the soft break");
        assert_eq!(layout.offset_at(1, 99.0), 11);
    }
}
