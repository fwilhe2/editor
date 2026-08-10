//! `edit-tui` — the terminal shell over the editor core.
//!
//! Pure Rust, so it depends on `editor-core` directly: no FFI, no bindings. The
//! loop is render → block on a key → route it to the core, and every capability
//! it offers also exists in the CLI.
//!
//! The terminal is global state this process borrows. Raw mode and the alternate
//! screen must be handed back on *every* exit path — normal quit, error, or panic
//! — or the user is left with a shell that no longer echoes. That is what
//! [`restore_terminal`] and the panic hook are for.

mod app;

use std::io::{self, Stdout};
use std::process::ExitCode;
use std::sync::Arc;

use editor_core::Editor;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::Terminal;

use app::{App, RedrawFlag};

type Tui = Terminal<CrosstermBackend<Stdout>>;

const USAGE: &str = "usage: edit-tui <file>

Keys:
  arrows      move the cursor
  Ctrl+S      save
  Ctrl+Z/Y    undo / redo
  Ctrl+Q      quit (twice if there are unsaved changes)
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
            eprintln!("edit-tui: {error}");
            return ExitCode::FAILURE;
        }
    };

    let redraw = Arc::new(RedrawFlag::default());
    editor.set_observer(redraw.clone());
    redraw.raise(); // paint the first frame before waiting for input

    match run(editor, redraw) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("edit-tui: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(editor: Arc<Editor>, redraw: Arc<RedrawFlag>) -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    // Run the loop, then restore unconditionally — including when it returned an
    // error, which must not reach the user through a raw-mode terminal.
    let result = event_loop(&mut terminal, editor, redraw);
    restore_terminal();
    result
}

fn event_loop(terminal: &mut Tui, editor: Arc<Editor>, redraw: Arc<RedrawFlag>) -> io::Result<()> {
    let mut app = App::new(editor, redraw.clone());

    while !app.should_quit() {
        if redraw.take() {
            terminal.draw(|frame| app.draw(frame))?;
        }
        // Block until something happens; a TUI has no reason to spin.
        match event::read()? {
            Event::Key(key) => app.on_key(key),
            Event::Resize(_, _) => redraw.raise(),
            _ => {}
        }
    }
    Ok(())
}

fn setup_terminal() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // The alternate screen keeps the user's scrollback intact.
    execute!(stdout, EnterAlternateScreen)?;

    // From here on a panic would leave the terminal unusable, so teardown runs first.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    terminal.clear()?;
    Ok(terminal)
}

/// Undo [`setup_terminal`]. Errors are swallowed on purpose: this runs while
/// unwinding or on the way out, and there is nothing useful left to do about them.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}
