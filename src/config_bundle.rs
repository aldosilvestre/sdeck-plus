//! Portable, versioned configuration snapshots. No action is ever executed.
//!
//! The JSON envelope is `{ "format": "sdeck-plus", "version": 2,
//! "files": [{ "path": "...", "data": "<standard base64>" }] }`.
//! Image entries contain only `"path": "assets/<filename>"`, without data.
//! Import resolves these names from the existing local assets directory.
//! Limits are 192 MiB encoded, 128 MiB decoded, 32 MiB per asset/media file,
//! 4 MiB per configuration file, and 4096 entries. Credentials are included;
//! bundles and installed files are owner-only, and staging directories are 0700.

use crate::{
    desktop_actions::DesktopAction,
    features::FeatureAction,
    layers::{Action, LayerStore},
    local_artwork::ArtworkStore,
    obs::ObsConfig,
    organization::Organization,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, DirBuilder, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const CONFIGS: [&str; 5] = [
    "layers.conf",
    "organization.json",
    "artwork.conf",
    "obs.json",
    "info.json",
];
const MAX_FILE: usize = 32 * 1024 * 1024;
const MAX_CONFIG: usize = 4 * 1024 * 1024;
const MAX_TOTAL: usize = 128 * 1024 * 1024;
const MAX_BUNDLE: usize = 192 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: String,
    version: u32,
    files: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<String>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
        && !name.ends_with(['.', ' '])
}

fn allowed(path: &str) -> bool {
    CONFIGS.contains(&path)
        || path
            .split_once('/')
            .is_some_and(|(dir, name)| matches!(dir, "assets" | "media") && safe_name(name))
}

fn limit(path: &str) -> usize {
    if CONFIGS.contains(&path) {
        MAX_CONFIG
    } else {
        MAX_FILE
    }
}

// Check every component, not just the leaf: a symlinked assets directory or an
// audio file reached through a symlinked parent must not enter an export.
fn no_symlinks(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        // Preserve `..` so symlinks are checked before any parent traversal.
        current.push(component);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(invalid("symlink input is not permitted"));
        }
    }
    Ok(())
}

fn read_regular(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    no_symlinks(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max as u64 {
        return Err(invalid("non-regular or oversized input file"));
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(invalid("input file exceeds size limit"));
    }
    Ok(bytes)
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn private_dir(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)
}

fn sibling(path: &Path, role: &str) -> io::Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("path needs a parent"))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("path needs a UTF-8 filename"))?;
    Ok(parent.join(format!(
        ".{name}.{role}-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )))
}

fn stage_for(path: &Path) -> io::Result<PathBuf> {
    loop {
        let stage = sibling(path, "stage")?;
        match private_dir(&stage) {
            Ok(()) => return Ok(stage),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

fn absolute_target(path: &Path) -> io::Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    no_symlinks(
        path.parent()
            .ok_or_else(|| invalid("path needs a parent"))?,
    )?;
    let parent = fs::canonicalize(path.parent().ok_or_else(|| invalid("missing parent"))?)?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid("path needs a filename"))?;
    let target = parent.join(name);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(invalid("symlink destination")),
        Ok(_) => Ok(target),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(target),
        Err(error) => Err(error),
    }
}

fn cleanup<T>(stage: &Path, result: io::Result<T>) -> io::Result<T> {
    match fs::remove_dir_all(stage) {
        Ok(()) => result,
        Err(error) if error.kind() == io::ErrorKind::NotFound => result,
        Err(error) => Err(io::Error::other(match result {
            Ok(_) => format!("staging cleanup failed at {}: {error}", stage.display()),
            Err(original) => format!(
                "{original}; staging cleanup failed at {}: {error}",
                stage.display()
            ),
        })),
    }
}

fn add(files: &mut BTreeMap<String, Vec<u8>>, path: String, bytes: Vec<u8>) -> io::Result<()> {
    if !allowed(&path) || bytes.len() > limit(&path) {
        return Err(invalid("unsafe bundle path or oversized file"));
    }
    if files.contains_key(&path) {
        return Err(invalid("duplicate bundle entry"));
    }
    if files.len() >= MAX_ENTRIES
        || files.values().map(Vec::len).sum::<usize>() + bytes.len() > MAX_TOTAL
    {
        return Err(invalid("bundle exceeds entry or decoded size limit"));
    }
    files.insert(path, bytes);
    Ok(())
}

fn materialize(stage: &Path, files: &BTreeMap<String, Vec<u8>>) -> io::Result<()> {
    private_dir(&stage.join("assets"))?;
    private_dir(&stage.join("media"))?;
    for (path, bytes) in files {
        write_private(&stage.join(path), bytes)?;
    }
    Ok(())
}

fn validate(stage: &Path) -> io::Result<LayerStore> {
    if !stage.join("layers.conf").is_file() {
        return Err(invalid("bundle requires layers.conf"));
    }
    let layers = LayerStore::open_at(stage)?;
    if !layers.diagnostics().is_empty() || !layers.conflicts().is_empty() {
        return Err(invalid("invalid or recovered layer configuration"));
    }
    Organization::open_at(stage, &layers)?;
    ArtworkStore::open_at_for_layer(stage, Some(layers.active()))?;
    ObsConfig::load_at(stage)?;
    if stage.join("info.json").exists() {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Info {
            location: String,
        }
        let info: Info =
            serde_json::from_slice(&read_regular(&stage.join("info.json"), MAX_CONFIG)?)
                .map_err(|_| invalid("invalid information settings"))?;
        let location = info.location.trim();
        if location.is_empty() || location.len() > 100 || location.chars().any(char::is_control) {
            return Err(invalid("invalid weather location"));
        }
    }
    Ok(layers)
}

fn audio_bindings(layers: &LayerStore) -> Vec<(String, crate::layers::Trigger, PathBuf)> {
    layers
        .layers()
        .iter()
        .flat_map(|(name, layer)| {
            layer.bindings.iter().filter_map(move |(trigger, action)| {
                if let Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path })) =
                    action
                {
                    Some((name.clone(), trigger.clone(), path.clone()))
                } else {
                    None
                }
            })
        })
        .collect()
}

fn bind_audio(
    layers: &mut LayerStore,
    layer: &str,
    trigger: crate::layers::Trigger,
    path: PathBuf,
) -> io::Result<()> {
    layers.bind(
        layer,
        trigger,
        Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path })),
    )
}

fn persist_layers(layers: &LayerStore) -> io::Result<()> {
    layers.persist()?;
    fs::set_permissions(
        layers.root().join("layers.conf"),
        fs::Permissions::from_mode(0o600),
    )
}

/// Export the whitelisted configuration and local audio into one JSON bundle.
///
/// All validation happens on an isolated snapshot. The destination is replaced
/// only after validation, serialization and staging cleanup have succeeded.
pub fn export(root: &Path, destination: &Path) -> io::Result<()> {
    no_symlinks(root)?;
    if !fs::metadata(root)?.is_dir() {
        return Err(invalid("configuration root is not a directory"));
    }
    let root = fs::canonicalize(root)?;
    let destination = absolute_target(destination)?;
    if destination.starts_with(&root) {
        return Err(invalid(
            "export destination must be outside the configuration directory",
        ));
    }
    let stage = stage_for(&destination)?;
    let result = (|| {
        let mut files = BTreeMap::new();
        for name in CONFIGS {
            match fs::symlink_metadata(root.join(name)) {
                Ok(_) => add(
                    &mut files,
                    name.into(),
                    read_regular(&root.join(name), limit(name))?,
                )?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        match fs::symlink_metadata(root.join("assets")) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(invalid("assets must be a regular directory"));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        // Only manifest-owned artwork is copied, never stale assets, logs or
        // other files that happen to live inside the application directory.
        if let Some(manifest) = files.get("artwork.conf") {
            let manifest = std::str::from_utf8(manifest)
                .map_err(|_| invalid("artwork manifest is not UTF-8"))?;
            let names: BTreeSet<_> = manifest
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let fields: Vec<_> = line.split('\t').collect();
                    match fields.as_slice() {
                        [_, name] | [_, _, name] => Some((*name).to_owned()),
                        _ => None,
                    }
                })
                .collect();
            for name in names {
                if !safe_name(&name) {
                    return Err(invalid("unsafe asset filename"));
                }
                let bytes = read_regular(&root.join("assets").join(&name), MAX_FILE)?;
                add(&mut files, format!("assets/{name}"), bytes)?;
            }
        }
        materialize(&stage, &files)?;
        let mut layers = validate(&stage)?;
        for (layer, trigger, path) in audio_bindings(&layers) {
            let source = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()?.join(&path)
            };
            if fs::canonicalize(&source)? == destination {
                return Err(invalid(
                    "export destination is a sound file used by a binding",
                ));
            }
            let bytes = read_regular(&source, MAX_FILE)?;
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .filter(|ext| {
                    !ext.is_empty()
                        && ext.len() <= 16
                        && ext.bytes().all(|b| b.is_ascii_alphanumeric())
                })
                .unwrap_or("bin");
            let portable = format!(
                "media/{:x}.{}",
                Sha256::digest(&bytes),
                extension.to_ascii_lowercase()
            );
            if !files.contains_key(&portable) {
                add(&mut files, portable.clone(), bytes.clone())?;
                write_private(&stage.join(&portable), &bytes)?;
            }
            bind_audio(&mut layers, &layer, trigger, portable.into())?;
        }
        persist_layers(&layers)?;
        // Strict artwork loading may migrate a legacy manifest in the snapshot.
        for name in ["layers.conf", "artwork.conf"] {
            if stage.join(name).exists() {
                files.remove(name);
                add(
                    &mut files,
                    name.into(),
                    read_regular(&stage.join(name), MAX_CONFIG)?,
                )?;
            }
        }
        validate_portable_audio(&layers, &stage)?;
        let bundle = Bundle {
            format: "sdeck-plus".into(),
            version: 2,
            files: files
                .into_iter()
                .map(|(path, bytes)| {
                    let data = (!path.starts_with("assets/")).then(|| STANDARD.encode(bytes));
                    Entry { path, data }
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&bundle).map_err(io::Error::other)?;
        if bytes.len() > MAX_BUNDLE {
            return Err(invalid("encoded bundle exceeds size limit"));
        }
        Ok(bytes)
    })();
    let bytes = cleanup(&stage, result)?;
    atomic_export(&destination, &bytes)
}

fn atomic_export(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let (temp, mut file) = loop {
        let temp = sibling(destination, "tmp")?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, destination)
    })();
    if let Err(original) = result {
        return match fs::remove_file(&temp) {
            Ok(()) => Err(original),
            Err(cleanup) => Err(io::Error::other(format!(
                "{original}; export cleanup failed at {}: {cleanup}",
                temp.display()
            ))),
        };
    }
    Ok(())
}

fn validate_portable_audio(layers: &LayerStore, stage: &Path) -> io::Result<()> {
    for (_, _, path) in audio_bindings(layers) {
        let path = path
            .to_str()
            .ok_or_else(|| invalid("non-UTF-8 audio reference"))?;
        if !path.starts_with("media/") || !allowed(path) {
            return Err(invalid("audio reference must name bundled media"));
        }
        read_regular(&stage.join(path), MAX_FILE)?;
    }
    Ok(())
}

/// Install a validated bundle, retaining the previous root as a sibling backup.
///
/// Returns `None` for a first install. A failed install restores the old root;
/// if rollback itself fails, the error names the retained recovery backup.
pub fn import(source: &Path, root: &Path) -> io::Result<Option<PathBuf>> {
    let bytes = read_regular(source, MAX_BUNDLE)?;
    let bundle: Bundle =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid configuration bundle JSON"))?;
    if bundle.format != "sdeck-plus" || !matches!(bundle.version, 1 | 2) {
        return Err(invalid("unsupported bundle format or version"));
    }
    if bundle.files.len() > MAX_ENTRIES {
        return Err(invalid("too many bundle entries"));
    }
    let root = absolute_target(root)?;
    let mut files = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for entry in bundle.files {
        if !allowed(&entry.path) || !seen.insert(entry.path.clone()) {
            return Err(invalid("unsafe, duplicate or oversized bundle entry"));
        }
        let bytes = match entry.data {
            Some(data) => {
                if data.len() > limit(&entry.path).div_ceil(3) * 4 {
                    return Err(invalid("oversized bundle entry"));
                }
                STANDARD
                    .decode(&data)
                    .map_err(|_| invalid("invalid base64 bundle data"))?
            }
            None if bundle.version == 2 && entry.path.starts_with("assets/") => {
                read_regular(&root.join(&entry.path), MAX_FILE).map_err(|error| {
                    io::Error::new(
                        error.kind(),
                        format!("local artwork {}: {error}", entry.path),
                    )
                })?
            }
            None => return Err(invalid("bundle entry requires data")),
        };
        add(&mut files, entry.path, bytes)?;
    }
    match fs::symlink_metadata(&root) {
        Ok(metadata) if !metadata.is_dir() => {
            return Err(invalid("existing root is not a directory"))
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let stage = stage_for(&root)?;
    let result = (|| {
        materialize(&stage, &files)?;
        let mut layers = validate(&stage)?;
        validate_portable_audio(&layers, &stage)?;
        for (layer, trigger, path) in audio_bindings(&layers) {
            bind_audio(&mut layers, &layer, trigger, root.join(path))?;
        }
        persist_layers(&layers)?;
        for name in CONFIGS {
            if stage.join(name).exists() {
                fs::set_permissions(stage.join(name), fs::Permissions::from_mode(0o600))?;
            }
        }
        install(&stage, &root, |from, to| fs::rename(from, to))
    })();
    cleanup(&stage, result)
}

fn install(
    stage: &Path,
    root: &Path,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<Option<PathBuf>> {
    let backup = match fs::symlink_metadata(root) {
        Ok(_) => {
            let reserved = loop {
                let path = sibling(root, "backup")?;
                match private_dir(&path) {
                    Ok(()) => break path,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            };
            if let Err(error) = rename(root, &reserved) {
                return match fs::remove_dir(&reserved) {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(io::Error::other(format!(
                        "{error}; backup reservation cleanup failed at {}: {cleanup}",
                        reserved.display()
                    ))),
                };
            }
            Some(reserved)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if let Err(error) = rename(stage, root) {
        if let Some(backup) = &backup {
            if let Err(rollback) = rename(backup, root) {
                return Err(io::Error::other(format!(
                    "{error}; rollback failed: {rollback}; previous configuration retained at {}",
                    backup.display()
                )));
            }
        }
        return Err(error);
    }
    Ok(backup)
}

#[cfg(test)]
mod tests;
