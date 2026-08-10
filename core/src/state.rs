use std::path::{Path, PathBuf};

use ropey::Rope;
use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::error::{EditorError, Result};

/// A zero-based (line, column) address. Columns count characters, not bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u64,
    pub column: u64,
}

impl Position {
    pub fn new(line: u64, column: u64) -> Self {
        Position { line, column }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// The slice of the document a shell is currently showing.
///
/// Shells render from this and nothing else — it is the only read path out of the
/// core, which is what keeps large files from being copied into the UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    pub start_line: u64,
    pub lines: Vec<String>,
    pub total_lines: u64,
    pub cursor: Position,
}

/// Everything the editor knows about the open document.
///
/// This type is deliberately not public API for shells: they go through
/// [`crate::Editor`], which owns the lock and the change notifications.
pub struct EditorState {
    pub(crate) text: Rope,
    pub(crate) cursor: Position,
    pub(crate) scroll_offset: u64,
    pub(crate) path: Option<PathBuf>,
    pub(crate) undo_stack: Vec<Action>,
    pub(crate) redo_stack: Vec<Action>,
    pub(crate) dirty: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        EditorState {
            text: Rope::new(),
            cursor: Position::default(),
            scroll_offset: 0,
            path: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            dirty: false,
        }
    }
}

impl EditorState {
    pub(crate) fn load_file(&mut self, path: &Path) -> Result<()> {
        let text = std::fs::read_to_string(path).map_err(|source| EditorError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        self.load_text(Some(path.to_path_buf()), &text);
        Ok(())
    }

    /// Become a freshly opened document holding `text`. The read half of the file
    /// API for shells whose platform, not the core, does the reading.
    pub(crate) fn load_text(&mut self, path: Option<PathBuf>, text: &str) {
        self.text = Rope::from_str(text);
        self.cursor = Position::default();
        self.scroll_offset = 0;
        self.path = path;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.dirty = false;
    }

    pub(crate) fn save_to(&mut self, path: &Path) -> Result<()> {
        std::fs::write(path, self.text.to_string()).map_err(|source| EditorError::Write {
            path: path.to_path_buf(),
            source,
        })?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    pub(crate) fn save(&mut self) -> Result<()> {
        let path = self.path.clone().ok_or(EditorError::NoPath)?;
        self.save_to(&path)
    }

    /// Hand the whole document to a caller that will write it somewhere the core
    /// cannot reach, and count that as saved. The write half of the file API for
    /// shells whose platform does the writing.
    pub(crate) fn save_to_string(&mut self, path: Option<PathBuf>) -> String {
        if let Some(path) = path {
            self.path = Some(path);
        }
        self.dirty = false;
        self.text.to_string()
    }

    /// Apply an edit without touching the undo/redo stacks.
    fn apply(&mut self, action: &Action) {
        match action {
            Action::Insert { at, text } => {
                let idx = self.char_index(*at);
                self.text.insert(idx, text);
                self.cursor = self.position_of(idx + text.chars().count());
            }
            Action::Delete { at, text } => {
                let idx = self.char_index(*at);
                let end = (idx + text.chars().count()).min(self.text.len_chars());
                self.text.remove(idx..end);
                self.cursor = self.position_of(idx);
            }
        }
        self.dirty = true;
    }

    /// Apply an edit as a new user action: undoable, and it invalidates the redo stack.
    fn edit(&mut self, action: Action) {
        self.apply(&action);
        self.undo_stack.push(action);
        self.redo_stack.clear();
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.edit(Action::Insert {
            at: self.clamp(self.cursor),
            text: text.to_string(),
        });
    }

    /// Delete the character before the cursor. Returns false at the start of the document.
    pub(crate) fn backspace(&mut self) -> bool {
        let idx = self.char_index(self.cursor);
        if idx == 0 {
            return false;
        }
        let removed = self.text.char(idx - 1);
        let at = self.position_of(idx - 1);
        self.edit(Action::Delete {
            at,
            text: removed.to_string(),
        });
        true
    }

    pub(crate) fn undo(&mut self) -> bool {
        let Some(action) = self.undo_stack.pop() else {
            return false;
        };
        self.apply(&action.inverse());
        self.redo_stack.push(action);
        true
    }

    pub(crate) fn redo(&mut self) -> bool {
        let Some(action) = self.redo_stack.pop() else {
            return false;
        };
        self.apply(&action);
        self.undo_stack.push(action);
        true
    }

    pub(crate) fn move_cursor(&mut self, direction: Direction) {
        let cursor = self.clamp(self.cursor);
        let last_line = self.text.len_lines().saturating_sub(1) as u64;
        self.cursor = match direction {
            Direction::Left => {
                let idx = self.char_index(cursor);
                if idx == 0 {
                    cursor
                } else {
                    self.position_of(idx - 1)
                }
            }
            Direction::Right => {
                let idx = self.char_index(cursor);
                if idx >= self.text.len_chars() {
                    cursor
                } else {
                    self.position_of(idx + 1)
                }
            }
            Direction::Up => {
                if cursor.line == 0 {
                    Position::new(0, 0)
                } else {
                    self.clamp(Position::new(cursor.line - 1, cursor.column))
                }
            }
            Direction::Down => {
                if cursor.line >= last_line {
                    self.clamp(Position::new(last_line, u64::MAX))
                } else {
                    self.clamp(Position::new(cursor.line + 1, cursor.column))
                }
            }
        };
    }

    pub(crate) fn set_cursor(&mut self, position: Position) -> Position {
        self.cursor = self.clamp(position);
        self.cursor
    }

    /// Lines `start_line..end_line` (end exclusive), clamped to the document.
    ///
    /// A `start_line` past the end is pulled back to the last line, so a stale scroll
    /// offset shows the end of the file instead of an empty view.
    pub(crate) fn viewport(&self, start_line: u64, end_line: u64) -> Viewport {
        let total = self.text.len_lines() as u64;
        let start = start_line.min(total.saturating_sub(1));
        let end = end_line.clamp(start, total);
        let lines = (start..end).map(|i| self.line_text(i)).collect();
        Viewport {
            start_line: start,
            lines,
            total_lines: total,
            cursor: self.clamp(self.cursor),
        }
    }

    /// Where the scroll offset has to be for the cursor to be visible in a viewport
    /// `height` lines tall. Returns the current offset when nothing needs to move.
    pub(crate) fn wanted_offset(&self, height: u64) -> u64 {
        if height == 0 {
            return self.scroll_offset;
        }
        let cursor = self.clamp(self.cursor);
        let offset = if cursor.line < self.scroll_offset {
            cursor.line
        } else if cursor.line >= self.scroll_offset + height {
            cursor.line + 1 - height
        } else {
            self.scroll_offset
        };
        offset.min(self.line_count().saturating_sub(1))
    }

    pub(crate) fn line_count(&self) -> u64 {
        self.text.len_lines() as u64
    }

    pub(crate) fn char_count(&self) -> u64 {
        self.text.len_chars() as u64
    }

    pub(crate) fn line_text(&self, line: u64) -> String {
        let line = line as usize;
        if line >= self.text.len_lines() {
            return String::new();
        }
        let slice = self.text.line(line);
        slice.slice(..self.line_len_chars(line)).to_string()
    }

    /// Characters on `line`, excluding the line terminator.
    fn line_len_chars(&self, line: usize) -> usize {
        let slice = self.text.line(line);
        let mut len = slice.len_chars();
        if len > 0 && slice.char(len - 1) == '\n' {
            len -= 1;
            if len > 0 && slice.char(len - 1) == '\r' {
                len -= 1;
            }
        }
        len
    }

    /// Pull a position inside the document, so callers can pass sloppy coordinates.
    pub(crate) fn clamp(&self, position: Position) -> Position {
        let last_line = self.text.len_lines().saturating_sub(1);
        let line = (position.line as usize).min(last_line);
        let column = (position.column as usize).min(self.line_len_chars(line));
        Position::new(line as u64, column as u64)
    }

    fn char_index(&self, position: Position) -> usize {
        let position = self.clamp(position);
        self.text.line_to_char(position.line as usize) + position.column as usize
    }

    fn position_of(&self, index: usize) -> Position {
        let index = index.min(self.text.len_chars());
        let line = self.text.char_to_line(index);
        Position::new(line as u64, (index - self.text.line_to_char(line)) as u64)
    }
}
