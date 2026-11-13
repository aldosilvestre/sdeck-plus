//! Rows of a kind this build does not know are kept, never deleted.
use super::{hex, Action, LayerStore, Trigger, CONFLICT_LOG};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sdeck-unknown-kind-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) {
        fs::write(self.0.join(name), text).unwrap();
    }
    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.0.join(name)).unwrap()
    }
    /// The log lines without their timestamps.
    fn log(&self) -> Vec<String> {
        self.read(CONFLICT_LOG)
            .lines()
            .map(|line| line.split_once('\t').unwrap().1.to_owned())
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const BASE: &str = "42617365";
const OTHER: &str = "4f74686572";
const HEADER: &str = "streamdeck-layers-v1\nactive\t42617365\n";

fn plugin_action() -> String {
    format!(
        "feature-v1:{}",
        hex(br#"{"Plugin":{"plugin_id":"x","action_id":"y","settings":{}}}"#)
    )
}

fn plugin_row(layer: &str, key: u8) -> String {
    format!("binding\t{layer}\tk:{key}\t{}", plugin_action())
}

/// Known rows and unknown rows in the order the store writes them.
fn canonical() -> String {
    format!(
        "{HEADER}layer\t{BASE}\nbinding\t{BASE}\tk:1\tcommand:72756e\n\
         info\t{BASE}\tk:1\tfuture-source\n{}\nbinding\t{BASE}\tk:6\tplugin-v1:7a\n\
         layer\t{OTHER}\nbinding\t{OTHER}\tk:2\temit:61\n{}\n",
        plugin_row(BASE, 5),
        plugin_row(OTHER, 5)
    )
}

#[test]
fn unknown_rows_survive_load_and_every_save_byte_identically() {
    let fixture = Fixture::new();
    fixture.write("layers.conf", &canonical());

    let mut store = LayerStore::open_at(&fixture.0).unwrap();

    assert!(store.diagnostics().is_empty());
    assert_eq!(store.layers()["Base"].bindings.len(), 1);
    assert_eq!(store.layers()["Other"].bindings.len(), 1);
    assert_eq!(store.conflicts().len(), 4);
    assert!(store.conflicts().iter().all(|conflict| conflict.kept));
    assert_eq!(fixture.read("layers.conf"), canonical());
    store.persist().unwrap();
    assert_eq!(fixture.read("layers.conf"), canonical());
    // An unrelated edit still writes them back, grouped with their layer.
    store
        .bind("Base", Trigger::KeyPress(2), Action::Emit("b".into()))
        .unwrap();
    store.persist().unwrap();
    let expected = canonical().replacen(
        &format!("info\t{BASE}\tk:1"),
        &format!("binding\t{BASE}\tk:2\temit:62\ninfo\t{BASE}\tk:1"),
        1,
    );
    assert_eq!(fixture.read("layers.conf"), expected);
    let reopened = LayerStore::open_at(&fixture.0).unwrap();
    assert_eq!(reopened.conflicts().len(), 4);
}

#[test]
fn unknown_rows_are_neither_shown_nor_executed() {
    let fixture = Fixture::new();
    fixture.write("layers.conf", &canonical());
    let store = LayerStore::open_at(&fixture.0).unwrap();
    assert!(!store.layers()["Base"]
        .bindings
        .contains_key(&Trigger::KeyPress(5)));
    assert_eq!(store.info_source("Base", &Trigger::KeyPress(1)), None);
}

#[test]
fn conflict_log_says_kept_for_unknown_rows_and_logs_them_once() {
    let fixture = Fixture::new();
    fixture.write("layers.conf", &canonical());
    LayerStore::open_at(&fixture.0).unwrap();
    let plugin = plugin_row(BASE, 5);
    let expected = vec![
        format!("ERROR\tprofile=\"Base\"\tcontrol=k:1\treason=unknown information source\tkept=info\t{BASE}\tk:1\tfuture-source"),
        format!("ERROR\tprofile=\"Base\"\tcontrol=k:5\treason=unknown feature variant 'Plugin'\tkept={plugin}"),
        format!("ERROR\tprofile=\"Base\"\tcontrol=k:6\treason=unknown action kind 'plugin-v1'\tkept=binding\t{BASE}\tk:6\tplugin-v1:7a"),
        format!("ERROR\tprofile=\"Other\"\tcontrol=k:5\treason=unknown feature variant 'Plugin'\tkept={}", plugin_row(OTHER, 5)),
    ];
    let mut actual = fixture.log();
    actual.sort();
    let mut sorted = expected.clone();
    sorted.sort();
    assert_eq!(actual, sorted);
    assert!(!fixture.read(CONFLICT_LOG).contains("removed="));
    LayerStore::open_at(&fixture.0).unwrap();
    assert_eq!(fixture.log().len(), 4);
}

#[test]
fn binding_or_clearing_the_same_control_drops_only_that_row() {
    let fixture = Fixture::new();
    fixture.write("layers.conf", &canonical());
    let mut store = LayerStore::open_at(&fixture.0).unwrap();

    store
        .bind("Base", Trigger::KeyPress(5), Action::Emit("new".into()))
        .unwrap();
    store.persist().unwrap();

    let text = fixture.read("layers.conf");
    assert!(!text.contains(&plugin_row(BASE, 5)));
    assert!(text.contains(&format!("binding\t{BASE}\tk:5\temit:6e6577\n")));
    assert!(text.contains(&plugin_row(OTHER, 5)));
    assert!(text.contains("plugin-v1:7a") && text.contains("future-source"));

    store.unbind("Base", &Trigger::KeyPress(6)).unwrap();
    store
        .set_info(
            "Base",
            &Trigger::KeyPress(1),
            Some(crate::info::InfoSource::Clock),
        )
        .unwrap();
    store.persist().unwrap();
    let text = fixture.read("layers.conf");
    assert!(!text.contains("plugin-v1:7a") && !text.contains("future-source"));
    assert!(text.contains(&format!(
        "info\t{BASE}\tk:1\t{}\n",
        crate::info::InfoSource::Clock.id()
    )));
    assert!(text.contains(&plugin_row(OTHER, 5)));
}

#[test]
fn clearing_the_information_choice_drops_the_unknown_information_row() {
    let fixture = Fixture::new();
    fixture.write("layers.conf", &canonical());
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store.set_info("Base", &Trigger::KeyPress(1), None).unwrap();
    store.persist().unwrap();
    let expected = canonical().replace(&format!("info\t{BASE}\tk:1\tfuture-source\n"), "");
    assert_eq!(fixture.read("layers.conf"), expected);
}

#[test]
fn deleting_the_layer_drops_its_unknown_rows_only() {
    let fixture = Fixture::new();
    fixture.write("layers.conf", &canonical());
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    assert!(store.remove("Other").unwrap());
    store.persist().unwrap();
    let expected = canonical()
        .replace(&format!("layer\t{OTHER}\n"), "")
        .replace(&format!("binding\t{OTHER}\tk:2\temit:61\n"), "")
        .replace(&format!("{}\n", plugin_row(OTHER, 5)), "");
    assert_eq!(fixture.read("layers.conf"), expected);
}

#[test]
fn malformed_rows_of_a_known_kind_are_still_removed_and_logged() {
    let fixture = Fixture::new();
    let bad_variant = format!("feature-v1:{}", hex(br#"{"Timer":{"seconds":0}}"#));
    let two_keys = format!("feature-v1:{}", hex(br#"{"Plugin":{},"Timer":{}}"#));
    fixture.write(
        "layers.conf",
        &format!(
            "{HEADER}layer\t{BASE}\nbinding\t{BASE}\tk:1\tcommand:zz\n\
             binding\t{BASE}\tk:2\t{bad_variant}\nbinding\t{BASE}\tk:3\t{two_keys}\n\
             binding\t{BASE}\tk:99\t{}\nbinding\t{BASE}\tk:4\tnot-an-action\n{}\n",
            plugin_action(),
            plugin_row(BASE, 5)
        ),
    );

    let store = LayerStore::open_at(&fixture.0).unwrap();

    assert_eq!(store.conflicts().iter().filter(|c| !c.kept).count(), 5);
    assert_eq!(store.conflicts().iter().filter(|c| c.kept).count(), 1);
    let log = fixture.read(CONFLICT_LOG);
    assert_eq!(log.matches("removed=").count(), 5);
    assert_eq!(log.matches("kept=").count(), 1);
    assert_eq!(
        fixture.read("layers.conf"),
        format!("{HEADER}layer\t{BASE}\n{}\n", plugin_row(BASE, 5))
    );
}

#[test]
fn stores_without_unknown_rows_are_written_as_before() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store.create("Other").unwrap();
    store
        .bind("Base", Trigger::KeyPress(1), Action::Emit("a".into()))
        .unwrap();
    store
        .set_info(
            "Base",
            &Trigger::KeyPress(1),
            Some(crate::info::InfoSource::Clock),
        )
        .unwrap();
    store.persist().unwrap();
    let expected = format!(
        "{HEADER}layer\t{BASE}\nbinding\t{BASE}\tk:1\temit:61\ninfo\t{BASE}\tk:1\t{}\nlayer\t{OTHER}\n",
        crate::info::InfoSource::Clock.id()
    );
    assert_eq!(fixture.read("layers.conf"), expected);
    let reopened = LayerStore::open_at(&fixture.0).unwrap();
    assert!(reopened.conflicts().is_empty());
    assert!(!fixture.0.join(CONFLICT_LOG).exists());
    reopened.persist().unwrap();
    assert_eq!(fixture.read("layers.conf"), expected);
}

#[test]
fn touchscreen_store_with_an_unknown_action_stays_untouched_and_unwritable() {
    let fixture = Fixture::new();
    let layers = canonical();
    fixture.write("layers.conf", &layers);
    let touch = serde_json::json!({"version":2,"profiles":[{"name":"Base","selected":0,"pages":[
        {"owner":"@touchscreen:1","bindings":[{"trigger":"tap","action":plugin_action()}]}]}]})
    .to_string();
    fixture.write("touchscreen.json", &touch);

    let mut store = LayerStore::open_at(&fixture.0).unwrap();

    assert_eq!(store.diagnostics().len(), 1);
    assert!(
        store.diagnostics()[0].contains("uses an action this build cannot read")
            && store.diagnostics()[0].contains("unknown feature variant 'Plugin'")
            && store.diagnostics()[0].contains("saving is disabled"),
        "{:?}",
        store.diagnostics()
    );
    store
        .bind("Base", Trigger::KeyPress(2), Action::Emit("b".into()))
        .unwrap();
    assert!(store.persist().is_err());
    assert_eq!(fixture.read("touchscreen.json"), touch);
    assert_eq!(fixture.read("layers.conf"), layers);
}

#[test]
fn bundles_carry_unknown_rows_through_export_and_import() {
    use std::os::unix::fs::DirBuilderExt;
    let fixture = Fixture::new();
    let private = |name: &str| {
        let path = fixture.0.join(name);
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        path
    };
    let (source, target) = (private("source"), private("target"));
    fs::write(source.join("layers.conf"), canonical()).unwrap();
    let bundle = fixture.0.join("out.sdeck-plus.json");

    crate::config_bundle::export(&source, &bundle).unwrap();
    crate::config_bundle::import(&bundle, &target).unwrap();

    assert_eq!(
        fs::read_to_string(target.join("layers.conf")).unwrap(),
        canonical()
    );
    assert_eq!(
        fs::read_to_string(source.join("layers.conf")).unwrap(),
        canonical()
    );
}
