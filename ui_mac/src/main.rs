// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! `quill-mac` — the editor as a macOS application: AppKit, CoreGraphics and CoreText from
//! Rust through `objc2`, with no Swift, no Xcode project and no bindings to generate.
//!
//! **AppKit draws the chrome; this shell draws only the document.** The menu bar, the window,
//! the alerts and the open and save panels are AppKit's own and look like every Mac
//! application's; the text is painted by `paint.rs` with CoreText, measured by the same engine
//! that draws it (`doc/decisions/0002-appkit-shell.md`).
//!
//! **Developed without a Mac.** The portable half — the menus as data, the key selectors, the
//! PNG encoder, the Info.plist, the geometry — tests on Linux, and `cargo check --target
//! aarch64-apple-darwin` type-checks the AppKit half there too. It links and runs on the macOS
//! runners in `artifacts.yml`, which draw frames with `--render-to` and bundle `Quill.app`.

mod args;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod keys;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod menu;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod metrics;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod plist;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod png;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod theme;

#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod paint;
#[cfg(target_os = "macos")]
mod view;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprint!("quill-mac: {message}\n\n{}", args::USAGE);
            return ExitCode::from(2);
        }
    };
    if args.info_plist {
        print!("{}", plist::info_plist());
        return ExitCode::SUCCESS;
    }
    run(args)
}

#[cfg(target_os = "macos")]
fn run(args: args::Args) -> ExitCode {
    use std::sync::Arc;

    let app = Arc::new(quill_core::App::new());
    if let Some(path) = &args.file {
        let result = if path.exists() {
            app.open_file(path)
        } else {
            app.open_bytes(Some(&path.display().to_string()), b"")
        };
        if let Err(error) = result {
            eprintln!("quill-mac: {error}");
            return ExitCode::FAILURE;
        }
    }
    if let Some(out) = &args.render_to {
        let palette = if args.dark {
            theme::Palette::dark()
        } else {
            theme::Palette::light()
        };
        let (w, h) = (f64::from(args.size.0), f64::from(args.size.1));
        return match paint::render_to_png(&app, w, h, &palette)
            .and_then(|png| std::fs::write(out, png).map_err(|e| e.to_string()))
        {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("quill-mac: {message}");
                ExitCode::FAILURE
            }
        };
    }
    app::run(app)
}

/// Off macOS the crate still builds, so its portable half compiles and tests where the
/// repository is developed. `--info-plist` works everywhere; nothing else has anything to do.
#[cfg(not(target_os = "macos"))]
fn run(_args: args::Args) -> ExitCode {
    eprint!("quill-mac runs on macOS only.\n\n{}", args::USAGE);
    ExitCode::FAILURE
}
