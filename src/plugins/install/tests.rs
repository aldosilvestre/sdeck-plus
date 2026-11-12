//! Tests that need the injectable pieces (fake git runner, failure hook, test-only `file://`
//! clone). The public flow is covered by `tests/plugin_install.rs`.

use super::git::{valid_commit, GitRunner, SystemGit};
use super::place::{install_with, Step};
use super::stage::stage_with;
use super::*;
use crate::plugins::PluginRegistry;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::process::Command;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sdeck-install-unit-{}-{n}", std::process::id()));
        fs::create_dir_all(path.join("tmp")).unwrap();
        Self(path)
    }

    fn options(&self) -> StageOptions {
        StageOptions::new().temp_dir(self.0.join("tmp"))
    }

    fn nothing_staged(&self) {
        assert_eq!(fs::read_dir(self.0.join("tmp")).unwrap().count(), 0);
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_project(dir: &Path, id: &str, version: &str) {
    fs::create_dir_all(dir.join("lib")).unwrap();
    let manifest = serde_json::json!({
        "id": id, "version": version, "protocol_version": 1, "name": "Fixture",
        "executable": "run",
        "actions": [{"id": "go", "name": "Go", "controls": ["key"]}]
    });
    fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
    fs::write(dir.join("run"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(dir.join("run"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.join("lib/helper.py"), "x\n").unwrap();
}

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const URL: &str = "https://example.invalid/team/plugin.git";

/// A runner whose "clone" is the given closure writing into `dest`.
struct Fake<F: Fn(&Path)>(F, String);

impl<F: Fn(&Path)> GitRunner for Fake<F> {
    fn clone_repo(&self, _: &str, dest: &Path, _: &CancelToken) -> Result<(), StageError> {
        fs::create_dir(dest).unwrap();
        (self.0)(dest);
        Ok(())
    }

    fn head_commit(&self, _: &Path, _: &CancelToken) -> Result<String, StageError> {
        Ok(self.1.clone())
    }
}

fn stage_fake(scratch: &Scratch, clone: impl Fn(&Path)) -> Result<StagedPlugin, StageError> {
    stage_with(
        &PluginSource::Repository(URL.into()),
        &scratch.options(),
        &CancelToken::new(),
        &Fake(clone, HASH.into()),
    )
}

#[test]
fn the_git_directory_is_removed_and_never_copied() {
    let scratch = Scratch::new();
    let staged = stage_fake(&scratch, |dest| {
        write_project(dest, "cloned", "1.0.0");
        fs::create_dir_all(dest.join(".git/objects")).unwrap();
        fs::write(dest.join(".git/objects/pack"), "x").unwrap();
    })
    .unwrap();
    assert_eq!(staged.preview().commit.as_deref(), Some(HASH));
    assert_eq!(staged.preview().file_count, 3);
    assert!(!staged.directory().join("clone").exists());
    assert!(!staged.files().join(".git").exists());
}

#[test]
fn a_git_entry_that_is_not_a_plain_directory_is_rejected() {
    let scratch = Scratch::new();
    let error = stage_fake(&scratch, |dest| {
        write_project(dest, "cloned", "1.0.0");
        symlink("/tmp", dest.join(".git")).unwrap();
    })
    .unwrap_err();
    assert!(matches!(error, StageError::Invalid(_)), "{error:?}");
    scratch.nothing_staged();
}

#[test]
fn a_fake_clone_with_a_symlink_or_too_many_files_is_rejected() {
    let scratch = Scratch::new();
    let error = stage_fake(&scratch, |dest| {
        write_project(dest, "cloned", "1.0.0");
        symlink("run", dest.join("lib/link")).unwrap();
    })
    .unwrap_err();
    assert!(error.to_string().contains("lib/link"), "{error}");
    let error = stage_fake(&scratch, |dest| {
        write_project(dest, "cloned", "1.0.0");
        for n in 0..300 {
            fs::write(dest.join(format!("lib/f{n}")), "x").unwrap();
        }
    })
    .unwrap_err();
    assert!(error.to_string().contains("256 files"), "{error}");
    scratch.nothing_staged();
}

#[test]
fn only_full_hex_object_names_are_commits() {
    assert!(valid_commit(HASH));
    assert!(valid_commit(&"AbCdEf0123456789".repeat(4)));
    for bad in [
        "",
        "abc",
        &HASH[..39],
        &format!("{HASH}0"),
        &"z".repeat(40),
        "-x",
    ] {
        assert!(!valid_commit(bad), "{bad:?}");
    }
}

#[test]
fn the_file_protocol_is_off_unless_the_test_only_switch_is_set() {
    let plain = SystemGit::new("git".as_ref(), Duration::from_secs(1), 1);
    let args = plain.clone_args(URL, Path::new("/x/clone"));
    assert!(!args
        .iter()
        .any(|arg| arg.to_string_lossy().contains("protocol.file")));
    assert!(args.iter().any(|arg| arg == "protocol.allow=never"));
    let mut test_only = SystemGit::new("git".as_ref(), Duration::from_secs(1), 1);
    test_only.allow_file = true;
    assert!(test_only
        .clone_args(URL, Path::new("/x/clone"))
        .iter()
        .any(|arg| arg == "protocol.file.allow=always"));
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git(repo: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgsign=false"])
        .arg("-C")
        .arg(repo)
        .args(args);
    command.env_clear();
    command
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    let output = command.output().unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// The hardened system git against a LOCAL `file://` repository (never the network). The argv is
/// the production one plus the test-only `protocol.file.allow=always`.
#[test]
fn real_git_clones_a_local_repository_and_records_the_commit() {
    if !git_available() {
        eprintln!("skipped: git is not installed");
        return;
    }
    let scratch = Scratch::new();
    let repo = scratch.0.join("repo");
    write_project(&repo, "from-git", "3.1.4");
    symlink("run", repo.join("lib/link")).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let expected = git(&repo, &["rev-parse", "HEAD"]);
    let mut system = SystemGit::new("git".as_ref(), Duration::from_secs(60), 32 << 20);
    system.allow_file = true;
    let staged = stage_with(
        &PluginSource::Repository(format!("file://{}", repo.display())),
        &scratch.options(),
        &CancelToken::new(),
        &system,
    )
    .unwrap();
    assert_eq!(staged.preview().commit.as_deref(), Some(expected.as_str()));
    assert_eq!(staged.preview().id, "from-git");
    assert!(!staged.files().join(".git").exists());
    assert!(!staged.directory().join("clone").exists());
    // `core.symlinks=false` makes git check a symlink out as a plain text file, so a cloned
    // link can never point outside the plugin; the copy rules still reject real symlinks.
    let meta = fs::symlink_metadata(staged.files().join("lib/link")).unwrap();
    assert!(meta.file_type().is_file());
    drop(staged);
    scratch.nothing_staged();
}

#[test]
fn a_forced_failure_mid_swap_restores_the_old_plugin_and_its_enabled_state() {
    let scratch = Scratch::new();
    let root = scratch.0.join("plugins");
    let source = |path: &Path| PluginSource::Folder(fs::canonicalize(path).unwrap());
    let one = scratch.0.join("v1");
    write_project(&one, "hello", "1.0.0");
    let first = super::stage(&source(&one), &scratch.options(), &CancelToken::new()).unwrap();
    install(&first, &root, InstallMode::New).unwrap();
    let mut registry = PluginRegistry::open(&root);
    registry.set_enabled("hello", true).unwrap();
    let two = scratch.0.join("v2");
    write_project(&two, "hello", "2.0.0");
    let second = super::stage(&source(&two), &scratch.options(), &CancelToken::new()).unwrap();

    let error = install_with(&second, &root, InstallMode::Replace, &|step| {
        assert_eq!(step, Step::BackedUp);
        assert!(
            !root.join("hello").exists(),
            "the swap is between two renames"
        );
        Err(std::io::Error::other("forced failure"))
    })
    .unwrap_err();
    let InstallError::Io(message) = error else {
        panic!("{error:?}")
    };
    assert!(
        message.contains("forced failure") && message.contains("restored"),
        "{message}"
    );
    let after = PluginRegistry::open(&root);
    assert_eq!(after.plugin("hello").unwrap().version, "1.0.0");
    assert!(after.is_enabled("hello"), "the old enabled choice is back");
    assert!(after.problems().is_empty());
    let hidden: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(hidden.is_empty(), "{hidden:?}");
}
