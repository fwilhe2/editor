// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Keys to actions — with no terminal in sight, so it is tested like any other function.
//!
//! The terminal's conventions are nano's and every TUI editor's since: Ctrl+letter commands,
//! listed on a help line, with the arrows doing what arrows do. Ctrl+P opens a palette over
//! [`Command::ALL`] for anything a key does not reach — though every command has a key here,
//! and the test `every_command_has_a_key` keeps it that way.

use quill_core::{Command, Delete, Motion};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What a key asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Command(Command),
    /// A motion, and whether Shift extends the selection.
    Move(Motion, bool),
    Delete(Delete),
    Type(char),
    /// Escape: drop the selection, or cancel what is pending.
    Escape,
    Palette,
}

/// The command keys, as data — so the help line and the palette print them from the same table
/// the key handler reads, and cannot disagree with it.
pub const BINDINGS: [(char, KeyModifiers, Command); 15] = [
    ('n', KeyModifiers::CONTROL, Command::New),
    ('o', KeyModifiers::CONTROL, Command::Open),
    ('s', KeyModifiers::CONTROL, Command::Save),
    // Ctrl+Shift+S does not reach a terminal application: most terminals send Ctrl+S for it.
    // Alt+S does, everywhere.
    ('s', KeyModifiers::ALT, Command::SaveAs),
    ('q', KeyModifiers::CONTROL, Command::Close),
    ('z', KeyModifiers::CONTROL, Command::Undo),
    ('y', KeyModifiers::CONTROL, Command::Redo),
    ('x', KeyModifiers::CONTROL, Command::Cut),
    ('c', KeyModifiers::CONTROL, Command::Copy),
    ('v', KeyModifiers::CONTROL, Command::Paste),
    ('a', KeyModifiers::CONTROL, Command::SelectAll),
    ('f', KeyModifiers::CONTROL, Command::Find),
    ('g', KeyModifiers::CONTROL, Command::FindNext),
    ('g', KeyModifiers::ALT, Command::FindPrevious),
    ('?', KeyModifiers::ALT, Command::About),
];

/// How a binding reads on the help line and in the palette: `^S`, `M-S`.
pub fn describe(command: Command) -> Option<String> {
    BINDINGS
        .iter()
        .find(|(_, _, c)| *c == command)
        .map(|(key, mods, _)| {
            let prefix = if mods.contains(KeyModifiers::CONTROL) {
                "^"
            } else {
                "M-"
            };
            format!("{prefix}{}", key.to_ascii_uppercase())
        })
}

pub fn action_for(key: KeyEvent) -> Option<Action> {
    let mods = key.modifiers;
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let alt = mods.contains(KeyModifiers::ALT);
    let shift = mods.contains(KeyModifiers::SHIFT);

    if let KeyCode::Char(c) = key.code
        && (ctrl || alt)
    {
        let c = c.to_ascii_lowercase();
        if ctrl && c == 'p' {
            return Some(Action::Palette);
        }
        let wanted = if ctrl {
            KeyModifiers::CONTROL
        } else {
            KeyModifiers::ALT
        };
        return BINDINGS
            .iter()
            .find(|(k, m, _)| *k == c && *m == wanted)
            .map(|(_, _, command)| Action::Command(*command));
    }

    let word = ctrl || alt;
    Some(match key.code {
        KeyCode::Left if word => Action::Move(Motion::WordLeft, shift),
        KeyCode::Right if word => Action::Move(Motion::WordRight, shift),
        KeyCode::Home if ctrl => Action::Move(Motion::DocumentStart, shift),
        KeyCode::End if ctrl => Action::Move(Motion::DocumentEnd, shift),
        KeyCode::Left => Action::Move(Motion::Left, shift),
        KeyCode::Right => Action::Move(Motion::Right, shift),
        KeyCode::Up => Action::Move(Motion::Up, shift),
        KeyCode::Down => Action::Move(Motion::Down, shift),
        KeyCode::Home => Action::Move(Motion::RowStart, shift),
        KeyCode::End => Action::Move(Motion::RowEnd, shift),
        KeyCode::PageUp => Action::Move(Motion::PageUp, shift),
        KeyCode::PageDown => Action::Move(Motion::PageDown, shift),
        KeyCode::Backspace if word => Action::Delete(Delete::WordBackward),
        KeyCode::Delete if word => Action::Delete(Delete::WordForward),
        KeyCode::Backspace => Action::Delete(Delete::Backward),
        KeyCode::Delete => Action::Delete(Delete::Forward),
        KeyCode::Enter => Action::Type('\n'),
        KeyCode::Tab => Action::Type('\t'),
        KeyCode::Esc => Action::Escape,
        KeyCode::F(3) if shift => Action::Command(Command::FindPrevious),
        KeyCode::F(3) => Action::Command(Command::FindNext),
        KeyCode::F(1) => Action::Command(Command::About),
        KeyCode::Char(c) => Action::Type(c),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> Option<Action> {
        action_for(KeyEvent::new(code, mods))
    }

    #[test]
    fn every_command_has_a_key() {
        for command in Command::ALL {
            let (c, mods, _) = BINDINGS
                .iter()
                .find(|(_, _, bound)| *bound == command)
                .unwrap_or_else(|| panic!("{} has no key in the terminal", command.id()));
            assert_eq!(
                key(KeyCode::Char(*c), *mods),
                Some(Action::Command(command)),
                "{}'s binding is shadowed",
                command.id()
            );
        }
    }

    #[test]
    fn no_two_commands_share_a_key() {
        for (i, (c, m, _)) in BINDINGS.iter().enumerate() {
            assert!(
                !BINDINGS[i + 1..]
                    .iter()
                    .any(|(c2, m2, _)| c2 == c && m2 == m)
            );
            assert!(
                !(*c == 'p' && *m == KeyModifiers::CONTROL),
                "^P is the palette"
            );
        }
    }

    #[test]
    fn every_motion_has_a_key() {
        let keys = [
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
        ];
        let mut reached = Vec::new();
        for code in keys {
            for mods in [KeyModifiers::NONE, KeyModifiers::CONTROL] {
                if let Some(Action::Move(motion, _)) = key(code, mods) {
                    reached.push(motion);
                }
            }
        }
        for motion in Motion::ALL {
            assert!(reached.contains(&motion), "{} has no key", motion.name());
        }
    }

    #[test]
    fn shift_extends_and_ctrl_letters_are_never_text() {
        assert_eq!(
            key(KeyCode::Right, KeyModifiers::SHIFT),
            Some(Action::Move(Motion::Right, true))
        );
        assert_eq!(key(KeyCode::Char('w'), KeyModifiers::CONTROL), None);
        assert_eq!(
            key(KeyCode::Char('W'), KeyModifiers::SHIFT),
            Some(Action::Type('W'))
        );
    }
}
