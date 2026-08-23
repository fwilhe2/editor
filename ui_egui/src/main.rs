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
//! **Stage 1 of that plan**: the crate, its place in the workspace, and the version
//! pins. The argument handling and `Editor::open` below are final — they are the same
//! contract `edit-tui` and `edit-gtk` follow — but the window, the renderer and the
//! key map arrive in stages 2 to 4, so this binary currently opens the document and
//! says so rather than pretending to draw it.

use std::process::ExitCode;
use std::sync::Arc;

use editor_core::Editor;

const USAGE: &str = "usage: edit-egui <file>

A portable GUI shell: the same window on every platform, native to none of them.
For a shell that follows your platform's conventions, use edit-gtk on GNOME, or
the WinUI and SwiftUI apps on Windows and macOS.
";

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

    println!(
        "edit-egui: opened {path} ({} lines, {} chars)",
        editor.line_count(),
        editor.char_count()
    );
    eprintln!("edit-egui: no window yet — stage 2 of doc/plan-egui-shell.md");
    ExitCode::SUCCESS
}
