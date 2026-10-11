// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The window: one `HWND`, one message loop, and nothing that owns a document.
//!
//! **No `EDIT` or rich-edit control, ever.** Both own their own text, exactly as `GtkTextView`
//! and `contenteditable` do, and the moment one exists there are two documents. Every paint fills
//! the client area from [`App::view`]; every key goes to the core.
//!
//! **Nothing holds a borrow of [`Window`] across a modal loop.** A message box, a file dialog and
//! the clipboard's owner all run nested message loops that re-enter the window procedure — so a
//! `&mut Window` alive across one is a second `&mut` the moment `WM_PAINT` arrives, which is
//! undefined behaviour even when it seems to work. Every modal call below therefore happens in a
//! free function between two short, separate borrows ([`with`]). That rule is a property of this
//! file rather than of anybody's memory.
//!
//! Windows' conventions, without Windows' text controls: a system menu bar ([`crate::menu`]),
//! the comdlg32 Open, Save As and Find dialogs, a three-button "Do you want to save changes?",
//! `*name - Quill` as the title, the system caret (so the user's blink rate and screen readers
//! follow it), a dark title bar when the user's theme is dark, and per-monitor DPI.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use quill_core::{App, Command, Observer, Search};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Controls::SetScrollInfo;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, w};

use crate::gdi::{self, Fonts, Measurer, Status};
use crate::keymap::{self, Action, Modifiers, Utf16};
use crate::menu::{self, Entry};
use crate::theme::{Theme, wheel_rows};

/// Posted by the observer when the core changed. `WM_APP` is the first value Windows leaves to
/// applications, so this collides with nothing the system sends.
const WM_CORE_CHANGED: u32 = WM_APP + 1;

/// `CF_UNICODETEXT`: the one clipboard format this window speaks.
const CF_UNICODETEXT: u32 = 13;

thread_local! {
    /// The modeless Find dialog, while it is open — the message loop must hand it its keys.
    static FIND: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
    /// `FINDMSGSTRING`'s message number, registered at start-up.
    static FIND_MESSAGE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Everything the window needs — and no document state, which stays behind [`App`].
struct Window {
    app: Arc<App>,
    fonts: Fonts,
    theme: Theme,
    /// The last thing the window has to say, in the status bar.
    message: String,
    /// The title last set, so an unchanged one is not sent again.
    title: String,
    pending: Arc<AtomicBool>,
    utf16: Utf16,
    wheel: i32,
    /// The Find dialog's state: comdlg32 writes the query into this buffer, so it must stay put
    /// for as long as the dialog is open — hence boxed.
    find: Box<FINDREPLACEW>,
    find_text: Box<[u16; 256]>,
}

/// Turns a change in the core into a repaint request.
///
/// `HWND` is a raw pointer and so not `Send`, while [`Observer`] must be `Send + Sync`, so the
/// handle travels as an `isize` and is used only with `PostMessageW` — which Windows documents
/// as callable from any thread. The flag coalesces a burst into one paint.
struct Notifier {
    hwnd: isize,
    pending: Arc<AtomicBool>,
}

impl Observer for Notifier {
    fn changed(&self) {
        if self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut c_void)),
                WM_CORE_CHANGED,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

impl Window {
    fn client(hwnd: HWND) -> (i32, i32) {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rect);
        }
        (rect.right - rect.left, rect.bottom - rect.top)
    }

    /// Run `f` against the frame the window shows now, with the geometry it is laid out in.
    fn with_frame<T>(
        &self,
        hwnd: HWND,
        f: impl FnOnce(&quill_core::Frame, &crate::theme::Geometry) -> T,
    ) -> T {
        let measurer = Measurer::new(&self.fonts);
        let metrics = measurer.metrics();
        let (width, height) = Self::client(hwnd);
        let g = gdi::layout(&metrics, &self.fonts, &self.app, width, height);
        f(&gdi::frame(&metrics, &g), &g)
    }

    fn status(&self) -> Status {
        let caret = self.app.caret();
        let encoding = self.app.encoding();
        Status {
            left: if self.message.is_empty() {
                self.app.title()
            } else {
                self.message.clone()
            },
            right: format!(
                "Ln {}, Col {}    {}{}",
                caret.line + 1,
                caret.column + 1,
                if encoding.bom {
                    "UTF-8 with BOM    "
                } else {
                    "UTF-8    "
                },
                match encoding.line_ending {
                    quill_core::LineEnding::Crlf => "Windows (CRLF)",
                    quill_core::LineEnding::Lf => "Unix (LF)",
                }
            ),
        }
    }

    /// Notepad's title: an asterisk before the name when there are unsaved changes.
    fn window_title(&self) -> String {
        let dirty = if self.app.is_dirty() { "*" } else { "" };
        format!("{dirty}{} - {}", self.app.title(), quill_core::APP_NAME)
    }

    /// Bring the title, the scroll bar and the screen into line with the core.
    fn refresh(&mut self, hwnd: HWND) {
        let title = self.window_title();
        if title != self.title {
            let wide = wide(&title);
            unsafe {
                let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
            }
            self.title = title;
        }
        let (rows, scroll) = self.with_frame(hwnd, |frame, _| (frame.rows(), self.app.scroll()));
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS | SIF_DISABLENOSCROLL,
            nMin: 0,
            nMax: (self.app.line_count() + rows).saturating_sub(2) as i32,
            nPage: rows as u32,
            nPos: scroll.line as i32,
            ..Default::default()
        };
        unsafe {
            SetScrollInfo(hwnd, SB_VERT, &info, true);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }

    /// After input: bring the caret into view. The repaint comes from the observer.
    fn after_input(&mut self, hwnd: HWND) {
        self.with_frame(hwnd, |frame, _| self.app.follow_caret(frame));
    }

    fn paint(&mut self, hwnd: HWND) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let (width, height) = Self::client(hwnd);
            // Into a memory bitmap and blitted once: drawing text straight onto the window
            // flickers on every keystroke.
            let memory = CreateCompatibleDC(Some(hdc));
            let bitmap = CreateCompatibleBitmap(hdc, width.max(1), height.max(1));
            let previous = SelectObject(memory, bitmap.into());
            let (g, view) = gdi::paint(
                memory,
                &self.fonts,
                &self.app,
                width,
                height,
                &self.theme,
                &self.status(),
            );
            let _ = BitBlt(hdc, 0, 0, width, height, Some(memory), 0, 0, SRCCOPY);
            SelectObject(memory, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            let _ = EndPaint(hwnd, &ps);

            // The system caret, where the core says the cursor is — or hidden when it is
            // scrolled away.
            match view.caret {
                Some(caret) => {
                    let _ =
                        SetCaretPos(g.text_left + caret.x as i32, g.text_top + caret.top as i32);
                    let _ = ShowCaret(Some(hwnd));
                }
                None => {
                    let _ = HideCaret(Some(hwnd));
                }
            }
        }
    }

    fn act(&mut self, hwnd: HWND, action: Action) {
        self.message.clear();
        match action {
            Action::Move(motion, extend) => {
                self.with_frame(hwnd, |frame, _| self.app.move_caret(motion, extend, frame));
            }
            Action::Delete(kind) => {
                self.app.delete(kind);
            }
            Action::Escape => {
                self.app.clear_selection();
            }
            Action::Command(_) => unreachable!("commands go through `run`"),
        }
        self.after_input(hwnd);
    }

    fn place(&mut self, hwnd: HWND, lparam: LPARAM, extend: bool) {
        let x = (lparam.0 & 0xFFFF) as u16 as i16 as f32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as f32;
        self.with_frame(hwnd, |frame, g| {
            let (x, y) = (x - g.text_left as f32, y - g.text_top as f32);
            self.app.place_caret(frame, x, y, extend);
            self.app.follow_caret(frame);
        });
    }

    fn apply_theme(&mut self, hwnd: HWND) {
        let dark = prefers_dark();
        self.theme = if dark { Theme::dark() } else { Theme::light() };
        let flag = BOOL::from(dark);
        unsafe {
            // Older builds do not know the attribute; no dark title bar there, nothing broken.
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &flag as *const _ as *const c_void,
                std::mem::size_of::<BOOL>() as u32,
            );
        }
    }
}

/// Borrow the window's state for the length of `f`, and no longer. See the module comment for
/// why every modal call sits *between* two of these rather than inside one.
fn with<T>(hwnd: HWND, f: impl FnOnce(&mut Window) -> T) -> Option<T> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Window;
    if pointer.is_null() {
        return None;
    }
    Some(f(unsafe { &mut *pointer }))
}

/// Carry out a menu verb. A free function, because several of them open a modal dialog.
fn run(hwnd: HWND, command: Command) {
    match command {
        Command::New => {
            if may_discard(hwnd) {
                with(hwnd, |w| w.app.new_document());
            }
        }
        Command::Open => {
            if may_discard(hwnd)
                && let Some(path) = file_dialog(hwnd, false, "")
            {
                with(hwnd, |w| {
                    if let Err(error) = w.app.open_file(&path) {
                        w.message = error.to_string();
                    }
                });
            }
        }
        Command::Save => {
            save(hwnd);
        }
        Command::SaveAs => {
            save_as(hwnd);
        }
        // Through WM_CLOSE, so the menu item asks the same question as the close button.
        Command::Close => unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        },
        Command::Undo => {
            with(hwnd, |w| w.app.undo());
        }
        Command::Redo => {
            with(hwnd, |w| w.app.redo());
        }
        Command::Cut | Command::Copy => {
            let text = with(hwnd, |w| {
                if command == Command::Cut {
                    w.app.cut()
                } else {
                    w.app.selected_text()
                }
            });
            if let Some(text) = text.filter(|t| !t.is_empty()) {
                set_clipboard(hwnd, &text);
            }
        }
        Command::Paste => {
            if let Some(text) = clipboard(hwnd) {
                with(hwnd, |w| w.app.insert_text(&text));
            }
        }
        Command::SelectAll => {
            with(hwnd, |w| w.app.select_all());
        }
        Command::Find => open_find(hwnd),
        Command::FindNext | Command::FindPrevious => {
            let direction = if command == Command::FindNext {
                Search::Forward
            } else {
                Search::Backward
            };
            let searched = with(hwnd, |w| w.app.search().is_some()).unwrap_or(false);
            if !searched {
                return open_find(hwnd);
            }
            with(hwnd, |w| {
                if !w.app.find_again(direction) {
                    w.message = "No match".into();
                }
            });
        }
        Command::About => {
            let text = wide(&format!(
                "{} {}\n\nA shared core with native shells — this one Win32 and GDI.\nMIT licence.",
                quill_core::APP_NAME,
                quill_core::VERSION
            ));
            unsafe {
                MessageBoxW(
                    Some(hwnd),
                    PCWSTR(text.as_ptr()),
                    w!("About Quill"),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
        }
    }
    with(hwnd, |w| w.after_input(hwnd));
}

/// Windows' question before work is lost: Yes saves, No discards, Cancel keeps the document.
fn may_discard(hwnd: HWND) -> bool {
    let Some(Some(name)) = with(hwnd, |w| w.app.is_dirty().then(|| w.app.title())) else {
        return true;
    };
    let text = wide(&format!("Do you want to save changes to {name}?"));
    let answer = unsafe {
        MessageBoxW(
            Some(hwnd),
            PCWSTR(text.as_ptr()),
            w!("Quill"),
            MB_YESNOCANCEL | MB_ICONWARNING,
        )
    };
    match answer {
        IDYES => save(hwnd),
        IDNO => true,
        _ => false,
    }
}

/// Save where the document came from, or ask where. True when it was saved.
fn save(hwnd: HWND) -> bool {
    match with(hwnd, |w| w.app.path().is_some()) {
        Some(true) => with(hwnd, |w| match w.app.save_file() {
            Ok(()) => true,
            Err(error) => {
                w.message = error.to_string();
                false
            }
        })
        .unwrap_or(false),
        _ => save_as(hwnd),
    }
}

fn save_as(hwnd: HWND) -> bool {
    let name = with(hwnd, |w| w.app.title()).unwrap_or_default();
    let Some(path) = file_dialog(hwnd, true, &name) else {
        return false;
    };
    with(hwnd, |w| match w.app.save_file_as(&path) {
        Ok(()) => true,
        Err(error) => {
            w.message = error.to_string();
            false
        }
    })
    .unwrap_or(false)
}

/// comdlg32's Open or Save As dialog. Modal — called with no borrow alive.
fn file_dialog(hwnd: HWND, save: bool, initial: &str) -> Option<std::path::PathBuf> {
    let mut buffer = [0u16; 1024];
    for (slot, unit) in buffer.iter_mut().zip(initial.encode_utf16()) {
        *slot = unit;
    }
    let filter = wide("Text documents (*.txt)\0*.txt\0All files (*.*)\0*.*\0");
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrDefExt: w!("txt"),
        Flags: if save {
            OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST
        } else {
            OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST
        },
        ..Default::default()
    };
    let chosen = unsafe {
        if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        }
    };
    if !chosen.as_bool() {
        return None;
    }
    let end = buffer.iter().position(|&u| u == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]).into())
}

/// Open comdlg32's Find dialog — modeless, as in Notepad. Its answers arrive as `FINDMSGSTRING`.
fn open_find(hwnd: HWND) {
    if FIND.get() != 0 {
        unsafe {
            let _ = SetFocus(Some(HWND(FIND.get() as *mut c_void)));
        }
        return;
    }
    let dialog = with(hwnd, |w| {
        let query: Vec<u16> = w.app.search().unwrap_or_default().encode_utf16().collect();
        w.find_text.fill(0);
        for (slot, unit) in w.find_text.iter_mut().zip(query.iter().take(255)) {
            *slot = *unit;
        }
        *w.find = FINDREPLACEW {
            lStructSize: std::mem::size_of::<FINDREPLACEW>() as u32,
            hwndOwner: hwnd,
            Flags: FR_DOWN | FR_HIDEWHOLEWORD | FR_HIDEMATCHCASE,
            lpstrFindWhat: windows::core::PWSTR(w.find_text.as_mut_ptr()),
            wFindWhatLen: w.find_text.len() as u16,
            ..Default::default()
        };
        unsafe { FindTextW(&mut *w.find) }
    });
    if let Some(dialog) = dialog {
        FIND.set(dialog.0 as isize);
    }
}

/// What the Find dialog sent: Find Next pressed, or the dialog closing.
fn on_find(hwnd: HWND, lparam: LPARAM) {
    let request = unsafe { &*(lparam.0 as *const FINDREPLACEW) };
    if request.Flags.contains(FR_DIALOGTERM) {
        FIND.set(0);
        return;
    }
    if request.Flags.contains(FR_FINDNEXT) {
        let query = unsafe { request.lpstrFindWhat.to_string() }.unwrap_or_default();
        let direction = if request.Flags.contains(FR_DOWN) {
            Search::Forward
        } else {
            Search::Backward
        };
        with(hwnd, |w| {
            if !w.app.find(&query, direction) {
                w.message = format!("Cannot find \"{query}\"");
                w.refresh(hwnd);
            }
            w.after_input(hwnd);
        });
    }
}

/// Put `text` on the clipboard as `CF_UNICODETEXT`, with Windows' CRLF line breaks.
fn set_clipboard(hwnd: HWND, text: &str) {
    let units: Vec<u16> = text
        .replace('\n', "\r\n")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2) {
            let target = GlobalLock(memory) as *mut u16;
            if !target.is_null() {
                std::ptr::copy_nonoverlapping(units.as_ptr(), target, units.len());
                let _ = GlobalUnlock(memory);
                // The clipboard owns the memory from here on.
                let _ = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

fn clipboard(hwnd: HWND) -> Option<String> {
    unsafe {
        OpenClipboard(Some(hwnd)).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|handle| {
            let memory = HGLOBAL(handle.0);
            let source = GlobalLock(memory) as *const u16;
            if source.is_null() {
                return None;
            }
            let mut len = 0;
            while *source.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(source, len));
            let _ = GlobalUnlock(memory);
            Some(text)
        });
        let _ = CloseClipboard();
        text
    }
}

fn build_menu() -> windows::core::Result<HMENU> {
    unsafe {
        let bar = CreateMenu()?;
        for (title, entries) in menu::MENUS {
            let popup = CreatePopupMenu()?;
            for entry in entries {
                match entry {
                    Entry::Separator => AppendMenuW(popup, MF_SEPARATOR, 0, PCWSTR::null())?,
                    Entry::Item { command, .. } => {
                        let text = wide(&menu::text(entry).unwrap());
                        AppendMenuW(
                            popup,
                            MF_STRING,
                            usize::from(menu::id(*command)),
                            PCWSTR(text.as_ptr()),
                        )?
                    }
                }
            }
            let title = wide(title);
            AppendMenuW(bar, MF_POPUP, popup.0 as usize, PCWSTR(title.as_ptr()))?;
        }
        Ok(bar)
    }
}

/// Is the user's theme dark? The Settings app's registry value is the supported answer for an
/// unpackaged Win32 process; absent means light, as Windows assumes.
fn prefers_dark() -> bool {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut c_void),
            Some(&mut size),
        )
    };
    result.is_ok() && value == 0
}

/// Which modifiers are down now — `WM_KEYDOWN` carries neither Ctrl nor Shift.
fn modifiers() -> Modifiers {
    unsafe {
        Modifiers {
            ctrl: GetKeyState(VK_CONTROL.0 as i32) < 0,
            shift: GetKeyState(VK_SHIFT.0 as i32) < 0,
            alt: GetKeyState(VK_MENU.0 as i32) < 0,
        }
    }
}

fn wheel_lines() -> u32 {
    let mut lines: u32 = 3;
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWHEELSCROLLLINES,
            0,
            Some(&mut lines as *mut _ as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    lines
}

/// `MAKEINTRESOURCE`: a resource named by number travels as a "pointer" whose value is the
/// number — `app.rc`'s icon is `1`.
fn resource(id: u16) -> PCWSTR {
    PCWSTR(usize::from(id) as *const u16)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

extern "system" fn wndproc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = lparam.0 as *const CREATESTRUCTW;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        // The handlers that may open a modal loop come first, before any borrow is taken.
        match message {
            WM_CLOSE => {
                if may_discard(hwnd) {
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }
            WM_COMMAND => {
                if let Some(command) = menu::from_id((wparam.0 & 0xFFFF) as u16) {
                    run(hwnd, command);
                    return LRESULT(0);
                }
                return DefWindowProcW(hwnd, message, wparam, lparam);
            }
            WM_KEYDOWN => {
                let mods = modifiers();
                match keymap::action_for_key(wparam.0 as u16, mods) {
                    Some(Action::Command(command)) => run(hwnd, command),
                    Some(action) => {
                        with(hwnd, |w| w.act(hwnd, action));
                    }
                    None => return DefWindowProcW(hwnd, message, wparam, lparam),
                }
                return LRESULT(0);
            }
            m if m != 0 && m == FIND_MESSAGE.get() => {
                on_find(hwnd, lparam);
                return LRESULT(0);
            }
            _ => {}
        }

        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Window;
        if pointer.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let window = &mut *pointer;
        match message {
            WM_CREATE => {
                if let Ok(bar) = build_menu() {
                    let _ = SetMenu(hwnd, Some(bar));
                }
                window.fonts = Fonts::new(GetDpiForWindow(hwnd).max(96));
                window.apply_theme(hwnd);
                window.refresh(hwnd);
                LRESULT(0)
            }
            // The observer asked for a repaint. Cleared before refreshing, so a change landing
            // in between can post again.
            WM_CORE_CHANGED => {
                window.pending.store(false, Ordering::Release);
                window.refresh(hwnd);
                LRESULT(0)
            }
            WM_INITMENUPOPUP => {
                let popup = HMENU(wparam.0 as *mut c_void);
                let selected = window.app.selection().is_some();
                for (command, enabled) in [
                    (Command::Undo, window.app.can_undo()),
                    (Command::Redo, window.app.can_redo()),
                    (Command::Cut, selected),
                    (Command::Copy, selected),
                ] {
                    let state = if enabled { MF_ENABLED } else { MF_GRAYED };
                    let _ = EnableMenuItem(popup, u32::from(menu::id(command)), state);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                window.paint(hwnd);
                LRESULT(0)
            }
            WM_SIZE => {
                window.refresh(hwnd);
                LRESULT(0)
            }
            WM_CHAR => {
                if let Some(c) = window.utf16.push(wparam.0 as u16).and_then(keymap::typed) {
                    window.message.clear();
                    window.app.type_text(c.encode_utf8(&mut [0; 4]));
                    window.after_input(hwnd);
                }
                LRESULT(0)
            }
            WM_SETFOCUS => {
                let height = window.with_frame(hwnd, |frame, _| frame.metrics.line_height());
                let width = crate::theme::scale(2, window.fonts.dpi);
                let _ = CreateCaret(hwnd, None, width, height as i32);
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_KILLFOCUS => {
                let _ = DestroyCaret();
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let _ = SetFocus(Some(hwnd));
                let _ = SetCapture(hwnd);
                window.place(hwnd, lparam, modifiers().shift);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if wparam.0 & 0x0001 != 0 {
                    window.place(hwnd, lparam, true);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
                let rows = wheel_rows(delta, wheel_lines(), &mut window.wheel);
                if rows != 0 {
                    window.with_frame(hwnd, |frame, _| window.app.scroll_by(frame, rows));
                }
                LRESULT(0)
            }
            WM_VSCROLL => {
                let rows = window.with_frame(hwnd, |frame, _| frame.rows() as i64);
                let by = match SCROLLBAR_COMMAND((wparam.0 & 0xFFFF) as i32) {
                    SB_LINEUP => Some(-1),
                    SB_LINEDOWN => Some(1),
                    SB_PAGEUP => Some(-rows),
                    SB_PAGEDOWN => Some(rows),
                    SB_THUMBTRACK | SB_THUMBPOSITION => {
                        let mut info = SCROLLINFO {
                            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                            fMask: SIF_TRACKPOS,
                            ..Default::default()
                        };
                        let _ = GetScrollInfo(hwnd, SB_VERT, &mut info);
                        window.app.scroll_to(info.nTrackPos.max(0) as u64);
                        None
                    }
                    _ => None,
                };
                if let Some(by) = by {
                    window.with_frame(hwnd, |frame, _| window.app.scroll_by(frame, by));
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                window.fonts = Fonts::new((wparam.0 & 0xFFFF) as u32);
                let suggested = lparam.0 as *const RECT;
                if !suggested.is_null() {
                    let r = *suggested;
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                window.refresh(hwnd);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                window.apply_theme(hwnd);
                window.refresh(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            // The last message a window receives, and the only safe place to free what
            // WM_NCCREATE stored.
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(pointer));
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

/// Open the window and run the message loop until it closes.
pub fn run_window(app: Arc<App>) -> windows::core::Result<()> {
    unsafe {
        // The manifest asks for this too; asking here as well covers a binary run without one.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        FIND_MESSAGE.set(RegisterWindowMessageW(FINDMSGSTRING));

        let instance = GetModuleHandleW(None)?;
        let class_name = w!("QuillWindow");
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hIcon: LoadIconW(Some(instance.into()), resource(1)).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_IBEAM)?,
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let pending = Arc::new(AtomicBool::new(false));
        // Owned by the window from WM_NCCREATE to WM_NCDESTROY.
        let state = Box::into_raw(Box::new(Window {
            app: app.clone(),
            fonts: Fonts::new(96),
            theme: Theme::light(),
            message: String::new(),
            title: String::new(),
            pending: pending.clone(),
            utf16: Utf16::default(),
            wheel: 0,
            find: Box::default(),
            find_text: Box::new([0; 256]),
        }));
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Quill"),
            WS_OVERLAPPEDWINDOW | WS_VSCROLL,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            900,
            640,
            None,
            None,
            Some(instance.into()),
            Some(state as *const c_void),
        )?;
        // Only now can the observer be wired: it needs a window to post to.
        app.observe(Arc::new(Notifier {
            hwnd: hwnd.0 as isize,
            pending,
        }));
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).into() {
            // The modeless Find dialog gets its own keys — Tab, Enter, Escape.
            let find = FIND.get();
            if find != 0 && IsDialogMessageW(HWND(find as *mut c_void), &message).as_bool() {
                continue;
            }
            // What turns WM_KEYDOWN into WM_CHAR. Without it nothing types.
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}
