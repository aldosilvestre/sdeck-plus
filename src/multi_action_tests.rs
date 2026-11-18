//! Multi action storage and validation through the public store API.

use crate::{
    desktop_actions::{DesktopAction, MediaCommand},
    features::{FeatureAction, MultiStep},
    layers::{Action, LayerStore, Trigger},
    organization::NavigationAction,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "streamdeck-multi-action-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).expect("remove test fixture");
        }
    }
}

fn multi(steps: Vec<MultiStep>) -> Action {
    Action::Feature(FeatureAction::Multi { steps })
}

fn step(action: Action) -> MultiStep {
    MultiStep::Action(action)
}

fn shortcut() -> Action {
    Action::Shortcut {
        modifiers: vec!["CTRL".into()],
        key: "C".into(),
    }
}

#[test]
fn a_multi_action_survives_save_and_reload_with_every_step_in_order() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store.create("Other").unwrap();
    let action = multi(vec![
        step(shortcut()),
        MultiStep::Wait { milliseconds: 250 },
        step(Action::Command("echo one".into())),
        step(Action::Feature(FeatureAction::Desktop(
            DesktopAction::Media(MediaCommand::PlayPause),
        ))),
        step(Action::Launch {
            executable: "/usr/bin/true".into(),
            args: vec!["a b".into()],
        }),
        step(Action::SwitchLayer("Other".into())),
    ]);
    store
        .bind("Base", Trigger::KeyPress(3), action.clone())
        .unwrap();
    store
        .bind("Base", Trigger::KeyHold(4), action.clone())
        .unwrap();
    store.persist().unwrap();

    let loaded = LayerStore::open_at(&fixture.0).unwrap();
    assert_eq!(
        loaded.binding("Base", &Trigger::KeyPress(3)),
        Some(action.clone())
    );
    assert_eq!(loaded.binding("Base", &Trigger::KeyHold(4)), Some(action));
    assert_eq!(
        FeatureAction::Multi {
            steps: vec![step(shortcut()), MultiStep::Wait { milliseconds: 10 }]
        }
        .summary(),
        "Multi action (2 steps)"
    );
}

#[test]
fn a_multi_action_is_stored_as_one_feature_row_older_builds_keep() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(2),
            multi(vec![
                step(shortcut()),
                MultiStep::Wait {
                    milliseconds: 60_000,
                },
            ]),
        )
        .unwrap();
    store.persist().unwrap();
    let text = fs::read_to_string(fixture.0.join("layers.conf")).unwrap();
    assert!(text.starts_with("streamdeck-layers-v1"), "{text}");
    let row = text
        .lines()
        .find(|line| line.contains("feature-v1:"))
        .expect("a feature row");
    let hex = row.rsplit("feature-v1:").next().unwrap();
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let object = json.as_object().unwrap();
    // One key that older builds do not know: they keep the row unchanged.
    assert_eq!(object.keys().collect::<Vec<_>>(), ["Multi"]);
    assert_eq!(
        json["Multi"]["steps"][1],
        serde_json::json!({"Wait": {"milliseconds": 60000}})
    );
}

#[test]
fn invalid_multi_actions_are_refused_without_changing_bindings_or_disk() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store.create("Other").unwrap();
    store
        .bind("Base", Trigger::KeyPress(1), Action::Emit("keep".into()))
        .unwrap();
    store.persist().unwrap();
    let original = fs::read(fixture.0.join("layers.conf")).unwrap();
    let bindings = store.layers()["Base"].bindings.clone();
    let wait = |milliseconds| MultiStep::Wait { milliseconds };
    let nested = multi(vec![step(shortcut())]);
    let too_long = (0..16)
        .map(|_| step(Action::Command("x".repeat(200))))
        .collect();
    for (trigger, action, message) in [
        (
            Trigger::KeyPress(1),
            multi(vec![]),
            "A multi action needs 1 to 16 steps",
        ),
        (
            Trigger::KeyPress(1),
            multi((0..17).map(|_| step(shortcut())).collect()),
            "A multi action needs 1 to 16 steps",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![wait(100)]),
            "A multi action needs at least one action step",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![step(shortcut()), wait(9)]),
            "Step 2: a wait must last 10 ms to 60 seconds",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![step(shortcut()), wait(60_001)]),
            "Step 2: a wait must last 10 ms to 60 seconds",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![step(shortcut()), step(nested)]),
            "Step 2: a multi action cannot contain another multi action",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![step(Action::Feature(FeatureAction::Timer {
                seconds: 5,
            }))]),
            "Step 1: timers cannot be steps of a multi action",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![step(Action::Feature(FeatureAction::CancelTimer))]),
            "Step 1: timers cannot be steps of a multi action",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![step(Action::None)]),
            "Step 1: None cannot be a step of a multi action",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![
                step(Action::SwitchLayer("Other".into())),
                step(shortcut()),
            ]),
            "Step 1: Go to profile or page can only be the last step",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![
                step(Action::Feature(FeatureAction::Navigation(
                    NavigationAction::NextPage,
                ))),
                wait(100),
            ]),
            "Step 1: Go to profile or page can only be the last step",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![step(Action::Feature(FeatureAction::Touchscreen(
                crate::features::TouchscreenAction::NextPage,
            )))]),
            "Step 1: touchscreen page actions require a touchscreen gesture",
        ),
        (
            Trigger::KeyPress(1),
            multi(vec![step(shortcut()), step(Action::Command(" ".into()))]),
            "Step 2: commands must be non-empty shell strings without NUL bytes",
        ),
        (
            Trigger::KeyPress(1),
            multi(too_long),
            "Multi action is too large to store",
        ),
        (
            Trigger::EncoderPress(1),
            multi(vec![step(shortcut())]),
            "multi actions require a key press or key hold",
        ),
        (
            Trigger::ZoneTap(1),
            multi(vec![step(shortcut())]),
            "multi actions require a key press or key hold",
        ),
    ] {
        let error = store.bind("Base", trigger, action).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), message);
        assert_eq!(store.layers()["Base"].bindings, bindings);
        assert_eq!(fs::read(fixture.0.join("layers.conf")).unwrap(), original);
    }
}

#[test]
fn a_stored_multi_action_with_an_invalid_step_is_not_loaded() {
    for json in [
        r#"{"Multi":{"steps":[{"Action":"none"}]}}"#,
        r#"{"Multi":{"steps":[{"Action":"bogus:00"}]}}"#,
        r#"{"Multi":{"steps":[{"Wait":{"milliseconds":5}}]}}"#,
    ] {
        assert!(FeatureAction::decode(json).is_err(), "{json}");
    }
}
