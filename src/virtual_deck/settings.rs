//! What the app remembers about the virtual deck: `virtual-deck.json` in its
//! config directory (`$XDG_CONFIG_HOME/sdeck-plus/`, else
//! `$HOME/.config/sdeck-plus/`), for example `{"port":8765}`.
//!
//! Only the port is kept. The password is set for each session and held in
//! the app's memory only: nothing about it is ever written here. A file an
//! earlier build wrote may still hold a password, as an Argon2id
//! `password_hash` or as a verifier (`salt`, `iterations`, `stored_key`,
//! `server_key`): it is valid, its port is used, those fields are ignored
//! whatever they hold, and the next save drops them. The file is validated
//! when read and replaced atomically, owner-only (0600, a new directory
//! 0700). A file that cannot be read or does not validate (for example a
//! field no build ever wrote) is kept exactly as it is: the store says why
//! and refuses every write until it is repaired or removed.
use super::DEFAULT_PORT;
use serde::{de::IgnoredAny, Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// The settings file inside the config directory.
pub const FILE_NAME: &str = "virtual-deck.json";
/// The lowest port the app offers: lower ones need administrator rights.
pub const MIN_PORT: u16 = 1024;
/// The highest TCP port.
pub const MAX_PORT: u16 = 65535;
const PORT_RANGE: &str = "Use a port from 1024 to 65535.";

/// The remembered settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The port the server listens on, 1024 to 65535.
    pub port: u16,
}

impl Default for Settings {
    /// Port 8765.
    fn default() -> Self {
        Self { port: DEFAULT_PORT }
    }
}

/// `virtual-deck.json` as it is read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    port: u16,
    /// Earlier builds' password fields: accepted whatever they hold, never
    /// used, and dropped by the next save.
    #[serde(default, rename = "password_hash")]
    _password_hash: Option<IgnoredAny>,
    #[serde(default, rename = "salt")]
    _salt: Option<IgnoredAny>,
    #[serde(default, rename = "iterations")]
    _iterations: Option<IgnoredAny>,
    #[serde(default, rename = "stored_key")]
    _stored_key: Option<IgnoredAny>,
    #[serde(default, rename = "server_key")]
    _server_key: Option<IgnoredAny>,
}

/// `virtual-deck.json` as it is written: the port and nothing else.
#[derive(Serialize)]
struct Written {
    port: u16,
}

fn parse(bytes: &[u8]) -> Result<Settings, String> {
    let stored: Stored =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid settings: {error}"))?;
    check_port(stored.port)?;
    Ok(Settings { port: stored.port })
}

/// The app's config directory: `$XDG_CONFIG_HOME/sdeck-plus`, else
/// `$HOME/.config/sdeck-plus`.
pub fn config_dir() -> Result<PathBuf, String> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    config_dir_from(xdg.as_deref(), home.as_deref())
}

/// [`config_dir`] for the given `XDG_CONFIG_HOME` and `HOME`; relative paths
/// are ignored, as XDG asks.
pub fn config_dir_from(xdg: Option<&Path>, home: Option<&Path>) -> Result<PathBuf, String> {
    xdg.filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| {
            home.filter(|path| path.is_absolute())
                .map(|path| path.join(".config"))
        })
        .map(|config| config.join("sdeck-plus"))
        .ok_or_else(|| "Neither XDG_CONFIG_HOME nor HOME gives an absolute config folder.".into())
}

/// Accepts 1024 to 65535; otherwise "Use a port from 1024 to 65535.".
pub fn check_port(port: u16) -> Result<(), String> {
    if port >= MIN_PORT {
        Ok(())
    } else {
        Err(PORT_RANGE.into())
    }
}

/// The port typed in `text` (spaces around it allowed), checked like
/// [`check_port`].
pub fn parse_port(text: &str) -> Result<u16, String> {
    let port = text
        .trim()
        .parse::<u16>()
        .map_err(|_| PORT_RANGE.to_string())?;
    check_port(port)?;
    Ok(port)
}

/// The settings file of one config directory.
#[derive(Debug)]
pub struct SettingsStore {
    path: PathBuf,
    settings: Settings,
    /// Why writes are refused: the file on disk could not be used.
    refused: Option<String>,
}

impl SettingsStore {
    /// Reads `virtual-deck.json` in `config_dir`. No file means the defaults;
    /// a file that cannot be read or validated is kept, the defaults are
    /// offered and [`Self::error`] says why every write is refused. Opening
    /// never writes, not even to drop an earlier build's password fields.
    pub fn open(config_dir: &Path) -> Self {
        let path = config_dir.join(FILE_NAME);
        match read(&path) {
            Ok(settings) => Self {
                settings: settings.unwrap_or_default(),
                path,
                refused: None,
            },
            Err(reason) => Self {
                settings: Settings::default(),
                refused: Some(refusal(&path, &reason)),
                path,
            },
        }
    }

    /// Where the settings live.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The settings in effect.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Why writes are refused, when the file on disk is unusable.
    pub fn error(&self) -> Option<&str> {
        self.refused.as_deref()
    }

    /// Remembers `port` (1024 to 65535), the same one included: the file is
    /// rewritten with the port only, so an earlier build's password fields go.
    pub fn set_port(&mut self, port: u16) -> Result<(), String> {
        check_port(port)?;
        self.save(Settings { port })
    }

    /// Writes `settings` atomically, unless the file on disk is (or has
    /// become) unusable; then it is kept and writes stay refused.
    fn save(&mut self, settings: Settings) -> Result<(), String> {
        if let Some(error) = &self.refused {
            return Err(error.clone());
        }
        if let Err(reason) = read(&self.path) {
            let error = refusal(&self.path, &reason);
            self.refused = Some(error.clone());
            return Err(error);
        }
        let written = Written {
            port: settings.port,
        };
        write(&self.path, &written)
            .map_err(|error| format!("Could not save {}: {error}", self.path.display()))?;
        self.settings = settings;
        Ok(())
    }
}

/// The settings in `path`, `None` when there is no file.
fn read(path: &Path) -> Result<Option<Settings>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    parse(&bytes).map(Some)
}

fn refusal(path: &Path, reason: &str) -> String {
    format!(
        "Could not use {}: {reason} The file was kept as it is and nothing is saved until \
         it is repaired or removed.",
        path.display(),
        reason = sentence(reason)
    )
}

/// `reason` ending in a full stop.
fn sentence(reason: &str) -> String {
    let reason = reason.trim_end();
    if reason.ends_with('.') {
        reason.to_string()
    } else {
        format!("{reason}.")
    }
}

/// Write to a private temporary file beside `path`, then rename it over `path`.
fn write(path: &Path, written: &Written) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no config folder"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let temporary = parent.join(format!(
        ".{FILE_NAME}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        serde_json::to_writer(&mut output, written)?;
        output.write_all(b"\n")?;
        output.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
