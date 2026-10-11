// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! Event routing and drawing. Holds no document state.
//!
//! Every frame is drawn from [`App::view`], and every key becomes a call into the core. The
//! fields below are the presentation state a terminal genuinely owns — a status message, what
//! the prompt line holds, the register a terminal uses for a clipboard — and nothing the core
//! could know.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use quill_core::{App, Command, Frame, Observer, Search, command};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::cells::{self, Cells};
use crate::keymap::{self, Action};

/// Raised by the core through [`Observer`]; the event loop draws when it is set.
///
/// The flag doubles as coalescing: three notifications from one keystroke are one redraw.
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

impl Observer for RedrawFlag {
    fn changed(&self) {
        self.raise();
    }
}

/// What the line under the status bar is doing.
#[derive(Clone, Debug, PartialEq)]
enum Mode {
    Normal,
    /// Asking for a line of text.
    Prompt {
        kind: Ask,
        input: String,
    },
    /// Ctrl+P: every command, filtered by what has been typed.
    Palette {
        query: String,
        selected: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    Open,
    SaveAs,
    Find,
}

impl Ask {
    fn label(self) -> &'static str {
        match self {
            Ask::Open => "Open",
            Ask::SaveAs => "Save as",
            Ask::Find => "Find",
        }
    }
}

pub struct Shell {
    app: Arc<App>,
    redraw: Arc<RedrawFlag>,
    mode: Mode,
    status: String,
    /// A terminal has no clipboard this process can rely on, so it keeps a register, as vi and
    /// Emacs do. (OSC 52 would reach the system clipboard in some terminals; a named gap.)
    register: String,
    /// A command that would discard unsaved changes, waiting for the same key again.
    confirming: Option<Command>,
    quit: bool,
    /// The terminal's size as of the last draw — what input handling measures frames against.
    size: (u16, u16),
}

impl Shell {
    pub fn new(app: Arc<App>, redraw: Arc<RedrawFlag>) -> Shell {
        Shell {
            app,
            redraw,
            mode: Mode::Normal,
            status: String::new(),
            register: String::new(),
            confirming: None,
            quit: false,
            size: (80, 24),
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.size = (width, height);
        self.redraw.raise();
    }

    // --- layout ----------------------------------------------------------------------------

    /// The text area, the gutter beside it, and the two bars under it.
    fn areas(&self, area: Rect) -> (Rect, Rect, Rect, Rect) {
        let [body, status, hint] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        let digits = self.app.line_count().to_string().len() as u16;
        let gutter = (digits + 1).min(body.width.saturating_sub(1));
        let [gutter, text] =
            Layout::horizontal([Constraint::Length(gutter), Constraint::Min(1)]).areas(body);
        (gutter, text, status, hint)
    }

    fn text_area(&self) -> Rect {
        let (width, height) = self.size;
        self.areas(Rect::new(0, 0, width, height)).1
    }

    fn frame_for(area: Rect) -> Frame<'static> {
        Frame::new(f32::from(area.width), f32::from(area.height), &Cells)
    }

    fn frame(&self) -> Frame<'static> {
        Self::frame_for(self.text_area())
    }

    // --- input -----------------------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        // Windows reports press *and* release; acting on both types every key twice.
        if key.kind != KeyEventKind::Press {
            return;
        }
        // The status line may change without the document changing.
        self.redraw.raise();
        match self.mode.clone() {
            Mode::Prompt { kind, input } => return self.on_prompt_key(kind, input, key),
            Mode::Palette { query, selected } => return self.on_palette_key(query, selected, key),
            Mode::Normal => {}
        }
        let Some(action) = keymap::action_for(key) else {
            return;
        };
        if self.confirming.is_some() && action != Action::Command(self.confirming.unwrap()) {
            self.confirming = None;
            self.status.clear();
        }
        self.act(action);
    }

    fn act(&mut self, action: Action) {
        let frame = self.frame();
        match action {
            Action::Command(command) => self.run(command),
            Action::Move(motion, extend) => self.app.move_caret(motion, extend, &frame),
            Action::Delete(kind) => {
                self.app.delete(kind);
            }
            Action::Type(c) => self.app.type_text(c.encode_utf8(&mut [0; 4])),
            Action::Escape => {
                self.app.clear_selection();
                self.status.clear();
            }
            Action::Palette => {
                self.mode = Mode::Palette {
                    query: String::new(),
                    selected: 0,
                }
            }
        }
        // Input handling, never drawing, is what scrolls to the caret.
        self.app.follow_caret(&frame);
    }

    /// Carry out a menu verb.
    pub fn run(&mut self, command: Command) {
        match command {
            Command::New | Command::Open | Command::Close if !self.may_discard(command) => {}
            Command::New => self.app.new_document(),
            Command::Open => self.ask(Ask::Open, String::new()),
            Command::Close => self.quit = true,
            Command::Save if self.app.path().is_none() => self.ask(Ask::SaveAs, String::new()),
            Command::Save => self.save(None),
            Command::SaveAs => {
                let current = self.app.path().map(|p| p.display().to_string());
                self.ask(Ask::SaveAs, current.unwrap_or_default());
            }
            Command::Undo => self.say_unless(self.app.undo(), "Nothing to undo"),
            Command::Redo => self.say_unless(self.app.redo(), "Nothing to redo"),
            Command::Cut => {
                let text = self.app.cut();
                if !text.is_empty() {
                    self.register = text;
                }
            }
            Command::Copy => {
                let text = self.app.selected_text();
                if !text.is_empty() {
                    self.register = text;
                    self.status = "Copied".into();
                }
            }
            Command::Paste => self.app.insert_text(&self.register.clone()),
            Command::SelectAll => self.app.select_all(),
            Command::Find => self.ask(Ask::Find, self.app.search().unwrap_or_default()),
            Command::FindNext | Command::FindPrevious if self.app.search().is_none() => {
                self.ask(Ask::Find, String::new());
            }
            Command::FindNext => {
                let found = self.app.find_again(Search::Forward);
                self.say_unless(found, "No match");
            }
            Command::FindPrevious => {
                let found = self.app.find_again(Search::Backward);
                self.say_unless(found, "No match");
            }
            Command::About => {
                self.status = format!(
                    "{} {} — a shared core with native shells. MIT licensed.",
                    quill_core::APP_NAME,
                    quill_core::VERSION
                );
            }
        }
    }

    /// Whether `command` may throw the document away now. With unsaved changes, the first press
    /// warns and the second goes ahead — the terminal's version of a Save/Discard dialog.
    fn may_discard(&mut self, command: Command) -> bool {
        if !self.app.is_dirty() || self.confirming == Some(command) {
            self.confirming = None;
            return true;
        }
        let key = keymap::describe(command).unwrap_or_default();
        self.status = format!("Unsaved changes — {key} again to discard them, ^S to save");
        self.confirming = Some(command);
        false
    }

    fn ask(&mut self, kind: Ask, input: String) {
        self.mode = Mode::Prompt { kind, input };
    }

    fn say_unless(&mut self, done: bool, message: &str) {
        if !done {
            self.status = message.into();
        }
    }

    fn save(&mut self, path: Option<&str>) {
        let result = match path {
            Some(path) => self.app.save_file_as(path),
            None => self.app.save_file(),
        };
        self.status = match result {
            Ok(()) => format!("Saved {}", self.app.title()),
            Err(error) => error.to_string(),
        };
    }

    fn on_prompt_key(&mut self, kind: Ask, mut input: String, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.answer(kind, &input);
            }
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::Prompt { kind, input };
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                input.push(c);
                self.mode = Mode::Prompt { kind, input };
            }
            _ => {}
        }
    }

    fn answer(&mut self, kind: Ask, input: &str) {
        if input.is_empty() {
            return;
        }
        match kind {
            Ask::Open => {
                let result = if std::path::Path::new(input).exists() {
                    self.app.open_file(input)
                } else {
                    // A name that does not exist yet is a new file, as in every terminal editor.
                    self.app.open_bytes(Some(input), b"")
                };
                if let Err(error) = result {
                    self.status = error.to_string();
                }
            }
            Ask::SaveAs => self.save(Some(input)),
            Ask::Find => {
                let found = self.app.find(input, Search::Forward);
                self.say_unless(found, "No match");
                self.app.follow_caret(&self.frame());
            }
        }
    }

    /// The palette's rows for `query`, best first.
    fn matches(query: &str) -> Vec<Command> {
        let mut ranked: Vec<(u32, Command)> = Command::ALL
            .iter()
            .filter_map(|&c| command::score(query, c.label()).map(|score| (score, c)))
            .collect();
        ranked.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
        ranked.into_iter().map(|(_, c)| c).collect()
    }

    fn on_palette_key(&mut self, mut query: String, mut selected: usize, key: KeyEvent) {
        let matches = Self::matches(&query);
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                if let Some(&command) = matches.get(selected) {
                    self.run(command);
                    self.app.follow_caret(&self.frame());
                }
                return;
            }
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Down => selected = (selected + 1).min(matches.len().saturating_sub(1)),
            KeyCode::Backspace => {
                query.pop();
                selected = 0;
            }
            KeyCode::Char(c) => {
                query.push(c);
                selected = 0;
            }
            _ => {}
        }
        self.mode = Mode::Palette { query, selected };
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        let text = self.text_area();
        let frame = Self::frame_for(text);
        let inside = text.contains(Position::new(mouse.column, mouse.row));
        let x = f32::from(mouse.column.saturating_sub(text.x));
        let y = f32::from(mouse.row.saturating_sub(text.y));
        let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                self.app.place_caret(&frame, x, y, shift);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.app.place_caret(&frame, x, y, true);
                self.app.follow_caret(&frame);
            }
            MouseEventKind::ScrollDown => self.app.scroll_by(&frame, 3),
            MouseEventKind::ScrollUp => self.app.scroll_by(&frame, -3),
            _ => {}
        }
    }

    // --- drawing ---------------------------------------------------------------------------

    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        self.size = (area.width, area.height);
        let (gutter, text, status, hint) = self.areas(area);
        let view = self.app.view(&Self::frame_for(text));

        let selected = Style::default().add_modifier(Modifier::REVERSED);
        let mut numbers = Vec::new();
        let mut rows = Vec::new();
        for row in &view.rows {
            numbers.push(Line::styled(
                if row.first {
                    format!(
                        "{:>w$}",
                        row.line + 1,
                        w = gutter.width.saturating_sub(1) as usize
                    )
                } else {
                    String::new()
                },
                Style::default().fg(Color::DarkGray),
            ));
            let mut spans = Vec::new();
            let mut x = 0.0;
            for c in row.text.chars() {
                let width = cells::width(c) as f32;
                let style = match row.selection {
                    Some(s) if x >= s.from && x < s.to => selected,
                    _ => Style::default(),
                };
                spans.push(Span::styled(cells::shown(c), style));
                x += width;
            }
            // A selected line break: one reversed cell past the text.
            if let Some(s) = row.selection
                && s.to > x
            {
                spans.push(Span::styled(" ", selected));
            }
            rows.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(numbers), gutter);
        frame.render_widget(Paragraph::new(rows), text);
        frame.render_widget(self.status_bar(&view), status);
        frame.render_widget(self.hint_line(), hint);

        if let Mode::Palette { query, selected } = &self.mode {
            self.draw_palette(frame, text, query, *selected);
        } else if let Mode::Prompt { input, kind } = &self.mode {
            let x = hint.x + kind.label().len() as u16 + 2 + input.chars().count() as u16;
            frame.set_cursor_position((x.min(hint.right().saturating_sub(1)), hint.y));
        } else if let Some(caret) = view.caret {
            // The terminal's own cursor, so it blinks the way the user configured it.
            let x = text.x + (caret.x as u16).min(text.width.saturating_sub(1));
            frame.set_cursor_position((x, text.y + caret.top as u16));
        }
    }

    fn status_bar(&self, view: &quill_core::View) -> Paragraph<'_> {
        let caret = self.app.caret();
        let encoding = self.app.encoding();
        let dirty = if self.app.is_dirty() { " [+]" } else { "" };
        let left = format!(" {}{dirty}", self.app.title());
        let right = format!(
            "{}:{}  {} lines  {}{}  {} ",
            caret.line + 1,
            caret.column + 1,
            view.line_count,
            encoding.line_ending.as_str(),
            if encoding.bom { " BOM" } else { "" },
            self.status
        );
        Paragraph::new(Line::from(vec![
            Span::styled(left, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::raw(right),
        ]))
        .style(Style::default().fg(Color::Black).bg(Color::Gray))
    }

    fn hint_line(&self) -> Paragraph<'_> {
        if let Mode::Prompt { kind, input } = &self.mode {
            return Paragraph::new(format!("{}: {input}", kind.label()));
        }
        let key = Style::default().add_modifier(Modifier::REVERSED);
        let mut spans = Vec::new();
        for command in [
            Command::Save,
            Command::Close,
            Command::Find,
            Command::Undo,
            Command::Cut,
            Command::Paste,
        ] {
            spans.push(Span::styled(keymap::describe(command).unwrap(), key));
            spans.push(Span::raw(format!(" {}  ", command.label())));
        }
        spans.push(Span::styled("^P", key));
        spans.push(Span::raw(" Commands"));
        Paragraph::new(Line::from(spans))
    }

    fn draw_palette(&self, frame: &mut ratatui::Frame, over: Rect, query: &str, selected: usize) {
        let matches = Self::matches(query);
        let width = over.width.min(60);
        let height = (matches.len() as u16 + 2).min(over.height);
        let area = Rect::new(over.x + (over.width - width) / 2, over.y, width, height);
        let items: Vec<ListItem> = matches
            .iter()
            .map(|c| {
                let key = keymap::describe(*c).unwrap_or_default();
                ListItem::new(format!("{:<14} {:>4}  {}", c.label(), key, c.summary()))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" > {query} ")),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut state = ListState::default().with_selected(Some(selected));
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(list, area, &mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::Position as At;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn shell(text: &str) -> Shell {
        let app = Arc::new(App::new());
        app.open_bytes(Some("t.txt"), text.as_bytes()).unwrap();
        Shell::new(app, Arc::new(RedrawFlag::default()))
    }

    fn press(shell: &mut Shell, code: KeyCode) {
        shell.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(shell: &mut Shell, c: char) {
        shell.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    fn typing(shell: &mut Shell, text: &str) {
        for c in text.chars() {
            press(shell, KeyCode::Char(c));
        }
    }

    /// Draw into an off-screen buffer and return its rows as text.
    fn screen(shell: &mut Shell, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| shell.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn typing_reaches_the_document_and_the_screen() {
        let mut shell = shell("");
        typing(&mut shell, "hi");
        assert_eq!(screen(&mut shell, 20, 4)[0], "1 hi");
    }

    #[test]
    fn long_lines_wrap_at_the_terminals_width_and_carry_one_number() {
        let mut shell = shell("hello wide world\nnext");
        let rows = screen(&mut shell, 10, 6);
        assert_eq!(&rows[..4], ["1 hello", "  wide", "  world", "2 next"]);
    }

    #[test]
    fn down_follows_the_wrapped_rows() {
        let mut shell = shell("hello wide world");
        screen(&mut shell, 10, 6);
        press(&mut shell, KeyCode::Down);
        assert_eq!(shell.app.caret(), At::new(0, 6));
    }

    #[test]
    fn cut_and_paste_go_through_the_register() {
        let mut shell = shell("hello");
        shell.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT));
        ctrl(&mut shell, 'x');
        assert_eq!(screen(&mut shell, 20, 4)[0], "1");
        ctrl(&mut shell, 'v');
        ctrl(&mut shell, 'v');
        assert_eq!(screen(&mut shell, 20, 4)[0], "1 hellohello");
    }

    #[test]
    fn closing_with_unsaved_changes_asks_twice() {
        let mut shell = shell("");
        typing(&mut shell, "x");
        ctrl(&mut shell, 'q');
        assert!(!shell.should_quit());
        assert!(screen(&mut shell, 80, 4)[2].contains("Unsaved changes"));
        ctrl(&mut shell, 'q');
        assert!(shell.should_quit());
    }

    #[test]
    fn any_other_key_cancels_the_question() {
        let mut shell = shell("");
        typing(&mut shell, "x");
        ctrl(&mut shell, 'q');
        press(&mut shell, KeyCode::Left);
        ctrl(&mut shell, 'q');
        assert!(!shell.should_quit());
    }

    #[test]
    fn find_prompts_selects_and_find_next_repeats() {
        let mut shell = shell("ab ab");
        ctrl(&mut shell, 'f');
        typing(&mut shell, "ab");
        assert!(screen(&mut shell, 40, 4)[3].starts_with("Find: ab"));
        press(&mut shell, KeyCode::Enter);
        assert_eq!(shell.app.selection(), Some((At::new(0, 0), At::new(0, 2))));
        shell.on_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE));
        assert_eq!(shell.app.selection(), Some((At::new(0, 3), At::new(0, 5))));
    }

    #[test]
    fn the_palette_runs_what_it_finds() {
        let mut shell = shell("abc");
        ctrl(&mut shell, 'p');
        typing(&mut shell, "select all");
        assert!(
            screen(&mut shell, 70, 10)
                .iter()
                .any(|r| r.contains("Select All"))
        );
        press(&mut shell, KeyCode::Enter);
        assert_eq!(shell.app.selected_text(), "abc");
    }

    #[test]
    fn a_selection_is_drawn_reversed() {
        let mut shell = shell("abc");
        shell.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).unwrap();
        terminal.draw(|frame| shell.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        assert!(
            buffer[(2, 0)].modifier.contains(Modifier::REVERSED),
            "the a"
        );
        assert!(
            !buffer[(3, 0)].modifier.contains(Modifier::REVERSED),
            "the b"
        );
    }

    #[test]
    fn a_click_puts_the_caret_under_it() {
        let mut shell = shell("hello wide world");
        screen(&mut shell, 10, 6);
        shell.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(shell.app.caret(), At::new(0, 8));
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut shell = shell("");
        let mut key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        key.kind = KeyEventKind::Release;
        shell.on_key(key);
        assert_eq!(screen(&mut shell, 20, 4)[0], "1");
    }

    #[test]
    fn the_observer_flag_coalesces_notifications() {
        let app = Arc::new(App::new());
        let flag = Arc::new(RedrawFlag::default());
        app.observe(flag.clone());
        app.type_text("a");
        app.type_text("b");
        assert!(flag.take());
        assert!(!flag.take());
    }
}
