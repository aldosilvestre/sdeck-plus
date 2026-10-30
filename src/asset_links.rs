//! App-owned symbolic links to the sounds and files bindings use.
//!
//! A binding that plays a sound or opens a file keeps a link inside the
//! application's data directory instead of the user's own path. The link is
//! the single place that names the real file, so a moved or deleted original
//! shows up as a dangling link that [`missing_target`] reports.
use crate::{desktop_actions::DesktopAction, features::FeatureAction, layers::Action};
use std::{
    fs, io,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

/// Directory, under the data root, holding the links.
const LINKS: &str = "links";
const MAX_STEM: usize = 48;
const MAX_EXTENSION: usize = 12;
const MAX_ATTEMPTS: u32 = 10_000;

/// Replace the user's path in `action` by a link inside `<root>/links`.
///
/// Only absolute paths of sounds and "open" targets are linked; anything else
/// is returned unchanged, as is a path that already points into the links
/// directory. A file that does not exist yet still gets a (dangling) link, so
/// the control can show that it could not be configured.
///
/// # Errors
/// Reports a failure to create the links directory or the link itself.
pub fn link_action(root: &Path, action: Action) -> io::Result<Action> {
    Ok(match action {
        Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path })) => {
            let path = link_for(root, &path)?.unwrap_or(path);
            Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path }))
        }
        Action::Feature(FeatureAction::Desktop(DesktopAction::Open { target })) => {
            let target = match link_for(root, Path::new(&target))? {
                Some(link) => link.to_string_lossy().into_owned(),
                None => target,
            };
            Action::Feature(FeatureAction::Desktop(DesktopAction::Open { target }))
        }
        other => other,
    })
}

/// The action as the user chose it, with an app link shown as its real target.
pub fn unlink_action(root: &Path, action: &Action) -> Action {
    match action {
        Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path })) => {
            Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio {
                path: real_target(root, path).unwrap_or_else(|| path.clone()),
            }))
        }
        Action::Feature(FeatureAction::Desktop(DesktopAction::Open { target })) => {
            let target = real_target(root, Path::new(target)).map_or_else(
                || target.clone(),
                |real| real.to_string_lossy().into_owned(),
            );
            Action::Feature(FeatureAction::Desktop(DesktopAction::Open { target }))
        }
        other => other.clone(),
    }
}

/// Whether `action` names a sound or file that no longer exists. A link whose
/// original is gone counts as missing. A relative path is looked up from the
/// working directory, as playback does, and "~" is not expanded, as playback
/// does not. URLs given to "Open" are not files, and a path that cannot be
/// inspected is not guessed about.
pub fn missing_target(action: &Action) -> bool {
    let path = match action {
        Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path })) => {
            path.as_path()
        }
        Action::Feature(FeatureAction::Desktop(DesktopAction::Open { target })) => {
            if target.contains("://") || target.starts_with("mailto:") {
                return false;
            }
            Path::new(target)
        }
        _ => return false,
    };
    !path.as_os_str().is_empty() && !path.try_exists().unwrap_or(true)
}

/// The real file behind one of our links, if `path` is one.
fn real_target(root: &Path, path: &Path) -> Option<PathBuf> {
    path.starts_with(root.join(LINKS))
        .then(|| fs::read_link(path).ok())
        .flatten()
}

/// The link standing for `path`, created or reused; `None` when `path` is
/// not something to link.
fn link_for(root: &Path, path: &Path) -> io::Result<Option<PathBuf>> {
    let links = root.join(LINKS);
    if !path.is_absolute() || path.starts_with(&links) {
        return Ok(None);
    }
    fs::create_dir_all(&links)?;
    fs::set_permissions(&links, fs::Permissions::from_mode(0o700))?;
    // One link per original, however often it is saved.
    for entry in fs::read_dir(&links)?.flatten() {
        let known = entry.path();
        if fs::read_link(&known).is_ok_and(|target| target == path) {
            return Ok(Some(known));
        }
    }
    let stem = safe_stem(path);
    let extension = safe_extension(path);
    for attempt in 1..=MAX_ATTEMPTS {
        let name = match &extension {
            Some(extension) => format!("{stem}-{attempt}.{extension}"),
            None => format!("{stem}-{attempt}"),
        };
        let link = links.join(name);
        // `symlink` never replaces an existing entry, even a stray regular file.
        match symlink(path, &link) {
            Ok(()) => return Ok(Some(link)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("no free link name"))
}

/// The file stem reduced to ASCII letters, digits, `-` and `_`.
fn safe_stem(path: &Path) -> String {
    let stem: String = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem.trim_matches('-');
    let stem: String = stem.chars().take(MAX_STEM).collect();
    if stem.is_empty() {
        "file".into()
    } else {
        stem
    }
}

/// The extension when it is short and alphanumeric, so players still see it.
fn safe_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= MAX_EXTENSION
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .map(str::to_owned)
}
