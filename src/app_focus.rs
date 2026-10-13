//! Focused application identity supplied by the desktop compositor.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusedApplication {
    pub window_class: String,
    pub executable: Option<String>,
}

const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);

/// Owns the Hyprland event subscription and its background callback thread.
///
/// Callbacks run on the observer thread and must return promptly. Startup socket
/// failures are returned directly; query and event failures arrive as `Err`.
pub struct FocusObserver {
    event_socket: Option<UnixStream>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FocusObserver {
    pub fn start(
        callback: impl Fn(Result<Option<FocusedApplication>, String>) + Send + 'static,
    ) -> io::Result<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is unset"))?;
        let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "HYPRLAND_INSTANCE_SIGNATURE is unset",
            )
        })?;
        let directory = PathBuf::from(runtime).join("hypr").join(signature);
        Self::start_at(
            &directory.join(".socket.sock"),
            &directory.join(".socket2.sock"),
            callback,
        )
    }

    fn start_at(
        request_path: &Path,
        event_path: &Path,
        callback: impl Fn(Result<Option<FocusedApplication>, String>) + Send + 'static,
    ) -> io::Result<Self> {
        // Subscribe before querying, so focus changes during the query are queued.
        let events = UnixStream::connect(event_path)?;
        let event_socket = events.try_clone()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = Arc::clone(&stopping);
        let request_path = request_path.to_owned();
        let thread = thread::Builder::new()
            .name("hyprland-focus".into())
            .spawn(move || {
                observe(events, &request_path, &worker_stopping, callback);
            })?;
        Ok(Self {
            event_socket: Some(event_socket),
            stopping,
            thread: Some(thread),
        })
    }

    /// Unblocks the event read and joins the observer. Safe to call repeatedly.
    pub fn stop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(socket) = self.event_socket.take() {
            if let Err(error) = socket.shutdown(Shutdown::Both) {
                if error.kind() != io::ErrorKind::NotConnected {
                    eprintln!("Hyprland focus shutdown: {error}");
                }
            }
        }
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                eprintln!("Hyprland focus callback thread panicked");
            }
        }
    }
}

impl Drop for FocusObserver {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(serde::Deserialize)]
struct ActiveWindow {
    address: String,
    class: String,
    pid: u32,
}

struct ExternalWindow {
    address: String,
    identity: FocusedApplication,
}

fn query_window(path: &Path) -> Result<Option<ActiveWindow>, String> {
    let query = || -> Result<Option<ActiveWindow>, Box<dyn std::error::Error>> {
        let mut socket = UnixStream::connect(path)?;
        socket.set_read_timeout(Some(REQUEST_TIMEOUT))?;
        socket.set_write_timeout(Some(REQUEST_TIMEOUT))?;
        socket.write_all(b"j/activewindow")?;
        let mut response = String::new();
        socket.read_to_string(&mut response)?;
        let value: serde_json::Value = serde_json::from_str(&response)?;
        if value.as_object().is_some_and(|object| object.is_empty()) {
            return Ok(None);
        }
        let window: ActiveWindow = serde_json::from_value(value)?;
        if window.address.is_empty() {
            Ok(None)
        } else {
            Ok(Some(window))
        }
    };
    query().map_err(|error| format!("Hyprland active window unavailable: {error}"))
}

fn same_address(left: &str, right: &str) -> bool {
    left.trim_start_matches("0x") == right.trim_start_matches("0x")
}

fn observe(
    events: UnixStream,
    request_path: &Path,
    stopping: &AtomicBool,
    callback: impl Fn(Result<Option<FocusedApplication>, String>),
) {
    let mut events = BufReader::new(events);
    let mut retained: Option<ExternalWindow> = None;
    let mut emitted = None;
    let mut closed_address: Option<String> = None;
    loop {
        if stopping.load(Ordering::Acquire) {
            break;
        }
        let update = match query_window(request_path) {
            Ok(Some(window)) if window.pid == std::process::id() => {
                if closed_address.as_deref().is_some_and(|address| {
                    retained
                        .as_ref()
                        .is_some_and(|previous| same_address(&previous.address, address))
                }) {
                    retained = None;
                }
                Ok(retained.as_ref().map(|window| window.identity.clone()))
            }
            Ok(Some(window)) => {
                let executable = std::fs::read_link(format!("/proc/{}/exe", window.pid))
                    .ok()
                    .and_then(|path| path.file_name()?.to_str().map(str::to_owned));
                let identity = FocusedApplication {
                    window_class: window.class,
                    executable,
                };
                retained = Some(ExternalWindow {
                    address: window.address,
                    identity: identity.clone(),
                });
                Ok(Some(identity))
            }
            Ok(None) => {
                retained = None;
                Ok(None)
            }
            Err(error) => {
                // Never retain a window explicitly reported closed, even if the
                // query fails. A later own-window query must not resurrect it.
                if closed_address.as_deref().is_some_and(|address| {
                    retained
                        .as_ref()
                        .is_some_and(|previous| same_address(&previous.address, address))
                }) {
                    retained = None;
                }
                Err(error)
            }
        };
        if stopping.load(Ordering::Acquire) {
            break;
        }
        if emitted.as_ref() != Some(&update) {
            callback(update.clone());
            emitted = Some(update);
        }
        closed_address = None;
        loop {
            let mut line = String::new();
            let read = events.read_line(&mut line);
            if stopping.load(Ordering::Acquire) {
                return;
            }
            match read {
                Ok(0) => {
                    callback(Err("Hyprland focus event source disconnected".into()));
                    return;
                }
                Ok(_) => {
                    let line = line.trim_end_matches(['\r', '\n']);
                    if line.starts_with("activewindowv2>>") {
                        break;
                    }
                    if let Some(address) = line.strip_prefix("closewindow>>") {
                        if retained
                            .as_ref()
                            .is_some_and(|window| same_address(&window.address, address))
                        {
                            closed_address = Some(address.to_owned());
                            break;
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    callback(Err(format!(
                        "Hyprland focus event source unavailable: {error}"
                    )));
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
