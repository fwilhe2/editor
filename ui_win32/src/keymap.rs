// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Keys and characters to actions, with no Windows types in sight — a virtual-key code is a
//! `u16` here, so all of it compiles and is tested on Linux.
//!
//! Windows splits a keystroke in two: `WM_KEYDOWN` carries the key (an arrow, F3, Ctrl+S) and
//! `WM_CHAR`, produced by `TranslateMessage`, carries the text it typed — in UTF-16 units, so a
//! character outside the Basic Multilingual Plane arrives as two messages. [`action_for_key`]
//! answers the first, [`Utf16`] and [`typed`] the second.

use quill_core::{Command, Delete, Motion};

use crate::menu;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Command(Command),
    Move(Motion, bool),
    Delete(Delete),
    Escape,
}

// The virtual-key codes used here, copied from `winuser.h`. A `cfg(windows)` test pins each one
// to the `windows` crate's own value, on the Windows runner.
pub const VK_BACK: u16 = 0x08;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_PRIOR: u16 = 0x21;
pub const VK_NEXT: u16 = 0x22;
pub const VK_END: u16 = 0x23;
pub const VK_HOME: u16 = 0x24;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;
pub const VK_DELETE: u16 = 0x2E;

pub fn action_for_key(vk: u16, mods: Modifiers) -> Option<Action> {
    if !mods.alt
        && let Some(command) = menu::command_for(vk, mods.ctrl, mods.shift)
    {
        return Some(Action::Command(command));
    }
    let (ctrl, shift) = (mods.ctrl, mods.shift);
    let motion = |m| Some(Action::Move(m, shift));
    match vk {
        VK_LEFT if ctrl => motion(Motion::WordLeft),
        VK_RIGHT if ctrl => motion(Motion::WordRight),
        VK_HOME if ctrl => motion(Motion::DocumentStart),
        VK_END if ctrl => motion(Motion::DocumentEnd),
        VK_LEFT => motion(Motion::Left),
        VK_RIGHT => motion(Motion::Right),
        VK_UP => motion(Motion::Up),
        VK_DOWN => motion(Motion::Down),
        VK_HOME => motion(Motion::RowStart),
        VK_END => motion(Motion::RowEnd),
        VK_PRIOR => motion(Motion::PageUp),
        VK_NEXT => motion(Motion::PageDown),
        VK_BACK if ctrl => Some(Action::Delete(Delete::WordBackward)),
        VK_DELETE if ctrl => Some(Action::Delete(Delete::WordForward)),
        // Backspace's character (0x08) also arrives as WM_CHAR, and `typed` drops it.
        VK_BACK => Some(Action::Delete(Delete::Backward)),
        VK_DELETE => Some(Action::Delete(Delete::Forward)),
        VK_ESCAPE => Some(Action::Escape),
        _ => None,
    }
}

/// Reassembles `WM_CHAR`'s UTF-16 units into characters: a high surrogate waits here for its low
/// half. Per window, not a static, since two windows may be typed into in turn.
#[derive(Default)]
pub struct Utf16 {
    high: Option<u16>,
}

impl Utf16 {
    pub fn push(&mut self, unit: u16) -> Option<char> {
        match unit {
            0xD800..=0xDBFF => {
                self.high = Some(unit);
                None
            }
            0xDC00..=0xDFFF => {
                let high = self.high.take()?;
                char::decode_utf16([high, unit]).next()?.ok()
            }
            _ => {
                self.high = None;
                char::from_u32(u32::from(unit))
            }
        }
    }
}

/// What a `WM_CHAR` character types: Enter's `\r` is a line break, Tab is a tab, and every other
/// control character — Backspace's 0x08, Ctrl+S's 0x13 — was already handled as a key.
pub fn typed(c: char) -> Option<char> {
    match c {
        '\r' => Some('\n'),
        '\t' => Some('\t'),
        c if c.is_control() => None,
        c => Some(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
    };

    #[test]
    fn every_motion_has_a_key() {
        let keys = [
            VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VK_HOME, VK_END, VK_PRIOR, VK_NEXT,
        ];
        let mut reached = Vec::new();
        for vk in keys {
            for ctrl in [false, true] {
                let mods = Modifiers { ctrl, ..NONE };
                if let Some(Action::Move(motion, _)) = action_for_key(vk, mods) {
                    reached.push(motion);
                }
            }
        }
        for motion in Motion::ALL {
            assert!(reached.contains(&motion), "{} has no key", motion.name());
        }
    }

    #[test]
    fn menu_accelerators_are_keys_too() {
        let ctrl = Modifiers { ctrl: true, ..NONE };
        assert_eq!(
            action_for_key(u16::from(b'Y'), ctrl),
            Some(Action::Command(Command::Redo))
        );
        assert_eq!(
            action_for_key(
                0x72,
                Modifiers {
                    shift: true,
                    ..NONE
                }
            ),
            Some(Action::Command(Command::FindPrevious))
        );
        assert_eq!(
            action_for_key(u16::from(b'Y'), NONE),
            None,
            "a plain Y is text"
        );
    }

    #[test]
    fn surrogate_pairs_become_one_character() {
        let mut input = Utf16::default();
        let units: Vec<u16> = "a😀".encode_utf16().collect();
        let typed: Vec<char> = units.iter().filter_map(|&u| input.push(u)).collect();
        assert_eq!(typed, ['a', '😀']);
        assert_eq!(
            Utf16::default().push(0xDC00),
            None,
            "a lone low half is dropped"
        );
    }

    #[test]
    fn control_characters_are_not_text() {
        assert_eq!(typed('\r'), Some('\n'));
        assert_eq!(typed('\u{8}'), None);
        assert_eq!(typed('\u{13}'), None);
        assert_eq!(typed('é'), Some('é'));
    }

    #[cfg(windows)]
    #[test]
    fn the_copied_virtual_key_codes_match_winuser_h() {
        use windows::Win32::UI::Input::KeyboardAndMouse as vk;
        for (ours, theirs) in [
            (VK_BACK, vk::VK_BACK),
            (VK_ESCAPE, vk::VK_ESCAPE),
            (VK_PRIOR, vk::VK_PRIOR),
            (VK_NEXT, vk::VK_NEXT),
            (VK_END, vk::VK_END),
            (VK_HOME, vk::VK_HOME),
            (VK_LEFT, vk::VK_LEFT),
            (VK_UP, vk::VK_UP),
            (VK_RIGHT, vk::VK_RIGHT),
            (VK_DOWN, vk::VK_DOWN),
            (VK_DELETE, vk::VK_DELETE),
            (0x72, vk::VK_F3),
        ] {
            assert_eq!(ours, theirs.0);
        }
    }
}
