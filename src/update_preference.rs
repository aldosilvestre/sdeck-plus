//! Strict, private update preferences; release results are never persisted.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Preference {
    automatic_check: bool,
}

pub(crate) fn path() -> io::Result<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    config_path(xdg.as_deref(), home.as_deref())
}

pub(crate) fn config_path(xdg: Option<&Path>, home: Option<&Path>) -> io::Result<PathBuf> {
    xdg.filter(|p| p.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| home.filter(|p| p.is_absolute()).map(|p| p.join(".config")))
        .map(|p| p.join("sdeck-plus/updates.json"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no absolute config home"))
}

pub(crate) fn load(path: &Path) -> io::Result<bool> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Preference>(&bytes)
            .map(|p| p.automatic_check)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e),
    }
}

pub(crate) fn save(path: &Path, automatic_check: bool) -> io::Result<()> {
    load(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing config directory"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        serde_json::to_writer(&mut file, &Preference { automatic_check })?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Change running behavior only after the preference is durably saved.
pub(crate) fn apply(path: &Path, current: &std::cell::Cell<bool>, enabled: bool) -> io::Result<()> {
    save(path, enabled)?;
    current.set(enabled);
    Ok(())
}
