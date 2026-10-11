// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The menu bar as data — Windows' growable surface, and the only place a verb is listed.
//!
//! A Windows text editor of this size has a menu bar (Notepad still does), drawn by the system so
//! it follows the theme and gains Alt-key navigation for free. Each item names a [`Command`], its
//! label with an `&` mnemonic, and its accelerator; `win.rs` builds the `HMENU` from this table
//! and `keymap.rs` answers keys from it, so a label and the key it advertises cannot drift. The
//! tests check every [`Command`] is here — a verb in the core that this window forgot fails the
//! build on any host.
//!
//! The accelerators are Windows' own: Ctrl+Y redoes and F3 finds the next match, as in Notepad.

use quill_core::Command;

/// A key that triggers an item: a virtual-key code and its modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accel {
    pub vk: u16,
    pub ctrl: bool,
    pub shift: bool,
}

const fn ctrl(vk: u8) -> Option<Accel> {
    Some(Accel {
        vk: vk as u16,
        ctrl: true,
        shift: false,
    })
}

const fn ctrl_shift(vk: u8) -> Option<Accel> {
    Some(Accel {
        vk: vk as u16,
        ctrl: true,
        shift: true,
    })
}

/// F3, and Shift+F3.
const VK_F3: u16 = 0x72;

#[derive(Clone, Copy, Debug)]
pub enum Entry {
    Item {
        command: Command,
        /// With its `&` mnemonic; the accelerator text is added from `key` (or `shown`).
        label: &'static str,
        key: Option<Accel>,
        /// A key the system handles that the label should still mention (Alt+F4).
        shown: Option<&'static str>,
    },
    Separator,
}

const fn item(command: Command, label: &'static str, key: Option<Accel>) -> Entry {
    Entry::Item {
        command,
        label,
        key,
        shown: None,
    }
}

pub const MENUS: [(&str, &[Entry]); 3] = [
    (
        "&File",
        &[
            item(Command::New, "&New", ctrl(b'N')),
            item(Command::Open, "&Open...", ctrl(b'O')),
            item(Command::Save, "&Save", ctrl(b'S')),
            item(Command::SaveAs, "Save &As...", ctrl_shift(b'S')),
            Entry::Separator,
            // Exit rather than Close: one document per window, and closing it ends the program.
            // Alt+F4 is the system's, through DefWindowProc, and must stay so.
            Entry::Item {
                command: Command::Close,
                label: "E&xit",
                key: None,
                shown: Some("Alt+F4"),
            },
        ],
    ),
    (
        "&Edit",
        &[
            item(Command::Undo, "&Undo", ctrl(b'Z')),
            item(Command::Redo, "&Redo", ctrl(b'Y')),
            Entry::Separator,
            item(Command::Cut, "Cu&t", ctrl(b'X')),
            item(Command::Copy, "&Copy", ctrl(b'C')),
            item(Command::Paste, "&Paste", ctrl(b'V')),
            Entry::Separator,
            item(Command::Find, "&Find...", ctrl(b'F')),
            item(
                Command::FindNext,
                "Find &Next",
                Some(Accel {
                    vk: VK_F3,
                    ctrl: false,
                    shift: false,
                }),
            ),
            item(
                Command::FindPrevious,
                "Find Pre&vious",
                Some(Accel {
                    vk: VK_F3,
                    ctrl: false,
                    shift: true,
                }),
            ),
            Entry::Separator,
            item(Command::SelectAll, "Select &All", ctrl(b'A')),
        ],
    ),
    ("&Help", &[item(Command::About, "&About Quill", None)]),
];

/// The `WM_COMMAND` identifier of a command: its index in [`Command::ALL`], past the range the
/// system uses.
pub fn id(command: Command) -> u16 {
    100 + Command::ALL.iter().position(|&c| c == command).unwrap() as u16
}

pub fn from_id(id: u16) -> Option<Command> {
    Command::ALL.get(id.checked_sub(100)? as usize).copied()
}

/// How Windows writes an accelerator after a menu label's tab.
pub fn describe(key: Accel) -> String {
    let name = match key.vk {
        VK_F3 => "F3".to_string(),
        vk => char::from(vk as u8).to_string(),
    };
    match (key.ctrl, key.shift) {
        (true, true) => format!("Ctrl+Shift+{name}"),
        (true, false) => format!("Ctrl+{name}"),
        (false, true) => format!("Shift+{name}"),
        (false, false) => name,
    }
}

/// An item's full text: label, a tab, the accelerator.
pub fn text(entry: &Entry) -> Option<String> {
    let Entry::Item {
        label, key, shown, ..
    } = entry
    else {
        return None;
    };
    Some(match (key, shown) {
        (Some(key), _) => format!("{label}\t{}", describe(*key)),
        (None, Some(shown)) => format!("{label}\t{shown}"),
        (None, None) => label.to_string(),
    })
}

/// The command a key triggers, if any.
pub fn command_for(vk: u16, ctrl: bool, shift: bool) -> Option<Command> {
    MENUS
        .iter()
        .flat_map(|(_, entries)| entries.iter())
        .find_map(|entry| match entry {
            Entry::Item {
                command,
                key: Some(key),
                ..
            } if key.vk == vk && key.ctrl == ctrl && key.shift == shift => Some(*command),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> impl Iterator<Item = (Command, Option<Accel>)> {
        MENUS
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .filter_map(|entry| match entry {
                Entry::Item { command, key, .. } => Some((*command, *key)),
                Entry::Separator => None,
            })
    }

    #[test]
    fn every_command_is_in_the_menu_bar_exactly_once() {
        for command in Command::ALL {
            let count = items().filter(|(c, _)| *c == command).count();
            assert_eq!(count, 1, "{} is in the menus {count} times", command.id());
        }
    }

    #[test]
    fn every_accelerator_reaches_its_item_and_none_is_shared() {
        let keys: Vec<_> = items().filter_map(|(c, k)| Some((c, k?))).collect();
        for (command, key) in &keys {
            assert_eq!(command_for(key.vk, key.ctrl, key.shift), Some(*command));
            assert_eq!(keys.iter().filter(|(_, k)| k == key).count(), 1);
        }
    }

    #[test]
    fn ids_round_trip() {
        for command in Command::ALL {
            assert_eq!(from_id(id(command)), Some(command));
        }
        assert_eq!(from_id(99), None);
    }

    #[test]
    fn labels_carry_their_accelerators() {
        let save_as = MENUS[0].1[3];
        assert_eq!(text(&save_as).unwrap(), "Save &As...\tCtrl+Shift+S");
        let exit = MENUS[0].1[5];
        assert_eq!(text(&exit).unwrap(), "E&xit\tAlt+F4");
    }

    #[test]
    fn every_mnemonic_is_unique_within_its_menu() {
        for (_, entries) in MENUS {
            let mut seen = Vec::new();
            for entry in entries {
                if let Entry::Item { label, .. } = entry {
                    let mnemonic = label.split('&').nth(1).and_then(|s| s.chars().next());
                    let mnemonic = mnemonic.unwrap().to_ascii_lowercase();
                    assert!(!seen.contains(&mnemonic), "&{mnemonic} twice in one menu");
                    seen.push(mnemonic);
                }
            }
        }
    }
}
