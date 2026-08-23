//! Points ↔ document coordinates.
//!
//! `ui_web/src/layout.rs` in egui's points instead of the browser's pixels, and for
//! the same reason: the arithmetic that places a caret and sizes a viewport is a pure
//! function of two measured numbers, and kept separate it can be tested with no
//! window, no display and no GPU.
//!
//! Only what stage 3 needs lives here. Hit testing and wheel deltas arrive with the
//! input handling in stage 4.

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
}
