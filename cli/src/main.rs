//! `edit` — the scriptable front-end over the editor core.
//!
//! Everything any GUI or the TUI can do must be reachable from here; this binary is
//! what keeps that parity rule honest. It is non-interactive: no prompts, no TTY
//! assumptions, stdout parseable, diagnostics on stderr, non-zero exit on failure.
//!
//! The process is stateless but the editor is not, so a mutating command writes the
//! document before exiting. Cursor position and undo history only survive between
//! invocations when `--session` names a file to keep them in.

mod report;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use editor_core::{Direction, Editor, Session};

use report::{Cursor, DocumentReport, Format, Report, ViewReport};

#[derive(Parser)]
#[command(
    name = "edit",
    version,
    about = "Drive the editor core from the shell",
    long_about = "Drive the editor core from the shell.\n\n\
        Lines and columns are 1-based. Column 1 is before the first character, so \
        inserting at column 1 prepends to the line.\n\n\
        Each invocation loads the file, applies one command and writes it back. Pass \
        --session to carry the cursor and the undo history across invocations; without \
        it every command starts at line 1, column 1 with empty history."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// File holding cursor position and undo history between invocations
    #[arg(long, global = true, value_name = "PATH")]
    session: Option<PathBuf>,

    /// Output format
    #[arg(long, global = true, value_enum, default_value = "text")]
    format: Format,

    /// Apply the command and report the result, but write nothing to disk
    #[arg(long, global = true)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Create an empty file
    New {
        file: PathBuf,
        /// Overwrite the file if it already exists
        #[arg(long)]
        force: bool,
    },

    /// Print a range of lines
    View {
        file: PathBuf,
        /// First line to print
        #[arg(long, default_value_t = 1)]
        start: u64,
        /// How many lines to print
        #[arg(long, default_value_t = 40)]
        lines: u64,
    },

    /// Insert text at the cursor
    Insert {
        file: PathBuf,
        /// Text to insert; "-" reads it from stdin
        ///
        /// Hyphen-leading values are taken literally, so text like "--foo" inserts fine.
        #[arg(long, allow_hyphen_values = true)]
        text: String,
        /// Move the cursor here first
        #[arg(long)]
        line: Option<u64>,
        /// Move the cursor here first
        #[arg(long)]
        col: Option<u64>,
    },

    /// Delete the character before the cursor
    Backspace {
        file: PathBuf,
        /// Move the cursor here first
        #[arg(long)]
        line: Option<u64>,
        /// Move the cursor here first
        #[arg(long)]
        col: Option<u64>,
        /// Repeat count
        #[arg(long, default_value_t = 1)]
        times: u64,
    },

    /// Move the cursor (needs --session to persist)
    Move {
        file: PathBuf,
        #[arg(value_enum)]
        direction: MoveDirection,
        /// Repeat count
        #[arg(long, default_value_t = 1)]
        times: u64,
    },

    /// Undo the last action recorded in the session
    Undo { file: PathBuf },

    /// Redo the last undone action recorded in the session
    Redo { file: PathBuf },

    /// Report cursor, size and history state
    Info { file: PathBuf },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum MoveDirection {
    Up,
    Down,
    Left,
    Right,
}

impl From<MoveDirection> for Direction {
    fn from(direction: MoveDirection) -> Self {
        match direction {
            MoveDirection::Up => Direction::Up,
            MoveDirection::Down => Direction::Down,
            MoveDirection::Left => Direction::Left,
            MoveDirection::Right => Direction::Right,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = cli.format;

    match run(&cli) {
        Ok(report) => {
            report.print(format);
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("edit: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<Report, String> {
    match &cli.command {
        Command::New { file, force } => {
            if file.exists() && !force {
                return Err(format!("{} already exists (use --force)", file.display()));
            }
            let editor = Editor::new();
            write_document(&editor, file, cli.dry_run)?;
            save_session(&editor, cli)?;
            Ok(Report::Document(DocumentReport::new(
                &editor,
                true,
                !cli.dry_run,
            )))
        }

        Command::View { file, start, lines } => {
            let editor = load(file, cli)?;
            let start_line = start.saturating_sub(1);
            let viewport = editor.get_viewport(start_line, start_line.saturating_add(*lines));
            Ok(Report::View(ViewReport {
                path: file.display().to_string(),
                start_line: viewport.start_line + 1,
                lines: viewport.lines,
                total_lines: viewport.total_lines,
                cursor: Cursor::from_core(viewport.cursor),
            }))
        }

        Command::Insert {
            file,
            text,
            line,
            col,
        } => {
            let editor = load(file, cli)?;
            place_cursor(&editor, *line, *col);
            let text = if text == "-" { read_stdin()? } else { text.clone() };
            editor.insert_text(&text);
            finish(&editor, cli, !text.is_empty())
        }

        Command::Backspace {
            file,
            line,
            col,
            times,
        } => {
            let editor = load(file, cli)?;
            place_cursor(&editor, *line, *col);
            let mut changed = false;
            for _ in 0..*times {
                changed |= editor.handle_backspace();
            }
            finish(&editor, cli, changed)
        }

        Command::Move {
            file,
            direction,
            times,
        } => {
            let editor = load(file, cli)?;
            let before = editor.cursor();
            for _ in 0..*times {
                editor.move_cursor((*direction).into());
            }
            // The document is untouched; only the session needs writing.
            save_session(&editor, cli)?;
            Ok(Report::Document(DocumentReport::new(
                &editor,
                editor.cursor() != before,
                false,
            )))
        }

        Command::Undo { file } => {
            let editor = load(file, cli)?;
            require_session(cli, "undo")?;
            let changed = editor.undo();
            finish(&editor, cli, changed)
        }

        Command::Redo { file } => {
            let editor = load(file, cli)?;
            require_session(cli, "redo")?;
            let changed = editor.redo();
            finish(&editor, cli, changed)
        }

        Command::Info { file } => {
            let editor = load(file, cli)?;
            Ok(Report::Document(DocumentReport::new(&editor, false, false)))
        }
    }
}

/// Open the document and, if a session file was named, resume its cursor and history.
fn load(file: &Path, cli: &Cli) -> Result<Editor, String> {
    let editor = Editor::open(file).map_err(|e| e.to_string())?;
    if let Some(path) = &cli.session {
        if path.exists() {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read session {}: {e}", path.display()))?;
            let session: Session = serde_json::from_str(&raw)
                .map_err(|e| format!("cannot parse session {}: {e}", path.display()))?;
            editor.restore_session(session);
        }
    }
    Ok(editor)
}

/// Persist the document (when it changed) and the session, then report.
fn finish(editor: &Editor, cli: &Cli, changed: bool) -> Result<Report, String> {
    let written = changed && !cli.dry_run;
    if written {
        editor.save_file().map_err(|e| e.to_string())?;
    }
    save_session(editor, cli)?;
    Ok(Report::Document(DocumentReport::new(
        editor, changed, written,
    )))
}

fn save_session(editor: &Editor, cli: &Cli) -> Result<(), String> {
    let (Some(path), false) = (&cli.session, cli.dry_run) else {
        return Ok(());
    };
    let session = serde_json::to_string(&editor.session()).expect("session is serializable");
    std::fs::write(path, session)
        .map_err(|e| format!("cannot write session {}: {e}", path.display()))
}

fn write_document(editor: &Editor, file: &Path, dry_run: bool) -> Result<(), String> {
    if dry_run {
        return Ok(());
    }
    editor.save_file_as(file).map_err(|e| e.to_string())
}

/// Apply `--line`/`--col` if either was given, filling the other from the current cursor.
fn place_cursor(editor: &Editor, line: Option<u64>, col: Option<u64>) {
    if line.is_none() && col.is_none() {
        return;
    }
    let current = Cursor::from_core(editor.cursor());
    editor.set_cursor(Cursor::to_core(
        line.unwrap_or(current.line),
        col.unwrap_or(1),
    ));
}

/// Undo history lives in the session file; without one the stacks are always empty.
fn require_session(cli: &Cli, what: &str) -> Result<(), String> {
    if cli.session.is_none() {
        return Err(format!(
            "{what} needs --session: history is not stored in the document"
        ));
    }
    Ok(())
}

fn read_stdin() -> Result<String, String> {
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|e| format!("cannot read stdin: {e}"))?;
    Ok(buffer)
}
