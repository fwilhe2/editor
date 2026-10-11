// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The document on a `CGContext` — the whole of this shell's own drawing.
//!
//! **One path, two callers**: the view's `drawRect:` hands [`paint`] the context AppKit gave it,
//! and `--render-to` hands it a bitmap from [`render_to_png`] with no application object and no
//! window server. Both are **flipped** — origin top left, y down — which a flipped `NSView` gets
//! for free and the bitmap sets up; CoreText sets glyphs upright in an unflipped space, so the
//! text matrix is flipped back once per frame.

use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextGetBytesPerRow, CGBitmapContextGetData, CGColorSpace,
    CGContext, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use quill_core::{App, Frame, View};

use crate::metrics::CoreText;
use crate::theme::{Palette, Rgb, text_origin};

fn rect(x: f64, y: f64, w: f64, h: f64) -> CGRect {
    CGRect::new(CGPoint::new(x, y), CGSize::new(w, h))
}

fn fill(context: &CGContext, Rgb(r, g, b): Rgb) {
    CGContext::set_rgb_fill_color(Some(context), r, g, b, 1.0);
}

/// Where the text starts in a view.
pub fn origin(text: &CoreText, app: &App) -> (f64, f64) {
    text_origin(text.digit(), app.line_count())
}

/// The frame the core lays text out in, for a view `width` × `height` points.
pub fn frame<'a>(text: &'a CoreText, app: &App, width: f64, height: f64) -> Frame<'a> {
    let (x, y) = origin(text, app);
    Frame::new(
        (width - x - crate::theme::PADDING).max(1.0) as f32,
        (height - 2.0 * y).max(1.0) as f32,
        text,
    )
}

/// Draw the document into `context`, flipped, as a view `width` × `height` points. Returns the
/// view it drew, so the caller can tell an input method where the caret is.
pub fn paint(
    context: &CGContext,
    text: &CoreText,
    app: &App,
    width: f64,
    height: f64,
    palette: &Palette,
    caret: bool,
) -> View {
    let cx = Some(context);
    fill(context, palette.background);
    CGContext::fill_rect(cx, rect(0.0, 0.0, width, height));
    CGContext::set_text_matrix(
        cx,
        CGAffineTransform {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            tx: 0.0,
            ty: 0.0,
        },
    );

    let (left, top) = origin(text, app);
    let view = app.view(&frame(text, app, width, height));
    let row_height = f64::from(view.row_height);
    let ascent: CGFloat = text.ascent();
    for row in &view.rows {
        let y = top + f64::from(row.top);
        if let Some(selection) = row.selection {
            fill(context, palette.selection);
            CGContext::fill_rect(
                cx,
                rect(
                    left + f64::from(selection.from),
                    y,
                    f64::from(selection.to - selection.from),
                    row_height,
                ),
            );
        }
        if row.first {
            let number = text.line(&(row.line + 1).to_string());
            // SAFETY: null out-pointers are documented as "not wanted".
            let width = unsafe {
                number.typographic_bounds(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            fill(context, palette.gutter);
            CGContext::set_text_position(cx, left - crate::theme::PADDING - width, y + ascent);
            // SAFETY: the context is live for the call.
            unsafe { number.draw(context) };
        }
        fill(context, palette.text);
        CGContext::set_text_position(cx, left, y + ascent);
        // SAFETY: the context is live for the call.
        unsafe { text.line(&row.text).draw(context) };
    }

    if caret && let Some(at) = view.caret {
        fill(context, palette.text);
        CGContext::fill_rect(
            cx,
            rect(
                (left + f64::from(at.x)).round(),
                top + f64::from(at.top),
                1.5,
                f64::from(at.height),
            ),
        );
    }
    view
}

/// One frame into a PNG with no window server: a `CGBitmapContext` in sRGB, flipped before
/// [`paint`] runs, so `paint` is the same function a view calls. `--render-to`.
pub fn render_to_png(
    app: &App,
    width: f64,
    height: f64,
    palette: &Palette,
) -> Result<Vec<u8>, String> {
    let (w, h) = (width.round() as usize, height.round() as usize);
    // SAFETY: the name is one of CoreGraphics' exported constants.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or("no sRGB colour space")?;
    // SAFETY: a null data pointer asks CoreGraphics to allocate the pixels, and a stride of zero
    // asks it to choose; both are documented.
    let context = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            w,
            h,
            8,
            0,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }
    .ok_or("CGBitmapContextCreate returned nothing")?;
    let cx = Some(&*context);
    CGContext::translate_ctm(cx, 0.0, h as CGFloat);
    CGContext::scale_ctm(cx, 1.0, -1.0);
    let text = CoreText::new();
    paint(&context, &text, app, width, height, palette, true);

    let stride = CGBitmapContextGetBytesPerRow(cx);
    let data = CGBitmapContextGetData(cx) as *const u8;
    if data.is_null() {
        return Err("the bitmap context has no pixels".into());
    }
    // SAFETY: the context owns `stride * h` bytes at `data` and outlives this read; its rows are
    // stored top first.
    let pixels = unsafe { std::slice::from_raw_parts(data, stride * h) };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in pixels.chunks_exact(stride) {
        // The frame filled its whole rectangle first, so alpha is 255 and premultiplied is
        // straight.
        rgba.extend_from_slice(&row[..w * 4]);
    }
    Ok(crate::png::encode(w as u32, h as u32, &rgba))
}
