// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The editing keys — motion, deletion, Enter, Tab — mapped to the core's vocabulary.
//!
//! Typed *text* does not come through here: it arrives from the input method as a committed
//! string (`gtk::IMMulticontext`), which is how dead keys, compose sequences and CJK input work.
//! Command keys do not either: they are accelerators on the window's actions ([`crate::menu`]).
//! What is left is a pure function of a key and its modifiers, testable with no display.

use libadwaita::gtk::gdk::{Key, ModifierType};
use quill_core::{Delete, Motion};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Move(Motion, bool),
    Delete(Delete),
    Type(char),
    Escape,
}

pub fn action_for(key: Key, mods: ModifierType) -> Option<Action> {
    let shift = mods.contains(ModifierType::SHIFT_MASK);
    let ctrl = mods.contains(ModifierType::CONTROL_MASK);
    let motion = |m| Some(Action::Move(m, shift));
    match key {
        Key::Left | Key::KP_Left if ctrl => motion(Motion::WordLeft),
        Key::Right | Key::KP_Right if ctrl => motion(Motion::WordRight),
        Key::Home | Key::KP_Home if ctrl => motion(Motion::DocumentStart),
        Key::End | Key::KP_End if ctrl => motion(Motion::DocumentEnd),
        Key::Left | Key::KP_Left => motion(Motion::Left),
        Key::Right | Key::KP_Right => motion(Motion::Right),
        Key::Up | Key::KP_Up => motion(Motion::Up),
        Key::Down | Key::KP_Down => motion(Motion::Down),
        Key::Home | Key::KP_Home => motion(Motion::RowStart),
        Key::End | Key::KP_End => motion(Motion::RowEnd),
        Key::Page_Up | Key::KP_Page_Up => motion(Motion::PageUp),
        Key::Page_Down | Key::KP_Page_Down => motion(Motion::PageDown),
        Key::BackSpace if ctrl => Some(Action::Delete(Delete::WordBackward)),
        Key::Delete | Key::KP_Delete if ctrl => Some(Action::Delete(Delete::WordForward)),
        Key::BackSpace => Some(Action::Delete(Delete::Backward)),
        Key::Delete | Key::KP_Delete => Some(Action::Delete(Delete::Forward)),
        Key::Return | Key::KP_Enter if !ctrl => Some(Action::Type('\n')),
        Key::Tab if !ctrl => Some(Action::Type('\t')),
        Key::Escape => Some(Action::Escape),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_motion_has_a_key() {
        let keys = [
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::Home,
            Key::End,
            Key::Page_Up,
            Key::Page_Down,
        ];
        let mut reached = Vec::new();
        for key in keys {
            for mods in [ModifierType::empty(), ModifierType::CONTROL_MASK] {
                if let Some(Action::Move(motion, _)) = action_for(key, mods) {
                    reached.push(motion);
                }
            }
        }
        for motion in Motion::ALL {
            assert!(reached.contains(&motion), "{} has no key", motion.name());
        }
    }

    #[test]
    fn shift_extends_and_letters_are_left_to_the_input_method() {
        assert_eq!(
            action_for(Key::Right, ModifierType::SHIFT_MASK),
            Some(Action::Move(Motion::Right, true))
        );
        assert_eq!(action_for(Key::a, ModifierType::empty()), None);
        assert_eq!(action_for(Key::s, ModifierType::CONTROL_MASK), None);
    }
}
