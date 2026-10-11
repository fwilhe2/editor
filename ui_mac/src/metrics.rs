// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Text measured by CoreText — the core's `Metrics`, answered by a shaping engine.
//!
//! Two halves. [`fold`] is portable: CoreText answers "where is the caret at this UTF-16 index",
//! the trait asks "how far after each `char`", and turning one into the other is arithmetic that
//! tests on any host. `CoreText` is the Mac's, and it is **also what draws**: `paint.rs` sets
//! each row with a `CTLine` made the same way as the one measured, so the caret and the ink
//! cannot disagree.

/// The cumulative advance after each character of `text`, appended to `out`, from `offset_at` —
/// the caret's offset at a UTF-16 index, which is what `CTLineGetOffsetForStringIndex` answers.
///
/// A `char` is one UTF-16 unit or two, and its advance is the offset after its **last** unit. A
/// combining mark can answer an offset before the one ahead of it — CoreText gives a caret stop
/// per cluster, not per character — and the trait promises a non-decreasing sequence, so each
/// answer is at least the last.
pub fn fold(text: &str, offset_at: impl Fn(usize) -> f64, out: &mut Vec<f32>) {
    let mut units = 0;
    let mut last = 0.0f32;
    for c in text.chars() {
        units += c.len_utf16();
        last = last.max(offset_at(units) as f32);
        out.push(last);
    }
}

/// The document's font size, in points.
pub const POINTS: f64 = 13.0;

#[cfg(target_os = "macos")]
pub use mac::CoreText;

#[cfg(target_os = "macos")]
mod mac {
    use objc2_core_foundation::{
        CFAttributedString, CFBoolean, CFDictionary, CFIndex, CFRetained, CFString, CFType, CGFloat,
    };
    use objc2_core_text::{
        CTFont, CTFontUIFontType, CTLine, kCTFontAttributeName,
        kCTForegroundColorFromContextAttributeName,
    };
    use quill_core::layout::{Metrics, printable};

    use super::{POINTS, fold};

    /// The user's fixed-pitch font, and lines set in it.
    pub struct CoreText {
        font: CFRetained<CTFont>,
    }

    impl CoreText {
        pub fn new() -> CoreText {
            // SAFETY: a null language means the user's own, which the UI fonts follow.
            let font = unsafe {
                CTFont::new_ui_font_for_language(CTFontUIFontType::UserFixedPitch, POINTS, None)
            }
            .expect("the fixed-pitch UI font exists on every macOS");
            CoreText { font }
        }

        /// `text` as drawn ([`printable`]), set in this font, taking its colour from the context
        /// — the line [`Metrics::advances`] measured and `paint.rs` draws.
        pub fn line(&self, text: &str) -> CFRetained<CTLine> {
            // SAFETY: both keys are constants CoreText exports for exactly these attributes.
            let (font_key, from_context): (&CFString, &CFString) = unsafe {
                (
                    kCTFontAttributeName,
                    kCTForegroundColorFromContextAttributeName,
                )
            };
            let yes = CFBoolean::new(true);
            let values: [&CFType; 2] = [self.font.as_ref(), yes.as_ref()];
            let attributes =
                CFDictionary::<CFString, CFType>::from_slices(&[font_key, from_context], &values);
            let string = CFString::from_str(&printable(text).0);
            // SAFETY: keys that are strings and values a font and a boolean, the shape CoreText
            // documents for an attributed string's attributes.
            let attributed = unsafe {
                CFAttributedString::new(None, Some(&string), Some(attributes.as_opaque()))
            }
            .expect("an attributed string from a string and two attributes");
            // SAFETY: the attributed string is valid for the call.
            unsafe { CTLine::with_attributed_string(&attributed) }
        }

        /// How far below a row's top its baseline is.
        pub fn ascent(&self) -> CGFloat {
            // SAFETY: an ordinary read of a live font's metrics.
            unsafe { self.font.ascent() }
        }

        /// The width of one digit — what the line numbers' gutter is measured in.
        pub fn digit(&self) -> f64 {
            let mut out = Vec::new();
            self.advances("0", &mut out);
            f64::from(out[0])
        }
    }

    impl Metrics for CoreText {
        fn advances(&self, text: &str, out: &mut Vec<f32>) {
            let (printed, starts) = printable(text);
            let line = self.line(text);
            // CoreText answers in UTF-16 units of the *printed* string; fold it per printed
            // character, then keep the advance at each original character's last printed one.
            let mut per_printed = Vec::new();
            fold(
                &printed,
                // SAFETY: a null secondary offset is documented as allowed.
                |units| unsafe {
                    line.offset_for_string_index(units as CFIndex, std::ptr::null_mut())
                },
                &mut per_printed,
            );
            let mut chars = 0;
            for pair in starts.windows(2) {
                chars += printed[pair[0]..pair[1]].chars().count();
                out.push(per_printed[chars - 1]);
            }
        }

        fn line_height(&self) -> f32 {
            // SAFETY: ordinary reads of a live font's metrics.
            let height = unsafe { self.font.ascent() + self.font.descent() + self.font.leading() };
            height.ceil() as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Offsets as CoreText gives them for `a😀b`: the emoji is two UTF-16 units and one stop.
    #[test]
    fn a_character_outside_the_bmp_is_one_advance() {
        let offsets = [0.0, 10.0, 10.0, 34.0, 44.0];
        let mut out = Vec::new();
        fold("a\u{1f600}b", |units| offsets[units], &mut out);
        assert_eq!(out, [10.0, 34.0, 44.0]);
    }

    #[test]
    fn a_combining_mark_adds_nothing_and_nothing_goes_backwards() {
        let offsets = [0.0, 0.0, 9.0];
        let mut out = Vec::new();
        fold("e\u{301}", |units| offsets[units], &mut out);
        assert_eq!(out, [0.0, 9.0]);
    }
}
