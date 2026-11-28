//! The virtual deck's remembered settings (S5 port; S4 says the password is
//! never among them): `virtual-deck.json` in the app's config directory holds
//! the port only, validated when read, replaced atomically, private (0600),
//! and never overwritten while invalid. A file from an earlier build that
//! still holds a password (an Argon2id `password_hash`, or a verifier's
//! `salt`, `iterations`, `stored_key` and `server_key`) is valid: its port is
//! kept, those fields are ignored, and the next save drops them.
//!
//! Every test works in its own scratch directory under the system temp dir;
//! nothing reads or writes the user's configuration.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use sdeck_plus::virtual_deck::{
    settings::{check_port, config_dir_from, parse_port, Settings, SettingsStore, FILE_NAME},
    DEFAULT_PORT,
};
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const PORT_RANGE: &str = "Use a port from 1024 to 65535.";
/// What an older build wrote: an Argon2id PHC string.
const OLD_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2FsdA$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g";
/// The fields earlier builds kept about the password.
const PASSWORD_FIELDS: [&str; 5] = [
    "password_hash",
    "salt",
    "iterations",
    "stored_key",
    "server_key",
];

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

fn written(scratch: &Scratch) -> Value {
    serde_json::from_slice(&fs::read(scratch.file()).unwrap()).unwrap()
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

/// The file holds the port and nothing about a password, owner-only, with no
/// temporary file left beside it.
fn assert_port_only(scratch: &Scratch, port: u16) {
    assert_eq!(written(scratch), json!({ "port": port }));
    let text = fs::read_to_string(scratch.file()).unwrap();
    for field in PASSWORD_FIELDS {
        assert!(!text.contains(field), "{field}: {text}");
    }
    assert!(!text.contains("password"), "{text}");
    assert_eq!(mode(&scratch.file()), 0o600, "the file is owner-only");
    assert_eq!(
        entries(&scratch.0),
        [FILE_NAME],
        "no temporary file is left"
    );
}

/// Settings files earlier builds wrote with a password in them: an Argon2id
/// hash, a whole verifier, and fields that were never valid (all ignored now).
fn files_with_a_password(port: u16) -> Vec<(Value, &'static str)> {
    let verifier = json!({
        "port": port,
        "salt": STANDARD.encode([7u8; 16]),
        "iterations": 200_000,
        "stored_key": STANDARD.encode([1u8; 32]),
        "server_key": STANDARD.encode([2u8; 32]),
    });
    vec![
        (
            json!({"port": port, "password_hash": OLD_HASH}),
            "an Argon2id hash",
        ),
        (verifier, "a verifier"),
        (
            json!({"port": port, "password_hash": "plain text"}),
            "a hash that is not Argon2id",
        ),
        (
            json!({"port": port, "salt": STANDARD.encode([1u8; 8]), "iterations": 9_999}),
            "an incomplete, invalid verifier",
        ),
        (
            json!({"port": port, "password_hash": OLD_HASH, "stored_key": null}),
            "a hash beside part of a verifier",
        ),
    ]
}

#[test]
fn the_file_is_virtual_deck_json() {
    assert_eq!(FILE_NAME, "virtual-deck.json");
}

#[test]
fn without_a_file_the_store_offers_port_8765_and_writes_nothing() {
    let scratch = Scratch::new();
    let store = SettingsStore::open(&scratch.0);
    assert_eq!(store.settings(), &Settings::default());
    assert_eq!(store.settings().port, DEFAULT_PORT);
    assert_eq!(DEFAULT_PORT, 8765);
    assert_eq!(store.error(), None);
    assert_eq!(store.path(), scratch.file());
    assert!(entries(&scratch.0).is_empty(), "opening creates nothing");
}

#[test]
fn the_port_round_trips_through_a_private_file_that_holds_nothing_else() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_port(9123).unwrap();
    assert_eq!(store.settings().port, 9123);
    assert_port_only(&scratch, 9123);
    let reopened = SettingsStore::open(&scratch.0);
    assert_eq!(reopened.error(), None);
    assert_eq!(reopened.settings(), store.settings());
    assert_eq!(reopened.settings().port, 9123);
    // Saving the same port again writes the same file.
    let mut store = reopened;
    store.set_port(9123).unwrap();
    assert_port_only(&scratch, 9123);
}

#[test]
fn a_missing_config_directory_is_created_private() {
    let scratch = Scratch::new();
    let dir = scratch.0.join("config/sdeck-plus");
    let mut store = SettingsStore::open(&dir);
    store.set_port(9000).unwrap();
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join(FILE_NAME)), 0o600);
}

/// L40: the password lives in memory only. A file an earlier build wrote
/// with a password in it opens without error on its port, is left as it is
/// until the next save, and that save keeps the port and drops the rest.
#[test]
fn a_file_with_an_earlier_builds_password_keeps_its_port_and_loses_the_password_on_the_next_save() {
    for (contents, why) in files_with_a_password(9123) {
        // A new port.
        let scratch = Scratch::new();
        let text = contents.to_string();
        fs::write(scratch.file(), &text).unwrap();
        let store = SettingsStore::open(&scratch.0);
        assert_eq!(store.error(), None, "{why}: the file is valid");
        assert_eq!(store.settings().port, 9123, "{why}: its port is kept");
        assert_eq!(
            fs::read_to_string(scratch.file()).unwrap(),
            text,
            "{why}: opening leaves it as it is"
        );
        let mut store = store;
        store.set_port(9124).unwrap();
        assert_eq!(store.settings().port, 9124, "{why}");
        assert_port_only(&scratch, 9124);
        assert_eq!(SettingsStore::open(&scratch.0).settings().port, 9124);
        // The same port, as when the app saves the settings it has: the port
        // stays, the password goes.
        let scratch = Scratch::new();
        fs::write(scratch.file(), &text).unwrap();
        let mut store = SettingsStore::open(&scratch.0);
        let port = store.settings().port;
        store.set_port(port).unwrap();
        assert_port_only(&scratch, 9123);
        let reopened = SettingsStore::open(&scratch.0);
        assert_eq!(reopened.error(), None, "{why}");
        assert_eq!(reopened.settings().port, 9123, "{why}");
    }
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
    store.set_port(9000).unwrap();
    let before = fs::read(scratch.file()).unwrap();
    assert_eq!(store.set_port(1023), Err(PORT_RANGE.to_string()));
    assert_eq!(store.settings().port, 9000);
    assert_eq!(fs::read(scratch.file()).unwrap(), before);
    // Without a file, nothing is created either.
    let empty = Scratch::new();
    let mut store = SettingsStore::open(&empty.0);
    assert_eq!(store.set_port(80), Err(PORT_RANGE.to_string()));
    assert_eq!(store.settings().port, DEFAULT_PORT);
    assert!(entries(&empty.0).is_empty());
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
    assert_eq!(store.error(), Some(error.as_str()), "{why}");
    assert_eq!(fs::read(scratch.file()).unwrap(), contents, "{why}");
    assert_eq!(entries(&scratch.0), [FILE_NAME], "{why}");
}

#[test]
fn an_invalid_file_is_kept_and_every_write_is_refused() {
    assert_kept_and_refused(b"not json", "malformed");
    assert_kept_and_refused(b"{\"port\":80}", "port below 1024");
    assert_kept_and_refused(b"{\"port\":70000}", "port above 65535");
    assert_kept_and_refused(b"{\"port\":\"8765\"}", "port not a number");
    assert_kept_and_refused(b"{\"password_hash\":null}", "no port");
    assert_kept_and_refused(
        json!({"port": 80, "password_hash": OLD_HASH})
            .to_string()
            .as_bytes(),
        "an earlier build's file with a port below 1024",
    );
    assert_kept_and_refused(
        json!({"port": 8765, "password": "correct horse"})
            .to_string()
            .as_bytes(),
        "a field no build ever wrote",
    );
}

#[test]
fn a_file_that_turns_invalid_after_opening_is_not_overwritten() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_port(9123).unwrap();
    fs::write(scratch.file(), b"edited by hand").unwrap();
    let error = store.set_port(9000).unwrap_err();
    assert!(error.contains(FILE_NAME), "{error}");
    assert_eq!(fs::read(scratch.file()).unwrap(), b"edited by hand");
    assert_eq!(store.error(), Some(error.as_str()), "writes stay refused");
    assert_eq!(store.settings().port, 9123);
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
