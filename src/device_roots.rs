//! Where each device model keeps its configuration.
//!
//! The Stream Deck + keeps the data directory it has always used. Every other
//! model owns `<data>/devices/<model id>/`, a complete independent copy of the
//! stores, so two models never share a file. Nothing is created here: a root
//! appears only when its stores are first opened.

use crate::device_model::DeviceModel;
use std::{
    io,
    path::{Component, Path, PathBuf},
};

/// Directory, inside the Stream Deck + root, that holds the other models' roots.
pub const DEVICES_DIR: &str = "devices";

/// The application data directory, which is also the Stream Deck + root.
///
/// # Errors
/// Fails when neither `XDG_DATA_HOME` nor `HOME` names a data location.
pub fn data_dir() -> io::Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no app data directory"))?;
    Ok(base.join("sdeck-plus"))
}

/// Storage root of `model` under the real application data directory.
///
/// # Errors
/// Fails when there is no data directory.
pub fn data_root(model: &DeviceModel) -> io::Result<PathBuf> {
    data_dir().map(|dir| data_root_in(&dir, model))
}

/// Storage root of `model` when the Stream Deck + root is `plus_root`.
pub fn data_root_in(plus_root: &Path, model: &DeviceModel) -> PathBuf {
    if model.id == DeviceModel::default_model().id {
        plus_root.to_path_buf()
    } else {
        plus_root.join(DEVICES_DIR).join(model.id)
    }
}

/// The model whose private root is exactly `root`, if it is one.
///
/// `root` is compared both as written (with `.` and `..` folded away) and with
/// every existing symlink resolved, so an alias of `devices/<id>` is still that
/// model's root.
pub fn owner_of_root(root: &Path) -> Option<&'static DeviceModel> {
    owner_of_resolved(&fold(root)).or_else(|| owner_of_resolved(&resolve(root)))
}

fn owner_of_resolved(root: &Path) -> Option<&'static DeviceModel> {
    let id = root.file_name()?.to_str()?;
    let parent = root.parent()?.file_name()?;
    if parent != DEVICES_DIR {
        return None;
    }
    DeviceModel::by_id(id).filter(|model| model.id != DeviceModel::default_model().id)
}

/// `path` without `.` and `..` components, without touching the filesystem.
fn fold(path: &Path) -> PathBuf {
    let mut folded = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !folded.pop() {
                    folded.push("..");
                }
            }
            other => folded.push(other),
        }
    }
    folded
}

/// `path` with every existing part canonicalized; a part that does not exist
/// yet keeps its folded spelling.
fn resolve(path: &Path) -> PathBuf {
    let mut head = fold(path);
    let mut tail = Vec::new();
    loop {
        if let Ok(real) = head.canonicalize() {
            return tail
                .into_iter()
                .rev()
                .fold(real, |base, part| base.join(part));
        }
        match head.file_name().map(|name| name.to_owned()) {
            Some(name) => {
                tail.push(name);
                head.pop();
            }
            None => return fold(path),
        }
    }
}

/// Refuse to open the private root of one model as another model.
///
/// Opening a `devices/<id>` root with the wrong table drops that model's rows
/// and rewrites the store, so every store open calls this first.
///
/// # Errors
/// `InvalidInput` when `root` belongs to a different model.
pub fn ensure_root_fits(root: &Path, model: &DeviceModel) -> io::Result<()> {
    match owner_of_root(root) {
        Some(owner) if owner.id != model.id => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is the data folder of the {}, not the {}",
                root.display(),
                owner.name,
                model.name
            ),
        )),
        _ => Ok(()),
    }
}
