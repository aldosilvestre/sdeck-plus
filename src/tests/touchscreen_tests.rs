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
    let (root, mut store, _, source) = artwork_fixture("touchscreen-owner");
    let mut images = ArtworkStore::open_at_for_layer(store.root(), Some("Base")).unwrap();
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
    store.persist().unwrap();
    organization.persist().unwrap();
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
    let (commands, requests) = mpsc::channel();
    let window = ApplicationWindow::builder().build();
    let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
    let data_root = layers.borrow().root().to_owned();
    assert!(widgets
        .feature_runtime
        .complete_configuration_import(&data_root, Ok(None)));
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
    let controls = widgets.feature_runtime.touchscreen_controls();
    assert!(controls.is_visible());
    let mut buttons = Vec::new();
    let mut child = controls.first_child();
    while let Some(widget) = child {
        if let Some(button) = widget.downcast_ref::<Button>() {
            buttons.push(button.clone());
        }
        child = widget.next_sibling();
    }
    let button = |caption: &str| {
        buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some(caption))
            .unwrap()
    };
    button("New touchscreen page").emit_clicked();
    let second = layers
        .borrow()
        .touchscreen_owner("Base")
        .unwrap()
        .to_owned();
    assert_ne!(second, owner);
    assert_eq!(layers.borrow().touchscreen_pages("Base").len(), 2);
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
    use sdeck_plus::features::{FeatureAction, TouchscreenAction};
    let next = Action::Feature(FeatureAction::Touchscreen(TouchscreenAction::NextPage));
    let previous = Action::Feature(FeatureAction::Touchscreen(TouchscreenAction::PreviousPage));
    let picker = controls
        .first_child()
        .unwrap()
        .next_sibling()
        .unwrap()
        .downcast::<gtk::DropDown>()
        .unwrap();
    let save = {
        let mut all = Vec::new();
        collect_layout_buttons(widgets.root.upcast_ref(), &mut all);
        all.into_iter()
            .find(|button| button_caption(button).as_deref() == Some("Save binding"))
            .unwrap()
    };
    select_string(
        &widgets.binding_ui.event_model,
        &widgets.binding_ui.event_picker,
        "Swipe right",
    );
    widgets.binding_ui.populate_action(&previous);
    save.emit_clicked();
    assert_eq!(
        layers
            .borrow()
            .binding("Base", &Trigger::TouchSwipe(SwipeDirection::Right)),
        Some(previous)
    );
    picker.set_selected(0);
    select_string(
        &widgets.binding_ui.event_model,
        &widgets.binding_ui.event_picker,
        "Swipe left",
    );
    widgets.binding_ui.populate_action(&next);
    save.emit_clicked();
    assert_eq!(
        layers
            .borrow()
            .binding("Base", &Trigger::TouchSwipe(SwipeDirection::Left)),
        Some(next)
    );
    assert!(
        requests
            .try_iter()
            .all(|command| !matches!(command, LayerCommand::Execute { .. })),
        "saving and selecting pages must not execute actions"
    );
    for (start_x, end_x, expected) in [(600, 20, second.as_str()), (20, 600, owner.as_str())] {
        let event = protocol::Event::TouchFlick {
            start_x,
            start_y: 40,
            end_x,
            end_y: 40,
        };
        let (id, action, page_return) = matching_binding(&layers, &event).unwrap();
        widgets
            .feature_runtime
            .execute_with_return(id, action, page_return);
        assert_eq!(layers.borrow().touchscreen_owner("Base"), Some(expected));
        assert_eq!(layers.borrow().active(), "Base");
    }
    assert!(!button("Previous touchscreen page").is_sensitive());
    button("Next touchscreen page").emit_clicked();
    assert_eq!(
        layers.borrow().touchscreen_owner("Base"),
        Some(second.as_str())
    );
    assert!(!button("Next touchscreen page").is_sensitive());
    widgets.binding_ui.control_picker.set_selected(0);
    assert!(!controls.is_visible());
    window.close();
    fs::remove_dir_all(root).unwrap();
}
