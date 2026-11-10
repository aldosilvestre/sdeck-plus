//! Strict, private record of the device model used last, shared by every model.
//!
//! The file sits next to `tray.json`. A missing file means the Stream Deck +;
//! a malformed or unknown original is never replaced, so saving reports the
//! same error as loading until the user repairs or removes the file.
// Nothing reads the preference until the session opens stores per model.
#![allow(dead_code)]

use sdeck_plus::device_model::DeviceModel;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Preference {
    model: String,
}

pub(super) fn path() -> io::Result<PathBuf> {
    crate::tray_preference::path().map(|tray| tray.with_file_name("device.json"))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn read(path: &Path) -> io::Result<&'static DeviceModel> {
    match fs::read(path) {
        Ok(contents) => {
            let preference = serde_json::from_slice::<Preference>(&contents)
                .map_err(|error| invalid(format!("invalid device preference: {error}")))?;
            DeviceModel::by_id(&preference.model).ok_or_else(|| {
                invalid(format!(
                    "unknown device model {:?} in device preference",
                    preference.model
                ))
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(DeviceModel::default_model()),
        Err(error) => Err(error),
    }
}

/// The model used last; the Stream Deck + when nothing was saved.
///
/// # Errors
/// Reports an unreadable, malformed or unknown-model file.
pub(super) fn load_last_model() -> io::Result<&'static DeviceModel> {
    load_from(&path()?)
}

/// Remember `model` for the next start without a device.
///
/// # Errors
/// Refuses to replace an unreadable, malformed or unknown-model original.
pub(super) fn save_last_model(model: &DeviceModel) -> io::Result<()> {
    save_to(&path()?, model)
}

fn load_from(path: &Path) -> io::Result<&'static DeviceModel> {
    read(path)
}

fn save_to(path: &Path, model: &DeviceModel) -> io::Result<()> {
    // A repaired original is required first, including damage done after startup.
    read(path)?;
    write(
        path,
        &Preference {
            model: model.id.into(),
        },
    )
}

fn write(path: &Path, preference: &Preference) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing config directory"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        serde_json::to_writer(&mut file, preference)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        if let Err(error) = fs::remove_file(&temporary) {
            eprintln!("Could not remove temporary device preference: {error}");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdeck_plus::device_model::{PLUS, PLUS_XL};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn scratch() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "sdeck-device-pref-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn missing_file_means_the_plus_and_save_round_trips_each_model() {
        let root = scratch();
        let path = root.join("sdeck-plus").join("device.json");
        assert_eq!(load_from(&path).unwrap().id, PLUS.id);
        save_to(&path, &PLUS_XL).unwrap();
        assert_eq!(load_from(&path).unwrap().id, PLUS_XL.id);
        assert_eq!(fs::read(&path).unwrap(), b"{\"model\":\"plus-xl\"}\n");
        save_to(&path, &PLUS).unwrap();
        assert_eq!(load_from(&path).unwrap().id, PLUS.id);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saved_file_is_owner_only_and_leaves_no_temporary_file() {
        let root = scratch();
        let path = root.join("device.json");
        save_to(&path, &PLUS_XL).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let names: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["device.json"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_unknown_and_extended_originals_are_preserved_and_block_saving() {
        for original in [
            &b"{\"model\":"[..],
            b"{\"model\":\"plus-xxl\"}",
            b"{\"model\":\"plus\",\"extra\":1}",
            b"{}",
            b"[]",
        ] {
            let root = scratch();
            let path = root.join("device.json");
            fs::write(&path, original).unwrap();
            assert!(load_from(&path).is_err());
            assert!(save_to(&path, &PLUS_XL).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
            assert!(!root.join("device.json.tmp").exists());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn a_stale_temporary_file_fails_the_save_without_changing_the_preference() {
        let root = scratch();
        let path = root.join("device.json");
        save_to(&path, &PLUS_XL).unwrap();
        fs::write(root.join("device.json.tmp"), b"stale").unwrap();
        assert!(save_to(&path, &PLUS).is_err());
        assert_eq!(load_from(&path).unwrap().id, PLUS_XL.id);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_file_sits_next_to_tray_json() {
        let tray = crate::tray_preference::path().unwrap();
        assert_eq!(path().unwrap(), tray.with_file_name("device.json"));
    }
}
