//! Installing a staged plugin under the plugins folder: copy to a hidden sibling, then rename.
//! The registry ignores names starting with `.`, so the in-progress and backup directories are
//! never mistaken for plugins, and the executable path never points at a half-written folder.

use super::copy::copy_project;
use super::stage::StagedPlugin;
use super::CancelToken;
use crate::plugins::{PluginManifest, PluginRegistry, MAX_MANIFEST_BYTES};
use std::fmt;
use std::fs;
use std::io;
use std::io::Read;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

/// What to do when `<plugins>/<id>` already exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallMode {
    /// Refuse with [`InstallError::AlreadyInstalled`].
    New,
    /// Swap the installed plugin for the staged one and disable it until enabled again.
    Replace,
}

/// The outcome of a successful install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub version: String,
    pub destination: PathBuf,
    /// An installed plugin was swapped out.
    pub replaced: bool,
    /// The replaced plugin was enabled and has been removed from the enabled list.
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    /// The plugin is installed and the mode was [`InstallMode::New`]; a UI can offer Replace.
    AlreadyInstalled {
        id: String,
        installed_version: Option<String>,
    },
    /// `plugins.json` is invalid: nothing was touched.
    InvalidState(String),
    /// A filesystem step failed; the message says whether the previous plugin was restored.
    Io(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInstalled {
                id,
                installed_version: Some(version),
            } => write!(f, "plugin `{id}` is already installed (version {version})"),
            Self::AlreadyInstalled { id, .. } => write!(f, "plugin `{id}` is already installed"),
            Self::InvalidState(message) => write!(f, "invalid plugin state: {message}"),
            Self::Io(message) => write!(f, "cannot install the plugin: {message}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// A point of [`install_with`] where tests inject a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The old directory has been moved aside; the new one is not in place yet.
    BackedUp,
}

/// Installs a staged plugin as `<plugins_root>/<id>` and never enables it.
///
/// The folder is created owner-only when missing. With [`InstallMode::Replace`] the installed
/// plugin is first removed from the enabled list (an invalid `plugins.json` aborts before
/// anything is touched), moved to a hidden backup, replaced by one rename, and the backup deleted
/// only after success; any failure restores the old plugin. A request that is already running
/// keeps the old process. Synchronous: call it from a worker thread.
pub fn install(
    staged: &StagedPlugin,
    plugins_root: &Path,
    mode: InstallMode,
) -> Result<Installed, InstallError> {
    install_with(staged, plugins_root, mode, &|_| Ok(()))
}

pub(super) fn install_with(
    staged: &StagedPlugin,
    root: &Path,
    mode: InstallMode,
    inject: &dyn Fn(Step) -> io::Result<()>,
) -> Result<Installed, InstallError> {
    let manifest = staged.manifest();
    let id = manifest.id.as_str();
    let io_err = |error: io::Error| InstallError::Io(error.to_string());
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)
        .map_err(io_err)?;
    let destination = root.join(id);
    let exists = match fs::symlink_metadata(&destination) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(io_err(error)),
    };
    if exists && mode == InstallMode::New {
        return Err(already_installed(id, &destination));
    }
    let replacing = exists;
    let mut registry = None;
    let mut was_enabled = false;
    if replacing {
        let opened = PluginRegistry::open(root);
        if let Some(error) = opened.state_error() {
            return Err(InstallError::InvalidState(error.to_string()));
        }
        was_enabled = opened.is_enabled(id);
        registry = Some(opened);
    }
    let fresh = create_sibling(root, id, "new").map_err(io_err)?;
    let discard = |path: &Path| {
        let _ = fs::remove_dir_all(path);
    };
    if let Err(error) = copy_project(&staged.files(), &fresh, &CancelToken::new()) {
        discard(&fresh);
        return Err(InstallError::Io(error.to_string()));
    }
    let installed = |replaced: bool| Installed {
        id: id.to_owned(),
        version: manifest.version.clone(),
        destination: destination.clone(),
        replaced,
        disabled: replaced && was_enabled,
    };
    if !replacing {
        return match fs::rename(&fresh, &destination) {
            Ok(()) => Ok(installed(false)),
            Err(error) => {
                discard(&fresh);
                if destination.exists() {
                    Err(already_installed(id, &destination))
                } else {
                    Err(io_err(error))
                }
            }
        };
    }
    // New code must be enabled explicitly, so the old choice is dropped before the swap.
    if was_enabled {
        let registry = registry.as_mut().expect("opened above");
        if let Err(error) = registry.set_enabled(id, false) {
            discard(&fresh);
            return Err(InstallError::InvalidState(error.to_string()));
        }
    }
    let backup = unused_name(root, id, "old");
    let swap = fs::rename(&destination, &backup)
        .and_then(|()| inject(Step::BackedUp))
        .and_then(|()| fs::rename(&fresh, &destination));
    if let Err(error) = swap {
        discard(&fresh);
        let restored = match fs::symlink_metadata(&backup) {
            Ok(_) => fs::remove_dir_all(&destination)
                .or_else(|error| {
                    if error.kind() == io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(error)
                    }
                })
                .and_then(|()| fs::rename(&backup, &destination)),
            Err(_) => Ok(()),
        };
        let mut message = error.to_string();
        match restored {
            Ok(()) => {
                message.push_str("; the previous plugin was restored");
                if was_enabled {
                    let reenabled = registry
                        .as_mut()
                        .is_some_and(|registry| registry.set_enabled(id, true).is_ok());
                    if !reenabled {
                        message.push_str(" but stays disabled");
                    }
                }
            }
            Err(restore) => message.push_str(&format!(
                "; restoring failed ({restore}), the previous plugin is kept at {}",
                backup.display()
            )),
        }
        return Err(InstallError::Io(message));
    }
    // Best effort: a leftover hidden backup is harmless and ignored by the registry.
    let _ = fs::remove_dir_all(&backup);
    Ok(installed(true))
}

fn already_installed(id: &str, destination: &Path) -> InstallError {
    InstallError::AlreadyInstalled {
        id: id.to_owned(),
        installed_version: installed_version(destination),
    }
}

fn installed_version(destination: &Path) -> Option<String> {
    let path = destination.join("manifest.json");
    if !fs::symlink_metadata(&path).ok()?.file_type().is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .ok()?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    PluginManifest::parse(&bytes).ok().map(|m| m.version)
}

static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn candidate(root: &Path, id: &str, role: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    root.join(format!(".{id}.{role}-{}-{n}", std::process::id()))
}

/// Creates a fresh hidden directory beside the destination; leftovers of other runs are skipped.
fn create_sibling(root: &Path, id: &str, role: &str) -> io::Result<PathBuf> {
    for _ in 0..64 {
        let path = candidate(root, id, role);
        match fs::DirBuilder::new().mode(0o755).create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("no unused temporary name"))
}

/// A hidden name nothing occupies (the rename target must not exist).
fn unused_name(root: &Path, id: &str, role: &str) -> PathBuf {
    loop {
        let path = candidate(root, id, role);
        if fs::symlink_metadata(&path).is_err() {
            return path;
        }
    }
}
