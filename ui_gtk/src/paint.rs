// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Measuring and drawing — Pango on cairo, and nothing that needs a window.
//!
//! The same [`paint`] draws the widget on screen and `--render-to`'s PNG, so the image a test
//! compares is the image a person sees. And the same Pango layout that [`PangoMetrics`]
//! measures with is the one [`paint`] draws with: **one engine for both**, or the caret the
//! core places from the measurements lands where the ink is not.

use libadwaita::gtk::{cairo, pango};
use quill_core::layout::{self, Metrics};
use quill_core::{App, Frame};

/// Space around the text, in pixels.
pub const PADDING: f64 = 10.0;
/// The document's font. GNOME's monospace setting would be the better answer for the window
/// (a named gap); a fixed name keeps `--render-to` the same on every machine with the font.
pub const FONT: &str = "Monospace 11";

/// [`Metrics`] in Pango, in pixels.
pub struct PangoMetrics {
    layout: pango::Layout,
    line_height: f32,
    digit: f32,
}

impl PangoMetrics {
    pub fn new(context: &pango::Context) -> PangoMetrics {
        let layout = pango::Layout::new(context);
        layout.set_font_description(Some(&pango::FontDescription::from_string(FONT)));
        layout.set_text("0");
        let (_, logical) = layout.pixel_extents();
        PangoMetrics {
            line_height: logical.height().max(1) as f32,
            digit: logical.width().max(1) as f32,
            layout,
        }
    }

    /// The layout [`paint`] draws `text` with — the very object that measured it.
    fn shaped(&self, text: &str) -> &pango::Layout {
        self.layout.set_text(&layout::printable(text).0);
        &self.layout
    }
}

impl Metrics for PangoMetrics {
    fn advances(&self, text: &str, out: &mut Vec<f32>) {
        let (printed, starts) = layout::printable(text);
        self.layout.set_text(&printed);
        let scale = pango::SCALE as f32;
        // `index_to_pos(i)` is where the character starting at byte `i` begins — so at the start
        // of the next character, it is where this one ends. At the end of the string it is the
        // string's full width.
        out.extend(
            starts[1..]
                .iter()
                .map(|&end| self.layout.index_to_pos(end as i32).x() as f32 / scale),
        );
    }

    fn line_height(&self) -> f32 {
        self.line_height
    }
}

/// An RGB colour, 0.0–1.0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub f64, pub f64, pub f64);

/// The window's colours. libadwaita's own palette, light and dark, so the page matches the
/// header bar above it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub background: Rgb,
    pub text: Rgb,
    pub gutter: Rgb,
    pub selection: Rgb,
    pub caret: Rgb,
}

impl Palette {
    pub fn light() -> Palette {
        Palette {
            background: Rgb(1.0, 1.0, 1.0),
            text: Rgb(0.0, 0.0, 0.0),
            gutter: Rgb(0.6, 0.6, 0.6),
            // Adwaita's accent blue (#3584e4) at the opacity selections use.
            selection: Rgb(0.80, 0.88, 0.98),
            caret: Rgb(0.0, 0.0, 0.0),
        }
    }

    pub fn dark() -> Palette {
        Palette {
            background: Rgb(0.118, 0.118, 0.118),
            text: Rgb(1.0, 1.0, 1.0),
            gutter: Rgb(0.5, 0.5, 0.5),
            selection: Rgb(0.16, 0.30, 0.48),
            caret: Rgb(1.0, 1.0, 1.0),
        }
    }
}

/// Where the text starts inside the widget: past the line numbers and the padding.
pub fn text_origin(metrics: &PangoMetrics, app: &App) -> (f64, f64) {
    let digits = app.line_count().to_string().len().max(2) as f64;
    let gutter = digits * f64::from(metrics.digit) + 2.0 * PADDING;
    (gutter, PADDING)
}

/// The frame the core lays text out in, for a widget `width` × `height` pixels.
pub fn frame<'a>(metrics: &'a PangoMetrics, app: &App, width: f64, height: f64) -> Frame<'a> {
    let (x, y) = text_origin(metrics, app);
    Frame::new(
        (width - x - PADDING).max(1.0) as f32,
        (height - 2.0 * y).max(1.0) as f32,
        metrics,
    )
}

/// Draw the document as `app` holds it, into `cr`, as a widget `width` × `height` pixels.
///
/// Renders from the core's own scroll position and never scrolls: following the caret belongs
/// to input handling.
pub fn paint(
    cr: &cairo::Context,
    metrics: &PangoMetrics,
    app: &App,
    width: f64,
    height: f64,
    palette: &Palette,
    caret: bool,
) {
    let set = |Rgb(r, g, b): Rgb| cr.set_source_rgb(r, g, b);
    set(palette.background);
    let _ = cr.paint();

    let (left, top) = text_origin(metrics, app);
    let view = app.view(&frame(metrics, app, width, height));
    let line_height = f64::from(view.row_height);

    for row in &view.rows {
        let y = top + f64::from(row.top);
        if let Some(selection) = row.selection {
            set(palette.selection);
            cr.rectangle(
                left + f64::from(selection.from),
                y,
                f64::from(selection.to - selection.from),
                line_height,
            );
            let _ = cr.fill();
        }
        if row.first {
            set(palette.gutter);
            let number = (row.line + 1).to_string();
            let shaped = metrics.shaped(&number);
            let (_, logical) = shaped.pixel_extents();
            cr.move_to(left - PADDING - f64::from(logical.width()), y);
            pangocairo::functions::show_layout(cr, shaped);
        }
        set(palette.text);
        cr.move_to(left, y);
        pangocairo::functions::show_layout(cr, metrics.shaped(&row.text));
    }

    if caret && let Some(caret) = view.caret {
        set(palette.caret);
        cr.rectangle(
            (left + f64::from(caret.x)).round(),
            top + f64::from(caret.top),
            1.5,
            f64::from(caret.height),
        );
        let _ = cr.fill();
    }
}

/// Draw one frame into a PNG with no display: `--render-to`.
pub fn render_to_png(
    app: &App,
    width: i32,
    height: i32,
    palette: &Palette,
    out: &mut impl std::io::Write,
) -> Result<(), String> {
    let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, width, height)
        .map_err(|e| e.to_string())?;
    {
        let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
        let context = pangocairo::functions::create_context(&cr);
        let metrics = PangoMetrics::new(&context);
        paint(
            &cr,
            &metrics,
            app,
            f64::from(width),
            f64::from(height),
            palette,
            true,
        );
    }
    surface.write_to_png(out).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use libadwaita::gtk::pango::prelude::FontMapExt;

    fn metrics() -> PangoMetrics {
        PangoMetrics::new(&pangocairo::FontMap::new().create_context())
    }

    fn app(text: &str) -> App {
        let app = App::new();
        app.open_bytes(Some("t.txt"), text.as_bytes()).unwrap();
        app
    }

    fn png(app: &App, palette: &Palette) -> Vec<u8> {
        let mut out = Vec::new();
        render_to_png(app, 320, 200, palette, &mut out).unwrap();
        out
    }

    #[test]
    fn advances_are_cumulative_and_tabs_are_four_spaces() {
        let m = metrics();
        let mut out = Vec::new();
        m.advances("ab\t", &mut out);
        assert_eq!(out.len(), 3);
        assert!(out[0] > 0.0 && out[1] > out[0]);
        let mut spaces = Vec::new();
        m.advances("ab    ", &mut spaces);
        assert!((out[2] - spaces[5]).abs() < 0.01, "{out:?} vs {spaces:?}");
    }

    #[test]
    fn a_render_is_reproducible_byte_for_byte() {
        let app = app("hello\nworld, a line long enough to wrap in a narrow frame");
        assert_eq!(png(&app, &Palette::light()), png(&app, &Palette::light()));
    }

    #[test]
    fn the_dark_palette_is_a_different_frame() {
        let app = app("hello");
        assert_ne!(png(&app, &Palette::light()), png(&app, &Palette::dark()));
    }
}
