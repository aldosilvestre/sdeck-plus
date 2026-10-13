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
}
