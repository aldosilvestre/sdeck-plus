//! Hermetic checks of the AUR packaging files: repository files only, no makepkg or network.
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

const PACKAGES: [&str; 2] = ["sdeck-plus", "sdeck-plus-git"];
const MAINTAINER_EMAIL: &str = "aldosilvestre89@gmail.com";
const MAINTAINER: &str = "# Maintainer: Aldo Silvestre <aldosilvestre89@gmail.com>";

fn pkgbuild(package: &str) -> String {
    read(&format!("packaging/aur/{package}/PKGBUILD"))
}

fn srcinfo(package: &str) -> String {
    read(&format!("packaging/aur/{package}/.SRCINFO"))
}

/// Scalar assignment such as `pkgver=0.1.0` (quotes stripped).
fn scalar(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    text.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|value| value.trim_matches(|c| c == '\'' || c == '"').to_owned())
}

/// Array assignment such as `depends=('a' 'b')` on one line.
fn array(text: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key}=(");
    text.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|rest| {
            rest.trim_end_matches(')')
                .split_whitespace()
                .map(|item| item.trim_matches('\'').to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// `key = value` lines of a .SRCINFO, in order.
fn srcinfo_values(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix(&format!("{key} = ")))
        .map(str::to_owned)
        .collect()
}

fn cargo_version() -> String {
    let manifest = read("Cargo.toml");
    let package = manifest.split("[dependencies]").next().unwrap();
    scalar(&package.replace(" = ", "="), "version").expect("Cargo.toml version")
}

#[test]
fn every_installed_source_file_exists_in_the_repository() {
    let installed = [
        "packaging/dev.streamdeck.EventMonitor.desktop",
        "assets/icons/scalable/apps/sdeck-plus.svg",
        "LICENSE",
        "packaging/70-sdeck-plus.rules",
    ];
    for package in PACKAGES {
        let text = pkgbuild(package);
        for path in installed {
            assert!(text.contains(path), "{package} must install {path}");
            assert!(Path::new(&root().join(path)).is_file(), "{path} missing");
        }
        for target in [
            "/usr/bin/sdeck-plus",
            "/usr/share/applications/dev.streamdeck.EventMonitor.desktop",
            "/usr/share/icons/hicolor/scalable/apps/sdeck-plus.svg",
            "/usr/share/licenses/$pkgname/LICENSE",
            "/usr/lib/udev/rules.d/70-sdeck-plus.rules",
        ] {
            assert!(text.contains(target), "{package} must install to {target}");
        }
    }
}

#[test]
fn stable_pkgver_equals_the_cargo_version() {
    let text = pkgbuild("sdeck-plus");
    assert_eq!(scalar(&text, "pkgver"), Some(cargo_version()));
    assert!(text.contains("archive/refs/tags/v$pkgver.tar.gz"));
}

#[test]
fn desktop_file_matches_the_application_identity() {
    let desktop = read("packaging/dev.streamdeck.EventMonitor.desktop");
    let main = read("src/main.rs");
    let app_id = main
        .lines()
        .find_map(|line| line.strip_prefix("const APP_ID: &str = \""))
        .and_then(|rest| rest.strip_suffix("\";"))
        .expect("APP_ID");
    assert_eq!(app_id, "dev.streamdeck.EventMonitor");
    for line in [
        "Type=Application",
        "Exec=sdeck-plus",
        "Icon=sdeck-plus",
        "Terminal=false",
        "Categories=Settings;HardwareSettings;",
        &format!("StartupWMClass={app_id}"),
    ] {
        assert!(desktop.lines().any(|l| l == line), "missing {line}");
    }
}

#[test]
fn udev_rule_matches_the_ids_of_both_models() {
    // The vendor id lives in the HID module and the product ids in the device model table.
    let vendor = read("src/event_api.rs").to_lowercase();
    assert!(vendor.contains("0x0fd9"));
    let models = read("src/device_model.rs").to_lowercase();
    let rules = read("packaging/70-sdeck-plus.rules");
    let lines: Vec<_> = rules
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2, "one rule per model");
    for (line, pid) in lines.iter().zip(["0084", "00c6"]) {
        assert!(
            models.contains(&format!("pid: 0x{pid}")),
            "model table: {pid}"
        );
        assert_eq!(
            *line,
            format!(
                r#"SUBSYSTEM=="hidraw", ATTRS{{idVendor}}=="0fd9", ATTRS{{idProduct}}=="{pid}", TAG+="uaccess""#
            )
        );
    }
    assert!(rules.starts_with('#'), "rule needs a header comment");
}

#[test]
fn stable_package_pins_the_published_release_checksum() {
    let text = pkgbuild("sdeck-plus");
    let sums = array(&text, "sha256sums");
    assert_eq!(sums.len(), 1);
    assert!(
        sums[0].len() == 64
            && sums[0]
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "stable sha256sums must be a real lowercase hex digest, found {:?}",
        sums[0]
    );
    assert!(!text.contains("REPLACE with `updpkgsums`"));
}

#[test]
fn git_package_replaces_the_stable_package() {
    let text = pkgbuild("sdeck-plus-git");
    assert_eq!(array(&text, "provides"), ["sdeck-plus"]);
    assert_eq!(array(&text, "conflicts"), ["sdeck-plus"]);
    assert!(array(&text, "makedepends").contains(&"git".to_owned()));
    assert!(text.contains("pkgver()") && text.contains("git describe --tags --long"));
    assert!(text.contains("git+https://github.com/aldosilvestre/sdeck-plus.git"));
}

#[test]
fn packages_share_metadata_and_the_maintainer() {
    for package in PACKAGES {
        let text = pkgbuild(package);
        assert_eq!(text.lines().next(), Some(MAINTAINER));
        assert!(!text.contains("TODO(human)"));
        assert_eq!(array(&text, "arch"), ["x86_64", "aarch64"]);
        assert_eq!(array(&text, "license"), ["MIT"]);
        assert_eq!(
            scalar(&text, "url").as_deref(),
            Some("https://aldosilvestre.github.io/sdeck-plus/")
        );
        assert_eq!(array(&text, "options"), ["!lto"]);
    }
}

#[test]
fn packaging_files_carry_only_the_maintainer_email() {
    fn walk(dir: &Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else if let Ok(text) = fs::read_to_string(&path) {
                for word in text.split_whitespace() {
                    let word = word.trim_matches(|c: char| "<>()'\",;".contains(c));
                    if let Some((local, domain)) = word.split_once('@') {
                        let host = domain.trim_end_matches('/');
                        // ssh://aur@aur.archlinux.org/... is a URL, not a mailbox.
                        if !local.is_empty()
                            && host.contains('.')
                            && !word.contains("://")
                            && !local.contains('/')
                            && word != MAINTAINER_EMAIL
                        {
                            found.push(format!("{}: {word}", path.display()));
                        }
                    }
                }
            }
        }
    }
    let mut found = Vec::new();
    walk(&root().join("packaging"), &mut found);
    assert!(found.is_empty(), "unexpected email-like strings: {found:?}");
}

#[test]
fn srcinfo_matches_its_pkgbuild() {
    for package in PACKAGES {
        let build = pkgbuild(package);
        let info = srcinfo(package);
        assert_eq!(srcinfo_values(&info, "pkgbase"), [package]);
        assert_eq!(srcinfo_values(&info, "pkgname"), [package]);
        assert_eq!(
            srcinfo_values(&info, "pkgver"),
            [scalar(&build, "pkgver").unwrap()]
        );
        for (key, srcinfo_key) in [
            ("depends", "depends"),
            ("makedepends", "makedepends"),
            ("optdepends", "optdepends"),
            ("provides", "provides"),
            ("conflicts", "conflicts"),
        ] {
            let expected: Vec<String> = if key == "optdepends" {
                srcinfo_values(&info, "optdepends")
            } else {
                array(&build, key)
            };
            if key != "optdepends" {
                assert_eq!(
                    srcinfo_values(&info, srcinfo_key),
                    expected,
                    "{package} {key}"
                );
            }
        }
        // Every optdepends entry in the PKGBUILD appears in .SRCINFO.
        for dep in srcinfo_values(&info, "optdepends") {
            let name = dep.split(':').next().unwrap();
            assert!(
                build.contains(&format!("'{name}:")),
                "{package} optdepends {name}"
            );
        }
    }
}
