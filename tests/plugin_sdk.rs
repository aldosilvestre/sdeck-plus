//! Guards the plugin SDK: the starter template in `examples/plugin-template` works with the real
//! validator and its own unit tests, and the SDK page and `docs/plugins.md` state the numbers the
//! host really enforces. Only the harmless template is ever executed; nothing needs a display or
//! the network.

use sdeck_plus::plugins::{
    MAX_MANIFEST_BYTES, MAX_MESSAGE_CHARS, MAX_SETTINGS_BYTES, MAX_STDERR_BYTES, MAX_STDOUT_BYTES,
    MAX_TIMEOUT, PROTOCOL_VERSION,
};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const SDK_PAGE: &str = "plugin-sdk.html";
const FORWARDED_ENV: [&str; 8] = [
    "PATH",
    "HOME",
    "LANG",
    "XDG_RUNTIME_DIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn template() -> PathBuf {
    root().join("examples/plugin-template")
}

fn python3_exists() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
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

fn validate(binary: &Path, args: &[&str]) -> (Output, String) {
    let output = Command::new(binary)
        .arg(template())
        .args(args)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    (output, text)
}

// ---- the template -------------------------------------------------------------------------

#[test]
fn the_template_validates_without_running_it() {
    let Some(binary) = example() else {
        eprintln!("SKIPPED: examples/plugin_validate is not built next to this test");
        return;
    };
    let (output, text) = validate(&binary, &[]);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(text.starts_with("PLUGIN_VALIDATE_OK:"), "{text}");
    assert!(text.contains("hello"), "{text}");
    assert!(text.contains("say-hello"), "{text}");
}

#[test]
fn the_template_answers_a_real_invocation() {
    let Some(binary) = example() else {
        eprintln!("SKIPPED: examples/plugin_validate is not built next to this test");
        return;
    };
    if !python3_exists() {
        eprintln!("SKIPPED: python3 is not installed");
        return;
    }
    let (output, text) = validate(
        &binary,
        &["--invoke", "say-hello", "--settings", r#"{"name":"Ada"}"#],
    );
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(text.contains("PLUGIN_INVOKE_OK:"), "{text}");
    assert!(text.contains("Ada"), "the greeting names the user: {text}");
}

#[test]
fn the_template_rejects_settings_the_manifest_does_not_allow() {
    let Some(binary) = example() else {
        eprintln!("SKIPPED: examples/plugin_validate is not built next to this test");
        return;
    };
    if !python3_exists() {
        eprintln!("SKIPPED: python3 is not installed");
        return;
    }
    let (output, text) = validate(
        &binary,
        &["--invoke", "say-hello", "--settings", r#"{"nme":"Ada"}"#],
    );
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("PLUGIN_INVOKE_ERROR:"), "{text}");
    assert!(text.contains("unknown setting"), "{text}");
}

#[test]
fn the_template_python_tests_pass() {
    if !python3_exists() {
        eprintln!("SKIPPED: python3 is not installed");
        return;
    }
    let mut child = Command::new("python3")
        .args(["-B", "-I", "-m", "unittest", "discover", "-s", "tests"])
        .current_dir(template())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the template's unit tests did not finish within 60 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = child.wait_with_output().unwrap();
    let report = String::from_utf8_lossy(&output.stderr);
    assert!(status.success(), "unittest failed:\n{report}");
    assert!(
        report.contains("OK"),
        "unexpected unittest report:\n{report}"
    );
}

#[test]
fn the_template_is_a_complete_small_project() {
    let dir = template();
    let run = std::fs::metadata(dir.join("run")).expect("examples/plugin-template/run exists");
    assert_eq!(run.permissions().mode() & 0o777, 0o755, "run must be 0755");
    let lines = read("examples/plugin-template/run").lines().count();
    assert!(
        lines < 120,
        "run has {lines} lines; keep the template small"
    );
    for file in ["manifest.json", "README.md", "LICENSE", "tests/test_run.py"] {
        assert!(dir.join(file).is_file(), "template is missing {file}");
    }
    assert!(read("examples/plugin-template/README.md").contains("## Dependencies"));
    let manifest: serde_json::Value =
        serde_json::from_str(&read("examples/plugin-template/manifest.json")).unwrap();
    assert_eq!(manifest["id"], "hello");
    let requirements = manifest["requirements"].as_array().expect("requirements");
    assert!(
        requirements
            .iter()
            .any(|r| r["kind"] == "command" && r["name"] == "python3"),
        "the manifest must declare python3"
    );
}

// ---- the documentation matches the code -----------------------------------------------------

fn expected_numbers() -> Vec<(String, &'static str)> {
    vec![
        (
            format!("{} KiB", MAX_STDOUT_BYTES / 1024),
            "the stdout limit (MAX_STDOUT_BYTES)",
        ),
        (
            format!("{} KiB", MAX_MANIFEST_BYTES / 1024),
            "the manifest size limit (MAX_MANIFEST_BYTES)",
        ),
        (
            format!("{MAX_SETTINGS_BYTES} bytes"),
            "the settings payload limit (MAX_SETTINGS_BYTES)",
        ),
        (
            format!("{MAX_STDERR_BYTES} bytes"),
            "the stderr bytes kept (MAX_STDERR_BYTES)",
        ),
        (
            format!("{MAX_MESSAGE_CHARS} characters"),
            "the message/error length (MAX_MESSAGE_CHARS)",
        ),
        (
            format!("{} s", MAX_TIMEOUT.as_secs()),
            "the longest request timeout (MAX_TIMEOUT)",
        ),
        (
            format!(r#""protocol_version":{PROTOCOL_VERSION}"#),
            "the protocol version on the wire (PROTOCOL_VERSION)",
        ),
    ]
}

#[test]
fn the_sdk_page_and_the_contract_state_the_real_limits() {
    for document in [format!("docs/{SDK_PAGE}"), "docs/plugins.md".to_owned()] {
        let text = read(&document);
        for (needle, what) in expected_numbers() {
            assert!(
                text.contains(&needle),
                "{document} does not state {what}: expected the text `{needle}`"
            );
        }
    }
}

#[test]
fn both_documents_list_every_forwarded_environment_variable() {
    for document in [format!("docs/{SDK_PAGE}"), "docs/plugins.md".to_owned()] {
        let text = read(&document);
        for name in FORWARDED_ENV {
            assert!(
                text.contains(name),
                "{document} does not list the forwarded variable {name}"
            );
        }
    }
}

#[test]
fn the_sdk_page_names_every_invocation_outcome() {
    let page = read(&format!("docs/{SDK_PAGE}"));
    for outcome in [
        "Reported",
        "NonZeroExit",
        "MalformedResponse",
        "RequestIdMismatch",
        "OutputTooLarge",
        "Timeout",
    ] {
        assert!(page.contains(outcome), "the SDK page lacks `{outcome}`");
    }
}

#[test]
fn the_contract_points_to_the_sdk_page() {
    assert!(
        read("docs/plugins.md").contains(SDK_PAGE),
        "docs/plugins.md must point to {SDK_PAGE}"
    );
}

#[test]
fn the_site_links_to_the_sdk_page() {
    let sitemap = read("docs/sitemap.xml");
    assert!(
        sitemap.contains(&format!(
            "<loc>https://aldosilvestre.github.io/sdeck-plus/{SDK_PAGE}</loc>"
        )),
        "docs/sitemap.xml lacks {SDK_PAGE}"
    );
    for page in ["docs/docs.html", "docs/index.html"] {
        assert!(
            read(page).contains(&format!("href=\"{SDK_PAGE}")),
            "{page} does not link to {SDK_PAGE}"
        );
    }
}

// ---- structure of the SDK page --------------------------------------------------------------

/// Values of `name="..."` attributes, in document order. The page is hand-written, so a plain
/// scan is enough; it never has the pattern inside text because angle brackets are escaped.
fn attribute_values(html: &str, name: &str) -> Vec<String> {
    let pattern = format!(" {name}=\"");
    let mut values = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(&pattern) {
        rest = &rest[start + pattern.len()..];
        let end = rest.find('"').expect("unterminated attribute");
        values.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    values
}

#[test]
fn the_sdk_page_has_one_h1_and_unique_ids() {
    let page = read(&format!("docs/{SDK_PAGE}"));
    assert_eq!(
        page.matches("<h1").count(),
        1,
        "exactly one <h1> is expected"
    );
    let ids = attribute_values(&page, "id");
    let unique: BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "duplicate ids on the SDK page");
}

#[test]
fn every_link_on_the_sdk_page_resolves() {
    let page = read(&format!("docs/{SDK_PAGE}"));
    let ids: BTreeSet<String> = attribute_values(&page, "id").into_iter().collect();
    for href in attribute_values(&page, "href") {
        if href.starts_with("https://") || href.starts_with("mailto:") {
            continue;
        }
        if let Some(fragment) = href.strip_prefix('#') {
            assert!(ids.contains(fragment), "href=\"#{fragment}\" has no target");
            continue;
        }
        let (file, fragment) = match href.split_once('#') {
            Some((file, fragment)) => (file, Some(fragment)),
            None => (href.as_str(), None),
        };
        let target = root().join("docs").join(file);
        assert!(target.exists(), "href=\"{href}\" points to a missing file");
        if let (Some(fragment), true) = (fragment, file.ends_with(".html")) {
            let linked = std::fs::read_to_string(&target).unwrap();
            assert!(
                attribute_values(&linked, "id")
                    .iter()
                    .any(|id| id == fragment),
                "href=\"{href}\": {file} has no id `{fragment}`"
            );
        }
    }
}

#[test]
fn the_sdk_page_covers_the_sections_in_order() {
    let page = read(&format!("docs/{SDK_PAGE}"));
    let mut last = 0;
    for id in [
        "overview",
        "quick-start",
        "layout",
        "manifest",
        "settings",
        "protocol",
        "limits",
        "security",
        "dependencies",
        "testing",
        "installing",
        "compatibility",
        "troubleshooting",
        "reference-plugins",
        "checklist",
    ] {
        let at = page
            .find(&format!("id=\"{id}\""))
            .unwrap_or_else(|| panic!("the SDK page lacks the section id `{id}`"));
        assert!(at > last, "section `{id}` is out of order");
        last = at;
    }
}
