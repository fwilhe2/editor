//! Points ↔ document coordinates.
//!
//! `ui_web/src/layout.rs` in egui's points instead of the browser's pixels, and for
//! the same reason: the arithmetic that places a caret and sizes a viewport is a pure
//! function of two measured numbers, and kept separate it can be tested with no
//! window, no display and no GPU.
//!
//! Nothing here touches egui beyond the numbers it measures.

use editor_core::Position;

/// The width of one character and the height of one line, in points.
///
/// Measured from the font every frame rather than cached: egui's zoom factor changes
/// both, and nothing announces it. Exactly the browser shell's `#probe` rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub char_width: f32,
    pub line_height: f32,
}

impl Metrics {
    /// Guards against a zero or nonsensical measurement.
    ///
    /// Before the first layout every rectangle is empty, and a zero line height would
    /// divide by zero and place the caret at NaN.
    pub fn new(char_width: f32, line_height: f32) -> Self {
        Metrics {
            char_width: if char_width.is_finite() && char_width > 0.0 {
                char_width
            } else {
                8.0
            },
            line_height: if line_height.is_finite() && line_height > 0.0 {
                line_height
            } else {
                16.0
            },
        }
    }

    /// How many lines fit in `height` points.
    ///
    /// The one number only the shell can know, and the core needs it to size the
    /// viewport. Never zero: the first frame is laid out before anything has a size,
    /// and asking the core for no lines would render a blank window that never
    /// recovers.
    pub fn visible_lines(&self, height: f32) -> u64 {
        let lines = (height / self.line_height).floor();
        if lines.is_finite() && lines >= 1.0 {
            lines as u64
        } else {
            1
        }
    }

    /// Where to draw the caret, relative to the top left of the first rendered line,
    /// for a cursor `row` lines below the top of the view.
    pub fn caret_offset(&self, row: u64, column: u64) -> (f32, f32) {
        (
            column as f32 * self.char_width,
            row as f32 * self.line_height,
        )
    }

    /// Which character a click landed on, `x`/`y` relative to the top left of the
    /// first rendered line and `start_line` being the line at the top of the view.
    ///
    /// Columns round rather than truncate: clicking the right half of a character
    /// puts the caret after it, which is what every text surface does.
    pub fn position_at(&self, x: f32, y: f32, start_line: u64) -> Position {
        let row = (y / self.line_height).floor().max(0.0) as u64;
        let column = (x / self.char_width).round().max(0.0) as u64;
        // The core clamps both into the document, so overshooting is fine.
        Position::new(start_line.saturating_add(row), column)
    }

    /// A wheel delta in lines, from egui's points.
    ///
    /// egui reports a *positive* `y` for scrolling up, because it describes how the
    /// content moves; a scroll offset counts down the document, so the sign flips
    /// here. A small trackpad delta still moves one line — a trackpad that never
    /// scrolled would read as broken.
    pub fn wheel_lines(&self, delta_y: f32) -> i64 {
        let lines = -delta_y / self.line_height;
        let rounded = lines.trunc() as i64;
        if rounded == 0 && delta_y != 0.0 {
            if lines > 0.0 {
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
        assert!(metrics.visible_lines(100.0) >= 1);
    }

    #[test]
    fn a_nonsense_measurement_is_replaced_rather_than_propagated() {
        // A NaN here would reach the caret's position and put it nowhere at all.
        let metrics = Metrics::new(f32::NAN, f32::INFINITY);
        assert_eq!(metrics, Metrics::new(0.0, 0.0));
    }

    #[test]
    fn viewport_height_is_at_least_one_line() {
        assert_eq!(METRICS.visible_lines(100.0), 5);
        assert_eq!(METRICS.visible_lines(99.0), 4);
        // The first frame, before the window has a size.
        assert_eq!(METRICS.visible_lines(0.0), 1);
        assert_eq!(METRICS.visible_lines(f32::NAN), 1);
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
    fn a_wheel_delta_scrolls_the_way_the_content_moves() {
        // egui's y is positive when scrolling up, and the offset counts downwards.
        assert_eq!(METRICS.wheel_lines(-100.0), 5);
        assert_eq!(METRICS.wheel_lines(100.0), -5);
        assert_eq!(METRICS.wheel_lines(0.0), 0);
    }

    #[test]
    fn a_tiny_trackpad_delta_still_scrolls() {
        assert_eq!(METRICS.wheel_lines(-2.0), 1);
        assert_eq!(METRICS.wheel_lines(2.0), -1);
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
