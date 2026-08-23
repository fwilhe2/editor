//! One frame of the portable shell.
//!
//! Holds an `Arc<Editor>` and nothing else that matters: every pixel is derived from
//! the core each frame, which is what immediate mode makes natural and what the other
//! five shells have to arrange deliberately.
//!
//! The document is drawn from `get_viewport` and nothing else, and it is *painted*
//! rather than put in a widget: [`egui::TextEdit`] owns a `String` and would make the
//! toolkit a second source of truth exactly as `GtkTextView` and `contenteditable`
//! would. There is no [`egui::ScrollArea`] for the same reason — it would own a scroll
//! position, and the core owns that.
//!
//! One ordering rule carries most of the weight here: **`follow_cursor` is called from
//! `handle_input` and nowhere else.** If the painting called it, a wheel scroll away
//! from the caret would snap straight back the moment the window redrew.

use std::sync::Arc;

use editor_core::Editor;
use eframe::egui;

use crate::keymap;
use crate::layout::{self, Metrics};

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
    /// The last thing the shell has to say. Presentation only, like the TUI's status
    /// line: the core neither knows nor cares.
    message: String,
    /// Whether quitting with unsaved changes has already been refused once. The one
    /// piece of shell state the architecture allows, and the TUI keeps the same flag.
    quit_warned: bool,
}

impl App {
    pub fn new(editor: Arc<Editor>) -> Self {
        let title = Self::window_title(&editor);
        Self {
            editor,
            title,
            message: String::new(),
            quit_warned: false,
        }
    }

    /// What the open document is called.
    fn document_name(editor: &Editor) -> String {
        editor
            .path()
            .map(|path| path.display().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| UNTITLED.to_string())
    }

    /// The document's name, with a marker when it has unsaved changes.
    ///
    /// A dot rather than the trailing asterisk Windows favours or the "Edited" macOS
    /// puts in the proxy icon: this shell has no platform whose convention it could
    /// be following, and says so by picking one and using it everywhere.
    pub fn window_title(editor: &Editor) -> String {
        let name = Self::document_name(editor);
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

    /// Handle everything the user did this frame.
    ///
    /// Runs inside the central panel, before anything is painted, because that is the
    /// only place the viewport's real height is known. Being a distinct step matters:
    /// **`follow_cursor` is called here and nowhere else.** If the painting called it,
    /// a wheel scroll away from the caret would snap straight back the moment the
    /// window redrew.
    fn handle_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        origin: egui::Pos2,
        metrics: Metrics,
        height: u64,
    ) {
        if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let local = pointer - origin;
                let position = metrics.position_at(local.x, local.y, self.editor.scroll_offset());
                // Placing the caret is core API, so clicking costs the architecture
                // nothing. A click lands inside the view, so it needs no follow-up.
                self.editor.set_cursor(position);
            }
        }

        let (events, modifiers, scroll) = ui.input(|input| {
            (
                input.events.clone(),
                input.modifiers,
                input.smooth_scroll_delta.y,
            )
        });

        // Straight into the core's scroll offset, so this shell scrolls by exactly
        // the same rule as every other one.
        let delta = metrics.wheel_lines(scroll);
        if delta != 0 {
            self.editor
                .set_scroll_offset(layout::scrolled_by(self.editor.scroll_offset(), delta));
        }

        let mut acted = false;
        for event in &events {
            let Some(action) = keymap::action_for(event, modifiers) else {
                continue;
            };
            // Any key that is not the second half of a quit takes the warning back,
            // exactly as the TUI's prompt does.
            if action != keymap::UiAction::Quit {
                self.quit_warned = false;
                self.message.clear();
            }
            acted = true;

            match keymap::apply(action, &self.editor) {
                Some(keymap::Request::Save) => self.save(),
                Some(keymap::Request::Quit) => self.quit(ui.ctx()),
                None => {}
            }
        }

        // Only after a key: a wheel scroll must be allowed to leave the caret behind.
        if acted {
            self.editor.follow_cursor(height);
        }
    }

    /// Write the document back to the file it came from.
    fn save(&mut self) {
        self.message = match self.editor.save_file() {
            Ok(()) => format!("Saved {}", Self::document_name(&self.editor)),
            Err(error) => format!("{error}"),
        };
    }

    /// Quit, refusing once if there is unsaved work.
    fn quit(&mut self, ctx: &egui::Context) {
        if self.editor.is_dirty() && !self.quit_warned {
            self.quit_warned = true;
            self.message = "Unsaved changes — press again to quit".to_string();
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// The window's close button, held to the same bargain as the quit shortcut.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        if self.editor.is_dirty() && !self.quit_warned {
            self.quit_warned = true;
            self.message = "Unsaved changes — close again to discard them".to_string();
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    /// Draw the document.
    ///
    /// Painted, never put in a widget: [`egui::TextEdit`] owns a `String` and would
    /// be this toolkit's `GtkTextView` — a second document, which would win. Only the
    /// lines the core hands back are drawn, so the cost of a frame is bounded by the
    /// window rather than by the file.
    fn draw_document(
        &self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        font: &egui::FontId,
        metrics: Metrics,
        height: u64,
    ) {
        // Rendered from the core's stored offset, and `follow_cursor` is deliberately
        // not called here — that belongs to `handle_input`. A repaint that scrolls
        // would drag the view back to the caret every time.
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
    ///
    /// Labels, not buttons, deliberately: a focusable widget here would take Enter and
    /// the arrow keys away from the document, since this shell reads raw events rather
    /// than owning a focused text widget.
    fn draw_status(&self, ui: &mut egui::Ui) {
        let cursor = self.editor.cursor();
        ui.horizontal(|ui| {
            ui.label(Self::window_title(&self.editor));
            if !self.message.is_empty() {
                ui.separator();
                ui.label(&self.message);
            }
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
                ui.separator();
                // Shown always, dimmed when the core says the stack is empty, so the
                // state of the history is visible without a menu to open.
                for (name, available) in [
                    ("redo", self.editor.can_redo()),
                    ("undo", self.editor.can_undo()),
                ] {
                    let text = egui::RichText::new(name);
                    ui.label(if available { text } else { text.weak() });
                }
            });
        });
    }

    /// One whole frame, given nothing but a [`egui::Ui`].
    ///
    /// Separate from the [`eframe::App`] impl on purpose: an `eframe::Frame` cannot be
    /// built outside eframe, and this is what lets `egui_kittest` drive the real shell
    /// — every key, click and repaint — with no window, no display and no GPU.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        self.sync_title(ui.ctx());
        self.handle_close_request(ui.ctx());

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
        egui::CentralPanel::default().show(ui, |ui| {
            let rect = ui.max_rect();
            let height = metrics.visible_lines(rect.height());
            // Claim the area as one widget. Painted text has no accessibility node of
            // its own — a screen reader, and the `egui_kittest` harness with it, would
            // find an empty window — so the surface carries the visible text as its
            // label, and the response is what locates a click.
            let response = ui.allocate_rect(rect, egui::Sense::click());

            // Input first, painting second, so a frame shows what the key just did
            // rather than what the document looked like before it.
            self.handle_input(ui, &response, rect.min, metrics, height);
            self.draw_document(ui, &response, rect, &font, metrics, height);
        });
    }
}

impl eframe::App for App {
    /// Note the signature: 0.36 replaced `update(&mut self, ctx: &Context, …)` with
    /// this. Anything written against an older egui will not compile.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
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

    /// The whole shell, driven with no display and no GPU.
    ///
    /// The document is painted, and painted text has no accessibility node — a screen
    /// reader, and the harness with it, would find an empty window — so the surface
    /// carries the visible lines as its label. That is what these queries read, which
    /// means a change that broke accessibility would also break the tests.
    fn shell(text: &str) -> (Arc<Editor>, egui_kittest::Harness<'static>) {
        let editor = Arc::new(Editor::new());
        editor.insert_text(text);
        editor.set_cursor(editor_core::Position::new(0, 0));

        let mut app = App::new(editor.clone());
        let mut harness = egui_kittest::Harness::new_ui(move |ui| app.frame(ui));
        harness.run();
        (editor, harness)
    }

    #[test]
    fn the_document_is_announced_to_the_accessibility_tree() {
        use egui_kittest::kittest::Queryable as _;

        let (_editor, harness) = shell("alpha\nbeta");
        assert!(
            harness.query_by_label("alpha\nbeta").is_some(),
            "the visible lines are not reachable in the accessibility tree"
        );
    }

    #[test]
    fn typing_reaches_the_core_and_comes_back_on_screen() {
        use egui_kittest::kittest::Queryable as _;

        let (editor, mut harness) = shell("");
        harness.event(egui::Event::Text("hi".to_string()));
        harness.run();

        assert_eq!(editor.text(), "hi");
        assert!(
            harness.query_by_label("hi").is_some(),
            "typed but not drawn"
        );
    }

    #[test]
    fn enter_and_backspace_edit_the_document() {
        let (editor, mut harness) = shell("");
        harness.event(egui::Event::Text("ab".to_string()));
        harness.key_press(egui::Key::Enter);
        harness.event(egui::Event::Text("c".to_string()));
        harness.run();
        assert_eq!(editor.text(), "ab\nc");

        harness.key_press(egui::Key::Backspace);
        harness.run();
        assert_eq!(editor.text(), "ab\n");
    }

    #[test]
    fn undo_and_redo_go_through_the_cores_history() {
        let (editor, mut harness) = shell("");
        harness.event(egui::Event::Text("x".to_string()));
        harness.run();
        assert_eq!(editor.text(), "x");

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(editor.text(), "");

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Y);
        harness.run();
        assert_eq!(editor.text(), "x");
    }

    #[test]
    fn a_shortcut_does_not_type_its_own_letter() {
        // The bug that has appeared in three shells here: command+S saving *and*
        // inserting an "s".
        let (editor, mut harness) = shell("");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn arrows_move_the_cursor_without_editing() {
        use editor_core::Position;

        let (editor, mut harness) = shell("ab\ncd");
        harness.key_press(egui::Key::ArrowDown);
        harness.key_press(egui::Key::ArrowRight);
        harness.run();

        assert_eq!(editor.cursor(), Position::new(1, 1));
        assert_eq!(editor.text(), "ab\ncd");
    }

    #[test]
    fn the_status_line_counts_from_one() {
        use egui_kittest::kittest::Queryable as _;

        // The core is 0-based; only the display adds one. This is the assertion that
        // would catch that conversion leaking into the wrong place.
        let (_editor, harness) = shell("ab");
        assert!(
            harness
                .query_by_label("Ln 1, Col 1 · 1 lines · 2 chars")
                .is_some(),
            "the status line does not read as expected"
        );
    }

    #[test]
    fn quitting_with_unsaved_changes_warns_before_it_obeys() {
        use egui_kittest::kittest::Queryable as _;

        let (editor, mut harness) = shell("x");
        assert!(editor.is_dirty());

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Q);
        harness.run();
        assert!(
            harness
                .query_by_label("Unsaved changes — press again to quit")
                .is_some(),
            "the first quit should have been refused, and said so"
        );
    }

    #[test]
    fn saving_writes_the_file_and_reports_it() {
        use egui_kittest::kittest::Queryable as _;

        let path = std::env::temp_dir().join(format!("edit-egui-{}.txt", std::process::id()));
        std::fs::write(&path, "old\n").unwrap();

        let editor = Arc::new(Editor::open(&path).unwrap());
        let mut app = App::new(editor.clone());
        let mut harness = egui_kittest::Harness::new_ui(move |ui| app.frame(ui));
        harness.run();

        harness.event(egui::Event::Text("new ".to_string()));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new old\n");
        assert!(!editor.is_dirty(), "a saved document is not dirty");
        assert!(
            harness
                .query_by_label(&format!("Saved {}", path.display()))
                .is_some(),
            "the save was not reported in the status line"
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn scrolling_does_not_drag_the_view_back_to_the_caret() {
        // The rule that `draw_document` never calls `follow_cursor`. A wheel scroll
        // moves the view away from the cursor, and repainting must leave it there.
        let (editor, mut harness) = shell("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12");
        editor.set_scroll_offset(5);
        harness.run();
        harness.run();
        assert_eq!(
            editor.scroll_offset(),
            5,
            "a repaint scrolled the view back to the caret"
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
