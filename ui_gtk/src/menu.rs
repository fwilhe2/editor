// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The window's menus and accelerators, as data.
//!
//! GNOME's convention is a header bar with a *primary menu* behind ☰ rather than a menu bar,
//! with the actions also on accelerators. Both are built from the tables here, and the tests
//! below check every [`Command`] is in the menu and has a GAction name — so a verb added to the
//! core cannot be forgotten in this window. The accelerators are GNOME's own (Ctrl+Shift+Z for
//! Redo, not the Ctrl+Y Windows uses): a key is a platform convention, a verb is not.

use quill_core::Command;

/// The primary menu's sections, top to bottom.
pub const SECTIONS: [&[Command]; 5] = [
    &[Command::New, Command::Open],
    &[Command::Save, Command::SaveAs],
    &[
        Command::Undo,
        Command::Redo,
        Command::Cut,
        Command::Copy,
        Command::Paste,
        Command::SelectAll,
    ],
    &[Command::Find, Command::FindNext, Command::FindPrevious],
    &[Command::About, Command::Close],
];

/// The GAction a command is, on the window: `win.<this>`. A command's ID with its dot made a
/// dash, since the part of a detailed action name before the first dot is the action group.
pub fn action(command: Command) -> String {
    command.id().replace('.', "-")
}

/// GNOME's accelerators for a command (GTK's accelerator syntax).
pub fn accels(command: Command) -> &'static [&'static str] {
    match command {
        Command::New => &["<Control>n"],
        Command::Open => &["<Control>o"],
        Command::Save => &["<Control>s"],
        Command::SaveAs => &["<Control><Shift>s"],
        Command::Close => &["<Control>w"],
        Command::Undo => &["<Control>z"],
        Command::Redo => &["<Control><Shift>z"],
        Command::Cut => &["<Control>x"],
        Command::Copy => &["<Control>c"],
        Command::Paste => &["<Control>v"],
        Command::SelectAll => &["<Control>a"],
        Command::Find => &["<Control>f"],
        Command::FindNext => &["<Control>g"],
        Command::FindPrevious => &["<Control><Shift>g"],
        Command::About => &[],
    }
}

/// The label GNOME shows: the command's own, with an ellipsis when it asks for more.
pub fn label(command: Command) -> String {
    if command.asks() {
        format!("{}…", command.label())
    } else {
        command.label().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_is_in_the_menu_exactly_once() {
        for command in Command::ALL {
            let count = SECTIONS
                .iter()
                .flat_map(|s| s.iter())
                .filter(|&&c| c == command)
                .count();
            assert_eq!(
                count,
                1,
                "{} is in the primary menu {count} times",
                command.id()
            );
        }
    }

    #[test]
    fn action_names_are_valid_gaction_names() {
        for command in Command::ALL {
            let name = action(command);
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "{name}"
            );
        }
    }

    #[test]
    fn no_accelerator_is_used_twice() {
        let all: Vec<_> = Command::ALL.iter().flat_map(|&c| accels(c)).collect();
        for (i, accel) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(accel), "{accel} is bound twice");
        }
    }
}
