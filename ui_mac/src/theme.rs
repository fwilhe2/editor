// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Colours and the page's geometry, in points — the decisions about a frame that need no
//! AppKit, so they test on Linux.

/// An RGB colour, 0.0–1.0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub f64, pub f64, pub f64);

/// macOS's own text-view colours, light and dark: `textBackgroundColor`, `textColor`,
/// `selectedTextBackgroundColor` with the default accent, `secondaryLabelColor`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub background: Rgb,
    pub text: Rgb,
    pub gutter: Rgb,
    pub selection: Rgb,
}

impl Palette {
    pub fn light() -> Palette {
        Palette {
            background: Rgb(1.0, 1.0, 1.0),
            text: Rgb(0.0, 0.0, 0.0),
            gutter: Rgb(0.55, 0.55, 0.57),
            selection: Rgb(0.70, 0.84, 1.0),
        }
    }

    pub fn dark() -> Palette {
        Palette {
            background: Rgb(0.118, 0.118, 0.118),
            text: Rgb(1.0, 1.0, 1.0),
            gutter: Rgb(0.55, 0.55, 0.57),
            selection: Rgb(0.25, 0.40, 0.62),
        }
    }
}

/// Space around the text, in points.
pub const PADDING: f64 = 12.0;

/// Where the text starts in a view: past the line numbers, which are as wide as the document's
/// longest number (two digits at least) in `digit`-point digits.
pub fn text_origin(digit: f64, line_count: u64) -> (f64, f64) {
    let digits = line_count.to_string().len().max(2) as f64;
    (2.0 * PADDING + digits * digit, PADDING)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gutter_grows_with_the_document() {
        assert_eq!(text_origin(7.0, 9), (24.0 + 14.0, 12.0));
        assert_eq!(text_origin(7.0, 12_345), (24.0 + 35.0, 12.0));
    }
}
