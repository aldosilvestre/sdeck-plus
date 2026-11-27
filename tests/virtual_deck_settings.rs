//! The virtual deck's remembered settings (S4 password verifier, S5 port):
//! `virtual-deck.json` in the app's config directory, validated when read,
//! replaced atomically, private (0600), and never overwritten while invalid.
//! A file from an older version (Argon2id `password_hash`) keeps its port
//! and asks for a new password; it is rewritten only when one is saved.
//!
//! Every test works in its own scratch directory under the system temp dir;
//! nothing reads or writes the user's configuration.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ring::{digest, hmac, pbkdf2};
use sdeck_plus::virtual_deck::{
    build_verifier,
    settings::{check_port, config_dir_from, parse_port, Settings, SettingsStore, FILE_NAME},
    Verifier, DEFAULT_PORT, ITERATIONS,
};
use serde_json::{json, Value};
use std::{
    fs,
    num::NonZeroU32,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
};

const PORT_RANGE: &str = "Use a port from 1024 to 65535.";
const PASSWORD: &str = "correct horse";
/// What an older version wrote: an Argon2id PHC string.
const OLD_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2FsdA$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g";

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

/// One verifier for the whole binary: deriving is deliberately slow.
fn verifier() -> Verifier {
    static VERIFIER: OnceLock<Verifier> = OnceLock::new();
    VERIFIER
        .get_or_init(|| build_verifier(PASSWORD).unwrap())
        .clone()
}

fn written(scratch: &Scratch) -> Value {
    serde_json::from_slice(&fs::read(scratch.file()).unwrap()).unwrap()
}

fn base64_of(value: &Value) -> Vec<u8> {
    STANDARD
        .decode(value.as_str().expect("a base64 string"))
        .unwrap()
}

/// `virtual-deck.json` contents with a verifier's fields.
fn verifier_file(port: u16, extra: Value) -> String {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_port(port).unwrap();
    store.set_verifier(verifier()).unwrap();
    let mut file = written(&scratch);
    for (key, value) in extra.as_object().unwrap() {
        file[key] = value.clone();
    }
    file.to_string()
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
            verifier: None,
        }
    );
    assert!(!store.has_old_password());
    assert_eq!(DEFAULT_PORT, 8765);
    assert_eq!(store.error(), None);
    assert_eq!(store.path(), scratch.file());
    assert!(entries(&scratch.0).is_empty(), "opening creates nothing");
}

#[test]
fn a_verifier_and_a_port_round_trip_through_a_private_file() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_verifier(verifier()).unwrap();
    store.set_port(9123).unwrap();
    assert_eq!(store.settings().port, 9123);
    assert_eq!(store.settings().verifier, Some(verifier()));
    let file = written(&scratch);
    assert_eq!(
        file.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["iterations", "port", "salt", "server_key", "stored_key"]
    );
    assert_eq!(file["port"], 9123);
    assert_eq!(file["iterations"], ITERATIONS);
    assert_eq!(base64_of(&file["salt"]), verifier().salt());
    assert_eq!(base64_of(&file["stored_key"]).len(), 32);
    assert_eq!(base64_of(&file["server_key"]).len(), 32);
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
    store.set_verifier(verifier()).unwrap();
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join(FILE_NAME)), 0o600);
}

/// The keys are SCRAM-SHA-256's: StoredKey = SHA-256(HMAC(SaltedPassword,
/// "Client Key")), ServerKey = HMAC(SaltedPassword, "Server Key"), with
/// SaltedPassword = PBKDF2-HMAC-SHA-256(password, salt, iterations).
#[test]
fn the_password_is_stored_only_as_its_verifier_keys() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_verifier(verifier()).unwrap();
    let text = fs::read_to_string(scratch.file()).unwrap();
    assert!(!text.contains(PASSWORD) && !text.contains(&STANDARD.encode(PASSWORD)));
    let file = written(&scratch);
    let mut salted = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(ITERATIONS).unwrap(),
        &base64_of(&file["salt"]),
        PASSWORD.as_bytes(),
        &mut salted,
    );
    let key = hmac::Key::new(hmac::HMAC_SHA256, &salted);
    let client_key = hmac::sign(&key, b"Client Key");
    let stored_key = digest::digest(&digest::SHA256, client_key.as_ref());
    assert_eq!(base64_of(&file["stored_key"]), stored_key.as_ref());
    assert_eq!(
        base64_of(&file["server_key"]),
        hmac::sign(&key, b"Server Key").as_ref()
    );
}

#[test]
fn an_old_argon2_file_keeps_its_port_asks_for_a_password_and_is_kept_until_one_is_saved() {
    let scratch = Scratch::new();
    let old = json!({"port": 9123, "password_hash": OLD_HASH}).to_string();
    fs::write(scratch.file(), &old).unwrap();
    let store = SettingsStore::open(&scratch.0);
    assert_eq!(store.error(), None, "the old file is valid");
    assert_eq!(
        store.settings(),
        &Settings {
            port: 9123,
            verifier: None,
        },
        "no usable verifier: the password must be set again"
    );
    assert!(store.has_old_password());
    assert_eq!(
        fs::read_to_string(scratch.file()).unwrap(),
        old,
        "untouched"
    );
    // A port change keeps the old hash as it is.
    let mut store = SettingsStore::open(&scratch.0);
    store.set_port(9124).unwrap();
    assert_eq!(
        written(&scratch),
        json!({"port": 9124, "password_hash": OLD_HASH})
    );
    assert!(SettingsStore::open(&scratch.0).has_old_password());
    // A new password replaces it with a verifier, on the same port.
    store.set_verifier(verifier()).unwrap();
    let file = written(&scratch);
    assert_eq!(file["port"], 9124);
    assert!(file.get("password_hash").is_none(), "{file}");
    assert_eq!(mode(&scratch.file()), 0o600);
    let reopened = SettingsStore::open(&scratch.0);
    assert!(!reopened.has_old_password());
    assert_eq!(reopened.settings().verifier, Some(verifier()));
    assert_eq!(entries(&scratch.0), [FILE_NAME]);
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
    store.set_verifier(verifier()).unwrap();
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
        store.set_verifier(verifier()).unwrap_err(),
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
        "an old hash that is not Argon2id",
    );
    assert_kept_and_refused(b"{\"password_hash\":null}", "no port");
    let cases = [
        (json!({"password": PASSWORD}), "unknown field"),
        (json!({"password_hash": OLD_HASH}), "old hash and verifier"),
        (json!({"iterations": 9_999}), "too few iterations"),
        (json!({"iterations": 2_000_001}), "too many iterations"),
        (json!({"salt": STANDARD.encode([1u8; 8])}), "short salt"),
        (
            json!({"stored_key": STANDARD.encode([1u8; 31])}),
            "short key",
        ),
        (json!({"server_key": "not base64!"}), "not base64"),
        (json!({"stored_key": null}), "incomplete verifier"),
    ];
    for (extra, why) in cases {
        assert_kept_and_refused(verifier_file(8765, extra).as_bytes(), why);
    }
}

#[test]
fn a_file_that_turns_invalid_after_opening_is_not_overwritten() {
    let scratch = Scratch::new();
    let mut store = SettingsStore::open(&scratch.0);
    store.set_verifier(verifier()).unwrap();
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
