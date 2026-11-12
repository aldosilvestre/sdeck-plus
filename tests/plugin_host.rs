//! Public-API tests for plugin discovery, enablement and supervised invocation.
//! Only generated `/bin/sh` fixtures are ever executed.

use sdeck_plus::plugins::{
    InvokeOptions, InvokeOutcome, PluginBinding, PluginError, PluginRegistry, Settings,
};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sdeck-plugins-{}-{n}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn plugin_dir(&self, id: &str) -> PathBuf {
        self.0.join(id)
    }
    fn marker(&self, id: &str) -> PathBuf {
        self.plugin_dir(id).join("ran.marker")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn manifest(id: &str, executable: &str) -> Value {
    json!({
        "id": id, "version": "1.0.0", "protocol_version": 1, "name": "Fixture",
        "executable": executable,
        "actions": [{
            "id": "send", "name": "Send", "controls": ["key", "dial_press"],
            "settings": [
                {"type": "text", "id": "channel", "label": "Channel", "required": true, "max_length": 20},
                {"type": "integer", "id": "level", "label": "Level", "min": 0, "max": 255, "default": 10},
                {"type": "boolean", "id": "loud", "label": "Loud"}
            ]
        }]
    })
}

fn install_manifest(root: &Root, id: &str, manifest: &Value) {
    fs::create_dir_all(root.plugin_dir(id)).unwrap();
    fs::write(
        root.plugin_dir(id).join("manifest.json"),
        manifest.to_string(),
    )
    .unwrap();
}

/// Fixture writes and child spawns share one lock: a fork in another test thread would otherwise
/// still hold a freshly written script open and make its `exec` fail with ETXTBSY.
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

fn invoke(
    registry: &PluginRegistry,
    binding: &PluginBinding,
    options: &InvokeOptions,
) -> Result<InvokeOutcome, PluginError> {
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    registry.invoke(binding, options)
}

fn write_script(path: &Path, body: &str) {
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Installs a plugin whose `run` script is `body`; every run first touches `ran.marker`.
fn install(root: &Root, id: &str, body: &str) {
    install_manifest(root, id, &manifest(id, "run"));
    write_script(
        &root.plugin_dir(id).join("run"),
        &format!("touch ran.marker\n{body}"),
    );
}

const ECHO_OK: &str = r#"read line
printf '%s\n' "$line" > request.json
rid=$(printf '%s' "$line" | sed 's/.*"request_id":"\([^"]*\)".*/\1/')
printf '{"protocol_version":1,"request_id":"%s","ok":true,"message":"done"}\n' "$rid""#;

fn binding(id: &str) -> PluginBinding {
    let mut settings = Settings::new();
    settings.insert("channel".into(), json!("general"));
    PluginBinding::new(id, "send", settings)
}

fn options() -> InvokeOptions {
    InvokeOptions::new("req-1").timeout(Duration::from_secs(10))
}

fn enabled(root: &Root, id: &str) -> PluginRegistry {
    let mut registry = PluginRegistry::open(root.path());
    registry.set_enabled(id, true).unwrap();
    registry
}

#[test]
fn discovery_lists_valid_plugins_without_executing_them() {
    let root = Root::new();
    install(&root, "demo", ECHO_OK);
    let registry = PluginRegistry::open(root.path());
    assert_eq!(
        registry
            .plugins()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["demo"]
    );
    assert!(registry.problems().is_empty());
    assert!(!root.marker("demo").exists());
}

#[test]
fn plugins_stay_disabled_until_explicitly_enabled_and_state_persists() {
    let root = Root::new();
    install(&root, "demo", ECHO_OK);
    let registry = PluginRegistry::open(root.path());
    assert!(!registry.is_enabled("demo"));
    assert_eq!(
        invoke(&registry, &binding("demo"), &options()),
        Err(PluginError::Disabled("demo".into()))
    );
    assert!(!root.marker("demo").exists());

    let mut registry = registry;
    registry.set_enabled("demo", true).unwrap();
    assert!(PluginRegistry::open(root.path()).is_enabled("demo"));
    registry.set_enabled("demo", false).unwrap();
    assert!(!PluginRegistry::open(root.path()).is_enabled("demo"));
}

#[test]
fn invoke_round_trips_a_versioned_request_and_response() {
    let root = Root::new();
    install(&root, "demo", ECHO_OK);
    let registry = enabled(&root, "demo");
    let outcome = invoke(&registry, &binding("demo"), &options()).unwrap();
    assert_eq!(outcome.request_id, "req-1");
    assert_eq!(outcome.message.as_deref(), Some("done"));
    let request: Value = serde_json::from_str(
        &fs::read_to_string(root.plugin_dir("demo").join("request.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        request,
        json!({"protocol_version": 1, "type": "invoke", "request_id": "req-1", "action": "send",
               "settings": {"channel": "general", "level": 10}})
    );
}

#[test]
fn malformed_response_is_rejected() {
    let root = Root::new();
    install(&root, "demo", "read line\necho 'not json'");
    let registry = enabled(&root, "demo");
    let result = invoke(&registry, &binding("demo"), &options());
    assert!(
        matches!(result, Err(PluginError::MalformedResponse(_))),
        "{result:?}"
    );
}

#[test]
fn nonzero_exit_reports_stderr_diagnostics() {
    let root = Root::new();
    install(&root, "demo", "echo boom >&2\nexit 3");
    let registry = enabled(&root, "demo");
    assert_eq!(
        invoke(&registry, &binding("demo"), &options()),
        Err(PluginError::NonZeroExit {
            code: Some(3),
            diagnostics: "boom\n".into()
        })
    );
}

#[test]
fn timeout_kills_the_child_and_its_descendants() {
    let root = Root::new();
    install(&root, "demo", "sleep 30 &\necho $! > child.pid\nsleep 30");
    let registry = enabled(&root, "demo");
    let started = Instant::now();
    let result = invoke(
        &registry,
        &binding("demo"),
        &InvokeOptions::new("r").timeout(Duration::from_millis(300)),
    );
    assert_eq!(result, Err(PluginError::Timeout));
    assert!(started.elapsed() < Duration::from_secs(5));
    let pid = fs::read_to_string(root.plugin_dir("demo").join("child.pid")).unwrap();
    let stat = PathBuf::from(format!("/proc/{}/stat", pid.trim()));
    let deadline = Instant::now() + Duration::from_secs(5);
    // A killed orphan may linger as a zombie until init reaps it.
    while Instant::now() < deadline
        && fs::read_to_string(&stat).is_ok_and(|text| !text.contains(") Z"))
    {
        std::thread::yield_now();
    }
    assert!(fs::read_to_string(&stat).map_or(true, |text| text.contains(") Z")));
}

#[test]
fn descendant_holding_pipes_open_does_not_block_completion() {
    let root = Root::new();
    install(&root, "demo", &format!("sleep 30 &\n{ECHO_OK}"));
    let registry = enabled(&root, "demo");
    let started = Instant::now();
    assert!(invoke(&registry, &binding("demo"), &options()).is_ok());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn oversized_stdout_is_capped() {
    let root = Root::new();
    install(&root, "demo", "head -c 200000 /dev/zero | tr '\\0' x");
    let registry = enabled(&root, "demo");
    assert_eq!(
        invoke(&registry, &binding("demo"), &options()),
        Err(PluginError::OutputTooLarge)
    );
}

#[test]
fn unsafe_executables_are_reported_and_never_disable_other_plugins() {
    let root = Root::new();
    install(&root, "good", ECHO_OK);
    install_manifest(&root, "dotdot", &manifest("dotdot", "../good/run"));
    install_manifest(&root, "absolute", &manifest("absolute", "/bin/sh"));
    install_manifest(&root, "escape", &manifest("escape", "link"));
    write_script(&root.path().join("outside"), "touch outside.marker");
    symlink(
        root.path().join("outside"),
        root.plugin_dir("escape").join("link"),
    )
    .unwrap();
    install_manifest(&root, "mismatch", &manifest("other", "run"));
    fs::create_dir_all(root.plugin_dir("broken")).unwrap();
    fs::write(root.plugin_dir("broken").join("manifest.json"), "{").unwrap();

    let registry = PluginRegistry::open(root.path());
    assert_eq!(
        registry
            .plugins()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["good"]
    );
    let mut bad: Vec<&str> = registry
        .problems()
        .iter()
        .map(|p| p.directory.as_str())
        .collect();
    bad.sort_unstable();
    assert_eq!(bad, ["absolute", "broken", "dotdot", "escape", "mismatch"]);
    assert!(!root.path().join("outside.marker").exists());
    assert!(!root.path().join("good/ran.marker").exists());
}

#[test]
fn invalid_manifest_fields_are_reported() {
    let root = Root::new();
    let mut bad_bounds = manifest("bounds", "run");
    bad_bounds["actions"][0]["settings"][1]["min"] = json!(300);
    let mut duplicate = manifest("dup", "run");
    duplicate["actions"][0]["settings"][1]["id"] = json!("channel");
    let mut protocol = manifest("proto", "run");
    protocol["protocol_version"] = json!(2);
    let mut bad_id = manifest("Bad Id", "run");
    bad_id["id"] = json!("Bad Id");
    for (dir, value) in [
        ("bounds", bad_bounds),
        ("dup", duplicate),
        ("proto", protocol),
        ("Bad Id", bad_id),
    ] {
        install_manifest(&root, dir, &value);
    }
    let registry = PluginRegistry::open(root.path());
    assert_eq!(registry.plugins().count(), 0);
    assert_eq!(registry.problems().len(), 4);
}

#[test]
fn invalid_settings_are_rejected_before_execution() {
    let root = Root::new();
    install(&root, "demo", ECHO_OK);
    let registry = enabled(&root, "demo");
    let mut cases = Vec::new();
    for (key, value) in [
        ("level", json!(256)),
        ("level", json!("high")),
        ("extra", json!(1)),
        ("channel", json!("x".repeat(21))),
    ] {
        let mut b = binding("demo");
        b.settings.insert(key.into(), value);
        cases.push(b);
    }
    cases.push(PluginBinding::new("demo", "send", Settings::new()));
    for case in cases {
        let result = invoke(&registry, &case, &options());
        assert!(
            matches!(result, Err(PluginError::InvalidSettings(_))),
            "{result:?}"
        );
    }
    assert!(!root.marker("demo").exists());
}

#[test]
fn invalid_request_id_and_timeout_are_rejected_before_execution() {
    let root = Root::new();
    install(&root, "demo", ECHO_OK);
    let registry = enabled(&root, "demo");
    for bad in [
        InvokeOptions::new("has space"),
        InvokeOptions::new(""),
        InvokeOptions::new("ok").timeout(Duration::ZERO),
        InvokeOptions::new("ok").timeout(Duration::from_secs(3600)),
    ] {
        let result = invoke(&registry, &binding("demo"), &bad);
        assert!(
            matches!(result, Err(PluginError::InvalidRequest(_))),
            "{result:?}"
        );
    }
    assert!(!root.marker("demo").exists());
}

#[test]
fn missing_plugin_binding_stays_representable_and_cannot_be_enabled_or_invoked() {
    let root = Root::new();
    let mut registry = PluginRegistry::open(root.path());
    let ghost = binding("ghost");
    let restored: PluginBinding =
        serde_json::from_str(&serde_json::to_string(&ghost).unwrap()).unwrap();
    assert_eq!(restored, ghost);
    assert_eq!(
        invoke(&registry, &ghost, &options()),
        Err(PluginError::Missing("ghost".into()))
    );
    assert_eq!(
        registry.set_enabled("ghost", true),
        Err(PluginError::Missing("ghost".into()))
    );
    assert!(!root.path().join("plugins.json").exists());
}

#[test]
fn invalid_enabled_state_is_kept_and_blocks_writes() {
    let root = Root::new();
    install(&root, "demo", ECHO_OK);
    fs::write(root.path().join("plugins.json"), "{garbage").unwrap();
    let mut registry = PluginRegistry::open(root.path());
    assert!(registry.state_error().is_some());
    assert!(!registry.is_enabled("demo"));
    assert!(matches!(
        registry.set_enabled("demo", true),
        Err(PluginError::InvalidState(_))
    ));
    assert_eq!(
        fs::read_to_string(root.path().join("plugins.json")).unwrap(),
        "{garbage"
    );
    assert!(!root.marker("demo").exists());
}
