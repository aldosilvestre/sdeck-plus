//! Supervised one-request child process: JSON line in, bounded JSON line out.
//!
//! Process separation is not sandboxing: the child runs with the user's privileges.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use super::manifest::{Settings, PROTOCOL_VERSION};
use super::PluginError;

/// Largest accepted stdout of one invocation.
pub const MAX_STDOUT_BYTES: usize = 16 * 1024;
/// Stderr kept for diagnostics; the rest is drained and dropped.
pub const MAX_STDERR_BYTES: usize = 4096;
/// Longest accepted message/error text in a response.
pub const MAX_MESSAGE_CHARS: usize = 512;
/// Longest total time a caller may grant one invocation.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(60);

/// Request written as one JSON line on the child's stdin.
#[derive(Debug, Serialize)]
pub(super) struct Request<'a> {
    pub protocol_version: u32,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub request_id: &'a str,
    pub action: &'a str,
    pub settings: &'a Settings,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    protocol_version: u32,
    request_id: String,
    ok: bool,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Result of a successful invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeOutcome {
    pub request_id: String,
    pub message: Option<String>,
    /// Lossy, truncated stderr of the child.
    pub diagnostics: String,
}

pub(super) struct RawOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub(super) fn parse_response(
    output: RawOutput,
    request_id: &str,
) -> Result<InvokeOutcome, PluginError> {
    let diagnostics = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.success {
        return Err(PluginError::NonZeroExit {
            code: output.code,
            diagnostics,
        });
    }
    let malformed = |message: &str| PluginError::MalformedResponse(message.to_owned());
    let text = std::str::from_utf8(&output.stdout).map_err(|_| malformed("stdout is not UTF-8"))?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let line = lines.next().ok_or_else(|| malformed("empty response"))?;
    if lines.next().is_some() {
        return Err(malformed("stdout must contain exactly one JSON line"));
    }
    let response: Response = serde_json::from_str(line)
        .map_err(|error| PluginError::MalformedResponse(error.to_string()))?;
    if response.protocol_version != PROTOCOL_VERSION {
        return Err(malformed("unsupported response protocol_version"));
    }
    if response.request_id != request_id {
        return Err(PluginError::RequestIdMismatch);
    }
    for text in [&response.message, &response.error].into_iter().flatten() {
        if text.chars().count() > MAX_MESSAGE_CHARS {
            return Err(malformed("message exceeds the length limit"));
        }
    }
    if response.ok {
        Ok(InvokeOutcome {
            request_id: response.request_id,
            message: response.message,
            diagnostics,
        })
    } else {
        Err(PluginError::Reported(
            response
                .error
                .unwrap_or_else(|| "plugin reported failure".into()),
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run(_: &Path, _: &Path, _: &[u8], _: Duration) -> Result<RawOutput, PluginError> {
    Err(PluginError::Spawn(
        "plugin execution is only supported on Linux".into(),
    ))
}

/// Captures one pipe without blocking, keeping at most `keep` bytes.
#[cfg(target_os = "linux")]
struct Pipe<R> {
    reader: Option<R>,
    buf: Vec<u8>,
    keep: usize,
    overflow: bool,
}

#[cfg(target_os = "linux")]
impl<R: Read + std::os::fd::AsRawFd> Pipe<R> {
    fn new(reader: R, keep: usize) -> Result<Self, PluginError> {
        let fd = reader.as_raw_fd();
        // SAFETY: `fd` is a valid descriptor owned by `reader`, which outlives both calls.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        // SAFETY: as above; only adds O_NONBLOCK to the existing flags.
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(PluginError::Spawn("cannot configure child pipe".into()));
        }
        Ok(Self {
            reader: Some(reader),
            buf: Vec::new(),
            keep,
            overflow: false,
        })
    }

    /// Reads everything currently available; closes the pipe at EOF.
    fn drain(&mut self) -> Result<(), PluginError> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(());
        };
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    self.reader = None;
                    return Ok(());
                }
                Ok(n) => {
                    let room = self.keep.saturating_sub(self.buf.len());
                    self.buf.extend_from_slice(&chunk[..n.min(room)]);
                    self.overflow |= n > room;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(PluginError::Io(error.to_string())),
            }
        }
    }

    fn fd(&self) -> Option<i32> {
        self.reader.as_ref().map(std::os::fd::AsRawFd::as_raw_fd)
    }
}

/// Kills the whole process group and reaps the child, on every exit path.
#[cfg(target_os = "linux")]
struct Supervised {
    child: std::process::Child,
}

#[cfg(target_os = "linux")]
impl Supervised {
    fn kill_group(&self) {
        // SAFETY: plain syscall. The child leads its own process group (pgid == pid) and a group
        // id cannot be recycled while any member, including leaked descendants, is alive.
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
    }
}

#[cfg(target_os = "linux")]
impl Drop for Supervised {
    fn drop(&mut self) {
        self.kill_group();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(target_os = "linux")]
fn wait_readable(fds: &[i32], wait: Duration) {
    let millis = wait.as_millis().clamp(1, 25) as i32;
    if fds.is_empty() {
        std::thread::sleep(Duration::from_millis(millis as u64));
        return;
    }
    let mut polls: Vec<libc::pollfd> = fds
        .iter()
        .map(|&fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    // SAFETY: `polls` is a valid, exclusively borrowed array of `polls.len()` pollfd entries.
    unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, millis) };
}

#[cfg(target_os = "linux")]
pub(super) fn run(
    executable: &Path,
    dir: &Path,
    request: &[u8],
    timeout: Duration,
) -> Result<RawOutput, PluginError> {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::Instant;

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
    let deadline = Instant::now() + timeout;
    let mut command = Command::new(executable);
    command
        .current_dir(dir)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    for key in FORWARDED_ENV {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut supervised = Supervised {
        child: command
            .spawn()
            .map_err(|error| PluginError::Spawn(error.to_string()))?,
    };
    // The request fits the pipe buffer, so this never blocks; a child that ignores stdin is fine.
    if let Some(mut stdin) = supervised.child.stdin.take() {
        if let Err(error) = stdin.write_all(request) {
            if error.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(PluginError::Io(error.to_string()));
            }
        }
    }
    let stdout = supervised
        .child
        .stdout
        .take()
        .ok_or_else(|| PluginError::Spawn("no stdout".into()))?;
    let stderr = supervised
        .child
        .stderr
        .take()
        .ok_or_else(|| PluginError::Spawn("no stderr".into()))?;
    let mut out = Pipe::new(stdout, MAX_STDOUT_BYTES)?;
    let mut err = Pipe::new(stderr, MAX_STDERR_BYTES)?;
    loop {
        let exited = supervised
            .child
            .try_wait()
            .map_err(|error| PluginError::Io(error.to_string()))?;
        if let Some(status) = exited {
            // Descendants may still hold the pipes: kill them, then take what is buffered.
            supervised.kill_group();
            out.drain()?;
            err.drain()?;
            if out.overflow {
                return Err(PluginError::OutputTooLarge);
            }
            return Ok(RawOutput {
                success: status.success(),
                code: status.code(),
                stdout: out.buf,
                stderr: err.buf,
            });
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(PluginError::Timeout);
        }
        let open: Vec<i32> = [out.fd(), err.fd()].into_iter().flatten().collect();
        wait_readable(&open, remaining);
        out.drain()?;
        err.drain()?;
        if out.overflow {
            return Err(PluginError::OutputTooLarge);
        }
    }
}
