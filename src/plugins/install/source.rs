//! Where a plugin comes from: an absolute folder or an `https://` repository address.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_ADDRESS_CHARS: usize = 512;
const ONLY_HTTPS: &str = "only https:// repository addresses or absolute folder paths are accepted";

/// A candidate plugin location, already checked by [`parse_source`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginSource {
    /// A canonical, existing directory.
    Folder(PathBuf),
    /// A validated `https://` address (scheme lowercased).
    Repository(String),
}

impl fmt::Display for PluginSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Folder(path) => write!(f, "{}", path.display()),
            Self::Repository(url) => f.write_str(url),
        }
    }
}

/// A user-facing reason the text is not an acceptable plugin source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceError(String);

impl SourceError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SourceError {}

/// Classifies and checks user input: a leading `https://` (any case) means a repository, anything
/// else a folder path. Nothing is fetched or copied. Folders are resolved with `canonicalize`.
pub fn parse_source(input: &str) -> Result<PluginSource, SourceError> {
    parse(input, None)
}

/// [`parse_source`] that also refuses folders equal to, inside or containing `plugins_root`.
pub fn parse_source_in(input: &str, plugins_root: &Path) -> Result<PluginSource, SourceError> {
    parse(input, Some(plugins_root))
}

fn parse(input: &str, plugins_root: Option<&Path>) -> Result<PluginSource, SourceError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(SourceError::new(
            "enter a folder path or an https:// repository address",
        ));
    }
    if text
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
    {
        return validate_repository(text).map(PluginSource::Repository);
    }
    if text.starts_with('-') {
        return Err(SourceError::new(ONLY_HTTPS));
    }
    let local = text == "~" || text.starts_with("~/") || text.starts_with('/');
    if !local {
        // `ssh://`, `git@host:path`, `ext::...`, `file://`: anything shaped like a remote.
        let before_slash = text.split('/').next().unwrap_or(text);
        if before_slash.contains(':') {
            return Err(SourceError::new(ONLY_HTTPS));
        }
        return Err(SourceError::new(
            "use an absolute path for a folder (for example /home/you/my-plugin or ~/my-plugin)",
        ));
    }
    let path = expand_home(text)?;
    let resolved = fs::canonicalize(&path)
        .map_err(|error| SourceError::new(format!("{}: {error}", path.display())))?;
    if !resolved.is_dir() {
        return Err(SourceError::new(format!(
            "{} is not a folder",
            resolved.display()
        )));
    }
    if let Some(root) = plugins_root {
        check_outside_root(&resolved, root)?;
    }
    Ok(PluginSource::Folder(resolved))
}

fn expand_home(text: &str) -> Result<PathBuf, SourceError> {
    let Some(rest) = text.strip_prefix('~') else {
        return Ok(PathBuf::from(text));
    };
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or_else(|| SourceError::new("cannot expand `~`: HOME is not set"))?;
    Ok(home.join(rest.trim_start_matches('/')))
}

/// The folder must not be the plugins folder, live inside it or contain it, so installing can
/// never copy a tree into itself or expose another plugin's files.
pub(super) fn check_outside_root(folder: &Path, root: &Path) -> Result<(), SourceError> {
    let root = fs::canonicalize(root).unwrap_or_else(|_| lexical(root));
    if folder.starts_with(&root) || root.starts_with(folder) {
        return Err(SourceError::new(format!(
            "{} overlaps the plugins folder {}; choose the plugin's own project folder",
            folder.display(),
            root.display()
        )));
    }
    Ok(())
}

/// Collapses `.`/`..` for a path that does not exist yet.
fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The repository rules; the only text that is ever handed to git. Returns the normalised
/// address (`https://` in lowercase).
pub(super) fn validate_repository(text: &str) -> Result<String, SourceError> {
    let fail = |message: &str| Err(SourceError::new(format!("repository address: {message}")));
    let Some(rest) = text
        .get(..8)
        .filter(|scheme| scheme.eq_ignore_ascii_case("https://"))
        .map(|_| &text[8..])
    else {
        return Err(SourceError::new(ONLY_HTTPS));
    };
    if text.chars().count() > MAX_ADDRESS_CHARS {
        return fail("longer than 512 characters");
    }
    if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return fail("must not contain spaces or control characters");
    }
    if rest.contains('?') || rest.contains('#') {
        return fail("must not contain a query string or a fragment");
    }
    let (authority, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    if authority.contains('@') {
        return fail("must not contain a user name or password; credentials are never accepted");
    }
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    let host_ok = !host.is_empty()
        && !host.starts_with('-')
        && host.split('.').all(|label| {
            !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if !host_ok {
        return fail("the host must be a plain ASCII name (letters, digits, dots and hyphens)");
    }
    if let Some(port) = port {
        let numeric = !port.is_empty()
            && port.len() <= 5
            && port.chars().all(|c| c.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|p| p != 0);
        if !numeric {
            return fail("the port must be a number from 1 to 65535");
        }
    }
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    let segments: Vec<&str> = trimmed.strip_prefix('/').unwrap_or("").split('/').collect();
    let safe = |segment: &&str| {
        !segment.is_empty()
            && *segment != "."
            && *segment != ".."
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._~-".contains(c))
    };
    if trimmed.is_empty() || !trimmed.starts_with('/') || !segments.iter().all(safe) {
        return fail(
            "the path needs at least one segment of letters, digits and . _ ~ - (for example /owner/plugin.git)",
        );
    }
    Ok(format!("https://{rest}"))
}
