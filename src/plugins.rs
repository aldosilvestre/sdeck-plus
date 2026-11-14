//! External plugins: manifest discovery, explicit enablement and supervised invocation.
//!
//! Layout under a caller-provided root (normally `<data>/sdeck-plus/plugins`):
//! `plugins.json` (enabled state) and `<id>/manifest.json` plus the plugin's files. Opening a
//! registry only reads manifests; a plugin executable runs solely in [`PluginRegistry::invoke`],
//! and only when the plugin is installed, valid and explicitly enabled. See `docs/plugins.md`.

mod host;
mod install;
mod manifest;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use host::{
    InvokeOutcome, QueryOutcome, MAX_DETAIL_CHARS, MAX_HEADLINE_CHARS, MAX_MESSAGE_CHARS,
    MAX_STDERR_BYTES, MAX_STDOUT_BYTES, MAX_TIMEOUT,
};
pub use install::{
    install, parse_source, parse_source_in, stage, ActionPreview, CancelToken, InstallError,
    InstallMode, Installed, PluginPreview, PluginSource, RequirementPreview, SettingPreview,
    SourceError, StageError, StageOptions, StagedPlugin,
};
pub use manifest::{
    missing_commands, missing_commands_in, valid_id, PluginAction, PluginControl, PluginManifest,
    PluginValue, Requirement, RequirementKind, SettingField, Settings, MAX_MANIFEST_BYTES,
    MAX_SETTINGS_BYTES, PROTOCOL_VERSION,
};

const STATE_FILE: &str = "plugins.json";
const MAX_STATE_BYTES: u64 = 16 * 1024;
const MAX_PLUGINS: usize = 64;
const MAX_BINDING_SETTINGS: usize = 16;
const MAX_BINDING_TEXT: usize = 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Everything that can go wrong while discovering, configuring or invoking a plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginError {
    InvalidManifest(String),
    UnsafeExecutable(String),
    Io(String),
    /// `plugins.json` is unreadable or invalid; it is kept untouched and writes are refused.
    InvalidState(String),
    /// The binding names a plugin that is not installed (or whose manifest is invalid).
    Missing(String),
    Disabled(String),
    UnknownAction {
        plugin: String,
        action: String,
    },
    InvalidSettings(String),
    /// A query was asked of an action that declares no `display` value.
    NoDisplay {
        plugin: String,
        action: String,
    },
    InvalidRequest(String),
    Spawn(String),
    Timeout,
    OutputTooLarge,
    NonZeroExit {
        code: Option<i32>,
        diagnostics: String,
    },
    MalformedResponse(String),
    RequestIdMismatch,
    /// The plugin answered `ok: false` with this message.
    Reported(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidManifest(m) => write!(f, "invalid plugin manifest: {m}"),
            Self::UnsafeExecutable(m) => write!(f, "unsafe plugin executable: {m}"),
            Self::Io(m) => write!(f, "plugin I/O error: {m}"),
            Self::InvalidState(m) => write!(f, "invalid plugin state: {m}"),
            Self::Missing(id) => write!(f, "plugin `{id}` is not installed"),
            Self::Disabled(id) => write!(f, "plugin `{id}` is disabled"),
            Self::UnknownAction { plugin, action } => {
                write!(f, "plugin `{plugin}` has no action `{action}`")
            }
            Self::InvalidSettings(m) => write!(f, "invalid plugin settings: {m}"),
            Self::NoDisplay { plugin, action } => {
                write!(
                    f,
                    "action `{action}` of plugin `{plugin}` displays no value"
                )
            }
            Self::InvalidRequest(m) => write!(f, "invalid plugin request: {m}"),
            Self::Spawn(m) => write!(f, "cannot start plugin: {m}"),
            Self::Timeout => f.write_str("plugin timed out"),
            Self::OutputTooLarge => f.write_str("plugin output exceeded the size limit"),
            Self::NonZeroExit { code, diagnostics } => {
                write!(f, "plugin exited with {code:?}: {diagnostics}")
            }
            Self::MalformedResponse(m) => write!(f, "malformed plugin response: {m}"),
            Self::RequestIdMismatch => f.write_str("plugin answered a different request"),
            Self::Reported(m) => write!(f, "plugin reported an error: {m}"),
        }
    }
}

impl std::error::Error for PluginError {}

/// A persisted choice of plugin action plus its settings. Plain data: it stays valid when the
/// plugin is not installed, and holds neither executables nor credentials from the plugin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginBinding {
    pub plugin_id: String,
    pub action_id: String,
    #[serde(default)]
    pub settings: Settings,
}

impl PluginBinding {
    pub fn new(
        plugin_id: impl Into<String>,
        action_id: impl Into<String>,
        settings: Settings,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            action_id: action_id.into(),
            settings,
        }
    }

    /// Checks the binding's shape without a manifest, so it stays valid while its plugin is
    /// missing or disabled: ids, plain setting values (text, integer, boolean) and total size.
    /// Field-level limits are checked against the manifest by [`PluginRegistry::validate_binding`].
    pub fn validate(&self) -> Result<(), PluginError> {
        let invalid = |message: String| PluginError::InvalidSettings(message);
        valid_id(&self.plugin_id).map_err(|m| invalid(format!("plugin id: {m}")))?;
        valid_id(&self.action_id).map_err(|m| invalid(format!("action id: {m}")))?;
        if self.settings.len() > MAX_BINDING_SETTINGS {
            return Err(invalid(format!(
                "more than {MAX_BINDING_SETTINGS} settings"
            )));
        }
        for (key, value) in &self.settings {
            valid_id(key).map_err(|m| invalid(format!("setting `{key}`: {m}")))?;
            let plain = match value {
                serde_json::Value::Bool(_) => true,
                serde_json::Value::Number(number) => number.is_i64(),
                serde_json::Value::String(text) => text.chars().count() <= MAX_BINDING_TEXT,
                _ => false,
            };
            if !plain {
                return Err(invalid(format!(
                    "setting `{key}` must be text up to {MAX_BINDING_TEXT} characters, an integer or a boolean"
                )));
            }
        }
        let size = serde_json::to_vec(&self.settings).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_SETTINGS_BYTES {
            return Err(invalid(format!(
                "settings exceed the {MAX_SETTINGS_BYTES}-byte limit"
            )));
        }
        Ok(())
    }
}

/// Per-invocation parameters chosen by the caller.
#[derive(Clone, Debug)]
pub struct InvokeOptions {
    request_id: String,
    timeout: Duration,
}

impl InvokeOptions {
    /// `request_id` is 1-64 chars of `[A-Za-z0-9._-]`; the default timeout is 5 s.
    pub fn new(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Total time for spawn, exchange and exit; must be in `(0, MAX_TIMEOUT]`.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn validate(&self) -> Result<(), PluginError> {
        let id_ok = !self.request_id.is_empty()
            && self.request_id.len() <= 64
            && self
                .request_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
        if !id_ok {
            return Err(PluginError::InvalidRequest(
                "request id must be 1-64 of [A-Za-z0-9._-]".into(),
            ));
        }
        if self.timeout.is_zero() || self.timeout > MAX_TIMEOUT {
            return Err(PluginError::InvalidRequest(
                "timeout must be in (0, 60s]".into(),
            ));
        }
        Ok(())
    }
}

/// A plugin directory that could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginProblem {
    pub directory: String,
    pub error: PluginError,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateFile {
    version: u32,
    enabled: Vec<String>,
}

/// Installed plugins plus the explicit enabled state, rooted at a caller-provided directory.
#[derive(Debug)]
pub struct PluginRegistry {
    root: PathBuf,
    plugins: BTreeMap<String, PluginManifest>,
    problems: Vec<PluginProblem>,
    enabled: BTreeSet<String>,
    state_error: Option<PluginError>,
}

impl PluginRegistry {
    /// Discovers manifests and loads the enabled state. Never fails and never executes plugin
    /// code: malformed plugins are listed in [`Self::problems`], a bad `plugins.json` in
    /// [`Self::state_error`] (and then nothing is enabled).
    pub fn open(root: impl Into<PathBuf>) -> Self {
        let mut registry = Self {
            root: root.into(),
            plugins: BTreeMap::new(),
            problems: Vec::new(),
            enabled: BTreeSet::new(),
            state_error: None,
        };
        registry.rescan();
        registry
    }

    /// Re-reads manifests and enabled state from disk.
    pub fn rescan(&mut self) {
        self.plugins.clear();
        self.problems.clear();
        self.discover();
        match read_state(&self.root) {
            Ok(enabled) => (self.enabled, self.state_error) = (enabled, None),
            Err(error) => (self.enabled, self.state_error) = (BTreeSet::new(), Some(error)),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Valid installed plugins, ordered by id.
    pub fn plugins(&self) -> impl Iterator<Item = &PluginManifest> {
        self.plugins.values()
    }

    pub fn plugin(&self, id: &str) -> Option<&PluginManifest> {
        self.plugins.get(id)
    }

    pub fn problems(&self) -> &[PluginProblem] {
        &self.problems
    }

    pub fn state_error(&self) -> Option<&PluginError> {
        self.state_error.as_ref()
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.contains(id)
    }

    /// Enables or disables an installed plugin and atomically persists the choice. Refuses to
    /// write over an invalid `plugins.json`; enabling an unknown plugin is [`PluginError::Missing`].
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), PluginError> {
        if let Some(error) = &self.state_error {
            return Err(PluginError::InvalidState(error.to_string()));
        }
        if enabled && !self.plugins.contains_key(id) {
            return Err(PluginError::Missing(id.to_owned()));
        }
        let mut next = self.enabled.clone();
        if (enabled && !next.insert(id.to_owned())) || (!enabled && !next.remove(id)) {
            return Ok(());
        }
        write_state(&self.root, &next)?;
        self.enabled = next;
        Ok(())
    }

    /// Checks a binding against the installed manifest without running anything, returning the
    /// settings with defaults applied. Distinguishes missing, disabled, unknown-action and
    /// invalid-settings so callers can show a precise state.
    pub fn validate_binding(&self, binding: &PluginBinding) -> Result<Settings, PluginError> {
        let manifest = self
            .plugins
            .get(&binding.plugin_id)
            .ok_or_else(|| PluginError::Missing(binding.plugin_id.clone()))?;
        if !self.is_enabled(&binding.plugin_id) {
            return Err(PluginError::Disabled(binding.plugin_id.clone()));
        }
        let action =
            manifest
                .action(&binding.action_id)
                .ok_or_else(|| PluginError::UnknownAction {
                    plugin: binding.plugin_id.clone(),
                    action: binding.action_id.clone(),
                })?;
        action.validate_settings(&binding.settings)
    }

    /// Runs one action in a fresh child process and waits for its single response line.
    ///
    /// Synchronous: call from a worker thread, never from a GTK callback. Never retries.
    pub fn invoke(
        &self,
        binding: &PluginBinding,
        options: &InvokeOptions,
    ) -> Result<InvokeOutcome, PluginError> {
        options.validate()?;
        let settings = self.validate_binding(binding)?;
        let manifest = &self.plugins[&binding.plugin_id];
        let dir = self.root.join(&manifest.id);
        // Re-resolved at every call so a swapped file cannot escape the plugin directory.
        let executable = resolve_executable(&dir, &manifest.executable)?;
        let mut request = serde_json::to_vec(&host::Request {
            protocol_version: PROTOCOL_VERSION,
            kind: "invoke",
            request_id: &options.request_id,
            action: &binding.action_id,
            settings: &settings,
        })
        .map_err(|error| PluginError::InvalidRequest(error.to_string()))?;
        request.push(b'\n');
        let output = host::run(&executable, &dir, &request, options.timeout)?;
        host::parse_response(output, &options.request_id)
    }

    /// Asks one action's plugin for the value it displays, in a fresh child process.
    ///
    /// Uses the action's `display` as the value id and the binding's validated settings; the
    /// limits and errors of [`Self::invoke`] apply. Synchronous: call from a worker thread.
    pub fn query(
        &self,
        binding: &PluginBinding,
        options: &InvokeOptions,
    ) -> Result<QueryOutcome, PluginError> {
        options.validate()?;
        let settings = self.validate_binding(binding)?;
        let manifest = &self.plugins[&binding.plugin_id];
        let display = manifest
            .action(&binding.action_id)
            .and_then(|action| action.display.as_deref())
            .ok_or_else(|| PluginError::NoDisplay {
                plugin: binding.plugin_id.clone(),
                action: binding.action_id.clone(),
            })?;
        let dir = self.root.join(&manifest.id);
        let executable = resolve_executable(&dir, &manifest.executable)?;
        let mut request = serde_json::to_vec(&host::QueryRequest {
            protocol_version: PROTOCOL_VERSION,
            kind: "query",
            request_id: &options.request_id,
            value: display,
            settings: &settings,
        })
        .map_err(|error| PluginError::InvalidRequest(error.to_string()))?;
        request.push(b'\n');
        let output = host::run(&executable, &dir, &request, options.timeout)?;
        host::parse_query_response(output, &options.request_id)
    }

    fn discover(&mut self) {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => return self.report("", PluginError::Io(error.to_string())),
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_file()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for (index, name) in names.into_iter().enumerate() {
            if index >= MAX_PLUGINS {
                self.report(
                    &name,
                    PluginError::InvalidManifest(format!("more than {MAX_PLUGINS} plugins")),
                );
                continue;
            }
            match load_plugin(&self.root.join(&name), &name) {
                Ok(manifest) => {
                    self.plugins.insert(name, manifest);
                }
                Err(error) => self.report(&name, error),
            }
        }
    }

    fn report(&mut self, directory: &str, error: PluginError) {
        self.problems.push(PluginProblem {
            directory: directory.to_owned(),
            error,
        });
    }
}

fn load_plugin(dir: &Path, name: &str) -> Result<PluginManifest, PluginError> {
    let kind = fs::symlink_metadata(dir).map_err(io_error)?.file_type();
    if !kind.is_dir() {
        return Err(PluginError::UnsafeExecutable(
            "plugin directory is not a plain directory".into(),
        ));
    }
    let bytes = read_regular(&dir.join("manifest.json"), MAX_MANIFEST_BYTES)?;
    let manifest = PluginManifest::parse(&bytes)?;
    if manifest.id != name {
        return Err(PluginError::InvalidManifest(format!(
            "manifest id `{}` does not match directory `{name}`",
            manifest.id
        )));
    }
    resolve_executable(dir, &manifest.executable)?;
    Ok(manifest)
}

fn io_error(error: std::io::Error) -> PluginError {
    PluginError::Io(error.to_string())
}

/// Reads a regular, non-symlink file of at most `cap` bytes.
fn read_regular(path: &Path, cap: u64) -> Result<Vec<u8>, PluginError> {
    let meta = fs::symlink_metadata(path).map_err(io_error)?;
    if !meta.file_type().is_file() {
        return Err(PluginError::Io(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(io_error)?
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > cap {
        return Err(PluginError::Io(format!(
            "{} exceeds {cap} bytes",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Resolves `relative` inside `dir`: lexically plain, canonically confined (symlinks included),
/// a regular file and executable.
fn resolve_executable(dir: &Path, relative: &str) -> Result<PathBuf, PluginError> {
    use std::os::unix::fs::PermissionsExt;
    manifest::valid_relative_path(relative).map_err(PluginError::UnsafeExecutable)?;
    let unsafe_path =
        |message: &str| PluginError::UnsafeExecutable(format!("{relative}: {message}"));
    let base = fs::canonicalize(dir).map_err(io_error)?;
    let target = fs::canonicalize(base.join(relative)).map_err(|_| unsafe_path("not found"))?;
    if !target.starts_with(&base) {
        return Err(unsafe_path("resolves outside the plugin directory"));
    }
    let meta = fs::metadata(&target).map_err(io_error)?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return Err(unsafe_path("not an executable regular file"));
    }
    Ok(target)
}

fn read_state(root: &Path) -> Result<BTreeSet<String>, PluginError> {
    let path = root.join(STATE_FILE);
    if fs::symlink_metadata(&path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(BTreeSet::new());
    }
    let invalid = |message: String| PluginError::InvalidState(message);
    let bytes = read_regular(&path, MAX_STATE_BYTES).map_err(|error| invalid(error.to_string()))?;
    let state: StateFile =
        serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    if state.version != 1 || state.enabled.len() > MAX_PLUGINS {
        return Err(invalid("unsupported version or too many entries".into()));
    }
    for id in &state.enabled {
        valid_id(id).map_err(invalid)?;
    }
    Ok(state.enabled.into_iter().collect())
}

/// Serialize, write a private temp file, then rename over `plugins.json`.
fn write_state(root: &Path, enabled: &BTreeSet<String>) -> Result<(), PluginError> {
    use std::os::unix::fs::OpenOptionsExt;
    let state = StateFile {
        version: 1,
        enabled: enabled.iter().cloned().collect(),
    };
    let bytes =
        serde_json::to_vec_pretty(&state).map_err(|error| PluginError::Io(error.to_string()))?;
    fs::create_dir_all(root).map_err(io_error)?;
    let temp = root.join(format!(".{STATE_FILE}.{}.tmp", std::process::id()));
    let result = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)
        .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&temp, root.join(STATE_FILE)));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(io_error)
}
