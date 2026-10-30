use crate::{
    key_hold::{KeyDispatch, KeyHoldRecognizer, HOLD_DURATION},
    layers::{Action, LayerStore, PageReturn, Trigger},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        loop {
            let path = std::env::temp_dir().join(format!(
                "sdeck-key-hold-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test fixture: {error}"),
            }
        }
    }

    fn store(&self, hold: bool) -> LayerStore {
        let mut store = LayerStore::open_at(&self.0).unwrap();
        store
            .bind("Base", Trigger::KeyPress(1), Action::Emit("tap".into()))
            .unwrap();
        if hold {
            store
                .bind("Base", Trigger::KeyHold(1), Action::Emit("hold".into()))
                .unwrap();
        }
        store
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn expected(trigger: Trigger, name: &str) -> Vec<KeyDispatch> {
    vec![KeyDispatch {
        owner: "Base".into(),
        trigger,
        action: Action::Emit(name.into()),
        page_return: None,
    }]
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

#[test]
fn key_hold_editor_maps_optional_mode_and_preserves_existing_event_choices() {
    use crate::{
        binding_editor::{events_for_control, trigger_from_selection, ControlKind, EventKind},
        key_hold_editor::key_trigger,
    };
    assert_eq!(
        events_for_control(ControlKind::Button),
        vec![EventKind::ButtonPress]
    );
    for number in 1..=8 {
        assert_eq!(
            key_trigger(number, false).unwrap(),
            Trigger::KeyPress(number)
        );
        assert_eq!(key_trigger(number, true).unwrap(), Trigger::KeyHold(number));
    }
    for number in [0, 9, 255] {
        assert_eq!(
            key_trigger(number, false).unwrap_err().0,
            "control number is out of range"
        );
        assert_eq!(
            key_trigger(number, true).unwrap_err().0,
            "control number is out of range"
        );
    }
    assert_eq!(
        trigger_from_selection(ControlKind::Touch, 0, EventKind::TouchPress).unwrap(),
        Trigger::TouchPress
    );
    assert_eq!(
        trigger_from_selection(ControlKind::Touch, 1, EventKind::TouchPress).unwrap(),
        Trigger::ZonePress(1)
    );
}

#[test]
fn key_hold_summary_adds_secondary_action_only_when_configured() {
    use crate::binding_editor::{control_summary, ControlKind};
    let fixture = Fixture::new();
    let mut store = fixture.store(false);
    assert_eq!(
        control_summary(&store, "Base", ControlKind::Button, 1),
        "Press: Emit tap"
    );
    store
        .bind("Base", Trigger::KeyHold(1), Action::Emit("hold".into()))
        .unwrap();
    assert_eq!(
        control_summary(&store, "Base", ControlKind::Button, 1),
        "Press: Emit tap\nHold 2 seconds: Emit hold"
    );
    assert_eq!(
        control_summary(&store, "Base", ControlKind::Button, 2),
        "Press: Unassigned"
    );
    store.unbind("Base", &Trigger::KeyHold(1)).unwrap();
    assert_eq!(
        control_summary(&store, "Base", ControlKind::Button, 1),
        "Press: Emit tap"
    );
}

#[test]
fn key_hold_unconfigured_down_is_immediate_and_snapshots_are_deduplicated() {
    let fixture = Fixture::new();
    let store = fixture.store(false);
    let mut keys = KeyHoldRecognizer::default();
    assert_eq!(
        keys.update(&store, 0, true, ms(0)),
        expected(Trigger::KeyPress(1), "tap")
    );
    assert!(keys.update(&store, 0, true, ms(100)).is_empty());
    assert!(keys.poll(&store, ms(3000)).is_empty());
    assert!(keys.update(&store, 0, false, ms(3000)).is_empty());
    assert!(keys.update(&store, 0, false, ms(3001)).is_empty());
    assert_eq!(
        keys.update(&store, 0, true, ms(4000)),
        expected(Trigger::KeyPress(1), "tap")
    );
    assert!(keys.update(&store, usize::MAX, true, ms(4000)).is_empty());
}

#[test]
fn key_hold_release_before_boundary_dispatches_only_tap_once() {
    let fixture = Fixture::new();
    let store = fixture.store(true);
    let mut keys = KeyHoldRecognizer::default();
    assert!(keys.update(&store, 0, true, ms(100)).is_empty());
    assert_eq!(keys.next_deadline(), Some(ms(2100)));
    assert!(keys.update(&store, 0, true, ms(2000)).is_empty());
    assert!(keys.poll(&store, ms(2099)).is_empty());
    assert_eq!(
        keys.update(&store, 0, false, ms(2099)),
        expected(Trigger::KeyPress(1), "tap")
    );
    assert_eq!(keys.next_deadline(), None);
    assert!(keys.update(&store, 0, false, ms(2100)).is_empty());
    assert!(keys.poll(&store, ms(9000)).is_empty());
}

#[test]
fn key_hold_exact_boundary_fires_once_and_later_release_never_taps() {
    let fixture = Fixture::new();
    let store = fixture.store(true);
    let mut keys = KeyHoldRecognizer::default();
    assert_eq!(HOLD_DURATION, ms(2000));
    assert!(keys.update(&store, 0, true, ms(0)).is_empty());
    assert!(keys.poll(&store, ms(1999)).is_empty());
    assert_eq!(
        keys.poll(&store, ms(2000)),
        expected(Trigger::KeyHold(1), "hold")
    );
    assert_eq!(keys.next_deadline(), None);
    assert!(keys.poll(&store, ms(2001)).is_empty());
    assert!(keys.update(&store, 0, true, ms(3000)).is_empty());
    assert!(keys.update(&store, 0, false, ms(4000)).is_empty());
}

#[test]
fn key_hold_delayed_release_at_or_after_boundary_dispatches_hold_not_tap() {
    let fixture = Fixture::new();
    let store = fixture.store(true);
    for released in [2000, 2001, 9000] {
        let mut keys = KeyHoldRecognizer::default();
        assert!(keys.update(&store, 0, true, ms(0)).is_empty());
        assert_eq!(
            keys.update(&store, 0, false, ms(released)),
            expected(Trigger::KeyHold(1), "hold")
        );
        assert!(keys.poll(&store, ms(10000)).is_empty());
        assert!(keys.update(&store, 0, false, ms(10001)).is_empty());
    }
}

#[test]
fn key_hold_snapshots_actions_and_followup_at_press() {
    let fixture = Fixture::new();
    for release in [1000, 2000] {
        let mut store = fixture.store(true);
        store.create("Other").unwrap();
        let followup = PageReturn {
            layer: "Other".into(),
            seconds: 5,
        };
        for trigger in [Trigger::KeyPress(1), Trigger::KeyHold(1)] {
            store
                .set_return("Base", &trigger, Some(followup.clone()))
                .unwrap();
        }
        let mut keys = KeyHoldRecognizer::default();
        assert!(keys.update(&store, 0, true, ms(0)).is_empty());
        store.unbind("Base", &Trigger::KeyPress(1)).unwrap();
        store.unbind("Base", &Trigger::KeyHold(1)).unwrap();
        let (trigger, name) = if release < 2000 {
            (Trigger::KeyPress(1), "tap")
        } else {
            (Trigger::KeyHold(1), "hold")
        };
        let mut dispatch = expected(trigger, name);
        dispatch[0].page_return = Some(followup);
        assert_eq!(keys.update(&store, 0, false, ms(release)), dispatch);
    }
}

#[test]
fn key_hold_owner_change_cancels_and_suppresses_until_release() {
    let fixture = Fixture::new();
    let mut store = fixture.store(true);
    store.create("Other").unwrap();
    store
        .bind("Other", Trigger::KeyPress(1), Action::Emit("other".into()))
        .unwrap();
    let mut keys = KeyHoldRecognizer::default();
    assert!(keys.update(&store, 0, true, ms(0)).is_empty());
    store.select("Other");
    assert!(keys.poll(&store, ms(2000)).is_empty());
    assert!(keys.update(&store, 0, true, ms(2001)).is_empty());
    store.select("Base");
    assert!(keys.update(&store, 0, true, ms(3000)).is_empty());
    assert!(keys.update(&store, 0, false, ms(3001)).is_empty());
    assert!(keys.update(&store, 0, true, ms(4000)).is_empty());
    assert_eq!(
        keys.update(&store, 0, false, ms(4001)),
        expected(Trigger::KeyPress(1), "tap")
    );
}

#[test]
fn key_hold_explicit_cancel_for_disconnect_shutdown_import_suppresses_held_keys() {
    let fixture = Fixture::new();
    let store = fixture.store(true);
    let mut keys = KeyHoldRecognizer::default();
    for start in [0, 4000, 8000] {
        assert!(keys.update(&store, 0, true, ms(start)).is_empty());
        keys.cancel();
        assert_eq!(keys.next_deadline(), None);
        assert!(keys.poll(&store, ms(start + 3000)).is_empty());
        assert!(keys.update(&store, 0, true, ms(start + 3001)).is_empty());
        assert!(keys.update(&store, 0, false, ms(start + 3002)).is_empty());
    }
    assert!(keys.update(&store, 0, true, ms(12000)).is_empty());
    assert_eq!(
        keys.update(&store, 0, false, ms(12001)),
        expected(Trigger::KeyPress(1), "tap")
    );
}

#[test]
fn key_hold_down_snapshot_at_deadline_dispatches_explicit_none_only_once() {
    let fixture = Fixture::new();
    let mut store = fixture.store(false);
    store
        .bind("Base", Trigger::KeyHold(1), Action::None)
        .unwrap();
    let mut keys = KeyHoldRecognizer::default();
    assert!(keys.update(&store, 0, true, ms(0)).is_empty());
    assert!(keys.update(&store, 0, true, ms(1999)).is_empty());
    assert_eq!(
        keys.update(&store, 0, true, ms(2000)),
        vec![KeyDispatch {
            owner: "Base".into(),
            trigger: Trigger::KeyHold(1),
            action: Action::None,
            page_return: None,
        }]
    );
    assert!(keys.poll(&store, ms(3000)).is_empty());
    assert!(keys.update(&store, 0, false, ms(3001)).is_empty());
}

#[test]
fn key_hold_new_secondary_binding_does_not_retarget_an_already_pressed_key() {
    let fixture = Fixture::new();
    let mut store = fixture.store(false);
    let mut keys = KeyHoldRecognizer::default();
    assert_eq!(
        keys.update(&store, 0, true, ms(0)),
        expected(Trigger::KeyPress(1), "tap")
    );
    store
        .bind("Base", Trigger::KeyHold(1), Action::Emit("hold".into()))
        .unwrap();
    assert!(keys.update(&store, 0, true, ms(2000)).is_empty());
    assert!(keys.update(&store, 0, false, ms(2001)).is_empty());
    assert!(keys.update(&store, 0, true, ms(3000)).is_empty());
    assert_eq!(
        keys.poll(&store, ms(5000)),
        expected(Trigger::KeyHold(1), "hold")
    );
}

#[test]
fn key_hold_remains_keypad_owned_when_touchscreen_pages_change() {
    use crate::{local_artwork::ArtworkStore, organization::Organization};
    let fixture = Fixture::new();
    let mut store = fixture.store(true);
    let organization = Organization::open_at(&fixture.0, &store).unwrap();
    let mut artwork = ArtworkStore::open_at_for_layer(&fixture.0, Some("Base")).unwrap();
    store
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(
        store
            .binding_layer("Base", &Trigger::KeyHold(1))
            .unwrap()
            .name,
        "Base"
    );
    let mut keys = KeyHoldRecognizer::default();
    assert!(keys.update(&store, 0, true, ms(0)).is_empty());
    store.create_touchscreen_page("Base").unwrap();
    assert_eq!(
        keys.poll(&store, ms(2000)),
        expected(Trigger::KeyHold(1), "hold")
    );
    store.persist().unwrap();
    let loaded = LayerStore::open_at(&fixture.0).unwrap();
    assert_eq!(
        loaded.binding("Base", &Trigger::KeyHold(1)),
        Some(Action::Emit("hold".into()))
    );
    assert!(loaded.diagnostics().is_empty());
    assert!(loaded.conflicts().is_empty());
}

#[test]
fn key_hold_independent_keys_and_hold_only_binding() {
    let fixture = Fixture::new();
    let mut store = fixture.store(true);
    store
        .bind("Base", Trigger::KeyHold(8), Action::Emit("eighth".into()))
        .unwrap();
    store
        .bind("Base", Trigger::KeyPress(2), Action::Emit("second".into()))
        .unwrap();
    let mut keys = KeyHoldRecognizer::default();
    assert!(keys.update(&store, 0, true, ms(0)).is_empty());
    assert!(keys.update(&store, 7, true, ms(500)).is_empty());
    assert_eq!(
        keys.update(&store, 1, true, ms(600)),
        expected(Trigger::KeyPress(2), "second")
    );
    assert_eq!(
        keys.poll(&store, ms(2000)),
        expected(Trigger::KeyHold(1), "hold")
    );
    assert_eq!(keys.next_deadline(), Some(ms(2500)));
    assert_eq!(
        keys.poll(&store, ms(2500)),
        expected(Trigger::KeyHold(8), "eighth")
    );
    assert!(keys.update(&store, 7, false, ms(2501)).is_empty());
    assert!(keys.update(&store, 7, true, ms(3000)).is_empty());
    assert!(keys.update(&store, 7, false, ms(3001)).is_empty());
}
