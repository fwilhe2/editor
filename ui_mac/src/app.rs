// SPDX-FileCopyrightText: 2026 Florian Wilhelm <fwilhelm.wgt+github@gmail.com>
//
// SPDX-License-Identifier: MIT

//! The application: `NSApplication`, its delegate, the window, and the menu bar built from
//! [`crate::menu`].
//!
//! One document per process, as every other shell here: closing the window quits, and Quit asks
//! about unsaved changes first.

use std::cell::OnceCell;
use std::ffi::CString;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSBackingStoreType, NSEventModifierFlags, NSMenu, NSMenuItem,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use quill_core::App;

use crate::menu::{self, Action, Item, MENUS, Role};
use crate::view::{self, QuillView};

pub struct Launch {
    app: Arc<App>,
    view: OnceCell<Retained<QuillView>>,
    window: OnceCell<Retained<NSWindow>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "QuillDelegate"]
    #[ivars = Launch]
    pub struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            self.open_window();
        }

        /// One window, one document: closing it is quitting.
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _app: &NSApplication) -> bool {
            true
        }

        /// ⌘Q with unsaved changes asks first, as closing the window does.
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            if view::may_discard(&self.ivars().app, self.mtm()) {
                NSApplicationTerminateReply::TerminateNow
            } else {
                NSApplicationTerminateReply::TerminateCancel
            }
        }
    }

    unsafe impl NSWindowDelegate for Delegate {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _window: &NSWindow) -> bool {
            let may = view::may_discard(&self.ivars().app, self.mtm());
            if may {
                // Answered once: the terminate question that follows the last window closing
                // must not ask again about changes the person just discarded.
                self.ivars().app.new_document();
            }
            may
        }
    }
);

impl Delegate {
    fn new(app: Arc<App>, mtm: MainThreadMarker) -> Retained<Delegate> {
        let this = Delegate::alloc(mtm).set_ivars(Launch {
            app,
            view: OnceCell::new(),
            window: OnceCell::new(),
        });
        // SAFETY: `init` is `NSObject`'s designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    fn open_window(&self) {
        let mtm = self.mtm();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(820.0, 600.0));
        // SAFETY: a titled, buffered window is the ordinary kind.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: the window is owned from Rust, so AppKit must not also release it on close —
        // objc2's documented rule for windows created this way.
        unsafe { window.setReleasedWhenClosed(false) };
        let view = QuillView::new(self.ivars().app.clone(), mtm);
        window.setContentView(Some(&view));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        window.makeFirstResponder(Some(&view));
        window.center();
        view.refresh();
        window.makeKeyAndOrderFront(None);
        let _ = self.ivars().view.set(view);
        let _ = self.ivars().window.set(window);
    }
}

fn selector(name: &str) -> Sel {
    Sel::register(&CString::new(name).expect("a selector has no NUL in it"))
}

/// One of [`MENUS`] as an `NSMenu`.
fn build(menu: &menu::Menu, app: &NSApplication, mtm: MainThreadMarker) -> Retained<NSMenu> {
    let built = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(menu.title));
    for item in menu.items {
        let made = match item {
            Item::Separator => NSMenuItem::separatorItem(mtm),
            Item::Entry { title, key, action } => {
                let (name, tag) = match action {
                    Action::Standard(name) => (*name, 0),
                    Action::Command(command) => (menu::selector(*command), menu::tag(*command)),
                };
                // SAFETY: every selector here names an action method taking one sender.
                let made = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(title),
                        Some(selector(name)),
                        &NSString::from_str(key.map_or("", |k| k.key)),
                    )
                };
                if let Some(key) = key {
                    let mut mask = NSEventModifierFlags::Command;
                    if key.option {
                        mask |= NSEventModifierFlags::Option;
                    }
                    made.setKeyEquivalentModifierMask(mask);
                }
                made.setTag(tag);
                made
            }
        };
        built.addItem(&made);
    }
    if menu.role == Role::Window {
        app.setWindowsMenu(Some(&built));
    }
    built
}

/// Run the application until it quits.
pub fn run(app: Arc<App>) -> std::process::ExitCode {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("quill-mac: AppKit runs on the main thread only");
        return std::process::ExitCode::FAILURE;
    };
    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let bar = NSMenu::new(mtm);
    for menu in &MENUS {
        let top = NSMenuItem::new(mtm);
        top.setTitle(&NSString::from_str(menu.title));
        top.setSubmenu(Some(&build(menu, &application, mtm)));
        bar.addItem(&top);
    }
    application.setMainMenu(Some(&bar));
    let delegate = Delegate::new(app, mtm);
    application.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    application.run();
    std::process::ExitCode::SUCCESS
}
