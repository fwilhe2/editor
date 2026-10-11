// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! This shell's contribution to layout: how wide text is, in CSS pixels, measured on a canvas.
//!
//! The canvas is given the font the text is drawn in (read from the page's computed style, so
//! CSS stays the one place it is chosen), and each character is measured on its own and summed
//! — which is exact because the stylesheet turns kerning and ligatures off for the document, so
//! the browser draws a row as the sum of its characters too. Measure with what you draw.

use quill_core::layout::{Metrics, printable};

/// [`Metrics`] in CSS pixels.
pub struct CanvasMetrics {
    /// `None` where there is no canvas — jsdom, in the smoke test — and every character is
    /// [`FALLBACK_ADVANCE`] wide.
    canvas: Option<web_sys::CanvasRenderingContext2d>,
    line_height: f32,
}

/// A width for a character when nothing can measure one. Never zero, so a caret placed by it is
/// still somewhere rather than a NaN.
pub const FALLBACK_ADVANCE: f32 = 8.0;

impl CanvasMetrics {
    pub fn new(canvas: Option<web_sys::CanvasRenderingContext2d>, line_height: f32) -> Self {
        CanvasMetrics {
            canvas,
            line_height: if line_height > 0.0 { line_height } else { 20.0 },
        }
    }

    fn width(&self, text: &str) -> f32 {
        match &self.canvas {
            Some(canvas) => canvas
                .measure_text(text)
                .map(|m| m.width() as f32)
                .unwrap_or(FALLBACK_ADVANCE),
            None => FALLBACK_ADVANCE * text.chars().count() as f32,
        }
    }
}

impl Metrics for CanvasMetrics {
    fn advances(&self, text: &str, out: &mut Vec<f32>) {
        let (printed, starts) = printable(text);
        let mut x = 0.0;
        for pair in starts.windows(2) {
            x += self.width(&printed[pair[0]..pair[1]]);
            out.push(x);
        }
    }

    fn line_height(&self) -> f32 {
        self.line_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_canvas_every_character_has_a_width() {
        let metrics = CanvasMetrics::new(None, 0.0);
        let mut out = Vec::new();
        metrics.advances("a\tb", &mut out);
        assert_eq!(out, [8.0, 40.0, 48.0], "a tab is four spaces wide");
        assert_eq!(metrics.line_height(), 20.0, "never zero");
    }
}
