//! The virtual deck's remembered settings (S4 password hash, S5 port):
//! `virtual-deck.json` in the app's config directory, validated when read,
//! replaced atomically, private (0600), and never overwritten while invalid.
//!
//! Every test works in its own scratch directory under the system temp dir;
//! nothing reads or writes the user's configuration.
use sdeck_plus::virtual_deck::{
    hash_password,
    settings::{check_port, config_dir_from, parse_port, Settings, SettingsStore, FILE_NAME},
    DEFAULT_PORT,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
};

const PORT_RANGE: &str = "Use a port from 1024 to 65535.";

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A scratch config directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sdeck-virtual-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join(FILE_NAME)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One Argon2id hash for the whole binary: hashing is deliberately slow.
fn hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password("correct horse").unwrap())
        .clone()
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Every entry of `dir`, sorted, so a stray temporary file shows up.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_file_is_virtual_deck_json() {
    assert_eq!(FILE_NAME, "virtual-deck.json");
}

#[test]
fn without_a_file_the_store_offers_port_8765_and_no_password_and_writes_nothing() {
    let scratch = Scratch::new();
    let store = SettingsStore::open(&scratch.0);
    assert_eq!(
        store.settings(),
        &Settings {
            port: DEFAULT_PORT,
            password_hash: None,
        }
    );
    assert_eq!(DEFAULT_PORT, 8765);
    assert_eq!(store.error(), None);
    assert_eq!(store.path(), scratch.file());
    assert!(entries(&scratch.0).is_empty(), "opening creates nothing");
}

#[test]
fn a_password_and_a_port_round_trip_through_a_private_file() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_password_hash(hash()).unwrap();
    store.set_port(9123).unwrap();
    assert_eq!(store.settings().port, 9123);
    assert_eq!(
        store.settings().password_hash.as_deref(),
        Some(hash().as_str())
    );
    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(scratch.file()).unwrap()).unwrap();
    assert_eq!(
        written,
        serde_json::json!({"port": 9123, "password_hash": hash()})
    );
    assert_eq!(mode(&scratch.file()), 0o600, "the file is owner-only");
    assert_eq!(
        entries(&scratch.0),
        [FILE_NAME],
        "no temporary file is left"
    );
    let reopened = SettingsStore::open(&scratch.0);
    assert_eq!(reopened.error(), None);
    assert_eq!(reopened.settings(), store.settings());
}

#[test]
fn a_missing_config_directory_is_created_private() {
    let scratch = Scratch::new();
    let dir = scratch.0.join("config/sdeck-plus");
    let mut store = SettingsStore::open(&dir);
    store.set_password_hash(hash()).unwrap();
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join(FILE_NAME)), 0o600);
}

#[test]
fn the_password_is_stored_only_as_its_hash() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_password_hash(hash()).unwrap();
    let text = fs::read_to_string(scratch.file()).unwrap();
    assert!(!text.contains("correct horse"));
    assert!(text.contains("$argon2id$"));
}

#[test]
fn a_hash_that_is_not_argon2id_is_refused_and_nothing_is_written() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    let error = store
        .set_password_hash("correct horse".to_string())
        .unwrap_err();
    assert_eq!(error, "The password hash is not valid.");
    assert_eq!(store.settings().password_hash, None);
    assert!(entries(&scratch.0).is_empty());
}

#[test]
fn ports_from_1024_to_65535_are_accepted_and_others_refused() {
    for port in [1024, 8765, 65535] {
        assert_eq!(check_port(port), Ok(()), "{port}");
    }
    for port in [0, 80, 1023] {
        assert_eq!(check_port(port), Err(PORT_RANGE.to_string()), "{port}");
    }
    assert_eq!(parse_port(" 8765 "), Ok(8765));
    assert_eq!(parse_port("1024"), Ok(1024));
    for text in ["", "abc", "1023", "65536", "70000", "-1", "80.5"] {
        assert_eq!(parse_port(text), Err(PORT_RANGE.to_string()), "{text:?}");
    }
}

#[test]
fn an_out_of_range_port_changes_nothing() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_password_hash(hash()).unwrap();
    let before = fs::read(scratch.file()).unwrap();
    assert_eq!(store.set_port(1023), Err(PORT_RANGE.to_string()));
    assert_eq!(store.settings().port, DEFAULT_PORT);
    assert_eq!(fs::read(scratch.file()).unwrap(), before);
}

/// Invalid contents: kept byte for byte, reported, and every write refused.
fn assert_kept_and_refused(contents: &[u8], why: &str) {
    let scratch = Scratch::new();
    fs::write(scratch.file(), contents).unwrap();
    let mut store = SettingsStore::open(&scratch.0);
    let error = store
        .error()
        .unwrap_or_else(|| panic!("{why}: no error"))
        .to_string();
    assert!(error.contains(FILE_NAME), "{why}: {error}");
    assert!(error.contains("kept"), "{why}: {error}");
    assert_eq!(store.settings(), &Settings::default(), "{why}");
    assert_eq!(store.set_port(9000).unwrap_err(), error, "{why}");
    assert_eq!(
        store.set_password_hash(hash()).unwrap_err(),
        store.error().unwrap(),
        "{why}"
    );
    assert_eq!(fs::read(scratch.file()).unwrap(), contents, "{why}");
    assert_eq!(entries(&scratch.0), [FILE_NAME], "{why}");
}

#[test]
fn an_invalid_file_is_kept_and_every_write_is_refused() {
    assert_kept_and_refused(b"not json", "malformed");
    assert_kept_and_refused(b"{\"port\":80}", "port below 1024");
    assert_kept_and_refused(b"{\"port\":70000}", "port above 65535");
    assert_kept_and_refused(
        b"{\"port\":8765,\"password_hash\":\"plain text\"}",
        "not a hash",
    );
    let extra = format!(
        "{{\"port\":8765,\"password_hash\":\"{}\",\"password\":\"x\"}}",
        hash()
    );
    assert_kept_and_refused(extra.as_bytes(), "unknown field");
    assert_kept_and_refused(b"{\"password_hash\":null}", "no port");
}

#[test]
fn a_file_that_turns_invalid_after_opening_is_not_overwritten() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_password_hash(hash()).unwrap();
    fs::write(scratch.file(), b"edited by hand").unwrap();
    let error = store.set_port(9000).unwrap_err();
    assert!(error.contains(FILE_NAME), "{error}");
    assert_eq!(fs::read(scratch.file()).unwrap(), b"edited by hand");
    assert_eq!(store.error(), Some(error.as_str()), "writes stay refused");
    assert_eq!(store.settings().port, DEFAULT_PORT);
}

#[test]
fn the_config_directory_follows_xdg_then_home() {
    let xdg = Path::new("/xdg/config");
    let home = Path::new("/home/someone");
    assert_eq!(
        config_dir_from(Some(xdg), Some(home)).unwrap(),
        Path::new("/xdg/config/sdeck-plus")
    );
    assert_eq!(
        config_dir_from(Some(Path::new("relative")), Some(home)).unwrap(),
        Path::new("/home/someone/.config/sdeck-plus")
    );
    assert_eq!(
        config_dir_from(None, Some(home)).unwrap(),
        Path::new("/home/someone/.config/sdeck-plus")
    );
    assert!(config_dir_from(None, Some(Path::new("relative"))).is_err());
    assert!(config_dir_from(None, None).is_err());
}
