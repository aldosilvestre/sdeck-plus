//! Author tooling: validates a plugin PROJECT directory (manifest.json plus its executable at the
//! directory root) the way the app would load it, without running anything.
//!
//! ```text
//! cargo run --offline --example plugin_validate -- <plugin-dir>
//! cargo run --offline --example plugin_validate -- <plugin-dir> --invoke <action> \
//!     [--settings '<json>'] [--timeout <seconds, default 5, max 60>] [--request-id <id>]
//! ```
//!
//! The project's regular files are copied into a fresh temp root under the plugin id and loaded
//! with `PluginRegistry::open`, which executes nothing. Output markers: `PLUGIN_VALIDATE_OK:` /
//! `PLUGIN_VALIDATE_ERROR:` and, only with `--invoke`, `PLUGIN_INVOKE_OK:` / `PLUGIN_INVOKE_ERROR:`.
//! Exit status: 0 ok, 1 problems or a failed invocation, 2 usage. `--invoke` RUNS the plugin
//! with your privileges; nothing else here ever does.

use sdeck_plus::plugins::{
    missing_commands, InvokeOptions, PluginBinding, PluginManifest, PluginRegistry, Settings,
    MAX_MANIFEST_BYTES, MAX_TIMEOUT,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const MAX_FILES: usize = 256;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DEPTH: usize = 8;
const SKIPPED: [&str; 2] = [".git", "__pycache__"];
const USAGE: &str = "usage: plugin_validate <plugin-dir> [--invoke <action> [--settings '<json>'] \
[--timeout <1-60 seconds>] [--request-id <id>]]";

struct Invoke {
    action: String,
    settings: Settings,
    timeout: Duration,
    request_id: String,
}

fn parse_args(args: &[String]) -> Result<(PathBuf, Option<Invoke>), String> {
    let mut dir = None;
    let (mut action, mut settings, mut timeout, mut request_id) = (None, None, None, None);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--invoke" => action = Some(value("--invoke")?),
            "--settings" => settings = Some(value("--settings")?),
            "--timeout" => timeout = Some(value("--timeout")?),
            "--request-id" => request_id = Some(value("--request-id")?),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path if dir.is_none() => dir = Some(PathBuf::from(path)),
            _ => return Err("only one plugin directory is accepted".into()),
        }
    }
    let dir = dir.ok_or("a plugin directory is required")?;
    let Some(action) = action else {
        if settings.is_some() || timeout.is_some() || request_id.is_some() {
            return Err("--settings, --timeout and --request-id need --invoke".into());
        }
        return Ok((dir, None));
    };
    let settings = match settings {
        None => Settings::new(),
        Some(text) => match serde_json::from_str(&text) {
            Ok(serde_json::Value::Object(map)) => map,
            Ok(_) => return Err("--settings must be a JSON object".into()),
            Err(error) => return Err(format!("--settings is not valid JSON: {error}")),
        },
    };
    let seconds = match timeout {
        None => 5,
        Some(text) => text
            .parse::<u64>()
            .ok()
            .filter(|n| (1..=MAX_TIMEOUT.as_secs()).contains(n))
            .ok_or_else(|| {
                format!(
                    "--timeout must be a whole number of 1-{} seconds",
                    MAX_TIMEOUT.as_secs()
                )
            })?,
    };
    Ok((
        dir,
        Some(Invoke {
            action,
            settings,
            timeout: Duration::from_secs(seconds),
            request_id: request_id.unwrap_or_else(|| "validate-1".into()),
        }),
    ))
}

/// A fresh temp root removed on drop, also when validation fails.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> std::io::Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!(
            "sdeck-plugin-validate-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Budget {
    files: usize,
    bytes: u64,
}

/// Copies regular files only. Symlinks, special files, oversized trees and unreadable entries
/// are reported as problems instead of being followed.
fn copy_tree(
    from: &Path,
    to: &Path,
    depth: usize,
    budget: &mut Budget,
    problems: &mut Vec<String>,
) {
    let entries = match fs::read_dir(from) {
        Ok(entries) => entries,
        Err(error) => return problems.push(format!("{}: {error}", from.display())),
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            problems.push(format!("{}: unreadable file type", path.display()));
            continue;
        };
        if kind.is_dir() {
            if SKIPPED.iter().any(|skip| name == *skip) {
                continue;
            }
            if depth >= MAX_DEPTH {
                problems.push(format!(
                    "{}: nested deeper than {MAX_DEPTH} levels",
                    path.display()
                ));
            } else if fs::create_dir(to.join(&name)).is_ok() {
                copy_tree(&path, &to.join(&name), depth + 1, budget, problems);
            }
        } else if kind.is_symlink() {
            problems.push(format!(
                "{}: symlinks are not copied; a plugin must ship regular files",
                path.display()
            ));
        } else if kind.is_file() {
            let size = entry.metadata().map_or(u64::MAX, |meta| meta.len());
            budget.files += 1;
            budget.bytes = budget.bytes.saturating_add(size.min(MAX_FILE_BYTES + 1));
            if budget.files > MAX_FILES {
                if budget.files == MAX_FILES + 1 {
                    problems.push(format!("more than {MAX_FILES} files"));
                }
            } else if size > MAX_FILE_BYTES {
                problems.push(format!(
                    "{}: larger than {MAX_FILE_BYTES} bytes",
                    path.display()
                ));
            } else if budget.bytes > MAX_TOTAL_BYTES {
                problems.push(format!(
                    "{}: project exceeds {MAX_TOTAL_BYTES} bytes",
                    path.display()
                ));
            } else if let Err(error) = fs::copy(&path, to.join(&name)) {
                problems.push(format!("{}: {error}", path.display()));
            }
        } else {
            problems.push(format!("{}: not a regular file", path.display()));
        }
    }
}

fn read_manifest(dir: &Path) -> Result<PluginManifest, String> {
    use std::io::Read;
    let path = dir.join("manifest.json");
    let meta =
        fs::symlink_metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !meta.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    PluginManifest::parse(&bytes).map_err(|error| error.to_string())
}

fn name_of<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn summarize(manifest: &PluginManifest) {
    println!("PLUGIN_VALIDATE_OK:");
    println!("  id: {}", manifest.id);
    println!("  name: {}", manifest.name);
    println!("  version: {}", manifest.version);
    println!("  protocol: {}", manifest.protocol_version);
    println!("  executable: {}", manifest.executable);
    for action in &manifest.actions {
        let controls: Vec<String> = action.controls.iter().map(name_of).collect();
        println!(
            "  action: {} ({}) controls: {}",
            action.id,
            action.name,
            controls.join(", ")
        );
        for field in &action.settings {
            let mut value = serde_json::to_value(field).unwrap_or_default();
            let kind = value["type"].as_str().unwrap_or("?").to_owned();
            let required = value["required"].take().as_bool().unwrap_or(false);
            let suffix = if required { ", required" } else { "" };
            println!("    setting: {} ({kind}{suffix})", field.id());
        }
    }
    if manifest.requirements.is_empty() {
        println!("  requirements: none");
    }
    let missing = missing_commands(&manifest.requirements);
    for requirement in &manifest.requirements {
        let mut line = format!(
            "  requirement: {} {}",
            requirement.kind.as_str(),
            requirement.name
        );
        if let Some(version) = &requirement.version {
            line.push_str(&format!(" {version}"));
        }
        if requirement.optional {
            line.push_str(" (optional)");
        }
        if requirement.kind == sdeck_plus::plugins::RequirementKind::Command {
            let found = !missing.contains(&requirement);
            line.push_str(if found {
                " - found on PATH"
            } else {
                " - MISSING from PATH"
            });
        }
        println!("{line}");
    }
}

fn validate(dir: &Path, invoke: Option<&Invoke>) -> bool {
    let manifest = match read_manifest(dir) {
        Ok(manifest) => manifest,
        Err(error) => {
            println!("PLUGIN_VALIDATE_ERROR: {error}");
            return false;
        }
    };
    let root = match TempRoot::new() {
        Ok(root) => root,
        Err(error) => {
            println!("PLUGIN_VALIDATE_ERROR: cannot create a temp root: {error}");
            return false;
        }
    };
    let target = root.0.join(&manifest.id);
    let mut problems = Vec::new();
    if let Err(error) = fs::create_dir(&target) {
        problems.push(format!("{}: {error}", target.display()));
    } else {
        let mut budget = Budget { files: 0, bytes: 0 };
        copy_tree(dir, &target, 0, &mut budget, &mut problems);
    }
    let mut registry = PluginRegistry::open(&root.0);
    problems.extend(registry.problems().iter().map(|p| p.error.to_string()));
    if !problems.is_empty() {
        for problem in &problems {
            println!("PLUGIN_VALIDATE_ERROR: {problem}");
        }
        return false;
    }
    summarize(&manifest);
    let Some(invoke) = invoke else {
        return true;
    };
    println!(
        "PLUGIN_INVOKE_WARNING: --invoke RUNS the plugin `{}` with your user's privileges (you asked for it).",
        manifest.id
    );
    let binding = PluginBinding::new(&manifest.id, &invoke.action, invoke.settings.clone());
    let options = InvokeOptions::new(&invoke.request_id).timeout(invoke.timeout);
    let result = registry
        .set_enabled(&manifest.id, true)
        .and_then(|()| registry.invoke(&binding, &options));
    match result {
        Ok(outcome) => {
            println!(
                "PLUGIN_INVOKE_OK: request {} message: {}",
                outcome.request_id,
                outcome.message.as_deref().unwrap_or("(none)")
            );
            if !outcome.diagnostics.is_empty() {
                println!("  stderr: {}", outcome.diagnostics.trim_end());
            }
            true
        }
        Err(error) => {
            println!("PLUGIN_INVOKE_ERROR: {error}");
            false
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, invoke) = match parse_args(&args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if validate(&dir, invoke.as_ref()) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
