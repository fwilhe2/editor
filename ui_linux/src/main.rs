//! `edit-gtk` — the GNOME shell over the editor core.
//!
//! Pure Rust like the TUI, so it depends on `editor-core` directly: no FFI. The
//! widgets come from libadwaita to follow the GNOME HIG (`AdwHeaderBar`,
//! `AdwAlertDialog` for unsaved changes, toasts for feedback).
//!
//! The `GtkTextView` here is a *renderer*, not the document. It is non-editable,
//! so GTK can never change the text on its own; keys are intercepted before it
//! sees them and routed to the core, and the buffer is refilled from
//! `get_viewport` afterwards. That keeps the core the only source of truth, at
//! the cost of one screen's worth of duplicated text.

mod keymap;

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;

use editor_core::{Editor, EditorObserver};
use libadwaita as adw;
use libadwaita::gtk;

use adw::prelude::*;
use gtk::glib;
use gtk::pango;

use keymap::Request;

const APP_ID: &str = "io.github.fwilhe2.Editor";
const USAGE: &str = "usage: edit-gtk <file>\n";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    if path == "-h" || path == "--help" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    // Like the other shells, a missing file is an error rather than a new buffer.
    let editor = match Editor::open(&path) {
        Ok(editor) => Arc::new(editor),
        Err(error) => {
            eprintln!("edit-gtk: {error}");
            return ExitCode::FAILURE;
        }
    };

    let application = adw::Application::builder().application_id(APP_ID).build();

    application.connect_activate(move |application| build_window(application, editor.clone()));

    // The file argument was consumed above; GApplication must not try to parse it.
    match application.run_with_args::<&str>(&[]) {
        code if code == glib::ExitCode::SUCCESS => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// Everything the callbacks need. Holds widgets and the core — but no editor
/// state, which stays behind [`Editor`].
struct Ui {
    editor: Arc<Editor>,
    text_view: gtk::TextView,
    adjustment: gtk::Adjustment,
    title: adw::WindowTitle,
    save_button: gtk::Button,
    undo_button: gtk::Button,
    redo_button: gtk::Button,
    toasts: adw::ToastOverlay,
    /// True while `refresh` is writing to widgets, so their change signals do not
    /// bounce straight back into the core.
    syncing: Cell<bool>,
    /// Set once the user has answered the unsaved-changes dialog.
    closing: Cell<bool>,
}

impl Ui {
    /// Repaint from the core. Renders whatever the core's scroll offset says, so
    /// dragging the scrollbar away from the cursor sticks.
    fn refresh(&self) {
        self.syncing.set(true);

        let height = self.visible_lines();
        let start = self.editor.scroll_offset();
        let viewport = self.editor.get_viewport(start, start + height);

        let buffer = self.text_view.buffer();
        buffer.set_text(&viewport.lines.join("\n"));

        // Draw the caret where the core says it is. The view is not editable, so
        // this mark is the only thing that ever moves it.
        let cursor = viewport.cursor;
        if cursor.line >= viewport.start_line {
            let row = (cursor.line - viewport.start_line) as i32;
            if let Some(iter) = buffer.iter_at_line_offset(row, cursor.column as i32) {
                buffer.place_cursor(&iter);
            }
        }

        self.adjustment.set_upper(viewport.total_lines as f64);
        self.adjustment.set_page_size(height as f64);
        self.adjustment.set_value(viewport.start_line as f64);

        let dirty = self.editor.is_dirty();
        self.save_button.set_sensitive(dirty);
        self.undo_button.set_sensitive(self.editor.can_undo());
        self.redo_button.set_sensitive(self.editor.can_redo());
        self.title.set_subtitle(&format!(
            "{}:{}{}",
            cursor.line + 1,
            cursor.column + 1,
            if dirty { "  •  Unsaved changes" } else { "" }
        ));

        self.syncing.set(false);
    }

    /// How many lines fit in the text view right now.
    ///
    /// The core needs this to size the viewport, and it is the one number only the
    /// shell can know. Before the first allocation the height is 0, hence the floor
    /// of one line.
    fn visible_lines(&self) -> u64 {
        let metrics = self.text_view.pango_context().metrics(None, None);
        let line_height = ((metrics.ascent() + metrics.descent()) / pango::SCALE).max(1);
        let usable =
            self.text_view.height() - self.text_view.top_margin() - self.text_view.bottom_margin();
        (usable / line_height).max(1) as u64
    }

    fn save(&self) -> bool {
        match self.editor.save_file() {
            Ok(()) => {
                self.toasts.add_toast(adw::Toast::new("Saved"));
                true
            }
            Err(error) => {
                self.toasts.add_toast(adw::Toast::new(&error.to_string()));
                false
            }
        }
    }
}

/// Bridges core notifications onto the GTK main loop.
///
/// `EditorObserver` is `Send + Sync` and GTK widgets are neither, so the observer
/// cannot touch the UI directly. It forwards through a channel that a task on the
/// main context drains — the same shape the Swift and C# shells will need once the
/// core does work off the UI thread.
struct Notifier(async_channel::Sender<()>);

impl EditorObserver for Notifier {
    fn state_changed(&self) {
        // The channel is unbounded, so this never blocks the caller.
        let _ = self.0.send_blocking(());
    }
}

fn build_window(application: &adw::Application, editor: Arc<Editor>) {
    let text_view = gtk::TextView::builder()
        .editable(false) // only the core edits
        .cursor_visible(true)
        .monospace(true)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(12)
        .right_margin(12)
        .hexpand(true)
        .vexpand(true)
        .build();

    let adjustment = gtk::Adjustment::new(0.0, 0.0, 1.0, 1.0, 1.0, 1.0);
    let scrollbar = gtk::Scrollbar::new(gtk::Orientation::Vertical, Some(&adjustment));

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    content.append(&text_view);
    content.append(&scrollbar);

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&content));

    let file_name = editor
        .path()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "Untitled".to_string());
    let title = adw::WindowTitle::new(&file_name, "");

    let undo_button = icon_button("edit-undo-symbolic", "Undo (Ctrl+Z)");
    let redo_button = icon_button("edit-redo-symbolic", "Redo (Ctrl+Shift+Z)");
    let save_button = icon_button("document-save-symbolic", "Save (Ctrl+S)");

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));
    header.pack_start(&undo_button);
    header.pack_start(&redo_button);
    header.pack_end(&save_button);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&header);
    root.append(&toasts);

    let window = adw::ApplicationWindow::builder()
        .application(application)
        .default_width(800)
        .default_height(600)
        .content(&root)
        .build();

    let ui = Rc::new(Ui {
        editor: editor.clone(),
        text_view: text_view.clone(),
        adjustment: adjustment.clone(),
        title,
        save_button: save_button.clone(),
        undo_button: undo_button.clone(),
        redo_button: redo_button.clone(),
        toasts,
        syncing: Cell::new(false),
        closing: Cell::new(false),
    });

    wire_notifications(&ui, &editor);
    wire_keys(&ui, &window);
    wire_buttons(&ui, &window);
    wire_scrollbar(&ui, &adjustment);
    wire_close_request(&ui, &window);

    // The first allocation decides how many lines fit, so the initial paint has to
    // wait for it; `default-height` also fires on every later resize.
    let resize_ui = ui.clone();
    window.connect_default_height_notify(move |_| resize_ui.refresh());

    window.present();
    text_view.grab_focus();
    ui.refresh();
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.set_tooltip_text(Some(tooltip));
    button
}

fn wire_notifications(ui: &Rc<Ui>, editor: &Arc<Editor>) {
    let (sender, receiver) = async_channel::unbounded();
    editor.set_observer(Arc::new(Notifier(sender)));

    let ui = ui.clone();
    glib::spawn_future_local(async move {
        while receiver.recv().await.is_ok() {
            // A single keystroke can produce several notifications (the edit, then
            // the scroll that follows it); collapse them into one repaint.
            while receiver.try_recv().is_ok() {}
            ui.refresh();
        }
    });
}

fn wire_keys(ui: &Rc<Ui>, window: &adw::ApplicationWindow) {
    let keys = gtk::EventControllerKey::new();
    // Capture, so the text view never sees a key we mean to handle.
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);

    let ui = ui.clone();
    let key_window = window.clone();
    keys.connect_key_pressed(move |_, key, _code, modifiers| {
        let Some(action) = keymap::action_for(key, modifiers) else {
            return glib::Propagation::Proceed;
        };

        match keymap::apply(action, &ui.editor) {
            Some(Request::Save) => {
                ui.save();
                ui.refresh();
            }
            Some(Request::Quit) => key_window.close(),
            None => {}
        }

        // Keep the caret on screen; the repaint itself comes from the observer.
        ui.editor.follow_cursor(ui.visible_lines());
        glib::Propagation::Stop
    });

    window.add_controller(keys);
}

fn wire_buttons(ui: &Rc<Ui>, window: &adw::ApplicationWindow) {
    let save_ui = ui.clone();
    ui.save_button.connect_clicked(move |_| {
        save_ui.save();
        save_ui.refresh();
    });

    let undo_ui = ui.clone();
    ui.undo_button.connect_clicked(move |_| {
        undo_ui.editor.undo();
        undo_ui.editor.follow_cursor(undo_ui.visible_lines());
    });

    let redo_ui = ui.clone();
    ui.redo_button.connect_clicked(move |_| {
        redo_ui.editor.redo();
        redo_ui.editor.follow_cursor(redo_ui.visible_lines());
    });

    let _ = window;
}

fn wire_scrollbar(ui: &Rc<Ui>, adjustment: &gtk::Adjustment) {
    let ui = ui.clone();
    adjustment.connect_value_changed(move |adjustment| {
        // Ignore the writes `refresh` just made, or the two would chase each other.
        if ui.syncing.get() {
            return;
        }
        ui.editor.set_scroll_offset(adjustment.value() as u64);
    });
}

/// GNOME HIG: never discard work silently — ask, with Save as the default.
fn wire_close_request(ui: &Rc<Ui>, window: &adw::ApplicationWindow) {
    let ui = ui.clone();
    window.connect_close_request(move |window| {
        if ui.closing.get() || !ui.editor.is_dirty() {
            return glib::Propagation::Proceed;
        }

        let dialog = adw::AlertDialog::new(
            Some("Save changes before closing?"),
            Some("Your changes will be lost if you don't save them."),
        );
        dialog.add_responses(&[
            ("cancel", "Cancel"),
            ("discard", "Discard"),
            ("save", "Save"),
        ]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");

        let response_ui = ui.clone();
        let response_window = window.clone();
        dialog.connect_response(None, move |_, response| match response {
            "discard" => {
                response_ui.closing.set(true);
                response_window.close();
            }
            "save" => {
                // A failed save must not take the document down with it.
                if response_ui.save() {
                    response_ui.closing.set(true);
                    response_window.close();
                } else {
                    response_ui.refresh();
                }
            }
            _ => {}
        });

        dialog.present(Some(window));
        glib::Propagation::Stop
    });
}
