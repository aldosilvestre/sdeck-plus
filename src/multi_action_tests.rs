//! Multi action storage and validation through the public store API.

use crate::{
    asset_links::{link_action, missing_target, unlink_action},
    desktop_actions::{DesktopAction, MediaCommand},
    features::{FeatureAction, MultiStep},
    layers::{normalize_keyboard_action, Action, LayerStore, Trigger},
    organization::NavigationAction,
};
use std::{
    fs,
    path::{Path, PathBuf},
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

fn go_to(page: &str) -> MultiStep {
    step(Action::SwitchLayer(page.into()))
}

fn wait(milliseconds: u32) -> MultiStep {
    MultiStep::Wait { milliseconds }
}

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

fn sound(path: &Path) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio {
        path: path.to_path_buf(),
    }))
}

fn open(target: &str) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Open {
        target: target.into(),
    }))
}

#[test]
fn removing_a_page_drops_only_the_multi_steps_that_go_to_it() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store.create("Other").unwrap();
    store.create("Third").unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(1),
            multi(vec![step(shortcut()), wait(100), go_to("Other")]),
        )
        .unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(2),
            multi(vec![wait(100), go_to("Other")]),
        )
        .unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(3),
            multi(vec![step(shortcut()), go_to("Third")]),
        )
        .unwrap();
    store
        .bind(
            "Third",
            Trigger::KeyHold(1),
            multi(vec![step(Action::Emit("a".into())), go_to("Other")]),
        )
        .unwrap();

    assert!(store.remove("Other").unwrap());
    store.persist().unwrap();

    let loaded = LayerStore::open_at(&fixture.0).unwrap();
    assert!(loaded.conflicts().is_empty(), "{:?}", loaded.conflicts());
    for layers in [&store, &loaded] {
        assert_eq!(
            layers.binding("Base", &Trigger::KeyPress(1)),
            Some(multi(vec![step(shortcut()), wait(100)]))
        );
        assert_eq!(layers.binding("Base", &Trigger::KeyPress(2)), None);
        assert_eq!(
            layers.binding("Base", &Trigger::KeyPress(3)),
            Some(multi(vec![step(shortcut()), go_to("Third")]))
        );
        assert_eq!(
            layers.binding("Third", &Trigger::KeyHold(1)),
            Some(multi(vec![step(Action::Emit("a".into()))]))
        );
    }
}

#[test]
fn removing_a_page_keeps_the_page_return_of_a_multi_action_that_survives() {
    use crate::layers::PageReturn;
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store.create("Other").unwrap();
    store.create("Third").unwrap();
    let back = Some(PageReturn {
        layer: "Third".into(),
        seconds: 5,
    });
    store
        .bind(
            "Base",
            Trigger::KeyPress(1),
            multi(vec![step(shortcut()), wait(100), go_to("Other")]),
        )
        .unwrap();
    store
        .set_return("Base", &Trigger::KeyPress(1), back.clone())
        .unwrap();

    assert!(store.remove("Other").unwrap());
    store.persist().unwrap();

    let loaded = LayerStore::open_at(&fixture.0).unwrap();
    assert!(loaded.conflicts().is_empty(), "{:?}", loaded.conflicts());
    for layers in [&store, &loaded] {
        assert_eq!(
            layers.binding("Base", &Trigger::KeyPress(1)),
            Some(multi(vec![step(shortcut()), wait(100)]))
        );
        assert_eq!(layers.page_return("Base", &Trigger::KeyPress(1)), back);
    }
}

/// Previous behavior of the shared page cleanup on touchscreen pages: a plain
/// Go to page there leaves with the page it goes to, and so does its return.
#[test]
fn removing_a_page_still_unbinds_a_plain_touch_go_to_page() {
    use crate::{layers::PageReturn, local_artwork::ArtworkStore, organization::Organization};
    let fixture = Fixture::new();
    let mut layers = LayerStore::open_at(&fixture.0).unwrap();
    let mut organization = Organization::open_at(&fixture.0, &layers).unwrap();
    let mut artwork = ArtworkStore::open_at_for_layer(&fixture.0, Some(layers.active())).unwrap();
    let page = organization.create_page(&mut layers).unwrap();
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    layers
        .bind(
            "Base",
            Trigger::ZoneTap(1),
            Action::SwitchLayer(page.clone()),
        )
        .unwrap();
    layers
        .set_return(
            "Base",
            &Trigger::ZoneTap(1),
            Some(PageReturn {
                layer: "Base".into(),
                seconds: 2,
            }),
        )
        .unwrap();
    layers
        .bind("Base", Trigger::ZoneTap(2), Action::Emit("keep".into()))
        .unwrap();

    assert!(layers.remove(&page).unwrap());

    assert_eq!(layers.binding("Base", &Trigger::ZoneTap(1)), None);
    assert_eq!(layers.page_return("Base", &Trigger::ZoneTap(1)), None);
    assert_eq!(
        layers.binding("Base", &Trigger::ZoneTap(2)),
        Some(Action::Emit("keep".into()))
    );
}

#[test]
fn a_multi_action_that_ends_on_an_unknown_page_is_refused() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store
        .bind("Base", Trigger::KeyPress(1), Action::Emit("keep".into()))
        .unwrap();
    store.persist().unwrap();
    let original = fs::read(fixture.0.join("layers.conf")).unwrap();
    let bindings = store.layers()["Base"].bindings.clone();

    for trigger in [Trigger::KeyPress(1), Trigger::KeyPress(2)] {
        let error = store
            .bind(
                "Base",
                trigger,
                multi(vec![step(shortcut()), go_to("Missing")]),
            )
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "unknown target layer");
        assert_eq!(store.layers()["Base"].bindings, bindings);
        assert_eq!(store.names(), ["Base"]);
        assert_eq!(fs::read(fixture.0.join("layers.conf")).unwrap(), original);
    }
}

/// Previous behavior of the shared target check, for a plain Go to page.
#[test]
fn a_plain_go_to_page_still_needs_an_existing_target() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store.persist().unwrap();
    let original = fs::read(fixture.0.join("layers.conf")).unwrap();

    let error = store
        .bind(
            "Base",
            Trigger::KeyPress(1),
            Action::SwitchLayer("Missing".into()),
        )
        .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(error.to_string(), "unknown target layer");
    assert!(store.layers()["Base"].bindings.is_empty());
    assert_eq!(fs::read(fixture.0.join("layers.conf")).unwrap(), original);
}

#[test]
fn keyboard_steps_are_stored_normalized_like_top_level_keyboard_actions() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    let chord = Action::Shortcut {
        modifiers: vec!["SUPER".into(), "control".into()],
        key: "s".into(),
    };
    let key = Action::KeyPress {
        key: "RETURN".into(),
    };
    let normalized_chord = normalize_keyboard_action(chord.clone()).unwrap();
    let normalized_key = normalize_keyboard_action(key.clone()).unwrap();
    assert_ne!(normalized_chord, chord, "the sample must need normalizing");
    assert_ne!(normalized_key, key, "the sample must need normalizing");
    store
        .bind(
            "Base",
            Trigger::KeyPress(1),
            multi(vec![step(chord.clone()), wait(50), step(key.clone())]),
        )
        .unwrap();
    store
        .bind("Base", Trigger::KeyPress(2), chord.clone())
        .unwrap();
    store.persist().unwrap();

    let loaded = LayerStore::open_at(&fixture.0).unwrap();
    assert_eq!(
        loaded.binding("Base", &Trigger::KeyPress(1)),
        Some(multi(vec![
            step(normalized_chord.clone()),
            wait(50),
            step(normalized_key)
        ]))
    );
    assert_eq!(
        loaded.binding("Base", &Trigger::KeyPress(2)),
        Some(normalized_chord)
    );

    // A step that cannot be normalized is refused like a top-level one, naming its step.
    let unknown = Action::KeyPress {
        key: "unknown".into(),
    };
    let expected = normalize_keyboard_action(unknown.clone()).unwrap_err();
    let bindings = store.layers()["Base"].bindings.clone();
    let original = fs::read(fixture.0.join("layers.conf")).unwrap();
    let error = store
        .bind(
            "Base",
            Trigger::KeyPress(3),
            multi(vec![step(shortcut()), step(unknown)]),
        )
        .unwrap_err();
    assert_eq!(error.kind(), expected.kind());
    assert_eq!(error.to_string(), format!("Step 2: {expected}"));
    assert_eq!(store.layers()["Base"].bindings, bindings);
    assert_eq!(fs::read(fixture.0.join("layers.conf")).unwrap(), original);
}

#[test]
fn a_stored_multi_action_loses_only_its_step_to_a_missing_page() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store.create("Other").unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(1),
            multi(vec![step(shortcut()), wait(100), go_to("Other")]),
        )
        .unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(2),
            multi(vec![wait(100), go_to("Other")]),
        )
        .unwrap();
    store.persist().unwrap();
    // The page disappears behind the store's back, as in a hand-edited file.
    let path = fixture.0.join("layers.conf");
    let text = fs::read_to_string(&path).unwrap();
    let layer_row = format!("layer\t{}\n", hex("Other"));
    assert!(text.contains(&layer_row), "{text}");
    fs::write(&path, text.replace(&layer_row, "")).unwrap();

    let loaded = LayerStore::open_at(&fixture.0).unwrap();

    assert!(
        loaded.diagnostics().is_empty(),
        "{:?}",
        loaded.diagnostics()
    );
    assert_eq!(
        loaded.binding("Base", &Trigger::KeyPress(1)),
        Some(multi(vec![step(shortcut()), wait(100)]))
    );
    assert_eq!(loaded.binding("Base", &Trigger::KeyPress(2)), None);
    let conflicts: Vec<_> = loaded
        .conflicts()
        .iter()
        .map(|conflict| {
            (
                conflict.layer.as_str(),
                conflict.control.as_str(),
                conflict.reason.as_str(),
                conflict.kept,
            )
        })
        .collect();
    assert_eq!(
        conflicts,
        [
            (
                "Base",
                "k:1",
                "multi action step references a missing target layer",
                false
            ),
            (
                "Base",
                "k:2",
                "binding references a missing target layer",
                false
            ),
        ]
    );
    let log = fs::read_to_string(fixture.0.join(crate::layers::CONFLICT_LOG)).unwrap();
    assert!(
        log.contains("reason=multi action step references a missing target layer"),
        "{log}"
    );
    // The cleaned binding was written back: the next start reports nothing.
    let rewritten = LayerStore::open_at(&fixture.0).unwrap();
    assert!(rewritten.conflicts().is_empty());
    assert_eq!(
        rewritten.binding("Base", &Trigger::KeyPress(1)),
        Some(multi(vec![step(shortcut()), wait(100)]))
    );
}

#[test]
fn a_stored_multi_step_to_the_legacy_default_page_loads_as_base() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    let with_target = |page: &str| multi(vec![step(Action::Emit("go".into())), go_to(page)]);
    store
        .bind("Base", Trigger::KeyPress(1), with_target("Base"))
        .unwrap();
    store.persist().unwrap();
    let path = fixture.0.join("layers.conf");
    let text = fs::read_to_string(&path).unwrap();
    let current = crate::layers::encode_action(&with_target("Base")).unwrap();
    let legacy = crate::layers::encode_action(&with_target("default")).unwrap();
    assert!(text.contains(&current), "{text}");
    fs::write(&path, text.replace(&current, &legacy)).unwrap();

    let loaded = LayerStore::open_at(&fixture.0).unwrap();

    assert!(loaded.conflicts().is_empty(), "{:?}", loaded.conflicts());
    assert_eq!(
        loaded.binding("Base", &Trigger::KeyPress(1)),
        Some(with_target("Base"))
    );
}

#[test]
fn asset_links_follow_the_sound_and_file_steps_of_a_multi_action() {
    let fixture = Fixture::new();
    let root = fixture.0.join("data");
    fs::create_dir_all(&root).unwrap();
    let audio = fixture.0.join("beep.wav");
    fs::write(&audio, b"audio").unwrap();
    let notes = fixture.0.join("notes.txt");
    fs::write(&notes, b"notes").unwrap();
    let chosen = multi(vec![
        step(sound(&audio)),
        wait(100),
        step(open(notes.to_str().unwrap())),
        step(open("https://example.com/")),
        step(Action::Emit("done".into())),
    ]);

    let linked = link_action(&root, chosen.clone()).unwrap();

    let Action::Feature(FeatureAction::Multi { steps }) = &linked else {
        panic!("still a multi action: {linked:?}");
    };
    assert_eq!(steps.len(), 5);
    let MultiStep::Action(Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio {
        path: sound_link,
    }))) = &steps[0]
    else {
        panic!("a sound step: {:?}", steps[0]);
    };
    let MultiStep::Action(Action::Feature(FeatureAction::Desktop(DesktopAction::Open {
        target: file_link,
    }))) = &steps[2]
    else {
        panic!("an open step: {:?}", steps[2]);
    };
    for (link, original) in [
        (sound_link.as_path(), &audio),
        (Path::new(file_link), &notes),
    ] {
        assert!(link.starts_with(root.join("links")), "{link:?}");
        assert_eq!(&fs::read_link(link).unwrap(), original);
    }
    assert_eq!(steps[1], wait(100));
    assert_eq!(steps[3], step(open("https://example.com/")));
    assert_eq!(steps[4], step(Action::Emit("done".into())));
    assert_eq!(unlink_action(&root, &linked), chosen);

    assert!(!missing_target(&linked));
    fs::remove_file(&notes).unwrap();
    assert!(missing_target(&linked), "a dangling step link is missing");
    assert!(missing_target(&chosen));
    assert!(!missing_target(&multi(vec![
        step(open("https://example.com/")),
        step(Action::Emit("x".into())),
    ])));
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
