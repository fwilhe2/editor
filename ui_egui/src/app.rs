//! One frame of the portable shell.
//!
//! Holds an `Arc<Editor>` and nothing else that matters: every pixel is derived from
//! the core each frame, which is what immediate mode makes natural and what the other
//! five shells have to arrange deliberately.
//!
//! **Stage 2**: the window and the observer are wired, and the title tracks the
//! document. The text itself is drawn in stage 3 — from `get_viewport`, painted rather
//! than put in a widget, because [`egui::TextEdit`] owns a `String` and would make the
//! toolkit a second source of truth exactly as `GtkTextView` and `contenteditable`
//! would. See `doc/plan-egui-shell.md`.

use std::sync::Arc;

use editor_core::Editor;
use eframe::egui;

/// What a document with no path would be called. Unreachable today — the binary
/// requires a file argument — but the name has to come from somewhere.
const UNTITLED: &str = "untitled.txt";

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
}

impl eframe::App for App {
    /// Note the signature: 0.36 replaced `update(&mut self, ctx: &Context, …)` with
    /// this. Anything written against an older egui will not compile.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.sync_title(ui.ctx());

        // Nothing here asks for another frame. A shell that repaints on a timer is
        // polling, and the core pushes: the notifier in `main.rs` is what wakes this
        // up. With the document untouched, this should idle at zero frames a second.
        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} — {} lines, {} characters\n\n\
                         no renderer yet: stage 3 of doc/plan-egui-shell.md",
                        Self::window_title(&self.editor),
                        self.editor.line_count(),
                        self.editor.char_count(),
                    ))
                    .weak(),
                );
            });
        });
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
