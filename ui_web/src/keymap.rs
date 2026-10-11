// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! `KeyboardEvent` values to actions — DOM-free, so it is tested on the host like any function.
//!
//! The shortcuts are the web's, not a desktop's. `primary` is Ctrl on Windows and Linux and ⌘ on
//! macOS, resolved at the DOM edge, so ⌘S saves on a Mac and Ctrl+S everywhere else. Keys the
//! browser keeps for itself — Ctrl+N opens a window, Ctrl+W closes the tab, Ctrl+T opens one —
//! are not taken: New, Close and About live in the toolbar and the Ctrl+K palette instead
//! (`doc/shells/web.md`). Cut, Copy and Paste are the browser's own `cut`/`copy`/`paste` events
//! on the focused input, so they need no binding here either; the test below holds every
//! command to being reachable *somehow*.
//!
//! Typed text is not here at all: it arrives as `input` events on a hidden `<textarea>`, which
//! is what makes an input method, a dead key and a phone's keyboard work.

use quill_core::{Command, Delete, Motion};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Command(Command),
    Move(Motion, bool),
    Delete(Delete),
    Type(char),
    Escape,
    Palette,
}

/// A key press, reduced to what the mapping depends on.
#[derive(Clone, Debug)]
pub struct Chord<'a> {
    /// `KeyboardEvent.key`.
    pub key: &'a str,
    /// Ctrl, or ⌘ on a Mac.
    pub primary: bool,
    pub shift: bool,
    pub alt: bool,
}

/// The commands with a primary-key shortcut, as data: what the palette prints beside each row.
pub const SHORTCUTS: [(Command, &str); 9] = [
    (Command::Open, "o"),
    (Command::Save, "s"),
    (Command::SaveAs, "S"),
    (Command::Undo, "z"),
    (Command::Redo, "Z"),
    (Command::SelectAll, "a"),
    (Command::Find, "f"),
    (Command::FindNext, "g"),
    (Command::FindPrevious, "G"),
];

pub fn action_for(chord: &Chord<'_>) -> Option<Action> {
    if chord.primary {
        let key = if chord.shift {
            chord.key.to_ascii_uppercase()
        } else {
            chord.key.to_ascii_lowercase()
        };
        if key == "k" {
            return Some(Action::Palette);
        }
        if key == "y" {
            return Some(Action::Command(Command::Redo));
        }
        if let Some((command, _)) = SHORTCUTS.iter().find(|(_, k)| *k == key) {
            return Some(Action::Command(*command));
        }
        return match chord.key {
            "ArrowLeft" => Some(Action::Move(Motion::WordLeft, chord.shift)),
            "ArrowRight" => Some(Action::Move(Motion::WordRight, chord.shift)),
            "Home" | "ArrowUp" => Some(Action::Move(Motion::DocumentStart, chord.shift)),
            "End" | "ArrowDown" => Some(Action::Move(Motion::DocumentEnd, chord.shift)),
            "Backspace" => Some(Action::Delete(Delete::WordBackward)),
            "Delete" => Some(Action::Delete(Delete::WordForward)),
            // Everything else with Ctrl/⌘ belongs to the browser: Ctrl+T, Ctrl+L, ⌘Q.
            _ => None,
        };
    }
    let shift = chord.shift;
    Some(match chord.key {
        // Alt+arrows is the Mac's word motion, and Linux and Windows browsers leave it alone.
        "ArrowLeft" if chord.alt => Action::Move(Motion::WordLeft, shift),
        "ArrowRight" if chord.alt => Action::Move(Motion::WordRight, shift),
        "Backspace" if chord.alt => Action::Delete(Delete::WordBackward),
        "ArrowLeft" => Action::Move(Motion::Left, shift),
        "ArrowRight" => Action::Move(Motion::Right, shift),
        "ArrowUp" => Action::Move(Motion::Up, shift),
        "ArrowDown" => Action::Move(Motion::Down, shift),
        "Home" => Action::Move(Motion::RowStart, shift),
        "End" => Action::Move(Motion::RowEnd, shift),
        "PageUp" => Action::Move(Motion::PageUp, shift),
        "PageDown" => Action::Move(Motion::PageDown, shift),
        "Backspace" => Action::Delete(Delete::Backward),
        "Delete" => Action::Delete(Delete::Forward),
        "Enter" => Action::Type('\n'),
        "Tab" => Action::Type('\t'),
        "Escape" => Action::Escape,
        "F3" if shift => Action::Command(Command::FindPrevious),
        "F3" => Action::Command(Command::FindNext),
        _ => return None,
    })
}

/// How the palette shows a command's shortcut: `Ctrl+S` (or `⌘S` on a Mac).
pub fn describe(command: Command, mac: bool) -> Option<String> {
    let (_, key) = SHORTCUTS.iter().find(|(c, _)| *c == command)?;
    let shift = key.chars().all(|c| c.is_ascii_uppercase());
    let key = key.to_ascii_uppercase();
    Some(match (mac, shift) {
        (true, true) => format!("⇧⌘{key}"),
        (true, false) => format!("⌘{key}"),
        (false, true) => format!("Ctrl+Shift+{key}"),
        (false, false) => format!("Ctrl+{key}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Commands reached through the browser's own `cut`/`copy`/`paste` events rather than a key
    /// of ours.
    const NATIVE: [Command; 3] = [Command::Cut, Command::Copy, Command::Paste];

    /// Commands reached only from the toolbar and the palette: their natural keys (Ctrl+N,
    /// Ctrl+W) are the browser's.
    const POINTER_ONLY: [Command; 3] = [Command::New, Command::Close, Command::About];

    fn chord(key: &str, primary: bool, shift: bool) -> Option<Action> {
        action_for(&Chord {
            key,
            primary,
            shift,
            alt: false,
        })
    }

    #[test]
    fn every_command_is_reachable() {
        for command in Command::ALL {
            let keyed = SHORTCUTS.iter().any(|(c, _)| *c == command);
            let native = NATIVE.contains(&command);
            let pointer = POINTER_ONLY.contains(&command);
            assert_eq!(
                [keyed, native, pointer].iter().filter(|&&b| b).count(),
                1,
                "{} must be reached exactly one way",
                command.id()
            );
        }
    }

    #[test]
    fn shortcuts_reach_their_commands() {
        for (command, key) in SHORTCUTS {
            let shift = key.chars().all(|c| c.is_ascii_uppercase());
            assert_eq!(
                chord(key, true, shift),
                Some(Action::Command(command)),
                "{key}"
            );
        }
        assert_eq!(chord("k", true, false), Some(Action::Palette));
    }

    #[test]
    fn every_motion_has_a_key() {
        let keys = [
            "ArrowLeft",
            "ArrowRight",
            "ArrowUp",
            "ArrowDown",
            "Home",
            "End",
            "PageUp",
            "PageDown",
        ];
        let mut reached = Vec::new();
        for key in keys {
            for primary in [false, true] {
                if let Some(Action::Move(motion, _)) = chord(key, primary, false) {
                    reached.push(motion);
                }
            }
        }
        for motion in Motion::ALL {
            assert!(reached.contains(&motion), "{} has no key", motion.name());
        }
    }

    #[test]
    fn the_browsers_own_keys_are_left_alone() {
        assert_eq!(chord("t", true, false), None);
        assert_eq!(chord("w", true, false), None);
        assert_eq!(chord("n", true, false), None);
        assert_eq!(
            chord("a", false, false),
            None,
            "text arrives as input, not as a key"
        );
    }

    #[test]
    fn shortcuts_are_described_per_platform() {
        assert_eq!(describe(Command::Redo, true).unwrap(), "⇧⌘Z");
        assert_eq!(describe(Command::Save, false).unwrap(), "Ctrl+S");
        assert_eq!(describe(Command::New, false), None);
    }
}
