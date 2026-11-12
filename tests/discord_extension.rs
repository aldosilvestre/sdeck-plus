//! Tests for the bundled Discord demo extension through the public plugin API.
//! No test ever invokes a toggle action or a valid `send-message`: nothing here can touch
//! hyprctl, the network or a token file.

use sdeck_plus::plugins::{
    InvokeOptions, PluginBinding, PluginControl, PluginError, PluginManifest, PluginRegistry,
    SettingField, Settings,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_WINDOW: &str = "class:^(discord|vesktop|legcord)$";

struct Root(PathBuf);

impl Root {
    /// A unique temp plugin root holding a copy of `extensions/discord` (manifest and `run`).
    fn with_extension() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("sdeck-discord-{}-{n}", std::process::id()));
        let target = root.join("discord");
        fs::create_dir_all(&target).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions/discord");
        for name in ["manifest.json", "run"] {
            // `fs::copy` keeps the executable bit.
            fs::copy(source.join(name), target.join(name)).unwrap();
        }
        Self(root)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn manifest() -> PluginManifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions/discord/manifest.json");
    PluginManifest::parse(&fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))).unwrap()
}

fn text_default(manifest: &PluginManifest, action: &str, field: &str) -> Option<String> {
    let action = manifest.action(action).unwrap();
    action.settings.iter().find_map(|f| match f {
        SettingField::Text { id, default, .. } if id == field => Some(default.clone()),
        _ => None,
    })?
}

fn python_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn manifest_loads_without_problems_and_discovery_runs_nothing() {
    let root = Root::with_extension();
    let registry = PluginRegistry::open(&root.0);
    assert!(registry.problems().is_empty(), "{:?}", registry.problems());
    let plugin = registry.plugin("discord").expect("discord plugin");
    assert_eq!(plugin.name, "Discord");
    assert_eq!(plugin.protocol_version, 1);
    assert_eq!(plugin.executable, "run");
    assert!(!registry.is_enabled("discord"));
    // Only the two copied files exist: discovery wrote nothing.
    let mut files: Vec<_> = fs::read_dir(root.0.join("discord"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert_eq!(files, ["manifest.json", "run"]);
}

#[test]
fn actions_controls_and_defaults_are_as_specified() {
    let manifest = manifest();
    let ids: Vec<_> = manifest.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["toggle-mute", "toggle-deafen", "send-message"]);
    for id in ["toggle-mute", "toggle-deafen"] {
        let action = manifest.action(id).unwrap();
        assert_eq!(
            action.controls,
            [PluginControl::Key, PluginControl::DialPress]
        );
        assert_eq!(
            text_default(&manifest, id, "mods").as_deref(),
            Some("CTRL SHIFT")
        );
        assert_eq!(
            text_default(&manifest, id, "window").as_deref(),
            Some(DEFAULT_WINDOW)
        );
    }
    assert_eq!(
        text_default(&manifest, "toggle-mute", "key").as_deref(),
        Some("M")
    );
    assert_eq!(
        text_default(&manifest, "toggle-deafen", "key").as_deref(),
        Some("D")
    );
    let send = manifest.action("send-message").unwrap();
    assert_eq!(send.controls, [PluginControl::Key]);
    let fields: Vec<_> = send.settings.iter().map(SettingField::id).collect();
    assert_eq!(fields, ["channel_id", "message"]);
    assert!(send
        .settings
        .iter()
        .all(|f| matches!(f, SettingField::Text { required: true, .. })));
}

#[test]
fn host_accepts_defaults_and_rejects_bad_settings() {
    let manifest = manifest();
    for id in ["toggle-mute", "toggle-deafen"] {
        let action = manifest.action(id).unwrap();
        let filled = action.validate_settings(&Settings::new()).unwrap();
        assert_eq!(filled["mods"], json!("CTRL SHIFT"));
        assert_eq!(filled["window"], json!(DEFAULT_WINDOW));
        let mut long = Settings::new();
        long.insert("key".into(), json!("M".repeat(17)));
        assert!(matches!(
            action.validate_settings(&long),
            Err(PluginError::InvalidSettings(_))
        ));
        let mut unknown = Settings::new();
        unknown.insert("token".into(), json!("x"));
        assert!(matches!(
            action.validate_settings(&unknown),
            Err(PluginError::InvalidSettings(_))
        ));
    }
    let send = manifest.action("send-message").unwrap();
    let mut valid = Settings::new();
    valid.insert("channel_id".into(), json!("123456789012345678"));
    valid.insert("message".into(), json!("hello"));
    assert!(send.validate_settings(&valid).is_ok());
    assert!(matches!(
        send.validate_settings(&Settings::new()),
        Err(PluginError::InvalidSettings(_))
    ));
    valid.insert("message".into(), json!("x".repeat(1025)));
    assert!(matches!(
        send.validate_settings(&valid),
        Err(PluginError::InvalidSettings(_))
    ));
}

#[test]
fn invalid_channel_id_is_reported_by_the_plugin_without_side_effects() {
    if !python_available() {
        eprintln!("skipping: python3 is not installed");
        return;
    }
    let root = Root::with_extension();
    let mut registry = PluginRegistry::open(&root.0);
    registry.set_enabled("discord", true).unwrap();
    // Passes the host's text rules but fails the plugin's stricter digits-only check, which runs
    // before the token file is read or any request is built.
    let mut settings = Settings::new();
    settings.insert("channel_id".into(), json!("not-a-channel"));
    settings.insert("message".into(), json!("hello"));
    let binding = PluginBinding::new("discord", "send-message", settings);
    let options = InvokeOptions::new("req-1").timeout(Duration::from_secs(10));
    match registry.invoke(&binding, &options) {
        Err(PluginError::Reported(text)) => assert!(text.contains("channel_id"), "{text}"),
        other => panic!("expected a reported rejection, got {other:?}"),
    }
}

#[test]
fn python_unit_tests_pass_when_python_is_present() {
    if !python_available() {
        eprintln!("skipping: python3 is not installed");
        return;
    }
    let mut child = Command::new("python3")
        .args([
            "-B",
            "-I",
            "-m",
            "unittest",
            "discover",
            "-s",
            "extensions/discord/tests",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .envs([("PYTHONDONTWRITEBYTECODE", "1")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("python unit tests timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "python unit tests failed: {status}");
}
