// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! `quill-gtk` — the editor as a GNOME application: GTK 4 and libadwaita, following the GNOME
//! Human Interface Guidelines.
//!
//! Rust-direct: it depends on `quill-core` as an ordinary crate, with no bindings. What it adds
//! to the core is a Pango [`quill_core::layout::Metrics`] and the chrome — the drawing is
//! [`paint`], the menus and accelerators are data in [`menu`], and the editing keys a pure
//! function in [`keymap`], all three testable with no display.

mod args;
mod keymap;
mod menu;
mod paint;
mod window;

use std::process::ExitCode;
use std::sync::Arc;

use libadwaita as adw;
use libadwaita::gtk::{gio, glib};
use quill_core::App;

use adw::prelude::*;

fn main() -> ExitCode {
    let args = match args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprint!("quill-gtk: {message}\n\n{}", args::USAGE);
            return ExitCode::from(2);
        }
    };

    let app = Arc::new(App::new());
    if let Some(path) = &args.file {
        let result = if path.exists() {
            app.open_file(path)
        } else {
            // A name that does not exist yet is a new document saved there on the first Save.
            app.open_bytes(Some(&path.display().to_string()), b"")
        };
        if let Err(error) = result {
            eprintln!("quill-gtk: {error}");
            return ExitCode::FAILURE;
        }
    }

    // One frame into a PNG with no display, and out: pango and cairo need no GTK at all.
    if let Some(out) = &args.render_to {
        let palette = if args.dark {
            paint::Palette::dark()
        } else {
            paint::Palette::light()
        };
        let result = std::fs::File::create(out)
            .map_err(|e| format!("{}: {e}", out.display()))
            .and_then(|mut file| {
                paint::render_to_png(&app, args.size.0, args.size.1, &palette, &mut file)
            });
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("quill-gtk: {message}");
                ExitCode::FAILURE
            }
        };
    }

    let application = adw::Application::builder()
        .application_id(quill_core::APP_ID)
        // One process per document: a second `quill-gtk notes.txt` opens a second window
        // rather than handing its file to the first process, which would need that process to
        // hold more than one `App`.
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.connect_activate(move |application| window::build(application, app.clone()));

    // The arguments were consumed above; GApplication must not parse them again.
    match application.run_with_args::<&str>(&[]) {
        code if code == glib::ExitCode::SUCCESS => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
