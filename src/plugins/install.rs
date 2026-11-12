//! Adding a plugin from a folder or an `https://` repository: headless, no GTK.
//!
//! The flow is [`parse_source`] -> [`stage`] (copy or shallow clone into a private temp
//! directory, validated like the registry and the validator do, previewed) -> [`install`]
//! (atomic rename under the plugins folder, never enabling). See `docs/plugins.md`.

mod copy;
mod git;
mod place;
mod source;
mod stage;
#[cfg(test)]
mod tests;

pub use place::{install, InstallError, InstallMode, Installed};
pub use source::{parse_source, parse_source_in, PluginSource, SourceError};
pub use stage::{
    stage, ActionPreview, PluginPreview, RequirementPreview, SettingPreview, StagedPlugin,
};

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_DEADLINE: Duration = Duration::from_secs(60);
const MIN_DEADLINE: Duration = Duration::from_millis(50);
const MAX_DEADLINE: Duration = Duration::from_secs(300);
const DEFAULT_MAX_CLONE_BYTES: u64 = 32 * 1024 * 1024;

/// A shared flag that stops a running [`stage`]; clones observe the same state.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Knobs for [`stage`]; the defaults are what the app uses.
#[derive(Clone, Debug)]
pub struct StageOptions {
    git_program: OsString,
    deadline: Duration,
    max_clone_bytes: u64,
    plugins_root: Option<PathBuf>,
    temp_dir: Option<PathBuf>,
}

impl Default for StageOptions {
    fn default() -> Self {
        Self {
            git_program: "git".into(),
            deadline: DEFAULT_DEADLINE,
            max_clone_bytes: DEFAULT_MAX_CLONE_BYTES,
            plugins_root: None,
            temp_dir: None,
        }
    }
}

impl StageOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// The git executable (default `git` from `PATH`); it is still run with the hardened argv
    /// and environment. Meant for tests and unusual installations.
    pub fn git_program(mut self, program: impl Into<OsString>) -> Self {
        self.git_program = program.into();
        self
    }

    /// Total time allowed for the clone (default 60 s), clamped to 50 ms-300 s.
    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline.clamp(MIN_DEADLINE, MAX_DEADLINE);
        self
    }

    /// Size ceiling of the downloaded tree, `.git` included (default 32 MiB).
    pub fn max_clone_bytes(mut self, bytes: u64) -> Self {
        self.max_clone_bytes = bytes;
        self
    }

    /// The plugins folder: folder sources equal to, inside or containing it are refused.
    pub fn plugins_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.plugins_root = Some(root.into());
        self
    }

    /// Where the private staging directory is created (default: the system temp directory).
    pub fn temp_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.temp_dir = Some(dir.into());
        self
    }
}

/// Why a candidate could not be staged. Nothing is left behind in any case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StageError {
    /// The source itself is not acceptable (unsafe address, overlaps the plugins folder).
    Source(String),
    /// The candidate breaks a copy limit or the plugin format; every problem is listed.
    Invalid(Vec<String>),
    /// git could not be started.
    GitMissing(String),
    /// git exited unsuccessfully; `diagnostics` is at most 4 KiB of stderr, address removed.
    GitFailed {
        status: Option<i32>,
        diagnostics: String,
    },
    DeadlineExceeded(Duration),
    /// The download grew beyond `limit` bytes and was stopped.
    TooLarge {
        limit: u64,
    },
    Cancelled,
    /// git reported something other than a 40 or 64 character hex commit.
    BadCommit(String),
    Io(String),
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(message) | Self::Io(message) | Self::GitMissing(message) => {
                f.write_str(message)
            }
            Self::Invalid(problems) => {
                f.write_str("the plugin does not follow the plugin format:")?;
                problems
                    .iter()
                    .try_for_each(|problem| write!(f, "\n- {problem}"))
            }
            Self::GitFailed {
                status,
                diagnostics,
            } => {
                match status {
                    Some(code) => write!(f, "git failed with exit status {code}")?,
                    None => f.write_str("git was stopped by a signal")?,
                }
                if diagnostics.is_empty() {
                    Ok(())
                } else {
                    write!(f, ": {diagnostics}")
                }
            }
            Self::DeadlineExceeded(limit) => {
                write!(f, "the download took longer than {} s", limit.as_secs_f32())
            }
            Self::TooLarge { limit } => {
                write!(f, "the repository is larger than {limit} bytes")
            }
            Self::Cancelled => f.write_str("cancelled"),
            Self::BadCommit(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StageError {}
