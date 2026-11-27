//! Argon2id password hashes: the app keeps only the PHC string, never the password.
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params,
};

const MIN_CHARACTERS: usize = 8;
/// A login body holds at most `http::MAX_BODY` bytes; 128 characters fit even
/// when every one is a 4-byte UTF-8 sequence sent percent-encoded.
const MAX_CHARACTERS: usize = 128;
const SALT_BYTES: usize = 16;
const INVALID_HASH: &str = "The password hash is not valid.";

/// Hashes a new password as an Argon2id PHC string with a random salt.
///
/// Rejects passwords shorter than 8 characters with "At least 8 characters."
/// and longer than 128 characters with "At most 128 characters.".
pub fn hash_password(password: &str) -> Result<String, String> {
    let characters = password.chars().count();
    if characters < MIN_CHARACTERS {
        return Err("At least 8 characters.".to_string());
    }
    if characters > MAX_CHARACTERS {
        return Err("At most 128 characters.".to_string());
    }
    let mut salt = [0u8; SALT_BYTES];
    getrandom::fill(&mut salt).map_err(|error| format!("No random bytes for the salt: {error}"))?;
    let salt = SaltString::encode_b64(&salt).map_err(|error| error.to_string())?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| format!("Could not hash the password: {error}"))
}

/// Accepts only a complete Argon2id PHC string with usable parameters.
pub(super) fn check_hash(hash: &str) -> Result<(), String> {
    let parsed = PasswordHash::new(hash).map_err(|_| INVALID_HASH.to_string())?;
    let usable = parsed.algorithm == Algorithm::Argon2id.ident()
        && parsed.salt.is_some()
        && parsed.hash.is_some()
        && Params::try_from(&parsed).is_ok();
    if usable {
        Ok(())
    } else {
        Err(INVALID_HASH.to_string())
    }
}

/// Whether `password` matches `hash`; the final comparison is constant-time
/// (`password_hash::Output` compares with `subtle`).
pub(super) fn verify(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}
