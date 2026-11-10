use super::{Action, LayerStore, PageReturn, RemovedBinding, SwipeDirection, Trigger};
use crate::{
    binding_editor::{control_summary, dial_action, ControlKind},
    desktop_actions::{DesktopAction, MediaCommand},
    features::FeatureAction,
    info::InfoSource,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sdeck-control-remove-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn store(&self) -> LayerStore {
        LayerStore::open_at(&self.0).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn media(command: MediaCommand) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Media(command)))
}

fn text(value: &str) -> Action {
    Action::Emit(value.into())
}

fn summary_lines(store: &LayerStore, control: ControlKind, number: u8) -> Vec<String> {
    control_summary(store, "Base", control, number)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn all_unassigned(store: &LayerStore, control: ControlKind, number: u8) -> bool {
    summary_lines(store, control, number)
        .iter()
        .all(|line| line.ends_with("Unassigned"))
}

#[test]
fn dial_rebound_from_track_to_volume_is_unassigned_after_removing_the_control() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    for (trigger, action) in [
        (Trigger::EncoderClockwise(1), media(MediaCommand::Next)),
        (
            Trigger::EncoderCounterClockwise(1),
            media(MediaCommand::Previous),
        ),
        (Trigger::EncoderPress(1), media(MediaCommand::PlayPause)),
        (Trigger::EncoderClockwise(1), media(MediaCommand::VolumeUp)),
        (
            Trigger::EncoderCounterClockwise(1),
            media(MediaCommand::VolumeDown),
        ),
        (Trigger::EncoderPress(1), media(MediaCommand::Mute)),
    ] {
        store.bind("Base", trigger, action).unwrap();
    }
    let removed = store
        .unbind_control("Base", ControlKind::Encoder, 1)
        .unwrap();
    assert_eq!(removed.len(), 3);
    assert_eq!(dial_action(&store, "Base", 1), None);
    assert!(all_unassigned(&store, ControlKind::Encoder, 1));
}

#[test]
fn key_removal_takes_press_and_hold_with_their_returns_and_displays() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    store.create("Studio").unwrap();
    store
        .bind("Base", Trigger::KeyPress(2), text("tap"))
        .unwrap();
    store
        .bind("Base", Trigger::KeyHold(2), text("hold"))
        .unwrap();
    let back = PageReturn {
        layer: "Studio".into(),
        seconds: 30,
    };
    store
        .set_return("Base", &Trigger::KeyHold(2), Some(back.clone()))
        .unwrap();
    store
        .set_info("Base", &Trigger::KeyPress(2), Some(InfoSource::Clock))
        .unwrap();
    let removed = store
        .unbind_control("Base", ControlKind::Button, 2)
        .unwrap();
    assert_eq!(removed.len(), 2);
    assert!(store.binding("Base", &Trigger::KeyPress(2)).is_none());
    assert!(store.binding("Base", &Trigger::KeyHold(2)).is_none());
    assert_eq!(store.page_return("Base", &Trigger::KeyHold(2)), None);
    assert_eq!(store.info_source("Base", &Trigger::KeyPress(2)), None);
    assert!(all_unassigned(&store, ControlKind::Button, 2));
}

#[test]
fn removing_a_key_with_one_event_still_removes_just_that_binding() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    store.bind("Base", Trigger::KeyPress(3), text("a")).unwrap();
    let removed = store
        .unbind_control("Base", ControlKind::Button, 3)
        .unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].trigger, Trigger::KeyPress(3));
    assert!(store
        .unbind_control("Base", ControlKind::Button, 3)
        .unwrap()
        .is_empty());
}

#[test]
fn legacy_rotation_ticks_are_removed_for_that_dial_only() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    store
        .bind("Base", Trigger::Rotation(2, 3), text("exact"))
        .unwrap();
    store
        .bind("Base", Trigger::Rotation(2, -1), text("back"))
        .unwrap();
    store
        .bind("Base", Trigger::Rotation(3, 3), text("other"))
        .unwrap();
    let removed = store
        .unbind_control("Base", ControlKind::Encoder, 2)
        .unwrap();
    assert_eq!(removed.len(), 2);
    assert!(store.binding("Base", &Trigger::Rotation(2, 3)).is_none());
    assert!(store.binding("Base", &Trigger::Rotation(2, -1)).is_none());
    assert!(store.binding("Base", &Trigger::Rotation(3, 3)).is_some());
    assert!(all_unassigned(&store, ControlKind::Encoder, 2));
}

#[test]
fn touch_strip_removal_takes_every_whole_strip_gesture_but_not_sections() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    let strip = [
        Trigger::TouchTap,
        Trigger::TouchPress,
        Trigger::TouchFlick,
        Trigger::TouchSwipe(SwipeDirection::Left),
        Trigger::TouchSwipe(SwipeDirection::Right),
        Trigger::TouchSwipe(SwipeDirection::Up),
        Trigger::TouchSwipe(SwipeDirection::Down),
    ];
    for trigger in &strip {
        store.bind("Base", trigger.clone(), text("strip")).unwrap();
    }
    store
        .bind("Base", Trigger::ZoneTap(1), text("zone"))
        .unwrap();
    let removed = store.unbind_control("Base", ControlKind::Touch, 0).unwrap();
    assert_eq!(removed.len(), strip.len());
    for trigger in &strip {
        assert!(store.binding("Base", trigger).is_none(), "{trigger:?}");
    }
    assert!(store.binding("Base", &Trigger::ZoneTap(1)).is_some());
}

#[test]
fn touch_section_removal_takes_its_tap_hold_returns_and_displays_only() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    store
        .bind("Base", Trigger::ZoneTap(2), text("tap"))
        .unwrap();
    store
        .bind("Base", Trigger::ZonePress(2), text("hold"))
        .unwrap();
    store
        .bind("Base", Trigger::ZoneTap(3), text("keep"))
        .unwrap();
    store
        .bind("Base", Trigger::TouchFlick, text("strip"))
        .unwrap();
    store
        .set_info("Base", &Trigger::ZonePress(2), Some(InfoSource::Volume))
        .unwrap();
    let removed = store.unbind_control("Base", ControlKind::Touch, 2).unwrap();
    assert_eq!(removed.len(), 2);
    assert_eq!(store.info_source("Base", &Trigger::ZonePress(2)), None);
    assert!(store.binding("Base", &Trigger::ZoneTap(3)).is_some());
    assert!(store.binding("Base", &Trigger::TouchFlick).is_some());
}

#[test]
fn other_controls_and_layers_are_untouched() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    store.create("Studio").unwrap();
    store
        .bind("Base", Trigger::EncoderPress(1), text("a"))
        .unwrap();
    store
        .bind("Base", Trigger::EncoderPress(2), text("b"))
        .unwrap();
    store.bind("Base", Trigger::KeyPress(1), text("c")).unwrap();
    store
        .bind("Studio", Trigger::KeyPress(1), text("d"))
        .unwrap();
    store
        .unbind_control("Base", ControlKind::Encoder, 1)
        .unwrap();
    assert!(store.binding("Base", &Trigger::EncoderPress(2)).is_some());
    assert!(store.binding("Base", &Trigger::KeyPress(1)).is_some());
    assert!(store.binding("Studio", &Trigger::KeyPress(1)).is_some());
    assert!(store.layers().contains_key("Base"));
}

#[test]
fn rejected_removals_leave_the_store_unchanged() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    store.bind("Base", Trigger::KeyPress(1), text("a")).unwrap();
    assert!(store
        .unbind_control("Missing", ControlKind::Button, 1)
        .is_err());
    assert!(store
        .unbind_control("Base", ControlKind::Button, 9)
        .is_err());
    assert!(store
        .unbind_control("Base", ControlKind::Encoder, 5)
        .is_err());
    assert!(store.binding("Base", &Trigger::KeyPress(1)).is_some());
}

#[test]
fn restoring_returns_every_binding_return_and_display_exactly() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    store.create("Studio").unwrap();
    store
        .bind("Base", Trigger::EncoderPress(1), text("p"))
        .unwrap();
    store
        .bind("Base", Trigger::EncoderClockwise(1), text("cw"))
        .unwrap();
    store
        .bind("Base", Trigger::Rotation(1, 4), text("legacy"))
        .unwrap();
    store
        .set_return(
            "Base",
            &Trigger::EncoderPress(1),
            Some(PageReturn {
                layer: "Studio".into(),
                seconds: 12,
            }),
        )
        .unwrap();
    store
        .set_info(
            "Base",
            &Trigger::EncoderClockwise(1),
            Some(InfoSource::Clock),
        )
        .unwrap();
    let before: Vec<_> = {
        let layer = store
            .binding_layer("Base", &Trigger::EncoderPress(1))
            .unwrap();
        vec![
            format!("{:?}", layer.bindings),
            format!("{:?}", layer.returns),
            format!("{:?}", layer.displays),
        ]
    };
    let removed: Vec<RemovedBinding> = store
        .unbind_control("Base", ControlKind::Encoder, 1)
        .unwrap();
    assert_eq!(removed.len(), 3);
    store.restore_removed(&removed).unwrap();
    let layer = store
        .binding_layer("Base", &Trigger::EncoderPress(1))
        .unwrap();
    let after = vec![
        format!("{:?}", layer.bindings),
        format!("{:?}", layer.returns),
        format!("{:?}", layer.displays),
    ];
    assert_eq!(before, after);
}

#[test]
fn a_failed_restore_changes_nothing() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    store.bind("Base", Trigger::KeyPress(1), text("a")).unwrap();
    let mut removed = store
        .unbind_control("Base", ControlKind::Button, 1)
        .unwrap();
    removed.push(RemovedBinding {
        layer: "Gone".into(),
        trigger: Trigger::KeyPress(2),
        action: text("b"),
        page_return: None,
        info: None,
    });
    assert!(store.restore_removed(&removed).is_err());
    assert!(store.binding("Base", &Trigger::KeyPress(1)).is_none());
}
