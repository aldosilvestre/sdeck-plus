//! Who may use the deck: the password verifier, login challenges, login
//! sessions and the per-address limit on failed logins.
use super::password::{Key, Verifier};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};

pub(super) const SESSION_COOKIE: &str = "sdeck_session";
/// Failed logins an address may make before it must wait.
pub(super) const MAX_TRIES: u32 = 5;
pub(super) const LOCKOUT: Duration = Duration::from_secs(30);
/// Failures older than this are forgotten.
const FAILURE_MEMORY: Duration = Duration::from_secs(15 * 60);
const MAX_TRACKED_ADDRESSES: usize = 256;
const MAX_SESSIONS: usize = 64;
const TOKEN_BYTES: usize = 32;
/// Random bytes of a server nonce; it travels as 64 lowercase hex digits.
pub(super) const NONCE_BYTES: usize = 32;
/// A challenge must be answered within this time, once.
pub(super) const CHALLENGE_LIFETIME: Duration = Duration::from_secs(30);
/// Unanswered challenges one address may hold; a new one replaces its oldest.
const CHALLENGES_PER_ADDRESS: usize = 4;
/// Unanswered challenges in all; a new one replaces the oldest.
const MAX_CHALLENGES: usize = 256;

/// SHA-256 of a session token: tokens themselves are never kept, and looking
/// one up compares digests, which reveals nothing about a live token.
pub(super) type SessionId = [u8; 32];

pub(super) struct Access {
    verifier: Verifier,
    sessions: HashMap<SessionId, Instant>,
    failures: HashMap<IpAddr, Failures>,
    /// Unanswered login challenges by server nonce.
    challenges: HashMap<[u8; NONCE_BYTES], Challenge>,
}

/// A login challenge: who asked for it and when.
struct Challenge {
    address: IpAddr,
    issued: Instant,
}

/// What a page needs to answer a challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Issued {
    /// 64 lowercase hex digits.
    pub nonce: String,
    pub salt: Vec<u8>,
    pub iterations: u32,
}

struct Failures {
    count: u32,
    last: Instant,
    locked_until: Option<Instant>,
}

impl Access {
    pub fn new(verifier: Verifier) -> Self {
        Self {
            verifier,
            sessions: HashMap::new(),
            failures: HashMap::new(),
            challenges: HashMap::new(),
        }
    }

    /// A new password ends every session and voids every challenge.
    pub fn set_password(&mut self, verifier: Verifier) {
        self.verifier = verifier;
        self.sessions.clear();
        self.challenges.clear();
    }

    /// A new single-use challenge for `address`: a random server nonce, the
    /// salt and the iteration count.
    pub fn challenge(&mut self, address: IpAddr, now: Instant) -> Result<Issued, String> {
        let mut nonce = [0u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|error| format!("No random bytes: {error}"))?;
        self.challenges
            .retain(|_, challenge| now.duration_since(challenge.issued) < CHALLENGE_LIFETIME);
        let held = self
            .challenges
            .values()
            .filter(|challenge| challenge.address == address)
            .count();
        if held >= CHALLENGES_PER_ADDRESS {
            self.forget_oldest_challenge(|challenge| challenge.address == address);
        }
        if self.challenges.len() >= MAX_CHALLENGES {
            self.forget_oldest_challenge(|_| true);
        }
        self.challenges.insert(
            nonce,
            Challenge {
                address,
                issued: now,
            },
        );
        Ok(Issued {
            nonce: hex(&nonce),
            salt: self.verifier.salt().to_vec(),
            iterations: self.verifier.iterations(),
        })
    }

    fn forget_oldest_challenge(&mut self, selected: impl Fn(&Challenge) -> bool) {
        let oldest = self
            .challenges
            .iter()
            .filter(|(_, challenge)| selected(challenge))
            .min_by_key(|(_, challenge)| challenge.issued)
            .map(|(nonce, _)| *nonce);
        if let Some(nonce) = oldest {
            self.challenges.remove(&nonce);
        }
    }

    /// Uses up the challenge `nonce` (64 hex digits) when it was issued to
    /// `address` less than [`CHALLENGE_LIFETIME`] ago; returns whether it
    /// was. Another address's challenge is left alone.
    pub fn take_challenge(&mut self, nonce: &str, address: IpAddr, now: Instant) -> bool {
        let Some(nonce) = parse_nonce(nonce) else {
            return false;
        };
        let usable = self.challenges.get(&nonce).map(|challenge| {
            (
                challenge.address == address,
                now.duration_since(challenge.issued) < CHALLENGE_LIFETIME,
            )
        });
        match usable {
            Some((true, fresh)) => {
                self.challenges.remove(&nonce);
                fresh
            }
            _ => false,
        }
    }

    /// Checks a ClientProof; returns the ServerSignature when it is right.
    pub fn check_proof(&self, auth_message: &[u8], proof: &Key) -> Option<Key> {
        self.verifier.check_proof(auth_message, proof)
    }

    /// How long `address` must still wait, if it is locked out.
    pub fn locked(&mut self, address: IpAddr, now: Instant) -> Option<Duration> {
        let failures = self.failures.get(&address)?;
        match failures.locked_until {
            Some(until) if until > now => return Some(until - now),
            Some(_) => {}
            None if now.duration_since(failures.last) <= FAILURE_MEMORY => return None,
            None => {}
        }
        self.failures.remove(&address);
        None
    }

    /// Records a wrong password; returns the tries left before the lockout.
    pub fn fail(&mut self, address: IpAddr, now: Instant) -> u32 {
        if !self.failures.contains_key(&address) && self.failures.len() >= MAX_TRACKED_ADDRESSES {
            self.failures.retain(|_, failures| {
                failures.locked_until.is_some_and(|until| until > now)
                    || now.duration_since(failures.last) <= FAILURE_MEMORY
            });
            if self.failures.len() >= MAX_TRACKED_ADDRESSES {
                if let Some(oldest) = self
                    .failures
                    .iter()
                    .min_by_key(|(_, failures)| failures.last)
                    .map(|(address, _)| *address)
                {
                    self.failures.remove(&oldest);
                }
            }
        }
        let failures = self.failures.entry(address).or_insert(Failures {
            count: 0,
            last: now,
            locked_until: None,
        });
        failures.count += 1;
        failures.last = now;
        if failures.count >= MAX_TRIES {
            failures.locked_until = Some(now + LOCKOUT);
        }
        MAX_TRIES.saturating_sub(failures.count)
    }

    pub fn succeed(&mut self, address: IpAddr) {
        self.failures.remove(&address);
    }

    /// Opens a session; returns its token (64 hex digits) and id.
    pub fn open_session(&mut self, now: Instant) -> Result<(String, SessionId), String> {
        let mut bytes = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes).map_err(|error| format!("No random bytes: {error}"))?;
        let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let id = session_id(&token).ok_or("Could not open a session.")?;
        if self.sessions.len() >= MAX_SESSIONS {
            if let Some(oldest) = self
                .sessions
                .iter()
                .min_by_key(|(_, opened)| **opened)
                .map(|(id, _)| *id)
            {
                self.sessions.remove(&oldest);
            }
        }
        self.sessions.insert(id, now);
        Ok((token, id))
    }

    /// The first live session among `tokens`.
    pub fn find_session<'a>(&self, tokens: impl IntoIterator<Item = &'a str>) -> Option<SessionId> {
        tokens
            .into_iter()
            .filter_map(session_id)
            .find(|id| self.sessions.contains_key(id))
    }

    pub fn close_session(&mut self, id: &SessionId) -> bool {
        self.sessions.remove(id).is_some()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The bytes of a server nonce as sent: exactly 64 lowercase hex digits.
fn parse_nonce(text: &str) -> Option<[u8; NONCE_BYTES]> {
    let digits = text.as_bytes();
    if digits.len() != NONCE_BYTES * 2
        || !digits
            .iter()
            .all(|d| matches!(d, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    let mut nonce = [0u8; NONCE_BYTES];
    for (byte, pair) in nonce.iter_mut().zip(digits.chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(nonce)
}

/// The id of a well-formed token: exactly 64 hex digits.
fn session_id(token: &str) -> Option<SessionId> {
    let well_formed =
        token.len() == TOKEN_BYTES * 2 && token.bytes().all(|byte| byte.is_ascii_hexdigit());
    well_formed.then(|| Sha256::digest(token.as_bytes()).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtual_deck::build_verifier;
    use std::{net::Ipv4Addr, sync::OnceLock};

    const PHONE: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 30));
    const LAPTOP: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 31));

    fn verifier() -> Verifier {
        static VERIFIER: OnceLock<Verifier> = OnceLock::new();
        VERIFIER
            .get_or_init(|| build_verifier("correct horse").unwrap())
            .clone()
    }

    #[test]
    fn five_failures_lock_one_address_for_thirty_seconds() {
        let mut access = Access::new(verifier());
        let start = Instant::now();
        let tries: Vec<u32> = (0..5).map(|_| access.fail(PHONE, start)).collect();
        assert_eq!(tries, [4, 3, 2, 1, 0]);
        assert_eq!(access.locked(PHONE, start), Some(LOCKOUT));
        assert_eq!(access.locked(LAPTOP, start), None, "per address");
        let later = start + LOCKOUT;
        assert_eq!(access.locked(PHONE, later), None);
        assert_eq!(
            access.fail(PHONE, later),
            4,
            "a fresh start after the lockout"
        );
    }

    #[test]
    fn a_success_forgets_the_failures() {
        let mut access = Access::new(verifier());
        let now = Instant::now();
        access.fail(PHONE, now);
        access.succeed(PHONE);
        assert_eq!(access.fail(PHONE, now), 4);
    }

    #[test]
    fn sessions_are_found_by_token_and_end_with_a_new_password() {
        let mut access = Access::new(verifier());
        let now = Instant::now();
        let (token, id) = access.open_session(now).unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(access.find_session(["nope", token.as_str()]), Some(id));
        assert_eq!(access.find_session([&"0".repeat(64)[..]]), None);
        let issued = access.challenge(PHONE, now).unwrap();
        access.set_password(verifier());
        assert_eq!(access.find_session([token.as_str()]), None);
        assert!(
            !access.take_challenge(&issued.nonce, PHONE, now),
            "a new password voids every challenge"
        );
    }

    #[test]
    fn a_challenge_is_used_once_by_its_own_address_within_its_lifetime() {
        let mut access = Access::new(verifier());
        let now = Instant::now();
        let issued = access.challenge(PHONE, now).unwrap();
        assert_eq!(issued.nonce.len(), 64);
        assert_eq!(issued.salt, verifier().salt());
        assert_eq!(issued.iterations, verifier().iterations());
        assert_ne!(access.challenge(PHONE, now).unwrap().nonce, issued.nonce);
        assert!(
            !access.take_challenge(&issued.nonce, LAPTOP, now),
            "other address"
        );
        assert!(!access.take_challenge(&issued.nonce.to_uppercase(), PHONE, now));
        assert!(access.take_challenge(&issued.nonce, PHONE, now));
        assert!(
            !access.take_challenge(&issued.nonce, PHONE, now),
            "used once"
        );
        let late = access.challenge(PHONE, now).unwrap();
        assert!(!access.take_challenge(&late.nonce, PHONE, now + CHALLENGE_LIFETIME));
    }

    #[test]
    fn unanswered_challenges_stay_bounded_per_address_and_in_all() {
        let mut access = Access::new(verifier());
        let now = Instant::now();
        let first = access.challenge(PHONE, now).unwrap();
        let later = now + Duration::from_millis(1);
        for _ in 0..CHALLENGES_PER_ADDRESS {
            access.challenge(PHONE, later).unwrap();
        }
        assert!(
            !access.take_challenge(&first.nonce, PHONE, later),
            "the oldest made room"
        );
        for index in 0..(MAX_CHALLENGES as u32 + 10) {
            access
                .challenge(IpAddr::V4(Ipv4Addr::from(index)), later)
                .unwrap();
        }
        assert!(access.challenges.len() <= MAX_CHALLENGES);
    }

    #[test]
    fn the_tracked_addresses_stay_bounded() {
        let mut access = Access::new(verifier());
        let now = Instant::now();
        for index in 0..(MAX_TRACKED_ADDRESSES as u32 + 10) {
            access.fail(IpAddr::V4(Ipv4Addr::from(index)), now);
        }
        assert!(access.failures.len() <= MAX_TRACKED_ADDRESSES);
    }
}
