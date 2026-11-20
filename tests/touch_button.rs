//! The capacitive touch buttons a model reports after its keys (the Stream
//! Deck Neo's two) are controls of their own: stored as `tb:<n>` in
//! layers.conf, run by a touch, offered by the editor and drawn by the preview
//! only for the models that have them. Everything goes through the public API.
use sdeck_plus::{
    binding_editor::{
        control_number_error, control_summary, control_triggers, controls_for, events_for_control,
        trigger_from_selection_for, ControlKind, EventKind,
    },
    config_bundle,
    device_model::{DeviceModel, CLASSIC, PLUS, PLUS_XL, XL},
    features::{FeatureAction, MultiStep},
    info::InfoSource,
    layers::{Action, LayerStore, TouchButtonPresses, Trigger, CONFLICT_LOG},
    organization::Organization,
    preview_layout::{ControlRef, PreviewLayout, MIN_HIT_PX},
    protocol::Event,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sdeck-touch-button-{}-{name}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.0.join(name)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn neo() -> &'static DeviceModel {
    DeviceModel::by_id("neo").unwrap()
}

fn emit(text: &str) -> Action {
    Action::Emit(text.into())
}

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

const BASE: &str = "42617365";
const HEADER: &str = "streamdeck-layers-v1\nactive\t42617365\n";

fn touch(index: usize, pressed: bool) -> Event {
    Event::Key { index, pressed }
}

/// Models in the table that have no touch buttons.
fn without_touch_buttons() -> [&'static DeviceModel; 4] {
    [&PLUS, &PLUS_XL, &CLASSIC, &XL]
}

#[test]
fn the_neo_has_two_touch_buttons_after_its_eight_keys() {
    let model = neo();
    assert_eq!(model.touch_button_numbers(), 1..=2);
    assert!(model.is_valid_touch_button(1) && model.is_valid_touch_button(2));
    assert!(!model.is_valid_touch_button(0) && !model.is_valid_touch_button(3));
    // Zero-based HID key indexes: 0-7 are keys, 8 and 9 the touch buttons.
    assert_eq!(model.touch_button_at(7), None);
    assert_eq!(model.touch_button_at(8), Some(1));
    assert_eq!(model.touch_button_at(9), Some(2));
    assert_eq!(model.touch_button_at(10), None);
    for model in without_touch_buttons() {
        assert!(model.touch_button_numbers().is_empty(), "{}", model.id);
        assert!(!model.is_valid_touch_button(1), "{}", model.id);
        assert_eq!(
            model.touch_button_at(usize::from(model.keys())),
            None,
            "{}",
            model.id
        );
    }
}

#[test]
fn touch_button_bindings_are_stored_as_tb_rows_and_read_back() {
    let fixture = Fixture::new("round-trip");
    let mut store = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    store.create("Studio").unwrap();
    store
        .bind("Base", Trigger::TouchButton(1), emit("left"))
        .unwrap();
    store
        .bind(
            "Base",
            Trigger::TouchButton(2),
            Action::SwitchLayer("Studio".into()),
        )
        .unwrap();
    store
        .bind("Base", Trigger::KeyPress(8), emit("key"))
        .unwrap();
    store
        .set_return(
            "Base",
            &Trigger::TouchButton(2),
            Some(sdeck_plus::layers::PageReturn {
                layer: "Base".into(),
                seconds: 5,
            }),
        )
        .unwrap();
    store.persist().unwrap();

    let text = fixture.read("layers.conf");
    let expected = format!(
        "{HEADER}layer\t{BASE}\nbinding\t{BASE}\tk:8\temit:{}\n\
         binding\t{BASE}\ttb:1\temit:{}\nbinding\t{BASE}\ttb:2\tswitch:{}\n\
         pagereturn\t{BASE}\ttb:2\t{BASE}\t5\nlayer\t{}\n",
        hex("key"),
        hex("left"),
        hex("Studio"),
        hex("Studio")
    );
    assert_eq!(text, expected);

    let reopened = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    assert!(
        reopened.conflicts().is_empty(),
        "{:?}",
        reopened.conflicts()
    );
    assert!(reopened.diagnostics().is_empty());
    assert_eq!(
        reopened.binding("Base", &Trigger::TouchButton(1)),
        Some(emit("left"))
    );
    assert_eq!(
        reopened.binding("Base", &Trigger::TouchButton(2)),
        Some(Action::SwitchLayer("Studio".into()))
    );
    assert_eq!(
        reopened
            .page_return("Base", &Trigger::TouchButton(2))
            .map(|value| value.seconds),
        Some(5)
    );
    reopened.persist().unwrap();
    assert_eq!(fixture.read("layers.conf"), expected);
    assert!(!fixture.0.join(CONFLICT_LOG).exists());
}

#[test]
fn touch_buttons_a_model_lacks_are_refused_and_change_nothing() {
    let fixture = Fixture::new("refused");
    let mut store = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    store
        .bind("Base", Trigger::TouchButton(1), emit("kept"))
        .unwrap();
    let before = store.layers()["Base"].bindings.clone();
    for number in [0, 3, 255] {
        let error = store
            .bind("Base", Trigger::TouchButton(number), emit("no"))
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "touch button out of range");
        assert!(store.unbind("Base", &Trigger::TouchButton(number)).is_err());
    }
    assert_eq!(store.layers()["Base"].bindings, before);
    // Nothing was written by the refused calls.
    assert!(!fixture.0.join("layers.conf").exists());

    for model in without_touch_buttons() {
        let root = Fixture::new(model.id);
        let mut other = LayerStore::open_at_model(&root.0, model).unwrap();
        let error = other
            .bind("Base", Trigger::TouchButton(1), emit("no"))
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "touch button out of range",
            "{}",
            model.id
        );
        assert!(other.layers()["Base"].bindings.is_empty(), "{}", model.id);
        assert!(!root.0.join("layers.conf").exists(), "{}", model.id);
    }
}

#[test]
fn malformed_touch_button_rows_are_removed_and_logged_like_other_controls() {
    let fixture = Fixture::new("malformed");
    let good = format!("binding\t{BASE}\ttb:2\temit:{}", hex("ok"));
    fs::write(
        fixture.0.join("layers.conf"),
        format!(
            "{HEADER}layer\t{BASE}\nbinding\t{BASE}\ttb:3\temit:61\n\
             binding\t{BASE}\ttb:0\temit:61\nbinding\t{BASE}\ttb:x\temit:61\n{good}\n"
        ),
    )
    .unwrap();
    let store = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    assert_eq!(store.conflicts().len(), 3);
    assert!(store.conflicts().iter().all(|conflict| !conflict.kept));
    assert_eq!(
        store.binding("Base", &Trigger::TouchButton(2)),
        Some(emit("ok"))
    );
    assert_eq!(
        fixture.read("layers.conf"),
        format!("{HEADER}layer\t{BASE}\n{good}\n")
    );
    let log = fixture.read(CONFLICT_LOG);
    assert_eq!(log.matches("removed=").count(), 3);
    assert!(log.contains("control=tb:3\treason=touch button out of range"));

    // A Stream Deck + knows the kind but has no touch buttons: same rule.
    let plus = Fixture::new("plus-row");
    fs::write(
        plus.0.join("layers.conf"),
        format!("{HEADER}layer\t{BASE}\n{good}\n"),
    )
    .unwrap();
    let store = LayerStore::open_at(&plus.0).unwrap();
    assert_eq!(store.conflicts().len(), 1);
    assert_eq!(store.conflicts()[0].reason, "touch button out of range");
}

#[test]
fn a_touch_runs_the_touch_buttons_own_binding_and_keys_are_unchanged() {
    let fixture = Fixture::new("dispatch");
    let mut store = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    store
        .bind("Base", Trigger::KeyPress(1), emit("key 1"))
        .unwrap();
    store
        .bind("Base", Trigger::KeyPress(8), emit("key 8"))
        .unwrap();
    store
        .bind("Base", Trigger::TouchButton(1), emit("left"))
        .unwrap();
    store
        .bind("Base", Trigger::TouchButton(2), emit("right"))
        .unwrap();
    assert_eq!(
        store.matching_binding(&touch(8, true)),
        Some((Trigger::TouchButton(1), emit("left")))
    );
    assert_eq!(
        store.matching_binding(&touch(9, true)),
        Some((Trigger::TouchButton(2), emit("right")))
    );
    assert_eq!(
        store.matching_binding(&touch(7, true)),
        Some((Trigger::KeyPress(8), emit("key 8")))
    );
    assert_eq!(store.matching_action(&touch(0, true)), Some(emit("key 1")));
    // A release runs nothing; neither does an index past the sensors.
    assert_eq!(store.matching_action(&touch(8, false)), None);
    assert_eq!(store.matching_action(&touch(10, true)), None);
    assert_eq!(
        Trigger::from_event_for(neo(), &touch(9, true)),
        Some(Trigger::TouchButton(2))
    );
    // Models without sensors keep reading the index as a key.
    assert_eq!(
        Trigger::from_event_for(&PLUS, &touch(8, true)),
        Some(Trigger::KeyPress(9))
    );
    assert_eq!(
        Trigger::from_event_for(&PLUS_XL, &touch(8, true)),
        Some(Trigger::KeyPress(9))
    );
    assert_eq!(
        Trigger::from_event(&touch(8, true)),
        Some(Trigger::KeyPress(9))
    );
    // Another page has its own touch buttons.
    store.create("Studio").unwrap();
    store
        .bind("Studio", Trigger::TouchButton(1), emit("studio"))
        .unwrap();
    assert!(store.select("Studio"));
    assert_eq!(store.matching_action(&touch(8, true)), Some(emit("studio")));
    assert_eq!(store.matching_action(&touch(9, true)), None);
}

#[test]
fn a_touch_fires_once_when_it_starts_and_never_on_release_or_repeat() {
    let mut presses = TouchButtonPresses::new(neo());
    assert_eq!(presses.update(8, true), Some(Trigger::TouchButton(1)));
    // Every report repeats the held state: no second run.
    assert_eq!(presses.update(8, true), None);
    assert_eq!(presses.update(9, true), Some(Trigger::TouchButton(2)));
    assert_eq!(presses.update(8, false), None);
    assert_eq!(presses.update(8, false), None);
    assert_eq!(presses.update(8, true), Some(Trigger::TouchButton(1)));
    // Keys and unknown indexes are not touch buttons.
    assert_eq!(presses.update(0, true), None);
    assert_eq!(presses.update(7, true), None);
    assert_eq!(presses.update(10, true), None);
    for model in without_touch_buttons() {
        let mut presses = TouchButtonPresses::new(model);
        let index = usize::from(model.keys());
        assert_eq!(presses.update(index, true), None, "{}", model.id);
        assert_eq!(presses.update(0, true), None, "{}", model.id);
    }
}

#[test]
fn touch_buttons_take_no_information_and_no_multi_action() {
    let fixture = Fixture::new("limits");
    let mut store = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    store
        .bind("Base", Trigger::TouchButton(1), emit("left"))
        .unwrap();
    assert!(store
        .set_info("Base", &Trigger::TouchButton(1), Some(InfoSource::Clock))
        .is_err());
    assert_eq!(store.info_source("Base", &Trigger::TouchButton(1)), None);
    let multi = Action::Feature(FeatureAction::Multi {
        steps: vec![MultiStep::Action(emit("one"))],
    });
    assert_eq!(
        store
            .bind("Base", Trigger::TouchButton(2), multi.clone())
            .unwrap_err()
            .to_string(),
        "multi actions require a key press or key hold"
    );
    assert_eq!(store.binding("Base", &Trigger::TouchButton(2)), None);
    assert!(store.bind("Base", Trigger::KeyPress(1), multi).is_ok());
}

#[test]
fn the_editor_offers_touch_buttons_only_on_models_that_have_them() {
    assert_eq!(
        controls_for(neo()),
        [ControlKind::Button, ControlKind::TouchButton]
    );
    assert_eq!(
        controls_for(&PLUS),
        [
            ControlKind::Button,
            ControlKind::Encoder,
            ControlKind::Touch
        ]
    );
    assert_eq!(
        controls_for(&PLUS_XL),
        [
            ControlKind::Button,
            ControlKind::Encoder,
            ControlKind::Touch
        ]
    );
    assert_eq!(controls_for(&CLASSIC), [ControlKind::Button]);
    assert_eq!(controls_for(&XL), [ControlKind::Button]);
    assert_eq!(
        events_for_control(ControlKind::TouchButton),
        [EventKind::ButtonPress]
    );
    let select = |model, number, event| {
        trigger_from_selection_for(model, ControlKind::TouchButton, number, event)
    };
    assert_eq!(
        select(neo(), 1, EventKind::ButtonPress).unwrap(),
        Trigger::TouchButton(1)
    );
    assert_eq!(
        select(neo(), 2, EventKind::ButtonPress).unwrap(),
        Trigger::TouchButton(2)
    );
    assert!(select(neo(), 0, EventKind::ButtonPress).is_err());
    assert!(select(neo(), 3, EventKind::ButtonPress).is_err());
    assert!(select(neo(), 1, EventKind::TouchTap).is_err());
    assert_eq!(
        control_number_error(neo(), ControlKind::TouchButton, 3).unwrap(),
        "Touch button 3 does not exist on the Stream Deck Neo; its touch buttons are 1..=2."
    );
    assert_eq!(
        control_number_error(neo(), ControlKind::TouchButton, 2),
        None
    );
    for model in without_touch_buttons() {
        assert!(
            select(model, 1, EventKind::ButtonPress).is_err(),
            "{}",
            model.id
        );
        assert_eq!(
            control_number_error(model, ControlKind::TouchButton, 1).unwrap(),
            format!("The {} has no touch buttons.", model.name)
        );
    }
}

#[test]
fn removing_a_touch_button_clears_it_and_undo_restores_it() {
    let fixture = Fixture::new("remove");
    let mut store = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    store
        .bind("Base", Trigger::TouchButton(2), emit("right"))
        .unwrap();
    store
        .bind("Base", Trigger::TouchButton(1), emit("left"))
        .unwrap();
    assert_eq!(
        control_triggers(&store, "Base", ControlKind::TouchButton, 2).unwrap(),
        [Trigger::TouchButton(2)]
    );
    assert!(control_triggers(&store, "Base", ControlKind::TouchButton, 3).is_err());
    assert_eq!(
        control_summary(&store, "Base", ControlKind::TouchButton, 2),
        "Press: Emit right"
    );
    let removed = store
        .unbind_control("Base", ControlKind::TouchButton, 2)
        .unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(store.binding("Base", &Trigger::TouchButton(2)), None);
    assert_eq!(
        store.binding("Base", &Trigger::TouchButton(1)),
        Some(emit("left"))
    );
    assert_eq!(
        control_summary(&store, "Base", ControlKind::TouchButton, 2),
        "Press: Unassigned"
    );
    store.restore_removed(&removed).unwrap();
    assert_eq!(
        store.binding("Base", &Trigger::TouchButton(2)),
        Some(emit("right"))
    );
    assert!(store
        .unbind_control("Base", ControlKind::TouchButton, 3)
        .is_err());
}

#[test]
fn bundles_and_pages_carry_touch_button_bindings() {
    let fixture = Fixture::new("bundle");
    let source = fixture.0.join("source");
    let mut store = LayerStore::open_at_model(&source, neo()).unwrap();
    let mut organization = Organization::open_at(&source, &store).unwrap();
    let page = organization.create_page(&mut store).unwrap();
    store
        .bind(&page, Trigger::TouchButton(1), emit("page"))
        .unwrap();
    store
        .bind("Base", Trigger::TouchButton(2), emit("base"))
        .unwrap();
    store.persist().unwrap();
    organization.persist().unwrap();
    let layers = fs::read_to_string(source.join("layers.conf")).unwrap();
    assert!(layers.contains("\ttb:1\t") && layers.contains("\ttb:2\t"));

    // The page keeps its touch button through a reopen of both stores.
    let reopened = LayerStore::open_at_model(&source, neo()).unwrap();
    let organization = Organization::open_at(&source, &reopened).unwrap();
    assert!(organization.profiles().contains(&"Base".to_owned()));
    assert_eq!(
        reopened.binding(&page, &Trigger::TouchButton(1)),
        Some(emit("page"))
    );

    let bundle = fixture.0.join("neo.sdeck-plus.json");
    config_bundle::export_for(&source, &bundle, neo()).unwrap();
    let target = fixture.0.join("target");
    config_bundle::import_for(&bundle, &target, neo()).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("layers.conf")).unwrap(),
        layers
    );
    let imported = LayerStore::open_at_model(&target, neo()).unwrap();
    assert!(imported.conflicts().is_empty());
    assert_eq!(
        imported.binding("Base", &Trigger::TouchButton(2)),
        Some(emit("base"))
    );
    // A Neo bundle is never imported over another model.
    assert!(config_bundle::import_for(&bundle, &fixture.0.join("plus"), &PLUS).is_err());
    assert!(!fixture.0.join("plus").join("layers.conf").exists());
}

#[test]
fn the_neo_preview_draws_its_touch_buttons_beside_the_window() {
    let layout = PreviewLayout::for_model(neo());
    let controls = layout.controls();
    let mut expected: Vec<_> = (1..=8).map(ControlRef::Key).collect();
    // The window (selected for its picture and live information) comes first.
    expected.extend([
        ControlRef::Window,
        ControlRef::TouchButton(1),
        ControlRef::TouchButton(2),
    ]);
    assert_eq!(controls, expected);
    assert_eq!(layout.touch_button_count(), 2);
    let strip = layout.strip.as_ref().expect("the Neo's window");
    assert!(!strip.touch);
    let left = layout.rect_of(ControlRef::TouchButton(1)).unwrap();
    let right = layout.rect_of(ControlRef::TouchButton(2)).unwrap();
    // One on each side of the window, inside its bezel, vertically centred on it.
    assert!(left.right() <= strip.rect.x && right.x >= strip.rect.right());
    for rect in [left, right] {
        assert!(strip.bezel.contains(&rect), "{rect:?}");
        assert!(!rect.overlaps(&strip.rect));
        assert!(rect.w >= MIN_HIT_PX && rect.h >= MIN_HIT_PX);
        assert_eq!(rect.y + rect.h / 2, strip.rect.y + strip.rect.h / 2);
        assert!(layout.chassis().contains(&rect));
        assert!(layout.keys.iter().all(|key| !key.overlaps(&rect)));
    }
    assert!(!layout.has(ControlRef::TouchButton(3)));
    assert!(!layout.has(ControlRef::TouchButton(0)));
    assert!(!layout.has(ControlRef::Strip));
    assert_eq!(
        ControlRef::TouchButton(2).accessible_name().unwrap(),
        "Touch button 2"
    );
    // Symmetric: the buttons mirror each other around the chassis centre.
    assert_eq!(left.x, layout.size.0 - right.right());
}

#[test]
fn models_without_touch_buttons_draw_none_and_the_plus_layouts_are_unchanged() {
    for model in without_touch_buttons() {
        let layout = PreviewLayout::for_model(model);
        assert_eq!(layout.touch_button_count(), 0, "{}", model.id);
        assert!(!layout.has(ControlRef::TouchButton(1)), "{}", model.id);
        assert!(
            !layout
                .controls()
                .iter()
                .any(|control| matches!(control, ControlRef::TouchButton(_))),
            "{}",
            model.id
        );
    }
    assert_eq!(PreviewLayout::for_model(&PLUS).size, (426, 414));
    assert_eq!(PreviewLayout::for_model(&PLUS_XL).size, (556, 456));
    assert_eq!(PreviewLayout::for_model(&PLUS).controls().len(), 17);
}
