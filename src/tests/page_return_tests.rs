use super::*;
use sdeck_plus::layers::PageReturn;

#[test]
fn page_return_option_is_saved_reloaded_and_tested_with_any_action() {
    run_gtk_test(assert_page_return_option_is_saved_reloaded_and_tested_with_any_action);
}

fn assert_page_return_option_is_saved_reloaded_and_tested_with_any_action() {
    if !init_gtk_for_test() {
        assert!(
            std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "GTK initialization failed despite an advertised display"
        );
        return;
    }
    let (root, store, images, _) = artwork_fixture("page-return");
    let layers = Rc::new(RefCell::new(store));
    let (output, _hardware) = mpsc::channel();
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
        .default_height(700)
        .build();
    install_style(&window);
    let widgets = build_widgets(window.clone(), layers.clone(), artwork, commands);
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(1000, 700);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);

    widgets.feature_runtime.assert_page_return();

    key_button(&widgets.key_labels[0]).emit_clicked();
    let ui = &widgets.binding_ui;
    let trigger = Trigger::KeyPress(1);
    let expected = PageReturn {
        layer: "Base".into(),
        seconds: 60,
    };
    ui.action_picker.set_selected(1);
    ui.text_entry.set_text("true");
    assert!(!ui.return_row.is_visible(), "off until the user opts in");
    ui.return_toggle.set_active(true);
    assert!(ui.return_row.is_visible());
    assert_eq!(dropdown_value(&ui.return_model, 0).as_deref(), Some("Base"));
    select_string(&ui.return_model, &ui.return_target, "Base");
    ui.return_seconds.set_text("60");
    let mut buttons = Vec::new();
    collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
    let find = |label: &str| {
        buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some(label))
            .unwrap()
    };
    while requests.try_recv().is_ok() {}

    // Saving stores the option without starting it.
    find("Save binding").emit_clicked();
    let LayerCommand::SaveBinding(snapshot, Some(_)) = requests.try_recv().unwrap() else {
        panic!("save must persist the binding");
    };
    assert_eq!(
        snapshot.page_return("Base", &trigger),
        Some(expected.clone())
    );
    assert!(!widgets.feature_runtime.page_return_active());
    snapshot.persist().unwrap();
    let reloaded = LayerStore::open_at(snapshot.root()).unwrap();
    assert_eq!(
        reloaded.page_return("Base", &trigger),
        Some(expected.clone())
    );

    // Reloading the control restores the option in the editor.
    ui.return_toggle.set_active(false);
    ui.load_selected_binding(&layers);
    assert!(ui.return_toggle.is_active() && ui.return_row.is_visible());
    assert_eq!(ui.return_seconds.text(), "60");

    // An invalid delay saves nothing.
    ui.return_seconds.set_text("0");
    find("Save binding").emit_clicked();
    assert!(requests.try_recv().is_err());
    ui.return_seconds.set_text("60");

    // Test runs the action and schedules the return.
    find("Test action").emit_clicked();
    assert!(matches!(
        requests.try_recv().unwrap(),
        LayerCommand::Execute { .. }
    ));
    assert!(widgets.feature_runtime.page_return_active());
    widgets.feature_runtime.cancel_timer();

    // Opting out removes it.
    ui.return_toggle.set_active(false);
    find("Save binding").emit_clicked();
    let LayerCommand::SaveBinding(snapshot, _) = requests.try_recv().unwrap() else {
        panic!("save must persist the binding");
    };
    assert_eq!(snapshot.page_return("Base", &trigger), None);

    if let Some(directory) = std::env::var_os("STREAMDECK_PAGE_RETURN_QA_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        for (name, on) in [("off", false), ("on", true)] {
            ui.return_toggle.set_active(on);
            let adjustment = widgets.root.vadjustment();
            adjustment.set_value((adjustment.upper() - adjustment.page_size()).max(0.0));
            rendered_texture(&window, &widgets.root)
                .save_to_png(directory.join(format!("return-{name}.png")))
                .unwrap();
        }
    }
    window.destroy();
    std::fs::remove_dir_all(root).unwrap();
}
