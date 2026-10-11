// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! `quill-tui` — the editor in a terminal.
//!
//! The cheapest shell with a real event loop, and so the first one built after the CLI: it is
//! where scrolling, repaint coalescing and teardown are first needed, and whatever it invents
//! that a second shell would want belongs in the core before that second shell is written.
//!
//! The terminal is global state this process borrows. Raw mode, the alternate screen and mouse
//! capture are handed back on **every** exit path — a normal quit, an error, a panic — or the
//! person is left with a terminal that no longer echoes. That is what [`restore`] and the
//! panic hook are for.

mod cells;
mod keymap;
mod shell;

use std::io::{self, Stdout};
use std::process::ExitCode;
use std::sync::Arc;

use quill_core::App;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use shell::{RedrawFlag, Shell};

const USAGE: &str = "usage: quill-tui [FILE]

A FILE that does not exist yet is created on the first save.
Ctrl+P lists every command; the bottom line shows the common ones.
";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let path = args.next();
    if matches!(path.as_deref(), Some("-h" | "--help")) {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let app = Arc::new(App::new());
    if let Some(path) = &path {
        let result = if std::path::Path::new(path).exists() {
            app.open_file(path)
        } else {
            app.open_bytes(Some(path), b"")
        };
        if let Err(error) = result {
            eprintln!("quill-tui: {error}");
            return ExitCode::FAILURE;
        }
    }

    let redraw = Arc::new(RedrawFlag::default());
    app.observe(redraw.clone());
    redraw.raise();

    match run(app, redraw) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("quill-tui: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(app: Arc<App>, redraw: Arc<RedrawFlag>) -> io::Result<()> {
    let mut terminal = setup()?;
    // Restore unconditionally — including when the loop failed, whose error must not reach the
    // person through a terminal still in raw mode.
    let result = event_loop(&mut terminal, app, redraw);
    restore();
    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: Arc<App>,
    redraw: Arc<RedrawFlag>,
) -> io::Result<()> {
    let mut shell = Shell::new(app, redraw.clone());
    let size = terminal.size()?;
    shell.resize(size.width, size.height);
    while !shell.should_quit() {
        if redraw.take() {
            terminal.draw(|frame| shell.draw(frame))?;
        }
        // Block until something happens: a terminal application has no reason to spin.
        match event::read()? {
            Event::Key(key) => shell.on_key(key),
            Event::Mouse(mouse) => shell.on_mouse(mouse),
            Event::Resize(width, height) => shell.resize(width, height),
            _ => {}
        }
    }
    Ok(())
}

fn setup() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // The alternate screen keeps the person's scrollback intact.
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    // From here on a panic would leave the terminal unusable, so teardown runs first.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    terminal.clear()?;
    Ok(terminal)
}

/// Undo [`setup`]. Errors are swallowed: this runs while unwinding or on the way out, and there
/// is nothing useful left to do about them.
fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
}
