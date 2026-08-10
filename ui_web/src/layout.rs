//! Pixels ↔ document coordinates.
//!
//! The other half of this shell that can be tested without a browser. Everything
//! here is arithmetic over two numbers measured from the page — the width of one
//! character and the height of one line — which is all a monospaced text surface
//! needs to place a caret, size a viewport and answer "which character did the
//! pointer land on".

use editor_core::Position;

/// Font metrics, measured from a probe element that carries the same CSS as a
/// rendered line. Re-measured on every repaint, because zoom and font loading can
/// change them after the page is up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub char_width: f64,
    pub line_height: f64,
}

impl Metrics {
    /// Guards against a zero measurement, which happens when the element has not
    /// been laid out yet and would otherwise divide by zero.
    pub fn new(char_width: f64, line_height: f64) -> Self {
        Metrics {
            char_width: if char_width > 0.0 { char_width } else { 8.0 },
            line_height: if line_height > 0.0 { line_height } else { 16.0 },
        }
    }

    /// How many lines fit in `height_px`.
    ///
    /// The one number only the shell can know, and the core needs it to size the
    /// viewport. Never zero: before the first layout the surface has no height, and
    /// a zero-line viewport would render nothing at all.
    pub fn visible_lines(&self, height_px: f64) -> u64 {
        let lines = (height_px / self.line_height).floor();
        if lines.is_finite() && lines >= 1.0 {
            lines as u64
        } else {
            1
        }
    }

    /// Where to draw the caret for a cursor `row` lines below the top of the view.
    pub fn caret_offset(&self, row: u64, column: u64) -> (f64, f64) {
        (
            column as f64 * self.char_width,
            row as f64 * self.line_height,
        )
    }

    /// Which character the pointer landed on, `x`/`y` relative to the first
    /// rendered line and `start_line` being the line at the top of the view.
    ///
    /// Columns round rather than truncate: clicking the right half of a character
    /// puts the caret after it, which is what every text surface does.
    pub fn position_at(&self, x: f64, y: f64, start_line: u64) -> Position {
        let row = (y / self.line_height).floor().max(0.0) as u64;
        let column = (x / self.char_width).round().max(0.0) as u64;
        // The core clamps both into the document, so overshooting is fine.
        Position::new(start_line.saturating_add(row), column)
    }

    /// A wheel event in lines. `delta_mode` is the DOM's: 0 pixels, 1 lines, 2 pages.
    ///
    /// A small pixel delta still moves one line — a trackpad that never scrolled
    /// would read as broken.
    pub fn wheel_lines(&self, delta_y: f64, delta_mode: u32, page: u64) -> i64 {
        let lines = match delta_mode {
            1 => delta_y,
            2 => delta_y * page.max(1) as f64,
            _ => delta_y / self.line_height,
        };
        let rounded = lines.trunc() as i64;
        if rounded == 0 && delta_y != 0.0 {
            if delta_y > 0.0 {
                1
            } else {
                -1
            }
        } else {
            rounded
        }
    }
}

/// Apply a signed line delta to a scroll offset, staying inside the document.
pub fn scrolled_by(offset: u64, delta: i64) -> u64 {
    if delta >= 0 {
        offset.saturating_add(delta as u64)
    } else {
        offset.saturating_sub(delta.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const METRICS: Metrics = Metrics {
        char_width: 8.0,
        line_height: 20.0,
    };

    #[test]
    fn a_zero_measurement_never_reaches_the_arithmetic() {
        let metrics = Metrics::new(0.0, 0.0);
        assert!(metrics.char_width > 0.0 && metrics.line_height > 0.0);
        // Would be a division by zero if the guard were missing.
        assert!(metrics.visible_lines(100.0) >= 1);
    }

    #[test]
    fn viewport_height_is_at_least_one_line() {
        assert_eq!(METRICS.visible_lines(100.0), 5);
        assert_eq!(METRICS.visible_lines(99.0), 4);
        // Before the first layout the surface is 0px tall; rendering nothing then
        // would leave a blank page that never recovers.
        assert_eq!(METRICS.visible_lines(0.0), 1);
        assert_eq!(METRICS.visible_lines(f64::NAN), 1);
    }

    #[test]
    fn the_caret_sits_on_the_character_grid() {
        assert_eq!(METRICS.caret_offset(0, 0), (0.0, 0.0));
        assert_eq!(METRICS.caret_offset(2, 3), (24.0, 40.0));
    }

    #[test]
    fn clicks_land_on_the_nearest_character_boundary() {
        // Left half of the third character: the caret goes before it.
        assert_eq!(METRICS.position_at(17.0, 5.0, 0), Position::new(0, 2));
        // Right half: after it.
        assert_eq!(METRICS.position_at(23.0, 5.0, 0), Position::new(0, 3));
    }

    #[test]
    fn clicks_are_relative_to_the_scrolled_view() {
        // Second visible row while scrolled to line 10 is document line 11.
        assert_eq!(METRICS.position_at(0.0, 25.0, 10), Position::new(11, 0));
    }

    #[test]
    fn a_click_outside_the_lines_clamps_instead_of_wrapping() {
        // Negative coordinates would underflow an unsigned line number.
        assert_eq!(METRICS.position_at(-40.0, -30.0, 3), Position::new(3, 0));
    }

    #[test]
    fn wheel_deltas_convert_by_their_mode() {
        assert_eq!(METRICS.wheel_lines(100.0, 0, 10), 5); // pixels
        assert_eq!(METRICS.wheel_lines(3.0, 1, 10), 3); // lines
        assert_eq!(METRICS.wheel_lines(1.0, 2, 10), 10); // pages
        assert_eq!(METRICS.wheel_lines(-100.0, 0, 10), -5);
    }

    #[test]
    fn a_tiny_trackpad_delta_still_scrolls() {
        assert_eq!(METRICS.wheel_lines(2.0, 0, 10), 1);
        assert_eq!(METRICS.wheel_lines(-2.0, 0, 10), -1);
        assert_eq!(METRICS.wheel_lines(0.0, 0, 10), 0);
    }

    #[test]
    fn scrolling_stops_at_the_top() {
        assert_eq!(scrolled_by(4, 3), 7);
        assert_eq!(scrolled_by(4, -3), 1);
        assert_eq!(scrolled_by(1, -9), 0, "no wrapping past the first line");
        // The bottom is the core's business: set_scroll_offset clamps it.
        assert_eq!(scrolled_by(u64::MAX, 5), u64::MAX);
    }
}
