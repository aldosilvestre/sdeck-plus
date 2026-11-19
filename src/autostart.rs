//! Per-user XDG session autostart, independent of the launch working directory.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

const ENTRY_NAME: &str = "sdeck-plus.desktop";

/// Whether our session autostart entry exists and is not marked `Hidden=true`.
///
/// # Errors
/// Returns configuration, read, UTF-8, or malformed-entry errors. Only a missing
/// entry is treated as disabled.
pub fn is_enabled() -> io::Result<bool> {
    is_enabled_at(&config_home(
        env::var_os("XDG_CONFIG_HOME").as_deref().map(Path::new),
        env::var_os("HOME").as_deref().map(Path::new),
    )?)
}

/// Uses the executable path captured at startup, or removes our entry when disabling.
///
/// # Errors
/// Returns configuration, executable-path, or filesystem errors. Removing a
/// missing entry succeeds. The parent binary must implement `--background`.
pub fn set_enabled(enabled: bool, executable: &Path) -> io::Result<()> {
    let root = config_home(
        env::var_os("XDG_CONFIG_HOME").as_deref().map(Path::new),
        env::var_os("HOME").as_deref().map(Path::new),
    )?;
    if enabled {
        enable_at(&root, executable)
    } else {
        disable_at(&root)
    }
}

fn config_home(xdg: Option<&Path>, home: Option<&Path>) -> io::Result<PathBuf> {
    // XDG requires absolute paths; empty and relative values are ignored.
    match xdg.filter(|path| path.is_absolute()) {
        Some(path) => Ok(path.to_path_buf()),
        None => match home.filter(|path| path.is_absolute()) {
            Some(path) => Ok(path.join(".config")),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "neither XDG_CONFIG_HOME nor HOME provides an absolute config path",
            )),
        },
    }
}

fn is_enabled_at(root: &Path) -> io::Result<bool> {
    let contents = match fs::read_to_string(root.join("autostart").join(ENTRY_NAME)) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut in_entry = false;
    let mut found_entry = false;
    let mut hidden = None;
    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_entry = line == "[Desktop Entry]";
            if in_entry && found_entry {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate Desktop Entry group",
                ));
            }
            found_entry |= in_entry;
            continue;
        }
        if in_entry {
            let (key, value) = line.split_once('=').ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid desktop entry key")
            })?;
            if key.trim() == "Hidden" {
                let value = match value.trim() {
                    "true" => true,
                    "false" => false,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid Hidden boolean",
                        ))
                    }
                };
                if hidden.replace(value).is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "duplicate Hidden key",
                    ));
                }
            }
        }
    }
    if !found_entry {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Desktop Entry group",
        ));
    }
    Ok(!hidden.unwrap_or(false))
}

fn enable_at(root: &Path, executable: &Path) -> io::Result<()> {
    let executable = executable
        .to_str()
        .filter(|value| executable.is_absolute() && !value.contains('='))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "desktop executable must be an absolute UTF-8 path without '='",
            )
        })?;
    let mut quoted = String::from("\"");
    for character in executable.chars() {
        // String-value escapes are decoded before Exec argument quoting.
        match character {
            '\\' => quoted.push_str(r"\\\\"),
            '"' | '`' | '$' => {
                quoted.push_str(r"\\");
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            '\n' => quoted.push_str(r"\n"),
            '\r' => quoted.push_str(r"\r"),
            '\t' => quoted.push_str(r"\t"),
            character if character.is_control() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsupported control character in desktop executable",
                ))
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    let name = crate::APP_NAME;
    let contents = format!(
        "[Desktop Entry]\nType=Application\nName={name}\n\
         Exec={quoted} --background\nTerminal=false\nHidden=false\n"
    );
    let directory = root.join("autostart");
    fs::create_dir_all(&directory)?;
    fs::write(directory.join(ENTRY_NAME), contents)
}

/// Repoints an enabled entry whose executable no longer exists at `executable`.
///
/// This happens when sdeck-plus moves between `install.sh` (`~/.local/bin`) and
/// a pacman package (`/usr/bin`): session startup then silently skips the
/// entry. Entries that still start something, disabled, symlinked or
/// unparseable entries, a running executable that no longer exists (Linux names
/// it `<path> (deleted)`) and Cargo builds (`target/debug`, `target/release`)
/// are left alone.
///
/// # Errors
/// Returns configuration, read, malformed-entry or write errors.
pub fn repair_missing_executable(executable: &Path) -> io::Result<bool> {
    repair_at(
        &config_home(
            env::var_os("XDG_CONFIG_HOME").as_deref().map(Path::new),
            env::var_os("HOME").as_deref().map(Path::new),
        )?,
        executable,
    )
}

fn repair_at(root: &Path, executable: &Path) -> io::Result<bool> {
    let entry = root.join("autostart").join(ENTRY_NAME);
    // Writing through a symlink would change a file owned elsewhere.
    if is_cargo_build(executable)
        || fs::metadata(executable).is_err()
        || fs::symlink_metadata(&entry).is_ok_and(|metadata| metadata.file_type().is_symlink())
        || !is_enabled_at(root)?
    {
        return Ok(false);
    }
    let contents = fs::read_to_string(&entry)?;
    let Some(program) = entry_program(&contents) else {
        return Ok(false);
    };
    match fs::metadata(&program) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        _ => return Ok(false),
    }
    enable_at(root, executable)?;
    Ok(true)
}

fn is_cargo_build(executable: &Path) -> bool {
    let mut parents = executable.ancestors().skip(1);
    let in_profile = parents
        .next()
        .and_then(Path::file_name)
        .is_some_and(|name| ["debug", "release", "deps"].iter().any(|dir| name == *dir));
    in_profile && parents.any(|parent| parent.file_name().is_some_and(|name| name == "target"))
}

/// The absolute program of the entry's `Exec` key, with both escape layers decoded.
fn entry_program(contents: &str) -> Option<PathBuf> {
    let mut in_entry = false;
    let value = contents.lines().map(str::trim).find_map(|line| {
        if line.starts_with('[') && line.ends_with(']') {
            in_entry = line == "[Desktop Entry]";
            return None;
        }
        let (key, value) = line.split_once('=')?;
        (in_entry && key.trim() == "Exec").then_some(value.trim_start())
    })?;
    // Desktop string escapes are decoded before Exec argument quoting.
    let mut decoded = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        decoded.push(match characters.next()? {
            's' => ' ',
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '\\' => '\\',
            _ => return None,
        });
    }
    let mut program = String::new();
    let mut characters = decoded.chars();
    if decoded.starts_with('"') {
        characters.next();
        loop {
            match characters.next()? {
                '"' => break,
                '\\' => program.push(characters.next()?),
                '%' => program.push(characters.next().filter(|next| *next == '%')?),
                character => program.push(character),
            }
        }
    } else {
        for character in characters.by_ref() {
            if character == ' ' {
                break;
            }
            program.push(character);
        }
    }
    // Reject an unterminated quote in the remaining arguments.
    if characters.filter(|character| *character == '"').count() % 2 != 0 {
        return None;
    }
    Some(PathBuf::from(program)).filter(|path| path.is_absolute())
}

fn disable_at(root: &Path) -> io::Result<()> {
    match fs::remove_file(root.join("autostart").join(ENTRY_NAME)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            loop {
                let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = env::temp_dir()
                    .join(format!("streamdeck-autostart-{}-{id}", std::process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("could not create test directory: {error}"),
                }
            }
        }

        fn entry(&self) -> PathBuf {
            self.0.join("autostart").join(ENTRY_NAME)
        }

        fn write(&self, contents: impl AsRef<[u8]>) {
            fs::create_dir_all(self.0.join("autostart")).unwrap();
            fs::write(self.entry(), contents).unwrap();
        }

        /// Creates a regular file standing in for an executable and returns its path.
        fn executable(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "binary").unwrap();
            path
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn config_path_uses_absolute_xdg_or_home_fallback() {
        // Given: explicit paths, with no process environment changes.
        let home = Some(Path::new("/test/home"));
        // When / Then: XDG precedence, invalid XDG fallback, and missing HOME.
        assert_eq!(
            config_home(Some(Path::new("/xdg")), home).unwrap(),
            Path::new("/xdg")
        );
        for xdg in [None, Some(Path::new("")), Some(Path::new("relative"))] {
            assert_eq!(
                config_home(xdg, home).unwrap(),
                Path::new("/test/home/.config")
            );
        }
        assert!(config_home(None, None).is_err());
        assert!(config_home(None, Some(Path::new("relative"))).is_err());
    }

    #[test]
    fn missing_entry_is_disabled() {
        // Given
        let root = TempRoot::new();
        // When / Then
        assert!(!is_enabled_at(&root.0).unwrap());
    }

    #[test]
    fn hidden_is_read_only_from_desktop_entry_group() {
        // Given
        for (contents, enabled) in [
            ("[Desktop Entry]\nHidden=true\n", false),
            ("[Desktop Entry]\n Hidden = false \n", true),
            (
                "[Desktop Entry]\n# Hidden=true\n[Other]\nHidden=true\n",
                true,
            ),
        ] {
            let root = TempRoot::new();
            root.write(contents);
            // When / Then
            assert_eq!(is_enabled_at(&root.0).unwrap(), enabled);
        }
    }

    #[test]
    fn malformed_and_unreadable_entries_return_errors() {
        // Given
        for contents in [
            &b"[Desktop Entry]\nHidden=maybe\n"[..],
            &b"[Desktop Entry]\nHidden=true\nHidden=false\n"[..],
            &b"[Desktop Entry]\n[Desktop Entry]\n"[..],
            &b"[Desktop Entry]\ninvalid\n"[..],
            &b"not a desktop entry\n"[..],
            &b"\xff"[..],
        ] {
            let root = TempRoot::new();
            root.write(contents);
            // When / Then
            assert_eq!(
                is_enabled_at(&root.0).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        let root = TempRoot::new();
        fs::create_dir_all(root.entry()).unwrap();
        assert!(is_enabled_at(&root.0).is_err());
    }

    #[test]
    fn enabling_writes_quoted_background_exec_and_replaces_hidden_entry() {
        // Given
        let root = TempRoot::new();
        root.write("[Desktop Entry]\nHidden=true\n");
        // When
        enable_at(&root.0, Path::new("/apps/a b\"\\$`%f';雪\n\t\r")).unwrap();
        // Then: both escape layers, literal percent, and one background argument.
        let contents = fs::read_to_string(root.entry()).unwrap();
        assert_eq!(
            contents
                .lines()
                .find(|line| line.starts_with("Exec="))
                .unwrap(),
            concat!(
                "Exec=\"/apps/a b",
                r#"\\"\\\\\\$\\`%%f';雪\n\t\r"#,
                "\" --background"
            )
        );
        assert!(is_enabled_at(&root.0).unwrap());
    }

    #[test]
    fn invalid_executable_does_not_create_entry() {
        // Given
        for path in ["relative", "/apps/a=b", "/apps/a\0b"] {
            let root = TempRoot::new();
            // When / Then
            assert_eq!(
                enable_at(&root.0, Path::new(path)).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert!(!root.entry().exists());
        }
        use std::os::unix::ffi::OsStrExt;
        let root = TempRoot::new();
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/apps/\xff"));
        assert_eq!(
            enable_at(&root.0, path).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn disabling_removes_only_our_entry() {
        // Given
        let root = TempRoot::new();
        root.write("[Desktop Entry]\n");
        let other = root.0.join("autostart/other.desktop");
        fs::write(&other, "other").unwrap();
        // When
        disable_at(&root.0).unwrap();
        // Then
        assert!(!root.entry().exists());
        assert_eq!(fs::read_to_string(other).unwrap(), "other");
    }

    #[test]
    fn missing_removal_succeeds_and_filesystem_errors_propagate() {
        // Given
        let root = TempRoot::new();
        // When / Then
        disable_at(&root.0).unwrap();
        fs::create_dir_all(root.entry()).unwrap();
        assert!(disable_at(&root.0).is_err());
        let blocked = root.0.join("blocked");
        fs::write(&blocked, "not a directory").unwrap();
        assert!(enable_at(&blocked, Path::new("/apps/monitor")).is_err());
        assert!(is_enabled_at(&blocked).is_err());
    }

    const STALE: &str = "[Desktop Entry]\nType=Application\nName=sdeck-plus\n\
                         Exec=\"/usr/bin/sdeck-plus-removed\" --background\nTerminal=false\nHidden=false\n";

    #[test]
    fn entry_whose_executable_is_gone_is_repointed_at_the_running_one() {
        // Given: a package or install.sh build replaced the executable at another path.
        let root = TempRoot::new();
        root.write(STALE);
        let current = root.executable(".local/bin/sdeck-plus");
        // When
        let repaired = repair_at(&root.0, &current).unwrap();
        // Then
        assert!(repaired);
        let fresh = TempRoot::new();
        enable_at(&fresh.0, &current).unwrap();
        assert_eq!(
            fs::read_to_string(root.entry()).unwrap(),
            fs::read_to_string(fresh.entry()).unwrap()
        );
        assert!(is_enabled_at(&root.0).unwrap());
    }

    #[test]
    fn entries_that_still_start_something_or_are_off_are_left_alone() {
        // An existing executable, also with both escape layers in its path.
        let root = TempRoot::new();
        let current = root.executable("sdeck-plus");
        let existing = root.executable("a b\"$`%f");
        enable_at(&root.0, &existing).unwrap();
        let before = fs::read_to_string(root.entry()).unwrap();
        assert!(!repair_at(&root.0, &current).unwrap());
        assert_eq!(fs::read_to_string(root.entry()).unwrap(), before);
        // Disabled, missing, PATH-relative, or unparseable entries.
        for contents in [
            STALE.replace("Hidden=false", "Hidden=true"),
            STALE.replace("\"/usr/bin/sdeck-plus-removed\"", "sdeck-plus-removed"),
            STALE.replace("Exec=\"/usr/bin/sdeck-plus-removed\" --background\n", ""),
            STALE.replace("--background", "\"--background"),
        ] {
            let root = TempRoot::new();
            let current = root.executable("sdeck-plus");
            root.write(&contents);
            assert!(!repair_at(&root.0, &current).unwrap(), "{contents}");
            assert_eq!(fs::read_to_string(root.entry()).unwrap(), contents);
        }
        let root = TempRoot::new();
        let current = root.executable("sdeck-plus");
        assert!(!repair_at(&root.0, &current).unwrap());
        assert!(!root.entry().exists());
    }

    #[test]
    fn a_running_executable_that_no_longer_exists_is_never_written() {
        // Given: Linux names a replaced running executable "<path> (deleted)".
        let root = TempRoot::new();
        root.write(STALE);
        root.executable("sdeck-plus");
        for current in [root.0.join("sdeck-plus (deleted)"), root.0.join("missing")] {
            // When / Then
            assert!(!repair_at(&root.0, &current).unwrap(), "{current:?}");
            assert_eq!(fs::read_to_string(root.entry()).unwrap(), STALE);
        }
    }

    #[test]
    fn a_symlinked_entry_is_left_alone() {
        // Given: the entry links to a stale file that is owned elsewhere.
        use std::os::unix::fs::symlink;
        let root = TempRoot::new();
        let current = root.executable("sdeck-plus");
        let target = root.0.join("elsewhere.desktop");
        fs::write(&target, STALE).unwrap();
        fs::create_dir_all(root.0.join("autostart")).unwrap();
        symlink(&target, root.entry()).unwrap();
        // When / Then
        assert!(!repair_at(&root.0, &current).unwrap());
        assert_eq!(fs::read_to_string(&target).unwrap(), STALE);
        assert!(fs::symlink_metadata(root.entry())
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn repair_reads_back_every_path_that_enabling_escapes() {
        for name in [
            "a b",
            "q\"uote",
            "dol$lar",
            "back\\slash",
            "per%cent",
            "pct%%",
            "tick`",
            "tab\tx",
            "\\s",
            "é ü",
        ] {
            // Given: an entry written for an existing executable with this name.
            let root = TempRoot::new();
            let current = root.executable("current");
            let existing = root.executable(name);
            enable_at(&root.0, &existing).unwrap();
            let before = fs::read(root.entry()).unwrap();
            // When / Then: it still starts something, so it stays as written.
            assert!(!repair_at(&root.0, &current).unwrap(), "{name:?}");
            assert_eq!(fs::read(root.entry()).unwrap(), before, "{name:?}");
            // When / Then: once that executable is gone, it is repointed.
            fs::remove_file(&existing).unwrap();
            assert!(repair_at(&root.0, &current).unwrap(), "{name:?}");
            let fresh = TempRoot::new();
            enable_at(&fresh.0, &current).unwrap();
            assert_eq!(
                fs::read(root.entry()).unwrap(),
                fs::read(fresh.entry()).unwrap(),
                "{name:?}"
            );
        }
    }

    #[test]
    fn malformed_exec_values_are_never_rewritten() {
        for contents in [
            "[Desktop Entry]\nExec=\n",
            "[Desktop Entry]\nExec=\"\n",
            "[Desktop Entry]\nExec=\\\n",
            "[Desktop Entry]\nExec=\\q\n",
            "[Desktop Entry]\nExec=\"/nonexist%\"\n",
            "[Desktop Entry]\nExec=\"\"\n",
            "[Desktop Entry]\nHidden=false\n[Other]\nExec=/nonexist\n",
        ] {
            // Given
            let root = TempRoot::new();
            let current = root.executable("sdeck-plus");
            root.write(contents);
            // When / Then: no repair (or an error), and the entry is unchanged.
            assert!(
                !repair_at(&root.0, &current).unwrap_or(false),
                "{contents:?}"
            );
            assert_eq!(fs::read_to_string(root.entry()).unwrap(), contents);
        }
    }

    #[test]
    fn cargo_builds_never_take_over_the_login_entry() {
        for current in [
            "src/sdeck-plus/target/debug/sdeck-plus",
            "src/sdeck-plus/target/release/sdeck-plus",
            "src/sdeck-plus/target/x86_64-unknown-linux-gnu/release/sdeck-plus",
            "src/sdeck-plus/target/debug/deps/sdeck_plus-0123abcd",
        ] {
            let root = TempRoot::new();
            root.write(STALE);
            let executable = root.executable(current);
            assert!(!repair_at(&root.0, &executable).unwrap(), "{current}");
            assert_eq!(fs::read_to_string(root.entry()).unwrap(), STALE);
        }
    }
}
