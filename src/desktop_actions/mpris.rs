//! Direct session-bus MPRIS transport; never sends physical media keys.

use super::MediaCommand;
use glib::variant::ToVariant;
use gtk::{gio, glib};
use std::io;

const SERVICE_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const CALL_TIMEOUT_MS: i32 = 3000;

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
    let reply = bus.call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "ListNames",
        None,
        CALL_TIMEOUT_MS,
    )?;
    let (mut players,) = reply.get::<(Vec<String>,)>().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Invalid D-Bus ListNames reply")
    })?;
    players.retain(|name| name.starts_with(SERVICE_PREFIX) && name.len() > SERVICE_PREFIX.len());
    players.sort();
    let first = players.first().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "No MPRIS media player is running")
    })?;
    let mut selected = first.as_str();
    let parameters = (PLAYER_INTERFACE, "PlaybackStatus").to_variant();
    for player in &players {
        let reply = bus.call(
            player,
            PLAYER_PATH,
            "org.freedesktop.DBus.Properties",
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
