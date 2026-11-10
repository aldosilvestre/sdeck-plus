//! Per-model data roots: the Stream Deck + keeps its directory byte for byte,
//! every other model owns `devices/<id>/`, and the two never share a file.

use sdeck_plus::{
    asset_links, config_bundle,
    desktop_actions::DesktopAction,
    device_model::{DeviceModel, PLUS, PLUS_XL},
    device_roots::{self, data_root_in, ensure_root_fits, owner_of_root},
    features::FeatureAction,
    info,
    layers::{Action, LayerStore, Trigger},
    local_artwork::ArtworkStore,
    obs::ObsConfig,
    organization::Organization,
};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sdeck-device-roots-{}-{name}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn plus(&self) -> PathBuf {
        self.0.join("sdeck-plus")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Tests may leave a read-only folder behind.
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(self.plus(), fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Relative path -> contents (`None` for a directory or a link) of a tree.
fn tree(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(base).unwrap().display().to_string();
            let kind = fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                out.insert(name, None);
                walk(base, &path, out);
            } else if kind.is_symlink() {
                out.insert(name, None);
            } else {
                out.insert(name, Some(fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn without_devices(tree: &BTreeMap<String, Option<Vec<u8>>>) -> BTreeMap<String, Option<Vec<u8>>> {
    tree.iter()
        .filter(|(name, _)| name.as_str() != "devices" && !name.starts_with("devices/"))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

fn command() -> Action {
    Action::Command("printf hello".into())
}

/// Every store, driven through the model-less wrappers the Stream Deck + used.
fn use_plus_with_legacy_apis(root: &Path) {
    let mut layers = LayerStore::open_at(root).unwrap();
    layers.create("Work").unwrap();
    layers
        .bind("Base", Trigger::KeyPress(8), command())
        .unwrap();
    layers.bind("Work", Trigger::ZoneTap(4), command()).unwrap();
    layers.persist().unwrap();
    let organization = Organization::open_at(root, &layers).unwrap();
    organization.persist().unwrap();
    ArtworkStore::open_at_for_layer(root, Some("Base")).unwrap();
    ObsConfig::default().save_at(root).unwrap();
    info::save_location(root, "Madrid").unwrap();
}

/// The same work through the model-aware APIs for `model`.
fn use_with_model_apis(root: &Path, model: &'static DeviceModel) -> LayerStore {
    let mut layers = LayerStore::open_at_model(root, model).unwrap();
    layers.create("Work").unwrap();
    layers
        .bind("Base", Trigger::KeyPress(8), command())
        .unwrap();
    layers.bind("Work", Trigger::ZoneTap(4), command()).unwrap();
    layers.persist().unwrap();
    let organization = Organization::open_at(root, &layers).unwrap();
    organization.persist().unwrap();
    ArtworkStore::open_at_for_layer_model(root, Some("Base"), model).unwrap();
    ObsConfig::default().save_at(root).unwrap();
    info::save_location(root, "Madrid").unwrap();
    layers
}

#[test]
fn roots_are_the_plus_directory_and_a_devices_folder_per_other_model() {
    let base = Path::new("/data/sdeck-plus");
    assert_eq!(data_root_in(base, &PLUS), base);
    assert_eq!(
        data_root_in(base, &PLUS_XL),
        Path::new("/data/sdeck-plus/devices/plus-xl")
    );
}

#[test]
fn model_ids_are_single_path_components() {
    for model in DeviceModel::all() {
        assert!(!model.id.is_empty());
        assert!(!model.id.contains(['/', '\\', '.']), "{}", model.id);
    }
}

#[test]
fn the_plus_tree_is_identical_through_legacy_and_model_apis_and_gets_no_devices_folder() {
    let legacy = Fixture::new("legacy");
    let modelled = Fixture::new("modelled");
    let (a, b) = (legacy.plus(), modelled.plus());
    use_plus_with_legacy_apis(&a);
    use_with_model_apis(&data_root_in(&b, &PLUS), &PLUS);
    let (before, after) = (tree(&a), tree(&b));
    assert_eq!(before, after);
    assert!(!a.join("devices").exists() && !b.join("devices").exists());
    assert!(before.contains_key("layers.conf"));
    assert!(before.contains_key("obs.json") && before.contains_key("info.json"));
}

#[test]
fn reopening_the_plus_root_never_rewrites_its_files() {
    let f = Fixture::new("reopen");
    use_plus_with_legacy_apis(&f.plus());
    let before = tree(&f.plus());
    let layers = LayerStore::open_at_model(f.plus(), &PLUS).unwrap();
    Organization::open_at(f.plus(), &layers).unwrap();
    ArtworkStore::open_at_for_layer_model(f.plus(), Some("Base"), &PLUS).unwrap();
    ObsConfig::load_at(&f.plus()).unwrap();
    assert_eq!(tree(&f.plus()), before);
}

#[test]
fn the_xl_root_is_created_only_when_its_stores_open_and_stays_independent() {
    let f = Fixture::new("independent");
    let plus = f.plus();
    use_plus_with_legacy_apis(&plus);
    let plus_before = tree(&plus);
    let xl = data_root_in(&plus, &PLUS_XL);
    assert!(!xl.exists());
    let mut layers = use_with_model_apis(&xl, &PLUS_XL);
    layers
        .bind("Base", Trigger::KeyPress(36), command())
        .unwrap();
    layers.persist().unwrap();
    assert!(xl.join("layers.conf").is_file());
    // The Stream Deck + files are the same bytes; only `devices/` is new.
    assert_eq!(without_devices(&tree(&plus)), plus_before);
    // No file of one model is the same file as one of the other.
    let xl_files: Vec<_> = tree(&xl).into_keys().collect();
    assert!(xl_files.iter().any(|name| name == "layers.conf"));
    for name in &xl_files {
        let (theirs, ours) = (xl.join(name), plus.join(name));
        assert!(!theirs.is_symlink());
        if let (Ok(a), Ok(b)) = (fs::metadata(&theirs), fs::metadata(&ours)) {
            assert!(a.ino() != b.ino() || a.dev() != b.dev(), "{name} is shared");
        }
    }
    // Under the Stream Deck + root the only new entries are the XL folders.
    let added: Vec<_> = tree(&plus)
        .into_keys()
        .filter(|name| !plus_before.contains_key(name))
        .collect();
    assert!(added
        .iter()
        .all(|name| name == "devices" || name.starts_with("devices/plus-xl")));
}

#[test]
fn the_xl_root_round_trips_key_36_dial_6_and_zone_6_through_the_model_apis() {
    let f = Fixture::new("xl-roundtrip");
    let xl = data_root_in(&f.plus(), &PLUS_XL);
    let mut layers = LayerStore::open_at_model(&xl, &PLUS_XL).unwrap();
    for trigger in [
        Trigger::KeyPress(36),
        Trigger::EncoderPress(6),
        Trigger::Rotation(6, 1),
        Trigger::ZoneTap(6),
    ] {
        layers.bind("Base", trigger, command()).unwrap();
    }
    layers.persist().unwrap();
    let written = tree(&xl);
    let reopened = LayerStore::open_at_model(&xl, &PLUS_XL).unwrap();
    assert!(reopened.diagnostics().is_empty());
    let bindings = &reopened.layers()["Base"].bindings;
    for trigger in [
        Trigger::KeyPress(36),
        Trigger::EncoderPress(6),
        Trigger::Rotation(6, 1),
        Trigger::ZoneTap(6),
    ] {
        assert!(bindings.contains_key(&trigger), "{trigger:?} was lost");
    }
    assert_eq!(tree(&xl), written);
}

#[test]
fn an_xl_root_cannot_be_opened_as_another_model_and_keeps_its_bytes() {
    let f = Fixture::new("guard");
    let xl = data_root_in(&f.plus(), &PLUS_XL);
    let mut layers = LayerStore::open_at_model(&xl, &PLUS_XL).unwrap();
    layers
        .bind("Base", Trigger::KeyPress(36), command())
        .unwrap();
    layers.persist().unwrap();
    let before = tree(&xl);
    assert_eq!(owner_of_root(&xl).map(|model| model.id), Some("plus-xl"));
    assert!(LayerStore::open_at(&xl).is_err());
    assert!(LayerStore::open_at_model(&xl, &PLUS).is_err());
    assert!(ArtworkStore::open_at(&xl).is_err());
    assert!(ArtworkStore::open_at_for_layer(&xl, None).is_err());
    assert!(ensure_root_fits(&xl, &PLUS).is_err());
    assert!(ensure_root_fits(&xl, &PLUS_XL).is_ok());
    assert!(ensure_root_fits(&f.plus(), &PLUS).is_ok());
    assert_eq!(tree(&xl), before);
}

#[test]
fn obs_info_links_and_the_errors_log_follow_the_root_they_are_read_from() {
    let f = Fixture::new("per-root");
    let plus = f.plus();
    let xl = data_root_in(&plus, &PLUS_XL);
    let plus_layers = use_with_model_apis(&plus, &PLUS);
    let xl_layers = use_with_model_apis(&xl, &PLUS_XL);
    let config = |endpoint: &str| ObsConfig {
        endpoint: endpoint.into(),
        password: String::new(),
    };
    config("ws://127.0.0.1:4456").save_at(&plus).unwrap();
    config("ws://127.0.0.1:4457").save_at(&xl).unwrap();
    assert_eq!(
        ObsConfig::load_at(plus_layers.root()).unwrap().endpoint,
        "ws://127.0.0.1:4456"
    );
    assert_eq!(
        ObsConfig::load_at(xl_layers.root()).unwrap().endpoint,
        "ws://127.0.0.1:4457"
    );
    info::save_location(&xl, "Lima").unwrap();
    assert_eq!(info::load_location(&plus).as_deref(), Some("Madrid"));
    assert_eq!(info::load_location(&xl).as_deref(), Some("Lima"));
    let sound = f.0.join("song.mp3");
    fs::write(&sound, b"audio").unwrap();
    let play = Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio {
        path: sound,
    }));
    asset_links::link_action(&xl, play).unwrap();
    assert!(xl.join("links").is_dir());
    assert!(!plus.join("links").exists());
}

#[test]
fn an_unwritable_parent_fails_the_xl_open_cleanly_and_leaves_the_plus_alone() {
    let f = Fixture::new("read-only");
    let plus = f.plus();
    use_plus_with_legacy_apis(&plus);
    let before = tree(&plus);
    fs::set_permissions(&plus, fs::Permissions::from_mode(0o500)).unwrap();
    let xl = data_root_in(&plus, &PLUS_XL);
    let opened = LayerStore::open_at_model(&xl, &PLUS_XL);
    let artwork = ArtworkStore::open_at_model(&xl, &PLUS_XL);
    let enforced = fs::create_dir(plus.join(".probe")).is_err();
    fs::set_permissions(&plus, fs::Permissions::from_mode(0o700)).unwrap();
    if !enforced {
        // Running with privileges that ignore permissions: nothing to assert.
        let _ = fs::remove_dir_all(plus.join("devices"));
        let _ = fs::remove_dir(plus.join(".probe"));
        return;
    }
    assert!(opened.is_err() && artwork.is_err());
    assert_eq!(tree(&plus), before);
}

#[test]
fn a_malformed_xl_store_keeps_its_bytes_and_refuses_to_save_defaults() {
    let f = Fixture::new("malformed");
    let xl = data_root_in(&f.plus(), &PLUS_XL);
    fs::create_dir_all(&xl).unwrap();
    fs::write(xl.join("layers.conf"), b"not a layers file\n").unwrap();
    let before = tree(&xl);
    let layers = LayerStore::open_at_model(&xl, &PLUS_XL).unwrap();
    // The session refuses a store with diagnostics (`checked_layer_store`);
    // opening it must not have replaced the original with defaults.
    assert!(!layers.diagnostics().is_empty());
    Organization::open_at(&xl, &layers).unwrap();
    ArtworkStore::open_at_for_layer_model(&xl, None, &PLUS_XL).unwrap();
    assert_eq!(tree(&xl)["layers.conf"], before["layers.conf"]);
}

fn export_for_model(root: &Path, destination: &Path, model: &'static DeviceModel) {
    config_bundle::export_for(root, destination, model).unwrap();
}

#[test]
fn bundles_record_their_model_in_format_five_and_round_trip_per_model() {
    let f = Fixture::new("bundle-roundtrip");
    let plus = f.plus();
    let xl = data_root_in(&plus, &PLUS_XL);
    use_with_model_apis(&plus, &PLUS);
    let mut xl_layers = use_with_model_apis(&xl, &PLUS_XL);
    xl_layers
        .bind("Base", Trigger::KeyPress(36), command())
        .unwrap();
    xl_layers.persist().unwrap();
    for (root, model) in [(&plus, &PLUS), (&xl, &PLUS_XL)] {
        let bundle = f.0.join(format!("{}.json", model.id));
        export_for_model(root, &bundle, model);
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&bundle).unwrap()).unwrap();
        assert_eq!(value["version"], 5);
        assert_eq!(value["model"], model.id);
        let target = f.0.join(format!("target-{}", model.id)).join("nested");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        config_bundle::import_for(&bundle, &target, model).unwrap();
        let restored = LayerStore::open_at_model(&target, model).unwrap();
        assert!(restored.diagnostics().is_empty());
        assert_eq!(
            restored.layers()["Base"]
                .bindings
                .contains_key(&Trigger::KeyPress(36)),
            model.id == "plus-xl"
        );
    }
}

#[test]
fn a_bundle_for_another_model_is_refused_before_anything_is_staged() {
    let f = Fixture::new("mismatch");
    let plus = f.plus();
    let xl = data_root_in(&plus, &PLUS_XL);
    use_with_model_apis(&plus, &PLUS);
    use_with_model_apis(&xl, &PLUS_XL);
    let xl_bundle = f.0.join("xl.json");
    let plus_bundle = f.0.join("plus.json");
    export_for_model(&xl, &xl_bundle, &PLUS_XL);
    export_for_model(&plus, &plus_bundle, &PLUS);
    let before = tree(&f.0);
    let error = config_bundle::import_for(&xl_bundle, &plus, &PLUS).unwrap_err();
    assert_eq!(
        error.to_string(),
        "This configuration is for Stream Deck + XL; connect or preview that model to import it."
    );
    let error = config_bundle::import_for(&plus_bundle, &xl, &PLUS_XL).unwrap_err();
    assert_eq!(
        error.to_string(),
        "This configuration is for Stream Deck +; connect or preview that model to import it."
    );
    assert_eq!(
        tree(&f.0),
        before,
        "a refusal must not stage or change anything"
    );
}

#[test]
fn bundles_before_format_five_are_plus_bundles() {
    let f = Fixture::new("legacy-bundle");
    let plus = f.plus();
    use_with_model_apis(&plus, &PLUS);
    let bundle = f.0.join("bundle.json");
    export_for_model(&plus, &bundle, &PLUS);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&bundle).unwrap()).unwrap();
    value["version"] = 4.into();
    value.as_object_mut().unwrap().remove("model");
    fs::write(&bundle, serde_json::to_vec(&value).unwrap()).unwrap();
    let target = f.0.join("imported");
    config_bundle::import(&bundle, &target).unwrap();
    assert!(LayerStore::open_at(&target)
        .unwrap()
        .diagnostics()
        .is_empty());
    let xl = data_root_in(&f.0.join("other"), &PLUS_XL);
    let error = config_bundle::import_for(&bundle, &xl, &PLUS_XL).unwrap_err();
    assert!(error
        .to_string()
        .starts_with("This configuration is for Stream Deck +;"));
}

#[test]
fn malformed_model_fields_are_rejected() {
    let f = Fixture::new("bad-model");
    let plus = f.plus();
    use_with_model_apis(&plus, &PLUS);
    let bundle = f.0.join("bundle.json");
    export_for_model(&plus, &bundle, &PLUS);
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&bundle).unwrap()).unwrap();
    let edits: [fn(&mut serde_json::Value); 3] = [
        |v| v["model"] = "plus-xxl".into(),
        |v| {
            v.as_object_mut().unwrap().remove("model");
        },
        |v| v["version"] = 4.into(), // model present before format 5
    ];
    for edit in edits {
        let mut value = original.clone();
        edit(&mut value);
        fs::write(&bundle, serde_json::to_vec(&value).unwrap()).unwrap();
        let target = f.0.join("target");
        assert!(config_bundle::import(&bundle, &target).is_err());
        assert!(!target.exists());
    }
}

#[test]
fn readers_of_earlier_formats_reject_format_five() {
    // The envelope an earlier build parsed: strict, without a `model` field.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct EarlierBundle {
        format: String,
        version: u32,
        files: Vec<serde_json::Value>,
    }
    let f = Fixture::new("older-reader");
    let plus = f.plus();
    use_with_model_apis(&plus, &PLUS);
    let bundle = f.0.join("bundle.json");
    export_for_model(&plus, &bundle, &PLUS);
    assert!(serde_json::from_slice::<EarlierBundle>(&fs::read(&bundle).unwrap()).is_err());
}

#[test]
fn importing_into_the_plus_root_carries_the_devices_folder_across_intact() {
    let f = Fixture::new("carry");
    let plus = f.plus();
    let xl = data_root_in(&plus, &PLUS_XL);
    use_with_model_apis(&plus, &PLUS);
    let mut xl_layers = use_with_model_apis(&xl, &PLUS_XL);
    xl_layers
        .bind("Base", Trigger::KeyPress(36), command())
        .unwrap();
    xl_layers.persist().unwrap();
    fs::write(xl.join("extra.bin"), [0u8, 1, 2, 3]).unwrap();
    let bundle = f.0.join("bundle.json");
    export_for_model(&plus, &bundle, &PLUS);
    let xl_before = tree(&xl);
    let backup = config_bundle::import_for(&bundle, &plus, &PLUS).unwrap();
    assert_eq!(tree(&xl), xl_before);
    assert!(plus.join("devices").is_dir());
    // The backup is the old Stream Deck + root without the other models.
    let backup = backup.expect("an existing root leaves a backup");
    assert!(!backup.join("devices").exists());
    // The restored XL root still opens with its key 36 binding.
    let reopened = LayerStore::open_at_model(&xl, &PLUS_XL).unwrap();
    assert!(reopened.layers()["Base"]
        .bindings
        .contains_key(&Trigger::KeyPress(36)));
}

#[test]
fn importing_into_the_xl_root_touches_only_that_root() {
    let f = Fixture::new("xl-import");
    let plus = f.plus();
    let xl = data_root_in(&plus, &PLUS_XL);
    use_with_model_apis(&plus, &PLUS);
    use_with_model_apis(&xl, &PLUS_XL);
    let bundle = f.0.join("bundle.json");
    export_for_model(&xl, &bundle, &PLUS_XL);
    let plus_before = without_devices(&tree(&plus));
    config_bundle::import_for(&bundle, &xl, &PLUS_XL).unwrap();
    assert_eq!(without_devices(&tree(&plus)), plus_before);
    assert!(xl.join("layers.conf").is_file());
}

#[test]
fn a_first_xl_import_creates_only_the_devices_folder_it_needs() {
    let f = Fixture::new("xl-first-import");
    let source = f.0.join("source");
    use_with_model_apis(&source, &PLUS);
    let xl_source = data_root_in(&f.0.join("xl-source"), &PLUS_XL);
    use_with_model_apis(&xl_source, &PLUS_XL);
    let bundle = f.0.join("bundle.json");
    export_for_model(&xl_source, &bundle, &PLUS_XL);
    let plus = f.plus();
    let xl = data_root_in(&plus, &PLUS_XL);
    fs::create_dir(&plus).unwrap();
    let before = tree(&plus);
    assert!(config_bundle::import_for(&bundle, &xl, &PLUS_XL)
        .unwrap()
        .is_none());
    assert!(xl.join("layers.conf").is_file());
    assert_eq!(without_devices(&tree(&plus)), before);
}

#[test]
fn the_data_directory_helper_is_the_plus_root() {
    // Pure: the same value every store opener used before this module existed.
    if let Ok(dir) = device_roots::data_dir() {
        assert_eq!(dir.file_name().unwrap(), "sdeck-plus");
        assert_eq!(data_root_in(&dir, &PLUS), dir);
    }
}
