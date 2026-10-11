// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! What a shell paints: the rows on screen, the selection on each, and the caret.
//!
//! [`View`] is the windowed read API. It holds only the rows that fit the [`Frame`] the shell
//! described, laid out with the shell's own [`Metrics`], so a paint costs what the window costs
//! and never what the file costs. Everything in it is already in the shell's unit — the caret's
//! x, a selection's extent, each row's top — so a shell draws it without doing arithmetic of
//! its own, which is where shells used to disagree.

use serde::{Deserialize, Serialize};

use crate::document::{Document, Position};
use crate::layout::{LineLayout, Metrics, wrap};

/// The surface a shell is about to paint, described in the shell's own unit.
///
/// Passed to every operation that depends on layout. The CLI passes [`Frame::fixed`]: one unit
/// per character, one per row, which is how `quill move FILE down --width 40` is a real answer
/// with no display at all.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    /// Where rows wrap. Zero or less means they do not.
    pub width: f32,
    /// How much of the document is visible — what Page Down and scrolling measure in.
    pub height: f32,
    pub metrics: &'a dyn Metrics,
}

impl<'a> Frame<'a> {
    pub fn new(width: f32, height: f32, metrics: &'a dyn Metrics) -> Self {
        Frame {
            width,
            height,
            metrics,
        }
    }

    /// A frame of `width` columns and `rows` rows, measured with [`crate::layout::Fixed`].
    pub fn fixed(width: u32, rows: u32) -> Frame<'static> {
        Frame {
            width: width as f32,
            height: rows as f32,
            metrics: &crate::layout::Fixed,
        }
    }

    /// How many whole rows fit — never zero, so a frame not yet measured still moves a caret.
    pub fn rows(&self) -> u64 {
        let height = self.metrics.line_height();
        if height > 0.0 && self.height > 0.0 {
            ((self.height / height).floor() as u64).max(1)
        } else {
            1
        }
    }
}

/// The first row on screen: the `row`-th wrapped row of logical `line`.
///
/// Owned by the core, so every shell scrolls the same way, and stored in rows rather than
/// pixels so that resizing a window keeps the same text at the top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Scroll {
    pub line: u64,
    pub row: u64,
}

/// Everything a shell needs to paint one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub rows: Vec<Row>,
    /// `None` when the caret is scrolled out of sight.
    pub caret: Option<Caret>,
    pub line_count: u64,
    pub scroll: Scroll,
    pub row_height: f32,
}

/// One row on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The logical line it belongs to.
    pub line: u64,
    /// Its columns within that line, `start..end`.
    pub start: u64,
    pub end: u64,
    pub text: String,
    /// Distance from the frame's top.
    pub top: f32,
    /// Whether this is the first row of its line — where a line number goes.
    pub first: bool,
    pub selection: Option<Selection>,
}

/// The selected stretch of a row, as x coordinates from the row's start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub from: f32,
    pub to: f32,
}

/// Where to draw the caret.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    pub position: Position,
    /// Index into [`View::rows`].
    pub row: usize,
    pub x: f32,
    pub top: f32,
    pub height: f32,
}

/// Layout questions about a document, asked against one frame.
pub(crate) struct Visual<'a> {
    pub doc: &'a Document,
    pub frame: &'a Frame<'a>,
}

impl Visual<'_> {
    pub fn layout(&self, line: usize) -> LineLayout {
        wrap(
            &self.doc.line_text(line),
            self.frame.width,
            self.frame.metrics,
        )
    }

    /// The (line, row) a char offset is drawn on.
    pub fn locate(&self, offset: usize) -> (usize, usize, LineLayout) {
        let line = self.doc.line_of(offset);
        let layout = self.layout(line);
        let row = layout.row_of(offset - self.doc.line_start(line));
        (line, row, layout)
    }

    /// `scroll` pulled into the document, so a stale offset never shows an empty frame.
    pub fn clamp(&self, scroll: Scroll) -> Scroll {
        let last = self.doc.line_count() - 1;
        let line = (scroll.line as usize).min(last);
        let rows = self.layout(line).rows().len();
        Scroll {
            line: line as u64,
            row: scroll.row.min(rows as u64 - 1),
        }
    }

    pub fn next(&self, at: Scroll) -> Option<Scroll> {
        let rows = self.layout(at.line as usize).rows().len() as u64;
        if at.row + 1 < rows {
            Some(Scroll {
                line: at.line,
                row: at.row + 1,
            })
        } else if (at.line as usize) + 1 < self.doc.line_count() {
            Some(Scroll {
                line: at.line + 1,
                row: 0,
            })
        } else {
            None
        }
    }

    pub fn previous(&self, at: Scroll) -> Option<Scroll> {
        if at.row > 0 {
            Some(Scroll {
                line: at.line,
                row: at.row - 1,
            })
        } else if at.line > 0 {
            let rows = self.layout(at.line as usize - 1).rows().len() as u64;
            Some(Scroll {
                line: at.line - 1,
                row: rows - 1,
            })
        } else {
            None
        }
    }

    /// `at` moved `delta` rows, stopping at either end of the document.
    pub fn step(&self, mut at: Scroll, delta: i64) -> Scroll {
        for _ in 0..delta.unsigned_abs() {
            let next = if delta > 0 {
                self.next(at)
            } else {
                self.previous(at)
            };
            match next {
                Some(next) => at = next,
                None => break,
            }
        }
        at
    }

    /// The scroll position that keeps `offset` on screen, moving as little as possible.
    pub fn follow(&self, scroll: Scroll, offset: usize) -> Scroll {
        let scroll = self.clamp(scroll);
        let (line, row, _) = self.locate(offset);
        let caret = Scroll {
            line: line as u64,
            row: row as u64,
        };
        if caret < scroll {
            return caret;
        }
        let rows = self.frame.rows();
        let mut at = scroll;
        for _ in 1..rows {
            if at == caret {
                return scroll;
            }
            match self.next(at) {
                Some(next) => at = next,
                None => return scroll,
            }
        }
        if at == caret {
            return scroll;
        }
        // Below the frame: put the caret on the last row.
        self.step(caret, 1 - rows as i64)
    }

    /// The char offset under a point in the frame — a click.
    pub fn hit(&self, scroll: Scroll, x: f32, y: f32) -> usize {
        let mut at = self.clamp(scroll);
        let height = self.frame.metrics.line_height().max(f32::MIN_POSITIVE);
        let mut top = 0.0;
        while top + height <= y {
            match self.next(at) {
                Some(next) => at = next,
                None => break,
            }
            top += height;
        }
        let layout = self.layout(at.line as usize);
        self.doc.line_start(at.line as usize) + layout.offset_at(at.row as usize, x.max(0.0))
    }

    /// Everything on screen, from `scroll` down.
    pub fn view(&self, scroll: Scroll, caret: usize, selection: Option<(usize, usize)>) -> View {
        let scroll = self.clamp(scroll);
        let row_height = self.frame.metrics.line_height();
        let mut rows = Vec::new();
        let mut caret_at = None;
        let (caret_line, caret_row, caret_layout) = self.locate(caret);
        let mut line = scroll.line as usize;
        let mut first_row = scroll.row as usize;
        let mut top = 0.0;

        'lines: while line < self.doc.line_count() {
            let layout = self.layout(line);
            let text: Vec<char> = self.doc.line_text(line).chars().collect();
            let line_start = self.doc.line_start(line);
            let line_end = self.doc.line_end(line);
            let last_line = line + 1 == self.doc.line_count();

            for (r, span) in layout.rows().iter().enumerate().skip(first_row) {
                if !rows.is_empty() && top >= self.frame.height {
                    break 'lines;
                }
                let last_row = r + 1 == layout.rows().len();
                let selection = selection.and_then(|(from, to)| {
                    let row_start = line_start + span.start;
                    let row_end = line_start + span.end;
                    // The line break is selected too: show it, as a short stretch past the text.
                    let newline = last_row && !last_line && from <= line_end && to > line_end;
                    let lo = from.max(row_start);
                    let hi = to.min(row_end);
                    if lo >= hi && !newline {
                        return None;
                    }
                    let from = layout.x_in_row(r, lo - line_start);
                    let mut to = layout.x_in_row(r, hi.max(lo) - line_start);
                    if newline {
                        to += row_height / 2.0;
                    }
                    Some(Selection { from, to })
                });
                if line == caret_line && r == caret_row {
                    caret_at = Some(Caret {
                        position: self.doc.position(caret),
                        row: rows.len(),
                        x: caret_layout.x_of(caret - line_start),
                        top,
                        height: row_height,
                    });
                }
                rows.push(Row {
                    line: line as u64,
                    start: span.start as u64,
                    end: span.end as u64,
                    text: text[span.start..span.end].iter().collect(),
                    top,
                    first: r == 0,
                    selection,
                });
                top += row_height;
            }
            first_row = 0;
            line += 1;
        }

        View {
            rows,
            caret: caret_at,
            line_count: self.doc.line_count() as u64,
            scroll,
            row_height,
        }
    }
}
