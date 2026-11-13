//! Public-API tests for plugin actions stored as bindings. No plugin is ever installed, so
//! they prove a saved binding is plain data that survives a missing plugin and never runs one.

use sdeck_plus::{
    config_bundle,
    desktop_actions::DesktopAction,
    features::FeatureAction,
    layers::{Action, LayerStore, Trigger},
    plugins::{PluginBinding, Settings},
};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sdeck-bindings-{}-{n}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn settings(value: Value) -> Settings {
    value.as_object().cloned().unwrap()
}

fn binding(settings: Settings) -> Action {
    Action::Feature(FeatureAction::Plugin(PluginBinding::new(
        "ghost", "wave", settings,
    )))
}

fn conf(root: &Path) -> Vec<u8> {
    fs::read(root.join("layers.conf")).unwrap()
}

#[test]
fn unavailable_plugin_binding_survives_save_load_and_bundle_roundtrip() {
    let root = Root::new();
    let action = binding(settings(
        json!({"name": "日本語 hello", "level": 7, "loud": true}),
    ));
    let mut store = LayerStore::open_at(root.path()).unwrap();
    store
        .bind("Base", Trigger::KeyPress(2), action.clone())
        .unwrap();
    store.persist().unwrap();
    let loaded = LayerStore::open_at(root.path()).unwrap();
    assert!(loaded.diagnostics().is_empty());
    assert_eq!(
        loaded.binding("Base", &Trigger::KeyPress(2)),
        Some(action.clone())
    );

    let outside = Root::new();
    let bundle = outside.path().join("out.sdeck-plus.json");
    config_bundle::export(root.path(), &bundle).unwrap();
    let destination = Root::new();
    config_bundle::import(&bundle, destination.path()).unwrap();
    let imported = LayerStore::open_at(destination.path()).unwrap();
    assert_eq!(
        imported.binding("Base", &Trigger::KeyPress(2)),
        Some(action)
    );
}

#[test]
fn plugin_binding_summary_names_plugin_and_action() {
    let action = FeatureAction::Plugin(PluginBinding::new("ghost", "wave", Settings::new()));
    assert_eq!(action.summary(), "Plugin ghost: wave");
}

#[test]
fn invalid_plugin_bindings_are_rejected_without_store_mutation() {
    let root = Root::new();
    let mut store = LayerStore::open_at(root.path()).unwrap();
    store
        .bind("Base", Trigger::KeyPress(1), Action::Emit("keep".into()))
        .unwrap();
    store.persist().unwrap();
    let before = conf(root.path());
    let bad: Vec<(&str, &str, Settings)> = vec![
        ("Bad Id", "wave", Settings::new()),
        ("", "wave", Settings::new()),
        ("ghost", "", Settings::new()),
        ("ghost", "Wave!", Settings::new()),
        ("ghost", "wave", settings(json!({"Bad Key": 1}))),
        ("ghost", "wave", settings(json!({"nested": {"a": 1}}))),
        ("ghost", "wave", settings(json!({"list": [1]}))),
        ("ghost", "wave", settings(json!({"ratio": 1.5}))),
        ("ghost", "wave", settings(json!({"none": null}))),
        ("ghost", "wave", settings(json!({"text": "x".repeat(1025)}))),
    ];
    for (plugin, action_id, values) in bad {
        let action = Action::Feature(FeatureAction::Plugin(PluginBinding::new(
            plugin, action_id, values,
        )));
        assert!(
            store.bind("Base", Trigger::KeyPress(1), action).is_err(),
            "{plugin}/{action_id}"
        );
    }
    // Five 1024-character fields stay under the per-field cap but exceed the 4096-byte limit.
    let big = settings(json!({
        "a": "x".repeat(1024), "b": "x".repeat(1024), "c": "x".repeat(1024),
        "d": "x".repeat(1024), "e": "x".repeat(1024),
    }));
    assert!(store
        .bind("Base", Trigger::KeyPress(1), binding(big))
        .is_err());
    assert_eq!(
        store.binding("Base", &Trigger::KeyPress(1)),
        Some(Action::Emit("keep".into()))
    );
    store.persist().unwrap();
    assert_eq!(conf(root.path()), before);
}

#[test]
fn stored_plugin_json_over_the_limit_or_with_unknown_fields_is_rejected_on_decode() {
    let padded = format!(
        r#"{{"Plugin":{{"plugin_id":"ghost","action_id":"wave","settings":{{"a":"{}"}}}}}}"#,
        "x".repeat(4096)
    );
    assert!(FeatureAction::decode(&padded).is_err());
    let extra =
        r#"{"Plugin":{"plugin_id":"ghost","action_id":"wave","settings":{},"exec":"/bin/sh"}}"#;
    assert!(FeatureAction::decode(extra).is_err());
    assert!(FeatureAction::decode(r#"{"Plugin":{"plugin_id":"ghost"}}"#).is_err());
    assert!(FeatureAction::decode(r#"{"PluginTwo":{}}"#).is_err());
    let ok = r#"{"Plugin":{"plugin_id":"ghost","action_id":"wave"}}"#;
    assert_eq!(
        FeatureAction::decode(ok).unwrap(),
        FeatureAction::Plugin(PluginBinding::new("ghost", "wave", Settings::new()))
    );
}

#[test]
fn the_largest_valid_plugin_binding_roundtrips_through_encode_decode_and_layers_conf() {
    let mut values = Settings::new();
    for (key, length) in [("a", 1024), ("b", 1024), ("c", 1024), ("d", 800)] {
        values.insert(key.into(), Value::String("é".repeat(length / 2)));
    }
    let action = FeatureAction::Plugin(PluginBinding::new("ghost", "wave", values));
    let encoded = action.encode().unwrap();
    assert!(encoded.len() <= 4096, "{}", encoded.len());
    assert_eq!(FeatureAction::decode(&encoded).unwrap(), action);
    let root = Root::new();
    let mut store = LayerStore::open_at(root.path()).unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(3),
            Action::Feature(action.clone()),
        )
        .unwrap();
    store.persist().unwrap();
    assert_eq!(
        LayerStore::open_at(root.path())
            .unwrap()
            .binding("Base", &Trigger::KeyPress(3)),
        Some(Action::Feature(action))
    );
}

#[test]
fn built_in_actions_still_roundtrip_through_the_shared_feature_parser() {
    let action = FeatureAction::Desktop(DesktopAction::Website {
        url: "https://example.com".into(),
    });
    assert_eq!(
        FeatureAction::decode(&action.encode().unwrap()).unwrap(),
        action
    );
    assert!(FeatureAction::decode(r#"{"Unknown":{}}"#).is_err());
}

#[test]
fn saving_and_exporting_a_plugin_binding_never_runs_an_installed_plugin() {
    let root = Root::new();
    let plugins = root.path().join("plugins");
    let dir = plugins.join("ghost");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("manifest.json"),
        json!({
            "id": "ghost", "version": "1", "protocol_version": 1, "name": "Ghost",
            "executable": "run",
            "actions": [{"id": "wave", "name": "Wave", "controls": ["key"], "settings": []}]
        })
        .to_string(),
    )
    .unwrap();
    let run = dir.join("run");
    fs::write(&run, "#!/bin/sh\ntouch ran.marker\n").unwrap();
    fs::set_permissions(&run, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        plugins.join("plugins.json"),
        r#"{"version":1,"enabled":["ghost"]}"#,
    )
    .unwrap();

    let data = root.path().join("data");
    let mut store = LayerStore::open_at(&data).unwrap();
    store
        .bind("Base", Trigger::KeyPress(1), binding(Settings::new()))
        .unwrap();
    store.persist().unwrap();
    config_bundle::export(&data, &root.path().join("b.sdeck-plus.json")).unwrap();
    assert!(!dir.join("ran.marker").exists());
}
