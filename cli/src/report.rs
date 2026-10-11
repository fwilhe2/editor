// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! What the CLI prints, and the one place 1-based meets 0-based.
//!
//! Every command produces one [`Report`], rendered as human text or as a single JSON object.
//! Diagnostics never go to stdout: a caller parses it.

use quill_core::{App, Position};
use serde::Serialize;

/// A 1-based (line, column) address. **The CLI is 1-based end to end and the core is 0-based**,
/// and this type is the only place the two meet — no other file adds or subtracts one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Place {
    pub line: u64,
    pub column: u64,
}

impl Place {
    pub fn to_core(self) -> Position {
        Position::new(self.line.saturating_sub(1), self.column.saturating_sub(1))
    }

    pub fn from_core(position: Position) -> Self {
        Place {
            line: position.line + 1,
            column: position.column + 1,
        }
    }

    /// `LINE:COLUMN`, or a bare `LINE` meaning its first column.
    pub fn parse(text: &str) -> Result<Place, String> {
        let (line, column) = text.split_once(':').unwrap_or((text, "1"));
        let number = |part: &str| {
            part.trim()
                .parse::<u64>()
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| format!("{text:?} is not LINE:COLUMN (both start at 1)"))
        };
        Ok(Place {
            line: number(line)?,
            column: number(column)?,
        })
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// Human-readable lines.
    Text,
    /// One JSON object, for agents and scripts.
    Json,
}

/// Where the document is after a command — the report every command but the readers prints.
#[derive(Debug, Serialize)]
pub struct State {
    pub path: String,
    /// What a window's title bar would call the document.
    pub title: String,
    /// False when the command was a no-op (nothing to undo, no match, Backspace at the start).
    pub changed: bool,
    /// Whether the file was written.
    pub written: bool,
    pub caret: Place,
    pub selection: Option<[Place; 2]>,
    pub lines: u64,
    pub chars: u64,
    pub line_ending: &'static str,
    pub bom: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Unsaved changes. Always false after a command that wrote the file.
    pub dirty: bool,
    /// The first row on screen: its line (1-based) and which of that line's wrapped rows
    /// (1-based) it is.
    pub scroll: [u64; 2],
    /// The last query `find` was given, which `find-next` repeats.
    pub search: Option<String>,
}

impl State {
    pub fn of(app: &App, path: &str, changed: bool, written: bool) -> State {
        let encoding = app.encoding();
        State {
            path: app
                .path()
                .map_or_else(|| path.to_string(), |p| p.display().to_string()),
            title: app.title(),
            changed,
            written,
            caret: Place::from_core(app.caret()),
            selection: app
                .selection()
                .map(|(from, to)| [Place::from_core(from), Place::from_core(to)]),
            lines: app.line_count(),
            chars: app.char_count(),
            line_ending: encoding.line_ending.as_str(),
            bom: encoding.bom,
            can_undo: app.can_undo(),
            can_redo: app.can_redo(),
            dirty: app.is_dirty(),
            scroll: {
                let scroll = app.scroll();
                [scroll.line + 1, scroll.row + 1]
            },
            search: app.search(),
        }
    }
}

/// One row as a shell would draw it.
#[derive(Debug, Serialize)]
pub struct Row {
    pub line: u64,
    /// Whether it is the first row of its line — wrapped rows after it carry no line number.
    pub first: bool,
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Report {
    State(State),
    /// `view`: rows, numbered from 1.
    Rows {
        path: String,
        rows: Vec<Row>,
        caret: Place,
        lines: u64,
    },
    /// `copy`, `cut`, `export`: text for stdout, printed verbatim in text form.
    Text {
        text: String,
    },
    /// `commands`.
    Commands {
        commands: Vec<CommandRow>,
    },
}

#[derive(Debug, Serialize)]
pub struct CommandRow {
    pub id: &'static str,
    pub label: &'static str,
    pub summary: &'static str,
}

impl Report {
    pub fn print(&self, format: Format) {
        if format == Format::Json {
            println!(
                "{}",
                serde_json::to_string(self).expect("a report serialises")
            );
            return;
        }
        match self {
            Report::State(state) => {
                print!(
                    "{}: {} lines, caret {}",
                    state.path, state.lines, state.caret
                );
                if let Some([from, to]) = state.selection {
                    print!(", selection {from}–{to}");
                }
                if !state.changed {
                    print!(" (unchanged)");
                }
                println!();
            }
            Report::Rows { rows, .. } => {
                let width = rows
                    .iter()
                    .map(|r| r.line)
                    .max()
                    .unwrap_or(1)
                    .to_string()
                    .len();
                for row in rows {
                    if row.first {
                        println!("{:>width$}  {}", row.line, row.text);
                    } else {
                        println!("{:>width$}  {}", "", row.text);
                    }
                }
            }
            Report::Text { text } => print!("{text}"),
            Report::Commands { commands } => {
                for c in commands {
                    println!("{:<20} {:<15} {}", c.id, c.label, c.summary);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_one_based_on_the_outside() {
        let place = Place::parse("3:7").unwrap();
        assert_eq!(place.to_core(), Position::new(2, 6));
        assert_eq!(Place::from_core(Position::new(2, 6)), place);
        assert_eq!(Place::parse("4").unwrap().to_core(), Position::new(3, 0));
        assert!(Place::parse("0:1").is_err());
        assert!(Place::parse("x").is_err());
    }
}
