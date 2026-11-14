//! Staging: fetch or copy a candidate into a private temp directory, validate it with the same
//! rules the registry and the validator use, and describe it for the confirmation screen.

use super::copy::{copy_project, CopyStats};
use super::git::{GitRunner, SystemGit};
use super::source::{check_outside_root, validate_repository};
use super::{CancelToken, PluginSource, StageError, StageOptions};
use crate::plugins::{
    PluginControl, PluginManifest, PluginRegistry, PluginValue, Requirement, MAX_MANIFEST_BYTES,
};
use std::fs;
use std::io::Read;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

const STAGE_PREFIX: &str = "sdeck-plugin-stage-";

/// One setting of an action, as shown before installing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingPreview {
    pub id: String,
    pub label: String,
    /// `text`, `integer` or `boolean`.
    pub kind: String,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionPreview {
    pub id: String,
    pub name: String,
    pub controls: Vec<PluginControl>,
    pub settings: Vec<SettingPreview>,
    /// Id of the declared value this action displays.
    pub display: Option<String>,
}

/// A declared requirement; `missing` is only ever true for `command` requirements that no `PATH`
/// directory provides. Missing commands never block installing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequirementPreview {
    pub requirement: Requirement,
    pub missing: bool,
}

/// Everything a confirmation screen shows before anything is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginPreview {
    pub id: String,
    pub name: String,
    pub version: String,
    pub protocol_version: u32,
    pub actions: Vec<ActionPreview>,
    /// Values the plugin can report for display.
    pub values: Vec<PluginValue>,
    pub requirements: Vec<RequirementPreview>,
    pub source: PluginSource,
    /// The downloaded commit, for repositories only.
    pub commit: Option<String>,
    pub file_count: usize,
    pub total_bytes: u64,
}

/// A validated candidate in a private staging directory, removed when dropped.
#[derive(Debug)]
pub struct StagedPlugin {
    dir: StageDir,
    manifest: PluginManifest,
    preview: PluginPreview,
}

impl StagedPlugin {
    pub fn preview(&self) -> &PluginPreview {
        &self.preview
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// The private staging directory (mode 0700); the plugin's files are in `<directory>/<id>`.
    pub fn directory(&self) -> &Path {
        &self.dir.0
    }

    pub(super) fn files(&self) -> PathBuf {
        self.dir.0.join("root").join(&self.manifest.id)
    }
}

/// The private staging directory; removed on drop, so on every error path too.
#[derive(Debug)]
struct StageDir(PathBuf);

impl StageDir {
    fn create(parent: &Path) -> Result<Self, StageError> {
        use std::hash::{BuildHasher, Hasher};
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        for _ in 0..16 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u32(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            let path = parent.join(format!(
                "{STAGE_PREFIX}{}-{nanos:x}-{:016x}",
                std::process::id(),
                hasher.finish()
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(StageError::Io(format!(
                        "cannot create a staging folder in {}: {error}",
                        parent.display()
                    )))
                }
            }
        }
        Err(StageError::Io(
            "cannot find an unused staging folder name".into(),
        ))
    }
}

impl Drop for StageDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Stages a source: copies a folder, or shallow-clones a repository with the hardened system
/// git, then validates the result exactly like the registry would. Installs nothing.
///
/// Synchronous and potentially slow (a network clone): call it from a worker thread and use the
/// [`CancelToken`] to stop it.
pub fn stage(
    source: &PluginSource,
    options: &StageOptions,
    cancel: &CancelToken,
) -> Result<StagedPlugin, StageError> {
    if let PluginSource::Repository(url) = source {
        validate_repository(url).map_err(|error| StageError::Source(error.to_string()))?;
    }
    let git = SystemGit::new(
        &options.git_program,
        options.deadline,
        options.max_clone_bytes,
    );
    stage_with(source, options, cancel, &git)
}

/// [`stage`] with an injectable git; repository addresses must already be validated.
pub(super) fn stage_with(
    source: &PluginSource,
    options: &StageOptions,
    cancel: &CancelToken,
    git: &dyn GitRunner,
) -> Result<StagedPlugin, StageError> {
    let parent = options.temp_dir.clone().unwrap_or_else(std::env::temp_dir);
    let dir = StageDir::create(&parent)?;
    let work = dir.0.join("work");
    let mkdir = |path: &Path| {
        fs::DirBuilder::new()
            .mode(0o755)
            .create(path)
            .map_err(|error| StageError::Io(format!("{}: {error}", path.display())))
    };
    mkdir(&work)?;
    let (stats, commit) = match source {
        PluginSource::Folder(folder) => {
            if let Some(root) = &options.plugins_root {
                check_outside_root(folder, root)
                    .map_err(|error| StageError::Source(error.to_string()))?;
            }
            (copy_project(folder, &work, cancel)?, None)
        }
        PluginSource::Repository(url) => fetch(url, &dir.0, &work, cancel, git)?,
    };
    let manifest = read_manifest(&work)?;
    // The registry insists that the directory is named after the manifest id; the project
    // directory need not be, so the staged files are moved (not copied) under that name.
    let registry_root = dir.0.join("root");
    mkdir(&registry_root)?;
    fs::rename(&work, registry_root.join(&manifest.id))
        .map_err(|error| StageError::Io(format!("cannot stage `{}`: {error}", manifest.id)))?;
    let registry = PluginRegistry::open(&registry_root);
    let mut problems: Vec<String> = registry
        .problems()
        .iter()
        .map(|problem| format!("{}: {}", problem.directory, problem.error))
        .collect();
    if problems.is_empty() && registry.plugin(&manifest.id).is_none() {
        problems.push(format!("plugin `{}` was not recognised", manifest.id));
    }
    if !problems.is_empty() {
        return Err(StageError::Invalid(problems));
    }
    let preview = preview(&manifest, source, commit, stats);
    Ok(StagedPlugin {
        dir,
        manifest,
        preview,
    })
}

fn fetch(
    url: &str,
    stage_dir: &Path,
    work: &Path,
    cancel: &CancelToken,
    git: &dyn GitRunner,
) -> Result<(CopyStats, Option<String>), StageError> {
    let clone = stage_dir.join("clone");
    git.clone_repo(url, &clone, cancel)?;
    let commit = git.head_commit(&clone, cancel)?;
    // The history is never needed again and must not be copied or run against.
    let history = clone.join(".git");
    match fs::symlink_metadata(&history) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(&history)
            .map_err(|error| StageError::Io(format!("cannot remove .git: {error}")))?,
        Ok(_) => {
            return Err(StageError::Invalid(vec![
                ".git: not a plain directory in the downloaded repository".into(),
            ]))
        }
        Err(_) => {}
    }
    let stats = copy_project(&clone, work, cancel)?;
    fs::remove_dir_all(&clone)
        .map_err(|error| StageError::Io(format!("cannot remove the clone: {error}")))?;
    Ok((stats, Some(commit)))
}

fn read_manifest(project: &Path) -> Result<PluginManifest, StageError> {
    let path = project.join("manifest.json");
    let problem = |text: String| StageError::Invalid(vec![text]);
    let meta = fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            problem("manifest.json not found at the project root".into())
        } else {
            problem(format!("manifest.json: {error}"))
        }
    })?;
    if !meta.file_type().is_file() {
        return Err(problem("manifest.json is not a regular file".into()));
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| problem(format!("manifest.json: {error}")))?;
    PluginManifest::parse(&bytes).map_err(|error| problem(error.to_string()))
}

fn preview(
    manifest: &PluginManifest,
    source: &PluginSource,
    commit: Option<String>,
    stats: CopyStats,
) -> PluginPreview {
    let missing = manifest.missing_commands();
    PluginPreview {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        protocol_version: manifest.protocol_version,
        actions: manifest
            .actions
            .iter()
            .map(|action| ActionPreview {
                id: action.id.clone(),
                name: action.name.clone(),
                controls: action.controls.clone(),
                settings: action
                    .settings
                    .iter()
                    .map(|field| {
                        let value = serde_json::to_value(field).unwrap_or_default();
                        SettingPreview {
                            id: field.id().to_owned(),
                            label: field.label().to_owned(),
                            kind: value["type"].as_str().unwrap_or("?").to_owned(),
                            required: value["required"].as_bool().unwrap_or(false),
                        }
                    })
                    .collect(),
                display: action.display.clone(),
            })
            .collect(),
        values: manifest.values.clone(),
        requirements: manifest
            .requirements
            .iter()
            .map(|requirement| RequirementPreview {
                requirement: requirement.clone(),
                missing: missing.contains(&requirement),
            })
            .collect(),
        source: source.clone(),
        commit,
        file_count: stats.files,
        total_bytes: stats.bytes,
    }
}
