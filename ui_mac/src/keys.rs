// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Keys as the text system names them — selectors, not key codes.
//!
//! A Mac application does not read ⌥← as "Option and the left arrow". It hands the key event to
//! `interpretKeyEvents:`, the text system looks it up in the user's key bindings (the system's,
//! plus `~/Library/KeyBindings/DefaultKeyBinding.dict`) and calls back with a selector —
//! `moveWordLeft:`, `deleteBackward:`, `moveToEndOfDocument:` — or with text to insert. So the
//! user's own bindings, and Emacs-style ⌃A/⌃E, work here as they do in TextEdit, and this table
//! is the whole of the mapping: a pure function of a selector's name.

use quill_core::{Delete, Motion};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Move(Motion, bool),
    Delete(Delete),
    Type(char),
    Escape,
}

/// What a `doCommandBySelector:` selector means, or `None` for one this view does not answer
/// (AppKit then beeps, as every text view does for a key that means nothing).
pub fn action(selector: &str) -> Option<Action> {
    let (name, extend) = match selector.strip_suffix("AndModifySelection:") {
        Some(name) => (name, true),
        None => (selector.strip_suffix(':').unwrap_or(selector), false),
    };
    let motion = match name {
        "moveLeft" | "moveBackward" => Motion::Left,
        "moveRight" | "moveForward" => Motion::Right,
        "moveWordLeft" | "moveWordBackward" => Motion::WordLeft,
        "moveWordRight" | "moveWordForward" => Motion::WordRight,
        "moveUp" => Motion::Up,
        "moveDown" => Motion::Down,
        // ⌘← and ⌘→: the ends of the line on screen.
        "moveToLeftEndOfLine" | "moveToBeginningOfLine" => Motion::RowStart,
        "moveToRightEndOfLine" | "moveToEndOfLine" => Motion::RowEnd,
        // ⌥↑ and ⌥↓ move by paragraph on the Mac; a line is this editor's paragraph, and the
        // nearest motion it has is the row's end. A named gap rather than a guess.
        "moveToBeginningOfParagraph" => Motion::RowStart,
        "moveToEndOfParagraph" => Motion::RowEnd,
        "moveToBeginningOfDocument" => Motion::DocumentStart,
        "moveToEndOfDocument" => Motion::DocumentEnd,
        // Page Up and Down move the caret on the Mac only with Option; plain, they scroll.
        // This editor's scroll follows its caret, so both mean a screenful.
        "pageUp" | "scrollPageUp" => Motion::PageUp,
        "pageDown" | "scrollPageDown" => Motion::PageDown,
        "scrollToBeginningOfDocument" => Motion::DocumentStart,
        "scrollToEndOfDocument" => Motion::DocumentEnd,
        _ => {
            return match selector {
                "deleteBackward:" | "deleteBackwardByDecomposingPreviousCharacter:" => {
                    Some(Action::Delete(Delete::Backward))
                }
                "deleteForward:" => Some(Action::Delete(Delete::Forward)),
                "deleteWordBackward:" => Some(Action::Delete(Delete::WordBackward)),
                "deleteWordForward:" => Some(Action::Delete(Delete::WordForward)),
                "insertNewline:" | "insertLineBreak:" | "insertNewlineIgnoringFieldEditor:" => {
                    Some(Action::Type('\n'))
                }
                "insertTab:" | "insertTabIgnoringFieldEditor:" => Some(Action::Type('\t')),
                "cancelOperation:" => Some(Action::Escape),
                _ => None,
            };
        }
    };
    Some(Action::Move(motion, extend))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_motion_has_a_selector() {
        let selectors = [
            "moveLeft:",
            "moveRight:",
            "moveWordLeft:",
            "moveWordRight:",
            "moveUp:",
            "moveDown:",
            "moveToLeftEndOfLine:",
            "moveToRightEndOfLine:",
            "pageUp:",
            "pageDown:",
            "moveToBeginningOfDocument:",
            "moveToEndOfDocument:",
        ];
        let reached: Vec<Motion> = selectors
            .iter()
            .filter_map(|s| match action(s) {
                Some(Action::Move(m, _)) => Some(m),
                _ => None,
            })
            .collect();
        for motion in Motion::ALL {
            assert!(
                reached.contains(&motion),
                "{} has no selector",
                motion.name()
            );
        }
    }

    #[test]
    fn and_modify_selection_extends() {
        assert_eq!(
            action("moveWordLeftAndModifySelection:"),
            Some(Action::Move(Motion::WordLeft, true))
        );
        assert_eq!(action("moveLeft:"), Some(Action::Move(Motion::Left, false)));
    }

    #[test]
    fn editing_selectors_and_unknown_ones() {
        assert_eq!(
            action("deleteWordBackward:"),
            Some(Action::Delete(Delete::WordBackward))
        );
        assert_eq!(action("insertNewline:"), Some(Action::Type('\n')));
        assert_eq!(action("transpose:"), None, "beeps, as TextEdit would not");
    }
}
