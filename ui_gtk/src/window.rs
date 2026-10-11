// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The window: libadwaita chrome around a widget that draws the document itself.
//!
//! **The document is drawn, not held.** There is no `GtkTextView` here: a text widget owns a
//! buffer of its own, and the moment one exists there are two documents that disagree. The
//! `gtk::DrawingArea` below paints [`App::view`] on every frame and forwards every key, click
//! and scroll to the core; the input method's committed text goes to [`App::type_text`].
//!
//! GNOME's conventions, from the HIG rather than from a sibling shell: a header bar with a
//! primary menu instead of a menu bar, a search bar that slides down under it, toasts for
//! feedback, an `AdwAlertDialog` before discarding work with Save as the suggested answer, and
//! GNOME's own accelerators ([`crate::menu`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use libadwaita as adw;
use libadwaita::gtk::{self, gdk, gio, glib};
use quill_core::{App, Command, Observer, Search};

use adw::prelude::*;

use crate::keymap::{self, Action};
use crate::menu;
use crate::paint::{self, Palette, PangoMetrics};

/// Carries the core's notifications onto the GTK main loop.
///
/// [`Observer`] is `Send + Sync` and GTK objects are neither, so the observer cannot touch the
/// window. It sends on a channel a task on the main context drains — and drains *all* of, so
/// three notifications from one keystroke are one repaint.
struct Notifier(async_channel::Sender<()>);

impl Observer for Notifier {
    fn changed(&self) {
        let _ = self.0.send_blocking(());
    }
}

/// Everything the callbacks share: widgets and the core, and no document state.
pub struct Ui {
    app: Arc<App>,
    window: adw::ApplicationWindow,
    area: gtk::DrawingArea,
    title: adw::WindowTitle,
    toasts: adw::ToastOverlay,
    search_bar: gtk::SearchBar,
    search_entry: gtk::SearchEntry,
    adjustment: gtk::Adjustment,
    im: gtk::IMMulticontext,
    /// Built from the widget's own Pango context on first use.
    metrics: RefCell<Option<PangoMetrics>>,
    /// True while `refresh` writes the scrollbar, so its signal does not echo into the core.
    syncing: Cell<bool>,
    /// Set once the person has answered the unsaved-changes question.
    closing: Cell<bool>,
    /// Fractional wheel movement not yet a whole row — a touchpad scrolls in small steps.
    wheel: Cell<f64>,
}

impl Ui {
    fn metrics(&self) -> std::cell::Ref<'_, PangoMetrics> {
        if self.metrics.borrow().is_none() {
            *self.metrics.borrow_mut() = Some(PangoMetrics::new(&self.area.pango_context()));
        }
        std::cell::Ref::map(self.metrics.borrow(), |m| m.as_ref().unwrap())
    }

    /// Run `f` against the frame the widget shows now.
    fn with_frame<T>(&self, f: impl FnOnce(&quill_core::Frame) -> T) -> T {
        let metrics = self.metrics();
        let frame = paint::frame(
            &metrics,
            &self.app,
            f64::from(self.area.width()),
            f64::from(self.area.height()),
        );
        f(&frame)
    }

    /// After input: bring the caret into view. The repaint comes from the observer.
    fn after_input(&self) {
        self.with_frame(|frame| self.app.follow_caret(frame));
    }

    fn origin(&self) -> (f64, f64) {
        paint::text_origin(&self.metrics(), &self.app)
    }

    /// Bring every widget into line with the core, and schedule a repaint.
    fn refresh(&self) {
        let name = self.app.title();
        let dirty = self.app.is_dirty();
        self.title.set_title(&if dirty {
            format!("• {name}")
        } else {
            name.clone()
        });
        let folder = self
            .app
            .path()
            .and_then(|p| p.parent().map(|d| d.display().to_string()))
            .unwrap_or_default();
        self.title.set_subtitle(&folder);
        self.window
            .set_title(Some(&format!("{name} — {}", quill_core::APP_NAME)));

        let has_selection = self.app.selection().is_some();
        for (command, enabled) in [
            (Command::Undo, self.app.can_undo()),
            (Command::Redo, self.app.can_redo()),
            (Command::Cut, has_selection),
            (Command::Copy, has_selection),
        ] {
            if let Some(action) = self.window.lookup_action(&menu::action(command))
                && let Ok(action) = action.downcast::<gio::SimpleAction>()
            {
                action.set_enabled(enabled);
            }
        }

        let (rows, view) = self.with_frame(|frame| (frame.rows(), self.app.view(frame)));
        self.syncing.set(true);
        let lines = self.app.line_count() as f64;
        let page = rows as f64;
        self.adjustment.configure(
            view.scroll.line as f64,
            0.0,
            lines - 1.0 + page,
            1.0,
            page,
            page,
        );
        self.syncing.set(false);

        // Tell the input method where the caret is, so a candidate window opens beside it.
        if let Some(caret) = view.caret {
            let (x, y) = self.origin();
            self.im.set_cursor_location(&gdk::Rectangle::new(
                (x + f64::from(caret.x)) as i32,
                (y + f64::from(caret.top)) as i32,
                1,
                caret.height as i32,
            ));
        }
        self.area.queue_draw();
    }

    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    /// Carry out a menu verb.
    fn run(self: &Rc<Self>, command: Command) {
        let ui = self.clone();
        match command {
            Command::New => drop(glib::spawn_future_local(async move {
                if ui.may_discard().await {
                    ui.app.new_document();
                }
            })),
            Command::Open => drop(glib::spawn_future_local(async move {
                if ui.may_discard().await {
                    ui.open().await;
                }
            })),
            Command::Save => drop(glib::spawn_future_local(async move {
                ui.save().await;
            })),
            Command::SaveAs => drop(glib::spawn_future_local(async move {
                ui.save_as().await;
            })),
            Command::Paste => drop(glib::spawn_future_local(async move {
                if let Ok(Some(text)) = ui.area.clipboard().read_text_future().await {
                    ui.app.insert_text(&text);
                    ui.after_input();
                }
            })),
            _ => self.run_now(command),
        }
    }

    /// The verbs that need no dialog.
    fn run_now(&self, command: Command) {
        match command {
            Command::Close => self.window.close(),
            Command::Undo => {
                self.app.undo();
            }
            Command::Redo => {
                self.app.redo();
            }
            Command::Cut => {
                let text = self.app.cut();
                if !text.is_empty() {
                    self.area.clipboard().set_text(&text);
                }
            }
            Command::Copy => {
                let text = self.app.selected_text();
                if !text.is_empty() {
                    self.area.clipboard().set_text(&text);
                }
            }
            Command::SelectAll => self.app.select_all(),
            Command::Find => {
                self.search_bar.set_search_mode(true);
                if let Some(query) = self.app.search() {
                    self.search_entry.set_text(&query);
                }
                self.search_entry.grab_focus();
            }
            Command::FindNext | Command::FindPrevious => {
                if self.app.search().is_none() {
                    return self.run_now(Command::Find);
                }
                let direction = if command == Command::FindNext {
                    Search::Forward
                } else {
                    Search::Backward
                };
                if !self.app.find_again(direction) {
                    self.toast("No match");
                }
            }
            Command::About => self.about(),
            Command::New | Command::Open | Command::Save | Command::SaveAs | Command::Paste => {
                unreachable!("{} needs a dialog", command.id())
            }
        }
        self.after_input();
    }

    /// GNOME HIG: never discard work silently. True when the document may go.
    async fn may_discard(self: &Rc<Self>) -> bool {
        if !self.app.is_dirty() {
            return true;
        }
        let dialog = adw::AlertDialog::new(
            Some("Save Changes?"),
            Some(&format!(
                "“{}” has unsaved changes. Changes which are not saved will be permanently lost.",
                self.app.title()
            )),
        );
        dialog.add_responses(&[
            ("cancel", "_Cancel"),
            ("discard", "_Discard"),
            ("save", "_Save"),
        ]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        match dialog.choose_future(&self.window).await.as_str() {
            "discard" => true,
            "save" => self.save().await,
            _ => false,
        }
    }

    async fn open(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder().title("Open").modal(true).build();
        let Ok(file) = dialog.open_future(Some(&self.window)).await else {
            return;
        };
        if let Some(path) = file.path()
            && let Err(error) = self.app.open_file(&path)
        {
            self.toast(&error.to_string());
        }
    }

    /// Save where the document came from, or ask where. True when it was saved.
    async fn save(self: &Rc<Self>) -> bool {
        if self.app.path().is_none() {
            return self.save_as().await;
        }
        match self.app.save_file() {
            Ok(()) => true,
            Err(error) => {
                self.toast(&error.to_string());
                false
            }
        }
    }

    async fn save_as(self: &Rc<Self>) -> bool {
        let dialog = gtk::FileDialog::builder()
            .title("Save As")
            .modal(true)
            .initial_name(self.app.title())
            .build();
        let Ok(file) = dialog.save_future(Some(&self.window)).await else {
            return false;
        };
        let Some(path) = file.path() else {
            return false;
        };
        match self.app.save_file_as(&path) {
            Ok(()) => true,
            Err(error) => {
                self.toast(&error.to_string());
                false
            }
        }
    }

    fn about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name(quill_core::APP_NAME)
            .application_icon(quill_core::APP_ID)
            .version(quill_core::VERSION)
            .license_type(gtk::License::MitX11)
            .comments("A shared core with native shells")
            .build();
        about.present(Some(&self.window));
    }
}

/// Build the window around `app` and show it.
pub fn build(application: &adw::Application, app: Arc<App>) {
    let area = gtk::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .focusable(true)
        .build();
    let adjustment = gtk::Adjustment::new(0.0, 0.0, 1.0, 1.0, 1.0, 1.0);
    let scrollbar = gtk::Scrollbar::new(gtk::Orientation::Vertical, Some(&adjustment));
    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.append(&area);
    body.append(&scrollbar);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&body));

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Find")
        .width_chars(30)
        .build();
    let search_bar = gtk::SearchBar::builder().child(&search_entry).build();
    search_bar.connect_entry(&search_entry);

    let title = adw::WindowTitle::new(quill_core::UNTITLED, "");
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));
    let open = gtk::Button::builder()
        .label("Open")
        .action_name(format!("win.{}", menu::action(Command::Open)))
        .tooltip_text("Open a File (Ctrl+O)")
        .build();
    header.pack_start(&open);
    let primary = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main Menu")
        .menu_model(&primary_menu())
        .primary(true)
        .build();
    header.pack_end(&primary);
    let save = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .action_name(format!("win.{}", menu::action(Command::Save)))
        .tooltip_text("Save (Ctrl+S)")
        .build();
    header.pack_end(&save);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&search_bar);
    toolbar.set_content(Some(&toasts));

    let window = adw::ApplicationWindow::builder()
        .application(application)
        .default_width(820)
        .default_height(620)
        .content(&toolbar)
        .build();

    let im = gtk::IMMulticontext::new();
    im.set_client_widget(Some(&area));

    // The widgets live as long as the window, and so does this; the closures below holding it
    // are a cycle that lasts exactly the life of the process.
    let ui = Rc::new(Ui {
        app: app.clone(),
        window: window.clone(),
        area: area.clone(),
        title,
        toasts,
        search_bar,
        search_entry,
        adjustment,
        im,
        metrics: RefCell::new(None),
        syncing: Cell::new(false),
        closing: Cell::new(false),
        wheel: Cell::new(0.0),
    });

    wire_actions(&ui, application);
    wire_drawing(&ui);
    wire_keys(&ui);
    wire_pointer(&ui);
    wire_search(&ui);
    wire_notifications(&ui);
    wire_close(&ui);

    window.present();
    area.grab_focus();
    ui.refresh();
}

/// The primary menu, from [`menu::SECTIONS`].
fn primary_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    for section in menu::SECTIONS {
        let part = gio::Menu::new();
        for &command in section {
            part.append(
                Some(&menu::label(command)),
                Some(&format!("win.{}", menu::action(command))),
            );
        }
        menu.append_section(None, &part);
    }
    menu
}

fn wire_actions(ui: &Rc<Ui>, application: &adw::Application) {
    for command in Command::ALL {
        let action = gio::SimpleAction::new(&menu::action(command), None);
        let handler = ui.clone();
        action.connect_activate(move |_, _| handler.run(command));
        ui.window.add_action(&action);
        application.set_accels_for_action(
            &format!("win.{}", menu::action(command)),
            menu::accels(command),
        );
    }
}

fn wire_drawing(ui: &Rc<Ui>) {
    let painter = ui.clone();
    ui.area.set_draw_func(move |area, cr, width, height| {
        let palette = if adw::StyleManager::default().is_dark() {
            Palette::dark()
        } else {
            Palette::light()
        };
        let metrics = painter.metrics();
        paint::paint(
            cr,
            &metrics,
            &painter.app,
            f64::from(width),
            f64::from(height),
            &palette,
            area.has_focus(),
        );
    });
    // A resize changes where rows wrap, and the font may change with the system setting.
    let resized = ui.clone();
    ui.area.connect_resize(move |_, _, _| resized.refresh());
    let dark = ui.clone();
    adw::StyleManager::default().connect_dark_notify(move |_| dark.area.queue_draw());

    let scrolled = ui.clone();
    ui.adjustment.connect_value_changed(move |adjustment| {
        if !scrolled.syncing.get() {
            scrolled.app.scroll_to(adjustment.value() as u64);
        }
    });
}

fn wire_keys(ui: &Rc<Ui>) {
    let keys = gtk::EventControllerKey::new();
    keys.set_im_context(Some(&ui.im));
    let handler = ui.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let Some(action) = keymap::action_for(key, mods) else {
            return glib::Propagation::Proceed;
        };
        let app = &handler.app;
        match action {
            Action::Move(motion, extend) => {
                handler.with_frame(|frame| app.move_caret(motion, extend, frame));
            }
            Action::Delete(kind) => {
                app.delete(kind);
            }
            Action::Type(c) => app.type_text(c.encode_utf8(&mut [0; 4])),
            Action::Escape => {
                app.clear_selection();
            }
        }
        handler.after_input();
        glib::Propagation::Stop
    });
    ui.area.add_controller(keys);

    let typed = ui.clone();
    ui.im.connect_commit(move |_, text| {
        typed.app.type_text(text);
        typed.after_input();
    });

    let focus = gtk::EventControllerFocus::new();
    let entered = ui.clone();
    focus.connect_enter(move |_| {
        entered.im.focus_in();
        entered.area.queue_draw();
    });
    let left = ui.clone();
    focus.connect_leave(move |_| {
        left.im.focus_out();
        left.area.queue_draw();
    });
    ui.area.add_controller(focus);
}

fn wire_pointer(ui: &Rc<Ui>) {
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);
    let clicked = ui.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        clicked.area.grab_focus();
        let extend = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);
        let (left, top) = clicked.origin();
        clicked.with_frame(|frame| {
            clicked
                .app
                .place_caret(frame, (x - left) as f32, (y - top) as f32, extend)
        });
    });
    ui.area.add_controller(click);

    let drag = gtk::GestureDrag::new();
    let dragged = ui.clone();
    drag.connect_drag_update(move |gesture, dx, dy| {
        let Some((x, y)) = gesture.start_point() else {
            return;
        };
        let (left, top) = dragged.origin();
        dragged.with_frame(|frame| {
            let (x, y) = ((x + dx - left) as f32, (y + dy - top) as f32);
            dragged.app.place_caret(frame, x, y, true);
            dragged.app.follow_caret(frame);
        });
    });
    ui.area.add_controller(drag);

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let wheel = ui.clone();
    scroll.connect_scroll(move |_, _, dy| {
        // One notch of a wheel is 1.0; three rows per notch, as GTK's own text views scroll.
        let total = wheel.wheel.get() + dy * 3.0;
        let rows = total.trunc();
        wheel.wheel.set(total - rows);
        if rows != 0.0 {
            wheel.with_frame(|frame| wheel.app.scroll_by(frame, rows as i64));
        }
        glib::Propagation::Stop
    });
    ui.area.add_controller(scroll);
}

fn wire_search(ui: &Rc<Ui>) {
    let activated = ui.clone();
    ui.search_entry.connect_activate(move |entry| {
        if !activated.app.find(&entry.text(), Search::Forward) {
            activated.toast("No match");
        }
        activated.after_input();
    });
    let stopped = ui.clone();
    ui.search_entry.connect_stop_search(move |_| {
        stopped.search_bar.set_search_mode(false);
        stopped.area.grab_focus();
    });
}

fn wire_notifications(ui: &Rc<Ui>) {
    let (sender, receiver) = async_channel::unbounded();
    ui.app.observe(Arc::new(Notifier(sender)));
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        while receiver.recv().await.is_ok() {
            while receiver.try_recv().is_ok() {}
            ui.refresh();
        }
    });
}

fn wire_close(ui: &Rc<Ui>) {
    let ui = ui.clone();
    ui.window.clone().connect_close_request(move |window| {
        if ui.closing.get() || !ui.app.is_dirty() {
            return glib::Propagation::Proceed;
        }
        let asking = ui.clone();
        let window = window.clone();
        glib::spawn_future_local(async move {
            if asking.may_discard().await {
                asking.closing.set(true);
                window.close();
            }
        });
        glib::Propagation::Stop
    });
}
