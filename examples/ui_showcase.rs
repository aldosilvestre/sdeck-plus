//! Render shared native primitives without opening a device or running actions.
#[path = "../src/ui.rs"]
mod ui;

use gtk::{glib, prelude::*, Application, ApplicationWindow, Box as GtkBox, Orientation};
use std::{cell::Cell, path::PathBuf, rc::Rc};

fn main() -> glib::ExitCode {
    let directory = std::env::args_os().nth(1).map(PathBuf::from);
    if let Some(directory) = &directory {
        if let Err(error) = std::fs::create_dir_all(directory) {
            eprintln!("Could not create capture directory: {error}");
            return glib::ExitCode::FAILURE;
        }
    }
    let app = Application::builder()
        .application_id("dev.streamdeck.PrimitiveShowcase")
        .build();
    let failed = Rc::new(Cell::new(false));
    let quit = gtk::gio::SimpleAction::new("quit", None);
    let weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            app.quit();
        }
    });
    app.add_action(&quit);
    let failure = failed.clone();
    app.connect_activate(move |app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Configuration primitives")
            .default_width(760)
            .default_height(540)
            .build();
        let provider = gtk::CssProvider::new();
        let css_failure = failure.clone();
        provider.connect_parsing_error(move |_, _, error| {
            css_failure.set(true);
            eprintln!("CSS parsing failed: {error}");
        });
        provider.load_from_data(include_str!("../src/ui.css"));
        gtk::style_context_add_provider_for_display(
            &gtk::prelude::WidgetExt::display(&window),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let content = GtkBox::new(Orientation::Vertical, 16);
        content.set_margin_top(24);
        content.set_margin_bottom(24);
        content.set_margin_start(24);
        content.set_margin_end(24);
        content.add_css_class("app-shell");
        content.append(&ui::section_heading("input-gaming-symbolic", "Configure a control"));
        content.append(&ui::helper("Choose an action, fill in its fields, and save. Testing is explicit."));
        let actions = GtkBox::new(Orientation::Horizontal, 8);
        let save = ui::button("document-save-symbolic", "Save binding");
        save.add_css_class("suggested-action");
        actions.append(&save);
        actions.append(&ui::button("media-playback-start-symbolic", "Test action"));
        let remove = ui::button("edit-delete-symbolic", "Remove binding");
        remove.add_css_class("destructive-action");
        actions.append(&remove);
        content.append(&actions);
        let disabled = ui::button("folder-new-symbolic", "New folder");
        disabled.set_sensitive(false);
        content.append(&disabled);
        let field = gtk::Entry::builder()
            .placeholder_text("/home/aldo/Music/notification.ogg")
            .build();
        let label = gtk::Label::new(Some("Audio file"));
        label.set_xalign(0.0);
        label.set_mnemonic_widget(Some(&field));
        content.append(&label);
        content.append(&field);
        let success = ui::helper("Binding saved.");
        ui::feedback(&success, false);
        content.append(&success);
        let error = ui::helper("Choose an existing audio file before testing.");
        ui::feedback(&error, true);
        content.append(&error);
        let help = ui::popover("Setup help", &ui::helper("Select a key or dial on the device preview. Save stores the action; Test runs it immediately."));
        content.append(&ui::menu_button("help-browser-symbolic", "Setup help", &help));
        let toast = ui::Toast::new();
        let saved = toast.clone();
        save.connect_clicked(move |_| saved.show("Binding saved successfully.", false));
        window.set_child(Some(&toast.wrap(&content)));
        if let Some(directory) = directory.clone() {
        let app = app.clone();
        let failure = failure.clone();
        let captured = Rc::new(Cell::new(false));
        window.connect_map(move |window| {
            let Some(clock) = window.frame_clock() else {
                failure.set(true);
                app.quit();
                return;
            };
            let app = app.clone();
            let content = content.clone();
            let directory = directory.clone();
            let failure = failure.clone();
            let window = window.clone();
            let captured = captured.clone();
            clock.connect_after_paint(move |_| {
                if captured.replace(true) {
                    return;
                }
                let snapshot = gtk::Snapshot::new();
                gtk::WidgetPaintable::new(Some(&content)).snapshot(
                    &snapshot, f64::from(content.width()), f64::from(content.height()),
                );
                match (snapshot.to_node(), window.renderer()) {
                    (Some(node), Some(renderer)) => {
                        let texture = renderer.render_texture(&node, None);
                        if let Err(error) = texture.save_to_png(directory.join("primitives.png")) {
                            failure.set(true);
                            eprintln!("Could not save primitive capture: {error}");
                        }
                    }
                    _ => failure.set(true),
                }
                app.quit();
            });
        });
        }
        window.present();
    });
    let exit = app.run_with_args::<&str>(&[]);
    if failed.get() {
        glib::ExitCode::FAILURE
    } else {
        exit
    }
}
