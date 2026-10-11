// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The window's colours, and the pixel arithmetic that does not need a window.
//!
//! Chosen per theme rather than read with `GetSysColor`: the classic system colours never went
//! dark — `COLOR_WINDOW` is white on a dark Windows 11 — so reading them gives a white page under
//! a dark title bar. These are Windows 11's own surface values.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// GDI's `COLORREF` byte order: `0x00BBGGRR`, not the `0xRRGGBB` everyone writes by hand.
    pub fn colorref(self) -> u32 {
        u32::from(self.0) | (u32::from(self.1) << 8) | (u32::from(self.2) << 16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub background: Rgb,
    pub text: Rgb,
    pub gutter: Rgb,
    pub selection: Rgb,
    pub status_background: Rgb,
    pub status_text: Rgb,
}

impl Theme {
    pub fn light() -> Theme {
        Theme {
            background: Rgb(0xFF, 0xFF, 0xFF),
            text: Rgb(0x1A, 0x1A, 0x1A),
            gutter: Rgb(0x8A, 0x8A, 0x8A),
            selection: Rgb(0xCC, 0xE4, 0xF7),
            status_background: Rgb(0xF3, 0xF3, 0xF3),
            status_text: Rgb(0x44, 0x44, 0x44),
        }
    }

    pub fn dark() -> Theme {
        Theme {
            background: Rgb(0x20, 0x20, 0x20),
            text: Rgb(0xE4, 0xE4, 0xE4),
            gutter: Rgb(0x80, 0x80, 0x80),
            selection: Rgb(0x26, 0x4F, 0x78),
            status_background: Rgb(0x2B, 0x2B, 0x2B),
            status_text: Rgb(0xB0, 0xB0, 0xB0),
        }
    }
}

/// Where everything goes in a client area, in device pixels. Pure arithmetic over two measured
/// numbers, so it is tested here rather than discovered on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// The right edge of the line numbers.
    pub gutter_right: i32,
    pub text_left: i32,
    pub text_top: i32,
    pub text_width: i32,
    pub text_height: i32,
    pub status_top: i32,
}

/// Lay a client area `width` × `height` out: line numbers, text, and a status bar
/// `status_height` tall at the bottom.
pub fn geometry(
    width: i32,
    height: i32,
    dpi: u32,
    digit_width: i32,
    status_height: i32,
    line_count: u64,
) -> Geometry {
    let pad = scale(10, dpi);
    let digits = line_count.to_string().len().max(2) as i32;
    let gutter_right = pad + digits * digit_width;
    let text_left = gutter_right + pad;
    let status_top = (height - status_height).max(0);
    Geometry {
        gutter_right,
        text_left,
        text_top: pad,
        text_width: (width - text_left - pad).max(1),
        text_height: (status_top - 2 * pad).max(1),
        status_top,
    }
}

/// A length written at 96 DPI, at `dpi`.
pub fn scale(value: i32, dpi: u32) -> i32 {
    (value * dpi as i32 / 96).max(1)
}

/// The rows a wheel turn of `delta` moves, given the user's lines-per-notch setting
/// (`SPI_GETWHEELSCROLLLINES`): a notch is 120, and a high-resolution wheel sends less.
/// `remainder` carries what did not make a whole row to the next turn.
pub fn wheel_rows(delta: i32, lines_per_notch: u32, remainder: &mut i32) -> i64 {
    let total = *remainder + delta * lines_per_notch as i32;
    let rows = total / 120;
    *remainder = total % 120;
    // A wheel turned *towards* the user (negative delta) scrolls down.
    -i64::from(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_is_bgr() {
        assert_eq!(Rgb(0x11, 0x22, 0x33).colorref(), 0x0033_2211);
    }

    #[test]
    fn a_notch_is_three_rows_and_fractions_add_up() {
        let mut rest = 0;
        assert_eq!(wheel_rows(-120, 3, &mut rest), 3);
        assert_eq!(wheel_rows(40, 3, &mut rest), -1);
        assert_eq!(wheel_rows(20, 3, &mut rest), 0);
        assert_eq!(wheel_rows(20, 3, &mut rest), -1);
        assert_eq!(rest, 0);
    }

    #[test]
    fn the_status_bar_sits_at_the_bottom_and_the_text_beside_the_numbers() {
        let g = geometry(800, 600, 96, 8, 30, 5);
        assert_eq!(g.status_top, 570);
        assert_eq!(g.gutter_right, 10 + 2 * 8, "two digits at least");
        assert_eq!(g.text_left, 36);
        assert_eq!(g.text_width, 800 - 36 - 10);
        let wide = geometry(800, 600, 96, 8, 30, 12_345);
        assert_eq!(wide.gutter_right, 10 + 5 * 8);
    }

    #[test]
    fn lengths_scale_with_dpi() {
        assert_eq!(scale(10, 96), 10);
        assert_eq!(scale(10, 144), 15);
        assert_eq!(scale(0, 96), 1, "never zero");
    }
}
