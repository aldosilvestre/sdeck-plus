use crate::{
    features::{FeatureAction, TouchscreenAction},
    layers::{Action, LayerStore, SwipeDirection, Trigger},
    local_artwork::ArtworkStore,
    organization::Organization,
    protocol::Event,
};
use std::fs;

#[test]
fn horizontal_swipes_select_touch_navigation_without_changing_keypad_bindings() {
    let root = std::env::temp_dir().join(format!("sdeck-touch-nav-{}", std::process::id()));
    let mut layers = LayerStore::open_at(&root).unwrap();
    let mut organization = Organization::open_at(&root, &layers).unwrap();
    let keypad = organization.create_page(&mut layers).unwrap();
    let mut artwork = ArtworkStore::open_at_for_layer(&root, Some(&keypad)).unwrap();
    layers
        .bind(
            &keypad,
            Trigger::KeyPress(1),
            Action::Emit("unchanged".into()),
        )
        .unwrap();
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    let first = layers.touchscreen_owner(&keypad).unwrap().to_owned();
    let second = layers.create_touchscreen_page("Base").unwrap();
    let previous = Action::Feature(FeatureAction::Touchscreen(TouchscreenAction::PreviousPage));
    let next = Action::Feature(FeatureAction::Touchscreen(TouchscreenAction::NextPage));
    layers
        .bind(
            &keypad,
            Trigger::TouchSwipe(SwipeDirection::Right),
            previous.clone(),
        )
        .unwrap();
    layers.select_touchscreen_page("Base", &first);
    layers
        .bind(
            &keypad,
            Trigger::TouchSwipe(SwipeDirection::Left),
            next.clone(),
        )
        .unwrap();
    let left = Event::TouchFlick {
        start_x: 600,
        start_y: 40,
        end_x: 20,
        end_y: 40,
    };
    let right = Event::TouchFlick {
        start_x: 20,
        start_y: 40,
        end_x: 600,
        end_y: 40,
    };
    assert_eq!(layers.matching_action(&left), Some(next.clone()));
    assert!(layers.step_touchscreen_page("Base", true));
    assert_eq!(layers.touchscreen_owner(&keypad), Some(second.as_str()));
    assert_eq!(layers.matching_action(&right), Some(previous));
    assert!(layers.step_touchscreen_page("Base", false));
    assert_eq!(layers.touchscreen_owner(&keypad), Some(first.as_str()));
    assert_eq!(layers.active(), keypad);
    assert_eq!(
        layers.binding(&keypad, &Trigger::KeyPress(1)),
        Some(Action::Emit("unchanged".into()))
    );
    layers.persist().unwrap();
    let before = fs::read(root.join("touchscreen.json")).unwrap();
    for trigger in [Trigger::KeyPress(1), Trigger::EncoderPress(1)] {
        let old = layers.binding(&keypad, &trigger);
        let error = layers
            .bind(&keypad, trigger.clone(), next.clone())
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "touchscreen page actions require a touchscreen gesture"
        );
        assert_eq!(layers.binding(&keypad, &trigger), old);
    }
    layers.persist().unwrap();
    assert_eq!(fs::read(root.join("touchscreen.json")).unwrap(), before);
    assert_eq!(layers.touchscreen_pages("Base").len(), 2);
    organization.persist().unwrap();
    let mut loaded = LayerStore::open_at(&root).unwrap();
    let organization = Organization::open_at(&root, &loaded).unwrap();
    loaded
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(loaded.matching_action(&left), Some(next));
    fs::remove_dir_all(root).unwrap();
}
