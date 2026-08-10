//! The platform-agnostic editor core.
//!
//! Every shell — the CLI, the TUI, GTK4, SwiftUI, WinUI — drives the editor through
//! [`Editor`] and renders from [`Viewport`]. No shell holds editor state of its own,
//! and no capability may exist in a shell that is missing here.
//!
//! Rust shells depend on this crate directly. macOS and Windows will reach it through
//! UniFFI-generated bindings; [`Editor`] is shaped for that (an opaque object with
//! interior mutability and plain-scalar arguments) but the annotations are not added yet.

mod action;
mod error;
mod state;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

pub use action::Action;
pub use error::{EditorError, Result};
pub use state::{Direction, EditorState, Position, Viewport};

/// Implemented by the shell, called by the core when the document changes.
///
/// This is the reactive half of the architecture: shells never poll. On macOS and
/// Windows this becomes a UniFFI foreign trait implemented in Swift/C#.
pub trait EditorObserver: Send + Sync {
    fn state_changed(&self);
}

/// The handle every shell talks to.
///
/// All methods take `&self` — the lock lives inside — so a shell can share one
/// `Arc<Editor>` between its UI thread and any background work.
pub struct Editor {
    state: RwLock<EditorState>,
    observer: RwLock<Option<Arc<dyn EditorObserver>>>,
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new()
    }
}

impl Editor {
    /// An empty, unnamed document.
    pub fn new() -> Self {
        Editor {
            state: RwLock::new(EditorState::default()),
            observer: RwLock::new(None),
        }
    }

    /// An editor with `path` already loaded.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let editor = Editor::new();
        editor.load_file(path)?;
        Ok(editor)
    }

    pub fn set_observer(&self, observer: Arc<dyn EditorObserver>) {
        *self.observer.write().unwrap() = Some(observer);
    }

    // --- document -------------------------------------------------------------

    pub fn load_file(&self, path: impl AsRef<Path>) -> Result<()> {
        self.mutate(|state| state.load_file(path.as_ref()))
    }

    pub fn save_file(&self) -> Result<()> {
        self.mutate(|state| state.save())
    }

    pub fn save_file_as(&self, path: impl AsRef<Path>) -> Result<()> {
        self.mutate(|state| state.save_to(path.as_ref()))
    }

    /// Open `text` as the document, named `name`, as if it had been read from disk:
    /// cursor home, history cleared, not dirty. `None` leaves it unnamed.
    ///
    /// The counterpart of [`Editor::load_file`] for a shell whose platform does the
    /// reading — the browser, where a file arrives from the File API as a string and
    /// there is no path to open. Inserting the text instead would be wrong twice
    /// over: the document would start dirty, and undo would erase the file.
    pub fn load_text(&self, name: Option<&str>, text: &str) {
        self.mutate(|state| state.load_text(name.map(PathBuf::from), text));
    }

    /// The whole document, marked saved under `name` (`None` keeps the current one).
    ///
    /// The counterpart of [`Editor::save_file_as`] for a shell whose platform does
    /// the writing — the browser hands these bytes to a download. This is the one
    /// place a shell legitimately takes the entire buffer: saving is not rendering,
    /// and the read path for rendering is still [`Editor::get_viewport`].
    pub fn save_to_string(&self, name: Option<&str>) -> String {
        self.mutate(|state| state.save_to_string(name.map(PathBuf::from)))
    }

    // --- editing --------------------------------------------------------------

    /// Insert a single character at the cursor. The keystroke path for UI shells.
    pub fn handle_input(&self, ch: char) {
        self.insert_text(&ch.to_string());
    }

    /// Insert a string at the cursor, as one undoable action.
    pub fn insert_text(&self, text: &str) {
        self.mutate(|state| state.insert_text(text));
    }

    /// Delete the character before the cursor. False if already at the document start.
    pub fn handle_backspace(&self) -> bool {
        self.mutate(|state| state.backspace())
    }

    /// Undo the most recent action. False if there is nothing to undo.
    pub fn undo(&self) -> bool {
        self.mutate(|state| state.undo())
    }

    /// Redo the most recently undone action. False if there is nothing to redo.
    pub fn redo(&self) -> bool {
        self.mutate(|state| state.redo())
    }

    // --- cursor and view ------------------------------------------------------

    pub fn move_cursor(&self, direction: Direction) {
        self.mutate(|state| state.move_cursor(direction));
    }

    /// Jump the cursor to `position`, clamped into the document. Returns where it landed.
    pub fn set_cursor(&self, position: Position) -> Position {
        self.mutate(|state| state.set_cursor(position))
    }

    pub fn cursor(&self) -> Position {
        let state = self.state.read().unwrap();
        state.clamp(state.cursor)
    }

    /// Lines `start_line..end_line` (end exclusive) plus the cursor — the only read path.
    pub fn get_viewport(&self, start_line: u64, end_line: u64) -> Viewport {
        self.state.read().unwrap().viewport(start_line, end_line)
    }

    /// First visible line, owned by the core so every shell scrolls identically.
    pub fn scroll_offset(&self) -> u64 {
        self.state.read().unwrap().scroll_offset
    }

    /// Scroll just enough to keep the cursor visible in a viewport `height` lines
    /// tall, and return the resulting offset.
    ///
    /// Every graphical shell needs this and none of them should invent their own
    /// rule, so it lives here rather than in the TUI, GTK and Qt shells separately.
    ///
    /// Deliberately does not notify when the offset does not move: shells call this
    /// while laying out a frame, and an unconditional notification would have each
    /// redraw request the next one forever.
    pub fn follow_cursor(&self, height: u64) -> u64 {
        let (current, wanted) = {
            let state = self.state.read().unwrap();
            (state.scroll_offset, state.wanted_offset(height))
        };
        if wanted != current {
            self.mutate(|state| state.scroll_offset = wanted);
        }
        wanted
    }

    pub fn set_scroll_offset(&self, offset: u64) {
        self.mutate(|state| {
            let last_line = state.line_count().saturating_sub(1);
            state.scroll_offset = offset.min(last_line);
        });
    }

    // --- inspection -----------------------------------------------------------

    pub fn line_count(&self) -> u64 {
        self.state.read().unwrap().line_count()
    }

    pub fn char_count(&self) -> u64 {
        self.state.read().unwrap().char_count()
    }

    /// True when there are unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.state.read().unwrap().dirty
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.state.read().unwrap().path.clone()
    }

    pub fn can_undo(&self) -> bool {
        !self.state.read().unwrap().undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.state.read().unwrap().redo_stack.is_empty()
    }

    /// The whole document. Convenience for tests and small files — shells render
    /// from [`Editor::get_viewport`] instead.
    pub fn text(&self) -> String {
        self.state.read().unwrap().text.to_string()
    }

    // --- session --------------------------------------------------------------

    /// The state a stateless caller (the CLI) must carry between invocations.
    pub fn session(&self) -> Session {
        let state = self.state.read().unwrap();
        Session {
            cursor: state.cursor,
            scroll_offset: state.scroll_offset,
            undo_stack: state.undo_stack.clone(),
            redo_stack: state.redo_stack.clone(),
        }
    }

    pub fn restore_session(&self, session: Session) {
        self.mutate(|state| {
            state.cursor = state.clamp(session.cursor);
            state.scroll_offset = session.scroll_offset;
            state.undo_stack = session.undo_stack;
            state.redo_stack = session.redo_stack;
        });
    }

    // --- internals ------------------------------------------------------------

    /// Run `f` under the write lock, release it, *then* notify.
    ///
    /// The lock must be dropped first: an observer is free to call back into the
    /// editor to re-read state, which would otherwise deadlock.
    fn mutate<T>(&self, f: impl FnOnce(&mut EditorState) -> T) -> T {
        let out = {
            let mut state = self.state.write().unwrap();
            f(&mut state)
        };
        self.notify();
        out
    }

    fn notify(&self) {
        let observer = self.observer.read().unwrap().clone();
        if let Some(observer) = observer {
            observer.state_changed();
        }
    }
}

/// Cursor and history, serialized so a process that exits can pick up where it left off.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub cursor: Position,
    #[serde(default)]
    pub scroll_offset: u64,
    #[serde(default)]
    pub undo_stack: Vec<Action>,
    #[serde(default)]
    pub redo_stack: Vec<Action>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_with(text: &str) -> Editor {
        let editor = Editor::new();
        editor.insert_text(text);
        editor
    }

    #[test]
    fn insert_advances_the_cursor() {
        let editor = editor_with("hello");
        assert_eq!(editor.text(), "hello");
        assert_eq!(editor.cursor(), Position::new(0, 5));
    }

    #[test]
    fn insert_across_lines_tracks_the_cursor() {
        let editor = editor_with("ab\ncd");
        assert_eq!(editor.cursor(), Position::new(1, 2));
        assert_eq!(editor.line_count(), 2);
    }

    #[test]
    fn backspace_joins_lines() {
        let editor = editor_with("ab\ncd");
        editor.set_cursor(Position::new(1, 0));
        assert!(editor.handle_backspace());
        assert_eq!(editor.text(), "abcd");
        assert_eq!(editor.cursor(), Position::new(0, 2));
    }

    #[test]
    fn backspace_at_start_is_a_no_op() {
        let editor = editor_with("abc");
        editor.set_cursor(Position::new(0, 0));
        assert!(!editor.handle_backspace());
        assert_eq!(editor.text(), "abc");
    }

    #[test]
    fn undo_and_redo_round_trip() {
        let editor = editor_with("hello");
        editor.insert_text(" world");
        assert_eq!(editor.text(), "hello world");

        assert!(editor.undo());
        assert_eq!(editor.text(), "hello");
        assert_eq!(editor.cursor(), Position::new(0, 5));

        assert!(editor.undo());
        assert_eq!(editor.text(), "");
        assert!(!editor.undo());

        assert!(editor.redo());
        assert!(editor.redo());
        assert_eq!(editor.text(), "hello world");
        assert!(!editor.redo());
    }

    #[test]
    fn a_new_edit_drops_the_redo_stack() {
        let editor = editor_with("abc");
        editor.undo();
        editor.insert_text("xyz");
        assert!(!editor.can_redo());
        assert_eq!(editor.text(), "xyz");
    }

    #[test]
    fn undo_restores_a_backspaced_character() {
        let editor = editor_with("abc");
        editor.handle_backspace();
        assert_eq!(editor.text(), "ab");
        editor.undo();
        assert_eq!(editor.text(), "abc");
        assert_eq!(editor.cursor(), Position::new(0, 3));
    }

    #[test]
    fn horizontal_movement_crosses_line_boundaries() {
        let editor = editor_with("ab\ncd");
        editor.set_cursor(Position::new(1, 0));
        editor.move_cursor(Direction::Left);
        assert_eq!(editor.cursor(), Position::new(0, 2));
        editor.move_cursor(Direction::Right);
        assert_eq!(editor.cursor(), Position::new(1, 0));
    }

    #[test]
    fn vertical_movement_clamps_to_shorter_lines() {
        let editor = editor_with("long line\nx\n");
        editor.set_cursor(Position::new(0, 9));
        editor.move_cursor(Direction::Down);
        assert_eq!(editor.cursor(), Position::new(1, 1));
    }

    #[test]
    fn movement_stops_at_the_document_edges() {
        let editor = editor_with("ab");
        editor.set_cursor(Position::new(0, 0));
        editor.move_cursor(Direction::Left);
        editor.move_cursor(Direction::Up);
        assert_eq!(editor.cursor(), Position::new(0, 0));

        editor.set_cursor(Position::new(0, 2));
        editor.move_cursor(Direction::Right);
        editor.move_cursor(Direction::Down);
        assert_eq!(editor.cursor(), Position::new(0, 2));
    }

    #[test]
    fn viewport_returns_only_the_requested_lines() {
        let editor = editor_with("one\ntwo\nthree\nfour");
        let viewport = editor.get_viewport(1, 3);
        assert_eq!(viewport.start_line, 1);
        assert_eq!(viewport.lines, vec!["two", "three"]);
        assert_eq!(viewport.total_lines, 4);
    }

    #[test]
    fn viewport_clamps_out_of_range_requests() {
        let editor = editor_with("one\ntwo");
        let viewport = editor.get_viewport(0, 500);
        assert_eq!(viewport.lines, vec!["one", "two"]);

        // A start past the end lands on the last line rather than returning nothing,
        // so a stale scroll offset can never blank the view.
        let past_end = editor.get_viewport(99, 120);
        assert_eq!(past_end.start_line, 1);
        assert_eq!(past_end.lines, vec!["two"]);
    }

    #[test]
    fn an_empty_range_returns_no_lines() {
        let editor = editor_with("one\ntwo");
        assert!(editor.get_viewport(1, 1).lines.is_empty());
    }

    #[test]
    fn cursor_is_clamped_into_the_document() {
        let editor = editor_with("ab");
        assert_eq!(
            editor.set_cursor(Position::new(99, 99)),
            Position::new(0, 2)
        );
    }

    #[test]
    fn observers_are_notified_on_every_mutation() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Counter(AtomicUsize);
        impl EditorObserver for Counter {
            fn state_changed(&self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let editor = Editor::new();
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        editor.set_observer(counter.clone());

        editor.handle_input('a');
        editor.move_cursor(Direction::Left);
        editor.undo();
        assert_eq!(counter.0.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn an_observer_may_read_the_editor_without_deadlocking() {
        struct Reader(RwLock<Option<Arc<Editor>>>);
        impl EditorObserver for Reader {
            fn state_changed(&self) {
                if let Some(editor) = self.0.read().unwrap().as_ref() {
                    let _ = editor.get_viewport(0, 1);
                }
            }
        }

        let editor = Arc::new(Editor::new());
        let reader = Arc::new(Reader(RwLock::new(Some(editor.clone()))));
        editor.set_observer(reader);
        editor.handle_input('a');
        assert_eq!(editor.text(), "a");
    }

    #[test]
    fn follow_cursor_scrolls_only_when_the_cursor_leaves_the_view() {
        let editor = editor_with("l0\nl1\nl2\nl3\nl4\nl5");

        editor.set_cursor(Position::new(0, 0));
        assert_eq!(editor.follow_cursor(3), 0);

        // Inside the view: no movement.
        editor.set_cursor(Position::new(2, 0));
        assert_eq!(editor.follow_cursor(3), 0);

        // Below it: scroll just far enough to bring the cursor onto the last row.
        editor.set_cursor(Position::new(4, 0));
        assert_eq!(editor.follow_cursor(3), 2);

        // Above it: the cursor's line becomes the first row.
        editor.set_cursor(Position::new(1, 0));
        assert_eq!(editor.follow_cursor(3), 1);
    }

    #[test]
    fn follow_cursor_is_silent_when_nothing_moves() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Counter(AtomicUsize);
        impl EditorObserver for Counter {
            fn state_changed(&self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let editor = editor_with("l0\nl1\nl2");
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        editor.set_observer(counter.clone());

        // A shell calls this every frame; notifying here would make each redraw
        // schedule the next one and spin forever.
        editor.follow_cursor(10);
        editor.follow_cursor(10);
        assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn follow_cursor_tolerates_a_zero_height_view() {
        let editor = editor_with("l0\nl1");
        assert_eq!(editor.follow_cursor(0), 0);
    }

    #[test]
    fn a_session_carries_cursor_and_history_across_editors() {
        let editor = editor_with("hello");
        let session = editor.session();

        let resumed = Editor::new();
        resumed.insert_text("hello");
        resumed.restore_session(session);
        assert_eq!(resumed.cursor(), Position::new(0, 5));
        assert!(resumed.undo());
        assert_eq!(resumed.text(), "");
    }

    #[test]
    fn files_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!("editor-core-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("round-trip.txt");

        let editor = editor_with("first\nsecond");
        editor.save_file_as(&path).unwrap();
        assert!(!editor.is_dirty());

        let reopened = Editor::open(&path).unwrap();
        assert_eq!(reopened.text(), "first\nsecond");
        assert_eq!(reopened.cursor(), Position::new(0, 0));
        assert!(!reopened.can_undo());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn loaded_text_behaves_like_a_file_read_from_disk() {
        let editor = editor_with("scratch");
        editor.load_text(Some("notes.txt"), "one\ntwo");

        assert_eq!(editor.text(), "one\ntwo");
        assert_eq!(editor.cursor(), Position::new(0, 0));
        assert_eq!(editor.path().unwrap().display().to_string(), "notes.txt");
        assert!(!editor.is_dirty(), "a freshly loaded document is not dirty");
        // The previous document's history must not survive the load, or undo would
        // rewrite text that never came from this file.
        assert!(!editor.can_undo());
    }

    #[test]
    fn loading_text_without_a_name_leaves_the_document_unnamed() {
        let editor = Editor::new();
        editor.load_text(None, "text from a browser file picker");
        assert!(editor.path().is_none());
        assert!(matches!(editor.save_file(), Err(EditorError::NoPath)));
    }

    #[test]
    fn saving_to_a_string_hands_over_the_document_and_clears_dirty() {
        let editor = editor_with("one\ntwo");
        assert!(editor.is_dirty());

        assert_eq!(editor.save_to_string(Some("out.txt")), "one\ntwo");
        assert!(!editor.is_dirty());
        assert_eq!(editor.path().unwrap().display().to_string(), "out.txt");

        // A later save without a name keeps the one it was saved under.
        editor.insert_text("!");
        assert_eq!(editor.save_to_string(None), "one\ntwo!");
        assert_eq!(editor.path().unwrap().display().to_string(), "out.txt");
    }

    #[test]
    fn text_survives_a_round_trip_without_a_filesystem() {
        let editor = Editor::new();
        // No trailing newline, so a round trip that "helpfully" adds one is caught.
        editor.load_text(Some("a.txt"), "first\nsecond");
        let saved = editor.save_to_string(None);

        let reopened = Editor::new();
        reopened.load_text(Some("a.txt"), &saved);
        assert_eq!(reopened.text(), "first\nsecond");
        assert_eq!(reopened.line_count(), 2);
    }

    #[test]
    fn saving_without_a_path_is_an_error() {
        let editor = editor_with("x");
        assert!(matches!(editor.save_file(), Err(EditorError::NoPath)));
    }
}
