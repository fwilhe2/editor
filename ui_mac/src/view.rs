// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The editor's view: one flipped `NSView` that draws the document and takes its keys.
//!
//! **Not an `NSTextView`.** It owns a text storage of its own, which would be a second document;
//! this view paints [`App::view`] in `drawRect:` and forwards everything to the core.
//!
//! **Keys arrive as the text system's.** `keyDown:` hands the event to `interpretKeyEvents:`,
//! which calls back with `insertText:replacementRange:` for text and `doCommandBySelector:` for
//! everything else, read by [`crate::keys`] — so the user's own key bindings work here as they
//! do in TextEdit.
//!
//! **AppKit's undo stack is not used.** Edit ▸ Undo sends `undo:` down the responder chain and
//! this view answers it from the core's history (architecture rule 2), so AppKit never records an
//! edit it does not understand.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSAppearanceCustomization,
    NSApplication, NSBeep, NSEvent, NSEventModifierFlags, NSGraphicsContext, NSMenuItem,
    NSModalResponseOK, NSOpenPanel, NSPasteboard, NSPasteboardTypeString, NSSavePanel, NSTextField,
    NSTextInputClient, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound, NSPoint, NSRange, NSRect,
    NSSize, NSString, NSUInteger,
};
use quill_core::{App, Command, Observer, Search};

use crate::keys::{self, Action};
use crate::menu;
use crate::metrics::CoreText;
use crate::paint;
use crate::theme::Palette;

/// What the view draws from and answers to. No document state: that is all behind [`App`].
pub struct Shell {
    pub app: Arc<App>,
    text: CoreText,
    /// Raised by the observer, cleared by the repaint it asked for.
    pending: Arc<AtomicBool>,
    /// Scrolling not yet a whole row — a trackpad scrolls in points.
    wheel: Cell<f64>,
}

/// Turns a change in the core into a repaint on the main thread.
///
/// [`Observer`] is `Send + Sync` and an `NSView` is neither, so the view travels as an address
/// and is touched only through `performSelectorOnMainThread:withObject:waitUntilDone:`, which
/// Apple documents as callable from any thread. The view lives as long as the process (the window
/// and the delegate both hold it), so the address stays good. The flag coalesces a burst.
pub struct Notifier {
    view: usize,
    pending: Arc<AtomicBool>,
}

impl Observer for Notifier {
    fn changed(&self) {
        if self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        // SAFETY: see the type's comment; the selector is the view's own `coreChanged`, which
        // takes no argument.
        unsafe {
            let view = &*(self.view as *const AnyObject);
            let _: () = msg_send![
                view,
                performSelectorOnMainThread: sel!(coreChanged),
                withObject: std::ptr::null::<AnyObject>(),
                waitUntilDone: false
            ];
        }
    }
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "QuillView"]
    #[ivars = Shell]
    pub struct QuillView;

    impl QuillView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(coreChanged))]
        fn core_changed(&self) {
            self.ivars().pending.store(false, Ordering::Release);
            self.refresh();
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let Some(context) = NSGraphicsContext::currentContext() else {
                return;
            };
            let size = self.bounds().size;
            let shell = self.ivars();
            let focused = self.window().is_some_and(|window| window.isKeyWindow());
            paint::paint(
                &context.CGContext(),
                &shell.text,
                &shell.app,
                size.width,
                size.height,
                &self.palette(),
                focused,
            );
        }

        /// A resize moves where rows wrap.
        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            // SAFETY: the superclass's own implementation, with the same argument.
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.after_input();
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            self.interpretKeyEvents(&NSArray::from_slice(&[event]));
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
            let shift = event.modifierFlags().contains(NSEventModifierFlags::Shift);
            self.place(event, shift);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.place(event, true);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let shell = self.ivars();
            let row = f64::from(quill_core::layout::Metrics::line_height(&shell.text));
            // A trackpad reports points, a wheel lines.
            let rows = if event.hasPreciseScrollingDeltas() {
                -event.scrollingDeltaY() / row
            } else {
                -event.scrollingDeltaY() * 3.0
            };
            let total = shell.wheel.get() + rows;
            let whole = total.trunc();
            shell.wheel.set(total - whole);
            if whole != 0.0 {
                self.with_frame(|frame| shell.app.scroll_by(frame, whole as i64));
            }
        }

        // --- the Edit menu's standard selectors ---------------------------------------------

        #[unsafe(method(undo:))]
        fn undo(&self, _sender: Option<&AnyObject>) {
            self.run(Command::Undo);
        }

        #[unsafe(method(redo:))]
        fn redo(&self, _sender: Option<&AnyObject>) {
            self.run(Command::Redo);
        }

        #[unsafe(method(cut:))]
        fn cut(&self, _sender: Option<&AnyObject>) {
            self.run(Command::Cut);
        }

        #[unsafe(method(copy:))]
        fn copy(&self, _sender: Option<&AnyObject>) {
            self.run(Command::Copy);
        }

        #[unsafe(method(paste:))]
        fn paste(&self, _sender: Option<&AnyObject>) {
            self.run(Command::Paste);
        }

        #[unsafe(method(selectAll:))]
        fn select_all(&self, _sender: Option<&AnyObject>) {
            self.run(Command::SelectAll);
        }

        /// Every other menu verb, with the command in the item's tag.
        #[unsafe(method(quillCommand:))]
        fn quill_command(&self, sender: Option<&AnyObject>) {
            let command = sender
                .and_then(|sender| sender.downcast_ref::<NSMenuItem>())
                .and_then(|item| menu::from_tag(item.tag()));
            if let Some(command) = command {
                self.run(command);
            }
        }

        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let app = &self.ivars().app;
            let command = item.action().and_then(|action| {
                let name = action.name().to_str().unwrap_or_default();
                match name {
                    menu::COMMAND_SELECTOR => menu::from_tag(item.tag()),
                    _ => menu::from_selector(name),
                }
            });
            match command {
                Some(Command::Undo) => app.can_undo(),
                Some(Command::Redo) => app.can_redo(),
                Some(Command::Cut | Command::Copy) => app.selection().is_some(),
                Some(Command::Paste) => read_pasteboard().is_some(),
                _ => true,
            }
        }
    }

    unsafe impl NSObjectProtocol for QuillView {}

    /// The input client: what the text system says a key means.
    ///
    /// ponytail: marked text — an input method's composition, drawn inline before it commits —
    /// is not held: `hasMarkedText` is always false, so a composing input method commits as it
    /// goes. Holding it needs a composition in the view's state and an underline in `paint.rs`;
    /// grind's Mac shell has both, as the model for when this one needs them.
    unsafe impl NSTextInputClient for QuillView {
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, _replacement: NSRange) {
            let text = string_of(string);
            self.ivars().app.type_text(&text);
            self.after_input();
        }

        #[unsafe(method(doCommandBySelector:))]
        fn do_command_by_selector(&self, selector: Sel) {
            let name = selector.name().to_str().unwrap_or_default();
            match keys::action(name) {
                Some(action) => self.act(action),
                None => NSBeep(),
            }
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(&self, _string: &AnyObject, _selected: NSRange, _replacement: NSRange) {
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {}

        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            NSRange::new(NSNotFound as NSUInteger, 0)
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            NSRange::new(NSNotFound as NSUInteger, 0)
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            false
        }

        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring(
            &self,
            _proposed: NSRange,
            _actual: *mut NSRange,
        ) -> Option<Retained<NSAttributedString>> {
            None
        }

        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }

        /// Where the caret is on the screen — an input method's candidate window goes there.
        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect_for_character_range(&self, _range: NSRange, _actual: *mut NSRange) -> NSRect {
            let shell = self.ivars();
            let size = self.bounds().size;
            let view = shell
                .app
                .view(&paint::frame(&shell.text, &shell.app, size.width, size.height));
            let Some(caret) = view.caret else {
                return NSRect::ZERO;
            };
            let (left, top) = paint::origin(&shell.text, &shell.app);
            let local = NSRect::new(
                NSPoint::new(left + f64::from(caret.x), top + f64::from(caret.top)),
                NSSize::new(1.0, f64::from(caret.height)),
            );
            let in_window = self.convertRect_toView(local, None);
            self.window()
                .map_or(in_window, |window| window.convertRectToScreen(in_window))
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn character_index_for_point(&self, _point: NSPoint) -> NSUInteger {
            NSNotFound as NSUInteger
        }
    }
);

impl QuillView {
    pub fn new(app: Arc<App>, mtm: MainThreadMarker) -> Retained<QuillView> {
        let pending = Arc::new(AtomicBool::new(false));
        let this = QuillView::alloc(mtm).set_ivars(Shell {
            app: app.clone(),
            text: CoreText::new(),
            pending: pending.clone(),
            wheel: Cell::new(0.0),
        });
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(820.0, 600.0));
        // SAFETY: `initWithFrame:` is `NSView`'s designated initialiser.
        let view: Retained<QuillView> = unsafe { msg_send![super(this), initWithFrame: frame] };
        app.observe(Arc::new(Notifier {
            view: Retained::as_ptr(&view) as usize,
            pending,
        }));
        view
    }

    fn palette(&self) -> Palette {
        let dark = self
            .effectiveAppearance()
            .name()
            .to_string()
            .contains("Dark");
        if dark {
            Palette::dark()
        } else {
            Palette::light()
        }
    }

    fn with_frame<T>(&self, f: impl FnOnce(&quill_core::Frame) -> T) -> T {
        let shell = self.ivars();
        let size = self.bounds().size;
        f(&paint::frame(
            &shell.text,
            &shell.app,
            size.width,
            size.height,
        ))
    }

    /// After input: bring the caret into view. The repaint comes from the observer.
    fn after_input(&self) {
        let app = &self.ivars().app;
        self.with_frame(|frame| app.follow_caret(frame));
    }

    /// The window's title, its edited dot and its proxy icon, and a repaint.
    pub fn refresh(&self) {
        let app = &self.ivars().app;
        if let Some(window) = self.window() {
            window.setTitle(&NSString::from_str(&app.title()));
            // The dot in the close button: the Mac's "unsaved changes", rather than a mark in
            // the title.
            window.setDocumentEdited(app.is_dirty());
            if let Some(path) = app.path().filter(|p| p.is_absolute()) {
                window.setRepresentedFilename(&NSString::from_str(&path.display().to_string()));
            }
        }
        self.setNeedsDisplay(true);
    }

    fn place(&self, event: &NSEvent, extend: bool) {
        let at = self.convertPoint_fromView(event.locationInWindow(), None);
        let shell = self.ivars();
        let (left, top) = paint::origin(&shell.text, &shell.app);
        self.with_frame(|frame| {
            let (x, y) = ((at.x - left) as f32, (at.y - top) as f32);
            shell.app.place_caret(frame, x, y, extend);
            shell.app.follow_caret(frame);
        });
    }

    fn act(&self, action: Action) {
        let app = &self.ivars().app;
        match action {
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
        }
        self.after_input();
    }

    /// Carry out a menu verb.
    pub fn run(&self, command: Command) {
        let app = &self.ivars().app;
        let mtm = self.mtm();
        match command {
            Command::New => {
                if may_discard(app, mtm) {
                    app.new_document();
                }
            }
            Command::Open => {
                if may_discard(app, mtm) {
                    let panel = NSOpenPanel::openPanel(mtm);
                    if panel.runModal() == NSModalResponseOK
                        && let Some(path) = panel.URL().and_then(|url| url.path())
                        && let Err(error) = app.open_file(path.to_string())
                    {
                        tell(mtm, "The document could not be opened.", &error.to_string());
                    }
                }
            }
            Command::Save => {
                save(app, mtm);
            }
            Command::SaveAs => {
                save_as(app, mtm);
            }
            Command::Close => {
                if let Some(window) = self.window() {
                    window.performClose(None);
                }
            }
            Command::Undo => {
                app.undo();
            }
            Command::Redo => {
                app.redo();
            }
            Command::Cut => write_pasteboard(&app.cut()),
            Command::Copy => write_pasteboard(&app.selected_text()),
            Command::Paste => {
                if let Some(text) = read_pasteboard() {
                    app.insert_text(&text);
                }
            }
            Command::SelectAll => app.select_all(),
            Command::Find => {
                let initial = app.search().unwrap_or_default();
                if let Some(query) = ask(mtm, "Find", "Search the document for:", "Find", &initial)
                    && !app.find(&query, Search::Forward)
                {
                    NSBeep();
                }
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
                    NSBeep();
                }
            }
            Command::About => {
                NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanel(None);
            }
        }
        self.after_input();
        self.refresh();
    }
}

/// The Mac's question before work is lost: Save, Don't Save, Cancel. True when the document may
/// go.
pub fn may_discard(app: &App, mtm: MainThreadMarker) -> bool {
    if !app.is_dirty() {
        return true;
    }
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&format!(
        "Do you want to save the changes made to the document “{}”?",
        app.title()
    )));
    alert.setInformativeText(&NSString::from_str(
        "Your changes will be lost if you don’t save them.",
    ));
    alert.addButtonWithTitle(&NSString::from_str("Save"));
    alert.addButtonWithTitle(&NSString::from_str("Don’t Save"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    match alert.runModal() {
        answer if answer == NSAlertFirstButtonReturn => save(app, mtm),
        answer if answer == NSAlertSecondButtonReturn => true,
        _ => false,
    }
}

fn save(app: &App, mtm: MainThreadMarker) -> bool {
    if app.path().is_none_or(|p| !p.is_absolute()) {
        return save_as(app, mtm);
    }
    match app.save_file() {
        Ok(()) => true,
        Err(error) => {
            tell(mtm, "The document could not be saved.", &error.to_string());
            false
        }
    }
}

fn save_as(app: &App, mtm: MainThreadMarker) -> bool {
    let panel = NSSavePanel::savePanel(mtm);
    panel.setNameFieldStringValue(&NSString::from_str(&app.title()));
    if panel.runModal() != NSModalResponseOK {
        return false;
    }
    let Some(path) = panel.URL().and_then(|url| url.path()) else {
        return false;
    };
    match app.save_file_as(path.to_string()) {
        Ok(()) => true,
        Err(error) => {
            tell(mtm, "The document could not be saved.", &error.to_string());
            false
        }
    }
}

/// Ask for one line in an alert — the platform's own shape for a short question.
fn ask(
    mtm: MainThreadMarker,
    title: &str,
    detail: &str,
    action: &str,
    initial: &str,
) -> Option<String> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.addButtonWithTitle(&NSString::from_str(action));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    let field = NSTextField::textFieldWithString(&NSString::from_str(initial), mtm);
    field.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(260.0, 24.0),
    ));
    alert.setAccessoryView(Some(&field));
    alert.window().setInitialFirstResponder(Some(&field));
    (alert.runModal() == NSAlertFirstButtonReturn)
        .then(|| field.stringValue().to_string())
        .filter(|answer| !answer.is_empty())
}

fn tell(mtm: MainThreadMarker, message: &str, detail: &str) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(message));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.runModal();
}

fn write_pasteboard(text: &str) {
    if text.is_empty() {
        return;
    }
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    // SAFETY: the type is a constant AppKit exports.
    unsafe { board.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) };
}

fn read_pasteboard() -> Option<String> {
    // SAFETY: the type is a constant AppKit exports.
    unsafe { NSPasteboard::generalPasteboard().stringForType(NSPasteboardTypeString) }
        .map(|text| text.to_string())
        .filter(|text| !text.is_empty())
}

/// The text in an `insertText:` argument, which is an `NSString` or an `NSAttributedString`.
fn string_of(object: &AnyObject) -> String {
    if let Some(string) = object.downcast_ref::<NSString>() {
        return string.to_string();
    }
    if let Some(attributed) = object.downcast_ref::<NSAttributedString>() {
        return attributed.string().to_string();
    }
    String::new()
}
