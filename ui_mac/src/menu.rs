// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The menu bar as data, in the Mac's own shape.
//!
//! The Mac convention is not GNOME's or Windows': an application menu named for the app (About,
//! Hide, Quit), then File, Edit, Window — and the Edit menu's items send the **standard
//! selectors** (`undo:`, `cut:`, `selectAll:`) down the responder chain rather than calling the
//! document directly, which is why ⌘C works in an alert's text field as well as in the page. The
//! editor's own verbs that have no standard selector travel as `quillCommand:` with the command
//! in the item's tag.
//!
//! `app.rs` builds the `NSMenu`s from [`MENUS`]; the tests hold every [`Command`] to appearing
//! exactly once, and the key equivalents to Apple's.

use quill_core::Command;

/// The action every [`Command`] without a standard selector sends; its tag says which.
pub const COMMAND_SELECTOR: &str = "quillCommand:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// The key equivalent: a lower-case letter, or upper-case for one that needs Shift.
    pub key: &'static str,
    pub option: bool,
}

const fn cmd(key: &'static str) -> Option<Key> {
    Some(Key { key, option: false })
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    /// One of the editor's verbs.
    Command(Command),
    /// A selector AppKit itself answers (`hide:`, `terminate:`, `performMiniaturize:`).
    Standard(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub enum Item {
    Entry {
        title: &'static str,
        key: Option<Key>,
        action: Action,
    },
    Separator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Application,
    Window,
    Plain,
}

pub struct Menu {
    pub title: &'static str,
    pub role: Role,
    pub items: &'static [Item],
}

const fn verb(title: &'static str, key: Option<Key>, command: Command) -> Item {
    Item::Entry {
        title,
        key,
        action: Action::Command(command),
    }
}

const fn standard(title: &'static str, key: Option<Key>, selector: &'static str) -> Item {
    Item::Entry {
        title,
        key,
        action: Action::Standard(selector),
    }
}

pub static MENUS: [Menu; 4] = [
    Menu {
        // AppKit replaces the first menu's title with the application's name.
        title: "Quill",
        role: Role::Application,
        items: &[
            verb("About Quill", None, Command::About),
            Item::Separator,
            standard("Hide Quill", cmd("h"), "hide:"),
            Item::Entry {
                title: "Hide Others",
                key: Some(Key {
                    key: "h",
                    option: true,
                }),
                action: Action::Standard("hideOtherApplications:"),
            },
            standard("Show All", None, "unhideAllApplications:"),
            Item::Separator,
            standard("Quit Quill", cmd("q"), "terminate:"),
        ],
    },
    Menu {
        title: "File",
        role: Role::Plain,
        items: &[
            verb("New", cmd("n"), Command::New),
            verb("Open…", cmd("o"), Command::Open),
            Item::Separator,
            verb("Close", cmd("w"), Command::Close),
            verb("Save", cmd("s"), Command::Save),
            verb("Save As…", cmd("S"), Command::SaveAs),
        ],
    },
    Menu {
        title: "Edit",
        role: Role::Plain,
        items: &[
            verb("Undo", cmd("z"), Command::Undo),
            verb("Redo", cmd("Z"), Command::Redo),
            Item::Separator,
            verb("Cut", cmd("x"), Command::Cut),
            verb("Copy", cmd("c"), Command::Copy),
            verb("Paste", cmd("v"), Command::Paste),
            verb("Select All", cmd("a"), Command::SelectAll),
            Item::Separator,
            verb("Find…", cmd("f"), Command::Find),
            verb("Find Next", cmd("g"), Command::FindNext),
            verb("Find Previous", cmd("G"), Command::FindPrevious),
        ],
    },
    Menu {
        title: "Window",
        role: Role::Window,
        items: &[
            standard("Minimize", cmd("m"), "performMiniaturize:"),
            standard("Zoom", None, "performZoom:"),
        ],
    },
];

/// The selector a command's item sends: the standard one where AppKit has one, so text fields
/// in panels answer it too, and [`COMMAND_SELECTOR`] otherwise.
pub fn selector(command: Command) -> &'static str {
    match command {
        Command::Undo => "undo:",
        Command::Redo => "redo:",
        Command::Cut => "cut:",
        Command::Copy => "copy:",
        Command::Paste => "paste:",
        Command::SelectAll => "selectAll:",
        Command::Close => "performClose:",
        _ => COMMAND_SELECTOR,
    }
}

/// A command's tag on its menu item: its place in [`Command::ALL`], plus one so that no command
/// is the default tag, zero.
pub fn tag(command: Command) -> isize {
    1 + Command::ALL.iter().position(|&c| c == command).unwrap() as isize
}

pub fn from_tag(tag: isize) -> Option<Command> {
    Command::ALL.get(usize::try_from(tag - 1).ok()?).copied()
}

/// Which command a standard selector stands for, when the view receives it.
pub fn from_selector(name: &str) -> Option<Command> {
    Command::ALL
        .into_iter()
        .find(|&c| selector(c) == name && name != COMMAND_SELECTOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> impl Iterator<Item = (&'static str, Option<Key>, Action)> {
        MENUS
            .iter()
            .flat_map(|m| m.items.iter())
            .filter_map(|item| match item {
                Item::Entry { title, key, action } => Some((*title, *key, *action)),
                Item::Separator => None,
            })
    }

    #[test]
    fn every_command_is_in_the_menu_bar_exactly_once() {
        for command in Command::ALL {
            let count = entries()
                .filter(|(_, _, action)| matches!(action, Action::Command(c) if *c == command))
                .count();
            assert_eq!(count, 1, "{} is in the menus {count} times", command.id());
        }
    }

    #[test]
    fn apples_key_equivalents() {
        let key = |command| {
            entries()
                .find(|(_, _, a)| matches!(a, Action::Command(c) if *c == command))
                .and_then(|(_, key, _)| key)
                .map(|k| k.key)
        };
        assert_eq!(key(Command::Redo), Some("Z"), "⇧⌘Z, not ⌘Y");
        assert_eq!(key(Command::Close), Some("w"));
        assert_eq!(key(Command::FindNext), Some("g"));
    }

    #[test]
    fn no_key_equivalent_is_used_twice() {
        let keys: Vec<Key> = entries().filter_map(|(_, key, _)| key).collect();
        for (i, key) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(key), "{key:?} twice");
        }
    }

    #[test]
    fn tags_and_selectors_round_trip() {
        for command in Command::ALL {
            assert_eq!(from_tag(tag(command)), Some(command));
            if selector(command) != COMMAND_SELECTOR {
                assert_eq!(from_selector(selector(command)), Some(command));
            }
        }
        assert_eq!(from_tag(0), None);
    }

    #[test]
    fn ellipses_mark_the_items_that_ask() {
        for (title, _, action) in entries() {
            if let Action::Command(command) = action {
                assert_eq!(title.ends_with('…'), command.asks(), "{title}");
            }
        }
    }
}
