//! `edit-egui` — the portable shell over the editor core.
//!
//! Rust-direct like the CLI, TUI and GTK shells: it depends on `editor-core` as an
//! ordinary Cargo dependency, with no FFI and no bindings. What makes it a different
//! kind of shell is not how it reaches the core but what it refuses to be — this one
//! is **deliberately not native to any platform**. It has no menu bar, no libadwaita,
//! no Mica, no system fonts and no system file dialog, and it looks and behaves the
//! same everywhere.
//!
//! That is the compromise the README argues against for real applications, taken here
//! on purpose and in exchange for two things the native shells cannot offer: it builds
//! on every platform with no system dependencies at all, and its behaviour can be
//! tested headlessly, which makes it the only GUI in this project that CI actually
//! runs. See `doc/decision-egui-shell.md` for why that trade is worth making, and
//! `doc/plan-egui-shell.md` for the stages this shell is being built in.
//!
//! **Stages 1 and 2 of that plan**: the crate, the window, and the observer. The text
//! is not rendered yet and no key does anything — the renderer is stage 3 and the key
//! map is stage 4.
//!
//! This file owns the process: arguments, the editor, the window, and the bridge that
//! turns a core notification into a repaint.

mod app;

use std::process::ExitCode;
use std::sync::Arc;

use editor_core::{Editor, EditorObserver};
use eframe::egui;

use app::App;

const USAGE: &str = "usage: edit-egui <file>

A portable GUI shell: the same window on every platform, native to none of them.
For a shell that follows your platform's conventions, use edit-gtk on GNOME, or
the WinUI and SwiftUI apps on Windows and macOS.
";

/// The window's size before the platform has an opinion about it.
const INITIAL_SIZE: [f32; 2] = [800.0, 600.0];

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    if path == "-h" || path == "--help" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    // Opening fails on a missing file rather than inventing an empty buffer, so a
    // typo cannot silently create a document. `edit new <file>` creates one.
    let editor = match Editor::open(&path) {
        Ok(editor) => Arc::new(editor),
        Err(error) => {
            eprintln!("edit-egui: {error}");
            return ExitCode::FAILURE;
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(App::window_title(&editor))
            .with_inner_size(INITIAL_SIZE)
            .with_min_inner_size([320.0, 240.0]),
        ..Default::default()
    };

    // The creation closure is `FnOnce` and runs once the context exists, which is the
    // only moment the observer can be wired: the notifier needs that context.
    let result = eframe::run_native(
        "Editor",
        options,
        Box::new(move |cc| {
            editor.set_observer(Arc::new(Notifier(cc.egui_ctx.clone())));
            Ok(Box::new(App::new(editor)))
        }),
    );

    if let Err(error) = result {
        eprintln!("edit-egui: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Turns a change in the core into a repaint.
///
/// The simplest of the six observer bridges, because `egui::Context` is `Clone`,
/// `Send` and `Sync` and can therefore live inside a `Send + Sync` observer directly
/// — no `async_channel` as in the GTK shell, no `AtomicBool` as in the TUI and browser
/// ones. `request_repaint` also coalesces on its own: it raises a flag that the next
/// frame clears, so a keystroke that notifies twice still paints once.
struct Notifier(egui::Context);

impl EditorObserver for Notifier {
    fn state_changed(&self) {
        self.0.request_repaint();
    }
}
