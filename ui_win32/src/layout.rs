//! Device pixels ↔ document coordinates.
//!
//! The sibling of `ui_web/src/layout.rs` and `ui_egui/src/layout.rs`, in GDI's
//! integer pixels rather than CSS pixels or egui points, and kept apart from the
//! window for exactly the same reason: placing a caret and sizing a viewport is a
//! pure function of two measured numbers, so isolated it can be tested with no
//! window, no display and — because nothing here is a Windows type — **no Windows**.
//!
//! That last part is the point. `ui_windows/` could not be compiled at all on the
//! machine this repository is developed on. Every assertion in this file runs on
//! `cargo test -p editor-win32` on any host.

use editor_core::Position;

/// One notch of a mouse wheel, as Windows reports it in `WM_MOUSEWHEEL`.
///
/// Named `WHEEL_DELTA` in `winuser.h`. Copied rather than imported so this module
/// stays free of Windows types; `app.rs` has a test that pins it against the real
/// constant when built for Windows.
pub const WHEEL_DELTA: i32 = 120;

/// What `SPI_GETWHEELSCROLLLINES` returns when the user asked for a page per notch.
///
/// `WHEEL_PAGESCROLL` in `winuser.h`, and a real setting rather than a curiosity —
/// it is what the "One screen at a time" mouse option sets.
pub const WHEEL_PAGESCROLL: u32 = u32::MAX;

/// The width of one character and the height of one line, in device pixels.
///
/// Measured from the font with `GetTextMetricsW` rather than assumed, and
/// re-measured whenever the font is rebuilt — which is every `WM_DPICHANGED`,
/// because a window dragged to a different monitor gets a different pixel grid and
/// nothing else announces it. Same rule as the browser shell's `#probe`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    pub char_width: i32,
    pub line_height: i32,
}

impl Metrics {
    /// Guards against a zero or negative measurement.
    ///
    /// A window gets `WM_SIZE` before it has a font, and a zero line height would
    /// divide by zero on the first paint. The fallbacks are a plausible 10x20 grid
    /// rather than anything meaningful — they exist to keep the arithmetic total.
    pub fn new(char_width: i32, line_height: i32) -> Self {
        Metrics {
            char_width: if char_width > 0 { char_width } else { 10 },
            line_height: if line_height > 0 { line_height } else { 20 },
        }
    }

    /// How many lines fit in `height` pixels.
    ///
    /// The one number only the shell can know, and the core needs it to size the
    /// viewport. Never zero: a window is created before it is sized, and asking the
    /// core for no lines would paint a blank window that never recovers.
    pub fn visible_lines(&self, height: i32) -> u64 {
        let lines = height / self.line_height;
        if lines >= 1 {
            lines as u64
        } else {
            1
        }
    }

    /// Where to draw text or the caret, relative to the top left of the first
    /// rendered line, for a cursor `row` lines below the top of the view.
    pub fn caret_offset(&self, row: u64, column: u64) -> (i32, i32) {
        (
            pixels(column, self.char_width),
            pixels(row, self.line_height),
        )
    }

    /// Which character a click landed on, `x`/`y` relative to the top left of the
    /// first rendered line and `start_line` being the line at the top of the view.
    ///
    /// Columns round rather than truncate: clicking the right half of a character
    /// puts the caret after it, which is what every text surface on Windows does.
    pub fn position_at(&self, x: i32, y: i32, start_line: u64) -> Position {
        let row = if y > 0 {
            (y / self.line_height) as u64
        } else {
            0
        };
        let column = if x > 0 {
            ((x + self.char_width / 2) / self.char_width) as u64
        } else {
            0
        };
        // The core clamps both into the document, so overshooting is fine.
        Position::new(start_line.saturating_add(row), column)
    }
}

/// `cells` of `size` pixels each, clamped into GDI's signed 32-bit coordinates.
///
/// Saturating in `u64` is not enough on its own: `u64::MAX as i32` is `-1`, which
/// would draw the caret of a very long line at the left edge rather than off the
/// right one. Clamping before the cast is what keeps it off-screen, which is wrong
/// but survivable — wrapping would put it in the middle of the text.
fn pixels(cells: u64, size: i32) -> i32 {
    cells.saturating_mul(size as u64).min(i32::MAX as u64) as i32
}

/// How many lines one notch of the wheel should move.
///
/// Windows does not decide this for the application: `SPI_GETWHEELSCROLLLINES` is
/// a user setting, three by default, and honouring it is the difference between a
/// window that scrolls like the rest of the desktop and one that does not. The
/// sentinel [`WHEEL_PAGESCROLL`] means a screenful, which is why this needs to know
/// how tall the view is.
///
/// Zero is a legitimate setting — it means the wheel does not scroll at all.
pub fn wheel_step(lines_per_notch: u32, page: u64) -> u64 {
    if lines_per_notch == WHEEL_PAGESCROLL {
        page.max(1)
    } else {
        lines_per_notch as u64
    }
}

/// A `WM_MOUSEWHEEL` delta in lines of the document.
///
/// Windows reports a *positive* delta for a wheel pushed away from the user, which
/// scrolls the view up; a scroll offset counts down the document, so the sign flips
/// here. The same flip as the browser and egui shells, arriving from the opposite
/// convention.
///
/// A high-precision wheel or a trackpad sends deltas smaller than one notch, and
/// those still move a line rather than nothing — the same call `ui_egui/` makes, and
/// for the same reason: a trackpad that never scrolled would read as broken. The
/// cost is that very fine scrolling is coarser than the hardware allows.
pub fn wheel_lines(delta: i32, step: u64) -> i64 {
    if delta == 0 || step == 0 {
        return 0;
    }
    let notches = delta as i64 * step as i64 / WHEEL_DELTA as i64;
    if notches != 0 {
        -notches
    } else if delta > 0 {
        -1
    } else {
        1
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
        char_width: 8,
        line_height: 20,
    };

    #[test]
    fn a_zero_measurement_never_reaches_the_arithmetic() {
        // A window is sized before it has a font; dividing by that would panic.
        let metrics = Metrics::new(0, 0);
        assert!(metrics.char_width > 0 && metrics.line_height > 0);
        assert!(metrics.visible_lines(100) >= 1);
    }

    #[test]
    fn a_negative_measurement_is_replaced_rather_than_propagated() {
        assert_eq!(Metrics::new(-4, -9), Metrics::new(0, 0));
    }

    #[test]
    fn viewport_height_is_at_least_one_line() {
        assert_eq!(METRICS.visible_lines(100), 5);
        assert_eq!(METRICS.visible_lines(99), 4);
        // Before the first WM_SIZE, and when the status bar is taller than the window.
        assert_eq!(METRICS.visible_lines(0), 1);
        assert_eq!(METRICS.visible_lines(-40), 1);
    }

    #[test]
    fn the_caret_sits_on_the_character_grid() {
        assert_eq!(METRICS.caret_offset(0, 0), (0, 0));
        assert_eq!(METRICS.caret_offset(2, 3), (24, 40));
    }

    #[test]
    fn a_very_long_line_does_not_wrap_the_caret_round() {
        // u64 columns times a pixel width overflows i32 long before it overflows
        // u64. Saturating keeps the caret off-screen to the right, which is wrong
        // but survivable; wrapping would draw it in the middle of the text.
        let (x, _) = METRICS.caret_offset(0, u64::MAX);
        assert!(x > 0);
    }

    #[test]
    fn clicks_land_on_the_nearest_character_boundary() {
        // Left half of the third character: the caret goes before it.
        assert_eq!(METRICS.position_at(17, 5, 0), Position::new(0, 2));
        // Right half: after it.
        assert_eq!(METRICS.position_at(21, 5, 0), Position::new(0, 3));
    }

    #[test]
    fn clicks_are_relative_to_the_scrolled_view() {
        // Second visible row while scrolled to line 10 is document line 11.
        assert_eq!(METRICS.position_at(0, 25, 10), Position::new(11, 0));
    }

    #[test]
    fn a_click_outside_the_lines_clamps_instead_of_wrapping() {
        // A click can arrive with negative coordinates while the mouse is captured,
        // and a negative line number would underflow into the end of the document.
        assert_eq!(METRICS.position_at(-40, -30, 3), Position::new(3, 0));
    }

    #[test]
    fn the_wheel_honours_the_users_own_setting() {
        // Three lines a notch is the Windows default, not a number this shell picks.
        assert_eq!(wheel_step(3, 20), 3);
        assert_eq!(wheel_step(1, 20), 1);
        // "One screen at a time" in the mouse control panel.
        assert_eq!(wheel_step(WHEEL_PAGESCROLL, 20), 20);
        // A page is never zero, or the wheel would stop working on a tiny window.
        assert_eq!(wheel_step(WHEEL_PAGESCROLL, 0), 1);
    }

    #[test]
    fn a_wheel_delta_scrolls_the_way_the_content_moves() {
        // Windows' delta is positive away from the user, which moves the view up;
        // the offset counts downwards, so the sign flips.
        assert_eq!(wheel_lines(WHEEL_DELTA, 3), -3);
        assert_eq!(wheel_lines(-WHEEL_DELTA, 3), 3);
        assert_eq!(wheel_lines(2 * WHEEL_DELTA, 3), -6);
        assert_eq!(wheel_lines(0, 3), 0);
    }

    #[test]
    fn a_wheel_the_user_disabled_does_not_scroll() {
        // SPI_GETWHEELSCROLLLINES really can be zero, and it means what it says.
        assert_eq!(wheel_lines(WHEEL_DELTA, 0), 0);
    }

    #[test]
    fn a_high_precision_wheel_still_scrolls() {
        // A trackpad sends fractions of a notch. Rounding them to zero would look
        // like broken hardware.
        assert_eq!(wheel_lines(8, 3), -1);
        assert_eq!(wheel_lines(-8, 3), 1);
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
