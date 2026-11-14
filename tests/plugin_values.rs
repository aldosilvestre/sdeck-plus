//! Public-API tests for display values: the manifest `values` / `display` fields, the `query`
//! request and its response limits, and the staged preview. Only generated `/bin/sh` fixtures
//! are ever executed.

use sdeck_plus::plugins::{
    stage, CancelToken, InvokeOptions, PluginBinding, PluginError, PluginManifest, PluginRegistry,
    PluginSource, QueryOutcome, Settings, StageOptions,
};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sdeck-values-{}-{n}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn dir(&self, id: &str) -> PathBuf {
        self.0.join(id)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn manifest() -> Value {
    json!({
        "id": "demo", "version": "1.0.0", "protocol_version": 1, "name": "Demo",
        "executable": "run",
        "values": [{"id": "brightness", "label": "Brightness"}],
        "actions": [
            {"id": "show", "name": "Show", "controls": ["key", "touch"], "display": "brightness",
             "settings": [{"type": "integer", "id": "zone", "label": "Zone", "min": 0, "max": 9, "default": 2}]},
            {"id": "set", "name": "Set", "controls": ["key"]}
        ]
    })
}

static SPAWN_LOCK: Mutex<()> = Mutex::new(());

fn write_script(path: &Path, body: &str) {
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn install(root: &Root, manifest: &Value, body: &str) -> PluginRegistry {
    let dir = root.dir("demo");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
    write_script(&dir.join("run"), &format!("touch ran.marker\n{body}"));
    let mut registry = PluginRegistry::open(&root.0);
    registry.set_enabled("demo", true).unwrap();
    registry
}

fn answer(response: &str) -> String {
    format!(
        "read line\nprintf '%s\\n' \"$line\" > request.json\n\
         printf '%s\\n' '{response}'"
    )
}

fn good(value: &str) -> String {
    answer(&format!(
        r#"{{"protocol_version":1,"request_id":"q-1","ok":true,"value":{value}}}"#
    ))
}

fn query(registry: &PluginRegistry, binding: &PluginBinding) -> Result<QueryOutcome, PluginError> {
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    registry.query(
        binding,
        &InvokeOptions::new("q-1").timeout(Duration::from_secs(10)),
    )
}

fn show() -> PluginBinding {
    PluginBinding::new("demo", "show", Settings::new())
}

fn ran(root: &Root) -> bool {
    root.dir("demo").join("ran.marker").exists()
}

fn rejection(manifest: Value) -> String {
    match PluginManifest::parse(manifest.to_string().as_bytes()) {
        Err(PluginError::InvalidManifest(message)) => message,
        other => panic!("expected InvalidManifest, got {other:?}"),
    }
}

// ---- manifest -------------------------------------------------------------------------------

#[test]
fn a_manifest_with_values_and_display_parses() {
    let parsed = PluginManifest::parse(manifest().to_string().as_bytes()).unwrap();
    assert_eq!(parsed.values.len(), 1);
    assert_eq!(parsed.values[0].id, "brightness");
    assert_eq!(parsed.values[0].label, "Brightness");
    assert_eq!(
        parsed.action("show").unwrap().display.as_deref(),
        Some("brightness")
    );
    assert_eq!(parsed.action("set").unwrap().display, None);
}

#[test]
fn an_old_style_manifest_is_unchanged_and_serializes_without_the_new_keys() {
    let mut old = manifest();
    old.as_object_mut().unwrap().remove("values");
    old["actions"][0].as_object_mut().unwrap().remove("display");
    let parsed = PluginManifest::parse(old.to_string().as_bytes()).unwrap();
    assert!(parsed.values.is_empty());
    assert!(parsed.actions.iter().all(|a| a.display.is_none()));
    let text = serde_json::to_string(&parsed).unwrap();
    assert!(
        !text.contains("values") && !text.contains("display"),
        "{text}"
    );
}

#[test]
fn value_ids_labels_and_counts_are_bounded() {
    let with = |values: Value| {
        let mut m = manifest();
        m["values"] = values;
        m["actions"][0].as_object_mut().unwrap().remove("display");
        m
    };
    let many = |n: usize| {
        Value::Array(
            (0..n)
                .map(|i| json!({"id": format!("v{i}"), "label": "L"}))
                .collect(),
        )
    };
    assert!(PluginManifest::parse(with(many(16)).to_string().as_bytes()).is_ok());
    assert!(rejection(with(many(17))).contains("16"));
    for bad in ["", "Bad", "has space", &"a".repeat(49)] {
        rejection(with(json!([{"id": bad, "label": "L"}])));
    }
    let ok_id = "a".repeat(48);
    assert!(PluginManifest::parse(
        with(json!([{"id": ok_id, "label": "x".repeat(64)}]))
            .to_string()
            .as_bytes()
    )
    .is_ok());
    for label in [String::new(), "x".repeat(65), "tab\there".into()] {
        rejection(with(json!([{"id": "a", "label": label}])));
    }
    assert!(rejection(with(
        json!([{"id": "a", "label": "A"}, {"id": "a", "label": "B"}])
    ))
    .contains("duplicate"));
}

#[test]
fn display_must_name_a_declared_value() {
    let mut unknown = manifest();
    unknown["actions"][0]["display"] = json!("volume");
    assert!(rejection(unknown).contains("volume"));
    let mut none_declared = manifest();
    none_declared.as_object_mut().unwrap().remove("values");
    rejection(none_declared);
    let mut bad_shape = manifest();
    bad_shape["values"][0]["extra"] = json!(1);
    rejection(bad_shape);
}

// ---- wire -----------------------------------------------------------------------------------

#[test]
fn query_sends_the_exact_request_and_returns_the_value() {
    let root = Root::new();
    let registry = install(
        &root,
        &manifest(),
        &good(r#"{"headline":"70%","detail":"Brightness","level":70}"#),
    );
    let outcome = query(&registry, &show()).unwrap();
    assert_eq!(
        outcome,
        QueryOutcome {
            headline: "70%".into(),
            detail: Some("Brightness".into()),
            level: Some(70)
        }
    );
    let sent = fs::read_to_string(root.dir("demo").join("request.json")).unwrap();
    assert_eq!(
        sent,
        "{\"protocol_version\":1,\"type\":\"query\",\"request_id\":\"q-1\",\"value\":\"brightness\",\"settings\":{\"zone\":2}}\n"
    );
}

#[test]
fn a_headline_alone_is_enough() {
    let root = Root::new();
    let registry = install(&root, &manifest(), &good(r#"{"headline":"Scene 3"}"#));
    assert_eq!(
        query(&registry, &show()).unwrap(),
        QueryOutcome {
            headline: "Scene 3".into(),
            detail: None,
            level: None
        }
    );
}

#[test]
fn value_limits_hold_at_every_boundary() {
    let accepted = [
        format!(r#"{{"headline":"{}"}}"#, "h".repeat(32)),
        format!(r#"{{"headline":"h","detail":"{}"}}"#, "d".repeat(64)),
        r#"{"headline":"h","level":0}"#.into(),
        r#"{"headline":"h","level":100}"#.into(),
    ];
    for body in &accepted {
        let root = Root::new();
        let registry = install(&root, &manifest(), &good(body));
        assert!(query(&registry, &show()).is_ok(), "{body}");
    }
    let rejected = [
        r#"{}"#.to_owned(),
        r#"{"headline":""}"#.into(),
        format!(r#"{{"headline":"{}"}}"#, "h".repeat(33)),
        r#"{"headline":"a\u0007b"}"#.into(),
        format!(r#"{{"headline":"h","detail":"{}"}}"#, "d".repeat(65)),
        r#"{"headline":"h","detail":"a\nb"}"#.into(),
        r#"{"headline":"h","level":101}"#.into(),
        r#"{"headline":"h","level":-1}"#.into(),
        r#"{"headline":"h","level":50.5}"#.into(),
        r#"{"headline":"h","level":"50"}"#.into(),
        r#"{"headline":"h","extra":1}"#.into(),
        r#""70%""#.into(),
    ];
    for body in &rejected {
        let root = Root::new();
        let registry = install(&root, &manifest(), &good(body));
        let result = query(&registry, &show());
        assert!(
            matches!(result, Err(PluginError::MalformedResponse(_))),
            "{body}: {result:?}"
        );
    }
}

#[test]
fn a_successful_query_without_a_value_is_malformed() {
    let root = Root::new();
    let registry = install(
        &root,
        &manifest(),
        &answer(r#"{"protocol_version":1,"request_id":"q-1","ok":true,"message":"hi"}"#),
    );
    assert!(matches!(
        query(&registry, &show()),
        Err(PluginError::MalformedResponse(_))
    ));
}

type Check = fn(&Result<QueryOutcome, PluginError>) -> bool;

#[test]
fn a_reported_failure_and_the_shared_envelope_checks_apply_to_queries() {
    let cases: [(&str, Check); 4] = [
        (
            r#"{"protocol_version":1,"request_id":"q-1","ok":false,"error":"offline"}"#,
            |r| matches!(r, Err(PluginError::Reported(m)) if m == "offline"),
        ),
        (
            r#"{"protocol_version":1,"request_id":"other","ok":true,"value":{"headline":"h"}}"#,
            |r| matches!(r, Err(PluginError::RequestIdMismatch)),
        ),
        (
            r#"{"protocol_version":2,"request_id":"q-1","ok":true,"value":{"headline":"h"}}"#,
            |r| matches!(r, Err(PluginError::MalformedResponse(_))),
        ),
        ("not json", |r| {
            matches!(r, Err(PluginError::MalformedResponse(_)))
        }),
    ];
    for (response, check) in cases {
        let root = Root::new();
        let registry = install(&root, &manifest(), &answer(response));
        let result = query(&registry, &show());
        assert!(check(&result), "{response}: {result:?}");
    }
}

#[test]
fn query_shares_the_exit_output_and_deadline_limits() {
    let root = Root::new();
    let registry = install(&root, &manifest(), "echo boom >&2\nexit 3");
    assert_eq!(
        query(&registry, &show()),
        Err(PluginError::NonZeroExit {
            code: Some(3),
            diagnostics: "boom\n".into()
        })
    );
    let root = Root::new();
    let registry = install(&root, &manifest(), "head -c 200000 /dev/zero | tr '\\0' x");
    assert_eq!(query(&registry, &show()), Err(PluginError::OutputTooLarge));
    let root = Root::new();
    let registry = install(&root, &manifest(), "sleep 30");
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(
        registry.query(
            &show(),
            &InvokeOptions::new("q").timeout(Duration::from_millis(300))
        ),
        Err(PluginError::Timeout)
    );
}

#[test]
fn rejected_queries_never_run_the_plugin() {
    let root = Root::new();
    let mut registry = install(&root, &manifest(), &good(r#"{"headline":"h"}"#));
    let ghost = PluginBinding::new("ghost", "show", Settings::new());
    assert_eq!(
        query(&registry, &ghost),
        Err(PluginError::Missing("ghost".into()))
    );
    let unknown = PluginBinding::new("demo", "nope", Settings::new());
    assert!(matches!(
        query(&registry, &unknown),
        Err(PluginError::UnknownAction { .. })
    ));
    let mut bad = show();
    bad.settings.insert("zone".into(), json!(10));
    assert!(matches!(
        query(&registry, &bad),
        Err(PluginError::InvalidSettings(_))
    ));
    let no_display = PluginBinding::new("demo", "set", Settings::new());
    assert_eq!(
        query(&registry, &no_display),
        Err(PluginError::NoDisplay {
            plugin: "demo".into(),
            action: "set".into()
        })
    );
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    assert!(matches!(
        registry.query(&show(), &InvokeOptions::new("has space")),
        Err(PluginError::InvalidRequest(_))
    ));
    drop(_guard);
    assert!(!ran(&root));
    registry.set_enabled("demo", false).unwrap();
    assert_eq!(
        query(&registry, &show()),
        Err(PluginError::Disabled("demo".into()))
    );
    assert!(!ran(&root));
}

#[test]
fn invoke_requests_stay_byte_identical() {
    let root = Root::new();
    let registry = install(
        &root,
        &manifest(),
        "read line\nprintf '%s\\n' \"$line\" > request.json\n\
         printf '%s\\n' '{\"protocol_version\":1,\"request_id\":\"i-1\",\"ok\":true}'",
    );
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    registry
        .invoke(
            &PluginBinding::new("demo", "set", Settings::new()),
            &InvokeOptions::new("i-1").timeout(Duration::from_secs(10)),
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.dir("demo").join("request.json")).unwrap(),
        "{\"protocol_version\":1,\"type\":\"invoke\",\"request_id\":\"i-1\",\"action\":\"set\",\"settings\":{}}\n"
    );
}

// ---- preview --------------------------------------------------------------------------------

fn staged_preview(manifest: &Value) -> sdeck_plus::plugins::PluginPreview {
    let root = Root::new();
    let project = root.0.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("manifest.json"), manifest.to_string()).unwrap();
    fs::write(project.join("run"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(project.join("run"), fs::Permissions::from_mode(0o755)).unwrap();
    let tmp = root.0.join("tmp");
    fs::create_dir_all(&tmp).unwrap();
    let staged = stage(
        &PluginSource::Folder(project),
        &StageOptions::new().temp_dir(tmp),
        &CancelToken::new(),
    )
    .unwrap();
    staged.preview().clone()
}

#[test]
fn the_preview_lists_declared_values_and_each_action_display() {
    let preview = staged_preview(&manifest());
    assert_eq!(preview.values.len(), 1);
    assert_eq!(preview.values[0].id, "brightness");
    assert_eq!(preview.values[0].label, "Brightness");
    let displays: Vec<_> = preview
        .actions
        .iter()
        .map(|a| a.display.as_deref())
        .collect();
    assert_eq!(displays, [Some("brightness"), None]);
}

#[test]
fn the_preview_of_an_old_manifest_has_no_values() {
    let mut old = manifest();
    old.as_object_mut().unwrap().remove("values");
    old["actions"][0].as_object_mut().unwrap().remove("display");
    let preview = staged_preview(&old);
    assert!(preview.values.is_empty());
    assert!(preview.actions.iter().all(|a| a.display.is_none()));
    assert_eq!(preview.actions.len(), 2);
}
