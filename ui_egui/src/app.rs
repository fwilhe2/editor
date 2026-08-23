//! One frame of the portable shell.
//!
//! Holds an `Arc<Editor>` and nothing else that matters: every pixel is derived from
//! the core each frame, which is what immediate mode makes natural and what the other
//! five shells have to arrange deliberately.
//!
//! **Stage 3**: the document is drawn, from `get_viewport` and nothing else. It is
//! *painted* rather than put in a widget, because [`egui::TextEdit`] owns a `String`
//! and would make the toolkit a second source of truth exactly as `GtkTextView` and
//! `contenteditable` would. There is no [`egui::ScrollArea`] for the same reason: it
//! would own a scroll position, and the core owns that.
//!
//! No key does anything yet — that is stage 4, and it is also where `follow_cursor`
//! gets called. See `doc/plan-egui-shell.md`.

use std::sync::Arc;

use editor_core::Editor;
use eframe::egui;

use crate::layout::Metrics;

/// What a document with no path would be called. Unreachable today — the binary
/// requires a file argument — but the name has to come from somewhere.
const UNTITLED: &str = "untitled.txt";

/// How wide the caret is drawn, in points.
const CARET_WIDTH: f32 = 2.0;

pub struct App {
    editor: Arc<Editor>,
    /// The last title handed to the window manager, so an unchanged one is not
    /// re-sent every frame. Presentation only: the core knows nothing about it.
    title: String,
}

impl App {
    pub fn new(editor: Arc<Editor>) -> Self {
        let title = Self::window_title(&editor);
        Self { editor, title }
    }

    /// The document's name, with a marker when it has unsaved changes.
    ///
    /// A dot rather than the trailing asterisk Windows favours or the "Edited" macOS
    /// puts in the proxy icon: this shell has no platform whose convention it could
    /// be following, and says so by picking one and using it everywhere.
    pub fn window_title(editor: &Editor) -> String {
        let name = editor
            .path()
            .map(|path| path.display().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| UNTITLED.to_string());

        if editor.is_dirty() {
            format!("• {name} — Editor")
        } else {
            format!("{name} — Editor")
        }
    }

    /// Keep the window's title in step with the document.
    fn sync_title(&mut self, ctx: &egui::Context) {
        let title = Self::window_title(&self.editor);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// The font the document is drawn in, taken from the style so that egui's zoom
    /// and any theme change are respected rather than fought.
    fn font(ui: &egui::Ui) -> egui::FontId {
        egui::TextStyle::Monospace.resolve(ui.style())
    }

    /// Measure the character grid.
    ///
    /// Re-measured every frame, because the zoom factor changes both numbers and
    /// nothing announces it — the same rule as the browser shell's hidden probe.
    fn metrics(ui: &egui::Ui, font: &egui::FontId) -> Metrics {
        let (char_width, line_height) = ui
            .ctx()
            .fonts_mut(|fonts| (fonts.glyph_width(font, ' '), fonts.row_height(font)));
        Metrics::new(char_width, line_height)
    }

    /// Draw the document.
    ///
    /// Painted, never put in a widget: [`egui::TextEdit`] owns a `String` and would
    /// be this toolkit's `GtkTextView` — a second document, which would win. Only the
    /// lines the core hands back are drawn, so the cost of a frame is bounded by the
    /// window rather than by the file.
    fn draw_document(&self, ui: &mut egui::Ui, font: &egui::FontId, metrics: Metrics) {
        let rect = ui.max_rect();
        let height = metrics.visible_lines(rect.height());
        // Claim the area as one widget. Painted text has no accessibility node of its
        // own — a screen reader, and the `egui_kittest` harness with it, would find an
        // empty window — so the surface carries the visible text as its label. Stage 4
        // needs this response anyway, to know where a click landed.
        let response = ui.allocate_rect(rect, egui::Sense::click());

        // Rendered from the core's stored offset, and `follow_cursor` is deliberately
        // not called here — that belongs to input handling in stage 4. A repaint that
        // scrolls would drag the view back to the caret every time.
        let start = self.editor.scroll_offset();
        let viewport = self
            .editor
            .get_viewport(start, start.saturating_add(height));

        let painter = ui.painter();
        let text_color = ui.visuals().text_color();
        for (row, line) in viewport.lines.iter().enumerate() {
            let (x, y) = metrics.caret_offset(row as u64, 0);
            painter.text(
                rect.min + egui::vec2(x, y),
                egui::Align2::LEFT_TOP,
                line,
                font.clone(),
                text_color,
            );
        }

        let cursor = viewport.cursor;
        let drawn = viewport.lines.len() as u64;
        if cursor.line >= viewport.start_line && cursor.line < viewport.start_line + drawn {
            let (x, y) = metrics.caret_offset(cursor.line - viewport.start_line, cursor.column);
            painter.rect_filled(
                egui::Rect::from_min_size(
                    rect.min + egui::vec2(x, y),
                    egui::vec2(CARET_WIDTH, metrics.line_height),
                ),
                0.0,
                ui.visuals().selection.bg_fill,
            );
        }
        // Scrolled out of view: there is nowhere honest to draw it.

        // What a screen reader announces, and what the harness queries. Built from
        // the same viewport that was painted, so the two cannot disagree.
        let announced = viewport.lines.join("\n");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, announced.clone())
        });
    }

    /// Everything the shell can say about the document without editing it.
    fn draw_status(&self, ui: &mut egui::Ui) {
        let cursor = self.editor.cursor();
        ui.horizontal(|ui| {
            ui.label(Self::window_title(&self.editor));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // +1 on both: the core is 0-based and stays that way, and this is the
                // only place in this shell that knows it.
                ui.label(format!(
                    "Ln {}, Col {} · {} lines · {} chars",
                    cursor.line + 1,
                    cursor.column + 1,
                    self.editor.line_count(),
                    self.editor.char_count(),
                ));
            });
        });
    }
}

impl eframe::App for App {
    /// Note the signature: 0.36 replaced `update(&mut self, ctx: &Context, …)` with
    /// this. Anything written against an older egui will not compile.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.sync_title(ui.ctx());

        let font = Self::font(ui);
        let metrics = Self::metrics(ui, &font);

        // The status line first: egui wants the central panel last, since it takes
        // whatever the other panels left. `Panel::bottom` is 0.36's replacement for
        // `TopBottomPanel`, which no longer exists.
        egui::Panel::bottom("status").show(ui, |ui| self.draw_status(ui));

        // Nothing here asks for another frame. A shell that repaints on a timer is
        // polling, and the core pushes: the notifier in `main.rs` is what wakes this
        // up. With the document untouched, this idles at zero frames a second.
        //
        // No `ScrollArea` either, for the same reason there is no `TextEdit`: it
        // would own a scroll position, and the core owns that.
        egui::CentralPanel::default().show(ui, |ui| self.draw_document(ui, &font, metrics));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_carries_the_document_name() {
        let editor = Editor::new();
        // No path yet, and nothing typed.
        assert_eq!(App::window_title(&editor), "untitled.txt — Editor");
    }

    #[test]
    fn the_title_marks_unsaved_changes() {
        let editor = Editor::new();
        editor.handle_input('x');
        assert!(editor.is_dirty());
        assert_eq!(App::window_title(&editor), "• untitled.txt — Editor");
    }

    /// Run passes until the context stops asking for another, or give up.
    ///
    /// It takes a few: the first passes load fonts and upload textures. A context
    /// that never settles would mean this shell repaints forever, which is the
    /// failure this helper exists to make visible.
    fn settle(ctx: &egui::Context) -> bool {
        for _ in 0..8 {
            // A pass hands back textures the caller is expected to upload, and
            // dropping them unapplied panics — the price of driving a context by
            // hand rather than through the harness stage 5 will use.
            let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
            output.textures_delta.clear();
            if !ctx.has_requested_repaint() {
                return true;
            }
        }
        false
    }

    #[test]
    fn an_idle_shell_stops_asking_to_be_repainted() {
        // The no-polling rule, made checkable: with nothing happening, the context
        // must go quiet. If this ever fails, something is repainting on a timer.
        let ctx = egui::Context::default();
        assert!(
            settle(&ctx),
            "the context never stopped requesting repaints"
        );
    }

    /// The document is painted, and painted text has no accessibility node — a
    /// screen reader, and the harness with it, would find an empty window. The
    /// surface carries the visible lines as its label instead, and this is what
    /// stops that from being silently undone.
    #[test]
    fn the_document_is_announced_to_the_accessibility_tree() {
        use egui_kittest::kittest::Queryable as _;

        let editor = Arc::new(Editor::new());
        editor.insert_text("alpha\nbeta");
        let app = App::new(editor);

        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            let font = App::font(ui);
            let metrics = App::metrics(ui, &font);
            app.draw_document(ui, &font, metrics);
        });
        harness.run();

        assert!(
            harness.query_by_label("alpha\nbeta").is_some(),
            "the visible lines are not reachable in the accessibility tree"
        );
    }

    #[test]
    fn a_core_notification_wakes_the_shell_up() {
        // The observer bridge itself, tested with no window: the notifier holds a
        // context, and `state_changed` is the only thing that makes an idle shell
        // draw again.
        use crate::Notifier;
        use editor_core::EditorObserver;

        let ctx = egui::Context::default();
        assert!(settle(&ctx));

        Notifier(ctx.clone()).state_changed();
        assert!(ctx.has_requested_repaint());
    }
}
