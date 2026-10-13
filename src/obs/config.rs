use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tungstenite::http::Uri;

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// OBS connection settings. Debug output always redacts the password.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObsConfig {
    pub endpoint: String,
    pub password: String,
}

impl Default for ObsConfig {
    fn default() -> Self {
        Self {
            endpoint: "ws://127.0.0.1:4455".into(),
            password: String::new(),
        }
    }
}

impl fmt::Debug for ObsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Even an unvalidated endpoint could contain pasted credentials.
        f.debug_struct("ObsConfig")
            .field("endpoint", &"<local OBS endpoint>")
            .field("password", &"<redacted>")
            .finish()
    }
}

impl ObsConfig {
    /// Checks that the URL addresses only loopback and the password is bounded.
    ///
    /// # Errors
    /// Returns `InvalidInput` for unsupported settings, without echoing secrets.
    pub fn validate(&self) -> io::Result<()> {
        self.address()?;
        if self.password.len() > 4096 {
            return Err(invalid_config());
        }
        Ok(())
    }

    pub(super) fn address(&self) -> io::Result<SocketAddr> {
        if self.endpoint.len() > 1024 || self.endpoint.contains('#') {
            return Err(invalid_config());
        }
        let uri: Uri = self.endpoint.parse().map_err(|_| invalid_config())?;
        if uri.scheme_str() != Some("ws")
            || uri.path() != "/" && !uri.path().is_empty()
            || uri.query().is_some()
            || uri.authority().is_some_and(|a| a.as_str().contains('@'))
        {
            return Err(invalid_config());
        }
        let host = uri.host().ok_or_else(invalid_config)?;
        // Resolve localhost directly, avoiding an unbounded DNS lookup.
        let ip = if host.eq_ignore_ascii_case("localhost") {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<IpAddr>()
                .map_err(|_| invalid_config())?
        };
        let port = match uri.port_u16() {
            Some(port) => port,
            None if uri.authority().is_some_and(|a| a.as_str() == host) => 80,
            None => return Err(invalid_config()),
        };
        if !ip.is_loopback() || port == 0 {
            return Err(invalid_config());
        }
        Ok(SocketAddr::new(ip, port))
    }

    /// Loads `obs.json` from the XDG application data root; missing means default.
    ///
    /// # Errors
    /// Returns an error for insecure permissions, malformed data, or I/O failure.
    pub fn load() -> io::Result<Self> {
        Self::load_at(&default_root()?)
    }

    /// Atomically saves owner-only settings in the XDG application data root.
    ///
    /// # Errors
    /// Returns an error for invalid settings, insecure roots, or I/O failure.
    pub fn save(&self) -> io::Result<()> {
        self.save_at(&default_root()?)
    }

    /// Loads settings from an explicitly supplied application root.
    ///
    /// # Errors
    /// Returns an error for insecure files, invalid settings, or I/O failure.
    pub fn load_at(root: &Path) -> io::Result<Self> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join("obs.json"))
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        check_owner(&file.metadata()?, true)?;
        let mut bytes = Vec::new();
        file.take(16_385).read_to_end(&mut bytes)?;
        if bytes.len() > 16_384 {
            return Err(invalid_config());
        }
        let config: Self = serde_json::from_slice(&bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid OBS configuration"))?;
        config.validate()?;
        Ok(config)
    }

    /// Saves settings at an explicit root, replacing `obs.json` atomically.
    ///
    /// # Errors
    /// Returns an error for invalid settings, insecure roots, or I/O failure.
    pub fn save_at(&self, root: &Path) -> io::Result<()> {
        self.validate()?;
        DirBuilder::new().recursive(true).mode(0o700).create(root)?;
        let metadata = fs::symlink_metadata(root)?;
        check_owner(&metadata, false)?;
        if !metadata.is_dir() || metadata.mode() & 0o022 != 0 {
            return Err(insecure_config());
        }
        let temp = root.join(format!(
            ".obs-{}-{}.tmp",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        let result = (|| {
            serde_json::to_writer(&mut file, self).map_err(|_| invalid_config())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temp, root.join("obs.json"))?;
            File::open(root)?.sync_all()
        })();
        if result.is_err() {
            match fs::remove_file(temp) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        result
    }
}

fn default_root() -> io::Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|p| Path::new(p).is_absolute())
                .map(|p| PathBuf::from(p).join(".local/share"))
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no XDG data root available"))?;
    Ok(base.join("sdeck-plus"))
}

fn check_owner(metadata: &fs::Metadata, regular_file: bool) -> io::Result<()> {
    // /proc/self is owned by this process's effective user on Linux.
    let owner = fs::metadata("/proc/self")?.uid();
    if metadata.uid() != owner
        || regular_file && (!metadata.is_file() || metadata.mode() & 0o077 != 0)
    {
        return Err(insecure_config());
    }
    Ok(())
}

fn invalid_config() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "OBS requires a local ws:// endpoint and bounded credentials",
    )
}

fn insecure_config() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "OBS settings must be owner-only in a trusted root",
    )
}
