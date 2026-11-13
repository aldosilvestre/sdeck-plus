//! Organize is a compact command menu; New/Edit profile open dedicated modal dialogs.
use super::ui_refresh_tests::{fixture, Fixture};
use super::*;

fn descendants<T: IsA<gtk::Widget>>(widget: &gtk::Widget, found: &mut Vec<T>) {
    if let Some(item) = widget.downcast_ref::<T>() {
        found.push(item.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        descendants(&current, found);
    }
}

fn all<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    descendants(widget, &mut found);
    found
}

/// The first label of a button: its caption, even when a key hint follows it.
fn caption(button: &Button) -> String {
    all::<Label>(button.upcast_ref())
        .first()
        .map(|label| label.text().to_string())
        .or_else(|| button.label().map(|label| label.to_string()))
        .unwrap_or_default()
}

fn named(root: &gtk::Widget, name: &str) -> Button {
    all::<Button>(root)
        .into_iter()
        .find(|button| caption(button) == name)
        .unwrap_or_else(|| panic!("no {name} button"))
}

/// A visible dialog of `parent` (earlier tests may leave theirs behind).
fn open_dialog(parent: &ApplicationWindow, title: &str) -> Option<gtk::Window> {
    gtk::Window::toplevels()
        .snapshot()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::Window>().ok())
        .find(|window| {
            window.is_visible()
                && window.title().as_deref() == Some(title)
                && window.transient_for().as_ref() == Some(parent.upcast_ref())
        })
}

fn open_window(fixture: &Fixture, title: &str) -> Option<gtk::Window> {
    open_dialog(&fixture.window, title)
}

/// Open the Organize menu and activate one of its rows, as a user would.
fn choose(fixture: &Fixture, command: &str) -> gtk::Window {
    let panel = fixture.widgets.management_menu.popover().unwrap();
    panel.set_autohide(false);
    fixture.widgets.management_menu.popup();
    named(panel.upcast_ref(), command).emit_clicked();
    assert!(!panel.is_visible(), "choosing a command closes the menu");
    open_window(fixture, command).unwrap_or_else(|| panic!("{command} opens its dialog"))
}

#[test]
fn organize_menu_lists_only_the_profile_commands() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("organize-menu", |_| {}) else {
            return;
        };
        let panel = fixture.widgets.management_menu.popover().unwrap();
        assert!(panel.has_css_class("command-menu"));
        let rows = all::<Button>(panel.upcast_ref());
        let names: Vec<_> = rows.iter().map(caption).collect();
        assert_eq!(names, ["Edit profile", "New profile"]);
        // Compact: no form fields, tabs or help inside the menu, and no invented shortcut hints.
        assert!(all::<Entry>(panel.upcast_ref()).is_empty());
        assert!(all::<gtk::Stack>(panel.upcast_ref()).is_empty());
        assert!(all::<gtk::DropDown>(panel.upcast_ref()).is_empty());
        assert!(all::<Label>(panel.upcast_ref())
            .iter()
            .all(|label| !label.has_css_class("key-hint")));
        assert!(rows.iter().all(|row| row.has_css_class("command-row")));
        assert!(
            open_window(&fixture, "New profile").is_none()
                && open_window(&fixture, "Edit profile").is_none()
        );
    });
}

#[test]
fn new_profile_opens_a_transient_modal_that_cancel_leaves_untouched() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("organize-new-cancel", |_| {}) else {
            return;
        };
        let before = fixture.layers.borrow().names();
        let dialog = choose(&fixture, "New profile");
        assert!(dialog.is_modal());
        assert_eq!(
            dialog.transient_for().as_ref().map(|w| w.upcast_ref()),
            Some(fixture.window.upcast_ref::<gtk::Window>())
        );
        let entries = all::<Entry>(dialog.upcast_ref());
        assert_eq!(
            entries[0].placeholder_text().as_deref(),
            Some("New profile name")
        );
        entries[0].set_text("Draft only");
        let cancel = named(dialog.upcast_ref(), "Cancel");
        assert!(all::<Label>(cancel.upcast_ref())
            .iter()
            .any(|label| label.text() == "Esc"));
        cancel.emit_clicked();
        assert!(open_window(&fixture, "New profile").is_none());
        assert_eq!(fixture.layers.borrow().names(), before);
        // The discarded draft does not reappear, and creation is disabled again.
        let again = choose(&fixture, "New profile");
        assert_eq!(all::<Entry>(again.upcast_ref())[0].text(), "");
        assert!(!named(again.upcast_ref(), "Create profile").is_sensitive());
    });
}

#[test]
fn new_profile_dialog_keeps_validation_and_creates_then_edits() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("organize-new-create", |_| {}) else {
            return;
        };
        let dialog = choose(&fixture, "New profile");
        let name = all::<Entry>(dialog.upcast_ref()).remove(0);
        let create = named(dialog.upcast_ref(), "Create profile");
        assert!(!create.is_sensitive(), "empty names are refused");
        name.set_text("Base");
        assert!(!create.is_sensitive(), "duplicates are refused");
        let error = all::<Label>(dialog.upcast_ref())
            .into_iter()
            .find(|label| label.has_css_class("feedback-error"))
            .expect("the duplicate is explained inside the dialog");
        assert_eq!(error.text(), "A profile named Base already exists.");
        assert!(error.is_visible());
        name.set_text("Created in dialog");
        assert!(create.is_sensitive());
        create.emit_clicked();
        assert!(fixture
            .layers
            .borrow()
            .layers()
            .contains_key("Created in dialog"));
        assert_eq!(fixture.layers.borrow().active(), "Created in dialog");
        // Creation returns to editing the new profile.
        assert!(open_window(&fixture, "New profile").is_none());
        assert!(open_window(&fixture, "Edit profile").is_some());
    });
}

#[test]
fn edit_profile_opens_its_form_in_a_modal_and_close_changes_nothing() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("organize-edit", |_| {}) else {
            return;
        };
        let before = fixture.layers.borrow().names();
        let dialog = choose(&fixture, "Edit profile");
        assert!(dialog.is_modal() && dialog.transient_for().is_some());
        let save = named(dialog.upcast_ref(), "Save changes");
        let delete = named(dialog.upcast_ref(), "Delete profile");
        assert!(save.has_css_class("suggested-action"));
        assert!(delete.has_css_class("destructive-action"));
        assert!(!all::<gtk::DropDown>(dialog.upcast_ref()).is_empty());
        named(dialog.upcast_ref(), "Close").emit_clicked();
        assert!(open_window(&fixture, "Edit profile").is_none());
        assert_eq!(fixture.layers.borrow().names(), before);
    });
}

/// The profile manager end to end: command rows, both dialogs, validation, Cancel,
/// creation and deletion through the real controls, in both themes and two sizes.
pub(super) fn assert_compact_profile_manager() {
    let (root, mut store, artwork_store, _) = artwork_fixture("profile-manager");
    if !store.layers().contains_key("Studio") {
        store.create("Studio").unwrap();
    }
    let layers = Rc::new(RefCell::new(store));
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(700)
        .build();
    install_style(&window);
    let (output, _hardware) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(artwork_store))),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output,
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(Default::default())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
        info: Rc::default(),
    };
    let (sender, commands) = mpsc::channel();
    let widgets = build_widgets(window.clone(), layers.clone(), artwork, sender);
    let stage = gtk::Fixed::new();
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    let panel = widgets.management_menu.popover().unwrap();
    panel.set_autohide(false);
    let open = |caption: &str| {
        widgets.management_menu.popup();
        named(panel.upcast_ref(), caption).emit_clicked();
        open_dialog(&window, caption).unwrap_or_else(|| panic!("{caption} dialog opens"))
    };
    let save = |dialog: &gtk::Window, name: &str| {
        if let Some(dir) = std::env::var_os("STREAMDECK_PROFILES_QA_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            wait_for_frame(dialog);
            rendered_texture(dialog, &dialog.child().unwrap())
                .save_to_png(dir.join(format!("{name}.png")))
                .unwrap();
        }
    };
    let manager = widgets.feature_runtime.management();
    let settings = gtk::Settings::default().unwrap();
    let original_theme = settings.gtk_theme_name();
    let original_dark = settings.is_gtk_application_prefer_dark_theme();
    for (theme, dark) in [("light", false), ("dark", true)] {
        settings.set_gtk_theme_name(Some(if dark { "Adwaita-dark" } else { "Adwaita" }));
        settings.set_gtk_application_prefer_dark_theme(dark);
        for (width, height) in [(640, 480), (1000, 700)] {
            widgets.root.set_size_request(width, height);
            widgets.root.allocate(width, height, -1, None);
            wait_for_frame(&window);
            let tag = format!("{theme}-{width}x{height}");
            // Given: an unsaved association in the Edit dialog.
            let edit = open("Edit profile");
            save(&edit, &format!("{tag}-edit"));
            let app = all::<Entry>(manager.upcast_ref()).remove(0);
            app.set_text("unsaved-app");
            // When: the New dialog rejects a duplicate and is cancelled.
            let creation = open("New profile");
            save(&creation, &format!("{tag}-new"));
            let name = all::<Entry>(creation.upcast_ref()).remove(0);
            let create = named(creation.upcast_ref(), "Create profile");
            name.set_text("Base");
            save(&creation, &format!("{tag}-invalid-new"));
            assert!(!create.is_sensitive() && creation.is_visible());
            named(creation.upcast_ref(), "Cancel").emit_clicked();
            assert!(!creation.is_visible());
            // Then: nothing was created and the Edit draft is unchanged.
            assert_eq!(layers.borrow().names(), ["Base", "Studio"]);
            assert_eq!(app.text(), "unsaved-app");
            app.set_text("");
            widgets
                .feature_runtime
                .report_profile_save("Could not save profile: storage is read-only.", true);
            save(&edit, &format!("{tag}-save-error"));
            edit.close();
        }
    }
    // Given: a valid name in the real creation form.
    let creation = open("New profile");
    all::<Entry>(creation.upcast_ref())[0].set_text("Created from manager");
    // When: create using its native button.
    named(creation.upcast_ref(), "Create profile").emit_clicked();
    // Then: creation returns to editing that profile and enqueues a real save.
    assert!(!creation.is_visible());
    let edit = open_dialog(&window, "Edit profile").expect("creation returns to editing");
    assert_eq!(layers.borrow().active(), "Created from manager");
    assert!(commands
        .try_iter()
        .any(|command| matches!(command, LayerCommand::SaveProfile(_, _))));
    let delete = named(edit.upcast_ref(), "Delete profile");
    delete.emit_clicked();
    current_dialog().response(ResponseType::Cancel);
    assert!(layers
        .borrow()
        .layers()
        .contains_key("Created from manager"));
    delete.emit_clicked();
    current_dialog().response(ResponseType::Accept);
    assert!(!layers
        .borrow()
        .layers()
        .contains_key("Created from manager"));
    settings.set_gtk_theme_name(original_theme.as_deref());
    settings.set_gtk_application_prefer_dark_theme(original_dark);
    window.destroy();
    std::fs::remove_dir_all(root).unwrap();
    eprintln!("GTK compact profile manager verified: command menu, modal dialogs, validation, cancel, creation and deletion");
}
