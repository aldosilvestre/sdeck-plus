use super::*;
use sdeck_plus::{
    desktop_actions::{audio_output::list_outputs, DesktopAction},
    features::FeatureAction,
};

#[test]
fn none_and_audio_output_bindings_are_editable_saved_and_dispatched() {
    run_gtk_test(assert_none_and_audio_output_bindings_are_editable_saved_and_dispatched);
}

fn assert_none_and_audio_output_bindings_are_editable_saved_and_dispatched() {
    if !init_gtk_for_test() {
        assert!(
            std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "GTK initialization failed despite an advertised display"
        );
        return;
    }
    let (root, store, images, _) = artwork_fixture("multimedia-actions");
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
        .default_height(850)
        .build();
    install_style(&window);
    let widgets = build_widgets(window.clone(), layers.clone(), artwork, commands);
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(1000, 950);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);
    key_button(&widgets.key_labels[0]).emit_clicked();
    let ui = &widgets.binding_ui;
    // Every untouched button starts as None, even after another action was edited.
    for profile in ["Base", "Studio"] {
        select_string(&ui.layer_model, &ui.layer_picker, profile);
        for label in &widgets.key_labels {
            ui.action_picker.set_selected(1);
            key_button(label).emit_clicked();
            // The unsaved edit is only discarded after the user agrees to it.
            discard_dialog().response(ResponseType::Accept);
            assert!(
                !ui.is_dirty(),
                "a freshly loaded control must be clean: {:?} vs {:?}",
                ui.baseline.borrow(),
                ui.fingerprint()
            );
            assert_eq!(ui.action_kind(), ActionKind::None);
            assert_eq!(ui.binding_from_widgets(&layers).unwrap().2, Action::None);
        }
    }
    select_string(&ui.layer_model, &ui.layer_picker, "Base");
    key_button(&widgets.key_labels[0]).emit_clicked();
    assert_shortcut_only_menu(ui);
    ui.action_picker.set_selected(5);
    ui.feature_editor.assert_roundtrips();
    ui.feature_editor.assert_audio_output_choices();
    let mut buttons = Vec::new();
    collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
    let click = |caption: &str| {
        buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some(caption))
            .unwrap()
            .emit_clicked();
    };
    let capture = |name: &str| {
        if let Some(directory) = std::env::var_os("STREAMDECK_ACTIONS_QA_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            ui.feature_editor.wait_for_audio_outputs();
            for (size, width) in [("wide", 1000), ("compact", 941)] {
                widgets.root.set_size_request(width, 950);
                window.set_default_size(width, 950);
                wait_for_frame(&window);
                let texture = rendered_texture(&window, &widgets.root);
                assert_eq!((texture.width(), texture.height()), (width, 950));
                texture
                    .save_to_png(
                        std::path::Path::new(&directory).join(format!("{name}-{size}.png")),
                    )
                    .unwrap();
            }
        }
    };
    while requests.try_recv().is_ok() {}

    ui.action_picker.set_selected(7);
    ui.return_toggle.set_active(true);
    ui.update_action_visibility();
    assert_eq!(ui.binding_from_widgets(&layers).unwrap().2, Action::None);
    assert!(!ui.return_toggle.is_visible() && !ui.return_row.is_visible());
    click("Save binding");
    let LayerCommand::SaveBinding(snapshot, _) = requests.try_recv().unwrap() else {
        panic!("expected saved None binding");
    };
    snapshot.persist().unwrap();
    let reloaded = LayerStore::open_at(snapshot.root()).unwrap();
    assert_eq!(
        reloaded.layers()["Base"].bindings[&Trigger::KeyPress(1)],
        Action::None
    );
    assert_eq!(reloaded.page_return("Base", &Trigger::KeyPress(1)), None);
    ui.load_selected_binding(&layers);
    assert_eq!(ui.action_kind(), ActionKind::None);
    capture("none");
    click("Test action");
    assert!(
        requests.try_recv().is_err(),
        "None must not queue any action"
    );
    assert!(!widgets.feature_runtime.page_return_active());

    key_button(&widgets.key_labels[1]).emit_clicked();
    // Normal tests use a stable disconnected target. Opt-in QA discovers the real host outputs.
    let name = if std::env::var_os("STREAMDECK_ACTIONS_QA_DIR").is_some() {
        list_outputs()
            .unwrap()
            .first()
            .expect("a connected audio output for QA")
            .name
            .clone()
    } else {
        "alsa_output.test-speakers".into()
    };
    let action = Action::Feature(FeatureAction::Desktop(DesktopAction::SetAudioOutput {
        name,
    }));
    ui.populate_action(&action);
    click("Save binding");
    let LayerCommand::SaveBinding(snapshot, _) = requests.try_recv().unwrap() else {
        panic!("expected saved audio output binding");
    };
    snapshot.persist().unwrap();
    let reloaded = LayerStore::open_at(snapshot.root()).unwrap();
    assert_eq!(
        reloaded.layers()["Base"].bindings[&Trigger::KeyPress(2)],
        action
    );
    ui.load_selected_binding(&layers);
    assert_eq!(ui.binding_from_widgets(&layers).unwrap().2, action);
    capture("audio-output");
    click("Test action");
    let LayerCommand::Execute {
        action: dispatched, ..
    } = requests.try_recv().unwrap()
    else {
        panic!("expected worker dispatch");
    };
    assert_eq!(dispatched, action);
    // Rotate is a choice in the same device picker, not a separate category action.
    ui.feature_editor.select_audio_rotation();
    let rotate = Action::Feature(FeatureAction::Desktop(DesktopAction::CycleAudioOutput));
    assert_eq!(ui.binding_from_widgets(&layers).unwrap().2, rotate);
    click("Save binding");
    let LayerCommand::SaveBinding(snapshot, _) = requests.try_recv().unwrap() else {
        panic!("expected saved rotating output binding");
    };
    snapshot.persist().unwrap();
    let reloaded = LayerStore::open_at(snapshot.root()).unwrap();
    assert_eq!(
        reloaded.layers()["Base"].bindings[&Trigger::KeyPress(2)],
        rotate
    );
    ui.load_selected_binding(&layers);
    assert_eq!(ui.binding_from_widgets(&layers).unwrap().2, rotate);
    capture("audio-rotate");
    click("Test action");
    let LayerCommand::Execute {
        action: dispatched, ..
    } = requests.try_recv().unwrap()
    else {
        panic!("expected rotating output dispatch");
    };
    assert_eq!(dispatched, rotate);
    window.destroy();
    std::fs::remove_dir_all(root).unwrap();
}
