// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The whole shell contract in one file: read this before any real shell.
//!
//! ```sh
//! cargo run -p quill-core --example minimal_shell
//! ```
//!
//! A shell that reads lines from stdin and prints frames to stdout — no toolkit, no raw mode,
//! nothing to install. It is deliberately too plain to use and exactly complete enough to show
//! the five things every shell in this repository does, in the order a shell is written:
//!
//! 1. **Answer the font question** — implement [`Metrics`] in your own unit ([`Columns`]).
//! 2. **Hear about changes** — implement [`Observer`]; note the change, draw later ([`Redraw`]).
//! 3. **Translate input into the core's vocabulary** — [`parse`] turns a line into an
//!    [`Action`]. It touches no state, so it is tested without a shell.
//! 4. **Forward the action to the core**, then `follow_caret` — [`Shell::act`].
//! 5. **Paint from [`App::view`]** and nothing else — [`Shell::paint`].
//!
//! What it leaves out is what makes a shell *native*: its platform's keys, menus, dialogs and
//! clipboard. Those are `ui_tui/` (the smallest real one) and its siblings; `doc/tour.md` walks
//! one keystroke through them. This file is not a seventh shell, so no ratchet covers it — but
//! its own tests at the bottom run with `cargo test`.
//!
//! **Adapting the template?** This is also the smallest thing to keep compiling while the core
//! changes underneath it (`doc/bootstrap.md`, step 4): when it builds and its tests pass against
//! your new core, the contract every real shell needs is back in place.

use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use quill_core::layout::Metrics;
use quill_core::{App, Command, Delete, Frame, Motion, Observer, Search};

// --- 1. the font question ------------------------------------------------------------------

/// One column per character, one row per line: this shell's unit is a character cell.
///
/// The core does all wrapping, caret movement and hit-testing, and asks a shell only this. A
/// real shell answers with the engine it draws with — Pango, GDI, CoreText, a canvas — so the
/// caret lands where the ink is. (`quill_core::layout::Fixed` is this same answer, ready-made;
/// it is spelled out here because writing it is step one of every shell.)
struct Columns;

impl Metrics for Columns {
    fn advances(&self, text: &str, out: &mut Vec<f32>) {
        out.extend((1..=text.chars().count()).map(|n| n as f32));
    }

    fn line_height(&self) -> f32 {
        1.0
    }
}

// --- 2. hearing about changes --------------------------------------------------------------

/// Set by the core after every change; taken by the loop before it paints.
///
/// The core calls [`Observer::changed`] on whichever thread made the change, so an observer
/// never draws: it leaves a note for the UI thread. A flag also coalesces — a command that
/// changes three things is one repaint. Every shell's bridge has this shape; only the way the
/// note reaches the UI thread differs (`doc/architecture.md` §4).
#[derive(Default)]
struct Redraw(AtomicBool);

impl Observer for Redraw {
    fn changed(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

// --- 3. input, in the core's vocabulary ----------------------------------------------------

/// What a line of input asks for. The real shells' keymaps return this same shape.
#[derive(Debug, PartialEq)]
enum Action {
    /// A menu verb, with whatever followed it on the line (a path, a search string).
    Command(Command, String),
    /// A caret motion; `true` extends the selection, as Shift does in a real shell.
    Move(Motion, bool),
    Delete(Delete),
    Type(String),
    Help,
}

/// `:edit.undo`, `:file.open notes.txt`, `:word-right`, `:+down` (extend), `:backspace` — and
/// any line not starting with a colon is typed, with its line break.
///
/// The names are the core's own ([`Command::id`], [`Motion::name`]): the verbs are data, so a
/// shell looks them up rather than keeping a list that could fall behind.
fn parse(line: &str) -> Option<Action> {
    let Some(rest) = line.strip_prefix(':') else {
        return Some(Action::Type(format!("{line}\n")));
    };
    let (word, argument) = rest.split_once(' ').unwrap_or((rest, ""));
    if let Some(command) = Command::from_id(word) {
        return Some(Action::Command(command, argument.to_string()));
    }
    let (name, extend) = match word.strip_prefix('+') {
        Some(name) => (name, true),
        None => (word, false),
    };
    if let Some(motion) = Motion::from_name(name) {
        return Some(Action::Move(motion, extend));
    }
    match word {
        "backspace" => Some(Action::Delete(Delete::Backward)),
        "delete" => Some(Action::Delete(Delete::Forward)),
        "help" => Some(Action::Help),
        _ => None,
    }
}

// --- 4 and 5. the shell --------------------------------------------------------------------

/// Everything this shell owns — and none of it is the document.
///
/// The test for a shell's field: could the core know it? A clipboard and a status line are the
/// platform's; the text, the caret, the selection, the history and the scroll position are not,
/// and live behind [`App`].
struct Shell {
    app: Arc<App>,
    redraw: Arc<Redraw>,
    /// A stand-in for the platform clipboard.
    clipboard: String,
    status: String,
    quit: bool,
}

/// The size of the "window": rows wrap at 40 columns and 8 of them show.
const WIDTH: f32 = 40.0;
const HEIGHT: f32 = 8.0;

impl Shell {
    fn new() -> Shell {
        let app = Arc::new(App::new());
        let redraw = Arc::new(Redraw::default());
        app.observe(redraw.clone());
        Shell {
            app,
            redraw,
            clipboard: String::new(),
            status: String::new(),
            quit: false,
        }
    }

    /// Step 4: every action is one call into the core. No editing rule is decided here.
    fn act(&mut self, action: Action) {
        // The frame travels with every call that depends on layout: Down means "one row on
        // screen", and only the shell knows how wide the screen is.
        let frame = Frame::new(WIDTH, HEIGHT, &Columns);
        self.status.clear();
        match action {
            Action::Type(text) => self.app.type_text(&text),
            Action::Move(motion, extend) => self.app.move_caret(motion, extend, &frame),
            Action::Delete(kind) => {
                self.app.delete(kind);
            }
            Action::Command(command, argument) => self.run(command, &argument),
            Action::Help => self.status = help(),
        }
        // Input handling brings the caret into view. Painting never does, or scrolling away
        // from the caret would snap straight back on the next repaint.
        self.app.follow_caret(&frame);
        // A status line is presentation, not a change to the document, so the core does not
        // announce it: the shell asks itself for the repaint.
        if !self.status.is_empty() || self.quit {
            self.redraw.0.store(true, Ordering::SeqCst);
        }
    }

    /// One arm per verb. The `match` has no `_` arm on purpose: a verb added to the core is a
    /// compile error here until this shell says what it does with it.
    fn run(&mut self, command: Command, argument: &str) {
        let done = match command {
            Command::New => {
                self.app.new_document();
                Ok(true)
            }
            Command::Open => self.app.open_file(argument).map(|()| true),
            Command::Save => self.app.save_file().map(|()| true),
            Command::SaveAs => self.app.save_file_as(argument).map(|()| true),
            // A real shell asks before discarding unsaved changes; that question is a platform
            // convention (a dialog, a second key press), so it is the shell's to ask.
            Command::Close => {
                self.quit = true;
                Ok(true)
            }
            Command::Undo => Ok(self.app.undo()),
            Command::Redo => Ok(self.app.redo()),
            Command::Cut => {
                self.clipboard = self.app.cut();
                Ok(true)
            }
            Command::Copy => {
                self.clipboard = self.app.selected_text();
                Ok(true)
            }
            Command::Paste => {
                self.app.insert_text(&self.clipboard);
                Ok(true)
            }
            Command::SelectAll => {
                self.app.select_all();
                Ok(true)
            }
            Command::Find => Ok(self.app.find(argument, Search::Forward)),
            Command::FindNext => Ok(self.app.find_again(Search::Forward)),
            Command::FindPrevious => Ok(self.app.find_again(Search::Backward)),
            Command::About => {
                self.status = format!("{} {}", quill_core::APP_NAME, quill_core::VERSION);
                Ok(true)
            }
        };
        match done {
            Ok(true) => {}
            Ok(false) => self.status = format!("{}: nothing to do", command.label()),
            Err(error) => self.status = error.to_string(),
        }
    }

    /// Step 5: one frame, from [`App::view`] alone.
    ///
    /// The view holds only the rows that fit, already wrapped, with the caret and each row's
    /// selection in this shell's unit — so painting is a loop with no arithmetic about the
    /// document in it. `[` `]` mark the selection and `|` the caret.
    fn paint(&self) -> String {
        let view = self.app.view(&Frame::new(WIDTH, HEIGHT, &Columns));
        let mut out = String::new();
        for (index, row) in view.rows.iter().enumerate() {
            // Marks are inserted from the right, so an earlier insert does not shift a later one.
            let mut marks = Vec::new();
            if let Some(selection) = row.selection {
                marks.push((selection.to as usize, ']'));
                marks.push((selection.from as usize, '['));
            } else if let Some(caret) = view.caret.filter(|caret| caret.row == index) {
                marks.push((caret.x as usize, '|'));
            }
            let mut text: Vec<char> = row.text.chars().collect();
            for (column, mark) in marks {
                text.insert(column.min(text.len()), mark);
            }
            // A wrapped row carries no number: it is the same line of the document.
            let number = if row.first {
                format!("{:>3} ", row.line + 1)
            } else {
                "    ".to_string()
            };
            out.push_str(&number);
            out.extend(text);
            out.push('\n');
        }
        let dirty = if self.app.is_dirty() { " •" } else { "" };
        out.push_str(&format!(
            "--- {}{dirty} {}\n",
            self.app.title(),
            self.status
        ));
        out
    }
}

/// The help text, built from the core's tables rather than written out.
fn help() -> String {
    let commands: Vec<_> = Command::ALL
        .iter()
        .map(|c| format!(":{}", c.id()))
        .collect();
    let motions: Vec<_> = Motion::ALL
        .iter()
        .map(|m| format!(":{}", m.name()))
        .collect();
    format!(
        "\n{}\n{} (:+name extends the selection)\n:backspace :delete :help — anything else is typed",
        commands.join(" "),
        motions.join(" ")
    )
}

/// The event loop: wait for input, act, paint if the core said something changed.
fn main() -> io::Result<()> {
    let mut shell = Shell::new();
    let mut stdout = io::stdout();
    writeln!(stdout, "Type text, or :help. :file.close quits.")?;
    for line in io::stdin().lock().lines() {
        match parse(&line?) {
            Some(action) => shell.act(action),
            None => writeln!(stdout, "not a command — :help lists them")?,
        }
        if shell.redraw.0.swap(false, Ordering::SeqCst) {
            write!(stdout, "{}", shell.paint())?;
        }
        if shell.quit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str]) -> Shell {
        let mut shell = Shell::new();
        for line in lines {
            shell.act(parse(line).unwrap());
        }
        shell
    }

    #[test]
    fn input_is_translated_without_a_shell() {
        assert_eq!(parse("hello"), Some(Action::Type("hello\n".into())));
        assert_eq!(
            parse(":file.open a b.txt"),
            Some(Action::Command(Command::Open, "a b.txt".into()))
        );
        assert_eq!(
            parse(":+word-left"),
            Some(Action::Move(Motion::WordLeft, true))
        );
        assert_eq!(parse(":nonsense"), None);
    }

    #[test]
    fn every_command_and_motion_can_be_asked_for() {
        for command in Command::ALL {
            assert!(parse(&format!(":{}", command.id())).is_some());
        }
        for motion in Motion::ALL {
            assert!(parse(&format!(":{}", motion.name())).is_some());
        }
    }

    #[test]
    fn typing_reaches_the_core_and_the_core_asks_for_a_repaint() {
        let shell = run(&["one"]);
        assert!(shell.redraw.0.load(Ordering::SeqCst));
        assert_eq!(shell.paint(), "  1 one\n  2 |\n--- Untitled • \n");
    }

    #[test]
    fn the_frame_is_what_the_core_laid_out() {
        // 45 characters in a 40-column frame: the core wraps, the shell only prints rows.
        let shell = run(&[&"word ".repeat(9), ":up", ":+row-end"]);
        let frame = shell.paint();
        let rows: Vec<&str> = frame.lines().collect();
        assert_eq!(rows[0], "  1 word word word word word word word word ");
        assert_eq!(rows[1], "    [word ]");
    }

    #[test]
    fn undo_is_the_cores() {
        let shell = run(&["one", ":edit.undo"]);
        assert_eq!(shell.paint(), "  1 |\n--- Untitled \n");
    }
}
