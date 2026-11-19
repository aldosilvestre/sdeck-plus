//! Multi action hardening (feature task T6): regression tests from the independent
//! verification of the stored model (L9). Malformed and hostile stored data is
//! refused without a panic, the 4096-byte cap holds at its boundary, and a Multi
//! in `touchscreen.json` is refused without touching the file.

use crate::{
    features::{FeatureAction, MultiStep},
    layers::{Action, LayerStore, Trigger, CONFLICT_LOG},
    local_artwork::ArtworkStore,
    organization::Organization,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// A fresh configuration folder under the system temp directory, removed on drop.
struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "streamdeck-multi-hardening-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).expect("remove test fixture");
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The stored `layers.conf` form of a "More actions" JSON payload.
fn feature(json: &str) -> String {
    format!("feature-v1:{}", hex(json.as_bytes()))
}

fn multi(steps: Vec<MultiStep>) -> Action {
    Action::Feature(FeatureAction::Multi { steps })
}

/// A Multi whose single step is a command of `length` bytes.
fn command_multi(length: usize) -> FeatureAction {
    FeatureAction::Multi {
        steps: vec![MultiStep::Action(Action::Command("x".repeat(length)))],
    }
}

#[test]
fn malformed_stored_multi_rows_are_removed_with_one_logged_conflict() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(2),
            multi(vec![MultiStep::Action(Action::Emit("a".into()))]),
        )
        .unwrap();
    store
        .bind("Base", Trigger::KeyPress(3), Action::Emit("keep".into()))
        .unwrap();
    store.persist().unwrap();
    let text = fs::read_to_string(fixture.0.join("layers.conf")).unwrap();
    let row = text
        .lines()
        .find(|line| line.contains("feature-v1:"))
        .expect("the multi row");
    let prefix = &row[..row.rfind("feature-v1:").unwrap()];
    let emit = format!("emit:{}", hex(b"a"));
    let nested = format!(r#"{{"Multi":{{"steps":[{{"Action":"{emit}"}}]}}}}"#);
    let shortcut = format!("shortcut:{}:{}", hex(b"CTRL"), hex(b"C"));
    let malformed = [
        r#"{"Multi":{"steps":[{"Wait":{"milliseconds":5}}]}}"#.to_owned(),
        r#"{"Multi":{"steps":[]}}"#.to_owned(),
        r#"{"Multi":{"steps":[{"Action":"none"}]}}"#.to_owned(),
        r#"{"Multi":{}}"#.to_owned(),
        r#"{"Multi":{"steps":"x"}}"#.to_owned(),
        format!(
            r#"{{"Multi":{{"steps":[{{"Action":"{}"}}]}}}}"#,
            feature(&nested)
        ),
        format!(
            r#"{{"Multi":{{"steps":[{{"Action":"{shortcut}"}},{{"Action":""}},{{"Action":"{emit}"}}]}}}}"#
        ),
    ];

    for payload in &malformed {
        let edited = Fixture::new();
        let replaced: String = text
            .lines()
            .map(|line| {
                if line.contains("feature-v1:") {
                    format!("{prefix}{}\n", feature(payload))
                } else {
                    format!("{line}\n")
                }
            })
            .collect();
        fs::write(edited.0.join("layers.conf"), replaced).unwrap();

        let loaded = LayerStore::open_at(&edited.0).unwrap();

        assert_eq!(
            loaded.binding("Base", &Trigger::KeyPress(2)),
            None,
            "{payload}"
        );
        assert_eq!(
            loaded.binding("Base", &Trigger::KeyPress(3)),
            Some(Action::Emit("keep".into())),
            "{payload}"
        );
        assert!(loaded.diagnostics().is_empty(), "{payload}");
        let conflicts: Vec<_> = loaded
            .conflicts()
            .iter()
            .map(|conflict| {
                (
                    conflict.layer.as_str(),
                    conflict.control.as_str(),
                    conflict.kept,
                )
            })
            .collect();
        assert_eq!(conflicts, [("Base", "k:2", false)], "{payload}");
        let log = fs::read_to_string(edited.0.join(CONFLICT_LOG)).unwrap();
        assert!(log.contains("control=k:2"), "{payload}: {log}");
    }

    // A well-formed Multi over the 4096-byte cap is refused when read back too.
    let commands: Vec<_> = (0..16)
        .map(|_| format!(r#"{{"Action":"command:{}"}}"#, hex(&[b'x'; 200])))
        .collect();
    let oversized = format!(r#"{{"Multi":{{"steps":[{}]}}}}"#, commands.join(","));
    assert!(oversized.len() > 4096);
    let error = FeatureAction::decode(&oversized).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(
        error.to_string(),
        "Action settings exceed the 4096-byte storage limit"
    );
}

#[test]
fn hostile_stored_multi_payloads_are_refused_without_a_panic() {
    // Each nesting level hex-encodes the one before, doubling its size.
    let mut nested = format!(
        r#"{{"Multi":{{"steps":[{{"Action":"emit:{}"}}]}}}}"#,
        hex(b"a")
    );
    assert!(FeatureAction::decode(&nested).is_ok(), "one level is valid");
    for depth in 1..=14 {
        nested = format!(
            r#"{{"Multi":{{"steps":[{{"Action":"{}"}}]}}}}"#,
            feature(&nested)
        );
        assert!(
            FeatureAction::decode(&nested).is_err(),
            "nesting depth {depth} ({} bytes)",
            nested.len()
        );
        if nested.len() > 1_000_000 {
            break;
        }
    }
    // Deep JSON brackets as the payload, inside a step and inside a nested step.
    for depth in [1_000, 100_000, 1_000_000] {
        let brackets = "[".repeat(depth);
        for payload in [
            brackets.clone(),
            format!(r#"{{"Multi":{brackets}"#),
            format!(r#"{{"Multi":{{"steps":[{{"Action":"{brackets}"}}]}}}}"#),
            format!(
                r#"{{"Multi":{{"steps":[{{"Action":"{}"}}]}}}}"#,
                feature(&brackets)
            ),
        ] {
            assert!(FeatureAction::decode(&payload).is_err(), "{depth} brackets");
        }
    }
    // One huge step that is no action at all.
    let huge = "a".repeat(1_000_000);
    assert!(FeatureAction::decode(&format!(
        r#"{{"Multi":{{"steps":[{{"Action":"{huge}"}}]}}}}"#
    ))
    .is_err());
}

#[test]
fn the_4096_byte_cap_holds_at_its_boundary_and_the_largest_multi_round_trips() {
    // The longest command that still fits, found through the public encoder.
    let longest = (1..2100)
        .take_while(|length| command_multi(*length).encode().is_ok())
        .last()
        .unwrap();
    let fitting = command_multi(longest).encode().unwrap();
    // The payload is 43 bytes plus two hex digits per command byte: odd, so 4095.
    assert_eq!(fitting.len(), 4095);
    let error = command_multi(longest + 1).encode().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(error.to_string(), "Multi action is too large to store");
    assert_eq!(
        FeatureAction::decode(&fitting).unwrap(),
        command_multi(longest)
    );
    // Reading back: 4095 and 4096 bytes are accepted, 4097 refused.
    for (length, accepted) in [(4095, true), (4096, true), (4097, false)] {
        let padded = format!("{fitting}{}", " ".repeat(length - fitting.len()));
        assert_eq!(padded.len(), length);
        assert_eq!(
            FeatureAction::decode(&padded).is_ok(),
            accepted,
            "{length} bytes"
        );
    }
    // The stored binding survives save and reload unchanged, without conflicts.
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(1),
            Action::Feature(command_multi(longest)),
        )
        .unwrap();
    store.persist().unwrap();
    let loaded = LayerStore::open_at(&fixture.0).unwrap();
    assert_eq!(
        loaded.binding("Base", &Trigger::KeyPress(1)),
        Some(Action::Feature(command_multi(longest)))
    );
    assert!(loaded.conflicts().is_empty(), "{:?}", loaded.conflicts());
    assert!(
        loaded.diagnostics().is_empty(),
        "{:?}",
        loaded.diagnostics()
    );
}

#[test]
fn a_multi_in_touchscreen_json_is_refused_and_the_file_is_left_untouched() {
    let action = feature(&format!(
        r#"{{"Multi":{{"steps":[{{"Action":"emit:{}"}}]}}}}"#,
        hex(b"a")
    ));
    for (trigger, reason) in [
        ("tap", "multi actions require a key press or key hold"),
        ("ztap:1", "multi actions require a key press or key hold"),
        ("b:1", "multi actions require a key press or key hold"),
        ("k:2", "invalid or duplicate touchscreen binding"),
    ] {
        let fixture = Fixture::new();
        let mut layers = LayerStore::open_at(&fixture.0).unwrap();
        let organization = Organization::open_at(&fixture.0, &layers).unwrap();
        let mut artwork =
            ArtworkStore::open_at_for_layer(&fixture.0, Some(layers.active())).unwrap();
        layers
            .initialize_touchscreens(&organization, &mut artwork)
            .unwrap();
        let document = serde_json::json!({
            "version": 2,
            "profiles": [{
                "name": "Base",
                "selected": 0,
                "pages": [{
                    "owner": "@touchscreen:1",
                    "bindings": [{"trigger": trigger, "action": action}]
                }]
            }]
        });
        let path = fixture.0.join("touchscreen.json");
        let before = serde_json::to_vec(&document).unwrap();
        fs::write(&path, &before).unwrap();

        let mut layers = LayerStore::open_at(&fixture.0).unwrap();
        let organization = Organization::open_at(&fixture.0, &layers).unwrap();
        let mut artwork =
            ArtworkStore::open_at_for_layer(&fixture.0, Some(layers.active())).unwrap();

        assert_eq!(
            layers.diagnostics(),
            [format!("Invalid touchscreen configuration: {reason}")],
            "{trigger}"
        );
        let error = layers
            .initialize_touchscreens(&organization, &mut artwork)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "valid layers and artwork are required for touchscreen migration",
            "{trigger}"
        );
        assert!(layers.persist().is_err(), "{trigger}: saving is disabled");
        assert_eq!(fs::read(&path).unwrap(), before, "{trigger}");
    }
}
