// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! `quill-web` — the editor in a browser tab.
//!
//! Rust compiled to `wasm32-unknown-unknown` and bound to the page with `wasm-bindgen`: the
//! crate depends on `quill-core` as an ordinary dependency, so this shell has no FFI facade
//! and no generated bindings beyond the JavaScript glue that loads the module.
//!
//! **The DOM is a renderer, not the document.** There is no `contenteditable` anywhere: each
//! frame rebuilds the visible rows from [`App::view`], and typing reaches the core through a
//! hidden `<textarea>` that is emptied on every `input` event. A `contenteditable` would be a
//! second copy of the text, edited by the browser by rules of its own.
//!
//! **The browser has no filesystem**, which is why every `*_file` method in the core has a
//! `*_bytes` twin: a file arrives from the picker as bytes and leaves as a download.
//!
//! **A page, not a window.** A browser tab has no menu bar and inventing one makes a worse
//! desktop application, so there is one toolbar of common verbs and **Ctrl+K**, a palette over
//! every [`Command`] (`doc/shells/web.md`).

mod keymap;
mod metrics;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use quill_core::layout::{self, Metrics};
use quill_core::{App, Command, Frame, Observer, Search, command};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    ClipboardEvent, Document, Event, HtmlElement, HtmlInputElement, HtmlTextAreaElement,
    InputEvent, KeyboardEvent, MouseEvent, WheelEvent,
};

use keymap::{Action, Chord};
use metrics::CanvasMetrics;

/// What a document with no name is downloaded as.
const DOWNLOAD_NAME: &str = "untitled.txt";

thread_local! {
    /// The live shell, so an animation-frame callback can find its way back to it. The page
    /// owns it until the tab closes.
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

/// Called by the generated glue as soon as the module is instantiated.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let app = Arc::new(App::new());
    let ui = Rc::new(Ui {
        app: app.clone(),
        dom: Dom::find(&document)?,
        pending: Arc::new(AtomicBool::new(false)),
        message: RefCell::new("Open a file, or start typing. Ctrl+K lists every command.".into()),
        dragging: Cell::new(false),
        wheel: Cell::new(0.0),
        selected: Cell::new(0),
        mac: window
            .navigator()
            .platform()
            .is_ok_and(|p| p.starts_with("Mac")),
    });
    app.observe(Arc::new(Notifier(ui.pending.clone())));

    wire_keyboard(&ui)?;
    wire_clipboard(&ui)?;
    wire_pointer(&ui)?;
    wire_toolbar(&ui)?;
    wire_find(&ui)?;
    wire_palette(&ui)?;
    wire_files(&ui)?;
    wire_window(&ui, &window)?;

    UI.with(|slot| *slot.borrow_mut() = Some(ui.clone()));
    ui.dom.input.focus()?;
    ui.refresh();
    Ok(())
}

/// Raises a repaint when the core changes, and schedules the frame that draws it.
///
/// [`Observer`] is `Send + Sync`, and the page is neither — a wasm module is single-threaded,
/// but the trait does not know that. So this holds a flag, which doubles as "a frame is already
/// scheduled": a keystroke that notifies three times repaints once.
struct Notifier(Arc<AtomicBool>);

impl Observer for Notifier {
    fn changed(&self) {
        if !self.0.swap(true, Ordering::SeqCst) {
            request_frame();
        }
    }
}

fn request_frame() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let callback = Closure::once_into_js(move || {
        let ui = UI.with(|slot| slot.borrow().clone());
        if let Some(ui) = ui {
            ui.refresh();
        }
    });
    let _ = window.request_animation_frame(callback.unchecked_ref());
}

/// The elements this shell writes to. No document state.
struct Dom {
    document: Document,
    page: HtmlElement,
    gutter: HtmlElement,
    rows: HtmlElement,
    caret: HtmlElement,
    /// Where typing, the input method and the clipboard events arrive. Always emptied.
    input: HtmlTextAreaElement,
    name: HtmlElement,
    message: HtmlElement,
    position: HtmlElement,
    findbar: HtmlElement,
    find_input: HtmlInputElement,
    palette: HtmlElement,
    palette_input: HtmlInputElement,
    palette_list: HtmlElement,
    file_input: HtmlInputElement,
    measure: web_sys::HtmlCanvasElement,
}

impl Dom {
    fn find(document: &Document) -> Result<Dom, JsValue> {
        Ok(Dom {
            document: document.clone(),
            page: element(document, "page")?,
            gutter: element(document, "gutter")?,
            rows: element(document, "rows")?,
            caret: element(document, "caret")?,
            input: element(document, "input")?,
            name: element(document, "name")?,
            message: element(document, "message")?,
            position: element(document, "position")?,
            findbar: element(document, "findbar")?,
            find_input: element(document, "find-input")?,
            palette: element(document, "palette")?,
            palette_input: element(document, "palette-input")?,
            palette_list: element(document, "palette-list")?,
            file_input: element(document, "file-input")?,
            measure: element(document, "measure")?,
        })
    }
}

fn element<T: JsCast>(document: &Document, id: &str) -> Result<T, JsValue> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("index.html is missing #{id}")))?
        .dyn_into::<T>()
        .map_err(|_| JsValue::from_str(&format!("#{id} is not the element this shell expects")))
}

struct Ui {
    app: Arc<App>,
    dom: Dom,
    pending: Arc<AtomicBool>,
    /// The last thing the shell has to say — presentation only, so changing it asks for a
    /// frame by hand.
    message: RefCell<String>,
    dragging: Cell<bool>,
    /// Wheel movement not yet a whole row.
    wheel: Cell<f64>,
    /// The highlighted palette row.
    selected: Cell<usize>,
    mac: bool,
}

impl Ui {
    /// Measure with the font the page draws in — read from the computed style, so the
    /// stylesheet stays the one place it is chosen.
    fn metrics(&self) -> CanvasMetrics {
        let window = web_sys::window().unwrap();
        let style = window.get_computed_style(&self.dom.page).ok().flatten();
        // From the longhands: Firefox answers the `font` shorthand with an empty string, and a
        // canvas left on its default 10px font measures every row too short to wrap.
        let font = style.as_ref().and_then(|s| {
            let get = |name| s.get_property_value(name).ok().filter(|v| !v.is_empty());
            Some(format!(
                "{} {} {} {}",
                get("font-style").unwrap_or_else(|| "normal".into()),
                get("font-weight").unwrap_or_else(|| "400".into()),
                get("font-size")?,
                get("font-family")?
            ))
        });
        let line_height = style
            .as_ref()
            .and_then(|s| s.get_property_value("line-height").ok())
            .and_then(|h| h.trim_end_matches("px").parse::<f32>().ok())
            .unwrap_or(0.0);
        let canvas = self
            .dom
            .measure
            .get_context("2d")
            .ok()
            .flatten()
            .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok());
        if let (Some(canvas), Some(font)) = (&canvas, &font) {
            canvas.set_font(font);
        }
        CanvasMetrics::new(canvas, line_height)
    }

    fn with_frame<T>(&self, f: impl FnOnce(&Frame) -> T) -> T {
        let metrics = self.metrics();
        let frame = Frame::new(
            self.dom.page.client_width() as f32,
            self.dom.page.client_height() as f32,
            &metrics,
        );
        f(&frame)
    }

    fn after_input(&self) {
        self.with_frame(|frame| self.app.follow_caret(frame));
    }

    fn say(&self, message: impl Into<String>) {
        *self.message.borrow_mut() = message.into();
        if !self.pending.swap(true, Ordering::SeqCst) {
            request_frame();
        }
    }

    /// Repaint from the core — from its scroll position, never scrolling.
    fn refresh(&self) {
        self.pending.store(false, Ordering::SeqCst);
        if let Err(error) = self.render() {
            web_sys::console::error_1(&error);
        }
    }

    fn render(&self) -> Result<(), JsValue> {
        let view = self.with_frame(|frame| self.app.view(frame));
        let doc = &self.dom.document;

        // One element per visible row, rebuilt each frame: the cost is the window's, never the
        // file's, which is the point of a windowed read.
        self.dom.rows.set_text_content(None);
        self.dom.gutter.set_text_content(None);
        for row in &view.rows {
            if let Some(selection) = row.selection {
                let mark = doc.create_element("div")?;
                mark.set_class_name("selection");
                mark.set_attribute(
                    "style",
                    &format!(
                        "top:{}px;left:{}px;width:{}px;height:{}px",
                        row.top,
                        selection.from,
                        selection.to - selection.from,
                        view.row_height
                    ),
                )?;
                self.dom.rows.append_child(&mark)?;
            }
            let line = doc.create_element("div")?;
            line.set_class_name("row");
            line.set_attribute("style", &format!("top:{}px", row.top))?;
            line.set_text_content(Some(&layout::printable(&row.text).0));
            self.dom.rows.append_child(&line)?;
            if row.first {
                let number = doc.create_element("div")?;
                number.set_class_name("number");
                number.set_attribute("style", &format!("top:{}px", row.top))?;
                number.set_text_content(Some(&(row.line + 1).to_string()));
                self.dom.gutter.append_child(&number)?;
            }
        }

        let caret = self.dom.caret.style();
        let input = self.dom.input.style();
        match view.caret {
            Some(at) => {
                let place = format!("translate({}px, {}px)", at.x, at.top);
                caret.set_property("transform", &place)?;
                caret.set_property("height", &format!("{}px", at.height))?;
                caret.set_property("display", "block")?;
                // The input follows the caret, so an input method's window opens beside it.
                input.set_property("transform", &place)?;
            }
            None => caret.set_property("display", "none")?,
        }

        let dirty = self.app.is_dirty();
        let name = self.app.title();
        self.dom
            .name
            .set_text_content(Some(&format!("{name}{}", if dirty { " •" } else { "" })));
        self.dom
            .message
            .set_text_content(Some(&self.message.borrow()));
        let position = self.app.caret();
        self.dom.position.set_text_content(Some(&format!(
            "Ln {}, Col {} · {} lines · {}",
            position.line + 1,
            position.column + 1,
            view.line_count,
            self.app.encoding().line_ending.as_str()
        )));
        for (command, enabled) in [
            (Command::Undo, self.app.can_undo()),
            (Command::Redo, self.app.can_redo()),
        ] {
            if let Some(button) = self
                .dom
                .document
                .query_selector(&format!("[data-command=\"{}\"]", command.id()))?
            {
                button.toggle_attribute_with_force("disabled", !enabled)?;
            }
        }
        doc.set_title(&format!(
            "{}{name} — {}",
            if dirty { "• " } else { "" },
            quill_core::APP_NAME
        ));
        Ok(())
    }

    fn act(self: &Rc<Self>, action: Action) {
        let app = &self.app;
        match action {
            Action::Command(command) => return self.run(command),
            Action::Move(motion, extend) => {
                self.with_frame(|frame| app.move_caret(motion, extend, frame));
            }
            Action::Delete(kind) => {
                app.delete(kind);
            }
            Action::Type(c) => app.type_text(c.encode_utf8(&mut [0; 4])),
            Action::Escape => {
                app.clear_selection();
            }
            Action::Palette => return self.open_palette(),
        }
        self.after_input();
    }

    /// Carry out a menu verb.
    fn run(self: &Rc<Self>, command: Command) {
        let app = &self.app;
        match command {
            Command::New | Command::Close => {
                if self.may_discard() {
                    app.new_document();
                }
            }
            Command::Open => {
                if self.may_discard() {
                    // Only allowed from a user gesture, which every caller here is.
                    self.dom.file_input.click();
                }
            }
            Command::Save => self.download(None),
            Command::SaveAs => {
                let window = web_sys::window().unwrap();
                if let Ok(Some(name)) =
                    window.prompt_with_message_and_default("Save as", &self.download_name())
                    && !name.is_empty()
                {
                    self.download(Some(&name));
                }
            }
            Command::Undo => {
                app.undo();
            }
            Command::Redo => {
                app.redo();
            }
            // From a button or the palette: ask the browser to send the input its own event, so
            // there is one path to the clipboard (see `wire_clipboard`).
            Command::Cut | Command::Copy => {
                if let Some(document) = self.dom.document.dyn_ref::<web_sys::HtmlDocument>() {
                    self.dom.input.focus().ok();
                    let verb = if command == Command::Cut {
                        "cut"
                    } else {
                        "copy"
                    };
                    let _ = document.exec_command(verb);
                }
            }
            Command::Paste => {
                let ui = self.clone();
                let read = web_sys::window()
                    .unwrap()
                    .navigator()
                    .clipboard()
                    .read_text();
                spawn_local(async move {
                    match JsFuture::from(read).await {
                        Ok(text) => {
                            ui.app.insert_text(&text.as_string().unwrap_or_default());
                            ui.after_input();
                        }
                        Err(_) => ui.say("The browser would not share the clipboard — use Ctrl+V"),
                    }
                });
            }
            Command::SelectAll => app.select_all(),
            Command::Find => {
                self.dom.findbar.set_hidden(false);
                self.dom
                    .find_input
                    .set_value(&app.search().unwrap_or_default());
                let _ = self.dom.find_input.focus();
                self.dom.find_input.select();
            }
            Command::FindNext | Command::FindPrevious => {
                if app.search().is_none() {
                    return self.run(Command::Find);
                }
                let direction = if command == Command::FindNext {
                    Search::Forward
                } else {
                    Search::Backward
                };
                if !app.find_again(direction) {
                    self.say("No match");
                }
            }
            Command::About => self.say(format!(
                "{} {} — a shared core with native shells, compiled to WebAssembly. MIT licensed.",
                quill_core::APP_NAME,
                quill_core::VERSION
            )),
        }
        self.after_input();
    }

    /// A tab's way of asking before discarding work: the browser's own confirm.
    fn may_discard(&self) -> bool {
        !self.app.is_dirty()
            || web_sys::window()
                .unwrap()
                .confirm_with_message("Discard your unsaved changes?")
                .unwrap_or(false)
    }

    fn download_name(&self) -> String {
        match self.app.path() {
            Some(_) => self.app.title(),
            None => DOWNLOAD_NAME.into(),
        }
    }

    /// Hand the document to a download — the browser's Save.
    fn download(&self, name: Option<&str>) {
        let name = name.map_or_else(|| self.download_name(), str::to_string);
        let bytes = self.app.save_bytes(Some(&name));
        match self.offer(&name, &bytes) {
            Ok(()) => self.say(format!("Saved {name} to your downloads")),
            Err(_) => self.say(format!("The browser refused to download {name}")),
        }
    }

    fn offer(&self, name: &str, bytes: &[u8]) -> Result<(), JsValue> {
        let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
        let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;
        let anchor: web_sys::HtmlAnchorElement =
            self.dom.document.create_element("a")?.dyn_into()?;
        anchor.set_href(&url);
        anchor.set_download(name);
        anchor.click();
        web_sys::Url::revoke_object_url(&url)
    }

    async fn load(self: Rc<Self>, file: web_sys::File) {
        let name = file.name();
        let buffer = match JsFuture::from(file.array_buffer()).await {
            Ok(buffer) => buffer,
            Err(_) => return self.say(format!("Could not read {name}")),
        };
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        match self.app.open_bytes(Some(&name), &bytes) {
            Ok(()) => self.say(format!("Opened {name}")),
            Err(error) => self.say(error.to_string()),
        }
        let _ = self.dom.input.focus();
    }

    // --- the palette -----------------------------------------------------------------------

    fn open_palette(&self) {
        self.dom.palette.set_hidden(false);
        self.dom.palette_input.set_value("");
        self.selected.set(0);
        self.render_palette();
        let _ = self.dom.palette_input.focus();
    }

    fn close_palette(&self) {
        self.dom.palette.set_hidden(true);
        let _ = self.dom.input.focus();
    }

    fn matches(&self) -> Vec<Command> {
        let query = self.dom.palette_input.value();
        let mut ranked: Vec<(u32, Command)> = Command::ALL
            .iter()
            .filter_map(|&c| command::score(&query, c.label()).map(|s| (s, c)))
            .collect();
        ranked.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
        ranked.into_iter().map(|(_, c)| c).collect()
    }

    fn render_palette(&self) {
        let list = &self.dom.palette_list;
        list.set_text_content(None);
        for (i, command) in self.matches().into_iter().enumerate() {
            let Ok(item) = self.dom.document.create_element("li") else {
                return;
            };
            item.set_attribute("data-command", command.id()).ok();
            if i == self.selected.get() {
                item.set_class_name("selected");
            }
            let key = keymap::describe(command, self.mac).unwrap_or_default();
            item.set_inner_html(&format!(
                "<span class=\"label\">{}</span><span class=\"summary\">{}</span><kbd>{key}</kbd>",
                command.label(),
                command.summary()
            ));
            list.append_child(&item).ok();
        }
    }
}

// --- wiring --------------------------------------------------------------------------------

/// Attach a listener for the life of the page. The closure is leaked on purpose: it lives as
/// long as the element it is attached to, which is as long as the page.
fn listen<E: wasm_bindgen::convert::FromWasmAbi + 'static>(
    target: &web_sys::EventTarget,
    event: &str,
    handler: impl FnMut(E) + 'static,
) -> Result<(), JsValue> {
    let closure = Closure::<dyn FnMut(E)>::new(handler);
    target.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

fn wire_keyboard(ui: &Rc<Ui>) -> Result<(), JsValue> {
    let keys = ui.clone();
    listen(&ui.dom.input, "keydown", move |event: KeyboardEvent| {
        // While an input method is composing, every key is its.
        if event.is_composing() {
            return;
        }
        let key = event.key();
        let chord = Chord {
            key: &key,
            primary: event.ctrl_key() || event.meta_key(),
            shift: event.shift_key(),
            alt: event.alt_key(),
        };
        let Some(action) = keymap::action_for(&chord) else {
            return;
        };
        // Handled keys must not also do their default: Tab moves focus, Ctrl+S saves the page.
        event.prevent_default();
        keys.say(String::new());
        keys.act(action);
    })?;

    // Typed text, from a keyboard, an input method or a phone: whatever the textarea received,
    // given to the core and taken back out, so the textarea never holds a document of its own.
    let typed = ui.clone();
    listen(&ui.dom.input, "input", move |event: InputEvent| {
        if event.is_composing() {
            return;
        }
        let text = typed.dom.input.value();
        typed.dom.input.set_value("");
        if !text.is_empty() {
            typed.app.type_text(&text);
            typed.after_input();
        }
    })?;
    let composed = ui.clone();
    listen(&ui.dom.input, "compositionend", move |_: Event| {
        let text = composed.dom.input.value();
        composed.dom.input.set_value("");
        if !text.is_empty() {
            composed.app.type_text(&text);
            composed.after_input();
        }
    })
}

/// Cut, Copy and Paste as the browser's own events: they carry the clipboard synchronously and
/// need no permission, which `navigator.clipboard` does.
fn wire_clipboard(ui: &Rc<Ui>) -> Result<(), JsValue> {
    for verb in ["copy", "cut"] {
        let clip = ui.clone();
        listen(&ui.dom.input, verb, move |event: ClipboardEvent| {
            let text = if verb == "cut" {
                clip.app.cut()
            } else {
                clip.app.selected_text()
            };
            if let Some(data) = event.clipboard_data() {
                let _ = data.set_data("text/plain", &text);
                event.prevent_default();
            }
            clip.after_input();
        })?;
    }
    let paste = ui.clone();
    listen(&ui.dom.input, "paste", move |event: ClipboardEvent| {
        if let Some(text) = event
            .clipboard_data()
            .and_then(|data| data.get_data("text/plain").ok())
        {
            event.prevent_default();
            paste.app.insert_text(&text);
            paste.after_input();
        }
    })
}

fn wire_pointer(ui: &Rc<Ui>) -> Result<(), JsValue> {
    let place = |ui: &Ui, event: &MouseEvent, extend: bool| {
        let rect = ui.dom.page.get_bounding_client_rect();
        let x = (f64::from(event.client_x()) - rect.left()) as f32;
        let y = (f64::from(event.client_y()) - rect.top()) as f32;
        ui.with_frame(|frame| ui.app.place_caret(frame, x, y, extend));
    };
    let down = ui.clone();
    listen(&ui.dom.page, "mousedown", move |event: MouseEvent| {
        event.prevent_default();
        let _ = down.dom.input.focus();
        down.dragging.set(true);
        place(&down, &event, event.shift_key());
    })?;
    let moved = ui.clone();
    let window = web_sys::window().unwrap();
    listen(&window, "mousemove", move |event: MouseEvent| {
        if moved.dragging.get() {
            place(&moved, &event, true);
            moved.after_input();
        }
    })?;
    let up = ui.clone();
    listen(&window, "mouseup", move |_: MouseEvent| {
        up.dragging.set(false)
    })?;

    let wheel = ui.clone();
    listen(&ui.dom.page, "wheel", move |event: WheelEvent| {
        event.prevent_default();
        let row = f64::from(wheel.metrics().line_height());
        // deltaMode 1 is lines, 0 pixels; the page mode is rare enough to treat as lines.
        let rows = match event.delta_mode() {
            0 => event.delta_y() / row,
            _ => event.delta_y(),
        };
        let total = wheel.wheel.get() + rows;
        let whole = total.trunc();
        wheel.wheel.set(total - whole);
        if whole != 0.0 {
            wheel.with_frame(|frame| wheel.app.scroll_by(frame, whole as i64));
        }
    })
}

fn wire_toolbar(ui: &Rc<Ui>) -> Result<(), JsValue> {
    let buttons = ui.dom.document.query_selector_all("button[data-command]")?;
    for i in 0..buttons.length() {
        let Some(button) = buttons.item(i) else {
            continue;
        };
        let element: web_sys::Element = button.clone().dyn_into()?;
        let id = element.get_attribute("data-command").unwrap_or_default();
        let command = Command::from_id(&id)
            .ok_or_else(|| JsValue::from_str(&format!("no command {id:?} in the core")))?;
        let clicked = ui.clone();
        listen(&button, "click", move |_: MouseEvent| {
            clicked.run(command);
            let _ = clicked.dom.input.focus();
        })?;
    }
    let palette = ui.clone();
    let button: HtmlElement = element(&ui.dom.document, "palette-button")?;
    listen(&button, "click", move |_: MouseEvent| {
        palette.open_palette()
    })
}

fn wire_find(ui: &Rc<Ui>) -> Result<(), JsValue> {
    let find = ui.clone();
    listen(
        &ui.dom.find_input,
        "keydown",
        move |event: KeyboardEvent| match event.key().as_str() {
            "Enter" => {
                event.prevent_default();
                let direction = if event.shift_key() {
                    Search::Backward
                } else {
                    Search::Forward
                };
                if !find.app.find(&find.dom.find_input.value(), direction) {
                    find.say("No match");
                }
                find.after_input();
            }
            "Escape" => {
                find.dom.findbar.set_hidden(true);
                let _ = find.dom.input.focus();
            }
            _ => {}
        },
    )?;
    let close = ui.clone();
    let button: HtmlElement = element(&ui.dom.document, "find-close")?;
    listen(&button, "click", move |_: MouseEvent| {
        close.dom.findbar.set_hidden(true);
        let _ = close.dom.input.focus();
    })
}

fn wire_palette(ui: &Rc<Ui>) -> Result<(), JsValue> {
    let typed = ui.clone();
    listen(&ui.dom.palette_input, "input", move |_: Event| {
        typed.selected.set(0);
        typed.render_palette();
    })?;
    let keys = ui.clone();
    listen(
        &ui.dom.palette_input,
        "keydown",
        move |event: KeyboardEvent| {
            let matches = keys.matches();
            match event.key().as_str() {
                "ArrowDown" => keys
                    .selected
                    .set((keys.selected.get() + 1).min(matches.len().saturating_sub(1))),
                "ArrowUp" => keys.selected.set(keys.selected.get().saturating_sub(1)),
                "Escape" => return keys.close_palette(),
                "Enter" => {
                    keys.close_palette();
                    if let Some(&command) = matches.get(keys.selected.get()) {
                        keys.run(command);
                    }
                    return;
                }
                _ => return,
            }
            event.prevent_default();
            keys.render_palette();
        },
    )?;
    let clicked = ui.clone();
    listen(&ui.dom.palette_list, "click", move |event: MouseEvent| {
        let command = event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|e| e.closest("[data-command]").ok().flatten())
            .and_then(|e| e.get_attribute("data-command"))
            .and_then(|id| Command::from_id(&id));
        if let Some(command) = command {
            clicked.close_palette();
            clicked.run(command);
        }
    })
}

fn wire_files(ui: &Rc<Ui>) -> Result<(), JsValue> {
    let picked = ui.clone();
    listen(&ui.dom.file_input, "change", move |_: Event| {
        let Some(file) = picked.dom.file_input.files().and_then(|files| files.get(0)) else {
            return;
        };
        picked.dom.file_input.set_value("");
        spawn_local(picked.clone().load(file));
    })
}

fn wire_window(ui: &Rc<Ui>, window: &web_sys::Window) -> Result<(), JsValue> {
    // A resize moves where rows wrap, which only a repaint discovers. Not through `say`:
    // passing it the current message would hold the `RefCell` borrow across its write.
    let resized = ui.clone();
    listen(window, "resize", move |_: Event| {
        resized.after_input();
        if !resized.pending.swap(true, Ordering::SeqCst) {
            request_frame();
        }
    })?;
    // The browser's own answer to "save before closing?": a page may ask for the prompt, but
    // not word it.
    let unload = ui.clone();
    listen(window, "beforeunload", move |event: Event| {
        if unload.app.is_dirty() {
            event.prevent_default();
            let _ = js_sys::Reflect::set(&event, &"returnValue".into(), &"".into());
        }
    })
}
