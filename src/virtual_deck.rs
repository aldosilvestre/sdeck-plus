//! The virtual Stream Deck Classic's web server: HTTPS + WSS on the LAN.
//!
//! A phone or browser opens `https://<lan-ip>:<port>/`, logs in with the
//! password the app set, and gets the deck page; the page then opens
//! `wss://<lan-ip>:<port>/ws` and exchanges JSON text frames:
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
//! Security: the certificate is self-signed (ECDSA P-256, `rustls` with the
//! `ring` provider) and its key is stored 0600 in a 0700 directory; only an
//! Argon2id hash of the password is held; each address gets five wrong
//! passwords, then must wait 30 seconds; a session is a random 32-byte cookie
//! (`HttpOnly; Secure; SameSite=Strict`); the WebSocket needs that cookie and
//! `Origin: https://<Host>`; at most eight viewers; one address may hold at
//! most eight connections that are not (yet) viewers, within 32 open
//! connections in all; each viewer may press keys at most 50 times a second
//! (later presses in that second are ignored, releases never are, and a
//! viewer that leaves lets go of every key it held); every request, header,
//! body and WebSocket message is size-limited and every read has a deadline.
//!
//! The server never touches a device: key input arrives on the `inputs`
//! channel and pictures leave through [`VirtualDeckServer::publish`].
mod access;
mod browser;
mod certificate;
mod http;
mod interfaces;
mod password;
mod server;
pub mod settings;
mod socket;
mod viewer;
mod x509;

pub use password::hash_password;

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
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

/// Where and how the server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// TCP port; 0 picks a free one (see [`VirtualDeckServer::local_port`]).
    pub port: u16,
    /// Address to listen on; `0.0.0.0` listens on every interface.
    pub bind: IpAddr,
    /// The app's config directory; the certificate lives in `virtual/` inside it.
    pub config_dir: PathBuf,
}

impl ServerConfig {
    /// Port 8765 on every interface.
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            port: DEFAULT_PORT,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            config_dir,
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
    fingerprint: String,
}

impl VirtualDeckServer {
    /// Loads or creates the certificate, starts listening and returns once
    /// the port is bound. `password_hash` comes from [`hash_password`].
    pub fn start(
        config: ServerConfig,
        password_hash: String,
        inputs: Sender<DeckInput>,
        snapshot: Snapshot,
    ) -> Result<Self, String> {
        password::check_hash(&password_hash)?;
        let lan = interfaces::lan_ipv4();
        let names = certificate_names(&lan, config.bind);
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let identity = certificate::load_or_create(&config.config_dir, &names, &provider)?;
        let fingerprint = identity.fingerprint();
        let tls_error = |error: rustls::Error| format!("Could not set up HTTPS: {error}");
        let mut tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(tls_error)?
            .with_no_client_auth()
            .with_single_cert(vec![identity.cert], identity.key)
            .map_err(tls_error)?;
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let shared = Arc::new(server::Shared::new(
            Arc::new(tls),
            password_hash,
            inputs,
            snapshot,
        ));
        let running = server::Running::start(config.bind, config.port, shared)?;
        Ok(Self {
            running,
            bind: config.bind,
            lan,
            fingerprint,
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

    /// Replaces the password hash; every session ends and every viewer gets
    /// bye "password-changed". An invalid hash changes nothing.
    pub fn set_password(&self, password_hash: String) -> Result<(), String> {
        self.running.shared.set_password(password_hash)
    }

    /// A new receiver of every later event; each call makes another one.
    pub fn events(&self) -> Receiver<ServerEvent> {
        self.running.shared.subscribe()
    }

    /// The page addresses to offer: `https://<lan-ip>:<port>/`, best first.
    pub fn url_candidates(&self) -> Vec<String> {
        let port = self.local_port();
        match self.bind {
            IpAddr::V6(address) if !address.is_unspecified() => {
                vec![format!("https://[{address}]:{port}/")]
            }
            IpAddr::V4(address) if !address.is_unspecified() => {
                vec![format!("https://{address}:{port}/")]
            }
            _ if self.lan.is_empty() => vec![format!("https://127.0.0.1:{port}/")],
            _ => self
                .lan
                .iter()
                .map(|address| format!("https://{address}:{port}/"))
                .collect(),
        }
    }

    /// "SHA-256 AA:BB:..." of the served certificate.
    pub fn fingerprint(&self) -> String {
        self.fingerprint.clone()
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

/// localhost, 127.0.0.1, every LAN IPv4 address and a specific bind address.
fn certificate_names(lan: &[Ipv4Addr], bind: IpAddr) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = ["localhost".to_string(), "127.0.0.1".to_string()].into();
    names.extend(lan.iter().map(Ipv4Addr::to_string));
    if !bind.is_unspecified() {
        names.insert(bind.to_string());
    }
    names
}
