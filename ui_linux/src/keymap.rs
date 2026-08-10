//! GDK key events → core operations.
//!
//! Kept free of widgets on purpose: this is the part of the shell worth testing,
//! and it runs without a display server. Shortcuts follow the GNOME HIG, which is
//! why redo is Ctrl+Shift+Z (Ctrl+Y is accepted as well, since users arrive from
//! other platforms).

use editor_core::{Direction, Editor};
use libadwaita::gtk::gdk::{Key, ModifierType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    Insert(char),
    Backspace,
    Move(Direction),
    Save,
    Undo,
    Redo,
    Quit,
}

/// What the shell must do itself, because it needs the window rather than the core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Save,
    Quit,
}

pub fn action_for(key: Key, modifiers: ModifierType) -> Option<UiAction> {
    let ctrl = modifiers.contains(ModifierType::CONTROL_MASK);
    let shift = modifiers.contains(ModifierType::SHIFT_MASK);

    if ctrl {
        // Ctrl-modified keys are commands; none of them may reach the document.
        return match key {
            Key::s | Key::S => Some(UiAction::Save),
            Key::z | Key::Z if shift => Some(UiAction::Redo),
            Key::z | Key::Z => Some(UiAction::Undo),
            Key::y | Key::Y => Some(UiAction::Redo),
            Key::q | Key::Q => Some(UiAction::Quit),
            _ => None,
        };
    }

    match key {
        Key::Left => Some(UiAction::Move(Direction::Left)),
        Key::Right => Some(UiAction::Move(Direction::Right)),
        Key::Up => Some(UiAction::Move(Direction::Up)),
        Key::Down => Some(UiAction::Move(Direction::Down)),
        Key::BackSpace => Some(UiAction::Backspace),
        Key::Return | Key::KP_Enter => Some(UiAction::Insert('\n')),
        Key::Tab => Some(UiAction::Insert('\t')),
        // Anything that produces a printable character is text. Control characters
        // are dropped so Escape and friends cannot end up in the document.
        other => other
            .to_unicode()
            .filter(|ch| !ch.is_control())
            .map(UiAction::Insert),
    }
}

/// Run an action against the core, returning what the shell still has to handle.
pub fn apply(action: UiAction, editor: &Editor) -> Option<Request> {
    match action {
        UiAction::Insert(ch) => editor.handle_input(ch),
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
        UiAction::Quit => return Some(Request::Quit),
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::Position;

    const NONE: ModifierType = ModifierType::empty();
    const CTRL: ModifierType = ModifierType::CONTROL_MASK;

    #[test]
    fn printable_keys_are_text() {
        assert_eq!(action_for(Key::a, NONE), Some(UiAction::Insert('a')));
        assert_eq!(
            action_for(Key::A, ModifierType::SHIFT_MASK),
            Some(UiAction::Insert('A'))
        );
        assert_eq!(action_for(Key::space, NONE), Some(UiAction::Insert(' ')));
    }

    #[test]
    fn control_keys_never_become_text() {
        assert_eq!(action_for(Key::Escape, NONE), None);
        assert_eq!(action_for(Key::F1, NONE), None);
        // A bare Ctrl press has no action, and must not insert anything.
        assert_eq!(action_for(Key::Control_L, CTRL), None);
    }

    #[test]
    fn editing_keys_map_to_core_operations() {
        assert_eq!(action_for(Key::Return, NONE), Some(UiAction::Insert('\n')));
        assert_eq!(
            action_for(Key::KP_Enter, NONE),
            Some(UiAction::Insert('\n'))
        );
        assert_eq!(action_for(Key::Tab, NONE), Some(UiAction::Insert('\t')));
        assert_eq!(action_for(Key::BackSpace, NONE), Some(UiAction::Backspace));
        assert_eq!(
            action_for(Key::Down, NONE),
            Some(UiAction::Move(Direction::Down))
        );
    }

    #[test]
    fn shortcuts_follow_gnome_conventions() {
        assert_eq!(action_for(Key::s, CTRL), Some(UiAction::Save));
        assert_eq!(action_for(Key::z, CTRL), Some(UiAction::Undo));
        assert_eq!(
            action_for(Key::z, CTRL | ModifierType::SHIFT_MASK),
            Some(UiAction::Redo),
            "GNOME's redo is Ctrl+Shift+Z"
        );
        assert_eq!(action_for(Key::y, CTRL), Some(UiAction::Redo));
        assert_eq!(action_for(Key::q, CTRL), Some(UiAction::Quit));
    }

    #[test]
    fn a_ctrl_shortcut_does_not_also_type_its_letter() {
        let editor = Editor::new();
        let action = action_for(Key::s, CTRL).unwrap();
        assert_eq!(apply(action, &editor), Some(Request::Save));
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn apply_routes_editing_to_the_core() {
        let editor = Editor::new();
        apply(UiAction::Insert('h'), &editor);
        apply(UiAction::Insert('i'), &editor);
        assert_eq!(editor.text(), "hi");

        apply(UiAction::Backspace, &editor);
        assert_eq!(editor.text(), "h");

        apply(UiAction::Undo, &editor);
        assert_eq!(editor.text(), "hi");
        apply(UiAction::Redo, &editor);
        assert_eq!(editor.text(), "h");
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
