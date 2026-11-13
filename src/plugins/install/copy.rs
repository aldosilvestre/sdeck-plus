//! The bounded project copy shared by staging and installing: regular files only, with the same
//! limits as the plugin validator. A violation is an error naming the path, never a silent skip
//! (only `.git` entries and `__pycache__` directories are skipped), and nothing is copied unless the
//! whole tree passes the scan.

use super::{CancelToken, StageError};
use std::fs;
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub(super) const MAX_FILES: usize = 256;
pub(super) const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub(super) const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const MAX_DEPTH: usize = 8;
const SKIPPED: [&str; 2] = [".git", "__pycache__"];
const MAX_PROBLEMS: usize = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CopyStats {
    pub files: usize,
    pub bytes: u64,
}

#[derive(Default)]
struct Plan {
    dirs: Vec<PathBuf>,
    files: Vec<(PathBuf, u64)>,
    problems: Vec<String>,
    bytes: u64,
}

impl Plan {
    fn problem(&mut self, text: String) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(text);
        }
    }
}

/// Copies `from` into the existing directory `to`. Files with any execute bit become 0755, other
/// files 0644 and directories 0755, so modes never depend on the source's umask or ownership.
pub(super) fn copy_project(
    from: &Path,
    to: &Path,
    cancel: &CancelToken,
) -> Result<CopyStats, StageError> {
    let mut plan = Plan::default();
    scan(from, Path::new(""), 0, &mut plan, cancel)?;
    if plan.files.len() > MAX_FILES {
        plan.problem(format!("more than {MAX_FILES} files"));
    }
    if plan.bytes > MAX_TOTAL_BYTES {
        plan.problem(format!(
            "the project is larger than {MAX_TOTAL_BYTES} bytes in total"
        ));
    }
    if !plan.problems.is_empty() {
        return Err(StageError::Invalid(plan.problems));
    }
    for dir in &plan.dirs {
        fs::DirBuilder::new()
            .mode(0o755)
            .create(to.join(dir))
            .map_err(|error| StageError::Io(format!("{}: {error}", dir.display())))?;
    }
    let mut problems = Vec::new();
    for (relative, _) in &plan.files {
        if cancel.is_cancelled() {
            return Err(StageError::Cancelled);
        }
        if let Err(problem) = copy_file(&from.join(relative), &to.join(relative)) {
            if problems.len() < MAX_PROBLEMS {
                problems.push(format!("{}: {problem}", relative.display()));
            }
        }
    }
    if !problems.is_empty() {
        return Err(StageError::Invalid(problems));
    }
    Ok(CopyStats {
        files: plan.files.len(),
        bytes: plan.bytes,
    })
}

fn scan(
    root: &Path,
    relative: &Path,
    depth: usize,
    plan: &mut Plan,
    cancel: &CancelToken,
) -> Result<(), StageError> {
    if cancel.is_cancelled() {
        return Err(StageError::Cancelled);
    }
    let here = root.join(relative);
    let entries = match fs::read_dir(&here) {
        Ok(entries) => entries,
        Err(error) => {
            plan.problem(format!("{}: {error}", display(relative)));
            return Ok(());
        }
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let child = relative.join(&name);
        let Ok(kind) = entry.file_type() else {
            plan.problem(format!("{}: unreadable file type", child.display()));
            continue;
        };
        if kind.is_dir() {
            if SKIPPED.iter().any(|skip| name == *skip) {
                continue;
            }
            if depth >= MAX_DEPTH {
                plan.problem(format!(
                    "{}: nested deeper than {MAX_DEPTH} levels",
                    child.display()
                ));
                continue;
            }
            plan.dirs.push(child.clone());
            scan(root, &child, depth + 1, plan, cancel)?;
        } else if kind.is_file() && name == ".git" {
            // A `.git` file (a worktree or submodule pointer) is as inert and pointless to copy
            // as the directory.
            continue;
        } else if kind.is_symlink() {
            plan.problem(format!(
                "{}: symlinks are not allowed; a plugin must ship regular files",
                child.display()
            ));
        } else if kind.is_file() {
            let size = entry.metadata().map_or(u64::MAX, |meta| meta.len());
            if size > MAX_FILE_BYTES {
                plan.problem(format!(
                    "{}: larger than {MAX_FILE_BYTES} bytes",
                    child.display()
                ));
            }
            plan.bytes = plan.bytes.saturating_add(size.min(MAX_FILE_BYTES + 1));
            plan.files.push((child, size));
        } else {
            plan.problem(format!("{}: not a regular file", child.display()));
        }
    }
    Ok(())
}

fn display(relative: &Path) -> String {
    if relative.as_os_str().is_empty() {
        ".".into()
    } else {
        relative.display().to_string()
    }
}

/// Opens without following a symlink swapped in after the scan, rechecks the opened file and
/// never writes more than the per-file limit.
fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc_nofollow())
        .open(from)
        .map_err(|error| error.to_string())?;
    let meta = file.metadata().map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    let mode = if meta.mode() & 0o111 == 0 {
        0o644
    } else {
        0o755
    };
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(to)
        .map_err(|error| error.to_string())?;
    // `mode` is filtered by the umask, so set it explicitly.
    fs::set_permissions(to, std::os::unix::fs::PermissionsExt::from_mode(mode))
        .map_err(|error| error.to_string())?;
    let copied = std::io::copy(&mut file.take(MAX_FILE_BYTES + 1), &mut out)
        .map_err(|error| error.to_string())?;
    if copied > MAX_FILE_BYTES {
        return Err(format!("larger than {MAX_FILE_BYTES} bytes"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn libc_nofollow() -> i32 {
    libc::O_NOFOLLOW
}

#[cfg(not(target_os = "linux"))]
fn libc_nofollow() -> i32 {
    0
}
