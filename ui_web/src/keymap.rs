//! `KeyboardEvent` values → core operations.
//!
//! DOM-free on purpose, like the GTK shell's keymap: this is the part of a browser
//! shell worth testing, and here it is the only part that can be tested without a
//! browser at all. The caller reads `event.key` and the modifier flags; nothing in
//! this module knows what an `Element` is.
//!
//! The shortcuts are the web's, not a desktop's. `primary` is Ctrl on Windows and
//! Linux and ⌘ on macOS — the split every browser application makes — so ⌘S saves
//! on a Mac and Ctrl+S saves everywhere else, without the shell caring which. Redo
//! is Ctrl/⌘+Shift+Z, with Ctrl+Y accepted for users arriving from Windows.
//!
//! There is no Quit: a tab is not an application window. Unsaved changes are
//! defended with `beforeunload` instead, which is the browser's own convention.

use editor_core::{Direction, Editor};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    Insert(char),
    Backspace,
    Move(Direction),
    Open,
    Save,
    Undo,
    Redo,
}

/// What the shell must do itself, because it needs the page rather than the core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Show the file picker. The browser only allows this from a user gesture,
    /// which a keydown handler is.
    Open,
    /// Hand the document to a download.
    Save,
}

/// A key press, reduced to what the mapping actually depends on.
///
/// `primary` is already resolved from `ctrlKey || metaKey` at the DOM edge, so the
/// platform question is answered in exactly one place.
#[derive(Clone, Debug)]
pub struct Chord<'a> {
    pub key: &'a str,
    pub primary: bool,
    pub shift: bool,
    pub alt: bool,
}

pub fn action_for(chord: &Chord<'_>) -> Option<UiAction> {
    if chord.primary {
        // Modified keys are commands; none of them may reach the document. The key
        // arrives upper-cased when Shift is down, hence the fold.
        return match chord.key.to_ascii_lowercase().as_str() {
            "o" => Some(UiAction::Open),
            "s" => Some(UiAction::Save),
            "z" if chord.shift => Some(UiAction::Redo),
            "z" => Some(UiAction::Undo),
            "y" => Some(UiAction::Redo),
            _ => None,
        };
    }

    // Alt is left to the browser: it opens menus and switches tabs.
    if chord.alt {
        return None;
    }

    match chord.key {
        "ArrowLeft" => Some(UiAction::Move(Direction::Left)),
        "ArrowRight" => Some(UiAction::Move(Direction::Right)),
        "ArrowUp" => Some(UiAction::Move(Direction::Up)),
        "ArrowDown" => Some(UiAction::Move(Direction::Down)),
        "Backspace" => Some(UiAction::Backspace),
        "Enter" => Some(UiAction::Insert('\n')),
        "Tab" => Some(UiAction::Insert('\t')),
        // `key` is the character itself for anything printable, and a name like
        // "Escape" or "F5" for everything else — so a single non-control character
        // is text, and a longer string never is.
        other => {
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) if !ch.is_control() => Some(UiAction::Insert(ch)),
                _ => None,
            }
        }
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
        UiAction::Open => return Some(Request::Open),
        UiAction::Save => return Some(Request::Save),
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::Position;

    fn plain(key: &str) -> Chord<'_> {
        Chord {
            key,
            primary: false,
            shift: false,
            alt: false,
        }
    }

    fn primary(key: &str) -> Chord<'_> {
        Chord {
            key,
            primary: true,
            shift: false,
            alt: false,
        }
    }

    #[test]
    fn printable_keys_are_text() {
        assert_eq!(action_for(&plain("a")), Some(UiAction::Insert('a')));
        assert_eq!(action_for(&plain(" ")), Some(UiAction::Insert(' ')));
        assert_eq!(action_for(&plain("ä")), Some(UiAction::Insert('ä')));
        // Shift alone is the browser's way of saying "capital A".
        assert_eq!(
            action_for(&Chord {
                key: "A",
                primary: false,
                shift: true,
                alt: false
            }),
            Some(UiAction::Insert('A'))
        );
    }

    #[test]
    fn named_keys_never_become_text() {
        // Every non-printable key arrives as a multi-character name, which is
        // exactly what stops "Escape" from being typed into the document.
        assert_eq!(action_for(&plain("Escape")), None);
        assert_eq!(action_for(&plain("F5")), None);
        assert_eq!(action_for(&plain("Shift")), None);
        assert_eq!(action_for(&plain("Dead")), None);
    }

    #[test]
    fn editing_keys_map_to_core_operations() {
        assert_eq!(action_for(&plain("Enter")), Some(UiAction::Insert('\n')));
        assert_eq!(action_for(&plain("Tab")), Some(UiAction::Insert('\t')));
        assert_eq!(action_for(&plain("Backspace")), Some(UiAction::Backspace));
        assert_eq!(
            action_for(&plain("ArrowDown")),
            Some(UiAction::Move(Direction::Down))
        );
    }

    #[test]
    fn shortcuts_follow_browser_conventions() {
        assert_eq!(action_for(&primary("s")), Some(UiAction::Save));
        assert_eq!(action_for(&primary("o")), Some(UiAction::Open));
        assert_eq!(action_for(&primary("z")), Some(UiAction::Undo));
        assert_eq!(action_for(&primary("y")), Some(UiAction::Redo));
        assert_eq!(
            action_for(&Chord {
                key: "Z",
                primary: true,
                shift: true,
                alt: false
            }),
            Some(UiAction::Redo),
            "Shift upper-cases the key, and Ctrl/⌘+Shift+Z is redo"
        );
    }

    #[test]
    fn the_same_mapping_serves_ctrl_and_command() {
        // The shell resolves ctrlKey || metaKey before it gets here, so macOS and
        // the rest of the world share one table.
        assert_eq!(action_for(&primary("s")), Some(UiAction::Save));
    }

    #[test]
    fn a_shortcut_does_not_also_type_its_letter() {
        let editor = Editor::new();
        let action = action_for(&primary("s")).unwrap();
        assert_eq!(apply(action, &editor), Some(Request::Save));
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn browser_shortcuts_we_do_not_own_are_left_alone() {
        // Ctrl+T, Ctrl+W and friends must reach the browser, or the tab becomes a
        // trap. Alt is left alone for the same reason.
        assert_eq!(action_for(&primary("t")), None);
        assert_eq!(
            action_for(&Chord {
                key: "ArrowLeft",
                primary: false,
                shift: false,
                alt: true
            }),
            None,
            "Alt+Left is the browser's Back"
        );
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
