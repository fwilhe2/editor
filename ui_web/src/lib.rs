//! `editor-web` — the browser shell over the editor core.
//!
//! A third class of shell. The CLI, TUI and GTK apps are Rust and call the core
//! directly; Windows and macOS are foreign languages reaching it through UniFFI.
//! This one is Rust *and* foreign at once: the crate depends on `editor-core` as an
//! ordinary Cargo dependency, compiles to `wasm32-unknown-unknown`, and talks to
//! the page through `wasm-bindgen` — UniFFI has no JavaScript target, and would be
//! the wrong tool anyway when the shell is written in Rust.
//!
//! The DOM here is a *renderer*, not the document. There is no `contenteditable`
//! anywhere: the surface is a plain focusable element whose lines are rebuilt from
//! `get_viewport` on every repaint, exactly as the GTK `TextView` and the WinUI
//! `TextBox` are. Letting the browser edit the text would create a second source of
//! truth, and the browser would win.
//!
//! The browser is also the first platform here with no filesystem. A file arrives
//! from the File API as a string and leaves as a download, which is what
//! `Editor::load_text` and `Editor::save_to_string` exist for; the shell never
//! learns a path, because there isn't one.

mod keymap;
mod layout;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use editor_core::{Editor, EditorObserver};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    Document, Event, File, HtmlAnchorElement, HtmlButtonElement, HtmlElement, HtmlInputElement,
    KeyboardEvent, MouseEvent, WheelEvent,
};

use keymap::{Chord, Request};
use layout::Metrics;

/// What a document with no name is downloaded as.
const UNTITLED: &str = "untitled.txt";

/// The probe element's text, whose width divided by its length is one character.
const PROBE_CHARS: f64 = 10.0;

thread_local! {
    /// The live shell, so the animation-frame callback can find its way back to it.
    /// The page owns it until the tab closes; nothing ever takes it out again.
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

/// Called by the generated JS glue as soon as the module is instantiated.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;

    let editor = Arc::new(Editor::new());
    let ui = Rc::new(Ui {
        editor: editor.clone(),
        dom: Dom::find(&document)?,
        pending: Arc::new(AtomicBool::new(false)),
        message: RefCell::new("Open a file, or start typing.".to_string()),
    });

    // The core pushes; the shell never polls. This is the same contract the TUI
    // implements with a flag and the GTK shell with a channel.
    editor.set_observer(Arc::new(Notifier(ui.pending.clone())));

    wire_keyboard(&ui)?;
    wire_pointer(&ui)?;
    wire_toolbar(&ui)?;
    wire_file_input(&ui)?;
    wire_window(&ui, &window)?;

    UI.with(|slot| *slot.borrow_mut() = Some(ui.clone()));

    ui.dom.surface.focus()?;
    ui.refresh();
    Ok(())
}

/// Raises a repaint when the core changes, and schedules the frame that draws it.
///
/// `EditorObserver` is `Send + Sync`, so this may not hold anything from the page —
/// a wasm module is single-threaded, but the trait does not know that. It holds a
/// flag instead, which doubles as "a frame is already scheduled": a keystroke that
/// notifies twice still repaints once.
struct Notifier(Arc<AtomicBool>);

impl EditorObserver for Notifier {
    fn state_changed(&self) {
        if !self.0.swap(true, Ordering::SeqCst) {
            request_frame();
        }
    }
}

fn request_frame() {
    let Some(window) = web_sys::window() else {
        return;
    };
    // `once_into_js` hands the closure to JS and frees it after the call, which is
    // what makes a per-frame allocation acceptable here.
    let callback = Closure::once_into_js(move || {
        UI.with(|slot| {
            let ui = slot.borrow().clone();
            if let Some(ui) = ui {
                ui.refresh();
            }
        });
    });
    let _ = window.request_animation_frame(callback.unchecked_ref());
}

/// The elements this shell writes to. No editor state — that all stays in the core.
struct Dom {
    document: Document,
    surface: HtmlElement,
    /// The positioning context for the caret, and the box whose height decides how
    /// many lines the core is asked for.
    lines: HtmlElement,
    /// Rebuilt from the viewport on every repaint.
    text: HtmlElement,
    caret: HtmlElement,
    probe: HtmlElement,
    name: HtmlElement,
    message: HtmlElement,
    position: HtmlElement,
    open: HtmlButtonElement,
    save: HtmlButtonElement,
    undo: HtmlButtonElement,
    redo: HtmlButtonElement,
    file_input: HtmlInputElement,
}

impl Dom {
    fn find(document: &Document) -> Result<Self, JsValue> {
        Ok(Dom {
            document: document.clone(),
            surface: element(document, "surface")?,
            lines: element(document, "lines")?,
            text: element(document, "text")?,
            caret: element(document, "caret")?,
            probe: element(document, "probe")?,
            name: element(document, "name")?,
            message: element(document, "message")?,
            position: element(document, "position")?,
            open: element(document, "open")?,
            save: element(document, "save")?,
            undo: element(document, "undo")?,
            redo: element(document, "redo")?,
            file_input: element(document, "file-input")?,
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
    editor: Arc<Editor>,
    dom: Dom,
    /// Set by the observer, cleared by the repaint it asked for.
    pending: Arc<AtomicBool>,
    /// Presentation only: the last thing the shell has to say. The core knows
    /// nothing about it, so changing it has to raise a repaint by hand.
    message: RefCell<String>,
}

impl Ui {
    /// Repaint from the core.
    ///
    /// Renders whatever the core's scroll offset says and deliberately does not call
    /// `follow_cursor` — key handling does that. If the repaint moved the view, a
    /// wheel scroll away from the cursor would snap straight back.
    fn refresh(&self) {
        self.pending.store(false, Ordering::SeqCst);
        if let Err(error) = self.render() {
            web_sys::console::error_1(&error);
        }
    }

    fn render(&self) -> Result<(), JsValue> {
        let metrics = self.metrics();
        let height = self.visible_lines(metrics);
        let start = self.editor.scroll_offset();
        let viewport = self
            .editor
            .get_viewport(start, start.saturating_add(height));

        // One element per visible line, rebuilt each frame. The cost is bounded by
        // the window, not the document — which is the whole point of the viewport.
        self.dom.text.set_text_content(None);
        for line in &viewport.lines {
            let element = self.dom.document.create_element("div")?;
            element.set_class_name("line");
            element.set_text_content(Some(line));
            self.dom.text.append_child(&element)?;
        }

        let cursor = viewport.cursor;
        let caret = self.dom.caret.style();
        if cursor.line >= viewport.start_line && cursor.line < viewport.start_line + height {
            let (x, y) = metrics.caret_offset(cursor.line - viewport.start_line, cursor.column);
            caret.set_property("transform", &format!("translate({x}px, {y}px)"))?;
            caret.set_property("height", &format!("{}px", metrics.line_height))?;
            caret.set_property("display", "block")?;
        } else {
            // Scrolled out of view: there is nowhere honest to draw it.
            caret.set_property("display", "none")?;
        }

        let dirty = self.editor.is_dirty();
        let name = self.document_name();
        self.dom
            .name
            .set_text_content(Some(&format!("{name}{}", if dirty { " •" } else { "" })));
        self.dom
            .message
            .set_text_content(Some(&self.message.borrow()));
        self.dom.position.set_text_content(Some(&format!(
            "Ln {}, Col {} · {} lines · {} chars",
            cursor.line + 1,
            cursor.column + 1,
            viewport.total_lines,
            self.editor.char_count()
        )));

        self.dom.undo.set_disabled(!self.editor.can_undo());
        self.dom.redo.set_disabled(!self.editor.can_redo());
        self.dom
            .document
            .set_title(&format!("{}{name} — Editor", if dirty { "• " } else { "" }));
        Ok(())
    }

    /// Measure the character grid from a probe carrying the same CSS as a line.
    ///
    /// Re-measured every frame rather than cached: page zoom and a late-loading
    /// font both change it, and neither fires an event this shell would hear.
    fn metrics(&self) -> Metrics {
        let rect = self.dom.probe.get_bounding_client_rect();
        Metrics::new(rect.width() / PROBE_CHARS, rect.height())
    }

    /// How many lines fit right now — the one number only the shell can know.
    fn visible_lines(&self, metrics: Metrics) -> u64 {
        metrics.visible_lines(self.dom.lines.client_height() as f64)
    }

    fn follow_cursor(&self) {
        let metrics = self.metrics();
        self.editor.follow_cursor(self.visible_lines(metrics));
    }

    fn on_key(&self, event: &KeyboardEvent) {
        let key = event.key();
        let chord = Chord {
            key: &key,
            // ⌘ on macOS, Ctrl everywhere else. Resolved here so the keymap does
            // not have to know which platform the browser is running on.
            primary: event.ctrl_key() || event.meta_key(),
            shift: event.shift_key(),
            alt: event.alt_key(),
        };

        let Some(action) = keymap::action_for(&chord) else {
            // Not ours: Ctrl+T, F5 and the rest still belong to the browser.
            return;
        };
        // Handled keys must not also do their default thing — Tab moves focus,
        // Ctrl+S opens the browser's own save dialog.
        event.prevent_default();

        self.set_message(String::new());
        match keymap::apply(action, &self.editor) {
            Some(Request::Open) => self.open_picker(),
            Some(Request::Save) => self.save(),
            None => {}
        }
        // Keep the caret on screen; the repaint itself comes from the observer.
        self.follow_cursor();
    }

    fn on_pointer(&self, event: &MouseEvent) -> Result<(), JsValue> {
        // The pointer is a first-class input on the web, and `set_cursor` is core
        // API, so clicking to place the caret costs the architecture nothing.
        let rect = self.dom.lines.get_bounding_client_rect();
        let metrics = self.metrics();
        let position = metrics.position_at(
            event.client_x() as f64 - rect.left(),
            event.client_y() as f64 - rect.top(),
            self.editor.scroll_offset(),
        );
        self.editor.set_cursor(position);
        // A click has to hand the keyboard back, or typing stops after it.
        self.dom.surface.focus()
    }

    fn on_wheel(&self, event: &WheelEvent) {
        let metrics = self.metrics();
        let page = self.visible_lines(metrics);
        let delta = metrics.wheel_lines(event.delta_y(), event.delta_mode(), page);
        if delta == 0 {
            return;
        }
        // Straight into the core's scroll offset, so this shell scrolls by exactly
        // the same rule as every other one.
        let offset = layout::scrolled_by(self.editor.scroll_offset(), delta);
        self.editor.set_scroll_offset(offset);
    }

    fn open_picker(&self) {
        // Clearing the value first, or picking the same file twice fires no change
        // event and the second open silently does nothing.
        self.dom.file_input.set_value("");
        self.dom.file_input.click();
    }

    /// Read a picked file into the core.
    ///
    /// The name is all the browser gives us — there is no path, and no way to write
    /// back to where the file came from. It travels with the document only so the
    /// download has something to be called.
    async fn load(self: Rc<Self>, file: File) {
        let name = file.name();
        match JsFuture::from(file.text()).await {
            Ok(value) => {
                let text = value.as_string().unwrap_or_default();
                self.editor.load_text(Some(&name), &text);
                self.set_message(format!("Opened {name}"));
            }
            Err(_) => self.set_message(format!("Could not read {name}")),
        }
    }

    /// Save by handing the document to a download — the only writing a page may do.
    fn save(&self) {
        let name = self.document_name();
        let text = self.editor.save_to_string(None);
        match self.download(&name, &text) {
            Ok(()) => self.set_message(format!("Saved {name} to your downloads")),
            // The document is now marked saved and isn't; say so loudly rather than
            // keeping a second copy of the whole buffer around for a case that only
            // happens when the browser blocks downloads outright.
            Err(_) => self.set_message(format!("The browser refused to download {name}")),
        }
    }

    fn download(&self, name: &str, text: &str) -> Result<(), JsValue> {
        let parts = js_sys::Array::new();
        parts.push(&JsValue::from_str(text));
        let blob = web_sys::Blob::new_with_str_sequence(&parts)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;

        let anchor: HtmlAnchorElement = self
            .dom
            .document
            .create_element("a")?
            .dyn_into()
            .map_err(|_| JsValue::from_str("an anchor is not an anchor"))?;
        anchor.set_href(&url);
        anchor.set_download(name);
        anchor.click();

        // The blob would otherwise be held until the tab closes.
        web_sys::Url::revoke_object_url(&url)
    }

    fn document_name(&self) -> String {
        self.editor
            .path()
            .map(|path| path.display().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| UNTITLED.to_string())
    }

    /// Ask for a repaint the core will not send: a resize, or anything else that
    /// changes the picture without changing the document.
    ///
    /// Coalesces exactly as the observer does, and for the same reason.
    fn request_repaint(&self) {
        if !self.pending.swap(true, Ordering::SeqCst) {
            request_frame();
        }
    }

    /// Change something only the shell knows about, and show it.
    fn set_message(&self, message: String) {
        *self.message.borrow_mut() = message;
        self.request_repaint();
    }
}

fn wire_keyboard(ui: &Rc<Ui>) -> Result<(), JsValue> {
    let handler = ui.clone();
    listen(&ui.dom.surface, "keydown", move |event: KeyboardEvent| {
        handler.on_key(&event);
    })
}

fn wire_pointer(ui: &Rc<Ui>) -> Result<(), JsValue> {
    let click = ui.clone();
    listen(&ui.dom.surface, "mousedown", move |event: MouseEvent| {
        if let Err(error) = click.on_pointer(&event) {
            web_sys::console::error_1(&error);
        }
    })?;

    let wheel = ui.clone();
    listen(&ui.dom.surface, "wheel", move |event: WheelEvent| {
        wheel.on_wheel(&event);
    })
}

fn wire_toolbar(ui: &Rc<Ui>) -> Result<(), JsValue> {
    // Every button hands the keyboard back afterwards: a toolbar that steals focus
    // leaves the user typing into nothing.
    let open = ui.clone();
    listen(&ui.dom.open, "click", move |_: Event| {
        open.open_picker();
    })?;

    let save = ui.clone();
    listen(&ui.dom.save, "click", move |_: Event| {
        save.save();
        let _ = save.dom.surface.focus();
    })?;

    let undo = ui.clone();
    listen(&ui.dom.undo, "click", move |_: Event| {
        undo.editor.undo();
        undo.follow_cursor();
        let _ = undo.dom.surface.focus();
    })?;

    let redo = ui.clone();
    listen(&ui.dom.redo, "click", move |_: Event| {
        redo.editor.redo();
        redo.follow_cursor();
        let _ = redo.dom.surface.focus();
    })
}

fn wire_file_input(ui: &Rc<Ui>) -> Result<(), JsValue> {
    let input = ui.dom.file_input.clone();
    let ui = ui.clone();
    listen(&input, "change", move |_: Event| {
        let Some(file) = ui.dom.file_input.files().and_then(|files| files.get(0)) else {
            return;
        };
        // Reading a file is a promise; nothing else in this shell is async.
        spawn_local(ui.clone().load(file));
    })
}

fn wire_window(ui: &Rc<Ui>, window: &web_sys::Window) -> Result<(), JsValue> {
    // A resize changes how many lines fit, which only a repaint can discover. It
    // must not go through `set_message`: passing the current message back in would
    // hold a `RefCell` borrow across the write and panic.
    let resize = ui.clone();
    listen(window, "resize", move |_: Event| {
        resize.request_repaint();
    })?;

    // The browser's own answer to GTK's "save before closing?" dialog. A page may
    // ask for the prompt but not word it, so there is nothing to phrase here.
    let unload = ui.clone();
    listen(window, "beforeunload", move |event: Event| {
        if !unload.editor.is_dirty() {
            return;
        }
        event.prevent_default();
        // Chrome still wants the legacy property set as well.
        let _ = js_sys::Reflect::set(
            &event,
            &JsValue::from_str("returnValue"),
            &JsValue::from_str(""),
        );
    })
}

/// Attach a listener for the lifetime of the page.
///
/// The closure is deliberately leaked: it lives exactly as long as the element it
/// is attached to, and both die with the tab.
fn listen<E, F>(target: &web_sys::EventTarget, event: &str, handler: F) -> Result<(), JsValue>
where
    // Any DOM event type: `FromWasmAbi` is what lets JS hand it to a Rust closure.
    E: wasm_bindgen::convert::FromWasmAbi + 'static,
    F: FnMut(E) + 'static,
{
    let closure = Closure::wrap(Box::new(handler) as Box<dyn FnMut(E)>);
    target.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}
