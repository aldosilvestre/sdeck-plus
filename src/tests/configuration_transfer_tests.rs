use super::*;
use gtk::glib::translate::IntoGlib;
use sdeck_plus::{button_text::ButtonText, organization::Organization};

fn collect_menu_buttons(widget: &gtk::Widget, buttons: &mut Vec<Button>) {
    if let Some(button) = widget.downcast_ref::<Button>() {
        buttons.push(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        collect_menu_buttons(&widget, buttons);
        child = widget.next_sibling();
    }
}

#[test]
fn menu_transfers_configuration_and_reloads_without_running_actions() {
    let _guard = gtk_test_lock();
    if !init_gtk_for_test() {
        assert!(
            std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "GTK initialization failed despite an advertised display"
        );
        return;
    }
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let (sandbox, _, _, source) = artwork_fixture("configuration-transfer");
    let root = sandbox.join("data");
    let mut store = LayerStore::open_at(&root).unwrap();
    store.create("Studio").unwrap();
    let original = Action::Command("printf 'must not execute during import'".into());
    store
        .bind("Base", Trigger::KeyPress(1), original.clone())
        .unwrap();
    let mut organization = Organization::open_at(&root, &store).unwrap();
    organization
        .create_named_page(&mut store, "Imported page")
        .unwrap();
    store.select("Base");
    store.persist().unwrap();
    organization.persist().unwrap();
    let mut images = ArtworkStore::open_at_for_layer(&root, Some("Base")).unwrap();
    let image_path = images
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &source)
        .unwrap();
    let text = ButtonText {
        text: "Imported text".into(),
        ..ButtonText::default()
    };
    images
        .set_button_text_in_layer("Base", ArtworkTarget::Keypad(1), text.clone())
        .unwrap();
    let layers = Rc::new(RefCell::new(store));
    let (output, hardware) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(images))),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output,
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(Default::default())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
        info: Rc::default(),
    };
    let (commands, requests) = mpsc::channel();
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(950)
        .build();
    install_style(&window);
    let widgets = build_widgets(
        window.clone(),
        layers.clone(),
        artwork.clone(),
        commands.clone(),
    );
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(1000, 950);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);
    let runtime = &widgets.feature_runtime;
    assert!(runtime.complete_configuration_import(&root, Ok(None)));
    let menu = &widgets._configuration_menu;
    let panel = menu.button().popover().unwrap();
    menu.button().popup();
    wait_for_frame(&panel);
    let mut entries = Vec::new();
    collect_menu_buttons(panel.upcast_ref(), &mut entries);
    entries.retain(|button| button_caption(button).as_deref() != Some("Close"));
    assert_eq!(entries.len(), 3);
    menu.button().popdown();
    let click = |caption: &str| {
        entries
            .iter()
            .find(|button| button_caption(button).as_deref() == Some(caption))
            .unwrap()
            .emit_clicked();
    };
    let capture = |name: &str, widget: &gtk::Widget| {
        if let Some(directory) = std::env::var_os("STREAMDECK_CONFIG_QA_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            wait_for_frame(widget);
            rendered_texture(&window, widget)
                .save_to_png(std::path::Path::new(&directory).join(format!("{name}.png")))
                .unwrap();
        }
    };
    let capture_themes = |name: &str, widget: &gtk::Widget| {
        if std::env::var_os("STREAMDECK_CONFIG_QA_DIR").is_none() {
            return;
        }
        let settings = gtk::Settings::default().unwrap();
        let previous = settings.is_gtk_application_prefer_dark_theme();
        for (theme, dark) in [("light", false), ("dark", true)] {
            settings.set_gtk_application_prefer_dark_theme(dark);
            capture(&format!("{name}-{theme}"), widget);
        }
        settings.set_gtk_application_prefer_dark_theme(previous);
    };
    for (name, width) in [("wide", 1000), ("compact", 941)] {
        widgets.root.set_size_request(width, 950);
        window.set_default_size(width, 950);
        wait_for_frame(&window);
        capture_themes(&format!("window-{name}"), widgets.root.upcast_ref());
        menu.button().popup();
        capture_themes(&format!("menu-{name}"), panel.upcast_ref());
        menu.button().popdown();
    }
    click("Help");
    let help = menu.help_for_test();
    assert!(help.is_visible());
    capture_themes("help", help.upcast_ref());
    help.response(ResponseType::Close);
    assert!(!help.is_visible());
    for import in [true, false] {
        let chooser = menu.chooser_for_test(import).unwrap();
        assert!(chooser.is_modal());
        assert_eq!(
            chooser.action(),
            if import {
                FileChooserAction::Open
            } else {
                FileChooserAction::Save
            }
        );
        assert_eq!(chooser.filters().n_items(), 1);
        chooser.emit_by_name::<()>("response", &[&ResponseType::Cancel.into_glib()]);
    }
    // Subscribe before sending work. The bounded channel receive is the completion signal.
    while requests.try_recv().is_ok() {}
    let (events, completed) = mpsc::channel();
    let worker = thread::spawn(move || {
        feature_runtime::run_worker(requests, events, Default::default());
    });
    let bundle = sandbox.join("configuration.sdeck-plus.json");
    menu.select_path_for_test(false, bundle.clone());
    assert!(!widgets.root.is_sensitive());
    let MonitorEvent::ConfigurationExportResult { path, result } =
        completed.recv_timeout(Duration::from_secs(10)).unwrap()
    else {
        panic!("expected completed export");
    };
    assert!(result.is_ok(), "{result:?}");
    runtime.complete_configuration_export(&path, result);
    assert!(widgets.root.is_sensitive());
    assert!(bundle.is_file());
    let exported: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&bundle).unwrap()).unwrap();
    let image_name = format!(
        "assets/{}",
        image_path.file_name().unwrap().to_str().unwrap()
    );
    let image_entry = exported["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == image_name)
        .unwrap();
    assert!(image_entry.get("data").is_none());

    layers
        .borrow_mut()
        .bind("Base", Trigger::KeyPress(1), Action::None)
        .unwrap();
    layers.borrow_mut().create("Temporary").unwrap();
    layers.borrow().persist().unwrap();
    let mut changed = Organization::open_at(&root, &layers.borrow()).unwrap();
    changed.sync(&layers.borrow());
    changed.persist().unwrap();
    artwork
        .store
        .borrow_mut()
        .as_mut()
        .unwrap()
        .clear_in_layer("Base", ArtworkTarget::Keypad(1))
        .unwrap();
    // Referenced images must remain available locally when restoring a snapshot.
    std::fs::copy(&source, &image_path).unwrap();
    assert!(runtime.complete_configuration_import(&root, Ok(None)));

    menu.select_path_for_test(true, bundle.clone());
    let confirmation = menu.confirmation_for_test().unwrap();
    capture_themes("import-confirmation", confirmation.upcast_ref());
    confirmation.response(ResponseType::Cancel);
    assert!(completed.try_recv().is_err());
    assert!(layers.borrow().layers().contains_key("Temporary"));
    menu.select_path_for_test(true, bundle.clone());
    menu.confirmation_for_test()
        .unwrap()
        .response(ResponseType::Accept);
    assert!(!widgets.root.is_sensitive());
    runtime.execute(
        "must-stay-inert".into(),
        Action::Feature(sdeck_plus::features::FeatureAction::Timer { seconds: 60 }),
    );
    assert!(
        !runtime.timer_active(),
        "physical actions must stay blocked during import"
    );
    let MonitorEvent::ConfigurationImportResult {
        root: imported_root,
        result,
    } = completed.recv_timeout(Duration::from_secs(10)).unwrap()
    else {
        panic!("expected completed import");
    };
    let backup = result.as_ref().unwrap().as_ref().unwrap().clone();
    assert!(backup.join("layers.conf").is_file());
    while hardware.try_recv().is_ok() {}
    assert!(runtime.complete_configuration_import(&imported_root, result));
    assert!(widgets.root.is_sensitive());
    assert!(!layers.borrow().layers().contains_key("Temporary"));
    assert_eq!(
        layers.borrow().layers()["Base"].bindings[&Trigger::KeyPress(1)],
        original
    );
    assert_eq!(
        artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .button_text_in_layer("Base", ArtworkTarget::Keypad(1)),
        Some(&text),
    );
    assert!(artwork
        .store
        .borrow()
        .as_ref()
        .unwrap()
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(1))
        .unwrap()
        .is_file());
    assert!(hardware
        .try_iter()
        .any(|command| matches!(command, ArtworkCommand::Replace(_))));
    key_button(&widgets.key_labels[1]).emit_clicked();
    assert_eq!(widgets.binding_ui.action_kind(), ActionKind::None);
    capture_themes("imported", widgets.root.upcast_ref());

    let before = std::fs::read(root.join("layers.conf")).unwrap();
    let bad = sandbox.join("invalid.sdeck-plus.json");
    std::fs::write(&bad, b"invalid archive").unwrap();
    menu.select_path_for_test(true, bad);
    menu.confirmation_for_test()
        .unwrap()
        .response(ResponseType::Accept);
    let MonitorEvent::ConfigurationImportResult {
        root: imported_root,
        result,
    } = completed.recv_timeout(Duration::from_secs(10)).unwrap()
    else {
        panic!("expected failed import");
    };
    assert!(result.is_err());
    assert!(!runtime.complete_configuration_import(&imported_root, result));
    assert!(widgets.root.is_sensitive());
    assert!(
        runtime.status.is_visible(),
        "import failures must be visible in the window"
    );
    assert_eq!(std::fs::read(root.join("layers.conf")).unwrap(), before);
    capture_themes("import-error", widgets.root.upcast_ref());
    commands.send(LayerCommand::Shutdown).unwrap();
    worker.join().unwrap();
    window.destroy();
    std::fs::remove_dir_all(sandbox).unwrap();
}
