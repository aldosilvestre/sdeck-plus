//! Public-API tests for the optional manifest `requirements` list and the PATH lookup.
//! Only scratch directories under the system temp dir are used; nothing is ever executed.

use sdeck_plus::plugins::{
    missing_commands_in, PluginError, PluginManifest, Requirement, RequirementKind,
};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sdeck-reqs-{}-{n}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.0.join(name);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn base() -> Value {
    json!({
        "id": "demo", "version": "1.0.0", "protocol_version": 1, "name": "Demo",
        "executable": "run",
        "actions": [{"id": "go", "name": "Go", "controls": ["key"]}]
    })
}

fn with(requirements: Value) -> Vec<u8> {
    let mut manifest = base();
    manifest["requirements"] = requirements;
    manifest.to_string().into_bytes()
}

fn rejection(requirements: Value) -> String {
    match PluginManifest::parse(&with(requirements)) {
        Err(PluginError::InvalidManifest(message)) => message,
        other => panic!("expected InvalidManifest, got {other:?}"),
    }
}

fn command(name: &str) -> Requirement {
    Requirement {
        kind: RequirementKind::Command,
        name: name.into(),
        description: None,
        version: None,
        optional: false,
    }
}

fn file(path: &Path, mode: u32) {
    fs::write(path, "#!/bin/sh\ntouch ran.marker\n").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn names(missing: Vec<&Requirement>) -> Vec<&str> {
    missing.into_iter().map(|r| r.name.as_str()).collect()
}

#[test]
fn a_manifest_without_requirements_parses_as_before() {
    let manifest = PluginManifest::parse(base().to_string().as_bytes()).unwrap();
    assert!(manifest.requirements.is_empty());
    let text = serde_json::to_string(&manifest).unwrap();
    assert!(!text.contains("requirements"), "{text}");
    assert_eq!(PluginManifest::parse(text.as_bytes()).unwrap(), manifest);
}

#[test]
fn valid_requirements_roundtrip_into_the_manifest() {
    let bytes = with(json!([
        {"kind": "command", "name": "hyprctl", "description": "Hyprland control", "version": ">= 0.40"},
        {"kind": "service", "name": "bluetooth", "optional": true},
        {"kind": "file", "name": "token file"}
    ]));
    let manifest = PluginManifest::parse(&bytes).unwrap();
    assert_eq!(
        manifest.requirements,
        vec![
            Requirement {
                kind: RequirementKind::Command,
                name: "hyprctl".into(),
                description: Some("Hyprland control".into()),
                version: Some(">= 0.40".into()),
                optional: false,
            },
            Requirement {
                kind: RequirementKind::Service,
                name: "bluetooth".into(),
                description: None,
                version: None,
                optional: true,
            },
            Requirement {
                kind: RequirementKind::File,
                name: "token file".into(),
                description: None,
                version: None,
                optional: false,
            },
        ]
    );
    let again = serde_json::to_vec(&manifest).unwrap();
    assert_eq!(PluginManifest::parse(&again).unwrap(), manifest);
}

#[test]
fn sixteen_requirements_are_accepted_and_seventeen_are_rejected() {
    let many = |n: usize| -> Value {
        (0..n)
            .map(|i| json!({"kind": "service", "name": format!("svc{i}")}))
            .collect()
    };
    assert_eq!(
        PluginManifest::parse(&with(many(16)))
            .unwrap()
            .requirements
            .len(),
        16
    );
    assert_eq!(rejection(many(17)), "more than 16 requirements");
}

#[test]
fn unknown_kinds_and_unknown_entry_fields_are_rejected() {
    let kind = rejection(json!([{"kind": "package", "name": "x"}]));
    assert!(kind.contains("unknown variant `package`"), "{kind}");
    let field = rejection(json!([{"kind": "command", "name": "x", "install": "pacman"}]));
    assert!(field.contains("unknown field `install`"), "{field}");
    let top = serde_json::to_vec(&{
        let mut manifest = base();
        manifest["dependencies"] = json!([]);
        manifest
    })
    .unwrap();
    assert!(matches!(
        PluginManifest::parse(&top),
        Err(PluginError::InvalidManifest(m)) if m.contains("unknown field `dependencies`")
    ));
}

#[test]
fn non_boolean_optional_and_non_array_requirements_are_rejected() {
    let optional = rejection(json!([{"kind": "file", "name": "x", "optional": "yes"}]));
    assert!(
        optional.contains("invalid type: string \"yes\""),
        "{optional}"
    );
    let shape = rejection(json!({"kind": "file", "name": "x"}));
    assert!(shape.contains("invalid type: map"), "{shape}");
}

#[test]
fn command_names_must_be_bare() {
    for bad in ["bin/ls", "/bin/ls", "my tool", "a;b", "é"] {
        assert_eq!(
            rejection(json!([{"kind": "command", "name": bad}])),
            format!("command requirement `{bad}` must be a bare command name of [A-Za-z0-9._+-]")
        );
    }
    // Slashes and spaces are fine for the informational kinds.
    assert!(PluginManifest::parse(&with(json!([
        {"kind": "file", "name": "~/.config/x/token file"},
        {"kind": "service", "name": "my service"}
    ])))
    .is_ok());
    assert!(PluginManifest::parse(&with(json!([
        {"kind": "command", "name": "g++"}, {"kind": "command", "name": "python3.12"}
    ])))
    .is_ok());
}

#[test]
fn over_long_or_unprintable_fields_are_rejected_at_the_boundary() {
    let message = "requirement names must be 1-64 printable characters";
    assert_eq!(rejection(json!([{"kind": "file", "name": ""}])), message);
    assert_eq!(
        rejection(json!([{"kind": "file", "name": "n".repeat(65)}])),
        message
    );
    assert_eq!(
        rejection(json!([{"kind": "file", "name": "a\u{7}b"}])),
        message
    );
    assert!(
        PluginManifest::parse(&with(json!([{"kind": "file", "name": "n".repeat(64)}]))).is_ok()
    );

    let description = "requirement `x` description must be at most 200 printable characters";
    assert_eq!(
        rejection(json!([{"kind": "file", "name": "x", "description": "d".repeat(201)}])),
        description
    );
    assert_eq!(
        rejection(json!([{"kind": "file", "name": "x", "description": "a\nb"}])),
        description
    );
    assert!(PluginManifest::parse(&with(
        json!([{"kind": "file", "name": "x", "description": "d".repeat(200)}])
    ))
    .is_ok());

    let version = "requirement `x` version must be at most 32 printable characters";
    assert_eq!(
        rejection(json!([{"kind": "file", "name": "x", "version": "v".repeat(33)}])),
        version
    );
    assert_eq!(
        rejection(json!([{"kind": "file", "name": "x", "version": "1\t2"}])),
        version
    );
    assert!(PluginManifest::parse(&with(
        json!([{"kind": "file", "name": "x", "version": "v".repeat(32)}])
    ))
    .is_ok());
}

#[test]
fn duplicate_kind_and_name_pairs_are_rejected_but_other_kinds_may_share_a_name() {
    assert_eq!(
        rejection(json!([
            {"kind": "command", "name": "curl"}, {"kind": "command", "name": "curl"}
        ])),
        "duplicate requirement command `curl`"
    );
    assert!(PluginManifest::parse(&with(json!([
        {"kind": "command", "name": "bluetoothctl"}, {"kind": "service", "name": "bluetoothctl"}
    ])))
    .is_ok());
}

#[test]
fn a_rejected_requirement_does_not_change_the_other_manifest_errors() {
    let mut manifest = base();
    manifest["protocol_version"] = json!(2);
    manifest["requirements"] = json!([{"kind": "command", "name": "ok"}]);
    assert_eq!(
        PluginManifest::parse(manifest.to_string().as_bytes()),
        Err(PluginError::InvalidManifest(
            "unsupported protocol_version 2".into()
        ))
    );
}

#[test]
fn the_path_check_reports_missing_commands_only_and_keeps_declaration_order() {
    let scratch = Scratch::new();
    let (first, second) = (scratch.dir("a"), scratch.dir("b"));
    file(&first.join("present"), 0o755);
    file(&second.join("later"), 0o700);
    let path = std::env::join_paths([&first, &second]).unwrap();
    let mut service = command("absent-service");
    service.kind = RequirementKind::Service;
    let requirements = vec![
        command("gone"),
        command("present"),
        service,
        command("later"),
        command("also-gone"),
    ];
    assert_eq!(
        names(missing_commands_in(&requirements, &path)),
        vec!["gone", "also-gone"]
    );
}

#[test]
fn non_executable_files_and_directories_do_not_count_as_commands() {
    let scratch = Scratch::new();
    let bin = scratch.dir("bin");
    file(&bin.join("plain"), 0o644);
    fs::create_dir(bin.join("folder")).unwrap();
    let requirements = vec![command("plain"), command("folder")];
    assert_eq!(
        names(missing_commands_in(&requirements, bin.as_os_str())),
        vec!["plain", "folder"]
    );
}

#[test]
fn relative_and_empty_path_entries_are_ignored() {
    let scratch = Scratch::new();
    let bin = scratch.dir("rel");
    file(&bin.join("tool"), 0o755);
    let relative = bin.strip_prefix("/").unwrap();
    let requirements = vec![command("tool")];
    for path in [
        OsString::new(),
        OsString::from(":"),
        OsString::from("."),
        relative.as_os_str().to_owned(),
        OsString::from(format!(":.:{}", relative.display())),
    ] {
        assert_eq!(
            names(missing_commands_in(&requirements, &path)),
            vec!["tool"],
            "{path:?}"
        );
    }
    // Run from `/`, the relative form would resolve; it must still be ignored.
    let found = std::env::join_paths(["", bin.to_str().unwrap()]).unwrap();
    assert!(missing_commands_in(&requirements, &found).is_empty());
}

#[test]
fn symlinks_count_as_std_metadata_resolves_them() {
    let scratch = Scratch::new();
    let (real, bin) = (scratch.dir("real"), scratch.dir("links"));
    file(&real.join("target"), 0o755);
    file(&real.join("plain-target"), 0o644);
    symlink(real.join("target"), bin.join("good")).unwrap();
    symlink(real.join("plain-target"), bin.join("not-exec")).unwrap();
    symlink(real.join("nowhere"), bin.join("dangling")).unwrap();
    let requirements = vec![command("good"), command("not-exec"), command("dangling")];
    assert_eq!(
        names(missing_commands_in(&requirements, bin.as_os_str())),
        vec!["not-exec", "dangling"]
    );
}

#[test]
fn the_path_check_never_runs_a_command() {
    let scratch = Scratch::new();
    let bin = scratch.dir("bin");
    file(&bin.join("probe"), 0o755);
    let requirements = vec![command("probe")];
    assert!(missing_commands_in(&requirements, bin.as_os_str()).is_empty());
    assert!(!bin.join("ran.marker").exists());
    assert!(!scratch.0.join("ran.marker").exists());
}
