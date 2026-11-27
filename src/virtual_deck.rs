//! The virtual Stream Deck Classic's web server: plain HTTP + WebSocket on
//! the LAN, so no browser ever shows a certificate warning.
//!
//! A phone or browser opens `http://<lan-ip>:<port>/`, logs in with the
//! password the app set for this session, and gets the deck page; the page then opens
//! `ws://<lan-ip>:<port>/ws` and exchanges JSON text frames:
//!
//! - server to page: `{"type":"key","key":n,"jpeg":"<base64>"}`,
//!   `{"type":"background","jpeg":"<base64>"}`, `{"type":"brightness","percent":n}`,
//!   `{"type":"reset"}` and, just before the server closes the socket,
//!   `{"type":"bye","reason":"disconnected"|"password-changed"|"stopped"}`;
//! - page to server: `{"type":"key","key":n,"pressed":true|false}`.
//!
//! Keys are 1 to 15, row-major from the top-left, five columns. A newly
//! connected page first gets the snapshot, then every published frame.
//!
//! Login (SCRAM-SHA-256 style; the password never travels): `POST
//! /login/start` answers `{"salt":"<base64>","iterations":n,"nonce":"<64 hex>"}`
//! with a random 32-byte server nonce that only this address may use, once,
//! within 30 seconds. The page derives the keys from the password (see
//! [`Verifier`]) in plain JavaScript, picks a client nonce and sends `POST
//! /login/finish` with `{"nonce","client_nonce","proof":"<base64>"}`, where
//! the proof covers AuthMessage = client nonce + server nonce. A right proof
//! gets 200 `{"server_signature":"<base64>"}`, which the page checks, and the
//! session cookie; the page then loads `/` only once `GET /` shows that the
//! browser kept the cookie (it says so otherwise); a wrong one 401 `{"error":"Wrong password.","tries_left":n}`;
//! an unknown, used, expired or another address's nonce 400 `{"error":"The
//! login expired. Try again."}`; a locked-out address 429 `{"error":"Too many
//! tries.","retry_after":30}`.
//!
//! Security: the app sets the password for each session and gives the
//! server only its verifier (salt, iterations, StoredKey, ServerKey), held in
//! memory, never the password; nothing about the password is written to disk
//! (the [`settings`] file keeps the port only); each address gets five wrong proofs, then must
//! wait 30 seconds; a session is a random 32-byte cookie, `sdeck_http_session`
//! (`HttpOnly; SameSite=Strict`, no `Secure` on plain HTTP; not the
//! `sdeck_session` of an earlier HTTPS build, whose `Secure` cookie a
//! browser would never let a plain HTTP page replace); the login requests and
//! `POST /logout` refuse a foreign or `null` `Origin`, and the WebSocket
//! needs the cookie and `Origin: http://<Host>`; at most eight viewers; one
//! address may hold at most eight connections that are not (yet) viewers,
//! within 32 open connections in all; each viewer may press keys at most 50
//! times a second (later presses in that second are ignored, releases never
//! are, and a viewer that leaves lets go of every key it held); every
//! request, header, body and WebSocket message is size-limited and every
//! read has a deadline. Plain HTTP is a deliberate trade-off: anyone on the
//! LAN can read the pictures, the presses and the session cookie, and could
//! tamper with the page on its way to a phone; the password itself never
//! crosses the network.
//!
//! The server never touches a device: key input arrives on the `inputs`
//! channel and pictures leave through [`VirtualDeckServer::publish`].
mod access;
mod browser;
mod http;
mod interfaces;
mod password;
mod server;
pub mod settings;
mod socket;
mod viewer;

pub use password::{build_verifier, Verifier, ITERATIONS};

use std::{
    net::{IpAddr, Ipv4Addr},
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    },
    time::SystemTime,
};

/// The port the app offers until the user picks another.
pub const DEFAULT_PORT: u16 = 8765;
/// Pages that may watch the deck at once.
pub const MAX_VIEWERS: usize = 8;
/// The Stream Deck Classic's keys.
const KEYS: usize = 15;

const INDEX_PAGE: &str = include_str!("virtual_deck/page/index.html");
const LOGIN_PAGE: &str = include_str!("virtual_deck/page/login.html");

/// Where the server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// TCP port; 0 picks a free one (see [`VirtualDeckServer::local_port`]).
    pub port: u16,
    /// Address to listen on; `0.0.0.0` listens on every interface.
    pub bind: IpAddr,
}

impl Default for ServerConfig {
    /// Port 8765 on every interface.
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        }
    }
}

/// A key pressed or released on a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeckInput {
    /// 1 to 15.
    pub key: u8,
    pub pressed: bool,
}

/// What the deck shows, sent to every page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeckFrame {
    /// The upright picture of key 1 to 15.
    Key { key: u8, jpeg: Vec<u8> },
    /// The upright keypad background.
    Background { jpeg: Vec<u8> },
    /// Brightness, 0 to 100.
    Brightness(u8),
    /// The deck was blanked.
    Reset,
}

/// A page connected to the deck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub id: u64,
    pub address: IpAddr,
    /// For example "Android · Chrome 131"; "Unknown browser" when unknown.
    pub browser: String,
    /// When the page connected.
    pub since: SystemTime,
}

/// Something the app may want to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    /// A viewer connected or left; read [`VirtualDeckServer::viewers`].
    ViewersChanged,
    Error(String),
}

/// Returns the deck's current pictures for a page that just connected.
pub type Snapshot = Box<dyn Fn() -> Vec<DeckFrame> + Send + Sync>;

/// The running server. Dropping it stops it like [`VirtualDeckServer::stop`].
pub struct VirtualDeckServer {
    running: server::Running,
    bind: IpAddr,
    lan: Vec<Ipv4Addr>,
}

impl VirtualDeckServer {
    /// Starts listening and returns once the port is bound. `verifier`
    /// comes from [`build_verifier`]; the server keeps it in memory only.
    pub fn start(
        config: ServerConfig,
        verifier: Verifier,
        inputs: Sender<DeckInput>,
        snapshot: Snapshot,
    ) -> Result<Self, String> {
        let shared = Arc::new(server::Shared::new(verifier, inputs, snapshot));
        let running = server::Running::start(config.bind, config.port, shared)?;
        Ok(Self {
            running,
            bind: config.bind,
            lan: interfaces::lan_ipv4(),
        })
    }

    /// The bound port (useful with port 0).
    pub fn local_port(&self) -> u16 {
        self.running.port
    }

    /// Sends `frame` to every connected page.
    pub fn publish(&self, frame: DeckFrame) {
        self.running.shared.publish(&frame);
    }

    /// The connected pages, oldest first.
    pub fn viewers(&self) -> Vec<Viewer> {
        self.running.shared.viewers()
    }

    /// Says bye ("disconnected") to viewer `id` and ends its login session,
    /// closing the other pages of that session too.
    pub fn disconnect(&self, id: u64) {
        self.running.shared.disconnect(id);
    }

    /// Replaces the password verifier; every session ends, every unanswered
    /// login challenge is void and every viewer gets bye "password-changed".
    pub fn set_password(&self, verifier: Verifier) {
        self.running.shared.set_password(verifier);
    }

    /// A new receiver of every later event; each call makes another one.
    pub fn events(&self) -> Receiver<ServerEvent> {
        self.running.shared.subscribe()
    }

    /// The page addresses to offer: `http://<lan-ip>:<port>/`, best first.
    pub fn url_candidates(&self) -> Vec<String> {
        let port = self.local_port();
        match self.bind {
            IpAddr::V6(address) if !address.is_unspecified() => {
                vec![format!("http://[{address}]:{port}/")]
            }
            IpAddr::V4(address) if !address.is_unspecified() => {
                vec![format!("http://{address}:{port}/")]
            }
            _ if self.lan.is_empty() => vec![format!("http://127.0.0.1:{port}/")],
            _ => self
                .lan
                .iter()
                .map(|address| format!("http://{address}:{port}/"))
                .collect(),
        }
    }

    /// Says bye ("stopped") to every viewer, closes the listener and every
    /// connection, and returns after every server thread has ended.
    pub fn stop(mut self) {
        self.running.stop();
    }
}

impl Drop for VirtualDeckServer {
    fn drop(&mut self) {
        self.running.stop();
    }
}
