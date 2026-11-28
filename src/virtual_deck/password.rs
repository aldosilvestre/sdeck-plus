//! The password verifier and the login proof (SCRAM-SHA-256 style, RFC 5802
//! and RFC 7677): the app holds in memory only a salt, an iteration count and
//! two keys derived from the password, never the password, and never writes
//! them to disk; a page proves it knows the password without sending it.
//!
//! ```text
//! SaltedPassword  = PBKDF2-HMAC-SHA-256(password, salt, iterations), 32 bytes
//! ClientKey       = HMAC(SaltedPassword, "Client Key")
//! StoredKey       = SHA-256(ClientKey)                       (stored)
//! ServerKey       = HMAC(SaltedPassword, "Server Key")       (stored)
//! AuthMessage     = client nonce + server nonce              (as sent, ASCII)
//! ClientSignature = HMAC(StoredKey, AuthMessage)
//! ClientProof     = ClientKey XOR ClientSignature            (the page sends it)
//! ServerSignature = HMAC(ServerKey, AuthMessage)             (the server answers it)
//! ```
//!
//! The server recovers ClientKey = ClientProof XOR ClientSignature and
//! accepts when SHA-256(ClientKey) equals StoredKey (compared in constant
//! time). The page computes the same in plain JavaScript (`page/login.html`),
//! because `crypto.subtle` needs HTTPS.
use ring::{digest, hmac, pbkdf2};
use std::{fmt, num::NonZeroU32};

const MIN_CHARACTERS: usize = 8;
/// Typing limit for the app's password fields (the password itself never
/// reaches the server).
const MAX_CHARACTERS: usize = 128;
/// Bytes of random salt in a new verifier.
const SALT_BYTES: usize = 16;
/// PBKDF2 iterations of a new verifier. The page runs them in plain
/// JavaScript: about 0.12 s on a fast laptop (Ryzen 9 5900HX, Node 26), so
/// roughly 0.5 to 1.2 s on a mid-range or older phone, 4 to 10 times slower.
/// Every iteration also slows down a guess at a login someone recorded on
/// the LAN.
pub const ITERATIONS: u32 = 200_000;
/// StoredKey, ServerKey, a proof and a signature are all SHA-256 sized.
const KEY_BYTES: usize = 32;

pub(super) type Key = [u8; KEY_BYTES];

/// What the app holds in memory instead of the password, for one session: a
/// salt, an iteration count, StoredKey and ServerKey. Built by
/// [`build_verifier`]; never written to disk.
///
/// Keep it private: with StoredKey and one recorded login, someone could
/// log in; with ServerKey, pass for the server.
#[derive(Clone, PartialEq, Eq)]
pub struct Verifier {
    salt: Vec<u8>,
    iterations: u32,
    stored_key: Key,
    server_key: Key,
}

impl fmt::Debug for Verifier {
    /// Never prints the keys.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Verifier")
            .field("salt_bytes", &self.salt.len())
            .field("iterations", &self.iterations)
            .finish_non_exhaustive()
    }
}

/// Builds the verifier of a new password with a random salt and
/// [`ITERATIONS`]; slow on purpose, so call it off the GTK thread.
///
/// Rejects passwords shorter than 8 characters with "At least 8 characters."
/// and longer than 128 characters with "At most 128 characters.".
pub fn build_verifier(password: &str) -> Result<Verifier, String> {
    let characters = password.chars().count();
    if characters < MIN_CHARACTERS {
        return Err("At least 8 characters.".to_string());
    }
    if characters > MAX_CHARACTERS {
        return Err("At most 128 characters.".to_string());
    }
    let mut salt = vec![0u8; SALT_BYTES];
    getrandom::fill(&mut salt).map_err(|error| format!("No random bytes for the salt: {error}"))?;
    Ok(Verifier::derive(password, salt, ITERATIONS))
}

impl Verifier {
    /// The verifier of `password` with this salt and iteration count.
    fn derive(password: &str, salt: Vec<u8>, iterations: u32) -> Self {
        let salted = salted_password(password.as_bytes(), &salt, iterations);
        Self {
            stored_key: sha256(&hmac_sha256(&salted, b"Client Key")),
            server_key: hmac_sha256(&salted, b"Server Key"),
            salt,
            iterations,
        }
    }

    /// The PBKDF2 salt the page needs.
    pub fn salt(&self) -> &[u8] {
        &self.salt
    }

    /// The PBKDF2 iteration count the page needs.
    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    /// Checks a page's ClientProof for `auth_message`; returns the
    /// ServerSignature to answer with when the proof is right.
    pub(super) fn check_proof(&self, auth_message: &[u8], proof: &Key) -> Option<Key> {
        let signature = hmac_sha256(&self.stored_key, auth_message);
        let mut client_key = [0u8; KEY_BYTES];
        for (byte, (proof, signature)) in client_key.iter_mut().zip(proof.iter().zip(&signature)) {
            *byte = proof ^ signature;
        }
        same(&sha256(&client_key), &self.stored_key)
            .then(|| hmac_sha256(&self.server_key, auth_message))
    }
}

fn salted_password(password: &[u8], salt: &[u8], iterations: u32) -> Key {
    let mut salted = [0u8; KEY_BYTES];
    let rounds = NonZeroU32::new(iterations).unwrap_or(NonZeroU32::MIN);
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        rounds,
        salt,
        password,
        &mut salted,
    );
    salted
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Key {
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data);
    let mut out = [0u8; KEY_BYTES];
    out.copy_from_slice(tag.as_ref());
    out
}

fn sha256(data: &[u8]) -> Key {
    let mut out = [0u8; KEY_BYTES];
    out.copy_from_slice(digest::digest(&digest::SHA256, data).as_ref());
    out
}

/// Whether two keys are equal, looking at every byte whatever they hold.
fn same(left: &Key, right: &Key) -> bool {
    let difference = left
        .iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right));
    std::hint::black_box(difference) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn hmac_sha256_matches_rfc_4231() {
        // Test cases 1, 2 and 6 (a key longer than the block).
        assert_eq!(
            hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hex(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn pbkdf2_hmac_sha256_matches_rfc_7914() {
        // Section 11, first 32 bytes of each 64-byte result.
        assert_eq!(
            hex(&salted_password(b"passwd", b"salt", 1)),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"
        );
        assert_eq!(
            hex(&salted_password(b"Password", b"NaCl", 80_000)),
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56"
        );
    }

    /// RFC 7677 section 3: user "user", password "pencil".
    #[test]
    fn the_proof_and_signature_follow_rfc_7677() {
        let salt = STANDARD.decode("W22ZaJ0SNY7soEsUEjb6gQ==").unwrap();
        let verifier = Verifier::derive("pencil", salt, 4096);
        let nonce = "rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0";
        let auth_message = format!(
            "n=user,r=rOprNGfwEbeRWgbNEkqO,r={nonce},s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096,\
             c=biws,r={nonce}"
        );
        let proof: Key = STANDARD
            .decode("dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=")
            .unwrap()
            .try_into()
            .unwrap();
        let signature = verifier
            .check_proof(auth_message.as_bytes(), &proof)
            .expect("the RFC's proof is accepted");
        assert_eq!(
            STANDARD.encode(signature),
            "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4="
        );
        let mut wrong = proof;
        wrong[31] ^= 1;
        assert_eq!(verifier.check_proof(auth_message.as_bytes(), &wrong), None);
        assert_eq!(verifier.check_proof(b"another message", &proof), None);
    }

    #[test]
    fn a_new_verifier_has_a_fresh_salt_and_never_shows_its_keys() {
        let first = build_verifier("12345678").unwrap();
        let second = build_verifier("12345678").unwrap();
        assert_eq!(first.salt().len(), SALT_BYTES);
        assert_eq!(first.iterations(), ITERATIONS);
        assert_ne!(first, second, "every verifier has its own salt");
        let shown = format!("{first:?}");
        for key in [first.stored_key, first.server_key] {
            assert!(
                !shown.contains(&STANDARD.encode(key)) && !shown.contains(&hex(&key)),
                "{shown}"
            );
        }
        assert!(!shown.contains("12345678"), "{shown}");
    }

    #[test]
    fn the_length_rules_count_characters() {
        assert_eq!(
            build_verifier("1234567"),
            Err("At least 8 characters.".to_string())
        );
        assert_eq!(
            build_verifier("ñññññññ"),
            Err("At least 8 characters.".to_string())
        );
        assert_eq!(
            build_verifier(&"a".repeat(129)),
            Err("At most 128 characters.".to_string())
        );
        assert!(build_verifier(&"ñ".repeat(128)).is_ok());
    }
}
