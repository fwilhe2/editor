//! What the CLI prints.
//!
//! Every command produces one report, rendered either as human text or as a single
//! JSON object. Diagnostics never go to stdout — callers parse it.

use editor_core::{Editor, Position};
use serde::Serialize;

/// A 1-based cursor address. The CLI is 1-based end to end; the core is 0-based,
/// and [`Cursor`] is the only place the two meet.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Cursor {
    pub line: u64,
    pub column: u64,
}

impl Cursor {
    /// 1-based (line, column) from the caller into a core position.
    pub fn to_core(line: u64, column: u64) -> Position {
        Position::new(line.saturating_sub(1), column.saturating_sub(1))
    }

    pub fn from_core(position: Position) -> Self {
        Cursor {
            line: position.line + 1,
            column: position.column + 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// Human-readable lines.
    Text,
    /// One JSON object, for agents and scripts.
    Json,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Report {
    View(ViewReport),
    Document(DocumentReport),
}

/// The result of a read.
#[derive(Debug, Serialize)]
pub struct ViewReport {
    pub path: String,
    pub start_line: u64,
    pub lines: Vec<String>,
    pub total_lines: u64,
    pub cursor: Cursor,
}

/// The result of everything else: where the document ended up.
#[derive(Debug, Serialize)]
pub struct DocumentReport {
    pub path: String,
    /// False when the command was a no-op (nothing to undo, cursor already at the edge).
    pub changed: bool,
    /// Whether the document was written back to disk.
    pub written: bool,
    pub cursor: Cursor,
    pub total_lines: u64,
    pub chars: u64,
    pub can_undo: bool,
    pub can_redo: bool,
}

impl DocumentReport {
    pub fn new(editor: &Editor, changed: bool, written: bool) -> Self {
        DocumentReport {
            path: editor
                .path()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            changed,
            written,
            cursor: Cursor::from_core(editor.cursor()),
            total_lines: editor.line_count(),
            chars: editor.char_count(),
            can_undo: editor.can_undo(),
            can_redo: editor.can_redo(),
        }
    }
}

impl Report {
    pub fn print(&self, format: Format) {
        match format {
            Format::Json => {
                println!("{}", serde_json::to_string(self).expect("report is serializable"))
            }
            Format::Text => self.print_text(),
        }
    }

    fn print_text(&self) {
        match self {
            // Bare lines, so `edit view` composes with grep, wc and friends.
            Report::View(view) => {
                for line in &view.lines {
                    println!("{line}");
                }
            }
            Report::Document(doc) => {
                println!(
                    "{}:{}:{}  {} lines, {} chars{}{}",
                    doc.path,
                    doc.cursor.line,
                    doc.cursor.column,
                    doc.total_lines,
                    doc.chars,
                    if doc.written { "" } else { "  (not written)" },
                    if doc.changed { "" } else { "  (no change)" },
                );
            }
        }
    }
}
