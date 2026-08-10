//! The UniFFI surface: how Swift and C# reach the editor core.
//!
//! This is a separate crate from `core` rather than annotations on `Editor`
//! itself, because the two audiences want different signatures. `Editor` takes
//! `impl AsRef<Path>` and returns `PathBuf`, `char` and `Option<PathBuf>` — none
//! of which cross an FFI boundary. Exporting it directly would mean degrading the
//! Rust API (Strings everywhere, no generics) for the benefit of foreign callers.
//! So the facade lives here, `core` stays idiomatic Rust, and the Rust shells
//! (CLI, TUI, GTK) never compile UniFFI at all.
//!
//! The facade is kept deliberately thin — every method forwards to exactly one
//! core call — so there is nowhere for behaviour to drift from the other shells.
//!
//! Positions here are 0-based, as in the core. Shells that show line numbers add
//! one for display, as the CLI, TUI and GTK shells all do.

use std::sync::Arc;

use editor_core::{Direction, Editor};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EditorError {
    #[error("{message}")]
    Failed { message: String },
}

impl From<editor_core::EditorError> for EditorError {
    fn from(error: editor_core::EditorError) -> Self {
        EditorError::Failed {
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct CursorPosition {
    pub line: u64,
    pub column: u64,
}

impl From<editor_core::Position> for CursorPosition {
    fn from(position: editor_core::Position) -> Self {
        CursorPosition {
            line: position.line,
            column: position.column,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum MoveDirection {
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

/// The slice of the document a foreign shell should render.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ViewportData {
    pub start_line: u64,
    pub lines: Vec<String>,
    pub total_lines: u64,
    pub cursor: CursorPosition,
}

/// Implemented in Swift or C#, called from Rust when the document changes.
///
/// This is the foreign-trait half of the architecture — the same contract the TUI
/// and GTK shells implement natively as `EditorObserver`.
#[uniffi::export(with_foreign)]
pub trait EditorListener: Send + Sync {
    fn state_changed(&self);
}

/// Adapts a foreign listener to the core's observer trait.
struct ListenerBridge(Arc<dyn EditorListener>);

impl editor_core::EditorObserver for ListenerBridge {
    fn state_changed(&self) {
        self.0.state_changed();
    }
}

/// The object foreign code holds. Owns nothing itself — the state is the core's.
#[derive(uniffi::Object)]
pub struct EditorHandle {
    inner: Editor,
}

#[uniffi::export]
impl EditorHandle {
    /// Open an existing file. Like every other shell, a missing file is an error
    /// rather than a silently created empty document.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, EditorError> {
        let inner = Editor::open(&path)?;
        Ok(Arc::new(EditorHandle { inner }))
    }

    #[uniffi::constructor]
    pub fn empty() -> Arc<Self> {
        Arc::new(EditorHandle {
            inner: Editor::new(),
        })
    }

    pub fn set_listener(&self, listener: Arc<dyn EditorListener>) {
        self.inner.set_observer(Arc::new(ListenerBridge(listener)));
    }

    // --- document -------------------------------------------------------------

    pub fn save(&self) -> Result<(), EditorError> {
        Ok(self.inner.save_file()?)
    }

    pub fn save_as(&self, path: String) -> Result<(), EditorError> {
        Ok(self.inner.save_file_as(&path)?)
    }

    pub fn path(&self) -> Option<String> {
        self.inner.path().map(|path| path.display().to_string())
    }

    // --- editing --------------------------------------------------------------

    /// Insert text at the cursor as one undoable action. Foreign shells send whole
    /// strings rather than `char`s, which have no representation across the ABI.
    pub fn insert_text(&self, text: String) {
        self.inner.insert_text(&text);
    }

    pub fn backspace(&self) -> bool {
        self.inner.handle_backspace()
    }

    pub fn undo(&self) -> bool {
        self.inner.undo()
    }

    pub fn redo(&self) -> bool {
        self.inner.redo()
    }

    // --- cursor and view ------------------------------------------------------

    pub fn move_cursor(&self, direction: MoveDirection) {
        self.inner.move_cursor(direction.into());
    }

    pub fn set_cursor(&self, line: u64, column: u64) -> CursorPosition {
        self.inner
            .set_cursor(editor_core::Position::new(line, column))
            .into()
    }

    pub fn cursor(&self) -> CursorPosition {
        self.inner.cursor().into()
    }

    /// Lines `start_line..end_line`, end exclusive. The only read path.
    pub fn viewport(&self, start_line: u64, end_line: u64) -> ViewportData {
        let viewport = self.inner.get_viewport(start_line, end_line);
        ViewportData {
            start_line: viewport.start_line,
            lines: viewport.lines,
            total_lines: viewport.total_lines,
            cursor: viewport.cursor.into(),
        }
    }

    /// Scroll just enough to keep the cursor visible, and return the new offset.
    pub fn follow_cursor(&self, height: u64) -> u64 {
        self.inner.follow_cursor(height)
    }

    pub fn scroll_offset(&self) -> u64 {
        self.inner.scroll_offset()
    }

    pub fn set_scroll_offset(&self, offset: u64) {
        self.inner.set_scroll_offset(offset);
    }

    // --- inspection -----------------------------------------------------------

    pub fn line_count(&self) -> u64 {
        self.inner.line_count()
    }

    pub fn char_count(&self) -> u64 {
        self.inner.char_count()
    }

    pub fn is_dirty(&self) -> bool {
        self.inner.is_dirty()
    }

    pub fn can_undo(&self) -> bool {
        self.inner.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.inner.can_redo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_facade_forwards_editing_to_the_core() {
        let handle = EditorHandle::empty();
        handle.insert_text("hello".to_string());
        assert_eq!(handle.char_count(), 5);
        assert_eq!(handle.cursor().column, 5);

        assert!(handle.backspace());
        assert_eq!(handle.char_count(), 4);

        assert!(handle.undo());
        assert_eq!(handle.char_count(), 5);
        assert!(handle.can_redo());
    }

    #[test]
    fn viewport_crosses_the_boundary_intact() {
        let handle = EditorHandle::empty();
        handle.insert_text("one\ntwo\nthree".to_string());

        let viewport = handle.viewport(1, 3);
        assert_eq!(viewport.start_line, 1);
        assert_eq!(viewport.lines, vec!["two", "three"]);
        assert_eq!(viewport.total_lines, 3);
    }

    #[test]
    fn positions_stay_zero_based_across_the_boundary() {
        let handle = EditorHandle::empty();
        handle.insert_text("ab\ncd".to_string());

        let cursor = handle.set_cursor(1, 1);
        assert_eq!((cursor.line, cursor.column), (1, 1));

        handle.move_cursor(MoveDirection::Up);
        assert_eq!((handle.cursor().line, handle.cursor().column), (0, 1));
    }

    #[test]
    fn a_foreign_listener_is_called_on_changes() {
        struct Counter(AtomicUsize);
        impl EditorListener for Counter {
            fn state_changed(&self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let handle = EditorHandle::empty();
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        handle.set_listener(counter.clone());

        handle.insert_text("x".to_string());
        handle.move_cursor(MoveDirection::Left);
        assert_eq!(counter.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn opening_a_missing_file_is_an_error() {
        let Err(EditorError::Failed { message }) =
            EditorHandle::open("/nonexistent/path/for/a/test".to_string())
        else {
            panic!("opening a missing file should fail");
        };
        assert!(message.contains("nonexistent"), "got {message:?}");
    }

    #[test]
    fn saving_without_a_path_reports_the_core_error() {
        let handle = EditorHandle::empty();
        handle.insert_text("x".to_string());
        assert!(handle.save().is_err());
    }
}
