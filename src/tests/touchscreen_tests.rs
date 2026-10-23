use super::*;
use sdeck_plus::organization::Organization;
use std::fs;

#[test]
fn touchscreen_page_indicators_follow_selection_artwork_and_live_info() {
    run_gtk_test(assert_touchscreen_page_indicators);
}
fn assert_touchscreen_page_indicators() {
    use sdeck_plus::info::{Glyph, InfoSource, InfoValue};
    if !init_gtk_for_test() {
        return;
    }
    fn drain(receiver: &mpsc::Receiver<ArtworkCommand>) -> Vec<ArtworkCommand> {
        receiver
            .try_iter()
            .flat_map(|command| match command {
                ArtworkCommand::Replace(commands) => commands,
                command => vec![command],
            })
            .collect()
    }
    fn indicators(commands: &[ArtworkCommand]) -> Vec<(u8, bool, bool)> {
        commands
            .iter()
            .filter_map(|command| match command {
                ArtworkCommand::ComposeTouchscreenZone {
                    zone,
                    previous,
                    next,
                    ..
                } => Some((*zone, *previous, *next)),
                _ => None,
            })
            .collect()
    }
    fn keys(commands: &[ArtworkCommand]) -> Vec<String> {
        commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    ArtworkCommand::Assign {
                        target: ArtworkTarget::Keypad(_),
                        ..
                    } | ArtworkCommand::Clear {
                        target: ArtworkTarget::Keypad(_),
                        ..
                    } | ArtworkCommand::ComposeButton { .. }
                        | ArtworkCommand::ComposeInfo {
                            target: ArtworkTarget::Keypad(_),
                            ..
                        }
                )
            })
            .map(|command| format!("{command:?}"))
            .collect()
    }
    fn arrow(artwork: &ArtworkUi, zone: u8, x: usize) {
        let pictures = artwork.pictures.borrow();
        let (_, picture) = pictures
            .iter()
            .find(|(target, _)| *target == ArtworkTarget::EncoderZone(zone))
            .unwrap();
        assert!(picture.get_visible());
        let texture = picture
            .paintable()
            .unwrap()
            .downcast::<gtk::gdk::Texture>()
            .unwrap();
        let mut pixels = vec![0; 200 * 100 * 4];
        texture.download(&mut pixels, 200 * 4);
        assert_eq!(
            &pixels[(50 * 200 + x) * 4..(50 * 200 + x) * 4 + 4],
            &[255; 4]
        );
    }
    let (root, mut store, mut images, source) = artwork_fixture("touch-indicators");
    let mut organization = Organization::open_at(store.root(), &store).unwrap();
    let keypad = organization.create_page(&mut store).unwrap();
    let folder = organization
        .create_folder(&mut store, "Indicators")
        .unwrap();
    store
        .initialize_touchscreens(&organization, &mut images)
        .unwrap();
    store.select("Base");
    let first = store.touchscreen_owner("Base").unwrap().to_owned();
    images
        .assign_in_layer(&first, ArtworkTarget::TouchStripBackground, &source)
        .unwrap();
    for zone in [1, 4] {
        images
            .assign_in_layer(&first, ArtworkTarget::EncoderZone(zone), &source)
            .unwrap();
    }
    for (trigger, info) in [
        (Trigger::EncoderPress(1), InfoSource::Cpu),
        (Trigger::KeyPress(1), InfoSource::Volume),
    ] {
        store
            .bind("Base", trigger.clone(), Action::Emit("unchanged".into()))
            .unwrap();
        store.set_info("Base", &trigger, Some(info)).unwrap();
    }
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
    let (commands, _requests) = mpsc::channel();
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(700)
        .build();
    install_style(&window);
    let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
    window.set_child(Some(&widgets.root));
    window.present();
    wait_for_frame(&window);
    drain(&hardware);
    artwork.refresh(true);
    let initial = drain(&hardware);
    assert!(
        indicators(&initial).is_empty(),
        "one page has no indicators"
    );
    let original_keys = keys(&initial);
    widgets.binding_ui.control_picker.set_selected(2);
    let create = widgets
        .feature_runtime
        .touchscreen_controls()
        .last_child()
        .unwrap()
        .downcast::<Button>()
        .unwrap();
    create.emit_clicked();
    let middle = layers
        .borrow()
        .touchscreen_owner("Base")
        .unwrap()
        .to_owned();
    create.emit_clicked();
    let last = layers
        .borrow()
        .touchscreen_owner("Base")
        .unwrap()
        .to_owned();
    for owner in [&middle, &last] {
        artwork
            .store
            .borrow_mut()
            .as_mut()
            .unwrap()
            .assign_in_layer(owner, ArtworkTarget::EncoderZone(4), &source)
            .unwrap();
    }
    drain(&hardware);
    for (owner, expected) in [
        (&first, vec![(4, false, true)]),
        (&middle, vec![(1, true, true), (4, true, true)]),
        (&last, vec![(1, true, false)]),
    ] {
        layers.borrow_mut().select_touchscreen_page("Base", owner);
        artwork.refresh(true);
        let snapshot = drain(&hardware);
        assert_eq!(indicators(&snapshot), expected);
        assert_eq!(
            keys(&snapshot),
            original_keys,
            "touch changes cannot change keypad images"
        );
        let strip = snapshot
            .iter()
            .position(|command| {
                matches!(
                    command,
                    ArtworkCommand::Assign {
                        target: ArtworkTarget::TouchStripBackground,
                        ..
                    } | ArtworkCommand::Clear {
                        target: ArtworkTarget::TouchStripBackground
                    }
                )
            })
            .unwrap();
        for (index, command) in snapshot.iter().enumerate() {
            if let ArtworkCommand::ComposeTouchscreenZone { zone, .. } = command {
                assert!(
                    index > strip,
                    "indicators must follow full-strip replacement"
                );
                arrow(&artwork, *zone, if *zone == 1 { 8 } else { 192 });
            }
        }
    }
    layers.borrow_mut().select_touchscreen_page("Base", &middle);
    artwork.refresh(true);
    drain(&hardware);
    let cpu = InfoValue::gauge(Glyph::Chip, 30, "30%".into());
    artwork.update_info(InfoSource::Cpu, cpu.clone());
    assert!(
        matches!(drain(&hardware).as_slice(), [ArtworkCommand::ComposeTouchscreenZone { zone: 1, previous: true, next: true, value: Some(value), .. }] if value == &cpu)
    );
    arrow(&artwork, 1, 8);
    artwork.restore_button_preview(&ArtworkTarget::EncoderZone(4));
    arrow(&artwork, 4, 192);
    artwork.update_info(InfoSource::Cpu, cpu);
    assert!(
        drain(&hardware).is_empty(),
        "identical values must not redraw"
    );
    artwork.update_info(
        InfoSource::Volume,
        InfoValue::gauge(Glyph::Speaker, 45, "45%".into()),
    );
    assert!(matches!(
        drain(&hardware).as_slice(),
        [ArtworkCommand::ComposeInfo {
            target: ArtworkTarget::Keypad(1),
            ..
        }]
    ));
    for keypad_page in [&keypad, &folder] {
        layers.borrow_mut().select(keypad_page);
        artwork.refresh(false);
        assert_eq!(
            indicators(&drain(&hardware)),
            [(1, true, true), (4, true, true)]
        );
        assert_eq!(
            layers.borrow().touchscreen_owner(keypad_page),
            Some(middle.as_str())
        );
    }
    layers.borrow_mut().select("Studio");
    artwork.refresh(false);
    assert!(
        indicators(&drain(&hardware)).is_empty(),
        "another single-page profile has no arrows"
    );
    layers.borrow_mut().select("Base");
    artwork.refresh(false);
    assert_eq!(
        indicators(&drain(&hardware)),
        [(1, true, true), (4, true, true)]
    );
    window.close();
    fs::remove_dir_all(root).unwrap();
}

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
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(700)
        .build();
    let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
    let data_root = layers.borrow().root().to_owned();
    assert!(widgets
        .feature_runtime
        .complete_configuration_import(&data_root, Ok(None)));
    window.set_child(Some(&widgets.root));
    window.present();
    wait_for_frame(&window);
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
    wait_for_frame(&window);
    assert!(
        controls.is_mapped(),
        "touch page controls must be rendered, not merely present"
    );
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
