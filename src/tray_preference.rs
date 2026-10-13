use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Preference {
    keep_running: bool,
}

pub(super) fn path() -> io::Result<PathBuf> {
    config_home(
        std::env::var_os("XDG_CONFIG_HOME")
            .as_deref()
            .map(Path::new),
        std::env::var_os("HOME").as_deref().map(Path::new),
    )
    .map(|root| root.join("sdeck-plus").join("tray.json"))
}

fn config_home(xdg: Option<&Path>, home: Option<&Path>) -> io::Result<PathBuf> {
    match xdg.filter(|path| path.is_absolute()) {
        Some(path) => Ok(path.to_path_buf()),
        None => home
            .filter(|path| path.is_absolute())
            .map(|path| path.join(".config"))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no absolute config home")),
    }
}

pub(super) fn load(path: &Path) -> io::Result<bool> {
    match fs::read(path) {
        Ok(contents) => serde_json::from_slice::<Preference>(&contents)
            .map(|preference| preference.keep_running)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

pub(super) fn save(path: &Path, keep_running: bool) -> io::Result<()> {
    // Refuse to replace unreadable or malformed originals, including changes
    // made outside the application after startup.
    load(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing config directory"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        serde_json::to_writer(&mut file, &Preference { keep_running })?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        if let Err(error) = fs::remove_file(&temporary) {
            eprintln!("Could not remove temporary tray preference: {error}");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn absolute_xdg_or_home_paths_only() {
        assert_eq!(
            config_home(Some(Path::new("/config")), Some(Path::new("/home/test"))).unwrap(),
            PathBuf::from("/config")
        );
        assert_eq!(
            config_home(Some(Path::new("relative")), Some(Path::new("/home/test"))).unwrap(),
            PathBuf::from("/home/test/.config")
        );
        assert!(config_home(None, Some(Path::new("relative"))).is_err());
    }

    #[test]
    fn default_persistence_and_failed_writes_preserve_original() {
        let root = std::env::temp_dir().join(format!(
            "sdeck-tray-preference-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let path = root.join("tray.json");
        assert!(load(&path).unwrap());
        assert!(!path.exists());
        save(&path, false).unwrap();
        assert!(!load(&path).unwrap());
        let original = fs::read(&path).unwrap();
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, b"another writer").unwrap();
        assert!(save(&path, true).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read(&temporary).unwrap(), b"another writer");
        fs::remove_file(&temporary).unwrap();
        save(&path, true).unwrap();
        assert!(load(&path).unwrap());
        for malformed in [
            b"not json".as_slice(),
            b"{}",
            b"{\"keep_running\":false,\"unknown\":1}",
        ] {
            fs::write(&path, malformed).unwrap();
            assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert!(save(&path, false).is_err());
            assert_eq!(fs::read(&path).unwrap(), malformed);
            assert!(!temporary.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
