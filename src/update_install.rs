//! Explicit installer execution. No GTK object crosses the worker boundary.
use std::{
    cell::Cell,
    ffi::{CString, OsString},
    fmt, fs,
    io::{self, Read},
    os::{
        fd::OwnedFd,
        unix::{
            net::UnixStream,
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Installer,
    Package,
    Other,
}
/// Inputs are the canonical executable and the absolute HOME, not launch argv.
pub(crate) fn origin(exe: Option<&Path>, home: Option<&Path>) -> Origin {
    if exe.is_some_and(|p| p.is_absolute() && p.starts_with("/usr")) {
        return Origin::Package;
    }
    match (exe, home) {
        (Some(exe), Some(home))
            if home.is_absolute() && exe == home.join(".local/bin/sdeck-plus") =>
        {
            Origin::Installer
        }
        _ => Origin::Other,
    }
}
pub(crate) fn current_origin() -> Origin {
    let exe = std::env::current_exe().and_then(fs::canonicalize).ok();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    origin(exe.as_deref(), home.as_deref())
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum InstallState {
    #[default]
    Idle,
    Running,
    Succeeded(String),
    Failed(String),
}
pub(crate) fn can_begin(
    check: &crate::update_check::UpdateState,
    install: &InstallState,
    origin: Origin,
) -> bool {
    matches!(check, crate::update_check::UpdateState::Available(_))
        && origin == Origin::Installer
        && matches!(install, InstallState::Idle | InstallState::Failed(_))
}
pub(crate) fn request_restart(state: &InstallState, flag: &Cell<bool>) -> bool {
    if matches!(state, InstallState::Succeeded(_)) {
        flag.set(true);
        true
    } else {
        false
    }
}
pub(crate) fn after_run<T>(flag: &Cell<bool>, run: impl FnOnce() -> T, spawn: impl FnOnce()) -> T {
    let result = run();
    if flag.get() {
        spawn();
    }
    result
}
pub(crate) fn spawn_replacement(path: &Path) -> io::Result<()> {
    Command::new(path)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}
pub(crate) fn clean_environment(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    vars.into_iter()
        .filter(|(key, _)| {
            matches!(
                key.to_str(),
                Some(
                    "PATH"
                        | "HOME"
                        | "LANG"
                        | "LANGUAGE"
                        | "LC_ALL"
                        | "LC_CTYPE"
                        | "RUSTUP_HOME"
                        | "XDG_CONFIG_HOME"
                        | "XDG_DATA_HOME"
                        | "XDG_CACHE_HOME"
                        | "XDG_STATE_HOME"
                        | "XDG_RUNTIME_DIR"
                )
            )
        })
        .collect()
}
#[derive(Clone, Debug)]
pub(crate) struct Spec {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) env: Vec<(OsString, OsString)>,
}
#[derive(Default)]
pub(crate) struct Tail(pub(crate) Vec<u8>);
impl Tail {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let bytes = &bytes[bytes.len().saturating_sub(4096)..];
        let discard = (self.0.len() + bytes.len()).saturating_sub(4096);
        self.0.drain(..discard);
        self.0.extend_from_slice(bytes);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Exited(i32),
    Signal(i32),
    Unknown,
}
impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exited(code) => write!(f, "exit code {code}"),
            Self::Signal(signal) => write!(f, "signal {signal}"),
            Self::Unknown => write!(f, "unknown exit status"),
        }
    }
}
pub(crate) trait Process {
    fn poll(&mut self, tail: &mut Tail) -> io::Result<Option<Outcome>>;
    fn signal(&mut self, signal: i32);
    fn group_alive(&self) -> bool;
}
pub(crate) trait Runner: Send {
    fn spawn(&mut self, spec: &Spec) -> io::Result<Box<dyn Process>>;
    fn now(&self) -> Duration;
    fn wait(&mut self, cancel: &mpsc::Receiver<()>, duration: Duration) -> bool;
}
pub(crate) struct SystemRunner(pub(crate) Instant);
struct SystemProcess {
    child: Child,
    group: i32,
    output: UnixStream,
}
fn drain(pipe: &mut impl Read, tail: &mut Tail) -> io::Result<()> {
    let mut bytes = [0; 4096];
    // A bounded amount per tick prevents a noisy child from starving cancellation.
    for _ in 0..256 {
        match pipe.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) => tail.push(&bytes[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
impl Process for SystemProcess {
    fn poll(&mut self, tail: &mut Tail) -> io::Result<Option<Outcome>> {
        drain(&mut self.output, tail)?;
        let status = self.child.try_wait()?;
        if status.is_some() {
            drain(&mut self.output, tail)?;
        }
        Ok(status.map(|s| {
            s.code()
                .map(Outcome::Exited)
                .or_else(|| s.signal().map(Outcome::Signal))
                .unwrap_or(Outcome::Unknown)
        }))
    }
    fn signal(&mut self, signal: i32) {
        // SAFETY: negative child-created PGID targets only this owned process group.
        unsafe {
            libc::kill(-self.group, signal);
        }
    }
    fn group_alive(&self) -> bool {
        // SAFETY: signal zero probes the owned process group without changing it.
        unsafe { libc::kill(-self.group, 0) == 0 }
    }
}
impl Drop for SystemProcess {
    fn drop(&mut self) {
        // Also reap on pipe/setup errors and kill any descendants retaining pipes.
        self.signal(libc::SIGKILL);
        let _ = self.child.wait();
    }
}
impl Runner for SystemRunner {
    fn spawn(&mut self, spec: &Spec) -> io::Result<Box<dyn Process>> {
        // Merge stdout/stderr at the source so the retained tail keeps write order.
        let (output, writer) = UnixStream::pair()?;
        output.set_nonblocking(true)?;
        let stderr = writer.try_clone()?;
        let child = Command::new(&spec.program)
            .args(&spec.args)
            .env_clear()
            .envs(spec.env.iter().cloned())
            .current_dir("/")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::from(OwnedFd::from(writer)))
            .stderr(Stdio::from(OwnedFd::from(stderr)))
            .spawn()?;
        let process = SystemProcess {
            group: child.id() as i32,
            child,
            output,
        };
        Ok(Box::new(process))
    }
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    fn wait(&mut self, cancel: &mpsc::Receiver<()>, duration: Duration) -> bool {
        !matches!(
            cancel.recv_timeout(duration),
            Err(mpsc::RecvTimeoutError::Timeout)
        )
    }
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> io::Result<Self> {
        // Fixed system temp root: never trust TMPDIR to put downloaded code in a checkout.
        let template = CString::new("/tmp/sdeck-plus-update-XXXXXX").map_err(io::Error::other)?;
        let mut bytes = template.into_bytes_with_nul();
        // SAFETY: mkdtemp mutates this NUL-terminated writable template, creating 0700 exclusively.
        if unsafe { libc::mkdtemp(bytes.as_mut_ptr().cast()) }.is_null() {
            return Err(io::Error::last_os_error());
        }
        bytes.pop();
        use std::os::unix::ffi::OsStringExt;
        Ok(Self(PathBuf::from(OsString::from_vec(bytes))))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_dir_all(&self.0) {
            eprintln!("Could not remove update script directory: {e}");
        }
    }
}
fn supervise(
    runner: &mut dyn Runner,
    spec: &Spec,
    cancel: &mpsc::Receiver<()>,
    deadline: Duration,
) -> Result<(), String> {
    if cancel.try_recv().is_ok() {
        return Err("Update failed: cancelled".into());
    }
    if runner.now() >= deadline {
        return Err("Update failed: timed out".into());
    }
    let mut process = runner
        .spawn(spec)
        .map_err(|e| format!("Update failed: {e}"))?;
    let mut tail = Tail::default();
    let mut stopping = None;
    loop {
        let code = match process.poll(&mut tail) {
            Ok(code) => code,
            Err(e) => {
                process.signal(libc::SIGKILL);
                return Err(format!("Update failed: {e}"));
            }
        };
        if stopping.is_none() {
            if let Some(code) = code {
                return if code == Outcome::Exited(0) {
                    Ok(())
                } else {
                    Err(format!(
                        "Update failed ({code})\n{}",
                        String::from_utf8_lossy(&tail.0)
                    ))
                };
            }
            let cancelled = runner.wait(cancel, Duration::from_millis(25));
            if cancelled || runner.now() >= deadline {
                process.signal(libc::SIGTERM);
                stopping = Some((
                    runner.now(),
                    if cancelled { "cancelled" } else { "timed out" },
                ));
            }
        } else if let Some((started, reason)) = stopping {
            if !process.group_alive() {
                return Err(format!(
                    "Update failed: {reason}\n{}",
                    String::from_utf8_lossy(&tail.0)
                ));
            }
            if runner.now().saturating_sub(started) >= Duration::from_secs(10) {
                process.signal(libc::SIGKILL);
                // The real Process destructor waits/reaps after killing the group.
                return Err(format!(
                    "Update failed: {reason}\n{}",
                    String::from_utf8_lossy(&tail.0)
                ));
            }
            runner.wait(cancel, Duration::from_millis(25));
        }
    }
}
pub(crate) fn run_update(
    runner: &mut dyn Runner,
    tag: &str,
    env: &[(OsString, OsString)],
    cancel: &mpsc::Receiver<()>,
) -> Result<(), String> {
    if crate::update_check::parse_tag(tag).is_none() {
        return Err("Update failed: invalid release tag".into());
    }
    let scratch = Scratch::new().map_err(|e| format!("Update failed: {e}"))?;
    let script = scratch.0.join("install.sh");
    let start = runner.now();
    let args = [
        "-q",
        "-fsS",
        "--proto",
        "=https",
        "--max-time",
        "30",
        "--max-filesize",
        "524288",
        "--output",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([
        script.clone().into_os_string(),
        format!("https://raw.githubusercontent.com/aldosilvestre/sdeck-plus/{tag}/install.sh")
            .into(),
    ])
    .collect();
    supervise(
        runner,
        &Spec {
            program: "/usr/bin/curl".into(),
            args,
            env: env.to_vec(),
        },
        cancel,
        start + Duration::from_secs(30),
    )?;
    let metadata = fs::symlink_metadata(&script).map_err(|e| format!("Update failed: {e}"))?;
    if !metadata.is_file() || metadata.len() > 524288 {
        return Err("Update failed: installer too large or not a regular file".into());
    }
    supervise(
        runner,
        &Spec {
            program: "/bin/bash".into(),
            args: vec![
                "--".into(),
                script.into_os_string(),
                "--tag".into(),
                tag.into(),
            ],
            env: env.to_vec(),
        },
        cancel,
        start + Duration::from_secs(30 * 60),
    )
}
pub(crate) struct Worker {
    stop: mpsc::Sender<()>,
    handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub(crate) fn start(
        tag: String,
    ) -> io::Result<(Self, async_channel::Receiver<Result<(), String>>)> {
        Self::with_runner(
            tag,
            SystemRunner(Instant::now()),
            clean_environment(std::env::vars_os()),
        )
    }
    pub(crate) fn with_runner(
        tag: String,
        mut runner: impl Runner + 'static,
        env: Vec<(OsString, OsString)>,
    ) -> io::Result<(Self, async_channel::Receiver<Result<(), String>>)> {
        let (stop, cancel) = mpsc::channel();
        let (send, receive) = async_channel::bounded(1);
        let handle = thread::Builder::new()
            .name("release-install".into())
            .spawn(move || {
                let result = run_update(&mut runner, &tag, &env, &cancel);
                let _ = send.try_send(result);
            })?;
        Ok((
            Self {
                stop,
                handle: Some(handle),
            },
            receive,
        ))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
