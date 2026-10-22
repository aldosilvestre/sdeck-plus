use super::*;
use sdeck_plus::organization::Organization;
use std::fs;

#[test]
fn touch_editor_artwork_and_identity_follow_profile_owner_not_keypad_page() {
    run_gtk_test(assert_profile_owner);
}
fn assert_profile_owner() {
    if !init_gtk_for_test() {
        return;
    }
    let (root, mut store, mut images, source) = artwork_fixture("touchscreen-owner");
    let mut organization = Organization::open_at(store.root(), &store).unwrap();
    images
        .assign_in_layer("Base", ArtworkTarget::TouchStripBackground, &source)
        .unwrap();
    store
        .bind(
            "Base",
            Trigger::ZoneTap(1),
            Action::Emit("profile touch".into()),
        )
        .unwrap();
    // The other keypad page initially has an identical touch configuration.
    let page = organization.create_page(&mut store).unwrap();
    store
        .bind(
            &page,
            Trigger::ZoneTap(1),
            Action::Emit("profile touch".into()),
        )
        .unwrap();
    images
        .copy_touchscreen_assignments(&[("Base".into(), page.clone())])
        .unwrap();
    store
        .initialize_touchscreens(&organization, &mut images)
        .unwrap();
    let owner = store.touchscreen_owner("Base").unwrap().to_owned();
    let layers = Rc::new(RefCell::new(store));
    let (output, hardware) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(images))),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output,
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(Default::default())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::TouchStripBackground)),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
        info: Rc::default(),
    };
    let (commands, _requests) = mpsc::channel();
    let window = ApplicationWindow::builder().build();
    let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
    // Test the real selected-control editor and hardware command composition.
    widgets.binding_ui.control_picker.set_selected(2);
    widgets.binding_ui.load_selected_binding(&layers);
    assert_eq!(
        widgets
            .binding_ui
            .selected_layer_and_trigger(&layers)
            .unwrap()
            .0,
        owner
    );
    let event = protocol::Event::TouchTap { x: 10, y: 10 };
    let identity = matching_binding(&layers, &event).unwrap().0;
    let captured = artwork.capture_owner();
    assert_eq!(captured.name, owner);
    artwork.refresh(true);
    let first_path = artwork
        .info_background(&page, ArtworkTarget::TouchStripBackground)
        .unwrap();
    assert!(hardware
        .try_iter()
        .any(|command| matches!(command, ArtworkCommand::Replace(_))));
    layers.borrow_mut().select("Base");
    widgets.binding_ui.load_selected_binding(&layers);
    assert_eq!(matching_binding(&layers, &event).unwrap().0, identity);
    assert_eq!(artwork.capture_owner().name, owner);
    assert_eq!(
        artwork.info_background("Base", ArtworkTarget::TouchStripBackground),
        Some(first_path)
    );
    let second = layers.borrow_mut().create_touchscreen_page("Base").unwrap();
    artwork.refresh(false);
    assert!(
        hardware
            .try_iter()
            .any(|command| matches!(command, ArtworkCommand::Replace(_))),
        "touch-only selection invalidates device cache"
    );
    assert_eq!(layers.borrow().active(), "Base");
    assert_eq!(artwork.capture_owner().name, second);
    // An outstanding chooser stays bound to the original touch page.
    artwork.import(&captured, &source).unwrap();
    assert!(artwork
        .store
        .borrow()
        .as_ref()
        .unwrap()
        .assigned_path_in_layer(&owner, ArtworkTarget::TouchStripBackground)
        .is_some());
    assert!(artwork
        .info_background("Base", ArtworkTarget::TouchStripBackground)
        .is_none());
    window.close();
    fs::remove_dir_all(root).unwrap();
}
