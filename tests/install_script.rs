//! Hermetic public-interface checks: no network, real build, or user directories.
use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
const SCRIPT: &str = include_str!("../install.sh");

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sdeck-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let s = Self(path);
        for dir in ["tools", "home", "tmp", "caller-target", "caller-cargo"] {
            fs::create_dir(s.path(dir)).unwrap();
        }
        s.write("caller-target/keep", "target");
        s.write("caller-cargo/keep", "cache");
        s.write("api", "{\n  \"tag_name\": \"v1.2.3\"\n}\n");
        s.tree("source/sdeck-plus-1.2.3");
        s.archive();
        s.tool(
            "curl",
            r#"#!/bin/bash
set -eu
printf '%s\n' "$*" >> "$FIXTURE/requests"
[[ ${FAIL_CURL:-} != yes ]] || exit 22
out=''
while (( $# )); do
    case "$1" in
        --output) out=$2; shift 2 ;;
        *) url=$1; shift ;;
    esac
done
case "$url" in
    https://fixture.invalid/latest|https://api.github.com/repos/aldosilvestre/sdeck-plus/releases/latest) cat "$FIXTURE/api" ;;
    https://fixture.invalid/tags/v*.tar.gz|https://github.com/aldosilvestre/sdeck-plus/archive/refs/tags/v1.2.3.tar.gz) cp "$FIXTURE/archive.tar.gz" "$out" ;;
    *) echo 'Unexpected URL' >&2; exit 99 ;;
esac
"#,
        );
        s.tool(
            "cargo",
            r#"#!/bin/bash
set -eu
printf '%s\n' "$PWD" "$CARGO_HOME" "$CARGO_TARGET_DIR" "$*" > "$FIXTURE/build"
case ${BUILD_MODE:-ok} in
    fail) exit 42 ;;
    int) kill -INT "$PPID"; exit 130 ;;
    term) kill -TERM "$PPID"; exit 143 ;;
esac
dir=''
while (( $# )); do
    case "$1" in --target-dir) dir=$2; shift 2 ;; *) shift ;; esac
done
mkdir -p "$dir/test-host/release"
printf 'binary' > "$dir/test-host/release/sdeck-plus"
"#,
        );
        s.tool("rustc", "#!/bin/bash\nprintf 'host: test-host\\n'\n");
        for name in [
            "cc",
            "pkg-config",
            "glib-compile-resources",
            "update-desktop-database",
            "gtk-update-icon-cache",
        ] {
            s.tool(name, "#!/bin/bash\nexit 0\n");
        }
        s.tool("pgrep", "#!/bin/bash\nexit 1\n");
        s
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn write(&self, name: &str, text: &str) {
        let p = self.path(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
    fn tool(&self, name: &str, text: &str) {
        let p = format!("tools/{name}");
        self.write(&p, text);
        fs::set_permissions(self.path(&p), fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn tree(&self, root: &str) {
        self.write(
            &format!("{root}/Cargo.toml"),
            "[package]\nname = \"sdeck-plus\"\nversion = \"1.2.3\"\nedition = \"2021\"\n",
        );
        self.write(&format!("{root}/LICENSE"), "license");
        self.write(
            &format!("{root}/assets/icons/scalable/apps/sdeck-plus.svg"),
            "icon",
        );
        self.write(&format!("{root}/install.sh"), SCRIPT);
    }
    /// Replace the fixture release with `manifest` under the directory for `version`.
    fn release_with_manifest(&self, version: &str, manifest: &str) {
        self.write("api", &format!("{{\"tag_name\": \"v{version}\"}}\n"));
        fs::remove_dir_all(self.path("source")).unwrap();
        self.tree(&format!("source/sdeck-plus-{version}"));
        self.write(&format!("source/sdeck-plus-{version}/Cargo.toml"), manifest);
        assert!(Command::new("tar")
            .arg("-czf")
            .arg(self.path("archive.tar.gz"))
            .arg("-C")
            .arg(self.path("source"))
            .arg(format!("sdeck-plus-{version}"))
            .status()
            .unwrap()
            .success());
    }
    fn archive(&self) {
        assert!(Command::new("tar")
            .args(["-czf"])
            .arg(self.path("archive.tar.gz"))
            .arg("-C")
            .arg(self.path("source"))
            .arg("sdeck-plus-1.2.3")
            .status()
            .unwrap()
            .success());
    }
    fn run(&self, checkout: bool, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut c = Command::new("bash");
        c.env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.path("tools").display()),
            )
            .env("HOME", self.path("home"))
            .env("PREFIX", self.path("prefix with % $ spaces"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("TMPDIR", self.path("tmp"))
            .env("FIXTURE", &self.0)
            .env("CARGO_HOME", self.path("caller-cargo"))
            .env("CARGO_TARGET_DIR", self.path("caller-target"))
            .env("SDECK_PLUS_INSTALL_TEST", "1")
            .env("SDECK_PLUS_RELEASE_API", "https://fixture.invalid/latest")
            .env("SDECK_PLUS_ARCHIVE_BASE", "https://fixture.invalid/tags")
            .envs(env.iter().copied())
            .current_dir(&self.0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if checkout {
            self.tree("checkout");
            c.arg(self.path("checkout/install.sh"));
        } else {
            c.args(["-s", "--"]);
        }
        c.args(args).stdin(Stdio::piped());
        let mut child = c.spawn().unwrap();
        if !checkout {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(SCRIPT.as_bytes())
                .unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("installer exceeded bounded wait: {output:?}");
            }
            std::thread::yield_now();
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            fs::read_to_string(self.path("caller-target/keep")).unwrap(),
            "target"
        );
        assert_eq!(
            fs::read_to_string(self.path("caller-cargo/keep")).unwrap(),
            "cache"
        );
        assert_eq!(
            fs::read_dir(self.path("tmp")).unwrap().count(),
            0,
            "owned scratch leaked"
        );
        output
    }
    fn no_install(&self) {
        for name in ["prefix with % $ spaces", "data", "build"] {
            assert!(!self.path(name).exists(), "unexpected write: {name}");
        }
    }
    fn installed(&self) {
        assert_eq!(
            fs::read_to_string(self.path("prefix with % $ spaces/bin/sdeck-plus")).unwrap(),
            "binary"
        );
        let mut files = Vec::new();
        collect_files(&self.path("data"), &mut files);
        files.sort();
        assert_eq!(
            files,
            [
                "applications/dev.streamdeck.EventMonitor.desktop",
                "doc/sdeck-plus/LICENSE",
                "icons/hicolor/scalable/apps/sdeck-plus.svg"
            ]
        );
        let desktop =
            fs::read_to_string(self.path("data/applications/dev.streamdeck.EventMonitor.desktop"))
                .unwrap();
        assert!(desktop.contains("Exec=/usr/bin/env \""));
        assert!(
            desktop.contains("prefix with %% \\\\$ spaces/bin/sdeck-plus\"\n"),
            "{desktop}"
        );
    }
}
fn collect_files(root: &Path, out: &mut Vec<String>) {
    fn visit(root: &Path, path: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(path).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                visit(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_str().unwrap().to_owned());
            }
        }
    }
    visit(root, root, out);
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn success(o: &Output) {
    assert!(
        o.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        o.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

#[test]
fn pinned_release_skips_api_and_installs_exact_tag() {
    for args in [vec!["--tag", "v1.2.3"], vec!["--tag=v1.2.3"]] {
        let s = Sandbox::new();
        s.write("api", "must not be read");
        let o = s.run(false, &args, &[]);
        success(&o);
        s.installed();
        let requests = fs::read_to_string(s.path("requests")).unwrap();
        assert_eq!(requests.lines().count(), 1);
        assert!(requests.ends_with("https://fixture.invalid/tags/v1.2.3.tar.gz\n"));
    }
}

#[test]
fn invalid_pinned_arguments_have_no_side_effects() {
    for args in [
        vec!["--tag", "v1.2"],
        vec!["--tag", "1.2.3"],
        vec!["--tag", "v1.2.3-rc1"],
        vec!["--tag", "v1.2.3/../x"],
        vec!["--tag", ""],
        vec!["--tag"],
        vec!["--tag="],
        vec!["--tag", "v1.2.3", "--tag=v1.2.3"],
    ] {
        let s = Sandbox::new();
        let o = s.run(false, &args, &[]);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {o:?}");
        assert!(o.stdout.is_empty());
        let argument = if args.len() > 2 { args[2] } else { args[0] };
        assert_eq!(
            String::from_utf8_lossy(&o.stderr),
            format!("Unknown argument: {argument}. Use --help for usage.\n")
        );
        s.no_install();
        assert!(!s.path("requests").exists());
    }
}

#[test]
fn checkout_rejects_pinned_tag_before_building() {
    let s = Sandbox::new();
    let o = s.run(true, &["--tag", "v1.2.3"], &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&o.stderr),
        "Unknown argument: --tag (only supported without a checkout). Use --help for usage.\n"
    );
    s.no_install();
    assert!(!s.path("requests").exists());
}

#[test]
fn piped_release_installs_only_deliverables_and_cleans_build_state() {
    let s = Sandbox::new();
    let o = s.run(false, &[], &[]);
    success(&o);
    assert!(String::from_utf8_lossy(&o.stdout).contains("Installed executable:"));
    s.installed();
    let build = fs::read_to_string(s.path("build")).unwrap();
    assert!(!build.contains("caller-target"));
    assert!(!build.contains("caller-cargo"));
    assert!(build.contains("build --release --locked --bin sdeck-plus"));
    assert!(build.lines().take(3).all(|p| !Path::new(p).exists()));
}

#[test]
fn invalid_or_absent_release_tag_is_rejected_without_installing() {
    for response in [
        "{}",
        "{\"tag_name\":\"v1.2.3-rc1\"}",
        "{\"tag_name\":\"../../bad\"}",
        "{\"tag_name\":\"1.2.3\"}",
    ] {
        let s = Sandbox::new();
        s.write("api", response);
        let o = s.run(false, &[], &[]);
        assert_eq!(o.status.code(), Some(1));
        assert!(o.stdout.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&o.stderr),
            "Latest release must have a tag of the form vX.Y.Z. A published release is required.\n"
        );
        s.no_install();
    }
}

#[test]
fn additional_invalid_release_tags_are_rejected() {
    for tag in ["v1.2", "v1.2.3/../x", ""] {
        let s = Sandbox::new();
        s.write("api", &format!("{{\"tag_name\":\"{tag}\"}}"));
        let o = s.run(false, &[], &[]);
        assert_eq!(o.status.code(), Some(1));
        assert!(o.stdout.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&o.stderr),
            "Latest release must have a tag of the form vX.Y.Z. A published release is required.\n"
        );
        s.no_install();
    }
}

#[test]
fn release_whose_cargo_version_differs_from_tag_is_rejected_before_building() {
    for version in ["9.9.9", "1.2.30"] {
        let s = Sandbox::new();
        s.write(
            "source/sdeck-plus-1.2.3/Cargo.toml",
            &format!("[package]\nname = \"sdeck-plus\"\nversion = \"{version}\"\n"),
        );
        s.archive();
        let o = s.run(false, &[], &[]);
        assert_eq!(o.status.code(), Some(1), "{o:?}");
        assert!(o.stdout.is_empty());
        assert!(String::from_utf8_lossy(&o.stderr).starts_with("Unsafe release archive"));
        s.no_install();
    }
}

#[test]
fn multiline_string_cannot_fake_the_package_version() {
    for delimiter in ["\"\"\"", "'''"] {
        let s = Sandbox::new();
        s.release_with_manifest(
            "1.2.3",
            &format!(
                "[package]\nname = \"sdeck-plus\"\ndescription = {delimiter}\nversion = \"1.2.3\"\n{delimiter}\nversion = \"9.9.9\"\n"
            ),
        );
        let o = s.run(false, &[], &[]);
        assert_eq!(o.status.code(), Some(1), "{o:?}");
        assert!(o.stdout.is_empty());
        assert!(String::from_utf8_lossy(&o.stderr).starts_with("Unsafe release archive"));
        s.no_install();
    }
}

#[test]
fn package_heading_comment_and_crlf_manifest_install() {
    for manifest in [
        "[package] # c\nname = \"sdeck-plus\"\nversion = \"1.2.3\" # c\n",
        "[package]\r\nname = \"sdeck-plus\"\r\nversion = \"1.2.3\"\r\n",
    ] {
        let s = Sandbox::new();
        s.release_with_manifest("1.2.3", manifest);
        let o = s.run(false, &[], &[]);
        success(&o);
        s.installed();
    }
}

#[test]
fn lookalike_version_forms_are_still_rejected() {
    for manifest in [
        "[package]\nversion.workspace = true\n",
        "[package]\npackage.version = \"1.2.3\"\n",
        "[package]\nmeta = { version = \"1.2.3\" }\n",
        "[dependencies]\nversion = \"1.2.3\"\n",
        "[package.metadata]\nversion = \"1.2.3\"\n",
    ] {
        let s = Sandbox::new();
        s.release_with_manifest("1.2.3", manifest);
        let o = s.run(false, &[], &[]);
        assert_eq!(o.status.code(), Some(1), "{manifest}: {o:?}");
        s.no_install();
    }
}

#[test]
fn repository_manifest_passes_for_its_own_tag() {
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let version = manifest
        .lines()
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|l| l.strip_suffix('"'))
        .unwrap()
        .to_owned();
    let s = Sandbox::new();
    s.release_with_manifest(&version, &manifest);
    let o = s.run(false, &[], &[]);
    success(&o);
    s.installed();
}

#[test]
fn oversized_release_is_rejected_before_building() {
    let s = Sandbox::new();
    fs::File::create(s.path("source/sdeck-plus-1.2.3/large"))
        .unwrap()
        .set_len(256 * 1024 * 1024)
        .unwrap();
    s.archive();
    let o = s.run(false, &[], &[]);
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert!(o.stdout.is_empty());
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("Unsafe release archive"));
    s.no_install();
}

#[test]
fn expanded_release_just_under_limit_still_installs() {
    let s = Sandbox::new();
    fs::File::create(s.path("source/sdeck-plus-1.2.3/large"))
        .unwrap()
        .set_len(128 * 1024 * 1024 - 32 * 1024)
        .unwrap();
    s.archive();
    success(&s.run(false, &[], &[]));
    s.installed();
}

#[test]
fn oversized_download_is_rejected_even_if_curl_ignores_limit() {
    let s = Sandbox::new();
    fs::OpenOptions::new()
        .write(true)
        .open(s.path("archive.tar.gz"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let o = s.run(false, &[], &[]);
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert!(o.stdout.is_empty());
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("Unsafe release archive"));
    s.no_install();
    let requests = fs::read_to_string(s.path("requests")).unwrap();
    assert!(requests.contains("--max-filesize 67108864"));
}

#[test]
fn too_many_archive_entries_are_rejected_before_building() {
    let s = Sandbox::new();
    for index in 0..5000 {
        s.write(&format!("source/sdeck-plus-1.2.3/entry-{index}"), "");
    }
    s.archive();
    let o = s.run(false, &[], &[]);
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert!(o.stdout.is_empty());
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("Unsafe release archive"));
    s.no_install();
}

#[test]
fn desktop_write_failure_removes_all_owned_temporary_files() {
    let s = Sandbox::new();
    s.tool("chmod", "#!/bin/bash\nexit 46\n");
    let o = s.run(false, &[], &[]);
    assert_eq!(o.status.code(), Some(46));
    assert!(o.stderr.is_empty());
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("Building sdeck-plus"));
    assert_eq!(
        fs::read_dir(s.path("data/applications")).unwrap().count(),
        0
    );
}

#[test]
fn release_overrides_are_ignored_without_explicit_test_mode() {
    let s = Sandbox::new();
    success(&s.run(false, &[], &[("SDECK_PLUS_INSTALL_TEST", "")]));
    s.installed();
    let requests = fs::read_to_string(s.path("requests")).unwrap();
    assert!(!requests.contains("fixture.invalid"), "{requests}");
    assert!(
        requests.contains("https://api.github.com/repos/aldosilvestre/sdeck-plus/releases/latest")
    );
    assert!(requests
        .contains("https://github.com/aldosilvestre/sdeck-plus/archive/refs/tags/v1.2.3.tar.gz"));
}

#[test]
fn api_failure_is_clear_and_leaves_no_installation() {
    let s = Sandbox::new();
    let o = s.run(false, &[], &[("FAIL_CURL", "yes")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&o.stderr), "Could not fetch the latest published release. Check curl, network access and GitHub Releases.\n");
    s.no_install();
}

#[test]
fn build_failure_and_signals_remove_owned_scratch() {
    for (mode, code) in [("fail", 42), ("int", 130), ("term", 143)] {
        let s = Sandbox::new();
        let o = s.run(false, &[], &[("BUILD_MODE", mode)]);
        assert_eq!(o.status.code(), Some(code), "{o:?}");
        assert!(o.stderr.is_empty(), "{o:?}");
        assert!(String::from_utf8_lossy(&o.stdout).contains("Building sdeck-plus"));
        assert!(!s.path("data").exists());
        assert!(!s.path("prefix with % $ spaces").exists());
    }
}

#[test]
fn archive_links_and_extra_top_level_directory_are_rejected() {
    for link in [true, false] {
        let s = Sandbox::new();
        if link {
            symlink("/tmp", s.path("source/sdeck-plus-1.2.3/escape")).unwrap();
        } else {
            s.write("source/sdeck-plus-1.2.3/../other/file", "bad");
        }
        let mut tar = Command::new("tar");
        tar.arg("-czf")
            .arg(s.path("archive.tar.gz"))
            .arg("-C")
            .arg(s.path("source"))
            .arg("sdeck-plus-1.2.3");
        if !link {
            tar.arg("other");
        }
        assert!(tar.status().unwrap().success());
        let o = s.run(false, &[], &[]);
        assert_eq!(o.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&o.stderr).contains("Unsafe release archive"));
        s.no_install();
    }
}

#[test]
fn checkout_build_and_piped_uninstall_keep_configuration() {
    let s = Sandbox::new();
    success(&s.run(true, &[], &[]));
    s.installed();
    assert!(!s.path("requests").exists());
    assert!(s.path("checkout/Cargo.toml").exists());
    s.write("data/sdeck-plus/organization.json", "keep profiles");
    s.write("config/autostart/sdeck-plus.desktop", "keep startup");
    let o = s.run(false, &["--uninstall"], &[]);
    success(&o);
    assert!(String::from_utf8_lossy(&o.stdout)
        .contains("Configuration, profiles and artwork were kept."));
    assert!(!s.path("prefix with % $ spaces/bin/sdeck-plus").exists());
    assert_eq!(
        fs::read_to_string(s.path("data/sdeck-plus/organization.json")).unwrap(),
        "keep profiles"
    );
    assert_eq!(
        fs::read_to_string(s.path("config/autostart/sdeck-plus.desktop")).unwrap(),
        "keep startup"
    );
    assert!(!s.path("requests").exists());
}

#[test]
fn help_unknown_arguments_and_relative_prefix_do_not_install() {
    let s = Sandbox::new();
    let help = s.run(false, &["--help"], &[]);
    success(&help);
    assert!(String::from_utf8_lossy(&help.stdout).starts_with("Usage:"));
    let bad = s.run(false, &["--bad"], &[]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(bad.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&bad.stderr),
        "Unknown argument: --bad. Use --help for usage.\n"
    );
    let relative = s.run(false, &[], &[("PREFIX", "relative")]);
    assert_eq!(relative.status.code(), Some(1));
    assert!(relative.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&relative.stderr),
        "Installation paths must be absolute.\n"
    );
    s.no_install();
    assert!(!s.path("requests").exists());
}

/// Run the installer with a fake sysfs holding one hidraw node per `(name, HID_ID)`.
fn run_with_hidraw(nodes: &[(&str, &str)]) -> String {
    let s = Sandbox::new();
    for (name, id) in nodes {
        s.write(
            &format!("sys/{name}/device/uevent"),
            &format!("DRIVER=hid-generic\nHID_ID={id}\n"),
        );
    }
    let root = s.path("sys");
    let o = s.run(
        true,
        &[],
        &[("SDECK_PLUS_HIDRAW_ROOT", root.to_str().unwrap())],
    );
    success(&o);
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn unreadable_stream_deck_nodes_print_one_udev_rule_per_model() {
    for id in ["0003:00000FD9:00000084", "0003:00000fd9:000000c6"] {
        let out = run_with_hidraw(&[("hidraw-sdeck-fixture", id)]);
        assert!(out.contains("cannot open the Stream Deck at /dev/hidraw-sdeck-fixture"));
        let rules: Vec<_> = out.lines().filter(|l| l.contains("SUBSYSTEM==")).collect();
        assert_eq!(rules.len(), 2, "{out}");
        assert!(
            rules[0].contains(r#"ATTRS{idProduct}=="0084""#)
                && rules[0].ends_with("| sudo tee /etc/udev/rules.d/70-sdeck-plus.rules")
        );
        assert!(
            rules[1].contains(r#"ATTRS{idProduct}=="00c6""#)
                && rules[1].contains("sudo tee -a /etc/udev/rules.d/70-sdeck-plus.rules")
        );
    }
}

#[test]
fn other_hid_devices_get_no_udev_note() {
    let out = run_with_hidraw(&[("hidraw-sdeck-fixture", "0003:00000FD9:00000080")]);
    assert!(!out.contains("udev"), "{out}");
}
