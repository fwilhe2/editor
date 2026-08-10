//! Event routing and rendering. Holds no editor state of its own.
//!
//! Everything on screen is derived from the core each frame: the text comes from
//! `get_viewport`, the cursor from `cursor()`, the scroll position from
//! `scroll_offset()`. The only fields here are presentation concerns — a status
//! message and the quit flags.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use editor_core::{Direction, Editor, EditorObserver};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Set by the core whenever the document changes; the event loop redraws when it is.
///
/// This is the shell's half of the reactive contract — the same `EditorObserver`
/// that Swift and C# will implement. The TUI could redraw unconditionally after
/// every keystroke, but then background work in the core would never reach the
/// screen, and that is exactly what the other shells will need.
#[derive(Default)]
pub struct RedrawFlag(AtomicBool);

impl RedrawFlag {
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::SeqCst)
    }

    pub fn raise(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl EditorObserver for RedrawFlag {
    fn state_changed(&self) {
        self.raise();
    }
}

pub struct App {
    editor: Arc<Editor>,
    /// Raised by the core through the observer, and by this shell for its own
    /// presentation changes (the status line, the quit prompt) which the core
    /// knows nothing about.
    redraw: Arc<RedrawFlag>,
    status: String,
    quit: bool,
    /// Ctrl+Q was pressed with unsaved changes; a second press confirms.
    confirming_quit: bool,
}

impl App {
    pub fn new(editor: Arc<Editor>, redraw: Arc<RedrawFlag>) -> Self {
        App {
            editor,
            redraw,
            status: "Ctrl+S save   Ctrl+Z undo   Ctrl+Y redo   Ctrl+Q quit".to_string(),
            quit: false,
            confirming_quit: false,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        // Windows reports press *and* release; acting on both double-types every key.
        if key.kind != KeyEventKind::Press {
            return;
        }
        // Any accepted key can change the status line even when the document is untouched.
        self.redraw.raise();

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Any key other than a second Ctrl+Q abandons the quit confirmation.
        if self.confirming_quit && !(ctrl && key.code == KeyCode::Char('q')) {
            self.confirming_quit = false;
        }

        match (ctrl, key.code) {
            (true, KeyCode::Char('q')) => self.request_quit(),
            (true, KeyCode::Char('s')) => self.save(),
            (true, KeyCode::Char('z')) => self.report("Nothing to undo", self.editor.undo()),
            (true, KeyCode::Char('y')) => self.report("Nothing to redo", self.editor.redo()),

            (_, KeyCode::Left) => self.editor.move_cursor(Direction::Left),
            (_, KeyCode::Right) => self.editor.move_cursor(Direction::Right),
            (_, KeyCode::Up) => self.editor.move_cursor(Direction::Up),
            (_, KeyCode::Down) => self.editor.move_cursor(Direction::Down),

            (_, KeyCode::Enter) => self.editor.handle_input('\n'),
            (_, KeyCode::Tab) => self.editor.handle_input('\t'),
            (_, KeyCode::Backspace) => {
                self.editor.handle_backspace();
            }
            // Ctrl-modified characters are commands, not text.
            (false, KeyCode::Char(ch)) => self.editor.handle_input(ch),

            _ => {}
        }
    }

    fn request_quit(&mut self) {
        if self.editor.is_dirty() && !self.confirming_quit {
            self.confirming_quit = true;
            self.status = "Unsaved changes — Ctrl+Q again to discard, Ctrl+S to save".to_string();
            return;
        }
        self.quit = true;
    }

    fn save(&mut self) {
        self.status = match self.editor.save_file() {
            Ok(()) => format!(
                "Saved {}",
                self.editor
                    .path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            ),
            Err(error) => format!("{error}"),
        };
    }

    fn report(&mut self, when_nothing_happened: &str, changed: bool) {
        if !changed {
            self.status = when_nothing_happened.to_string();
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let [text_area, status_area] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

        let start = self.editor.follow_cursor(u64::from(text_area.height));
        let viewport = self
            .editor
            .get_viewport(start, start + u64::from(text_area.height));

        let lines: Vec<Line> = viewport.lines.iter().map(Line::raw).collect();
        frame.render_widget(Paragraph::new(lines), text_area);
        frame.render_widget(self.status_bar(&viewport), status_area);

        // Place the hardware cursor from the core's logical position, so the terminal
        // draws the real caret instead of the UI faking one.
        let cursor = viewport.cursor;
        let on_screen = cursor.line >= viewport.start_line
            && cursor.line < viewport.start_line + u64::from(text_area.height);
        if on_screen && text_area.width > 0 {
            let row = (cursor.line - viewport.start_line) as u16;
            // Without horizontal scrolling the caret parks at the right edge on long lines.
            let column = cursor.column.min(u64::from(text_area.width - 1)) as u16;
            frame.set_cursor_position((text_area.x + column, text_area.y + row));
        }
    }

    fn status_bar(&self, viewport: &editor_core::Viewport) -> Paragraph<'_> {
        let path = self
            .editor
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "[no file]".to_string());

        // 1-based, matching what the CLI prints, so the two agree about a position.
        let position = format!(
            "{}:{}",
            viewport.cursor.line + 1,
            viewport.cursor.column + 1
        );

        let left = format!(
            "{path}{}  {position}  {} lines",
            if self.editor.is_dirty() { " *" } else { "" },
            viewport.total_lines
        );

        Paragraph::new(Line::from(vec![
            Span::styled(left, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("   "),
            Span::raw(self.status.clone()),
        ]))
        .style(Style::default().fg(Color::Black).bg(Color::Gray))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app_with(text: &str) -> App {
        let editor = Arc::new(Editor::new());
        editor.insert_text(text);
        editor.set_cursor(editor_core::Position::new(0, 0));
        App::new(editor, Arc::new(RedrawFlag::default()))
    }

    fn press(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(app: &mut App, ch: char) {
        app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
    }

    /// Render into an off-screen buffer and return it as lines of text.
    fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|row| {
                (0..width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn typing_reaches_the_document_and_the_screen() {
        let mut app = app_with("");
        press(&mut app, KeyCode::Char('h'));
        press(&mut app, KeyCode::Char('i'));
        assert_eq!(render(&mut app, 20, 3)[0], "hi");
    }

    #[test]
    fn enter_and_backspace_are_routed_to_the_core() {
        let mut app = app_with("ab");
        press(&mut app, KeyCode::Right); // between 'a' and 'b'
        press(&mut app, KeyCode::Enter);
        let screen = render(&mut app, 20, 4);
        assert_eq!(screen[0], "a");
        assert_eq!(screen[1], "b");

        press(&mut app, KeyCode::Backspace);
        assert_eq!(render(&mut app, 20, 4)[0], "ab");
    }

    #[test]
    fn ctrl_keys_are_commands_not_text() {
        let mut app = app_with("");
        press(&mut app, KeyCode::Char('x'));
        ctrl(&mut app, 'z');
        assert_eq!(render(&mut app, 20, 3)[0], "", "ctrl+z undid the insert");
        ctrl(&mut app, 'y');
        assert_eq!(render(&mut app, 20, 3)[0], "x");
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut app = app_with("");
        let mut key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        key.kind = KeyEventKind::Release;
        app.on_key(key);
        assert_eq!(render(&mut app, 20, 3)[0], "");
    }

    #[test]
    fn the_view_scrolls_to_follow_the_cursor_down_and_back() {
        let mut app = app_with("l0\nl1\nl2\nl3\nl4\nl5");
        // Two text rows plus the status bar.
        for _ in 0..4 {
            press(&mut app, KeyCode::Down);
        }
        let screen = render(&mut app, 20, 3);
        assert_eq!(&screen[..2], &["l3".to_string(), "l4".to_string()]);

        for _ in 0..4 {
            press(&mut app, KeyCode::Up);
        }
        let screen = render(&mut app, 20, 3);
        assert_eq!(&screen[..2], &["l0".to_string(), "l1".to_string()]);
    }

    #[test]
    fn the_status_bar_shows_a_one_based_position_and_the_dirty_marker() {
        let mut app = app_with("ab\ncd");
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Right);
        let status = render(&mut app, 60, 3).pop().unwrap();
        assert!(status.contains("2:2"), "got {status:?}");
        assert!(status.contains('*'), "unsaved changes marker: {status:?}");
    }

    #[test]
    fn quitting_with_unsaved_changes_needs_confirmation() {
        let mut app = app_with("x");
        ctrl(&mut app, 'q');
        assert!(!app.should_quit(), "first Ctrl+Q only warns");
        assert!(render(&mut app, 80, 3).pop().unwrap().contains("Unsaved"));

        ctrl(&mut app, 'q');
        assert!(app.should_quit());
    }

    #[test]
    fn any_other_key_cancels_the_quit_confirmation() {
        let mut app = app_with("x");
        ctrl(&mut app, 'q');
        press(&mut app, KeyCode::Char('y'));
        ctrl(&mut app, 'q');
        assert!(!app.should_quit(), "the confirmation should have reset");
    }

    #[test]
    fn a_clean_document_quits_immediately() {
        let mut app = App::new(Arc::new(Editor::new()), Arc::new(RedrawFlag::default()));
        ctrl(&mut app, 'q');
        assert!(app.should_quit());
    }

    #[test]
    fn a_status_only_change_still_asks_for_a_redraw() {
        let editor = Arc::new(Editor::new());
        let redraw = Arc::new(RedrawFlag::default());
        let mut app = App::new(editor, redraw.clone());

        redraw.take();
        // Undo on an empty history touches nothing in the core, so the observer
        // stays quiet — but the status line changed and must be repainted.
        ctrl(&mut app, 'z');
        assert!(redraw.take());
    }

    #[test]
    fn the_observer_flag_reports_core_changes() {
        let editor = Arc::new(Editor::new());
        let flag = Arc::new(RedrawFlag::default());
        editor.set_observer(flag.clone());

        assert!(!flag.take());
        editor.handle_input('a');
        assert!(flag.take(), "the core notified the shell");
        assert!(!flag.take(), "taking the flag clears it");
    }
}
