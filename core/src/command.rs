// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Every verb a person can ask for by name, as data.
//!
//! A menu item, a toolbar button, a command-palette row and a keyboard shortcut are four ways
//! to reach one verb, and they drift the moment each shell keeps its own list. So the list is
//! here, once: every shell builds its menus (or palette) from [`Command::ALL`] and maps each
//! command onto its own handler, and **each shell has a test that fails when a command is
//! missing from its menus** — which is how a new verb cannot be added to one window and
//! forgotten in the other five.
//!
//! What is *not* here is the shortcut: Redo is Ctrl+Y on Windows, Ctrl+Shift+Z on GNOME and
//! ⇧⌘Z on macOS. A key is a platform convention, so each shell assigns its own; a verb is the
//! program's, so it is defined once.
//!
//! Caret motions are not commands — they are [`crate::Motion`]s, reached from keys rather than
//! menus, and every shell's keymap test covers them instead.

/// A verb from a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Close,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Find,
    FindNext,
    FindPrevious,
    About,
}

/// Which menu a command belongs in. A shell with no menu bar (the browser, the terminal) still
/// groups by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Menu {
    File,
    Edit,
    Help,
}

impl Command {
    /// Every command, in menu order.
    pub const ALL: [Command; 15] = [
        Command::New,
        Command::Open,
        Command::Save,
        Command::SaveAs,
        Command::Close,
        Command::Undo,
        Command::Redo,
        Command::Cut,
        Command::Copy,
        Command::Paste,
        Command::SelectAll,
        Command::Find,
        Command::FindNext,
        Command::FindPrevious,
        Command::About,
    ];

    /// A stable identifier: what a palette searches, what a web page's buttons carry, what the
    /// CLI's `quill commands` prints.
    pub fn id(self) -> &'static str {
        match self {
            Command::New => "file.new",
            Command::Open => "file.open",
            Command::Save => "file.save",
            Command::SaveAs => "file.save-as",
            Command::Close => "file.close",
            Command::Undo => "edit.undo",
            Command::Redo => "edit.redo",
            Command::Cut => "edit.cut",
            Command::Copy => "edit.copy",
            Command::Paste => "edit.paste",
            Command::SelectAll => "edit.select-all",
            Command::Find => "edit.find",
            Command::FindNext => "edit.find-next",
            Command::FindPrevious => "edit.find-previous",
            Command::About => "help.about",
        }
    }

    /// The menu item's label, without a mnemonic or an ellipsis — a shell adds those by its
    /// own platform's rules ([`Command::asks`] says whether an ellipsis belongs).
    pub fn label(self) -> &'static str {
        match self {
            Command::New => "New",
            Command::Open => "Open",
            Command::Save => "Save",
            Command::SaveAs => "Save As",
            Command::Close => "Close",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::Cut => "Cut",
            Command::Copy => "Copy",
            Command::Paste => "Paste",
            Command::SelectAll => "Select All",
            Command::Find => "Find",
            Command::FindNext => "Find Next",
            Command::FindPrevious => "Find Previous",
            Command::About => "About",
        }
    }

    /// One sentence about what it does, for a palette row or a tooltip.
    pub fn summary(self) -> &'static str {
        match self {
            Command::New => "Start an empty document",
            Command::Open => "Open a text file",
            Command::Save => "Save the document where it came from",
            Command::SaveAs => "Save the document under a new name",
            Command::Close => "Close the document, asking first if it has unsaved changes",
            Command::Undo => "Undo the last change",
            Command::Redo => "Redo the change just undone",
            Command::Cut => "Move the selection to the clipboard",
            Command::Copy => "Copy the selection to the clipboard",
            Command::Paste => "Insert the clipboard's text",
            Command::SelectAll => "Select the whole document",
            Command::Find => "Search the document for some text",
            Command::FindNext => "Select the next match",
            Command::FindPrevious => "Select the previous match",
            Command::About => "Show the version and licence",
        }
    }

    pub fn menu(self) -> Menu {
        match self {
            Command::New | Command::Open | Command::Save | Command::SaveAs | Command::Close => {
                Menu::File
            }
            Command::About => Menu::Help,
            _ => Menu::Edit,
        }
    }

    /// Whether choosing it asks for more before acting — the menu label then ends in an
    /// ellipsis, on every platform that has the convention.
    pub fn asks(self) -> bool {
        matches!(self, Command::Open | Command::SaveAs | Command::Find)
    }

    pub fn from_id(id: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// How well `query` matches `text` for a command palette: `None` for no match, otherwise higher
/// is better. Every character of the query must appear in order; consecutive runs and a match
/// at a word's start score more.
///
/// In the core because two shells with a palette must rank a query the same way.
pub fn score(query: &str, text: &str) -> Option<u32> {
    let query: Vec<char> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0;
    let mut q = 0;
    let mut previous: Option<usize> = None;
    for (i, &c) in text.iter().enumerate() {
        if q < query.len() && c == query[q] {
            score += 1;
            if previous.is_some_and(|p| p + 1 == i) {
                score += 3;
            }
            if i == 0 || !text[i - 1].is_alphanumeric() {
                score += 5;
            }
            previous = Some(i);
            q += 1;
        }
    }
    (q == query.len()).then_some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_round_trip() {
        for (i, command) in Command::ALL.iter().enumerate() {
            assert_eq!(Command::from_id(command.id()), Some(*command));
            for other in &Command::ALL[i + 1..] {
                assert_ne!(command.id(), other.id());
            }
        }
    }

    #[test]
    fn a_palette_prefers_word_starts() {
        assert!(score("sa", "Save As") > score("sa", "Paste"));
        assert_eq!(score("xyz", "Save"), None);
        assert_eq!(score("", "anything"), Some(0));
    }
}
