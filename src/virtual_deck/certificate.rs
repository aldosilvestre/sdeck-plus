//! The server's self-signed certificate: ECDSA P-256, stored as
//! `<config>/virtual/cert.pem` and `key.pem` (0600, directory 0700), reused
//! while its names and validity still fit, generated afresh otherwise.
use super::x509;
use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, KeyPair, KeyUsagePurpose, SerialNumber,
    PKCS_ECDSA_P256_SHA256,
};
use rustls::{
    crypto::CryptoProvider,
    pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    sign::CertifiedKey,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, DirBuilder, OpenOptions, Permissions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const DIRECTORY: &str = "virtual";
const CERT_FILE: &str = "cert.pem";
const KEY_FILE: &str = "key.pem";
const COMMON_NAME: &str = "sdeck-plus virtual deck";
/// Below Apple's 825-day limit for TLS server certificates.
const VALID_DAYS: i64 = 397;
/// A certificate this close to its end is replaced at the next start.
const RENEW_DAYS: i64 = 30;
const DAY: i64 = 86_400;

/// The served certificate and key.
pub(super) struct Identity {
    pub cert: CertificateDer<'static>,
    pub key: PrivateKeyDer<'static>,
}

impl Identity {
    /// "SHA-256 AA:BB:..." of the DER certificate.
    pub fn fingerprint(&self) -> String {
        let hex: Vec<String> = Sha256::digest(self.cert.as_ref())
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect();
        format!("SHA-256 {}", hex.join(":"))
    }
}

/// Loads the stored certificate when it names exactly `names`, is valid for
/// at least 30 more days and its key matches and is private; otherwise
/// generates, stores and returns a new one.
pub(super) fn load_or_create(
    config_dir: &Path,
    names: &BTreeSet<String>,
    provider: &CryptoProvider,
) -> Result<Identity, String> {
    let directory = private_directory(config_dir)?;
    if let Some(identity) = load(&directory, names, provider) {
        return Ok(identity);
    }
    create(&directory, names)
}

fn private_directory(config_dir: &Path) -> Result<PathBuf, String> {
    let fail = |error: std::io::Error| {
        format!(
            "Could not prepare {} for the certificate: {error}",
            config_dir.join(DIRECTORY).display()
        )
    };
    fs::create_dir_all(config_dir).map_err(fail)?;
    let directory = config_dir.join(DIRECTORY);
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() => {
            if metadata.permissions().mode() & 0o777 != 0o700 {
                fs::set_permissions(&directory, Permissions::from_mode(0o700)).map_err(fail)?;
            }
        }
        Ok(_) => return Err(format!("{} is not a directory.", directory.display())),
        Err(_) => DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(fail)?,
    }
    Ok(directory)
}

fn load(directory: &Path, names: &BTreeSet<String>, provider: &CryptoProvider) -> Option<Identity> {
    let key_path = directory.join(KEY_FILE);
    let cert_path = directory.join(CERT_FILE);
    // A key another user could read, or a link to elsewhere, is not trusted.
    let key_metadata = fs::symlink_metadata(&key_path).ok()?;
    if !key_metadata.is_file() || key_metadata.permissions().mode() & 0o077 != 0 {
        return None;
    }
    if !fs::symlink_metadata(&cert_path).ok()?.is_file() {
        return None;
    }
    let cert = CertificateDer::from_pem_file(&cert_path).ok()?;
    let key = PrivateKeyDer::from_pem_file(&key_path).ok()?;
    let facts = x509::inspect(cert.as_ref())?;
    let now = unix_now();
    let fits = facts.names == *names
        && facts.not_before <= now
        && facts.not_after - now > RENEW_DAYS * DAY;
    if !fits {
        return None;
    }
    CertifiedKey::from_der(vec![cert.clone()], key.clone_key(), provider).ok()?;
    Some(Identity { cert, key })
}

fn create(directory: &Path, names: &BTreeSet<String>) -> Result<Identity, String> {
    let fail = |error: rcgen::Error| format!("Could not create the certificate: {error}");
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(fail)?;
    let mut params =
        CertificateParams::new(names.iter().cloned().collect::<Vec<_>>()).map_err(fail)?;
    params
        .distinguished_name
        .push(DnType::CommonName, COMMON_NAME);
    let today = unix_now().div_euclid(DAY);
    let (year, month, day) = ymd(today - 1);
    params.not_before = rcgen::date_time_ymd(year, month, day);
    let (year, month, day) = ymd(today + VALID_DAYS);
    params.not_after = rcgen::date_time_ymd(year, month, day);
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    // A fresh serial for every certificate: Firefox refuses two different
    // certificates with the same issuer and serial number.
    let mut serial = [0u8; 16];
    getrandom::fill(&mut serial).map_err(|error| format!("No random bytes: {error}"))?;
    serial[0] &= 0x7F;
    params.serial_number = Some(SerialNumber::from_slice(&serial));
    let cert = params.self_signed(&key).map_err(fail)?;

    // The key first: a crash between the two writes leaves a pair that does
    // not match, which the next start replaces.
    write_private(directory, KEY_FILE, key.serialize_pem().as_bytes())?;
    write_private(directory, CERT_FILE, cert.pem().as_bytes())?;
    Ok(Identity {
        cert: cert.der().clone(),
        key: PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
    })
}

/// Writes `name` atomically with mode 0600: a private temporary file, then rename.
fn write_private(directory: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let target = directory.join(name);
    let staging = directory.join(format!(".{name}.tmp"));
    let fail = |error: std::io::Error| format!("Could not write {}: {error}", target.display());
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&staging)
        .map_err(fail)?;
    file.set_permissions(Permissions::from_mode(0o600))
        .map_err(fail)?;
    file.write_all(bytes).map_err(fail)?;
    file.sync_all().map_err(fail)?;
    drop(file);
    fs::rename(&staging, &target).map_err(fail)
}

fn ymd(days: i64) -> (i32, u8, u8) {
    let (year, month, day) = x509::civil_from_days(days);
    (i32::try_from(year).unwrap_or(i32::MAX), month, day)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}
