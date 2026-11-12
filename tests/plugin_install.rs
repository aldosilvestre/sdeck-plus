//! Public-API tests for adding a plugin: source parsing, bounded staging, validation and the
//! atomic install. Only scratch directories under the system temp dir are used. Git is never run
//! against a network: the clone tests point `StageOptions::git_program` at generated `/bin/sh`
//! scripts that record the argv and environment they were given.

use sdeck_plus::plugins::{
    install, parse_source, parse_source_in, stage, CancelToken, InstallError, InstallMode,
    PluginRegistry, PluginSource, StageError, StageOptions,
};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sdeck-install-{}-{n}", std::process::id()));
        fs::create_dir_all(path.join("tmp")).unwrap();
        fs::create_dir_all(path.join("src")).unwrap();
        Self(path)
    }

    /// Staging directories are created here (via `StageOptions::temp_dir`) so leftovers show.
    fn tmp(&self) -> PathBuf {
        self.0.join("tmp")
    }

    fn plugins(&self) -> PathBuf {
        self.0.join("plugins")
    }

    fn options(&self) -> StageOptions {
        StageOptions::new()
            .temp_dir(self.tmp())
            .plugins_root(self.plugins())
    }

    fn nothing_staged(&self) {
        let left: Vec<_> = fs::read_dir(self.tmp()).unwrap().collect();
        assert!(left.is_empty(), "staging left behind: {left:?}");
    }

    /// A valid project named `dir` (not its id) with the manifest for `id` and version `version`.
    fn project(&self, dir: &str, id: &str, version: &str) -> PathBuf {
        let path = self.0.join("src").join(dir);
        fs::create_dir_all(path.join("lib")).unwrap();
        let mut value = manifest(id);
        value["version"] = json!(version);
        fs::write(path.join("manifest.json"), value.to_string()).unwrap();
        fs::write(path.join("run"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(path.join("run"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(path.join("lib/helper.py"), "print('x')\n").unwrap();
        fs::set_permissions(
            path.join("lib/helper.py"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
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
            {"kind": "command", "name": "sh"},
            {"kind": "command", "name": "sdeck-absent-tool"},
            {"kind": "service", "name": "bluetooth", "optional": true}
        ]
    })
}

fn folder(path: &Path) -> PluginSource {
    PluginSource::Folder(fs::canonicalize(path).unwrap())
}

fn invalid(error: StageError) -> Vec<String> {
    match error {
        StageError::Invalid(problems) => problems,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

// ---- source parsing -----------------------------------------------------------------------

#[test]
fn https_addresses_are_accepted() {
    for (input, expected) in [
        (
            "https://github.com/aldo/plugin",
            "https://github.com/aldo/plugin",
        ),
        (
            "https://github.com/aldo/plugin.git",
            "https://github.com/aldo/plugin.git",
        ),
        (
            "  https://git.example.com:8443/a/b/c.git/  ",
            "https://git.example.com:8443/a/b/c.git/",
        ),
        ("HTTPS://Example.com/a_b/c~d", "https://Example.com/a_b/c~d"),
        (
            "hTTps://xn--bcher-kva.example/a",
            "https://xn--bcher-kva.example/a",
        ),
    ] {
        assert_eq!(
            parse_source(input).unwrap(),
            PluginSource::Repository(expected.into()),
            "{input}"
        );
    }
}

#[test]
fn unsafe_repository_addresses_are_rejected_with_a_message() {
    let long = format!("https://example.com/{}", "a".repeat(512));
    let rejected = [
        "https://user:pass@example.com/a/b",
        "https://user@example.com/a/b",
        "https://example.com/a b",
        "https://example.com/a\tb",
        "https://example.com/a\u{1}b",
        "https://example.com/a\nb",
        "https://exämple.com/a",
        "https://[::1]/a",
        "https://[::1]:8443/a",
        "https://example.com/a?x=1",
        "https://example.com/a#frag",
        "https://example.com",
        "https://example.com/",
        "https://example.com//",
        "https://example.com/a//b",
        "https://example.com/../a",
        "https://example.com/a/./b",
        "https://example.com/a%2eb",
        "https://example.com/a\\b",
        "https://-example.com/a",
        "https:///a",
        "https://",
        "https://exa mple.com/a",
        "https://example..com/a",
        "https://example.com:/a",
        "https://example.com:99999/a",
        "https://example.com:0/a",
        "https://example.com:80a/a",
        "https://example.com:1:2/a",
        long.as_str(),
    ];
    for input in rejected {
        let error = parse_source(input).expect_err(input).to_string();
        assert!(!error.is_empty(), "{input}");
        assert!(
            !error.contains(input),
            "message must not echo the address: {error}"
        );
    }
}

#[test]
fn other_schemes_and_option_lookalikes_say_only_https_is_accepted() {
    for input in [
        "http://example.com/a/b",
        "ssh://git@example.com/a/b",
        "git@github.com:a/b.git",
        "file:///tmp/repo",
        "git://example.com/a",
        "ext::sh -c id",
        "-c core.sshCommand=x",
        "--upload-pack=touch /tmp/x",
        "ftp://example.com/a",
        "https:example.com/a",
    ] {
        let error = parse_source(input).expect_err(input).to_string();
        assert!(
            error.contains("https") && error.contains("folder"),
            "{input}: {error}"
        );
    }
    assert!(parse_source("").is_err());
    assert!(parse_source("   ").is_err());
}

#[test]
fn folders_must_be_absolute_existing_directories() {
    let scratch = Scratch::new();
    let project = scratch.project("checkout", "hello", "1.0.0");
    let canonical = fs::canonicalize(&project).unwrap();
    assert_eq!(
        parse_source(&format!(" {} ", project.display())).unwrap(),
        PluginSource::Folder(canonical.clone())
    );
    let through_link = scratch.0.join("link");
    symlink(&project, &through_link).unwrap();
    assert_eq!(
        parse_source(through_link.to_str().unwrap()).unwrap(),
        PluginSource::Folder(canonical)
    );
    assert!(parse_source("plugins/hello")
        .unwrap_err()
        .to_string()
        .contains("absolute path"));
    assert!(parse_source("./hello").is_err());
    assert!(parse_source(scratch.0.join("missing").to_str().unwrap()).is_err());
    let file = project.join("run");
    assert!(parse_source(file.to_str().unwrap())
        .unwrap_err()
        .to_string()
        .contains("not a folder"));
}

#[test]
fn tilde_expands_with_home() {
    let scratch = Scratch::new();
    let project = scratch.project("checkout", "hello", "1.0.0");
    let previous = std::env::var_os("HOME");
    std::env::set_var("HOME", &scratch.0);
    let parsed = parse_source("~/src/checkout");
    match previous {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    assert_eq!(parsed.unwrap(), folder(&project));
}

#[test]
fn the_plugins_folder_itself_is_never_a_source() {
    let scratch = Scratch::new();
    let root = scratch.plugins();
    fs::create_dir_all(root.join("hello")).unwrap();
    let outside = scratch.project("checkout", "hello", "1.0.0");
    assert!(parse_source_in(outside.to_str().unwrap(), &root).is_ok());
    for bad in [
        root.clone(),
        root.join("hello"),
        scratch.0.clone(),
        scratch.0.join("plugins/../plugins"),
    ] {
        let error = parse_source_in(bad.to_str().unwrap(), &root).expect_err(&format!("{bad:?}"));
        assert!(error.to_string().contains("plugins folder"), "{error}");
    }
}

// ---- folder staging -----------------------------------------------------------------------

#[test]
fn a_valid_project_is_staged_and_previewed() {
    let scratch = Scratch::new();
    let project = scratch.project("some-checkout", "hello-plugin", "1.2.3");
    fs::create_dir_all(project.join(".git")).unwrap();
    fs::write(project.join(".git/HEAD"), "ref").unwrap();
    fs::create_dir_all(project.join("__pycache__")).unwrap();
    fs::write(project.join("__pycache__/x.pyc"), "x").unwrap();
    let source = folder(&project);
    let staged = stage(&source, &scratch.options(), &CancelToken::new()).unwrap();
    let preview = staged.preview();
    assert_eq!(
        (
            preview.id.as_str(),
            preview.name.as_str(),
            preview.version.as_str()
        ),
        ("hello-plugin", "Fixture", "1.2.3")
    );
    assert_eq!(preview.protocol_version, 1);
    assert_eq!(preview.source, source);
    assert_eq!(preview.commit, None);
    assert_eq!(preview.file_count, 3);
    let expected: u64 = ["manifest.json", "run", "lib/helper.py"]
        .iter()
        .map(|name| fs::metadata(project.join(name)).unwrap().len())
        .sum();
    assert_eq!(preview.total_bytes, expected);
    let action = &preview.actions[0];
    assert_eq!((action.id.as_str(), action.name.as_str()), ("send", "Send"));
    assert_eq!(action.controls.len(), 2);
    let channel = &action.settings[0];
    assert_eq!(
        (channel.id.as_str(), channel.kind.as_str(), channel.required),
        ("channel", "text", true)
    );
    assert_eq!(action.settings[1].kind, "boolean");
    let missing: Vec<_> = preview
        .requirements
        .iter()
        .map(|r| (r.requirement.name.as_str(), r.missing))
        .collect();
    assert_eq!(
        missing,
        [
            ("sh", false),
            ("sdeck-absent-tool", true),
            ("bluetooth", false)
        ]
    );
    assert!(!staged.directory().join("hello-plugin/.git").exists());
    assert!(!staged.directory().join("hello-plugin/__pycache__").exists());
    let mode = fs::metadata(staged.directory())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    let directory = staged.directory().to_owned();
    drop(staged);
    assert!(!directory.exists());
    scratch.nothing_staged();
    assert!(!scratch.plugins().exists(), "staging never installs");
}

#[test]
fn staging_names_are_unpredictable_and_unique() {
    let scratch = Scratch::new();
    let project = scratch.project("p", "hello", "1.0.0");
    let source = folder(&project);
    let a = stage(&source, &scratch.options(), &CancelToken::new()).unwrap();
    let b = stage(&source, &scratch.options(), &CancelToken::new()).unwrap();
    assert_ne!(a.directory(), b.directory());
    assert!(a
        .directory()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("sdeck-plugin-stage-"));
}

fn rejected(scratch: &Scratch, project: &Path) -> Vec<String> {
    let error = stage(&folder(project), &scratch.options(), &CancelToken::new())
        .map(|_| ())
        .unwrap_err();
    scratch.nothing_staged();
    assert!(!scratch.plugins().exists());
    invalid(error)
}

fn mentions(problems: &[String], needle: &str) -> bool {
    problems.iter().any(|problem| problem.contains(needle))
}

#[test]
fn a_bad_manifest_or_executable_is_rejected_with_every_problem() {
    let scratch = Scratch::new();
    let project = scratch.project("bad-manifest", "hello", "1.0.0");
    fs::write(project.join("manifest.json"), "{ not json").unwrap();
    assert!(mentions(&rejected(&scratch, &project), "manifest"));

    let project = scratch.project("no-manifest", "hello", "1.0.0");
    fs::remove_file(project.join("manifest.json")).unwrap();
    assert!(mentions(&rejected(&scratch, &project), "manifest.json"));

    let project = scratch.project("no-exec", "hello", "1.0.0");
    fs::remove_file(project.join("run")).unwrap();
    assert!(mentions(&rejected(&scratch, &project), "run"));

    let project = scratch.project("not-executable", "hello", "1.0.0");
    fs::set_permissions(project.join("run"), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(mentions(&rejected(&scratch, &project), "executable"));

    let project = scratch.project("bad-id", "Hello Plugin", "1.0.0");
    assert!(mentions(&rejected(&scratch, &project), "id"));
}

#[test]
fn symlinks_and_special_files_are_errors_naming_the_path() {
    let scratch = Scratch::new();
    let project = scratch.project("with-link", "hello", "1.0.0");
    symlink("/etc/passwd", project.join("lib/passwd")).unwrap();
    let problems = rejected(&scratch, &project);
    assert!(mentions(&problems, "lib/passwd"), "{problems:?}");
    assert!(mentions(&problems, "symlink"), "{problems:?}");

    let project = scratch.project("with-socket", "hello", "1.0.0");
    let _listener = std::os::unix::net::UnixListener::bind(project.join("lib/sock")).unwrap();
    let problems = rejected(&scratch, &project);
    assert!(mentions(&problems, "lib/sock"), "{problems:?}");
}

#[test]
fn limits_are_errors_and_nothing_is_copied_partially() {
    let scratch = Scratch::new();
    let project = scratch.project("many", "hello", "1.0.0");
    for n in 0..255 {
        fs::write(project.join(format!("lib/f{n}")), "x").unwrap();
    }
    assert!(mentions(&rejected(&scratch, &project), "256 files"));

    let project = scratch.project("big-file", "hello", "1.0.0");
    let big = fs::File::create(project.join("lib/big.bin")).unwrap();
    big.set_len(8 * 1024 * 1024 + 1).unwrap();
    assert!(mentions(&rejected(&scratch, &project), "lib/big.bin"));

    let project = scratch.project("big-total", "hello", "1.0.0");
    for n in 0..5 {
        let file = fs::File::create(project.join(format!("lib/part{n}.bin"))).unwrap();
        file.set_len(7 * 1024 * 1024).unwrap();
    }
    assert!(mentions(&rejected(&scratch, &project), "33554432"));

    let project = scratch.project("deep", "hello", "1.0.0");
    let mut nested = project.clone();
    for _ in 0..9 {
        nested.push("d");
    }
    fs::create_dir_all(&nested).unwrap();
    assert!(mentions(&rejected(&scratch, &project), "deeper than 8"));
}

#[test]
fn the_limits_themselves_are_accepted() {
    let scratch = Scratch::new();
    let project = scratch.project("edge", "hello", "1.0.0");
    for n in 0..252 {
        fs::write(project.join(format!("lib/f{n}")), "x").unwrap();
    }
    let mut nested = project.clone();
    for _ in 0..8 {
        nested.push("d");
    }
    fs::create_dir_all(&nested).unwrap();
    let file = fs::File::create(project.join("lib/max.bin")).unwrap();
    file.set_len(8 * 1024 * 1024).unwrap();
    let staged = stage(&folder(&project), &scratch.options(), &CancelToken::new()).unwrap();
    assert_eq!(staged.preview().file_count, 256);
}

#[test]
fn an_unreadable_file_is_an_error() {
    let scratch = Scratch::new();
    let project = scratch.project("locked", "hello", "1.0.0");
    fs::set_permissions(
        project.join("lib/helper.py"),
        fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    if fs::File::open(project.join("lib/helper.py")).is_ok() {
        eprintln!("skipped: permissions are not enforced for this user");
        return;
    }
    assert!(mentions(&rejected(&scratch, &project), "lib/helper.py"));
}

#[test]
fn a_cancelled_token_stops_a_folder_copy() {
    let scratch = Scratch::new();
    let project = scratch.project("p", "hello", "1.0.0");
    let cancel = CancelToken::new();
    cancel.cancel();
    let error = stage(&folder(&project), &scratch.options(), &cancel)
        .map(|_| ())
        .unwrap_err();
    assert_eq!(error, StageError::Cancelled);
    scratch.nothing_staged();
}

#[test]
fn staging_refuses_the_plugins_folder_as_a_source() {
    let scratch = Scratch::new();
    let project = scratch.project("p", "hello", "1.0.0");
    let root = scratch.plugins();
    fs::create_dir_all(root.join("inside")).unwrap();
    for source in [root.join("inside"), root.clone(), scratch.0.clone()] {
        let error = stage(&folder(&source), &scratch.options(), &CancelToken::new())
            .map(|_| ())
            .unwrap_err();
        assert!(matches!(error, StageError::Source(_)), "{error:?}");
    }
    assert!(stage(&folder(&project), &scratch.options(), &CancelToken::new()).is_ok());
    scratch.nothing_staged();
}

#[test]
fn a_repository_address_is_revalidated_at_stage_time() {
    let scratch = Scratch::new();
    for url in ["http://example.com/a", "https://u:p@example.com/a", "-c x"] {
        let source = PluginSource::Repository(url.into());
        let error = stage(&source, &scratch.options(), &CancelToken::new())
            .map(|_| ())
            .unwrap_err();
        assert!(matches!(error, StageError::Source(_)), "{url}: {error:?}");
    }
    scratch.nothing_staged();
}

// ---- git through a fake executable ---------------------------------------------------------

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const URL: &str = "https://example.invalid/team/plugin.git";

/// Writes an executable `/bin/sh` script. Another test thread may fork while this one still has
/// the file open for writing, which makes the first `exec` fail with ETXTBSY; so the script is
/// run once with `--sdeck-warm` (it exits immediately) until that stops happening. After that no
/// writable descriptor to it exists anywhere and the spawn under test cannot hit the race.
fn script(scratch: &Scratch, name: &str, body: &str) -> PathBuf {
    let path = scratch.0.join(name);
    fs::write(
        &path,
        format!("#!/bin/sh\n[ \"$1\" = --sdeck-warm ] && exit 0\n{body}\n"),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..200 {
        match std::process::Command::new(&path)
            .arg("--sdeck-warm")
            .status()
        {
            Err(error) if error.raw_os_error() == Some(26) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            other => {
                other.unwrap();
                break;
            }
        }
    }
    path
}

/// A fake git that records argv/environment per subcommand and "clones" `fixture`.
fn recording_git(scratch: &Scratch, fixture: &Path, head: &str) -> PathBuf {
    let out = scratch.0.join("out");
    fs::create_dir_all(&out).unwrap();
    script(
        scratch,
        "git-record",
        &format!(
            r#"OUT='{out}'
case " $* " in
*" clone "*)
  printf '%s\n' "$@" > "$OUT/clone-argv"
  tr '\0' '\n' < /proc/$$/environ > "$OUT/clone-env"
  for a; do dest=$a; done
  mkdir "$dest" && cp -R '{fixture}'/. "$dest"
  mkdir "$dest/.git" && echo ref > "$dest/.git/HEAD"
  ;;
*" rev-parse "*)
  printf '%s\n' "$@" > "$OUT/head-argv"
  tr '\0' '\n' < /proc/$$/environ > "$OUT/head-env"
  echo '{head}'
  ;;
esac"#,
            out = out.display(),
            fixture = fixture.display(),
        ),
    )
}

fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn hardened() -> Vec<String> {
    [
        "-c",
        "protocol.allow=never",
        "-c",
        "protocol.https.allow=always",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.symlinks=false",
        "-c",
        "credential.helper=",
        "-c",
        "filter.lfs.smudge=",
        "-c",
        "filter.lfs.process=",
        "-c",
        "filter.lfs.required=false",
        "-c",
        "http.followRedirects=initial",
    ]
    .map(String::from)
    .to_vec()
}

fn env_map(path: &Path) -> std::collections::BTreeMap<String, String> {
    lines(path)
        .into_iter()
        .map(|line| {
            let (key, value) = line.split_once('=').unwrap();
            (key.to_owned(), value.to_owned())
        })
        .collect()
}

#[test]
fn git_runs_with_the_exact_hardened_argv_and_a_cleared_environment() {
    let scratch = Scratch::new();
    let fixture = scratch.project("fixture", "cloned", "2.0.0");
    let git = recording_git(&scratch, &fixture, HASH);
    let options = scratch.options().git_program(&git);
    let source = PluginSource::Repository(URL.into());
    let staged = stage(&source, &options, &CancelToken::new()).unwrap();
    assert_eq!(staged.preview().commit.as_deref(), Some(HASH));
    assert_eq!(staged.preview().source, source);
    assert_eq!(staged.preview().id, "cloned");
    assert_eq!(staged.preview().file_count, 3);
    assert!(!staged.directory().join("cloned/.git").exists());

    let argv = lines(&scratch.0.join("out/clone-argv"));
    let (head, tail) = argv.split_at(argv.len() - 2);
    let mut expected = hardened();
    expected.extend(
        [
            "clone",
            "--depth",
            "1",
            "--no-tags",
            "--single-branch",
            "--no-recurse-submodules",
            "--",
        ]
        .map(String::from),
    );
    assert_eq!(head, expected);
    assert_eq!(tail[0], URL);
    let dest = Path::new(&tail[1]);
    assert_eq!(dest.file_name().unwrap(), "clone");
    let stage_dir = dest.parent().unwrap();
    assert!(stage_dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("sdeck-plugin-stage-"));
    assert_eq!(stage_dir.parent().unwrap(), scratch.tmp());

    let mut expected = hardened();
    expected.extend([
        "-C".into(),
        tail[1].clone(),
        "rev-parse".into(),
        "HEAD".into(),
    ]);
    assert_eq!(lines(&scratch.0.join("out/head-argv")), expected);

    let proxies = [
        "HTTPS_PROXY",
        "https_proxy",
        "NO_PROXY",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ];
    for file in ["clone-env", "head-env"] {
        let env = env_map(&scratch.0.join("out").join(file));
        let mut allowed: Vec<&str> = vec![
            "PATH",
            "HOME",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_NOSYSTEM",
            "GIT_TERMINAL_PROMPT",
            "GIT_ASKPASS",
        ];
        for name in proxies.iter().chain(&["LANG"]) {
            if std::env::var_os(name).is_some() {
                allowed.push(name);
            }
        }
        let mut keys: Vec<&str> = env.keys().map(String::as_str).collect();
        keys.sort_unstable();
        allowed.sort_unstable();
        // `sh` may add its own bookkeeping variables to a script it starts; /proc/<pid>/environ
        // is the initial environment, so none are expected here.
        assert_eq!(keys, allowed, "{file}");
        assert_eq!(env["GIT_CONFIG_GLOBAL"], "/dev/null");
        assert_eq!(env["GIT_CONFIG_NOSYSTEM"], "1");
        assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");
        assert_eq!(env["GIT_ASKPASS"], "");
        assert_eq!(
            Path::new(&env["HOME"]),
            stage_dir,
            "HOME is the empty private dir"
        );
        assert_ne!(Some(env["HOME"].clone().into()), std::env::var_os("HOME"));
    }
    drop(staged);
    scratch.nothing_staged();
}

#[test]
fn commits_are_only_accepted_as_40_or_64_hex_characters() {
    let scratch = Scratch::new();
    let fixture = scratch.project("fixture", "cloned", "2.0.0");
    let sha256 = "a".repeat(64);
    for ok in [HASH, sha256.as_str()] {
        let git = recording_git(&scratch, &fixture, ok);
        let staged = stage(
            &PluginSource::Repository(URL.into()),
            &scratch.options().git_program(&git),
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(staged.preview().commit.as_deref(), Some(ok));
    }
    for bad in [
        "abc123",
        &"a".repeat(41),
        &"g".repeat(40),
        "--upload-pack=x",
        "",
        &format!("{HASH} extra"),
    ] {
        let git = recording_git(&scratch, &fixture, bad);
        let error = stage(
            &PluginSource::Repository(URL.into()),
            &scratch.options().git_program(&git),
            &CancelToken::new(),
        )
        .map(|_| ())
        .unwrap_err();
        assert!(
            matches!(error, StageError::BadCommit(_)),
            "{bad}: {error:?}"
        );
    }
    scratch.nothing_staged();
}

#[test]
fn a_cloned_symlink_or_oversized_tree_is_rejected() {
    let scratch = Scratch::new();
    let fixture = scratch.project("fixture", "cloned", "2.0.0");
    symlink("/etc/passwd", fixture.join("lib/passwd")).unwrap();
    let git = recording_git(&scratch, &fixture, HASH);
    let error = stage(
        &PluginSource::Repository(URL.into()),
        &scratch.options().git_program(&git),
        &CancelToken::new(),
    )
    .map(|_| ())
    .unwrap_err();
    assert!(mentions(&invalid(error), "lib/passwd"));
    scratch.nothing_staged();

    fs::remove_file(fixture.join("lib/passwd")).unwrap();
    fs::File::create(fixture.join("lib/big"))
        .unwrap()
        .set_len(9 * 1024 * 1024)
        .unwrap();
    let error = stage(
        &PluginSource::Repository(URL.into()),
        &scratch.options().git_program(&git),
        &CancelToken::new(),
    )
    .map(|_| ())
    .unwrap_err();
    assert!(mentions(&invalid(error), "lib/big"));
    scratch.nothing_staged();
}

fn gone(pid: &str) -> bool {
    for _ in 0..150 {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => return true,
            Ok(stat) if stat.rsplit(") ").next().is_some_and(|s| s.starts_with('Z')) => {
                return true
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    false
}

fn read_pid(scratch: &Scratch) -> String {
    fs::read_to_string(scratch.0.join("child.pid"))
        .unwrap()
        .trim()
        .to_owned()
}

/// A git that starts a grandchild, records its pid, then waits for it.
fn hanging_git(scratch: &Scratch, before: &str) -> PathBuf {
    script(
        scratch,
        "git-hang",
        &format!(
            "for a; do dest=$a; done\n{before}\nsleep 30 &\necho $! > '{pid}'\nwait",
            pid = scratch.0.join("child.pid").display()
        ),
    )
}

#[test]
fn a_missing_or_failing_git_is_reported_and_leaves_nothing() {
    let scratch = Scratch::new();
    let source = PluginSource::Repository(URL.into());
    let error = stage(
        &source,
        &scratch.options().git_program(scratch.0.join("no-such-git")),
        &CancelToken::new(),
    )
    .map(|_| ())
    .unwrap_err();
    assert!(matches!(error, StageError::GitMissing(_)), "{error:?}");
    scratch.nothing_staged();

    let git = script(
        &scratch,
        "git-fail",
        &format!("echo 'fatal: repository {URL} not found' >&2\nexit 128"),
    );
    let error = stage(
        &source,
        &scratch.options().git_program(&git),
        &CancelToken::new(),
    )
    .map(|_| ())
    .unwrap_err();
    match &error {
        StageError::GitFailed {
            status,
            diagnostics,
        } => {
            assert_eq!(*status, Some(128));
            assert!(diagnostics.contains("not found"), "{diagnostics}");
            assert!(!diagnostics.contains("example.invalid"), "address redacted");
        }
        other => panic!("{other:?}"),
    }
    scratch.nothing_staged();
}

#[test]
fn git_diagnostics_are_bounded() {
    let scratch = Scratch::new();
    let git = script(
        &scratch,
        "git-noisy",
        "head -c 200000 /dev/zero | tr '\\0' 'x' >&2\nexit 1",
    );
    let error = stage(
        &PluginSource::Repository(URL.into()),
        &scratch.options().git_program(&git),
        &CancelToken::new(),
    )
    .map(|_| ())
    .unwrap_err();
    match error {
        StageError::GitFailed { diagnostics, .. } => assert!(diagnostics.len() <= 4096 + 64),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_deadline_kills_the_whole_process_group() {
    let scratch = Scratch::new();
    let git = hanging_git(&scratch, "");
    let options = scratch
        .options()
        .git_program(&git)
        .deadline(Duration::from_millis(400));
    let error = stage(
        &PluginSource::Repository(URL.into()),
        &options,
        &CancelToken::new(),
    )
    .map(|_| ())
    .unwrap_err();
    assert!(
        matches!(error, StageError::DeadlineExceeded(_)),
        "{error:?}"
    );
    assert!(gone(&read_pid(&scratch)), "grandchild survived");
    scratch.nothing_staged();
}

#[test]
fn the_size_ceiling_kills_the_clone() {
    let scratch = Scratch::new();
    let git = hanging_git(
        &scratch,
        "mkdir \"$dest\"\nhead -c 300000 /dev/zero > \"$dest/big\"",
    );
    let options = scratch.options().git_program(&git).max_clone_bytes(100_000);
    let error = stage(
        &PluginSource::Repository(URL.into()),
        &options,
        &CancelToken::new(),
    )
    .map(|_| ())
    .unwrap_err();
    assert_eq!(error, StageError::TooLarge { limit: 100_000 });
    assert!(gone(&read_pid(&scratch)), "grandchild survived");
    scratch.nothing_staged();
}

#[test]
fn cancelling_mid_clone_kills_reaps_and_cleans() {
    let scratch = Scratch::new();
    let git = hanging_git(&scratch, "");
    let options = scratch.options().git_program(&git);
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let pid_file = scratch.0.join("child.pid");
    let watcher = std::thread::spawn(move || {
        for _ in 0..500 {
            if fs::read_to_string(&pid_file).is_ok_and(|text| !text.trim().is_empty()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        trigger.cancel();
    });
    let error = stage(&PluginSource::Repository(URL.into()), &options, &cancel)
        .map(|_| ())
        .unwrap_err();
    watcher.join().unwrap();
    assert_eq!(error, StageError::Cancelled);
    assert!(gone(&read_pid(&scratch)), "grandchild survived");
    scratch.nothing_staged();
}

// ---- install ------------------------------------------------------------------------------

fn staged(
    scratch: &Scratch,
    dir: &str,
    id: &str,
    version: &str,
) -> sdeck_plus::plugins::StagedPlugin {
    let project = scratch.project(dir, id, version);
    stage(&folder(&project), &scratch.options(), &CancelToken::new()).unwrap()
}

fn hidden_leftovers(root: &Path) -> Vec<String> {
    fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect()
}

#[test]
fn install_writes_the_plugin_without_enabling_it() {
    let scratch = Scratch::new();
    let candidate = staged(&scratch, "checkout", "hello", "1.2.3");
    let root = scratch.plugins();
    let done = install(&candidate, &root, InstallMode::New).unwrap();
    assert_eq!(done.id, "hello");
    assert_eq!(done.version, "1.2.3");
    assert_eq!(done.destination, root.join("hello"));
    assert!(!done.replaced && !done.disabled);
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let mode = |path: &str| {
        fs::metadata(root.join("hello").join(path))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode("run"), 0o755);
    assert_eq!(mode("manifest.json"), 0o644);
    assert_eq!(mode("lib/helper.py"), 0o644);
    assert_eq!(mode("lib"), 0o755);
    assert_eq!(mode(""), 0o755);
    assert!(
        !root.join("plugins.json").exists(),
        "install never touches plugins.json"
    );
    let registry = PluginRegistry::open(&root);
    assert!(registry.problems().is_empty(), "{:?}", registry.problems());
    assert!(registry.plugin("hello").is_some());
    assert!(!registry.is_enabled("hello"));
    assert!(hidden_leftovers(&root).is_empty());
}

#[test]
fn installing_over_an_existing_plugin_reports_its_version() {
    let scratch = Scratch::new();
    let root = scratch.plugins();
    install(
        &staged(&scratch, "one", "hello", "1.0.0"),
        &root,
        InstallMode::New,
    )
    .unwrap();
    let error = install(
        &staged(&scratch, "two", "hello", "2.0.0"),
        &root,
        InstallMode::New,
    )
    .unwrap_err();
    assert_eq!(
        error,
        InstallError::AlreadyInstalled {
            id: "hello".into(),
            installed_version: Some("1.0.0".into())
        }
    );
    let kept = fs::read_to_string(root.join("hello/manifest.json")).unwrap();
    assert!(kept.contains("1.0.0"));
    assert!(hidden_leftovers(&root).is_empty());
}

#[test]
fn replace_swaps_the_plugin_and_requires_a_new_enable() {
    let scratch = Scratch::new();
    let root = scratch.plugins();
    install(
        &staged(&scratch, "one", "hello", "1.0.0"),
        &root,
        InstallMode::New,
    )
    .unwrap();
    install(
        &staged(&scratch, "other", "keeper", "1.0.0"),
        &root,
        InstallMode::New,
    )
    .unwrap();
    let mut registry = PluginRegistry::open(&root);
    registry.set_enabled("hello", true).unwrap();
    registry.set_enabled("keeper", true).unwrap();

    let next = staged(&scratch, "two", "hello", "2.0.0");
    fs::write(scratch.0.join("src/two/lib/new.txt"), "n").unwrap();
    let done = install(&next, &root, InstallMode::Replace).unwrap();
    assert!(done.replaced && done.disabled);
    assert_eq!(done.version, "2.0.0");
    let registry = PluginRegistry::open(&root);
    assert_eq!(registry.plugin("hello").unwrap().version, "2.0.0");
    assert!(!registry.is_enabled("hello"));
    assert!(
        registry.is_enabled("keeper"),
        "other plugins keep their state"
    );
    assert!(hidden_leftovers(&root).is_empty());
}

#[test]
fn replacing_a_disabled_plugin_reports_no_disable() {
    let scratch = Scratch::new();
    let root = scratch.plugins();
    install(
        &staged(&scratch, "one", "hello", "1.0.0"),
        &root,
        InstallMode::New,
    )
    .unwrap();
    let done = install(
        &staged(&scratch, "two", "hello", "2.0.0"),
        &root,
        InstallMode::Replace,
    )
    .unwrap();
    assert!(done.replaced && !done.disabled);
}

#[test]
fn replace_without_an_installed_plugin_is_a_plain_install() {
    let scratch = Scratch::new();
    let root = scratch.plugins();
    let done = install(
        &staged(&scratch, "one", "hello", "1.0.0"),
        &root,
        InstallMode::Replace,
    )
    .unwrap();
    assert!(!done.replaced && !done.disabled);
    assert!(root.join("hello/manifest.json").is_file());
}

#[test]
fn replace_aborts_untouched_when_plugins_json_is_invalid() {
    let scratch = Scratch::new();
    let root = scratch.plugins();
    install(
        &staged(&scratch, "one", "hello", "1.0.0"),
        &root,
        InstallMode::New,
    )
    .unwrap();
    fs::write(root.join("plugins.json"), "{ broken").unwrap();
    let error = install(
        &staged(&scratch, "two", "hello", "2.0.0"),
        &root,
        InstallMode::Replace,
    )
    .unwrap_err();
    assert!(matches!(error, InstallError::InvalidState(_)), "{error:?}");
    assert_eq!(
        fs::read_to_string(root.join("plugins.json")).unwrap(),
        "{ broken"
    );
    assert!(fs::read_to_string(root.join("hello/manifest.json"))
        .unwrap()
        .contains("1.0.0"));
    assert!(hidden_leftovers(&root).is_empty());
}

#[test]
fn hidden_temp_names_are_not_plugins_and_do_not_block_an_install() {
    let scratch = Scratch::new();
    let root = scratch.plugins();
    for name in [".hello.new-1-1", ".hello.old-1-1", ".hello.new-99999-0"] {
        fs::create_dir_all(root.join(name)).unwrap();
        fs::write(root.join(name).join("junk"), "x").unwrap();
    }
    let registry = PluginRegistry::open(&root);
    assert!(registry.problems().is_empty(), "{:?}", registry.problems());
    assert_eq!(registry.plugins().count(), 0);
    let candidate = staged(&scratch, "one", "hello", "1.0.0");
    install(&candidate, &root, InstallMode::New).unwrap();
    install(
        &staged(&scratch, "two", "hello", "2.0.0"),
        &root,
        InstallMode::Replace,
    )
    .unwrap();
    assert_eq!(
        hidden_leftovers(&root).len(),
        3,
        "foreign leftovers are never touched"
    );
    assert!(PluginRegistry::open(&root).problems().is_empty());
}
