//! Runs the built `plugin_validate` example on generated plugin projects. Only `/bin/sh`
//! fixtures are ever executed, and only by the explicit `--invoke` tests.

use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sdeck-validate-{}-{n}", std::process::id()));
        fs::create_dir_all(path.join("tmp")).unwrap();
        Self(path)
    }

    /// A plugin project named `dir` (not necessarily its id) with a shell executable `run`.
    fn project(&self, dir: &str, manifest: &Value, script: &str) -> PathBuf {
        let path = self.0.join(dir);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("manifest.json"), manifest.to_string()).unwrap();
        fs::write(path.join("run"), script).unwrap();
        fs::set_permissions(path.join("run"), fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn marker(&self) -> PathBuf {
        self.0.join("ran.marker")
    }

    /// The validator's temp root lives here (via `TMPDIR`) so leftovers are detectable.
    fn tmp(&self) -> PathBuf {
        self.0.join("tmp")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `cargo test` only type-checks examples, so the first call builds this one (offline, same
/// profile) when it is not linked yet. `None` means it cannot be located or built.
fn example() -> Option<PathBuf> {
    static BUILD: std::sync::Once = std::sync::Once::new();
    let exe = std::env::current_exe().ok()?;
    let profile_dir = exe.parent()?.parent()?;
    let path = profile_dir.join("examples/plugin_validate");
    BUILD.call_once(|| {
        let profile = profile_dir.file_name().and_then(|name| name.to_str());
        let mut build = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        build.args(["build", "--offline", "--example", "plugin_validate"]);
        match profile {
            Some("debug") => {}
            Some("release") => {
                build.arg("--release");
            }
            _ => return,
        }
        if !path.is_file() {
            let _ = build.status();
        }
    });
    path.is_file().then_some(path)
}

macro_rules! binary {
    () => {
        match example() {
            Some(path) => path,
            None => {
                eprintln!("SKIPPED: examples/plugin_validate is not built next to this test");
                return;
            }
        }
    };
}

fn run(binary: &Path, scratch: &Scratch, args: &[&str]) -> Output {
    Command::new(binary)
        .args(args)
        .env("TMPDIR", scratch.tmp())
        .output()
        .unwrap()
}

fn out(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn manifest(id: &str) -> Value {
    json!({
        "id": id, "version": "1.2.3", "protocol_version": 1, "name": "Fixture",
        "executable": "run",
        "actions": [{
            "id": "send", "name": "Send", "controls": ["key", "dial_press"],
            "settings": [
                {"type": "text", "id": "channel", "label": "Channel", "required": true},
                {"type": "boolean", "id": "loud", "label": "Loud"}
            ]
        }],
        "requirements": [
            {"kind": "command", "name": "sh", "version": ">= 1"},
            {"kind": "command", "name": "sdeck-absent-tool"},
            {"kind": "service", "name": "bluetooth", "optional": true}
        ]
    })
}

fn marker_script(marker: &Path) -> String {
    format!(
        "#!/bin/sh\ncat >/dev/null\ntouch '{}'\nprintf '{}\\n'\n",
        marker.display(),
        r#"{"protocol_version":1,"request_id":"req-1","ok":true,"message":"hi"}"#
    )
}

fn nothing_left(scratch: &Scratch) {
    let left: Vec<_> = fs::read_dir(scratch.tmp()).unwrap().collect();
    assert!(left.is_empty(), "temp root not removed: {left:?}");
}

#[test]
fn a_valid_project_validates_without_running_it_and_its_directory_name_may_differ() {
    let binary = binary!();
    let scratch = Scratch::new();
    let project = scratch.project(
        "my-plugin-project",
        &manifest("fixture"),
        &marker_script(&scratch.marker()),
    );
    let output = run(&binary, &scratch, &[project.to_str().unwrap()]);
    let text = out(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(text.starts_with("PLUGIN_VALIDATE_OK:"), "{text}");
    for expected in [
        "id: fixture",
        "version: 1.2.3",
        "protocol: 1",
        "action: send",
        "key, dial_press",
        "channel (text, required)",
        "loud (boolean)",
        "requirement: command sh >= 1 - found on PATH",
        "requirement: command sdeck-absent-tool - MISSING from PATH",
        "requirement: service bluetooth (optional)",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    assert!(
        !scratch.marker().exists(),
        "validation must not run the plugin"
    );
    nothing_left(&scratch);
}

#[test]
fn a_project_without_requirements_validates_too() {
    let binary = binary!();
    let scratch = Scratch::new();
    let mut plain = manifest("fixture");
    plain.as_object_mut().unwrap().remove("requirements");
    let project = scratch.project("p", &plain, "#!/bin/sh\n");
    let output = run(&binary, &scratch, &[project.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", out(&output));
    assert!(
        out(&output).contains("requirements: none"),
        "{}",
        out(&output)
    );
}

#[test]
fn invalid_projects_report_errors_and_exit_one() {
    let binary = binary!();
    let scratch = Scratch::new();
    let mut cases: Vec<(&str, PathBuf, &str)> = Vec::new();

    let mut bad = manifest("fixture");
    bad["protocol_version"] = json!(2);
    cases.push((
        "manifest",
        scratch.project("bad-manifest", &bad, "#!/bin/sh\n"),
        "unsupported protocol_version 2",
    ));

    let mut unknown = manifest("fixture");
    unknown["requirements"] = json!([{"kind": "package", "name": "x"}]);
    cases.push((
        "kind",
        scratch.project("bad-kind", &unknown, "#!/bin/sh\n"),
        "unknown variant `package`",
    ));

    let missing = scratch.project("no-exe", &manifest("fixture"), "#!/bin/sh\n");
    fs::remove_file(missing.join("run")).unwrap();
    cases.push(("missing exe", missing, "run: not found"));

    let linked = scratch.project("linked-exe", &manifest("fixture"), "#!/bin/sh\n");
    fs::rename(linked.join("run"), linked.join("real-run")).unwrap();
    symlink("real-run", linked.join("run")).unwrap();
    cases.push(("symlink", linked, "symlink"));

    let big = scratch.project("big", &manifest("fixture"), "#!/bin/sh\n");
    fs::write(big.join("blob.bin"), vec![0u8; 9 * 1024 * 1024]).unwrap();
    cases.push(("large file", big, "blob.bin"));

    let nomanifest = scratch.0.join("empty");
    fs::create_dir_all(&nomanifest).unwrap();
    cases.push(("no manifest", nomanifest, "manifest.json"));

    for (name, path, needle) in cases {
        let output = run(&binary, &scratch, &[path.to_str().unwrap()]);
        let text = out(&output);
        assert_eq!(output.status.code(), Some(1), "{name}: {text}");
        assert!(text.starts_with("PLUGIN_VALIDATE_ERROR:"), "{name}: {text}");
        assert!(text.contains(needle), "{name}: {needle}\n{text}");
        assert!(!text.contains("PLUGIN_VALIDATE_OK"), "{name}: {text}");
    }
    nothing_left(&scratch);
}

#[test]
fn skipped_directories_do_not_count_and_the_file_count_is_bounded() {
    let binary = binary!();
    let scratch = Scratch::new();
    let project = scratch.project("p", &manifest("fixture"), "#!/bin/sh\n");
    fs::create_dir_all(project.join(".git")).unwrap();
    symlink("/nonexistent", project.join(".git/link")).unwrap();
    fs::create_dir_all(project.join("__pycache__")).unwrap();
    fs::write(
        project.join("__pycache__/x.pyc"),
        vec![0u8; 9 * 1024 * 1024],
    )
    .unwrap();
    let output = run(&binary, &scratch, &[project.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", out(&output));

    for n in 0..300 {
        fs::write(project.join(format!("f{n}.txt")), "x").unwrap();
    }
    let output = run(&binary, &scratch, &[project.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert!(out(&output).contains("more than"), "{}", out(&output));
    nothing_left(&scratch);
}

#[test]
fn bad_usage_exits_two() {
    let binary = binary!();
    let scratch = Scratch::new();
    let project = scratch.project("p", &manifest("fixture"), "#!/bin/sh\n");
    let dir = project.to_str().unwrap();
    for args in [
        vec![],
        vec![dir, "extra"],
        vec![dir, "--unknown"],
        vec![dir, "--settings", "{}"],
        vec![dir, "--timeout", "3"],
        vec![dir, "--request-id", "x"],
        vec![dir, "--invoke"],
        vec![dir, "--invoke", "send", "--timeout", "0"],
        vec![dir, "--invoke", "send", "--timeout", "61"],
        vec![dir, "--invoke", "send", "--timeout", "soon"],
        vec![dir, "--invoke", "send", "--settings", "[1]"],
        vec![dir, "--invoke", "send", "--settings", "{nope"],
    ] {
        let output = run(&binary, &scratch, &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {}", out(&output));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("usage"),
            "{args:?}"
        );
    }
    assert!(!scratch.marker().exists());
    nothing_left(&scratch);
}

#[test]
fn invoke_runs_the_plugin_once_only_when_asked_and_warns_first() {
    let binary = binary!();
    let scratch = Scratch::new();
    let project = scratch.project("p", &manifest("fixture"), &marker_script(&scratch.marker()));
    let dir = project.to_str().unwrap();
    let output = run(
        &binary,
        &scratch,
        &[
            dir,
            "--invoke",
            "send",
            "--settings",
            r#"{"channel":"general"}"#,
            "--request-id",
            "req-1",
            "--timeout",
            "10",
        ],
    );
    let text = out(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    let warning = text.find("RUNS the plugin").expect("warning");
    let outcome = text.find("PLUGIN_INVOKE_OK:").expect("outcome");
    assert!(warning < outcome, "{text}");
    assert!(text.contains("hi"), "{text}");
    assert!(
        scratch.marker().exists(),
        "the explicit invoke must run the plugin"
    );
    nothing_left(&scratch);
}

#[test]
fn invoke_failures_are_reported_and_exit_one() {
    let binary = binary!();
    let scratch = Scratch::new();
    let failing = scratch.project(
        "fail",
        &manifest("fixture"),
        "#!/bin/sh\ncat >/dev/null\necho boom >&2\nexit 3\n",
    );
    let dir = failing.to_str().unwrap();
    let output = run(
        &binary,
        &scratch,
        &[dir, "--invoke", "send", "--settings", r#"{"channel":"a"}"#],
    );
    let text = out(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.contains("PLUGIN_INVOKE_ERROR:") && text.contains("boom"),
        "{text}"
    );

    let marker = marker_script(&scratch.marker());
    let ok = scratch.project("ok", &manifest("fixture"), &marker);
    let dir = ok.to_str().unwrap();
    for args in [
        vec![dir, "--invoke", "nope"],
        vec![dir, "--invoke", "send"],
        vec![
            dir,
            "--invoke",
            "send",
            "--settings",
            r#"{"channel":"a","extra":1}"#,
        ],
    ] {
        let output = run(&binary, &scratch, &args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {}", out(&output));
        assert!(
            out(&output).contains("PLUGIN_INVOKE_ERROR:"),
            "{}",
            out(&output)
        );
    }
    assert!(
        !scratch.marker().exists(),
        "rejected invocations must not run the plugin"
    );
    nothing_left(&scratch);
}
