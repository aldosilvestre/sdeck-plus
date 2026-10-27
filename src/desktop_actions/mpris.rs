//! Direct session-bus MPRIS transport; never sends physical media keys.
//! Also reads what players report (status, title, artist) for live
//! information, without controlling them.

use super::MediaCommand;
use glib::variant::ToVariant;
use gtk::{gio, glib};
use std::{collections::HashMap, io};

const SERVICE_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const CALL_TIMEOUT_MS: i32 = 3000;
/// Reads share the info worker with every other source; a hung player must
/// not hold it for long.
const READ_TIMEOUT_MS: i32 = 1000;

/// An MPRIS player's `PlaybackStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

/// What one MPRIS player reports, read without controlling it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerState {
    /// Session bus name, e.g. `org.mpris.MediaPlayer2.mpv`.
    pub bus_name: String,
    pub status: PlaybackStatus,
    /// `xesam:title`; `None` when missing or blank.
    pub title: Option<String>,
    /// `xesam:artist` entries joined with `, `; `None` when missing or blank.
    pub artist: Option<String>,
    /// The player's `Identity` (e.g. `Firefox`).
    pub identity: Option<String>,
    /// The player's `DesktopEntry`: its `.desktop` file name without the
    /// suffix (e.g. `firefox`, `com.spotify.Client`).
    pub desktop_entry: Option<String>,
}

// Replace the D-Bus boundary, preserving real message encoding and decoding.
trait Bus {
    fn call(
        &mut self,
        destination: &str,
        path: &str,
        interface: &str,
        method: &str,
        parameters: Option<&glib::Variant>,
        timeout_ms: i32,
    ) -> io::Result<glib::Variant>;
}

impl Bus for gio::DBusConnection {
    fn call(
        &mut self,
        destination: &str,
        path: &str,
        interface: &str,
        method: &str,
        parameters: Option<&glib::Variant>,
        timeout_ms: i32,
    ) -> io::Result<glib::Variant> {
        self.call_sync(
            Some(destination),
            path,
            interface,
            method,
            parameters,
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            timeout_ms,
            None::<&gio::Cancellable>,
        )
        .map_err(|error| {
            io::Error::other(format!("MPRIS {destination} {interface}.{method}: {error}"))
        })
    }
}

pub(super) fn execute(command: MediaCommand) -> io::Result<()> {
    let mut bus =
        gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).map_err(|error| {
            io::Error::other(format!("Cannot connect to MPRIS session bus: {error}"))
        })?;
    execute_on(&mut bus, command)
}

/// Reads every MPRIS player on the session bus, sorted by bus name. Blocking;
/// keep it off the GTK thread.
///
/// # Errors
/// Returns session bus connection and player discovery failures. A single
/// player that fails to answer, or answers with invalid data, is left out.
pub fn read_players() -> io::Result<Vec<PlayerState>> {
    let mut bus =
        gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).map_err(|error| {
            io::Error::other(format!("Cannot connect to MPRIS session bus: {error}"))
        })?;
    read_players_on(&mut bus)
}

fn read_players_on(bus: &mut impl Bus) -> io::Result<Vec<PlayerState>> {
    let players = list_players(bus, READ_TIMEOUT_MS)?;
    Ok(players
        .into_iter()
        .filter_map(|name| read_player(bus, name).ok())
        .collect())
}

fn read_player(bus: &mut impl Bus, bus_name: String) -> io::Result<PlayerState> {
    let invalid = |what: &str| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid MPRIS {what} from {bus_name}"),
        )
    };
    let reply = bus.call(
        &bus_name,
        PLAYER_PATH,
        PROPERTIES_INTERFACE,
        "GetAll",
        Some(&(PLAYER_INTERFACE,).to_variant()),
        READ_TIMEOUT_MS,
    )?;
    let (properties,) = reply
        .get::<(HashMap<String, glib::Variant>,)>()
        .ok_or_else(|| invalid("properties"))?;
    let status = match properties
        .get("PlaybackStatus")
        .and_then(glib::Variant::get::<String>)
        .as_deref()
    {
        Some("Playing") => PlaybackStatus::Playing,
        Some("Paused") => PlaybackStatus::Paused,
        Some("Stopped") => PlaybackStatus::Stopped,
        _ => return Err(invalid("PlaybackStatus")),
    };
    let metadata = properties
        .get("Metadata")
        .and_then(glib::Variant::get::<HashMap<String, glib::Variant>>)
        .unwrap_or_default();
    let title = metadata
        .get("xesam:title")
        .and_then(glib::Variant::get::<String>)
        .and_then(non_blank);
    // The specification says a list of artists; some players send one string.
    let artist = metadata
        .get("xesam:artist")
        .and_then(|value| {
            value
                .get::<Vec<String>>()
                .or_else(|| value.get::<String>().map(|artist| vec![artist]))
        })
        .map(|artists| {
            artists
                .iter()
                .map(|artist| artist.trim())
                .filter(|artist| !artist.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .and_then(non_blank);
    let (identity, desktop_entry) = read_root(bus, &bus_name);
    Ok(PlayerState {
        bus_name,
        status,
        title,
        artist,
        identity,
        desktop_entry,
    })
}

/// `Identity` and `DesktopEntry` from the root interface, in one call; both
/// are optional, so a failure leaves them out.
fn read_root(bus: &mut impl Bus, bus_name: &str) -> (Option<String>, Option<String>) {
    let properties = bus
        .call(
            bus_name,
            PLAYER_PATH,
            PROPERTIES_INTERFACE,
            "GetAll",
            Some(&(ROOT_INTERFACE,).to_variant()),
            READ_TIMEOUT_MS,
        )
        .ok()
        .and_then(|reply| reply.get::<(HashMap<String, glib::Variant>,)>())
        .map(|(properties,)| properties)
        .unwrap_or_default();
    let text = |name: &str| {
        properties
            .get(name)
            .and_then(glib::Variant::get::<String>)
            .and_then(non_blank)
    };
    (text("Identity"), text("DesktopEntry"))
}

fn non_blank(text: String) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// MPRIS service names on the bus, sorted for a stable choice.
fn list_players(bus: &mut impl Bus, timeout_ms: i32) -> io::Result<Vec<String>> {
    let reply = bus.call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "ListNames",
        None,
        timeout_ms,
    )?;
    let (mut players,) = reply.get::<(Vec<String>,)>().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Invalid D-Bus ListNames reply")
    })?;
    players.retain(|name| name.starts_with(SERVICE_PREFIX) && name.len() > SERVICE_PREFIX.len());
    players.sort();
    Ok(players)
}

fn execute_on(bus: &mut impl Bus, command: MediaCommand) -> io::Result<()> {
    let method = match command {
        MediaCommand::PlayPause => "PlayPause",
        MediaCommand::Stop => "Stop",
        MediaCommand::Next => "Next",
        MediaCommand::Previous => "Previous",
        MediaCommand::VolumeUp | MediaCommand::VolumeDown | MediaCommand::Mute => {
            return Err(super::invalid(
                "volume and mute use PipeWire, not MPRIS transport",
            ));
        }
    };
    let players = list_players(bus, CALL_TIMEOUT_MS)?;
    let first = players.first().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "No MPRIS media player is running")
    })?;
    let mut selected = first.as_str();
    let parameters = (PLAYER_INTERFACE, "PlaybackStatus").to_variant();
    for player in &players {
        let reply = bus.call(
            player,
            PLAYER_PATH,
            PROPERTIES_INTERFACE,
            "Get",
            Some(&parameters),
            CALL_TIMEOUT_MS,
        )?;
        let status = reply
            .get::<(glib::Variant,)>()
            .and_then(|(value,)| value.get::<String>())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid MPRIS PlaybackStatus reply from {player}"),
                )
            })?;
        match status.as_str() {
            "Playing" => {
                selected = player;
                break;
            }
            "Paused" | "Stopped" => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Unknown MPRIS PlaybackStatus {status:?} from {player}"),
                ));
            }
        }
    }
    let reply = bus.call(
        selected,
        PLAYER_PATH,
        PLAYER_INTERFACE,
        method,
        None,
        CALL_TIMEOUT_MS,
    )?;
    reply.get::<()>().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid MPRIS {method} reply"),
        )
    })
}

#[cfg(test)]
#[path = "mpris_tests.rs"]
mod tests;
