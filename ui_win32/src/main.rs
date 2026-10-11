// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! `quill-win32` — the editor as a Windows application: Win32 and GDI from Rust, through
//! Microsoft's own `windows` crate.
//!
//! **The `.exe` needs nothing Windows does not already ship.** No .NET, no Windows App SDK, no
//! Visual C++ redistributable: it links system DLLs only, and `.cargo/config.toml` links the C
//! runtime statically. `artifacts.yml` reads the import table back on `windows-latest` and fails
//! if anything else appears. (`doc/decisions/0001-win32-shell.md` is why Win32 rather than
//! WinUI.)
//!
//! **And half of it tests on Linux.** The command line, the menus as data, the keys, the theme
//! and the window's geometry are portable modules; only `gdi.rs` and `win.rs` need Windows, and
//! `cargo check --target x86_64-pc-windows-msvc` type-checks even those without it.

// A GUI application, not a console one, or Explorer opens a console window behind it.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod args;
// Off Windows nothing calls into these, so their functions read as dead code — but their tests
// are the reason the crate builds there at all.
#[cfg_attr(not(windows), allow(dead_code))]
mod keymap;
#[cfg_attr(not(windows), allow(dead_code))]
mod menu;
#[cfg_attr(not(windows), allow(dead_code))]
mod theme;

#[cfg(windows)]
mod gdi;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::process::ExitCode;
    use std::sync::Arc;

    let args = match args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => return fail(&format!("{message}\n\n{}", args::USAGE)),
    };
    let app = Arc::new(quill_core::App::new());
    if let Some(path) = &args.file {
        let result = if path.exists() {
            app.open_file(path)
        } else {
            app.open_bytes(Some(&path.display().to_string()), b"")
        };
        if let Err(error) = result {
            return fail(&error.to_string());
        }
    }

    // One frame into a BMP with no window, and out.
    if let Some(out) = &args.render_to {
        let theme = if args.dark {
            theme::Theme::dark()
        } else {
            theme::Theme::light()
        };
        let bmp = gdi::render_to_bmp(&app, args.size.0, args.size.1, &theme);
        return match std::fs::write(out, bmp) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail(&format!("{}: {error}", out.display())),
        };
    }

    match win::run_window(app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&error.to_string()),
    }
}

/// Say something before there is a window to say it in. A GUI-subsystem binary has no console,
/// so `eprintln!` would go nowhere when started from Explorer — and somewhere when started from
/// a terminal or a CI step, which is why it prints as well.
#[cfg(windows)]
fn fail(text: &str) -> std::process::ExitCode {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::{PCWSTR, w};

    eprintln!("quill-win32: {text}");
    if std::env::var_os("QUILL_NO_DIALOGS").is_none() {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(wide.as_ptr()),
                w!("Quill"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    std::process::ExitCode::FAILURE
}

/// Off Windows the crate still builds, so its portable modules compile and test where the
/// repository is developed. The binary has nothing to do there.
#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    let _ = args::parse(std::env::args().skip(1));
    eprint!("quill-win32 runs on Windows only.\n\n{}", args::USAGE);
    std::process::ExitCode::FAILURE
}
