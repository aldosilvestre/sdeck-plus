//! Verify clipboard text delivery into a task-owned GTK window on Wayland.
use gtk::{glib, prelude::*, Application, ApplicationWindow, Entry};
use sdeck_plus::{
    desktop_actions::{DesktopAction, DesktopRuntime},
    keyboard::Keyboard,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

fn main() -> glib::ExitCode {
    let app = Application::builder()
        .application_id("dev.streamdeck.DesktopVerification")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let passed = Rc::new(Cell::new(false));
    let result = passed.clone();
    app.connect_activate(move |app| {
        let expected = "Stream Deck QA \u{65e5}\u{672c}\u{8a9e}";
        let entry = Entry::builder()
            .placeholder_text("Clipboard verification target")
            .hexpand(true)
            .build();
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Stream Deck desktop verification")
            .default_width(540)
            .default_height(120)
            .child(&entry)
            .build();
        let completed = result.clone();
        let application = app.clone();
        entry.connect_changed(move |entry| {
            if entry.text() == expected {
                completed.set(true);
                println!("TEXT_OK: exact Unicode text received by the GTK target");
                application.quit();
            }
        });
        let triggered = Rc::new(Cell::new(false));
        let runtime = Rc::new(RefCell::new(DesktopRuntime::new()));
        let keyboard = Rc::new(RefCell::new(Keyboard::new()));
        let target = entry.downgrade();
        let owner = window.downgrade();
        let application = app.clone();
        let trigger: Rc<dyn Fn()> = Rc::new(move || {
            let (Some(window), Some(target)) = (owner.upgrade(), target.upgrade()) else {
                return;
            };
            let focused = gtk::prelude::GtkWindowExt::focus(&window).is_some_and(|focus| {
                focus == target.clone().upcast::<gtk::Widget>() || focus.is_ancestor(&target)
            });
            if window.is_active() && focused && !triggered.replace(true) {
                println!("TEXT_TARGET_READY: owned GTK entry has compositor focus");
                let action = DesktopAction::Text {
                    value: expected.into(),
                    enter: false,
                };
                if let Err(error) = runtime
                    .borrow_mut()
                    .execute(&action, &mut *keyboard.borrow_mut())
                {
                    eprintln!("TEXT_ERROR: {error}");
                    application.quit();
                }
            }
        });
        let callback = trigger.clone();
        window.connect_is_active_notify(move |_| {
            let callback = callback.clone();
            glib::idle_add_local_once(move || callback());
        });
        window.connect_focus_widget_notify(move |_| {
            let callback = trigger.clone();
            glib::idle_add_local_once(move || callback());
        });
        let application = app.clone();
        glib::timeout_add_local_once(Duration::from_secs(8), move || {
            eprintln!("TEXT_TIMEOUT: no exact paste observed in the verification window");
            application.quit();
        });
        entry.grab_focus();
        window.present();
    });
    app.run_with_args::<&str>(&[]);
    if passed.get() {
        glib::ExitCode::SUCCESS
    } else {
        glib::ExitCode::FAILURE
    }
}
