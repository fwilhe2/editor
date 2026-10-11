// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! [`App`]: the one handle every shell holds.
//!
//! **Every `pub fn` in `impl App` is a capability, and every capability must be reachable from
//! the CLI.** `cli/tests/parity.rs` reads this file, lists those methods, and fails unless
//! `doc/cli-parity.md` names the command that reaches each one (or says why none does). Adding
//! a method here without a CLI verb fails the build — which is the point: a capability only a
//! GUI has is a capability a script, a test and an agent cannot use.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::document::{Document, Edit, Position};
use crate::encoding::{self, Encoding};
use crate::error::{Error, Result};
use crate::history::History;
use crate::motion::{self, Delete, Motion, Search};
use crate::session::Session;
use crate::view::{Frame, Scroll, View, Visual};

/// Implemented by a shell, called by the core after anything changed.
///
/// The reactive half of the architecture: a shell never polls. Called **after** the write lock
/// is released (see [`App`]), on whichever thread made the change — so a shell whose UI objects
/// are not `Send` forwards the notification to its UI thread rather than drawing here.
pub trait Observer: Send + Sync {
    fn changed(&self);
}

/// The editor: one document, its caret, its history, and the observers watching it.
///
/// Every method takes `&self`; the lock is inside, so a shell can share one `Arc<App>` between
/// its UI thread and its observer.
pub struct App {
    state: RwLock<State>,
    observers: RwLock<Vec<Arc<dyn Observer>>>,
}

struct State {
    doc: Document,
    encoding: Encoding,
    path: Option<PathBuf>,
    caret: usize,
    /// The other end of the selection; a selection exists when this differs from `caret`.
    anchor: Option<usize>,
    /// The x a run of Up/Down presses is trying to stay at, in the shell's unit.
    goal_x: Option<f32>,
    scroll: Scroll,
    history: History,
    /// The last thing searched for, so Find Next means the same thing in every shell.
    search: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        State {
            doc: Document::default(),
            encoding: Encoding::default(),
            path: None,
            caret: 0,
            anchor: None,
            goal_x: None,
            scroll: Scroll::default(),
            history: History::saved(),
            search: None,
        }
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

impl App {
    /// An empty, untitled document.
    pub fn new() -> App {
        App {
            state: RwLock::new(State::default()),
            observers: RwLock::new(Vec::new()),
        }
    }

    /// Start calling `observer` after every change. Shells register one; a test may register
    /// several.
    pub fn observe(&self, observer: Arc<dyn Observer>) {
        self.observers.write().unwrap().push(observer);
    }

    // --- the document ----------------------------------------------------------------------

    /// Replace the document with an empty, untitled one.
    pub fn new_document(&self) {
        self.mutate(|state| *state = State::default());
    }

    /// Read `path` and make it the document.
    pub fn open_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| Error::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let (text, encoding) = encoding::decode(&path.display().to_string(), &bytes)?;
        self.mutate(|state| state.load(Some(path.to_path_buf()), &text, encoding));
        Ok(())
    }

    /// Make `bytes` the document, named `name` — what a platform with no filesystem calls
    /// (the browser's file picker hands over bytes, never a path).
    ///
    /// Every `*_file` method has a `*_bytes` twin, so no capability assumes a disk. And this is
    /// a *load*, not a big insert: the caret goes home, history is cleared and the document is
    /// clean. Inserting the text instead would leave it dirty with an undo that erases the file.
    pub fn open_bytes(&self, name: Option<&str>, bytes: &[u8]) -> Result<()> {
        let (text, encoding) = encoding::decode(name.unwrap_or(crate::UNTITLED), bytes)?;
        self.mutate(|state| state.load(name.map(PathBuf::from), &text, encoding));
        Ok(())
    }

    /// Write the document where it came from.
    pub fn save_file(&self) -> Result<()> {
        let path = self.path().ok_or(Error::NoPath)?;
        self.save_file_as(path)
    }

    /// Write the document to `path`, which becomes where it came from.
    pub fn save_file_as(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let bytes = {
            let state = self.state.read().unwrap();
            encoding::encode(&state.doc.text(), state.encoding)
        };
        std::fs::write(path, bytes).map_err(|source| Error::Write {
            path: path.to_path_buf(),
            source,
        })?;
        self.mutate(|state| {
            state.path = Some(path.to_path_buf());
            state.history.mark_saved();
        });
        Ok(())
    }

    /// The document's bytes, in the encoding it was read in, counted as saved under `name`
    /// (`None` keeps the current name) — [`App::save_file_as`]'s twin for a platform that does
    /// the writing itself, such as a browser download.
    ///
    /// The one place a shell legitimately receives the whole document: saving is not rendering.
    pub fn save_bytes(&self, name: Option<&str>) -> Vec<u8> {
        self.mutate(|state| {
            if let Some(name) = name {
                state.path = Some(PathBuf::from(name));
            }
            state.history.mark_saved();
            encoding::encode(&state.doc.text(), state.encoding)
        })
    }

    // --- editing ---------------------------------------------------------------------------

    /// Type `text` at the caret, replacing the selection — the keystroke path. Consecutive
    /// typed characters undo together, a word at a time ([`crate::history`]).
    pub fn type_text(&self, text: &str) {
        self.mutate(|state| state.replace_selection(text, true));
    }

    /// Insert `text` at the caret, replacing the selection, as one undoable change — the paste
    /// path. CRLF in the text becomes LF, since the clipboard on Windows speaks CRLF.
    pub fn insert_text(&self, text: &str) {
        self.mutate(|state| state.replace_selection(text, false));
    }

    /// Delete the selection, or — with none — what `kind` names. False when there was nothing
    /// to delete (Backspace at the start of the document).
    pub fn delete(&self, kind: Delete) -> bool {
        self.mutate(|state| {
            if state.selection().is_some() {
                return state.replace_selection("", false);
            }
            let caret = state.caret;
            let target = state.resolve(kind.motion(), None);
            let (from, to) = (caret.min(target), caret.max(target));
            if from == to {
                return false;
            }
            state.anchor = Some(from);
            state.caret = to;
            state.replace_selection("", false)
        })
    }

    /// Undo the most recent change. False when there is nothing to undo.
    pub fn undo(&self) -> bool {
        self.mutate(|state| match state.history.undo() {
            Some(edit) => {
                state.apply(&edit);
                true
            }
            None => false,
        })
    }

    /// Redo the change most recently undone. False when there is nothing to redo.
    pub fn redo(&self) -> bool {
        self.mutate(|state| match state.history.redo() {
            Some(edit) => {
                state.apply(&edit);
                true
            }
            None => false,
        })
    }

    pub fn can_undo(&self) -> bool {
        self.state.read().unwrap().history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.state.read().unwrap().history.can_redo()
    }

    // --- the caret and the selection ---------------------------------------------------------

    /// Move the caret. `extend` grows the selection (Shift held) instead of collapsing it.
    ///
    /// Up, Down, the row ends and the page motions are measured in rows on screen, which is why
    /// a [`Frame`] comes along: the same motion moves differently in a narrow window, and the
    /// core is where that is decided ([`crate::layout`]).
    pub fn move_caret(&self, motion: Motion, extend: bool, frame: &Frame) {
        self.mutate(|state| {
            let had_selection = state.selection();
            let anchor = state.anchor.unwrap_or(state.caret);
            let target = match (had_selection, extend, motion) {
                // An arrow with a selection and no Shift lands on that side of it.
                (Some((from, _)), false, Motion::Left) => from,
                (Some((_, to)), false, Motion::Right) => to,
                _ => state.resolve(motion, Some(frame)),
            };
            if matches!(motion, Motion::PageUp | Motion::PageDown) {
                let rows = (frame.rows() as i64 - 1).max(1);
                let delta = if motion == Motion::PageDown {
                    rows
                } else {
                    -rows
                };
                let visual = Visual {
                    doc: &state.doc,
                    frame,
                };
                state.scroll = visual.step(visual.clamp(state.scroll), delta);
            }
            if !motion.is_vertical() {
                state.goal_x = None;
            }
            state.caret = target;
            state.anchor = extend.then_some(anchor);
            state.history.seal();
        });
    }

    /// Put the caret at `position` (clamped into the document) and return where it landed.
    pub fn set_caret(&self, position: Position, extend: bool) -> Position {
        self.mutate(|state| {
            let anchor = state.anchor.unwrap_or(state.caret);
            state.caret = state.doc.offset(position);
            state.anchor = extend.then_some(anchor);
            state.goal_x = None;
            state.history.seal();
            state.doc.position(state.caret)
        })
    }

    /// Put the caret under a point in the frame — a click, or with `extend` a Shift+click or a
    /// drag. `x` and `y` are measured from the frame's top left, in the shell's unit.
    pub fn place_caret(&self, frame: &Frame, x: f32, y: f32, extend: bool) -> Position {
        self.mutate(|state| {
            let anchor = state.anchor.unwrap_or(state.caret);
            let visual = Visual {
                doc: &state.doc,
                frame,
            };
            state.caret = visual.hit(state.scroll, x, y);
            state.anchor = extend.then_some(anchor);
            state.goal_x = None;
            state.history.seal();
            state.doc.position(state.caret)
        })
    }

    /// Select from `anchor` to `caret`.
    pub fn select(&self, anchor: Position, caret: Position) {
        self.mutate(|state| {
            state.anchor = Some(state.doc.offset(anchor));
            state.caret = state.doc.offset(caret);
            state.goal_x = None;
            state.history.seal();
        });
    }

    pub fn select_all(&self) {
        self.mutate(|state| {
            state.anchor = Some(0);
            state.caret = state.doc.len();
            state.goal_x = None;
            state.history.seal();
        });
    }

    /// Drop the selection, keeping the caret where it is. False when there was none.
    pub fn clear_selection(&self) -> bool {
        self.mutate(|state| state.anchor.take().is_some_and(|a| a != state.caret))
    }

    pub fn caret(&self) -> Position {
        let state = self.state.read().unwrap();
        state.doc.position(state.caret)
    }

    /// The selection's two ends in document order, or `None` when nothing is selected.
    pub fn selection(&self) -> Option<(Position, Position)> {
        let state = self.state.read().unwrap();
        state
            .selection()
            .map(|(from, to)| (state.doc.position(from), state.doc.position(to)))
    }

    /// The selected text — what Copy puts on the clipboard. Empty when nothing is selected.
    pub fn selected_text(&self) -> String {
        let state = self.state.read().unwrap();
        state
            .selection()
            .map_or_else(String::new, |(from, to)| state.doc.slice(from, to))
    }

    /// Remove the selection and return it — Cut. The clipboard is the shell's; the text is
    /// the core's.
    pub fn cut(&self) -> String {
        self.mutate(|state| {
            let Some((from, to)) = state.selection() else {
                return String::new();
            };
            let text = state.doc.slice(from, to);
            state.replace_selection("", false);
            text
        })
    }

    /// Select the next match of `query` after the selection (or before it, backwards), wrapping
    /// round the document's end. False when there is no match anywhere.
    pub fn find(&self, query: &str, direction: Search) -> bool {
        self.mutate(|state| {
            state.search = Some(query.to_string());
            state.find(direction)
        })
    }

    /// [`App::find`] again with the last query. False when there is none, or no match.
    pub fn find_again(&self, direction: Search) -> bool {
        self.mutate(|state| state.find(direction))
    }

    /// The last query given to [`App::find`] — what a Find prompt opens with.
    pub fn search(&self) -> Option<String> {
        self.state.read().unwrap().search.clone()
    }

    // --- what is on screen -----------------------------------------------------------------

    /// The rows that fill `frame` from the current scroll position, with the selection and the
    /// caret — **the** read path for a shell that paints.
    ///
    /// Never scrolls. A shell calls [`App::follow_caret`] after handling input and this while
    /// painting; if painting followed the caret, dragging a scrollbar away from it would snap
    /// straight back.
    pub fn view(&self, frame: &Frame) -> View {
        let state = self.state.read().unwrap();
        let visual = Visual {
            doc: &state.doc,
            frame,
        };
        visual.view(state.scroll, state.caret, state.selection())
    }

    /// Scroll just far enough for the caret to be inside `frame`. True when it scrolled.
    ///
    /// **Silent when nothing moves**: a shell calls this on every keystroke, and a notification
    /// here would make each repaint request the next one forever.
    pub fn follow_caret(&self, frame: &Frame) -> bool {
        let (current, wanted) = {
            let state = self.state.read().unwrap();
            let visual = Visual {
                doc: &state.doc,
                frame,
            };
            (state.scroll, visual.follow(state.scroll, state.caret))
        };
        if wanted == current {
            return false;
        }
        self.mutate(|state| state.scroll = wanted);
        true
    }

    /// Scroll by `rows` rows on screen (negative is up) — a wheel or a scrollbar.
    pub fn scroll_by(&self, frame: &Frame, rows: i64) {
        self.mutate(|state| {
            let visual = Visual {
                doc: &state.doc,
                frame,
            };
            state.scroll = visual.step(visual.clamp(state.scroll), rows);
        });
    }

    /// The first row on screen.
    pub fn scroll(&self) -> Scroll {
        self.state.read().unwrap().scroll
    }

    /// Put `line`'s first row at the top of the frame — a scrollbar dragged to a line.
    pub fn scroll_to(&self, line: u64) {
        self.mutate(|state| {
            let last = state.doc.line_count() as u64 - 1;
            state.scroll = Scroll {
                line: line.min(last),
                row: 0,
            };
        });
    }

    /// Lines `start..end` as they are in the document, unwrapped — the windowed read for a
    /// caller with no frame (the CLI's `view`). Clamped: a start past the end yields the last
    /// line rather than nothing, so a stale offset never shows an empty page.
    pub fn lines(&self, start: u64, end: u64) -> Vec<String> {
        let state = self.state.read().unwrap();
        let count = state.doc.line_count() as u64;
        let start = start.min(count - 1);
        (start..end.clamp(start, count))
            .map(|line| state.doc.line_text(line as usize))
            .collect()
    }

    // --- about the document ----------------------------------------------------------------

    pub fn line_count(&self) -> u64 {
        self.state.read().unwrap().doc.line_count() as u64
    }

    pub fn char_count(&self) -> u64 {
        self.state.read().unwrap().doc.len() as u64
    }

    /// Whether there are changes since the last save — a position in the history, so undoing
    /// back to the save makes it false again.
    pub fn is_dirty(&self) -> bool {
        self.state.read().unwrap().history.is_dirty()
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.state.read().unwrap().path.clone()
    }

    /// What to call the document in a title bar: its file name, or [`crate::UNTITLED`]. How the
    /// title is *built* — the dirty marker, the app's name, the order — is each platform's own
    /// convention, so that part stays in the shells.
    pub fn title(&self) -> String {
        self.path()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| crate::UNTITLED.to_string())
    }

    /// How the file on disk spells its line endings and whether it has a byte-order mark —
    /// what Save will write back.
    pub fn encoding(&self) -> Encoding {
        self.state.read().unwrap().encoding
    }

    // --- sessions --------------------------------------------------------------------------

    /// What a stateless caller (the CLI) must carry to the next invocation.
    pub fn session(&self) -> Session {
        let state = self.state.read().unwrap();
        Session {
            caret: state.doc.position(state.caret),
            anchor: state.anchor.map(|a| state.doc.position(a)),
            scroll: state.scroll,
            history: state.history.clone(),
            search: state.search.clone(),
        }
    }

    pub fn restore_session(&self, session: Session) {
        self.mutate(|state| {
            state.caret = state.doc.offset(session.caret);
            state.anchor = session.anchor.map(|a| state.doc.offset(a));
            state.scroll = session.scroll;
            state.history = session.history;
            state.search = session.search;
        });
    }

    // --- internals -------------------------------------------------------------------------

    /// Run `f` under the write lock, release the lock, **then** notify.
    ///
    /// In that order or not at all: an observer is expected to call straight back in to re-read
    /// (that is what it is for), and doing so while this thread still held the write lock would
    /// deadlock. `an_observer_may_read_back_without_deadlocking` pins it.
    fn mutate<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let out = {
            let mut state = self.state.write().unwrap();
            f(&mut state)
        };
        let observers = self.observers.read().unwrap().clone();
        for observer in observers {
            observer.changed();
        }
        out
    }
}

impl State {
    fn load(&mut self, path: Option<PathBuf>, text: &str, encoding: Encoding) {
        *self = State {
            doc: Document::new(text),
            encoding,
            path,
            search: self.search.take(),
            ..State::default()
        };
    }

    fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.caret).then(|| (anchor.min(self.caret), anchor.max(self.caret)))
    }

    /// Apply an edit that is not a new one — an undo or a redo.
    fn apply(&mut self, edit: &Edit) {
        self.doc.apply(edit);
        self.caret = edit.end();
        self.anchor = None;
        self.goal_x = None;
    }

    fn replace_selection(&mut self, text: &str, typed: bool) -> bool {
        let text = text.replace("\r\n", "\n");
        let (from, to) = self.selection().unwrap_or((self.caret, self.caret));
        if from == to && text.is_empty() {
            return false;
        }
        let edit = Edit {
            at: from,
            removed: self.doc.slice(from, to),
            inserted: text,
        };
        self.apply(&edit);
        let typed = typed && from == to && edit.inserted.chars().count() == 1;
        self.history.record(edit, typed);
        true
    }

    /// Where `motion` takes the caret. `frame` is `None` only for the motions [`Delete`] uses,
    /// none of which is measured in rows.
    fn resolve(&mut self, motion: Motion, frame: Option<&Frame>) -> usize {
        let doc = &self.doc;
        let caret = self.caret;
        match motion {
            Motion::Left => motion::grapheme_left(doc, caret),
            Motion::Right => motion::grapheme_right(doc, caret),
            Motion::WordLeft => motion::word_left(doc, caret),
            Motion::WordRight => motion::word_right(doc, caret),
            Motion::DocumentStart => 0,
            Motion::DocumentEnd => doc.len(),
            Motion::RowStart | Motion::RowEnd => {
                let frame = frame.expect("row motions are measured in a frame");
                let visual = Visual { doc, frame };
                let (line, row, layout) = visual.locate(caret);
                let start = doc.line_start(line);
                let span = layout.rows()[row];
                if motion == Motion::RowStart {
                    return start + span.start;
                }
                // ponytail: at a soft break the row's own end is drawn on the next row, so End
                // stops before a hanging space. A row broken inside a word has no space to stop
                // before, and its End shows at the next row's start; fixing that needs a caret
                // affinity (before/after the break) the model does not carry yet.
                let last = row + 1 == layout.rows().len();
                let hangs = span.end > span.start
                    && doc
                        .char_at(start + span.end - 1)
                        .is_some_and(char::is_whitespace);
                start
                    + if !last && hangs {
                        span.end - 1
                    } else {
                        span.end
                    }
            }
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => {
                let frame = frame.expect("vertical motions are measured in a frame");
                let visual = Visual { doc, frame };
                let (line, row, layout) = visual.locate(caret);
                let goal = *self
                    .goal_x
                    .get_or_insert_with(|| layout.x_of(caret - doc.line_start(line)));
                let rows = match motion {
                    Motion::Up => -1,
                    Motion::Down => 1,
                    Motion::PageUp => -(frame.rows() as i64 - 1).max(1),
                    _ => (frame.rows() as i64 - 1).max(1),
                };
                let here = Scroll {
                    line: line as u64,
                    row: row as u64,
                };
                let there = visual.step(here, rows);
                if there == here {
                    // Up on the first row goes to the start, Down on the last to the end —
                    // what every platform's text field does.
                    return if rows < 0 { 0 } else { doc.len() };
                }
                let layout = visual.layout(there.line as usize);
                doc.line_start(there.line as usize) + layout.offset_at(there.row as usize, goal)
            }
        }
    }

    fn find(&mut self, direction: Search) -> bool {
        let Some(query) = self.search.clone().filter(|q| !q.is_empty()) else {
            return false;
        };
        let (from, to) = self.selection().unwrap_or((self.caret, self.caret));
        let found = match direction {
            Search::Forward => self
                .doc
                .find(&query, to)
                .or_else(|| self.doc.find(&query, 0)),
            Search::Backward => self
                .doc
                .rfind(&query, from)
                .or_else(|| self.doc.rfind(&query, self.doc.len())),
        };
        let Some(at) = found else {
            return false;
        };
        self.anchor = Some(at);
        self.caret = at + query.chars().count();
        self.goal_x = None;
        self.history.seal();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn app(text: &str) -> App {
        let app = App::new();
        app.open_bytes(None, text.as_bytes()).unwrap();
        app
    }

    fn text(app: &App) -> String {
        app.state.read().unwrap().doc.text()
    }

    fn frame(width: u32, rows: u32) -> Frame<'static> {
        Frame::fixed(width, rows)
    }

    struct Counter(AtomicUsize);
    impl Observer for Counter {
        fn changed(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn typing_inserts_at_the_caret_and_undoes_a_word_at_a_time() {
        let app = App::new();
        for c in "hello world".chars() {
            app.type_text(&c.to_string());
        }
        assert_eq!(text(&app), "hello world");
        assert_eq!(app.caret(), Position::new(0, 11));
        assert!(app.undo());
        assert_eq!(text(&app), "hello");
        assert!(app.undo());
        assert_eq!(text(&app), "");
        assert!(app.redo() && app.redo());
        assert_eq!(text(&app), "hello world");
    }

    #[test]
    fn typing_over_a_selection_is_one_change() {
        let app = app("hello world");
        app.select(Position::new(0, 6), Position::new(0, 11));
        app.type_text("x");
        assert_eq!(text(&app), "hello x");
        app.undo();
        assert_eq!(text(&app), "hello world");
    }

    #[test]
    fn backspace_joins_lines_and_word_delete_takes_a_word() {
        let app = app("ab\ncd");
        app.set_caret(Position::new(1, 0), false);
        assert!(app.delete(Delete::Backward));
        assert_eq!(text(&app), "abcd");
        assert_eq!(app.caret(), Position::new(0, 2));

        let app = self::app("one two");
        app.set_caret(Position::new(0, 7), false);
        app.delete(Delete::WordBackward);
        assert_eq!(text(&app), "one ");
        app.set_caret(Position::new(0, 0), false);
        assert!(!app.delete(Delete::Backward), "nothing before the start");
    }

    #[test]
    fn shift_motions_select_and_plain_ones_collapse() {
        let app = app("hello");
        let f = frame(0, 10);
        app.move_caret(Motion::Right, true, &f);
        app.move_caret(Motion::Right, true, &f);
        assert_eq!(app.selected_text(), "he");
        app.move_caret(Motion::Left, false, &f);
        assert_eq!(
            app.caret(),
            Position::new(0, 0),
            "Left lands on the selection's start"
        );
        assert_eq!(app.selection(), None);
    }

    #[test]
    fn down_moves_by_rows_on_screen_and_keeps_its_column() {
        // Wrapped at 6: "hello " / "world" — then a short line, then a long one.
        let app = app("hello world\nab\nabcdefg");
        let f = frame(6, 10);
        app.set_caret(Position::new(0, 4), false);
        app.move_caret(Motion::Down, false, &f);
        assert_eq!(app.caret(), Position::new(0, 10), "the wrapped row below");
        app.move_caret(Motion::Down, false, &f);
        assert_eq!(app.caret(), Position::new(1, 2), "clamped on a short line");
        app.move_caret(Motion::Down, false, &f);
        assert_eq!(
            app.caret(),
            Position::new(2, 4),
            "and back to the column it started in"
        );
        app.move_caret(Motion::Down, false, &f);
        assert_eq!(
            app.caret(),
            Position::new(2, 7),
            "a wrapped row of the same line"
        );
        app.move_caret(Motion::Down, false, &f);
        assert_eq!(
            app.caret(),
            Position::new(2, 7),
            "Down on the last row goes to the end"
        );
    }

    #[test]
    fn home_and_end_mean_the_row_on_screen() {
        let app = app("hello world");
        let f = frame(6, 10);
        app.set_caret(Position::new(0, 2), false);
        app.move_caret(Motion::RowEnd, false, &f);
        assert_eq!(app.caret(), Position::new(0, 5), "before the hanging space");
        app.set_caret(Position::new(0, 8), false);
        app.move_caret(Motion::RowStart, false, &f);
        assert_eq!(app.caret(), Position::new(0, 6));
    }

    #[test]
    fn the_view_holds_only_what_fits_and_marks_the_selection() {
        let app = app("one\ntwo\nthree\nfour");
        app.select(Position::new(1, 1), Position::new(2, 2));
        let view = app.view(&frame(0, 2));
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[1].text, "two");
        let selected = view.rows[1].selection.unwrap();
        assert_eq!(
            (selected.from, selected.to),
            (1.0, 3.5),
            "the line break shows as half a row"
        );
        assert_eq!(
            view.caret, None,
            "the caret is on the third line, off screen"
        );
    }

    #[test]
    fn follow_caret_scrolls_only_when_the_caret_leaves_the_frame() {
        let app = app("l0\nl1\nl2\nl3\nl4\nl5");
        let f = frame(0, 3);
        app.set_caret(Position::new(2, 0), false);
        assert!(!app.follow_caret(&f));
        app.set_caret(Position::new(4, 0), false);
        assert!(app.follow_caret(&f));
        assert_eq!(app.scroll(), Scroll { line: 2, row: 0 });
        app.set_caret(Position::new(1, 0), false);
        assert!(app.follow_caret(&f));
        assert_eq!(app.scroll(), Scroll { line: 1, row: 0 });
    }

    #[test]
    fn follow_caret_is_silent_when_nothing_moves() {
        let app = app("l0\nl1");
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        app.observe(counter.clone());
        app.follow_caret(&frame(0, 10));
        app.follow_caret(&frame(0, 10));
        assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_click_lands_on_the_row_under_it() {
        let app = app("hello world\nnext");
        let f = frame(6, 10);
        assert_eq!(app.place_caret(&f, 2.4, 1.5, false), Position::new(0, 8));
        assert_eq!(app.place_caret(&f, 99.0, 99.0, false), Position::new(1, 4));
    }

    #[test]
    fn find_selects_and_wraps_round() {
        let app = app("one two one");
        let at = |column| Some((Position::new(0, column), Position::new(0, column + 3)));
        assert!(app.find("one", Search::Forward));
        assert_eq!(app.selection(), at(0));
        assert!(app.find_again(Search::Forward));
        assert_eq!(app.selection(), at(8));
        assert!(app.find_again(Search::Forward));
        assert_eq!(
            app.selection(),
            at(0),
            "past the last match, round to the first"
        );
        assert!(app.find_again(Search::Backward));
        assert_eq!(app.selection(), at(8), "and backwards round the start");
        assert!(!app.find("zzz", Search::Forward));
    }

    #[test]
    fn cut_removes_the_selection_and_returns_it() {
        let app = app("hello world");
        app.select(Position::new(0, 5), Position::new(0, 11));
        assert_eq!(app.cut(), " world");
        assert_eq!(text(&app), "hello");
        app.insert_text("\r\nthere");
        assert_eq!(
            text(&app),
            "hello\nthere",
            "CRLF from a clipboard is one line break"
        );
    }

    #[test]
    fn dirty_is_a_position_in_the_history() {
        let app = app("x");
        assert!(!app.is_dirty());
        app.type_text("y");
        assert!(app.is_dirty());
        app.undo();
        assert!(!app.is_dirty(), "undoing back to the save is clean again");
    }

    #[test]
    fn typing_after_a_save_is_a_new_undo_entry() {
        let app = App::new();
        app.type_text("a");
        app.save_bytes(Some("a.txt"));
        app.type_text("b");
        assert!(app.is_dirty());
        app.undo();
        assert!(!app.is_dirty());
        assert_eq!(text(&app), "a");
    }

    #[test]
    fn loading_resets_caret_history_and_dirtiness() {
        let app = App::new();
        app.type_text("scratch");
        app.open_bytes(Some("notes.txt"), b"one\ntwo").unwrap();
        assert_eq!(app.caret(), Position::new(0, 0));
        assert!(!app.can_undo());
        assert!(!app.is_dirty());
        assert_eq!(app.title(), "notes.txt");
    }

    #[test]
    fn an_untouched_save_returns_the_bytes_that_were_read() {
        let bytes = b"\xEF\xBB\xBFone\r\ntwo\r\n";
        let app = App::new();
        app.open_bytes(Some("a.txt"), bytes).unwrap();
        assert_eq!(app.save_bytes(None), bytes);
        app.set_caret(Position::new(2, 0), false);
        app.type_text("three");
        assert_eq!(app.save_bytes(None), b"\xEF\xBB\xBFone\r\ntwo\r\nthree");
    }

    #[test]
    fn files_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!("quill-core-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("round-trip.txt");
        let app = app("first\nsecond");
        assert!(matches!(app.save_file(), Err(Error::NoPath)));
        app.save_file_as(&path).unwrap();
        let reopened = App::new();
        reopened.open_file(&path).unwrap();
        assert_eq!(text(&reopened), "first\nsecond");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn observers_hear_every_change() {
        let app = App::new();
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        app.observe(counter.clone());
        app.type_text("a");
        app.move_caret(Motion::Left, false, &frame(0, 1));
        app.undo();
        assert_eq!(counter.0.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn an_observer_may_read_back_without_deadlocking() {
        struct Reader(RwLock<Option<Arc<App>>>);
        impl Observer for Reader {
            fn changed(&self) {
                if let Some(app) = self.0.read().unwrap().as_ref() {
                    let _ = app.view(&Frame::fixed(10, 10));
                }
            }
        }
        let app = Arc::new(App::new());
        let reader = Arc::new(Reader(RwLock::new(Some(app.clone()))));
        app.observe(reader.clone());
        app.type_text("a");
        // Break the cycle, or the test leaks.
        *reader.0.write().unwrap() = None;
        assert_eq!(text(&app), "a");
    }

    #[test]
    fn a_session_carries_caret_selection_and_history() {
        let app = app("hello");
        app.set_caret(Position::new(0, 5), false);
        app.type_text("!");
        app.select(Position::new(0, 0), Position::new(0, 2));
        let session = app.session();

        let resumed = self::app("hello!");
        resumed.restore_session(session);
        assert_eq!(resumed.selected_text(), "he");
        assert!(resumed.undo());
        assert_eq!(text(&resumed), "hello");
    }

    #[test]
    fn page_down_moves_a_screenful_and_scrolls_with_it() {
        let text = (0..20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let app = app(&text);
        let f = frame(0, 5);
        app.move_caret(Motion::PageDown, false, &f);
        assert_eq!(app.caret().line, 4);
        assert_eq!(app.scroll().line, 4);
    }
}
