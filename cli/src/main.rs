// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! `quill` — every capability of the core, from a shell script.
//!
//! The CLI is the first front-end built and the one every later one is measured against:
//! **whatever any window can do, this can do** (`doc/architecture.md`, rule 5), and
//! `cli/tests/parity.rs` holds it to that by reading the core's API. It exists so that a
//! capability cannot hide in a user interface — and so that an agent, a test or CI can drive
//! the editor with nothing but a process and its output.
//!
//! Each invocation loads the file, applies one command and writes the file back if it changed.
//! What a long-lived window would remember between keystrokes — the caret, the selection, the
//! history — survives only through `--session FILE`. A subcommand is a few lines driving
//! [`App`]; anything longer belongs in the core.

mod report;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use quill_core::{App, Command, Delete, Frame, Motion, Search, Session};

use report::{CommandRow, Format, Place, Report, Row, State};

#[derive(Parser)]
#[command(
    name = "quill",
    version,
    about = "Edit text files from the command line"
)]
struct Cli {
    /// How to print the result.
    #[arg(long, value_enum, default_value = "text", global = true)]
    format: Format,

    /// Carry the caret, selection, scroll position and undo history between invocations.
    #[arg(long, global = true, value_name = "FILE")]
    session: Option<PathBuf>,

    #[command(subcommand)]
    command: Cmd,
}

/// The surface a layout-dependent command measures against: one column per character and one
/// row per line, so `--width 40` answers what a 40-column window would do.
#[derive(Args, Clone, Copy)]
struct FrameArgs {
    /// Wrap rows at this many columns (0: do not wrap).
    #[arg(long, default_value_t = 0)]
    width: u32,
    /// How many rows are visible — what Page Down and scrolling measure in.
    #[arg(long, default_value_t = 24)]
    rows: u32,
}

impl FrameArgs {
    fn frame(self) -> Frame<'static> {
        Frame::fixed(self.width, self.rows)
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Create an empty file.
    New {
        file: PathBuf,
        /// Overwrite a file that already exists.
        #[arg(long)]
        force: bool,
    },
    /// Where the caret is, and what the file looks like on disk.
    Info { file: PathBuf },
    /// Print lines — or, with --width, the rows a window that wide would draw.
    View {
        file: PathBuf,
        /// First line to print (1-based).
        #[arg(long, default_value_t = 1)]
        from: u64,
        /// Last line to print, inclusive.
        #[arg(long)]
        to: Option<u64>,
        /// Scroll the caret into view first, as a window does after a keystroke.
        #[arg(long)]
        follow: bool,
        #[command(flatten)]
        frame: FrameArgs,
    },
    /// Insert text at the caret (or --at), replacing the selection — one undoable change.
    Insert {
        file: PathBuf,
        #[arg(long, allow_hyphen_values = true)]
        text: String,
        #[arg(long, value_parser = Place::parse)]
        at: Option<Place>,
    },
    /// Type text a character at a time, as a keyboard does — consecutive characters undo a
    /// word at a time.
    Type {
        file: PathBuf,
        #[arg(long, allow_hyphen_values = true)]
        text: String,
    },
    /// Delete the selection, or what KIND names.
    Delete {
        file: PathBuf,
        #[arg(value_enum, default_value = "backward")]
        kind: DeleteKind,
        #[arg(long, default_value_t = 1)]
        count: u32,
    },
    /// Move the caret: left, right, word-left, word-right, up, down, row-start, row-end,
    /// page-up, page-down, document-start, document-end.
    Move {
        file: PathBuf,
        #[arg(value_parser = parse_motion)]
        motion: Motion,
        /// Grow the selection, as Shift does.
        #[arg(long)]
        extend: bool,
        #[arg(long, default_value_t = 1)]
        count: u32,
        #[command(flatten)]
        frame: FrameArgs,
    },
    /// Put the caret at LINE:COLUMN.
    Goto {
        file: PathBuf,
        #[arg(value_parser = Place::parse)]
        place: Place,
        #[arg(long)]
        extend: bool,
    },
    /// Put the caret where a click at (X, Y) in a frame would — columns and rows from the top
    /// left of the visible text.
    Click {
        file: PathBuf,
        x: f32,
        y: f32,
        #[arg(long)]
        extend: bool,
        #[command(flatten)]
        frame: FrameArgs,
    },
    /// Select FROM..TO, everything (--all), or nothing (--none).
    Select {
        file: PathBuf,
        #[arg(value_parser = Place::parse, required_unless_present_any = ["all", "none"])]
        from: Option<Place>,
        #[arg(value_parser = Place::parse, required_unless_present_any = ["all", "none"])]
        to: Option<Place>,
        #[arg(long, conflicts_with = "none")]
        all: bool,
        #[arg(long)]
        none: bool,
    },
    /// Print the selection.
    Copy { file: PathBuf },
    /// Print the selection and delete it.
    Cut { file: PathBuf },
    /// Select the next match of QUERY, wrapping round the end.
    Find {
        file: PathBuf,
        #[arg(allow_hyphen_values = true)]
        query: String,
        #[arg(long)]
        backward: bool,
    },
    /// Find the last query again (needs --session).
    FindNext {
        file: PathBuf,
        #[arg(long)]
        backward: bool,
    },
    /// Undo the last change (needs --session).
    Undo { file: PathBuf },
    /// Redo the last undone change (needs --session).
    Redo { file: PathBuf },
    /// Scroll by rows (--by, negative is up) or to a line (--to).
    Scroll {
        file: PathBuf,
        #[arg(long, allow_hyphen_values = true, conflicts_with = "to")]
        by: Option<i64>,
        #[arg(long)]
        to: Option<u64>,
        #[command(flatten)]
        frame: FrameArgs,
    },
    /// Print the file exactly as Save would write it.
    Export { file: PathBuf },
    /// Replace the file's contents with SOURCE's bytes (`-` for stdin), as a browser's Open does.
    Import { file: PathBuf, source: PathBuf },
    /// Write the document to a new path.
    SaveAs { file: PathBuf, to: PathBuf },
    /// List the commands every window offers in its menus.
    Commands,
}

#[derive(Clone, Copy, ValueEnum)]
enum DeleteKind {
    Backward,
    Forward,
    WordBackward,
    WordForward,
}

impl From<DeleteKind> for Delete {
    fn from(kind: DeleteKind) -> Delete {
        match kind {
            DeleteKind::Backward => Delete::Backward,
            DeleteKind::Forward => Delete::Forward,
            DeleteKind::WordBackward => Delete::WordBackward,
            DeleteKind::WordForward => Delete::WordForward,
        }
    }
}

fn parse_motion(name: &str) -> Result<Motion, String> {
    Motion::from_name(name).ok_or_else(|| {
        let names: Vec<_> = Motion::ALL.iter().map(|m| m.name()).collect();
        format!("unknown motion {name:?}; one of: {}", names.join(", "))
    })
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(report) => {
            report.print(cli.format);
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("quill: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<Report, String> {
    let file = match &cli.command {
        Cmd::Commands => {
            return Ok(Report::Commands {
                commands: Command::ALL
                    .iter()
                    .map(|c| CommandRow {
                        id: c.id(),
                        label: c.label(),
                        summary: c.summary(),
                    })
                    .collect(),
            });
        }
        Cmd::New { file, force } => {
            if file.exists() && !force {
                return Err(format!(
                    "{} already exists (--force to replace it)",
                    file.display()
                ));
            }
            let app = App::new();
            app.new_document();
            app.save_file_as(file).map_err(|e| e.to_string())?;
            return Ok(Report::State(State::of(
                &app,
                &file.display().to_string(),
                true,
                true,
            )));
        }
        Cmd::Info { file }
        | Cmd::View { file, .. }
        | Cmd::Insert { file, .. }
        | Cmd::Type { file, .. }
        | Cmd::Delete { file, .. }
        | Cmd::Move { file, .. }
        | Cmd::Goto { file, .. }
        | Cmd::Click { file, .. }
        | Cmd::Select { file, .. }
        | Cmd::Copy { file }
        | Cmd::Cut { file }
        | Cmd::Find { file, .. }
        | Cmd::FindNext { file, .. }
        | Cmd::Undo { file }
        | Cmd::Redo { file }
        | Cmd::Scroll { file, .. }
        | Cmd::Export { file }
        | Cmd::Import { file, .. }
        | Cmd::SaveAs { file, .. } => file,
    };

    let app = App::new();
    app.open_file(file).map_err(|e| e.to_string())?;
    let session = cli.session.as_deref();
    let print = file.display().to_string();
    if let Some(session) = session {
        restore(&app, session, file)?;
    }

    let mut changed = true;
    let mut text = None;
    match &cli.command {
        Cmd::Info { .. } => changed = false,
        Cmd::View {
            from,
            to,
            follow,
            frame,
            ..
        } => {
            let report = view(&app, &print, *from, *to, *follow, *frame);
            persist(&app, session, file)?;
            return Ok(report);
        }
        Cmd::Insert { text, at, .. } => {
            if let Some(at) = at {
                app.set_caret(at.to_core(), false);
            }
            app.insert_text(text);
        }
        Cmd::Type { text, .. } => {
            for c in text.chars() {
                app.type_text(c.encode_utf8(&mut [0; 4]));
            }
        }
        Cmd::Delete { kind, count, .. } => {
            changed = false;
            for _ in 0..*count {
                changed |= app.delete((*kind).into());
            }
        }
        Cmd::Move {
            motion,
            extend,
            count,
            frame,
            ..
        } => {
            let frame = frame.frame();
            for _ in 0..*count {
                app.move_caret(*motion, *extend, &frame);
            }
            app.follow_caret(&frame);
        }
        Cmd::Goto { place, extend, .. } => {
            app.set_caret(place.to_core(), *extend);
        }
        Cmd::Click {
            x,
            y,
            extend,
            frame,
            ..
        } => {
            app.place_caret(&frame.frame(), *x, *y, *extend);
        }
        Cmd::Select {
            from,
            to,
            all,
            none,
            ..
        } => {
            if *none {
                changed = app.clear_selection();
            } else if *all {
                app.select_all();
            } else if let (Some(from), Some(to)) = (from, to) {
                app.select(from.to_core(), to.to_core());
            }
        }
        Cmd::Copy { .. } => text = Some(app.selected_text()),
        Cmd::Cut { .. } => text = Some(app.cut()),
        Cmd::Find {
            query, backward, ..
        } => changed = app.find(query, direction(*backward)),
        Cmd::FindNext { backward, .. } => {
            if app.search().is_none() {
                return Err("nothing has been searched for (find-next needs --session)".into());
            }
            changed = app.find_again(direction(*backward));
        }
        Cmd::Undo { .. } => {
            changed = app.undo();
            if !changed && session.is_none() {
                return Err("nothing to undo — history only survives with --session".into());
            }
        }
        Cmd::Redo { .. } => changed = app.redo(),
        Cmd::Scroll { by, to, frame, .. } => match (by, to) {
            (_, Some(line)) => app.scroll_to(line.saturating_sub(1)),
            (Some(rows), None) => app.scroll_by(&frame.frame(), *rows),
            (None, None) => return Err("scroll needs --by or --to".into()),
        },
        Cmd::Export { .. } => {
            let bytes = app.save_bytes(None);
            text = Some(String::from_utf8(bytes).map_err(|e| e.to_string())?);
            changed = false;
        }
        Cmd::Import { source, .. } => {
            let bytes = if source.as_os_str() == "-" {
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut std::io::stdin(), &mut bytes)
                    .map_err(|e| e.to_string())?;
                bytes
            } else {
                std::fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?
            };
            app.open_bytes(Some(&print), &bytes)
                .map_err(|e| e.to_string())?;
            app.save_file_as(file).map_err(|e| e.to_string())?;
        }
        Cmd::SaveAs { to, .. } => {
            app.save_file_as(to).map_err(|e| e.to_string())?;
            return Ok(Report::State(State::of(
                &app,
                &to.display().to_string(),
                true,
                true,
            )));
        }
        Cmd::New { .. } | Cmd::Commands => unreachable!("handled above"),
    }

    let written = app.is_dirty();
    if written {
        app.save_file().map_err(|e| e.to_string())?;
    }
    persist(&app, session, file)?;
    Ok(match text {
        Some(text) => Report::Text { text },
        None => Report::State(State::of(&app, &print, changed, written)),
    })
}

fn direction(backward: bool) -> Search {
    if backward {
        Search::Backward
    } else {
        Search::Forward
    }
}

fn view(
    app: &App,
    path: &str,
    from: u64,
    to: Option<u64>,
    follow: bool,
    frame: FrameArgs,
) -> Report {
    let caret = Place::from_core(app.caret());
    let lines = app.line_count();
    if frame.width == 0 && !follow {
        let start = from.max(1) - 1;
        let end = to.unwrap_or(lines).max(start + 1);
        let rows = app
            .lines(start, end)
            .into_iter()
            .zip(start + 1..)
            .map(|(text, line)| Row {
                line,
                first: true,
                text,
            })
            .collect();
        return Report::Rows {
            path: path.to_string(),
            rows,
            caret,
            lines,
        };
    }
    let frame = frame.frame();
    if follow {
        app.follow_caret(&frame);
    } else if from > 1 {
        app.scroll_to(from - 1);
    }
    let view = app.view(&frame);
    Report::Rows {
        path: path.to_string(),
        rows: view
            .rows
            .into_iter()
            .map(|row| Row {
                line: row.line + 1,
                first: row.first,
                text: row.text,
            })
            .collect(),
        caret,
        lines,
    }
}

/// A session on disk: the core's [`Session`], plus a fingerprint of the file it belongs to.
///
/// The fingerprint is why a session cannot corrupt a file. History is a list of edits *to
/// particular text*; replayed against a file somebody changed in the meantime, an undo would
/// delete characters that are not the ones it recorded. So a session whose file has changed is
/// ignored — with a warning, because silently losing an undo history is also a surprise.
#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    fingerprint: u64,
    session: Session,
}

fn restore(app: &App, session: &Path, file: &Path) -> Result<(), String> {
    let Ok(json) = std::fs::read_to_string(session) else {
        return Ok(());
    };
    let stored: Stored = serde_json::from_str(&json)
        .map_err(|e| format!("{} is not a session file: {e}", session.display()))?;
    if stored.fingerprint != fingerprint(file)? {
        eprintln!(
            "quill: {} changed since {} was written; starting a new session",
            file.display(),
            session.display()
        );
        return Ok(());
    }
    app.restore_session(stored.session);
    Ok(())
}

fn persist(app: &App, session: Option<&Path>, file: &Path) -> Result<(), String> {
    let Some(session) = session else {
        return Ok(());
    };
    let stored = Stored {
        fingerprint: fingerprint(file)?,
        session: app.session(),
    };
    let json = serde_json::to_string(&stored).expect("a session serialises");
    std::fs::write(session, json).map_err(|e| format!("{}: {e}", session.display()))
}

/// FNV-1a over the file's bytes: stable across Rust versions and platforms, which
/// `std::hash::DefaultHasher` does not promise, and plenty to tell "the same file" from "not".
fn fingerprint(file: &Path) -> Result<u64, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    }))
}
