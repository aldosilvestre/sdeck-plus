//! Asks Hyprland to float the main window at its default size, centered.
//!
//! Everything here is headless: the GTK side only calls [`spawn`] when the
//! window is mapped. Outside Hyprland, or on any IPC failure, nothing happens
//! (a debug line is printed when `SDECK_PLUS_DEBUG` is set).

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Bound for one request/response on the command socket.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
/// Total time spent waiting for Hyprland to list the freshly mapped window.
pub const LOOKUP_DEADLINE: Duration = Duration::from_secs(3);
/// Pause between two lookups while the window is not listed yet.
pub const LOOKUP_INTERVAL: Duration = Duration::from_millis(50);

/// How the app's own window is recognised among Hyprland's clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity<'a> {
    pub pid: u32,
    pub class: &'a str,
    pub title: &'a str,
}

/// A rectangle in Hyprland layout coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(serde::Deserialize)]
struct Client {
    address: String,
    #[serde(default = "yes")]
    mapped: bool,
    #[serde(default)]
    floating: bool,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    pid: u32,
    #[serde(default)]
    monitor: i64,
}

fn yes() -> bool {
    true
}

#[derive(serde::Deserialize)]
struct Monitor {
    id: i64,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    width: i32,
    height: i32,
    #[serde(default = "one")]
    scale: f64,
    /// Left, top, right, bottom space taken by bars.
    #[serde(default)]
    reserved: [i32; 4],
    #[serde(default)]
    transform: u8,
}

fn one() -> f64 {
    1.0
}

/// What `j/clients` says about our window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lookup {
    /// Not listed (yet), or not mapped.
    Missing,
    /// Already floating: the user's rules or choice win.
    AlreadyFloating,
    Tiled {
        address: String,
        monitor: i64,
    },
}

/// Command socket of the running Hyprland, or `None` outside Hyprland.
pub fn command_socket(runtime_dir: Option<&OsStr>, signature: Option<&OsStr>) -> Option<PathBuf> {
    let signature = signature.filter(|value| !value.is_empty())?;
    let runtime_dir = runtime_dir.filter(|value| !value.is_empty())?;
    Some(
        Path::new(runtime_dir)
            .join("hypr")
            .join(signature)
            .join(".socket.sock"),
    )
}

/// Finds our own window by pid, class and title; never the focused one.
pub fn find_window(clients_json: &str, identity: Identity<'_>) -> Result<Lookup, String> {
    let clients: Vec<Client> =
        serde_json::from_str(clients_json).map_err(|error| format!("bad clients list: {error}"))?;
    let mut own = clients.into_iter().filter(|client| {
        client.mapped
            && client.pid == identity.pid
            && client.class == identity.class
            && client.title == identity.title
    });
    Ok(match own.next() {
        None => Lookup::Missing,
        Some(client) if client.floating => Lookup::AlreadyFloating,
        Some(client) => Lookup::Tiled {
            address: client.address,
            monitor: client.monitor,
        },
    })
}

/// The monitor's area minus bars, in layout (scaled, rotated) pixels.
pub fn usable_area(monitors_json: &str, monitor: i64) -> Result<Area, String> {
    let monitors: Vec<Monitor> = serde_json::from_str(monitors_json)
        .map_err(|error| format!("bad monitors list: {error}"))?;
    let monitor = monitors
        .into_iter()
        .find(|candidate| candidate.id == monitor)
        .ok_or_else(|| format!("monitor {monitor} not listed"))?;
    let scale = if monitor.scale.is_finite() && monitor.scale > 0.0 {
        monitor.scale
    } else {
        1.0
    };
    let (width, height) = if monitor.transform % 2 == 1 {
        (monitor.height, monitor.width)
    } else {
        (monitor.width, monitor.height)
    };
    let [left, top, right, bottom] = monitor.reserved;
    let logical = |pixels: i32| (f64::from(pixels) / scale).round() as i32;
    let area = Area {
        x: monitor.x + left,
        y: monitor.y + top,
        width: logical(width) - left - right,
        height: logical(height) - top - bottom,
    };
    if area.width < 1 || area.height < 1 {
        return Err("monitor has no usable area".into());
    }
    Ok(area)
}

/// One `[[BATCH]]` request: float, resize to exact pixels, then center in
/// `area`. The size is clamped to the area. `centerwindow` is not used since
/// it only targets the focused window.
pub fn batch(address: &str, area: Area, width: i32, height: i32) -> String {
    let width = width.clamp(1, area.width);
    let height = height.clamp(1, area.height);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    format!(
        "[[BATCH]]/dispatch setfloating address:{address};\
         /dispatch resizewindowpixel exact {width} {height},address:{address};\
         /dispatch movewindowpixel exact {x} {y},address:{address}"
    )
}

/// True when a batch reply holds only `ok` answers.
pub fn batch_succeeded(response: &str) -> bool {
    let rest = response.trim().replace("ok", "");
    !response.trim().is_empty() && rest.is_empty()
}

fn request(socket: &Path, command: &str) -> Result<String, String> {
    let run = || -> std::io::Result<String> {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
        stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
        stream.write_all(command.as_bytes())?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    };
    run().map_err(|error| format!("Hyprland request failed: {error}"))
}

/// What [`float_window`] did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    Floated(String),
    AlreadyFloating,
    /// Never listed within the deadline.
    NotFound,
}

/// Floats our window once: polls `j/clients` until it is listed (bounded by
/// `deadline`), then dispatches one batch if it is tiled.
pub fn float_window(
    socket: &Path,
    identity: Identity<'_>,
    size: (i32, i32),
    deadline: Duration,
    interval: Duration,
) -> Result<Outcome, String> {
    let started = Instant::now();
    let (address, monitor) = loop {
        match find_window(&request(socket, "j/clients")?, identity)? {
            Lookup::AlreadyFloating => return Ok(Outcome::AlreadyFloating),
            Lookup::Tiled { address, monitor } => break (address, monitor),
            Lookup::Missing => {
                if started.elapsed() >= deadline {
                    return Ok(Outcome::NotFound);
                }
                std::thread::sleep(interval);
            }
        }
    };
    let area = usable_area(&request(socket, "j/monitors")?, monitor)?;
    let command = batch(&address, area, size.0, size.1);
    let response = request(socket, &command)?;
    if batch_succeeded(&response) {
        Ok(Outcome::Floated(command))
    } else {
        Err(format!("Hyprland rejected the batch: {}", response.trim()))
    }
}

fn debug(message: &str) {
    if std::env::var_os("SDECK_PLUS_DEBUG").is_some() {
        eprintln!("window_float: {message}");
    }
}

/// Starts the floating request on a short-lived worker thread. Returns at
/// once; does nothing outside Hyprland.
pub fn spawn(class: &'static str, title: &'static str, size: (i32, i32)) {
    let Some(socket) = command_socket(
        std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
        std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").as_deref(),
    ) else {
        debug("not running under Hyprland");
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("hyprland-float".into())
        .spawn(move || {
            let identity = Identity {
                pid: std::process::id(),
                class,
                title,
            };
            match float_window(&socket, identity, size, LOOKUP_DEADLINE, LOOKUP_INTERVAL) {
                Ok(outcome) => debug(&format!("{outcome:?}")),
                Err(error) => debug(&error),
            }
        });
    if let Err(error) = spawned {
        debug(&format!("could not start worker: {error}"));
    }
}

#[cfg(test)]
mod tests;
