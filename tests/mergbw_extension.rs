//! Tests for the bundled MeRGBW demo extension through the public plugin API.
//! The only real round trip uses an address the plugin rejects before any `busctl` call, so no
//! test here can touch Bluetooth, D-Bus or the network.

use sdeck_plus::plugins::{
    InvokeOptions, PluginBinding, PluginControl, PluginError, PluginManifest, PluginRegistry,
    SettingField, Settings,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ADDRESS_ACTIONS: [&str; 5] = [
    "power-on",
    "power-off",
    "set-color",
    "set-brightness",
    "set-scene",
];

struct Root(PathBuf);

impl Root {
    /// A unique temp plugin root holding a copy of `extensions/mergbw` (manifest and `run`).
    fn with_extension() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root =
            std::env::temp_dir().join(format!("sdeck-mergbw-{}-{nanos}-{n}", std::process::id()));
        let target = root.join("mergbw");
        fs::create_dir_all(&target).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions/mergbw");
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
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions/mergbw/manifest.json");
    PluginManifest::parse(&fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))).unwrap()
}

fn integer_field(manifest: &PluginManifest, action: &str, field: &str) -> (i64, i64, Option<i64>) {
    let action = manifest.action(action).unwrap();
    action
        .settings
        .iter()
        .find_map(|f| match f {
            SettingField::Integer {
                id,
                min,
                max,
                default,
                ..
            } if id == field => Some((*min, *max, *default)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no integer field {field}"))
}

fn python_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn address_settings(address: &str) -> Settings {
    let mut settings = Settings::new();
    settings.insert("address".into(), json!(address));
    settings
}

#[test]
fn manifest_loads_without_problems_and_discovery_runs_nothing() {
    let root = Root::with_extension();
    let registry = PluginRegistry::open(&root.0);
    assert!(registry.problems().is_empty(), "{:?}", registry.problems());
    let plugin = registry.plugin("mergbw").expect("mergbw plugin");
    assert_eq!(plugin.name, "MeRGBW Hexagon Light");
    assert_eq!(plugin.protocol_version, 1);
    assert_eq!(plugin.executable, "run");
    assert!(!registry.is_enabled("mergbw"));
    let mut files: Vec<_> = fs::read_dir(root.0.join("mergbw"))
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
    assert_eq!(ids, ADDRESS_ACTIONS);
    for id in ADDRESS_ACTIONS {
        let action = manifest.action(id).unwrap();
        assert_eq!(
            action.controls,
            [PluginControl::Key, PluginControl::DialPress]
        );
        assert!(
            matches!(
                action.settings.first(),
                Some(SettingField::Text {
                    id,
                    required: true,
                    max_length: 17,
                    ..
                }) if id == "address"
            ),
            "{id}: first setting must be the required address"
        );
    }
    assert_eq!(
        integer_field(&manifest, "set-color", "hue"),
        (0, 359, Some(0))
    );
    assert_eq!(
        integer_field(&manifest, "set-color", "saturation"),
        (0, 100, Some(100))
    );
    assert_eq!(
        integer_field(&manifest, "set-color", "brightness"),
        (0, 100, Some(100))
    );
    assert_eq!(
        integer_field(&manifest, "set-brightness", "brightness"),
        (0, 100, Some(100))
    );
    assert_eq!(
        integer_field(&manifest, "set-scene", "scene"),
        (1, 117, Some(2))
    );
}

#[test]
fn host_accepts_defaults_and_rejects_out_of_range_values() {
    let manifest = manifest();
    let color = manifest.action("set-color").unwrap();
    let filled = color
        .validate_settings(&address_settings("AA:BB:CC:DD:EE:FF"))
        .unwrap();
    assert_eq!(filled["hue"], json!(0));
    assert_eq!(filled["saturation"], json!(100));
    assert_eq!(filled["brightness"], json!(100));
    assert!(matches!(
        color.validate_settings(&Settings::new()),
        Err(PluginError::InvalidSettings(_))
    ));
    for (action, field, bad) in [
        ("set-color", "hue", 360),
        ("set-color", "hue", -1),
        ("set-color", "saturation", 101),
        ("set-brightness", "brightness", 101),
        ("set-scene", "scene", 0),
        ("set-scene", "scene", 118),
    ] {
        let mut settings = address_settings("AA:BB:CC:DD:EE:FF");
        settings.insert(field.into(), json!(bad));
        assert!(
            matches!(
                manifest
                    .action(action)
                    .unwrap()
                    .validate_settings(&settings),
                Err(PluginError::InvalidSettings(_))
            ),
            "{action}.{field}={bad}"
        );
    }
    let scene = manifest.action("set-scene").unwrap();
    let filled = scene
        .validate_settings(&address_settings("AA:BB:CC:DD:EE:FF"))
        .unwrap();
    assert_eq!(filled["scene"], json!(2));
}

#[test]
fn invalid_address_is_reported_by_the_plugin_without_side_effects() {
    if !python_available() {
        eprintln!("skipping: python3 is not installed");
        return;
    }
    let root = Root::with_extension();
    let mut registry = PluginRegistry::open(&root.0);
    registry.set_enabled("mergbw", true).unwrap();
    // 17 characters, so the host's text rules pass; the plugin's strict MAC check rejects it
    // before any `busctl` call, so the PATH and Bluetooth state are irrelevant.
    let binding = PluginBinding::new("mergbw", "power-on", address_settings("not-a-mac-address"));
    let options = InvokeOptions::new("req-1").timeout(Duration::from_secs(10));
    match registry.invoke(&binding, &options) {
        Err(PluginError::Reported(text)) => assert!(text.contains("address"), "{text}"),
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
            "extensions/mergbw/tests",
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
