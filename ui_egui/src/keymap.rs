//! `egui::Event` values → core operations.
//!
//! Widget-free on purpose, like `ui_linux/src/keymap.rs` and `ui_web/src/keymap.rs`:
//! `egui::Event` is a plain data type, so every decision in this file can be tested
//! with no window, no display and no GPU.
//!
//! egui hands text and keys over separately — printable input arrives as
//! [`egui::Event::Text`], everything else as [`egui::Event::Key`] — which does by
//! itself most of the work the browser shell has to do by inspecting the key name.
//!
//! The shortcuts are nobody's in particular, because this shell belongs to no
//! platform. [`egui::Modifiers::command`] is already ⌘ on macOS and Ctrl everywhere
//! else, which is exactly the browser shell's `primary`, resolved by the toolkit
//! rather than by the shell. Redo is command+Shift+Z, with Ctrl+Y accepted for people
//! arriving from Windows.

use editor_core::{Direction, Editor};
use eframe::egui;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiAction {
    /// Text to insert. A `String` rather than a `char` because egui delivers text as
    /// one, and because a dead key or an IME commit can produce more than one
    /// character at a time.
    Insert(String),
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

/// Map one event to an action.
///
/// `held` is the frame's current modifier state, needed only for text: an
/// [`egui::Event::Text`] carries no modifiers of its own, and a command-modified key
/// must never also reach the document. egui is not believed to deliver text for
/// those, and it is guarded regardless — that bug has appeared in three shells here.
pub fn action_for(event: &egui::Event, held: egui::Modifiers) -> Option<UiAction> {
    match event {
        egui::Event::Text(_) if held.command => None,
        egui::Event::Text(text) if text.is_empty() => None,
        egui::Event::Text(text) => Some(UiAction::Insert(text.clone())),

        egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => key_action(*key, *modifiers),

        // Releases are ignored: acting on both edges types everything twice, which is
        // the same trap the TUI meets as `KeyEventKind::Release`.
        _ => None,
    }
}

fn key_action(key: egui::Key, modifiers: egui::Modifiers) -> Option<UiAction> {
    if modifiers.command {
        return match key {
            egui::Key::S => Some(UiAction::Save),
            egui::Key::Z if modifiers.shift => Some(UiAction::Redo),
            egui::Key::Z => Some(UiAction::Undo),
            egui::Key::Y => Some(UiAction::Redo),
            egui::Key::Q => Some(UiAction::Quit),
            _ => None,
        };
    }

    // Alt belongs to the window manager and to whatever egui makes of it.
    if modifiers.alt {
        return None;
    }

    match key {
        egui::Key::ArrowLeft => Some(UiAction::Move(Direction::Left)),
        egui::Key::ArrowRight => Some(UiAction::Move(Direction::Right)),
        egui::Key::ArrowUp => Some(UiAction::Move(Direction::Up)),
        egui::Key::ArrowDown => Some(UiAction::Move(Direction::Down)),
        egui::Key::Backspace => Some(UiAction::Backspace),
        // egui documents that Enter never also produces a `Text` event, so this is
        // the only place a newline can come from.
        egui::Key::Enter => Some(UiAction::Insert("\n".to_string())),
        egui::Key::Tab => Some(UiAction::Insert("\t".to_string())),
        _ => None,
    }
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
        UiAction::Quit => return Some(Request::Quit),
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::Position;

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn plain(k: egui::Key) -> egui::Event {
        key(k, egui::Modifiers::NONE)
    }

    fn command(k: egui::Key) -> egui::Event {
        key(k, egui::Modifiers::COMMAND)
    }

    fn text(s: &str) -> egui::Event {
        egui::Event::Text(s.to_string())
    }

    #[test]
    fn typed_text_is_inserted() {
        assert_eq!(
            action_for(&text("a"), egui::Modifiers::NONE),
            Some(UiAction::Insert("a".into()))
        );
        assert_eq!(
            action_for(&text(" "), egui::Modifiers::NONE),
            Some(UiAction::Insert(" ".into()))
        );
        assert_eq!(
            action_for(&text("ä"), egui::Modifiers::NONE),
            Some(UiAction::Insert("ä".into()))
        );
        // A dead key or an IME can commit more than one character at once, which is
        // why this carries a String.
        assert_eq!(
            action_for(&text("ni"), egui::Modifiers::NONE),
            Some(UiAction::Insert("ni".into()))
        );
    }

    #[test]
    fn a_key_release_does_nothing() {
        // Acting on both edges would type every character twice.
        let release = egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(action_for(&release, egui::Modifiers::NONE), None);
    }

    #[test]
    fn editing_keys_map_to_core_operations() {
        assert_eq!(
            action_for(&plain(egui::Key::Enter), egui::Modifiers::NONE),
            Some(UiAction::Insert("\n".into()))
        );
        assert_eq!(
            action_for(&plain(egui::Key::Tab), egui::Modifiers::NONE),
            Some(UiAction::Insert("\t".into()))
        );
        assert_eq!(
            action_for(&plain(egui::Key::Backspace), egui::Modifiers::NONE),
            Some(UiAction::Backspace)
        );
        assert_eq!(
            action_for(&plain(egui::Key::ArrowDown), egui::Modifiers::NONE),
            Some(UiAction::Move(Direction::Down))
        );
    }

    #[test]
    fn shortcuts_use_the_toolkits_own_ctrl_command_split() {
        // `Modifiers::COMMAND` is ⌘ on macOS and Ctrl elsewhere, so one table serves
        // every platform this shell runs on.
        assert_eq!(
            action_for(&command(egui::Key::S), egui::Modifiers::COMMAND),
            Some(UiAction::Save)
        );
        assert_eq!(
            action_for(&command(egui::Key::Z), egui::Modifiers::COMMAND),
            Some(UiAction::Undo)
        );
        assert_eq!(
            action_for(&command(egui::Key::Y), egui::Modifiers::COMMAND),
            Some(UiAction::Redo)
        );
        assert_eq!(
            action_for(&command(egui::Key::Q), egui::Modifiers::COMMAND),
            Some(UiAction::Quit)
        );

        let shift_z = key(
            egui::Key::Z,
            egui::Modifiers {
                shift: true,
                ..egui::Modifiers::COMMAND
            },
        );
        assert_eq!(
            action_for(&shift_z, egui::Modifiers::COMMAND),
            Some(UiAction::Redo)
        );
    }

    #[test]
    fn a_shortcut_does_not_also_type_its_letter() {
        let editor = Editor::new();
        let action = action_for(&command(egui::Key::S), egui::Modifiers::COMMAND).unwrap();
        assert_eq!(apply(action, &editor), Some(Request::Save));
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn keys_this_shell_does_not_own_are_left_alone() {
        assert_eq!(
            action_for(&plain(egui::Key::Escape), egui::Modifiers::NONE),
            None
        );
        assert_eq!(
            action_for(&plain(egui::Key::F5), egui::Modifiers::NONE),
            None
        );
        assert_eq!(
            action_for(&command(egui::Key::T), egui::Modifiers::COMMAND),
            None
        );

        let alt_left = key(
            egui::Key::ArrowLeft,
            egui::Modifiers {
                alt: true,
                ..egui::Modifiers::NONE
            },
        );
        assert_eq!(action_for(&alt_left, egui::Modifiers::NONE), None);
    }

    #[test]
    fn text_arriving_under_a_shortcut_is_dropped() {
        // Belt and braces: if a platform ever does deliver an "s" alongside
        // command+S, it must not land in the document.
        assert_eq!(action_for(&text("s"), egui::Modifiers::COMMAND), None);
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
    fn apply_moves_the_cursor_without_editing() {
        let editor = Editor::new();
        editor.insert_text("ab\ncd");
        editor.set_cursor(Position::new(1, 2));

        apply(UiAction::Move(Direction::Up), &editor);
        assert_eq!(editor.cursor(), Position::new(0, 2));
        assert_eq!(editor.text(), "ab\ncd");
    }
}
