//! Author tooling: validates a plugin PROJECT directory (manifest.json plus its executable at the
//! directory root) the way the app would load it, without running anything.
//!
//! ```text
//! cargo run --offline --example plugin_validate -- <plugin-dir>
//! cargo run --offline --example plugin_validate -- <plugin-dir> --invoke <action> \
//!     [--settings '<json>'] [--timeout <seconds, default 5, max 60>] [--request-id <id>]
//! cargo run --offline --example plugin_validate -- <plugin-dir> --query <action> [same options]
//! ```
//!
//! This is a thin client of `sdeck_plus::plugins::stage`, the same code the app's Add plugin flow
//! uses: the project is copied (regular files only, bounded, every copy problem reported) into a
//! private staging directory and loaded with `PluginRegistry::open`, which executes nothing.
//! Output markers: `PLUGIN_VALIDATE_OK:` / `PLUGIN_VALIDATE_ERROR:` and, only with `--invoke`,
//! `PLUGIN_INVOKE_OK:` / `PLUGIN_INVOKE_ERROR:`, and only with `--query`, `PLUGIN_QUERY_OK:` /
//! `PLUGIN_QUERY_ERROR:` (asks the action's plugin for the value it displays). Exit status: 0 ok,
//! 1 problems or a failed request, 2 usage. `--invoke` and `--query` RUN the plugin with your
//! privileges; nothing else here ever does.

use sdeck_plus::plugins::{
    install, stage, CancelToken, InstallMode, InvokeOptions, PluginBinding, PluginPreview,
    PluginRegistry, PluginSource, RequirementKind, Settings, StageError, StageOptions, MAX_TIMEOUT,
};
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "usage: plugin_validate <plugin-dir> [--invoke|--query <action> \
[--settings '<json>'] [--timeout <1-60 seconds>] [--request-id <id>]]";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Invoke,
    Query,
}

struct Invoke {
    mode: Mode,
    action: String,
    settings: Settings,
    timeout: Duration,
    request_id: String,
}

fn parse_args(args: &[String]) -> Result<(PathBuf, Option<Invoke>), String> {
    let mut dir = None;
    let mut mode = None;
    let (mut action, mut settings, mut timeout, mut request_id) = (None, None, None, None);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--invoke" | "--query" => {
                if action.is_some() {
                    return Err("--invoke and --query are mutually exclusive".into());
                }
                mode = Some(if arg == "--invoke" {
                    Mode::Invoke
                } else {
                    Mode::Query
                });
                action = Some(value(arg)?);
            }
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
            return Err("--settings, --timeout and --request-id need --invoke or --query".into());
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
            mode: mode.unwrap_or(Mode::Invoke),
            action,
            settings,
            timeout: Duration::from_secs(seconds),
            request_id: request_id.unwrap_or_else(|| "validate-1".into()),
        }),
    ))
}

/// A fresh private temp plugins root removed on drop, also when anything fails.
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
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn name_of<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn summarize(preview: &PluginPreview, executable: &str) {
    println!("PLUGIN_VALIDATE_OK:");
    println!("  id: {}", preview.id);
    println!("  name: {}", preview.name);
    println!("  version: {}", preview.version);
    println!("  protocol: {}", preview.protocol_version);
    println!("  executable: {executable}");
    for action in &preview.actions {
        let controls: Vec<String> = action.controls.iter().map(name_of).collect();
        println!(
            "  action: {} ({}) controls: {}",
            action.id,
            action.name,
            controls.join(", ")
        );
        for setting in &action.settings {
            let suffix = if setting.required { ", required" } else { "" };
            println!("    setting: {} ({}{suffix})", setting.id, setting.kind);
        }
        if let Some(display) = &action.display {
            println!("    display: {display}");
        }
    }
    for value in &preview.values {
        println!("  value: {} ({})", value.id, value.label);
    }
    if preview.requirements.is_empty() {
        println!("  requirements: none");
    }
    for entry in &preview.requirements {
        let requirement = &entry.requirement;
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
        if requirement.kind == RequirementKind::Command {
            line.push_str(if entry.missing {
                " - MISSING from PATH"
            } else {
                " - found on PATH"
            });
        }
        println!("{line}");
    }
}

fn report(error: &StageError) {
    match error {
        StageError::Invalid(problems) => {
            for problem in problems {
                println!("PLUGIN_VALIDATE_ERROR: {problem}");
            }
        }
        other => println!("PLUGIN_VALIDATE_ERROR: {other}"),
    }
}

/// The project as a stage source: an existing directory (relative paths are fine here).
fn source_of(dir: &Path) -> Result<PluginSource, String> {
    let resolved = fs::canonicalize(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    if !resolved.is_dir() {
        return Err(format!("{} is not a folder", resolved.display()));
    }
    Ok(PluginSource::Folder(resolved))
}

fn validate(dir: &Path, invoke: Option<&Invoke>) -> bool {
    let source = match source_of(dir) {
        Ok(source) => source,
        Err(error) => {
            println!("PLUGIN_VALIDATE_ERROR: {error}");
            return false;
        }
    };
    let staged = match stage(&source, &StageOptions::new(), &CancelToken::new()) {
        Ok(staged) => staged,
        Err(error) => {
            report(&error);
            return false;
        }
    };
    summarize(staged.preview(), &staged.manifest().executable);
    let Some(invoke) = invoke else {
        return true;
    };
    let id = staged.manifest().id.clone();
    let (flag, marker) = match invoke.mode {
        Mode::Invoke => ("--invoke", "PLUGIN_INVOKE_WARNING"),
        Mode::Query => ("--query", "PLUGIN_QUERY_WARNING"),
    };
    println!(
        "{marker}: {flag} RUNS the plugin `{id}` with your user's privileges (you asked for it).",
    );
    // Only this private temp root is ever written to or enabled in, never the user's plugins.
    let result = TempRoot::new()
        .map_err(|error| format!("cannot create a temp root: {error}"))
        .and_then(|root| {
            install(&staged, &root.0, InstallMode::New).map_err(|error| error.to_string())?;
            let mut registry = PluginRegistry::open(&root.0);
            let binding = PluginBinding::new(&id, &invoke.action, invoke.settings.clone());
            let options = InvokeOptions::new(&invoke.request_id).timeout(invoke.timeout);
            registry
                .set_enabled(&id, true)
                .map_err(|error| error.to_string())?;
            match invoke.mode {
                Mode::Invoke => registry
                    .invoke(&binding, &options)
                    .map(|outcome| {
                        let mut text = format!(
                            "PLUGIN_INVOKE_OK: request {} message: {}",
                            outcome.request_id,
                            outcome.message.as_deref().unwrap_or("(none)")
                        );
                        if !outcome.diagnostics.is_empty() {
                            text.push_str(&format!(
                                "\n  stderr: {}",
                                outcome.diagnostics.trim_end()
                            ));
                        }
                        text
                    })
                    .map_err(|error| error.to_string()),
                Mode::Query => registry
                    .query(&binding, &options)
                    .map(|value| {
                        format!(
                            "PLUGIN_QUERY_OK: request {} headline: {} detail: {} level: {}",
                            invoke.request_id,
                            value.headline,
                            value.detail.as_deref().unwrap_or("(none)"),
                            value.level.map_or("(none)".to_owned(), |n| n.to_string())
                        )
                    })
                    .map_err(|error| error.to_string()),
            }
        });
    let kind = if invoke.mode == Mode::Query {
        "QUERY"
    } else {
        "INVOKE"
    };
    match result {
        Ok(text) => {
            println!("{text}");
            true
        }
        Err(error) => {
            println!("PLUGIN_{kind}_ERROR: {error}");
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
