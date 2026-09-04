//! `edit-win32` — the Windows shell over the editor core.
//!
//! Rust-direct like the CLI, TUI, GTK and portable shells: it depends on
//! `editor-core` as an ordinary Cargo dependency, with no FFI, no bindings and no
//! generated code. That is a change from what stood here before — a WinUI 3
//! application in C# reaching the core through UniFFI — and the reasoning is in
//! `doc/decision-win32-shell.md`.
//!
//! What it buys is stated in one line and meant literally: **the built `.exe` needs
//! nothing that Windows does not already ship.** No .NET runtime, no Windows App
//! SDK, no Visual C++ redistributable. It links `user32`, `gdi32`, `dwmapi` and
//! `advapi32`, all of which are part of the operating system, and the MSVC C runtime
//! is linked statically by `.cargo/config.toml`.
//!
//! What it costs is the Fluent control set. This window is drawn with GDI, so it
//! follows Windows' *conventions* — the shell font, the user's wheel and caret
//! settings, Ctrl+Y for redo, a Save/Don't Save/Cancel dialog on close, a dark title
//! bar when the user's theme is dark — without using Windows' *controls*.
//!
//! This file owns the process; `app.rs` owns the window.

// A GUI application, not a console one. Without this, launching the editor from
// Explorer flashes up a console window behind it and leaves it there for the life of
// the process. The cost is that there is no stderr to print to, which is why the
// argument errors below go into a message box — the same bargain every Windows GUI
// application makes.
#![cfg_attr(windows, windows_subsystem = "windows")]

// Off Windows there is no `app` to call into these, so every function in them reads
// as dead code — but their tests are the reason the crate builds here at all, and
// silencing the lint is what keeps `cargo clippy` clean on the development machine.
#[cfg_attr(not(windows), allow(dead_code))]
mod keymap;
#[cfg_attr(not(windows), allow(dead_code))]
mod layout;

#[cfg(windows)]
mod app;

const USAGE: &str = "usage: edit-win32 <file>

A Win32 shell for Windows, depending on nothing the system does not ship.
";

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::process::ExitCode;
    use std::sync::Arc;

    use editor_core::Editor;

    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        message_box("usage", USAGE);
        return ExitCode::from(2);
    };
    if path == "-h" || path == "--help" {
        message_box("edit-win32", USAGE);
        return ExitCode::SUCCESS;
    }

    // Opening fails on a missing file rather than inventing an empty buffer, so a
    // typo cannot silently create a document. `edit new <file>` creates one.
    let editor = match Editor::open(&path) {
        Ok(editor) => Arc::new(editor),
        Err(error) => {
            message_box("edit-win32", &format!("{error}"));
            return ExitCode::FAILURE;
        }
    };

    match app::run(editor) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            message_box("edit-win32", &format!("{error}"));
            ExitCode::FAILURE
        }
    }
}

/// Say something before there is a window to say it in.
///
/// A GUI subsystem binary has no console to print to — `eprintln!` here goes
/// nowhere at all when the program is launched from Explorer. The other shells can
/// use stderr; this one has to ask the system for a dialog.
#[cfg(windows)]
fn message_box(title: &str, text: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// On anything but Windows this crate still builds, so that `keymap.rs` and
/// `layout.rs` can be compiled and tested on the machine this repository is
/// developed on. The binary itself has nothing to do there.
#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprint!("edit-win32 runs on Windows only.\n\n{USAGE}");
    std::process::ExitCode::FAILURE
}
