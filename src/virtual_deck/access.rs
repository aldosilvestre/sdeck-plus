//! Who may use the deck: the password hash, login sessions and the per-address
//! limit on failed logins.
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

/// SHA-256 of a session token: tokens themselves are never kept, and looking
/// one up compares digests, which reveals nothing about a live token.
pub(super) type SessionId = [u8; 32];

pub(super) struct Access {
    password_hash: String,
    /// Bumped by every password change, so a login checked against the old
    /// hash cannot open a session after the change.
    generation: u64,
    sessions: HashMap<SessionId, Instant>,
    failures: HashMap<IpAddr, Failures>,
}

struct Failures {
    count: u32,
    last: Instant,
    locked_until: Option<Instant>,
}

impl Access {
    pub fn new(password_hash: String) -> Self {
        Self {
            password_hash,
            generation: 0,
            sessions: HashMap::new(),
            failures: HashMap::new(),
        }
    }

    /// The hash to check a login against and its generation.
    pub fn password(&self) -> (String, u64) {
        (self.password_hash.clone(), self.generation)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A new password ends every session.
    pub fn set_password(&mut self, hash: String) {
        self.password_hash = hash;
        self.generation += 1;
        self.sessions.clear();
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

/// The id of a well-formed token: exactly 64 hex digits.
fn session_id(token: &str) -> Option<SessionId> {
    let well_formed =
        token.len() == TOKEN_BYTES * 2 && token.bytes().all(|byte| byte.is_ascii_hexdigit());
    well_formed.then(|| Sha256::digest(token.as_bytes()).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const PHONE: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 30));
    const LAPTOP: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 31));

    #[test]
    fn five_failures_lock_one_address_for_thirty_seconds() {
        let mut access = Access::new(String::new());
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
        let mut access = Access::new(String::new());
        let now = Instant::now();
        access.fail(PHONE, now);
        access.succeed(PHONE);
        assert_eq!(access.fail(PHONE, now), 4);
    }

    #[test]
    fn sessions_are_found_by_token_and_end_with_a_new_password() {
        let mut access = Access::new("old".into());
        let (token, id) = access.open_session(Instant::now()).unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(access.find_session(["nope", token.as_str()]), Some(id));
        assert_eq!(access.find_session([&"0".repeat(64)[..]]), None);
        access.set_password("new".into());
        assert_eq!(access.find_session([token.as_str()]), None);
        assert_eq!(access.generation(), 1);
    }

    #[test]
    fn the_tracked_addresses_stay_bounded() {
        let mut access = Access::new(String::new());
        let now = Instant::now();
        for index in 0..(MAX_TRACKED_ADDRESSES as u32 + 10) {
            access.fail(IpAddr::V4(Ipv4Addr::from(index)), now);
        }
        assert!(access.failures.len() <= MAX_TRACKED_ADDRESSES);
    }
}
