//! Running the system `git` for a shallow, hook-free, prompt-free clone of an `https://` address.
//!
//! git is started from an argv list (never a shell) with a cleared environment, in its own
//! process group, with stdin from `/dev/null`. The supervisor enforces a total deadline and a
//! size ceiling on the destination and honours the [`CancelToken`]; on every exit path the whole
//! group is killed and the child reaped.

use super::{CancelToken, StageError};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(super) const MAX_DIAGNOSTIC_BYTES: usize = 4096;
const HEAD_DEADLINE: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(25);
const SIZE_EVERY: u32 = 4;
const MAX_WALK_ENTRIES: usize = 200_000;
const PROXY_AND_CA: [&str; 6] = [
    "HTTPS_PROXY",
    "https_proxy",
    "NO_PROXY",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// What the staging code needs from git; tests inject fakes that write a tree directly.
pub(super) trait GitRunner {
    /// Clones `url` into the not-yet-existing `dest` (shallow, no submodules, no tags).
    fn clone_repo(&self, url: &str, dest: &Path, cancel: &CancelToken) -> Result<(), StageError>;
    /// The commit `HEAD` of the clone at `repo`, as 40 or 64 hex characters.
    fn head_commit(&self, repo: &Path, cancel: &CancelToken) -> Result<String, StageError>;
}

/// The hardened system git. `dest.parent()` is used as `HOME`: a private, empty directory.
pub(super) struct SystemGit {
    program: OsString,
    deadline: Duration,
    max_bytes: u64,
    /// TEST ONLY: lets a local `file://` repository be cloned. This field and the argument it
    /// adds do not exist outside `cfg(test)`, so no public API can enable the file protocol.
    #[cfg(test)]
    pub(super) allow_file: bool,
}

impl SystemGit {
    pub(super) fn new(program: &OsStr, deadline: Duration, max_bytes: u64) -> Self {
        Self {
            program: program.to_owned(),
            deadline,
            max_bytes,
            #[cfg(test)]
            allow_file: false,
        }
    }

    /// The `-c` settings shared by every invocation.
    fn hardened(&self) -> Vec<OsString> {
        let settings = [
            "protocol.allow=never",
            "protocol.https.allow=always",
            "core.hooksPath=/dev/null",
            "core.fsmonitor=false",
            "core.symlinks=false",
            "credential.helper=",
            "filter.lfs.smudge=",
            "filter.lfs.process=",
            "filter.lfs.required=false",
            "http.followRedirects=initial",
        ];
        #[cfg(test)]
        let settings = settings
            .into_iter()
            .chain(self.allow_file.then_some("protocol.file.allow=always"));
        settings
            .into_iter()
            .flat_map(|setting| [OsString::from("-c"), OsString::from(setting)])
            .collect()
    }

    pub(super) fn clone_args(&self, url: &str, dest: &Path) -> Vec<OsString> {
        let mut args = self.hardened();
        args.extend(
            [
                "clone",
                "--depth",
                "1",
                "--no-tags",
                "--single-branch",
                "--no-recurse-submodules",
                "--",
            ]
            .map(OsString::from),
        );
        args.push(url.into());
        args.push(dest.into());
        args
    }

    pub(super) fn head_args(&self, repo: &Path) -> Vec<OsString> {
        let mut args = self.hardened();
        args.push("-C".into());
        args.push(repo.into());
        args.extend(["rev-parse", "HEAD"].map(OsString::from));
        args
    }
}

/// The only variables git ever sees. `HOME` is the private staging directory so that even a
/// git that ignored `GIT_CONFIG_GLOBAL` would find no user configuration.
pub(super) fn environment(home: &Path) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = Vec::new();
    for name in ["PATH", "LANG"] {
        if let Some(value) = std::env::var_os(name) {
            env.push((name.into(), value));
        }
    }
    env.push(("HOME".into(), home.into()));
    env.push(("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()));
    env.push(("GIT_CONFIG_NOSYSTEM".into(), "1".into()));
    // Prompts are off, and an empty askpass stops git from looking for another helper.
    env.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
    env.push(("GIT_ASKPASS".into(), "".into()));
    for name in PROXY_AND_CA {
        if let Some(value) = std::env::var_os(name) {
            env.push((name.into(), value));
        }
    }
    env
}

impl GitRunner for SystemGit {
    fn clone_repo(&self, url: &str, dest: &Path, cancel: &CancelToken) -> Result<(), StageError> {
        let args = self.clone_args(url, dest);
        let watch = Watch {
            dir: dest,
            limit: self.max_bytes,
        };
        self.run(args, dest, self.deadline, Some(watch), url, cancel)
            .map(|_| ())
    }

    fn head_commit(&self, repo: &Path, cancel: &CancelToken) -> Result<String, StageError> {
        let args = self.head_args(repo);
        let stdout = self.run(
            args,
            repo,
            self.deadline.min(HEAD_DEADLINE),
            None,
            "",
            cancel,
        )?;
        let text = String::from_utf8_lossy(&stdout);
        let commit = text.trim_end();
        if valid_commit(commit) {
            Ok(commit.to_owned())
        } else {
            let shown: String = commit.chars().take(80).collect();
            Err(StageError::BadCommit(format!(
                "git reported `{shown}` instead of a 40 or 64 character hex commit"
            )))
        }
    }
}

/// A full SHA-1 (40) or SHA-256 (64) object name.
pub(super) fn valid_commit(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.chars().all(|c| c.is_ascii_hexdigit())
}

struct Watch<'a> {
    dir: &'a Path,
    limit: u64,
}

impl SystemGit {
    fn run(
        &self,
        args: Vec<OsString>,
        located: &Path,
        deadline: Duration,
        watch: Option<Watch<'_>>,
        address: &str,
        cancel: &CancelToken,
    ) -> Result<Vec<u8>, StageError> {
        let home = located.parent().unwrap_or(located);
        let mut child = Command::new(&self.program)
            .args(args)
            .env_clear()
            .envs(environment(home))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|error| {
                StageError::GitMissing(format!(
                    "cannot start `{}`: {error}",
                    self.program.to_string_lossy()
                ))
            })?;
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        let mut running = Running(child);
        let started = Instant::now();
        let mut tick = 0u32;
        let status = loop {
            if let Some(status) = running.0.try_wait().map_err(io_error)? {
                break status;
            }
            if cancel.is_cancelled() {
                return Err(StageError::Cancelled);
            }
            if started.elapsed() >= deadline {
                return Err(StageError::DeadlineExceeded(deadline));
            }
            if let Some(watch) = &watch {
                if tick.is_multiple_of(SIZE_EVERY) && tree_bytes(watch.dir) > watch.limit {
                    return Err(StageError::TooLarge { limit: watch.limit });
                }
            }
            tick = tick.wrapping_add(1);
            std::thread::sleep(POLL);
        };
        // A normal exit may still leave helpers behind holding the pipes.
        running.kill_group();
        let out = stdout.map_or_else(Vec::new, |handle| handle.join().unwrap_or_default());
        let err = stderr.map_or_else(Vec::new, |handle| handle.join().unwrap_or_default());
        if status.success() {
            return Ok(out);
        }
        Err(StageError::GitFailed {
            status: status.code(),
            diagnostics: diagnostics(&err, address),
        })
    }
}

fn io_error(error: std::io::Error) -> StageError {
    StageError::Io(error.to_string())
}

/// Kills the whole process group and reaps the child on every exit path.
struct Running(Child);

impl Running {
    fn kill_group(&self) {
        #[cfg(target_os = "linux")]
        // SAFETY: plain syscall. The child leads its own process group (pgid == pid) and a group
        // id cannot be recycled while any member, including leaked descendants, is alive.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.kill_group();
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Reads a pipe to its end, keeping only the first [`MAX_DIAGNOSTIC_BYTES`].
fn drain<R: Read + Send + 'static>(mut pipe: R) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(read) = pipe.read(&mut chunk) {
            if read == 0 {
                break;
            }
            let room = MAX_DIAGNOSTIC_BYTES.saturating_sub(kept.len());
            kept.extend_from_slice(&chunk[..read.min(room)]);
        }
        kept
    })
}

/// stderr as text, without the address (which may be sensitive) or control characters.
fn diagnostics(stderr: &[u8], address: &str) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = if address.is_empty() {
        text.into_owned()
    } else {
        text.replace(address, "<repository>")
    };
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Sum of regular file sizes below `dir` (symlinks are not followed), bounded in entries.
fn tree_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_owned()];
    let mut seen = 0usize;
    while let Some(next) = stack.pop() {
        let Ok(entries) = fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            seen += 1;
            if seen > MAX_WALK_ENTRIES {
                return u64::MAX;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(entry.path()),
                Ok(kind) if kind.is_file() => {
                    total = total.saturating_add(entry.metadata().map_or(0, |meta| meta.len()));
                }
                _ => {}
            }
        }
    }
    total
}
