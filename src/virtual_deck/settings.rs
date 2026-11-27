//! What the app remembers about the virtual deck: `virtual-deck.json` in its
//! config directory (`$XDG_CONFIG_HOME/sdeck-plus/`, else
//! `$HOME/.config/sdeck-plus/`), for example
//! `{"port":8765,"salt":"…","iterations":200000,"stored_key":"…","server_key":"…"}`
//! (salt and keys in base64).
//!
//! The password itself is never kept: only its verifier (salt, iterations,
//! StoredKey and ServerKey; see [`Verifier`]). A file written by an older
//! version holds `password_hash` (Argon2id) instead: it is valid, keeps its
//! port, but has no verifier, so the app asks for a password again; the file
//! is rewritten only when a new password is saved. The file is validated when
//! read and replaced atomically, owner-only (0600, a new directory 0700). A
//! file that cannot be read or does not validate is kept exactly as it is:
//! the store says why and refuses every write until it is repaired or removed.
use super::{password::Verifier, DEFAULT_PORT};
use serde::{Deserialize, Serialize};
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
const INCOMPLETE: &str = "The password verifier is incomplete.";
const OLD_HASH: &str = "The password hash is not valid.";
const BOTH: &str = "The file holds both an old password hash and a password verifier.";

/// The remembered settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The port the server listens on, 1024 to 65535.
    pub port: u16,
    /// The password verifier; `None` until a password is set, and for a file
    /// that still holds an older version's Argon2id hash.
    pub verifier: Option<Verifier>,
}

impl Default for Settings {
    /// Port 8765 and no password.
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            verifier: None,
        }
    }
}

/// `virtual-deck.json` as it is written.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    salt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    iterations: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stored_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    server_key: Option<String>,
    /// An older version's Argon2id PHC string: kept, never used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    password_hash: Option<String>,
}

/// What a valid file holds.
struct Contents {
    settings: Settings,
    /// An older version's Argon2id hash, written back until a new password replaces it.
    old_hash: Option<String>,
}

impl std::fmt::Debug for Contents {
    /// Says whether an old hash is there, never what it is.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Contents")
            .field("settings", &self.settings)
            .field("old_hash", &self.old_hash.is_some())
            .finish()
    }
}

impl Contents {
    /// Port 8765 and no password.
    fn empty() -> Self {
        Self {
            settings: Settings::default(),
            old_hash: None,
        }
    }

    fn parse(bytes: &[u8]) -> Result<Self, String> {
        let file: File =
            serde_json::from_slice(bytes).map_err(|error| format!("invalid settings: {error}"))?;
        check_port(file.port)?;
        let verifier = match (file.salt, file.iterations, file.stored_key, file.server_key) {
            (None, None, None, None) => None,
            (Some(salt), Some(iterations), Some(stored), Some(server)) => {
                Some(Verifier::from_stored(&salt, iterations, &stored, &server)?)
            }
            _ => return Err(INCOMPLETE.into()),
        };
        if let Some(hash) = &file.password_hash {
            if verifier.is_some() {
                return Err(BOTH.into());
            }
            if !is_argon2id_hash(hash) {
                return Err(OLD_HASH.into());
            }
        }
        Ok(Self {
            settings: Settings {
                port: file.port,
                verifier,
            },
            old_hash: file.password_hash,
        })
    }

    fn file(&self) -> File {
        let stored = self.settings.verifier.as_ref().map(Verifier::stored);
        File {
            port: self.settings.port,
            iterations: self.settings.verifier.as_ref().map(Verifier::iterations),
            salt: stored.as_ref().map(|(salt, _, _)| salt.clone()),
            stored_key: stored.as_ref().map(|(_, key, _)| key.clone()),
            server_key: stored.map(|(_, _, key)| key),
            password_hash: self.old_hash.clone(),
        }
    }
}

/// Whether `hash` looks like the Argon2id PHC string older versions wrote:
/// `$argon2id$v=19$m=…,t=…,p=…$<salt>$<hash>`.
fn is_argon2id_hash(hash: &str) -> bool {
    let base64 = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
    };
    let parts: Vec<&str> = hash.split('$').collect();
    matches!(
        parts.as_slice(),
        ["", "argon2id", version, parameters, salt, digest]
            if version.starts_with("v=")
                && parameters.starts_with("m=")
                && base64(salt)
                && base64(digest)
    )
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
    contents: Contents,
    /// Why writes are refused: the file on disk could not be used.
    refused: Option<String>,
}

impl SettingsStore {
    /// Reads `virtual-deck.json` in `config_dir`. No file means the defaults;
    /// a file that cannot be read or validated is kept, the defaults are
    /// offered and [`Self::error`] says why every write is refused.
    pub fn open(config_dir: &Path) -> Self {
        let path = config_dir.join(FILE_NAME);
        match read(&path) {
            Ok(contents) => Self {
                contents: contents.unwrap_or_else(Contents::empty),
                path,
                refused: None,
            },
            Err(reason) => Self {
                contents: Contents::empty(),
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
        &self.contents.settings
    }

    /// Whether the file still holds an older version's Argon2id hash, which
    /// can no longer log in: a new password must be set.
    pub fn has_old_password(&self) -> bool {
        self.contents.old_hash.is_some()
    }

    /// Why writes are refused, when the file on disk is unusable.
    pub fn error(&self) -> Option<&str> {
        self.refused.as_deref()
    }

    /// Remembers `port` (1024 to 65535).
    /// An older version's password hash stays as it is.
    pub fn set_port(&mut self, port: u16) -> Result<(), String> {
        check_port(port)?;
        self.save(Contents {
            settings: Settings {
                port,
                verifier: self.contents.settings.verifier.clone(),
            },
            old_hash: self.contents.old_hash.clone(),
        })
    }

    /// Remembers the verifier of a new password (from
    /// [`build_verifier`](super::build_verifier)); an older version's
    /// password hash goes.
    pub fn set_verifier(&mut self, verifier: Verifier) -> Result<(), String> {
        self.save(Contents {
            settings: Settings {
                port: self.contents.settings.port,
                verifier: Some(verifier),
            },
            old_hash: None,
        })
    }

    /// Writes `contents` atomically, unless the file on disk is (or has
    /// become) unusable; then it is kept and writes stay refused.
    fn save(&mut self, contents: Contents) -> Result<(), String> {
        if let Some(error) = &self.refused {
            return Err(error.clone());
        }
        if let Err(reason) = read(&self.path) {
            let error = refusal(&self.path, &reason);
            self.refused = Some(error.clone());
            return Err(error);
        }
        check_port(contents.settings.port)?;
        write(&self.path, &contents.file())
            .map_err(|error| format!("Could not save {}: {error}", self.path.display()))?;
        self.contents = contents;
        Ok(())
    }
}

/// The settings in `path`, `None` when there is no file.
fn read(path: &Path) -> Result<Option<Contents>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    Contents::parse(&bytes).map(Some)
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
fn write(path: &Path, file: &File) -> io::Result<()> {
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
        serde_json::to_writer(&mut output, file)?;
        output.write_all(b"\n")?;
        output.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
