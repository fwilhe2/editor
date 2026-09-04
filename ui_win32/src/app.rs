//! The window: one `HWND`, one message loop, and nothing that owns a document.
//!
//! This is the Windows half of the shell and the only file here that needs Windows
//! at all. `keymap.rs` and `layout.rs` carry every decision that can be made without
//! a window, which is why they are testable on any host and this file is not.
//!
//! Two rules do most of the work, and they are the same two every other GUI shell in
//! this repository follows:
//!
//! - **The window is a renderer, not the document.** There is no `EDIT` control and
//!   no rich-edit control anywhere — they own their own text, exactly as
//!   `GtkTextView`, WinUI's `TextBox` and `contenteditable` do, and the moment one
//!   exists there are two sources of truth. Every paint fills the client area from
//!   `get_viewport` and puts the caret where the core says the cursor is.
//! - **`follow_cursor` is called from input handling and nowhere else.** If painting
//!   called it, a wheel scroll away from the caret would snap straight back on the
//!   next `WM_PAINT`.
//!
//! Everything below draws with GDI, which is in Windows itself. See
//! `doc/decision-win32-shell.md` for why that rather than Direct2D, and for what
//! this shell gives up by not being WinUI.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use editor_core::{Editor, EditorObserver};
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, SystemParametersInfoForDpi,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::keymap::{self, Modifiers};
use crate::layout::{self, Metrics};

/// Posted by the observer when the core changed something.
///
/// `WM_APP` is the first value Windows reserves for an application's own messages,
/// so this cannot collide with anything the system sends.
const WM_CORE_CHANGED: u32 = WM_APP + 1;

/// The document font's size in points, before DPI scaling.
const FONT_POINTS: i32 = 11;

/// How wide the caret is drawn, in pixels at 96 DPI.
const CARET_WIDTH: i32 = 2;

/// Padding around the status bar's text, in pixels at 96 DPI.
const STATUS_PADDING: i32 = 6;

/// The reference DPI every Windows metric is expressed against.
const USER_DEFAULT_SCREEN_DPI: i32 = 96;

/// What a document with no path is called.
const UNTITLED: &str = "Untitled";

/// Menu command identifiers.
///
/// Every one of these maps onto a [`keymap::UiAction`] that a keystroke already
/// produces, so the menu is a second way to reach the same code rather than a second
/// implementation of it. Nothing here is a capability the CLI lacks.
const IDM_SAVE: usize = 101;
const IDM_EXIT: usize = 102;
const IDM_UNDO: usize = 103;
const IDM_REDO: usize = 104;

/// Colours, chosen per theme rather than taken from `GetSysColor`.
///
/// The classic system colours never went dark — `COLOR_WINDOW` is white on a
/// dark-themed Windows 11 — so reading them would produce a white editor next to a
/// dark title bar. These are Windows' own dark surface values instead.
struct Theme {
    background: COLORREF,
    text: COLORREF,
    status_background: COLORREF,
    status_text: COLORREF,
}

impl Theme {
    fn light() -> Self {
        Theme {
            background: rgb(0xFF, 0xFF, 0xFF),
            text: rgb(0x1A, 0x1A, 0x1A),
            status_background: rgb(0xF3, 0xF3, 0xF3),
            status_text: rgb(0x44, 0x44, 0x44),
        }
    }

    fn dark() -> Self {
        Theme {
            background: rgb(0x20, 0x20, 0x20),
            text: rgb(0xE4, 0xE4, 0xE4),
            status_background: rgb(0x2B, 0x2B, 0x2B),
            status_text: rgb(0xB0, 0xB0, 0xB0),
        }
    }
}

/// GDI's byte order is `0x00BBGGRR`, not the `0xRRGGBB` everyone writes by hand.
const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

/// Everything the window needs, and no editor state.
///
/// `message` and `quit_warned` look like exceptions and are not: neither is anything
/// the core could know. They are the same two pieces of presentation state the TUI
/// and egui shells keep.
pub struct App {
    editor: Arc<Editor>,
    /// The pixel grid, re-measured whenever the font is rebuilt.
    metrics: Metrics,
    /// The document's fixed-pitch font.
    font: HFONT,
    /// The shell font, for the status bar. Windows expects UI chrome in the user's
    /// UI font, not in the document's.
    ui_font: HFONT,
    /// Current scaling, so the fonts can be rebuilt when the window changes monitor.
    dpi: u32,
    theme: Theme,
    /// The last thing the shell has to say, shown in the status bar.
    message: String,
    /// The title last handed to the window, so an unchanged one is not re-sent.
    title: String,
    /// Set while the close dialog is up, so re-entering it is impossible.
    closing: bool,
    /// Raised by the observer, cleared when the repaint it asked for is requested.
    /// This is what turns a burst of notifications into one paint.
    pending: Arc<AtomicBool>,
}

impl App {
    fn new(editor: Arc<Editor>, pending: Arc<AtomicBool>, dpi: u32) -> Self {
        App {
            editor,
            metrics: Metrics::new(0, 0),
            font: HFONT::default(),
            ui_font: HFONT::default(),
            dpi,
            theme: Theme::light(),
            message: String::new(),
            title: String::new(),
            closing: false,
            pending,
        }
    }

    /// What the open document is called.
    fn document_name(&self) -> String {
        self.editor
            .path()
            .map(|path| path.display().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| UNTITLED.to_string())
    }

    /// The document's name with Windows' unsaved-changes marker.
    ///
    /// A trailing asterisk, which is the Windows convention — not the leading dot the
    /// portable shell picked, and not macOS' proxy-icon dot.
    fn window_title(&self) -> String {
        let name = self.document_name();
        if self.editor.is_dirty() {
            format!("*{name} — Editor")
        } else {
            format!("{name} — Editor")
        }
    }

    /// Build the fonts for the current DPI and measure the character grid.
    ///
    /// Called at creation and again on every `WM_DPICHANGED`: a window dragged to a
    /// monitor with different scaling needs a different pixel grid, and nothing else
    /// announces that. The same rule as the browser shell's re-measured `#probe`.
    fn rebuild_fonts(&mut self, hwnd: HWND) {
        unsafe {
            if !self.font.is_invalid() {
                let _ = DeleteObject(self.font.into());
            }
            if !self.ui_font.is_invalid() {
                let _ = DeleteObject(self.ui_font.into());
            }

            // A negative height asks for that many pixels of *character* height,
            // which is what a point size maps onto.
            let height = -(FONT_POINTS * self.dpi as i32 / 72);
            let mut logfont = LOGFONTW {
                lfHeight: height,
                lfWeight: FW_NORMAL.0 as i32,
                lfCharSet: DEFAULT_CHARSET,
                lfOutPrecision: OUT_TT_PRECIS,
                lfQuality: CLEARTYPE_QUALITY,
                // FIXED_PITCH matters more than the face name: if Consolas is
                // missing, GDI substitutes another fixed-pitch modern face rather
                // than a proportional one, and the character grid survives.
                lfPitchAndFamily: FIXED_PITCH.0 | FF_MODERN.0,
                ..Default::default()
            };
            let face: Vec<u16> = "Consolas\0".encode_utf16().collect();
            logfont.lfFaceName[..face.len()].copy_from_slice(&face);
            self.font = CreateFontIndirectW(&logfont);

            // The status bar goes in whatever the user's shell font is, at this
            // window's DPI. Asking the system beats hard-coding "Segoe UI".
            let mut metrics = NONCLIENTMETRICSW {
                cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
                ..Default::default()
            };
            let ok = SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS.0,
                std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
                Some(&mut metrics as *mut _ as *mut c_void),
                0,
                self.dpi,
            )
            .is_ok();
            self.ui_font = if ok {
                CreateFontIndirectW(&metrics.lfMessageFont)
            } else {
                // Better a readable window than none: the document font will do.
                CreateFontIndirectW(&logfont)
            };

            self.measure(hwnd);
        }
    }

    /// Measure one character of the document font.
    ///
    /// `tmAveCharWidth` is the average width, which for a fixed-pitch font is *the*
    /// width — the one case where that metric means what it says.
    fn measure(&mut self, hwnd: HWND) {
        unsafe {
            let hdc = GetDC(Some(hwnd));
            if hdc.is_invalid() {
                return;
            }
            let previous = SelectObject(hdc, self.font.into());
            let mut text_metrics = TEXTMETRICW::default();
            if GetTextMetricsW(hdc, &mut text_metrics).as_bool() {
                self.metrics = Metrics::new(
                    text_metrics.tmAveCharWidth,
                    text_metrics.tmHeight + text_metrics.tmExternalLeading,
                );
            }
            SelectObject(hdc, previous);
            ReleaseDC(Some(hwnd), hdc);
        }
    }

    /// Scale a pixel constant written at 96 DPI to this window's scaling.
    fn scale(&self, value: i32) -> i32 {
        (value * self.dpi as i32 / USER_DEFAULT_SCREEN_DPI).max(1)
    }

    /// How tall the status bar is.
    fn status_height(&self) -> i32 {
        self.metrics.line_height + 2 * self.scale(STATUS_PADDING)
    }

    /// The part of the client area the document is drawn in.
    fn document_rect(&self, hwnd: HWND) -> RECT {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rect);
        }
        rect.bottom = (rect.bottom - self.status_height()).max(rect.top);
        rect
    }

    /// How many document lines fit in the window.
    fn visible_lines(&self, hwnd: HWND) -> u64 {
        let rect = self.document_rect(hwnd);
        self.metrics.visible_lines(rect.bottom - rect.top)
    }

    /// Follow the user's light/dark choice, including the title bar.
    ///
    /// `DWMWA_USE_IMMERSIVE_DARK_MODE` is the one piece of Windows 11 chrome a plain
    /// GDI window can still have — a light title bar over a dark document is the
    /// giveaway of an application that ignored the setting.
    fn apply_theme(&mut self, hwnd: HWND) {
        let dark = prefers_dark();
        self.theme = if dark { Theme::dark() } else { Theme::light() };
        unsafe {
            let flag = BOOL::from(dark);
            // Older builds do not know the attribute and return an error, which is
            // the correct outcome there: no dark title bar, and nothing broken.
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &flag as *const _ as *const c_void,
                std::mem::size_of::<BOOL>() as u32,
            );
        }
    }

    /// Paint the whole client area.
    ///
    /// Drawn into a memory bitmap and blitted once, because painting text straight
    /// onto the window flickers visibly on every keystroke. `WM_ERASEBKGND` is
    /// answered rather than left to the class brush for the same reason.
    ///
    /// Renders from the core's stored `scroll_offset` and deliberately never calls
    /// `follow_cursor` — that belongs to input handling.
    fn paint(&mut self, hwnd: HWND) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            // BeginPaint hides the caret for the duration, so nothing here has to.
            let hdc = BeginPaint(hwnd, &mut ps);
            if hdc.is_invalid() {
                return;
            }

            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let width = client.right - client.left;
            let height = client.bottom - client.top;

            let memory_dc = CreateCompatibleDC(Some(hdc));
            let bitmap = CreateCompatibleBitmap(hdc, width, height);
            let old_bitmap = SelectObject(memory_dc, bitmap.into());

            self.draw(memory_dc, client);

            let _ = BitBlt(hdc, 0, 0, width, height, Some(memory_dc), 0, 0, SRCCOPY);

            SelectObject(memory_dc, old_bitmap);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory_dc);
            let _ = EndPaint(hwnd, &ps);
        }

        self.place_caret(hwnd);
    }

    /// Everything on screen, onto whatever device context it is given.
    fn draw(&self, hdc: HDC, client: RECT) {
        let status_height = self.status_height();
        let mut document = client;
        document.bottom = (client.bottom - status_height).max(client.top);

        unsafe {
            let background = CreateSolidBrush(self.theme.background);
            FillRect(hdc, &document, background);
            let _ = DeleteObject(background.into());

            let mut status = client;
            status.top = document.bottom;
            let status_brush = CreateSolidBrush(self.theme.status_background);
            FillRect(hdc, &status, status_brush);
            let _ = DeleteObject(status_brush.into());

            SetBkMode(hdc, TRANSPARENT);
            self.draw_document(hdc, document);
            self.draw_status(hdc, status);
        }
    }

    /// Draw the lines the core handed back, and nothing else.
    ///
    /// Only the visible slice is asked for and only the visible slice is drawn, so
    /// the cost of a paint is bounded by the window rather than by the file — which
    /// is the whole reason `get_viewport` exists.
    fn draw_document(&self, hdc: HDC, rect: RECT) {
        let height = self.metrics.visible_lines(rect.bottom - rect.top);
        let start = self.editor.scroll_offset();
        let viewport = self
            .editor
            .get_viewport(start, start.saturating_add(height));

        unsafe {
            let previous = SelectObject(hdc, self.font.into());
            SetTextColor(hdc, self.theme.text);

            for (row, line) in viewport.lines.iter().enumerate() {
                let (x, y) = self.metrics.caret_offset(row as u64, 0);
                let (text, advances) = self.grid_run(line);
                if text.is_empty() {
                    continue;
                }
                // ExtTextOutW with an explicit advance per glyph forces the exact
                // character grid the caret arithmetic assumes, whatever the font
                // would have done on its own.
                let _ = ExtTextOutW(
                    hdc,
                    rect.left + x,
                    rect.top + y,
                    ETO_OPTIONS(0),
                    None,
                    PCWSTR(text.as_ptr()),
                    text.len() as u32,
                    Some(advances.as_ptr()),
                );
            }

            SelectObject(hdc, previous);
        }
    }

    /// One line as UTF-16, with a fixed advance for every unit.
    ///
    /// Tabs become a single space: the core counts a tab as one character, so
    /// anything wider would put the caret in the wrong place. The document keeps its
    /// tab; only the drawing is a space.
    ///
    /// A character outside the basic multilingual plane is two UTF-16 units and so
    /// takes two cells here while the core counts it as one. Wrong, rare, and the
    /// same class of gap as the CRLF handling in `EditorState::backspace`.
    fn grid_run(&self, line: &str) -> (Vec<u16>, Vec<i32>) {
        let mut text: Vec<u16> = Vec::with_capacity(line.len());
        for ch in line.chars() {
            let ch = if ch == '\t' { ' ' } else { ch };
            let mut buffer = [0u16; 2];
            text.extend_from_slice(ch.encode_utf16(&mut buffer));
        }
        let advances = vec![self.metrics.char_width; text.len()];
        (text, advances)
    }

    /// The status bar: what the document is, and where the cursor is in it.
    fn draw_status(&self, hdc: HDC, rect: RECT) {
        let cursor = self.editor.cursor();
        let left = if self.message.is_empty() {
            self.window_title()
        } else {
            self.message.clone()
        };
        // +1 on both: the core is 0-based and stays that way, and this is the only
        // place in this shell that knows it.
        let right = format!(
            "Ln {}, Col {}    {} lines    {} chars",
            cursor.line + 1,
            cursor.column + 1,
            self.editor.line_count(),
            self.editor.char_count(),
        );

        let padding = self.scale(STATUS_PADDING);
        let mut inner = rect;
        inner.left += padding;
        inner.right -= padding;

        unsafe {
            let previous = SelectObject(hdc, self.ui_font.into());
            SetTextColor(hdc, self.theme.status_text);

            let mut text: Vec<u16> = left.encode_utf16().collect();
            let mut area = inner;
            DrawTextW(
                hdc,
                &mut text,
                &mut area,
                DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS | DT_NOPREFIX,
            );

            let mut text: Vec<u16> = right.encode_utf16().collect();
            let mut area = inner;
            DrawTextW(
                hdc,
                &mut text,
                &mut area,
                DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_NOPREFIX,
            );

            SelectObject(hdc, previous);
        }
    }

    /// Put the system caret where the core says the cursor is.
    ///
    /// A real `CreateCaret` caret rather than a painted rectangle, which buys three
    /// things for free: the user's blink rate, the user's caret width setting, and a
    /// position that Windows reports to assistive technology and to IMEs.
    fn place_caret(&self, hwnd: HWND) {
        let rect = self.document_rect(hwnd);
        let height = self.metrics.visible_lines(rect.bottom - rect.top);
        let start = self.editor.scroll_offset();
        let cursor = self.editor.cursor();

        unsafe {
            if cursor.line < start || cursor.line >= start.saturating_add(height) {
                // Scrolled out of view: there is nowhere honest to draw it.
                let _ = HideCaret(Some(hwnd));
                return;
            }
            let (x, y) = self
                .metrics
                .caret_offset(cursor.line - start, cursor.column);
            let _ = SetCaretPos(rect.left + x, rect.top + y);
            let _ = ShowCaret(Some(hwnd));
        }
    }

    /// Keep the window's title in step with the document.
    fn sync_title(&mut self, hwnd: HWND) {
        let title = self.window_title();
        if title == self.title {
            return;
        }
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
        }
        self.title = title;
    }

    /// Redraw, and refresh everything that hangs off the document's state.
    fn refresh(&mut self, hwnd: HWND) {
        self.sync_title(hwnd);
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }

    /// Write the document back to the file it came from.
    fn save(&mut self) -> bool {
        match self.editor.save_file() {
            Ok(()) => {
                self.message = format!("Saved {}", self.document_name());
                true
            }
            Err(error) => {
                self.message = format!("{error}");
                false
            }
        }
    }

    /// Handle a key, and scroll to wherever it left the cursor.
    fn on_action(&mut self, hwnd: HWND, action: keymap::UiAction) {
        self.message.clear();
        if let Some(keymap::Request::Save) = keymap::apply(action, &self.editor) {
            self.save();
        }
        // Only after a key. A wheel scroll must be allowed to leave the caret behind,
        // which is why this is not in `paint`.
        self.editor.follow_cursor(self.visible_lines(hwnd));
        self.refresh(hwnd);
    }

    /// What the close dialog should ask, or `None` if there is nothing to ask.
    ///
    /// Deliberately *not* the function that shows the dialog. See [`on_close`] — the
    /// borrow of the `App` has to end before the modal loop starts.
    fn close_question(&mut self) -> Option<Vec<u16>> {
        if self.closing || !self.editor.is_dirty() {
            return None;
        }
        self.closing = true;
        let text = format!("Do you want to save changes to {}?\0", self.document_name());
        Some(text.encode_utf16().collect())
    }

    /// Act on the dialog's answer, and say whether the window may close.
    fn close_answer(&mut self, answer: MESSAGEBOX_RESULT) -> bool {
        self.closing = false;
        match answer {
            // A failed save must not take the document down with it.
            IDYES => self.save(),
            IDNO => true,
            _ => false,
        }
    }
}

/// Handle `WM_CLOSE`: the close button, Alt+F4, and the taskbar's Close.
///
/// Windows' convention is a three-button confirmation with Save as the default,
/// which is what the WinUI shell put in a `ContentDialog`. `MessageBoxW` is the same
/// conversation in a dialog the operating system draws — a `TaskDialog` would look
/// more modern but needs a Common Controls v6 manifest, and this binary deliberately
/// has no manifest at all.
///
/// **This is a free function, and the borrows below are scoped by hand, on purpose.**
/// `MessageBoxW` runs its own message loop: while the dialog is up, this window still
/// receives `WM_PAINT` and the window procedure is re-entered, which calls
/// [`app_from`] and produces a *second* `&mut App` while the first is alive. That is
/// aliasing UB even though it appears to work. So the `App` is borrowed twice, for as
/// long as it takes to read a question and to act on an answer, and never across the
/// dialog itself. Any future handler that opens a modal dialog — a file picker, a
/// find bar — must follow this shape.
///
/// Found by running the shell under Wine; it is not visible from reading the code.
unsafe fn on_close(hwnd: HWND) -> LRESULT {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }

    // First borrow: what to ask. Ends at the closing brace.
    let question = unsafe { (*pointer).close_question() };

    let may_close = match question {
        None => true,
        Some(text) => {
            // No borrow of the App is alive here, which is the whole point.
            let answer = unsafe {
                MessageBoxW(
                    Some(hwnd),
                    PCWSTR(text.as_ptr()),
                    w!("Editor"),
                    MB_YESNOCANCEL | MB_ICONWARNING,
                )
            };
            // Second borrow.
            unsafe { (*pointer).close_answer(answer) }
        }
    };

    if may_close {
        let _ = unsafe { DestroyWindow(hwnd) };
    }
    LRESULT(0)
}

impl Drop for App {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_invalid() {
                let _ = DeleteObject(self.font.into());
            }
            if !self.ui_font.is_invalid() {
                let _ = DeleteObject(self.ui_font.into());
            }
        }
    }
}

/// Turns a change in the core into a repaint request.
///
/// The fifth observer bridge in this project and the only one that goes through the
/// operating system's own queue. `HWND` is a raw pointer and therefore not `Send`,
/// while `EditorObserver` must be `Send + Sync`, so the handle is carried as an
/// `isize` and used only with `PostMessageW` — which Microsoft documents as callable
/// from any thread, unlike almost everything else here.
///
/// The `AtomicBool` is what coalesces a burst into one paint: the same job the
/// `async_channel` drain does in the GTK shell and the flag does in the browser one.
pub struct Notifier {
    hwnd: isize,
    pending: Arc<AtomicBool>,
}

impl Notifier {
    pub fn new(hwnd: HWND, pending: Arc<AtomicBool>) -> Self {
        Notifier {
            hwnd: hwnd.0 as isize,
            pending,
        }
    }
}

impl EditorObserver for Notifier {
    fn state_changed(&self) {
        // Already asked, not yet painted: one message is enough.
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

/// Build the menu bar.
///
/// A menu rather than a toolbar, deliberately: a Windows text editor of this size has
/// a menu bar — Notepad still does — and a toolbar would mean the Common Controls v6
/// toolbar class, which needs the manifest this binary does not have. The system
/// draws it, so it scales with DPI, follows the user's theme and gains Alt-key
/// navigation without any code here.
///
/// The accelerators after each tab are labels only. The keys themselves are handled
/// in `keymap.rs`, which is what makes them testable; this just tells the user what
/// they are, as every Windows menu does.
fn build_menu() -> windows::core::Result<HMENU> {
    unsafe {
        let file = CreatePopupMenu()?;
        AppendMenuW(file, MF_STRING, IDM_SAVE, w!("&Save\tCtrl+S"))?;
        AppendMenuW(file, MF_SEPARATOR, 0, PCWSTR::null())?;
        AppendMenuW(file, MF_STRING, IDM_EXIT, w!("E&xit\tAlt+F4"))?;

        let edit = CreatePopupMenu()?;
        AppendMenuW(edit, MF_STRING, IDM_UNDO, w!("&Undo\tCtrl+Z"))?;
        AppendMenuW(edit, MF_STRING, IDM_REDO, w!("&Redo\tCtrl+Y"))?;

        let bar = CreateMenu()?;
        AppendMenuW(bar, MF_POPUP, file.0 as usize, w!("&File"))?;
        AppendMenuW(bar, MF_POPUP, edit.0 as usize, w!("&Edit"))?;
        Ok(bar)
    }
}

/// Is the user's shell in dark mode?
///
/// There is no API for this that works in an unpackaged Win32 process, so the
/// registry value the Settings app writes is the supported answer. Absent means
/// light, which is what Windows itself assumes.
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

/// Which modifiers are down right now.
///
/// Read at the moment the message is handled rather than decoded from `lParam`,
/// because `WM_KEYDOWN` does not carry Ctrl or Shift at all.
fn modifiers() -> Modifiers {
    unsafe {
        Modifiers {
            ctrl: GetKeyState(VK_CONTROL.0 as i32) < 0,
            shift: GetKeyState(VK_SHIFT.0 as i32) < 0,
            alt: GetKeyState(VK_MENU.0 as i32) < 0,
        }
    }
}

/// The high word of `wParam`, as a signed wheel delta.
fn wheel_delta(wparam: WPARAM) -> i32 {
    ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32
}

/// How many lines the user's mouse settings say one notch moves.
fn wheel_scroll_lines() -> u32 {
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

/// Retrieve the `App` this window was created with.
///
/// Stored in `GWLP_USERDATA` at `WM_NCCREATE` and freed at `WM_NCDESTROY`, which is
/// the standard Win32 arrangement for giving a window procedure some state.
unsafe fn app_from(hwnd: HWND) -> Option<&'static mut App> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    if pointer.is_null() {
        None
    } else {
        Some(unsafe { &mut *pointer })
    }
}

extern "system" fn wndproc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        // Before `app_from`, deliberately: this one opens a modal dialog and must
        // not hold a borrow across it. See `on_close`.
        if message == WM_CLOSE {
            return on_close(hwnd);
        }

        if message == WM_NCCREATE {
            let create = lparam.0 as *const CREATESTRUCTW;
            let app = (*create).lpCreateParams as *mut App;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }

        let Some(app) = app_from(hwnd) else {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        };

        match message {
            WM_CREATE => {
                // The real scaling of the monitor this window opened on. Guessing
                // 96 and correcting at the first WM_DPICHANGED would mean a window
                // that starts blurry on every scaled display.
                app.dpi = GetDpiForWindow(hwnd).max(1);
                if let Ok(menu) = build_menu() {
                    let _ = SetMenu(hwnd, Some(menu));
                }
                app.apply_theme(hwnd);
                app.rebuild_fonts(hwnd);
                app.sync_title(hwnd);
                LRESULT(0)
            }

            // The observer asked for a repaint. Clearing the flag here rather than
            // after painting is deliberate: a change that lands between now and the
            // paint must be able to post again.
            WM_CORE_CHANGED => {
                app.pending.store(false, Ordering::Release);
                app.refresh(hwnd);
                LRESULT(0)
            }

            // A menu item was chosen. Each one routes through the same
            // `keymap::UiAction` a keystroke produces, so there is one code path for
            // both and no way for the two to drift.
            WM_COMMAND => {
                match wparam.0 & 0xFFFF {
                    IDM_SAVE => app.on_action(hwnd, keymap::UiAction::Save),
                    IDM_UNDO => app.on_action(hwnd, keymap::UiAction::Undo),
                    IDM_REDO => app.on_action(hwnd, keymap::UiAction::Redo),
                    // Routed rather than closed directly, so Exit gets the same
                    // unsaved-changes dialog as the close button.
                    IDM_EXIT => {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    _ => return DefWindowProcW(hwnd, message, wparam, lparam),
                }
                LRESULT(0)
            }

            // Grey out what the core says is unavailable, just before the menu is
            // shown. The WinUI shell did this by binding `IsEnabled` to `CanUndo`;
            // this is the same idea at the point Windows asks for it.
            WM_INITMENUPOPUP => {
                let popup = HMENU(wparam.0 as *mut c_void);
                let state = |available: bool| {
                    if available {
                        MF_ENABLED
                    } else {
                        MF_GRAYED
                    }
                };
                let _ = EnableMenuItem(popup, IDM_SAVE as u32, state(app.editor.is_dirty()));
                let _ = EnableMenuItem(popup, IDM_UNDO as u32, state(app.editor.can_undo()));
                let _ = EnableMenuItem(popup, IDM_REDO as u32, state(app.editor.can_redo()));
                LRESULT(0)
            }

            // Everything is drawn in WM_PAINT, so erasing first would only flicker.
            WM_ERASEBKGND => LRESULT(1),

            WM_PAINT => {
                app.paint(hwnd);
                LRESULT(0)
            }

            WM_SIZE => {
                // A taller window shows more lines, which changes the viewport.
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }

            WM_SETFOCUS => {
                let width = app.scale(CARET_WIDTH);
                let _ = CreateCaret(hwnd, None, width, app.metrics.line_height);
                app.place_caret(hwnd);
                LRESULT(0)
            }

            WM_KILLFOCUS => {
                let _ = DestroyCaret();
                LRESULT(0)
            }

            // Clicking is the only way to place the caret with the mouse, and it
            // costs the architecture nothing: `set_cursor` is core API.
            WM_LBUTTONDOWN => {
                let x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
                let rect = app.document_rect(hwnd);
                if y < rect.bottom {
                    let position = app.metrics.position_at(
                        x - rect.left,
                        y - rect.top,
                        app.editor.scroll_offset(),
                    );
                    app.editor.set_cursor(position);
                    app.refresh(hwnd);
                }
                let _ = SetFocus(Some(hwnd));
                LRESULT(0)
            }

            WM_MOUSEWHEEL => {
                let step = layout::wheel_step(wheel_scroll_lines(), app.visible_lines(hwnd));
                let delta = layout::wheel_lines(wheel_delta(wparam), step);
                if delta != 0 {
                    // Straight into the core's scroll offset, so this shell scrolls
                    // by the same rule as every other one. No `follow_cursor` here:
                    // the view is allowed to leave the caret behind.
                    app.editor
                        .set_scroll_offset(layout::scrolled_by(app.editor.scroll_offset(), delta));
                    app.refresh(hwnd);
                }
                LRESULT(0)
            }

            WM_KEYDOWN | WM_SYSKEYDOWN => {
                match keymap::action_for_key(wparam.0 as u16, modifiers()) {
                    Some(action) => {
                        app.on_action(hwnd, action);
                        LRESULT(0)
                    }
                    // Alt+F4 lives down here. Handling it ourselves would make the
                    // window unclosable from the keyboard.
                    None => DefWindowProcW(hwnd, message, wparam, lparam),
                }
            }

            WM_CHAR => {
                let Some(ch) = char::from_u32(wparam.0 as u32) else {
                    return LRESULT(0);
                };
                match keymap::action_for_char(ch, modifiers()) {
                    Some(action) => {
                        app.on_action(hwnd, action);
                        LRESULT(0)
                    }
                    None => LRESULT(0),
                }
            }

            // Dragged to a monitor with different scaling: new fonts, new grid, and
            // the size Windows suggests.
            WM_DPICHANGED => {
                app.dpi = (wparam.0 & 0xFFFF) as u32;
                app.rebuild_fonts(hwnd);
                let suggested = lparam.0 as *const RECT;
                if !suggested.is_null() {
                    let rect = *suggested;
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }

            // The user switched between light and dark while the window was open.
            WM_SETTINGCHANGE => {
                app.apply_theme(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }

            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }

            // The last message a window receives, and the only safe place to free
            // what WM_NCCREATE stored.
            WM_NCDESTROY => {
                let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !pointer.is_null() {
                    drop(Box::from_raw(pointer));
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }

            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

/// Open the window and run the message loop until it closes.
pub fn run(editor: Arc<Editor>) -> windows::core::Result<()> {
    unsafe {
        // Before any window exists, and without a manifest: this is what makes the
        // window sharp on a scaled display and what makes WM_DPICHANGED arrive.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let instance = GetModuleHandleW(None)?;
        let class_name = w!("EditorWin32Window");

        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            // An I-beam over a text surface, as every Windows editor has.
            hCursor: LoadCursorW(None, IDC_IBEAM)?,
            // No class brush: WM_ERASEBKGND is answered and WM_PAINT fills every
            // pixel, so a brush here would only paint the window twice.
            hbrBackground: HBRUSH::default(),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let pending = Arc::new(AtomicBool::new(false));
        // Boxed and handed to the window, which owns it from WM_NCCREATE until
        // WM_NCDESTROY.
        let app = Box::into_raw(Box::new(App::new(
            editor.clone(),
            pending.clone(),
            USER_DEFAULT_SCREEN_DPI as u32,
        )));

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Editor"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            900,
            640,
            None,
            None,
            Some(instance.into()),
            Some(app as *const c_void),
        )?;

        // Only now can the observer be wired: it needs the window to post to.
        editor.set_observer(Arc::new(Notifier::new(hwnd, pending)));

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).into() {
            // What turns WM_KEYDOWN into WM_CHAR. Without it nothing types.
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}
