//! Windows virtual-key codes and characters → core operations.
//!
//! Windows-free on purpose, like `ui_linux/src/keymap.rs`, `ui_web/src/keymap.rs`
//! and `ui_egui/src/keymap.rs`. A virtual-key code is a `u16` and a modifier is a
//! bool, so nothing here needs the `windows` crate — which is what lets the whole
//! key table be tested on a Linux machine that cannot open a Windows window.
//!
//! The constants below are copied from `winuser.h` rather than imported, and
//! [`tests::the_virtual_key_codes_match_the_windows_headers`] pins every one of them
//! against the real thing when the crate is built for Windows. Copying a wrong
//! number is the obvious failure mode of doing it this way, so it is the one thing
//! CI checks first.
//!
//! # Shortcuts
//!
//! Microsoft's keyboard conventions, not GNOME's and not this project's other
//! shells': **Ctrl+Y is redo**, with Ctrl+Shift+Z accepted because half the world
//! arrived from elsewhere. There is deliberately **no Ctrl+Q** — quitting on
//! Windows is the close button or Alt+F4, both of which arrive as `WM_CLOSE` and
//! neither of which is this file's business.

use editor_core::{Direction, Editor};

/// Virtual-key codes, from `winuser.h`.
///
/// Stable since Windows 3.1 and part of the ABI, so copying them is safe in a way
/// that copying most constants is not.
mod vk {
    pub const BACK: u16 = 0x08;
    pub const TAB: u16 = 0x09;
    pub const RETURN: u16 = 0x0D;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const S: u16 = 0x53;
    pub const Y: u16 = 0x59;
    pub const Z: u16 = 0x5A;
}

/// Which modifiers were held, read from `GetKeyState` when the message arrived.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiAction {
    /// Text to insert. A `String` rather than a `char` because a surrogate pair or
    /// an IME commit can produce more than one `char`, and because every other shell
    /// here carries it that way.
    Insert(String),
    Backspace,
    Move(Direction),
    Save,
    Undo,
    Redo,
}

/// What the shell must do itself, because it needs the window rather than the core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Save,
}

/// Map a `WM_KEYDOWN` to an action.
///
/// Enter, Tab and Backspace are claimed here rather than left to `WM_CHAR`, which
/// would also deliver them as `\r`, `\t` and `\x08`. [`action_for_char`] drops those,
/// so exactly one of the two paths acts on each — the alternative is every newline
/// arriving twice.
pub fn action_for_key(key: u16, modifiers: Modifiers) -> Option<UiAction> {
    if modifiers.ctrl {
        return match key {
            vk::S => Some(UiAction::Save),
            vk::Z if modifiers.shift => Some(UiAction::Redo),
            vk::Z => Some(UiAction::Undo),
            vk::Y => Some(UiAction::Redo),
            _ => None,
        };
    }

    // Alt belongs to the menu bar and the window manager. Alt+F4 in particular must
    // reach DefWindowProc, or this window could not be closed from the keyboard.
    if modifiers.alt {
        return None;
    }

    match key {
        vk::LEFT => Some(UiAction::Move(Direction::Left)),
        vk::RIGHT => Some(UiAction::Move(Direction::Right)),
        vk::UP => Some(UiAction::Move(Direction::Up)),
        vk::DOWN => Some(UiAction::Move(Direction::Down)),
        vk::BACK => Some(UiAction::Backspace),
        vk::RETURN => Some(UiAction::Insert("\n".to_string())),
        vk::TAB => Some(UiAction::Insert("\t".to_string())),
        _ => None,
    }
}

/// Map a `WM_CHAR` to an action.
///
/// Control characters are dropped, which does two jobs at once: it keeps Enter, Tab
/// and Backspace to [`action_for_key`], and it stops Ctrl+S from typing an `0x13`
/// into the document. The Ctrl check is belt and braces on top — that bug has
/// appeared in three shells in this repository.
pub fn action_for_char(ch: char, modifiers: Modifiers) -> Option<UiAction> {
    if modifiers.ctrl || ch.is_control() {
        return None;
    }
    Some(UiAction::Insert(ch.to_string()))
}

/// Run an action against the core, returning what the shell still has to handle.
pub fn apply(action: UiAction, editor: &Editor) -> Option<Request> {
    match action {
        UiAction::Insert(text) => editor.insert_text(&text),
        UiAction::Backspace => {
            editor.handle_backspace();
        }
        UiAction::Move(direction) => editor.move_cursor(direction),
        UiAction::Undo => {
            editor.undo();
        }
        UiAction::Redo => {
            editor.redo();
        }
        UiAction::Save => return Some(Request::Save),
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::Position;

    const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
    };
    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        shift: false,
        alt: false,
    };
    const CTRL_SHIFT: Modifiers = Modifiers {
        ctrl: true,
        shift: true,
        alt: false,
    };
    const ALT: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: true,
    };

    /// The risk of copying constants instead of importing them, made checkable.
    ///
    /// Only compiled for Windows, where the `windows` crate is a dependency, so it
    /// runs in CI rather than here — which is the right way round: this is the one
    /// assertion in the file that genuinely needs the platform.
    #[cfg(windows)]
    #[test]
    fn the_virtual_key_codes_match_the_windows_headers() {
        use windows::Win32::UI::Input::KeyboardAndMouse as kb;
        assert_eq!(vk::BACK, kb::VK_BACK.0);
        assert_eq!(vk::TAB, kb::VK_TAB.0);
        assert_eq!(vk::RETURN, kb::VK_RETURN.0);
        assert_eq!(vk::LEFT, kb::VK_LEFT.0);
        assert_eq!(vk::UP, kb::VK_UP.0);
        assert_eq!(vk::RIGHT, kb::VK_RIGHT.0);
        assert_eq!(vk::DOWN, kb::VK_DOWN.0);
        // The letters are their ASCII codes, which is a documented guarantee.
        assert_eq!(vk::S, b'S' as u16);
        assert_eq!(vk::Y, b'Y' as u16);
        assert_eq!(vk::Z, b'Z' as u16);
    }

    #[test]
    fn typed_characters_are_inserted() {
        assert_eq!(
            action_for_char('a', NONE),
            Some(UiAction::Insert("a".into()))
        );
        assert_eq!(
            action_for_char(' ', NONE),
            Some(UiAction::Insert(" ".into()))
        );
        assert_eq!(
            action_for_char('ä', NONE),
            Some(UiAction::Insert("ä".into()))
        );
    }

    #[test]
    fn control_characters_never_reach_the_document() {
        // WM_CHAR delivers these alongside the WM_KEYDOWN that action_for_key
        // already handled. Acting on both would insert every newline twice.
        assert_eq!(action_for_char('\r', NONE), None);
        assert_eq!(action_for_char('\t', NONE), None);
        assert_eq!(action_for_char('\u{8}', NONE), None);
        // Ctrl+S arrives here as 0x13 if it arrives at all.
        assert_eq!(action_for_char('\u{13}', NONE), None);
    }

    #[test]
    fn a_shortcut_does_not_also_type_its_letter() {
        // Belt and braces on top of the control-character filter.
        assert_eq!(action_for_char('s', CTRL), None);
    }

    #[test]
    fn editing_keys_map_to_core_operations() {
        assert_eq!(
            action_for_key(vk::RETURN, NONE),
            Some(UiAction::Insert("\n".into()))
        );
        assert_eq!(
            action_for_key(vk::TAB, NONE),
            Some(UiAction::Insert("\t".into()))
        );
        assert_eq!(action_for_key(vk::BACK, NONE), Some(UiAction::Backspace));
        assert_eq!(
            action_for_key(vk::DOWN, NONE),
            Some(UiAction::Move(Direction::Down))
        );
        assert_eq!(
            action_for_key(vk::LEFT, NONE),
            Some(UiAction::Move(Direction::Left))
        );
    }

    #[test]
    fn the_shortcuts_are_the_windows_ones() {
        assert_eq!(action_for_key(vk::S, CTRL), Some(UiAction::Save));
        assert_eq!(action_for_key(vk::Z, CTRL), Some(UiAction::Undo));
        // Ctrl+Y is redo on Windows. The GNOME and macOS shells do not agree, and
        // are not supposed to.
        assert_eq!(action_for_key(vk::Y, CTRL), Some(UiAction::Redo));
        assert_eq!(action_for_key(vk::Z, CTRL_SHIFT), Some(UiAction::Redo));
    }

    #[test]
    fn alt_is_left_to_the_window_manager() {
        // Alt+F4 must reach DefWindowProc, and so must every menu mnemonic.
        assert_eq!(action_for_key(vk::LEFT, ALT), None);
        assert_eq!(action_for_key(vk::S, ALT), None);
    }

    #[test]
    fn keys_this_shell_does_not_own_are_left_alone() {
        // Home, End, PageUp, PageDown and Delete: the core has no operation for any
        // of them, and inventing one in a shell is what the parity rule forbids.
        for key in [0x24u16, 0x23, 0x21, 0x22, 0x2E, 0x1B, 0x74] {
            assert_eq!(action_for_key(key, NONE), None);
        }
        assert_eq!(action_for_key(0x54, CTRL), None, "Ctrl+T");
        // No Ctrl+Q: quitting on Windows is Alt+F4 or the close button.
        assert_eq!(action_for_key(0x51, CTRL), None, "Ctrl+Q");
    }

    #[test]
    fn apply_routes_editing_to_the_core() {
        let editor = Editor::new();
        apply(UiAction::Insert("h".into()), &editor);
        apply(UiAction::Insert("i".into()), &editor);
        assert_eq!(editor.text(), "hi");

        apply(UiAction::Backspace, &editor);
        assert_eq!(editor.text(), "h");

        apply(UiAction::Undo, &editor);
        assert_eq!(editor.text(), "hi");
        apply(UiAction::Redo, &editor);
        assert_eq!(editor.text(), "h");
    }

    #[test]
    fn saving_is_handed_back_to_the_shell_without_touching_the_document() {
        let editor = Editor::new();
        let action = action_for_key(vk::S, CTRL).unwrap();
        assert_eq!(apply(action, &editor), Some(Request::Save));
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn apply_moves_the_cursor_without_editing() {
        let editor = Editor::new();
        editor.insert_text("ab\ncd");
        editor.set_cursor(Position::new(1, 2));

        apply(UiAction::Move(Direction::Up), &editor);
        assert_eq!(editor.cursor(), Position::new(0, 2));
        assert_eq!(editor.text(), "ab\ncd");
    }
}
