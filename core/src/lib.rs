// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The editor's core: every piece of state and every rule, and no user interface.
//!
//! Six front-ends drive this crate — a CLI, a terminal UI, a GNOME window, a Win32 window, an
//! AppKit window and a browser page — and none of them owns a byte of document state. They
//! render what [`App::view`] hands them and forward what the user did. `doc/architecture.md` is
//! the argument; the short version is the four rules this crate is shaped by:
//!
//! 1. **Reads are windowed.** [`App::view`] and [`App::lines`] return a slice; nothing hands a
//!    shell the whole document to render from. (Saving is the one honest exception.)
//! 2. **Undo lives here.** [`Document::apply`] returns the edit's inverse, and that is the whole
//!    mechanism ([`history`]).
//! 3. **The core pushes.** [`App`] drops its write lock *before* it notifies an [`Observer`],
//!    because every observer calls straight back in to re-read.
//! 4. **Line layout lives here too** ([`layout`]). Up, Down, Home, End, Page Down, a click and
//!    a selection's extent are all defined in terms of a *visual* line, so a shell that wrapped
//!    text itself would own half the editing model. A shell supplies only [`layout::Metrics`]:
//!    how wide a string is in its own unit, and how tall a line.
//!
//! **Positions are 0-based here.** Only a user interface is 1-based, and the CLI converts in
//! exactly one place (`cli/src/report.rs`).

pub mod command;
mod document;
mod encoding;
mod error;
pub mod history;
pub mod layout;
mod motion;
mod session;
mod view;

mod app;

pub use app::{App, Observer};
pub use command::Command;
pub use document::{Document, Edit, Position};
pub use encoding::{Encoding, LineEnding};
pub use error::{Error, Result};
pub use motion::{Delete, Motion, Search};
pub use session::Session;
pub use view::{Caret, Frame, Row, Scroll, Selection, View};

/// The application's name as a person reads it — a window title, a menu, an About box.
///
/// One of the three identity constants `scripts/bootstrap.py` rewrites when this template
/// becomes another application; every shell reads them from here rather than spelling them.
pub const APP_NAME: &str = "Quill";

/// The reverse-DNS application ID: GTK's application ID, the `.desktop` file's name, the
/// macOS bundle identifier, the Windows AppUserModelID.
pub const APP_ID: &str = "io.github.fwilhe2.Quill";

/// What a document that has never been saved is called.
pub const UNTITLED: &str = "Untitled";

/// The version every shell's About box shows: the workspace's own.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
