// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! GDI: the fonts, the measuring, the painting — and `--render-to`'s frame with no window.
//!
//! **One engine measures and draws.** [`GdiMetrics`] answers the core's layout questions with
//! `GetTextExtentExPointW`, and [`paint`] draws each row with `ExtTextOutW` *given the advances
//! that same call produced* — so the caret the core places from the measurements is where the
//! ink is. GDI does not shape, which is the honest limit of this shell (no ligatures, no complex
//! scripts); it also means there is nothing to disagree with.
//!
//! The only file that creates a GDI object, and the only one besides `win.rs` with `unsafe`.

use quill_core::layout::{Metrics, shown};
use quill_core::{App, Frame, View};
use windows::Win32::Foundation::{RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::core::PCWSTR;

use crate::theme::{Geometry, Rgb, Theme, geometry, scale};

/// The document's font size, in points, before DPI scaling.
const POINTS: i32 = 11;

/// The fonts a frame is drawn with, for one DPI.
pub struct Fonts {
    pub text: HFONT,
    pub ui: HFONT,
    pub dpi: u32,
}

impl Fonts {
    /// Consolas for the document — fixed pitch matters more than the face, so a missing Consolas
    /// becomes another fixed-pitch modern face — and Segoe UI for the status bar.
    pub fn new(dpi: u32) -> Fonts {
        let make = |face: &str, pitch: u8| {
            let mut font = LOGFONTW {
                lfHeight: -(POINTS * dpi as i32 / 72),
                lfWeight: FW_NORMAL.0 as i32,
                lfCharSet: DEFAULT_CHARSET,
                lfOutPrecision: OUT_TT_PRECIS,
                lfQuality: CLEARTYPE_QUALITY,
                lfPitchAndFamily: pitch,
                ..Default::default()
            };
            let wide: Vec<u16> = face.encode_utf16().collect();
            font.lfFaceName[..wide.len()].copy_from_slice(&wide);
            unsafe { CreateFontIndirectW(&font) }
        };
        Fonts {
            text: make("Consolas", FIXED_PITCH.0 | FF_MODERN.0),
            ui: make("Segoe UI", VARIABLE_PITCH.0 | FF_SWISS.0),
            dpi,
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.text.into());
            let _ = DeleteObject(self.ui.into());
        }
    }
}

/// [`Metrics`] in device pixels, measured on a DC with the document font selected.
pub struct GdiMetrics {
    hdc: HDC,
    line_height: f32,
}

impl GdiMetrics {
    /// Measure on `hdc`, which must have the text font selected for as long as this lives.
    pub fn new(hdc: HDC) -> GdiMetrics {
        let mut tm = TEXTMETRICW::default();
        unsafe {
            let _ = GetTextMetricsW(hdc, &mut tm);
        }
        GdiMetrics {
            hdc,
            line_height: (tm.tmHeight + tm.tmExternalLeading).max(1) as f32,
        }
    }

    /// `text` as drawn, in UTF-16, with the cumulative advance after every unit.
    fn shape(&self, text: &str) -> (Vec<u16>, Vec<i32>, Vec<usize>) {
        let mut units = Vec::with_capacity(text.len());
        let mut ends = Vec::with_capacity(text.len());
        for c in text.chars() {
            units.extend(shown(c).encode_utf16());
            ends.push(units.len());
        }
        let mut dx = vec![0; units.len()];
        // Never with an empty string: an empty `Vec` has no allocation, so its pointer dangles,
        // and GDI reading from it was an access violation in grind (W3).
        if !units.is_empty() {
            let mut size = SIZE::default();
            unsafe {
                let _ = GetTextExtentExPointW(
                    self.hdc,
                    PCWSTR(units.as_ptr()),
                    units.len() as i32,
                    0,
                    None,
                    Some(dx.as_mut_ptr()),
                    &mut size,
                );
            }
        }
        (units, dx, ends)
    }

    fn digit_width(&self) -> i32 {
        let (_, dx, _) = self.shape("0");
        dx[0].max(1)
    }
}

impl Metrics for GdiMetrics {
    fn advances(&self, text: &str, out: &mut Vec<f32>) {
        let (_, dx, ends) = self.shape(text);
        out.extend(ends.iter().map(|&end| dx[end - 1] as f32));
    }

    fn line_height(&self) -> f32 {
        self.line_height
    }
}

/// A memory DC with the document font in it, for measuring outside a paint.
pub struct Measurer {
    hdc: HDC,
    previous: HGDIOBJ,
}

impl Measurer {
    pub fn new(fonts: &Fonts) -> Measurer {
        unsafe {
            let hdc = CreateCompatibleDC(None);
            let previous = SelectObject(hdc, fonts.text.into());
            Measurer { hdc, previous }
        }
    }

    pub fn metrics(&self) -> GdiMetrics {
        GdiMetrics::new(self.hdc)
    }
}

impl Drop for Measurer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc, self.previous);
            let _ = DeleteDC(self.hdc);
        }
    }
}

/// The client area's layout, for the fonts and document as they are now.
pub fn layout(metrics: &GdiMetrics, fonts: &Fonts, app: &App, width: i32, height: i32) -> Geometry {
    let status = metrics.line_height as i32 + 2 * scale(6, fonts.dpi);
    geometry(
        width,
        height,
        fonts.dpi,
        metrics.digit_width(),
        status,
        app.line_count(),
    )
}

/// The frame the core lays text out in.
pub fn frame<'a>(metrics: &'a GdiMetrics, g: &Geometry) -> Frame<'a> {
    Frame::new(g.text_width as f32, g.text_height as f32, metrics)
}

/// What the status bar says, left and right.
pub struct Status {
    pub left: String,
    pub right: String,
}

/// Draw everything onto `hdc`, which covers a client area `width` × `height`. Returns the view
/// it drew, so the window can put the system caret where the core said.
pub fn paint(
    hdc: HDC,
    fonts: &Fonts,
    app: &App,
    width: i32,
    height: i32,
    theme: &Theme,
    status: &Status,
) -> (Geometry, View) {
    unsafe {
        let previous = SelectObject(hdc, fonts.text.into());
        let metrics = GdiMetrics::new(hdc);
        let g = layout(&metrics, fonts, app, width, height);
        let view = app.view(&frame(&metrics, &g));

        fill(hdc, rect(0, 0, width, g.status_top), theme.background);
        fill(
            hdc,
            rect(0, g.status_top, width, height),
            theme.status_background,
        );
        SetBkMode(hdc, TRANSPARENT);

        let row_height = view.row_height as i32;
        for row in &view.rows {
            let top = g.text_top + row.top as i32;
            if top >= g.status_top {
                break;
            }
            if let Some(selection) = row.selection {
                let left = g.text_left + selection.from as i32;
                let right = g.text_left + selection.to.ceil() as i32;
                fill(
                    hdc,
                    rect(left, top, right, top + row_height),
                    theme.selection,
                );
            }
            if row.first {
                let number = (row.line + 1).to_string();
                let (units, dx, _) = metrics.shape(&number);
                let width = dx.last().copied().unwrap_or(0);
                SetTextColor(hdc, color(theme.gutter));
                text_out(hdc, g.gutter_right - width, top, &units, &dx);
            }
            SetTextColor(hdc, color(theme.text));
            let (units, dx, _) = metrics.shape(&row.text);
            text_out(hdc, g.text_left, top, &units, &dx);
        }

        SelectObject(hdc, fonts.ui.into());
        SetTextColor(hdc, color(theme.status_text));
        let pad = scale(10, fonts.dpi);
        let mut area = rect(pad, g.status_top, width - pad, height);
        for (text, align) in [(&status.left, DT_LEFT), (&status.right, DT_RIGHT)] {
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            if !wide.is_empty() {
                DrawTextW(
                    hdc,
                    &mut wide,
                    &mut area,
                    DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS | align,
                );
            }
        }
        SelectObject(hdc, previous);
        (g, view)
    }
}

/// Draw `units` at (`x`, `y`), each advanced by exactly what measuring it produced.
unsafe fn text_out(hdc: HDC, x: i32, y: i32, units: &[u16], cumulative: &[i32]) {
    if units.is_empty() {
        return;
    }
    let advances: Vec<i32> = cumulative
        .iter()
        .scan(0, |previous, &at| {
            let step = at - *previous;
            *previous = at;
            Some(step)
        })
        .collect();
    unsafe {
        let _ = ExtTextOutW(
            hdc,
            x,
            y,
            ETO_OPTIONS(0),
            None,
            PCWSTR(units.as_ptr()),
            units.len() as u32,
            Some(advances.as_ptr()),
        );
    }
}

fn color(rgb: Rgb) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(rgb.colorref())
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

unsafe fn fill(hdc: HDC, area: RECT, rgb: Rgb) {
    unsafe {
        let brush = CreateSolidBrush(color(rgb));
        FillRect(hdc, &area, brush);
        let _ = DeleteObject(brush.into());
    }
}

/// One frame with no window, no compositor and no display: a memory DC over a DIB section,
/// written as a BMP — 54 bytes of header in front of the section's own bits. `--render-to`.
///
/// Two renders of one document are byte-identical, which is how `artifacts.yml` checks the
/// drawing on `windows-latest`, and how a drawing refactor is proved to be one.
pub fn render_to_bmp(app: &App, width: i32, height: i32, theme: &Theme) -> Vec<u8> {
    let stride = ((width * 3 + 3) & !3) as usize;
    let size = stride * height as usize;
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        // Positive: bottom-up, the order a BMP file stores rows in, so the bits copy straight
        // out.
        biHeight: height,
        biPlanes: 1,
        biBitCount: 24,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    let fonts = Fonts::new(96);
    let caret_hidden = Status {
        left: format!("{}{}", app.title(), if app.is_dirty() { " *" } else { "" }),
        right: String::new(),
    };
    let mut out = Vec::with_capacity(54 + size);
    unsafe {
        let hdc = CreateCompatibleDC(None);
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(hdc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(hdc);
            return out;
        };
        let previous = SelectObject(hdc, bitmap.into());
        let (g, view) = paint(hdc, &fonts, app, width, height, theme, &caret_hidden);
        // The system caret is a window's; a frame with no window draws its own.
        if let Some(caret) = view.caret {
            let x = g.text_left + caret.x as i32;
            let y = g.text_top + caret.top as i32;
            fill(hdc, rect(x, y, x + 2, y + caret.height as i32), theme.text);
        }
        let _ = GdiFlush();

        out.extend_from_slice(b"BM");
        out.extend_from_slice(&((54 + size) as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&54u32.to_le_bytes());
        out.extend_from_slice(std::slice::from_raw_parts(
            &header as *const _ as *const u8,
            40,
        ));
        out.extend_from_slice(std::slice::from_raw_parts(bits as *const u8, size));

        SelectObject(hdc, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(hdc);
    }
    out
}
