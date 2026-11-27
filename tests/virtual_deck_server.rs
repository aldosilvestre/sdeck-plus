//! The virtual Stream Deck Classic's HTTPS + WSS server (S4, S5, S1 clean stop).
//!
//! Every test starts a real server on 127.0.0.1 with an ephemeral port and a
//! scratch config directory, then talks to it with a real rustls client and a
//! tungstenite WebSocket client, through the public `sdeck_plus::virtual_deck`
//! API only. Nothing reaches a device or the network beyond loopback.
use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};
use sdeck_plus::virtual_deck::{
    hash_password, DeckFrame, DeckInput, ServerConfig, ServerEvent, VirtualDeckServer,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{ErrorKind, Read, Write},
    net::{IpAddr, Ipv4Addr, TcpStream},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, OnceLock,
    },
    thread,
    time::{Duration, SystemTime},
};
use tungstenite::{client::IntoClientRequest, handshake::HandshakeError, Message, WebSocket};

const PASSWORD: &str = "correct horse";
/// Upper bound of every wait on a real signal; never a fixed sleep.
const WAIT: Duration = Duration::from_secs(20);
/// The page policy; `connect-src` also names `wss://<Host>` for older Safari.
fn csp(port: u16) -> String {
    format!(
        "default-src 'self'; img-src 'self' data: blob:; \
         connect-src 'self' wss://127.0.0.1:{port}; \
         style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'"
    )
}
const ANDROID_CHROME: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/131.0.6778.135 Mobile Safari/537.36";
const MAC_SAFARI: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15";

static NEXT: AtomicU64 = AtomicU64::new(0);

type Tls = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;
type Socket = WebSocket<Tls>;

/// A scratch config directory under the system temp dir, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sdeck-virtual-deck-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One hash for the whole binary: Argon2id is deliberately slow.
fn password_hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password(PASSWORD).expect("hash the test password"))
        .clone()
}

fn config(dir: &std::path::Path, port: u16) -> ServerConfig {
    ServerConfig {
        port,
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        config_dir: dir.to_path_buf(),
    }
}

struct Harness {
    server: Option<VirtualDeckServer>,
    inputs: mpsc::Receiver<DeckInput>,
    port: u16,
    tls: Arc<rustls::ClientConfig>,
    scratch: Scratch,
}

impl Harness {
    fn start() -> Self {
        Self::with_snapshot(Vec::new())
    }

    fn with_snapshot(frames: Vec<DeckFrame>) -> Self {
        let scratch = Scratch::new();
        let (sender, inputs) = mpsc::channel();
        let server = VirtualDeckServer::start(
            config(&scratch.0, 0),
            password_hash(),
            sender,
            Box::new(move || frames.clone()),
        )
        .expect("start the virtual deck server");
        let port = server.local_port();
        assert_ne!(port, 0, "port 0 binds an ephemeral port");
        let tls = client_config(&scratch.0);
        Self {
            server: Some(server),
            inputs,
            port,
            tls,
            scratch,
        }
    }

    fn server(&self) -> &VirtualDeckServer {
        self.server.as_ref().unwrap()
    }

    fn connect(&self) -> Tls {
        let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).unwrap();
        tcp.set_read_timeout(Some(WAIT)).unwrap();
        tcp.set_write_timeout(Some(WAIT)).unwrap();
        let name = ServerName::from(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let connection = rustls::ClientConnection::new(self.tls.clone(), name).unwrap();
        rustls::StreamOwned::new(connection, tcp)
    }

    fn origin(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    fn request(&self, raw: &str) -> Response {
        let mut stream = self.connect();
        stream.write_all(raw.as_bytes()).unwrap();
        stream.flush().unwrap();
        read_response(&mut stream)
    }

    fn get(&self, path: &str, cookie: Option<&str>) -> Response {
        let cookie = cookie
            .map(|c| format!("Cookie: {c}\r\n"))
            .unwrap_or_default();
        self.request(&format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{cookie}\r\n",
            self.port
        ))
    }

    fn post(&self, path: &str, body: &str, cookie: Option<&str>) -> Response {
        let cookie = cookie
            .map(|c| format!("Cookie: {c}\r\n"))
            .unwrap_or_default();
        self.request(&format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
             Content-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\n{cookie}\r\n{body}",
            body.len(),
            port = self.port
        ))
    }

    fn login_with(&self, password: &str) -> Response {
        self.post(
            "/login",
            &format!("password={}", password.replace(' ', "+")),
            None,
        )
    }

    /// Logs in with the right password and returns the `Cookie` header value.
    fn login(&self) -> String {
        let response = self.login_with(PASSWORD);
        assert_eq!(response.status, 303, "{response:?}");
        session_cookie(&response)
    }

    fn open_viewer(
        &self,
        cookie: Option<&str>,
        origin: Option<&str>,
        user_agent: &str,
    ) -> Result<Socket, u16> {
        let mut request = format!("wss://127.0.0.1:{}/ws", self.port)
            .into_client_request()
            .unwrap();
        let headers = request.headers_mut();
        headers.insert("User-Agent", user_agent.parse().unwrap());
        if let Some(cookie) = cookie {
            headers.insert("Cookie", cookie.parse().unwrap());
        }
        if let Some(origin) = origin {
            headers.insert("Origin", origin.parse().unwrap());
        }
        match tungstenite::client(request, self.connect()) {
            Ok((socket, _)) => Ok(socket),
            Err(HandshakeError::Failure(tungstenite::Error::Http(response))) => {
                Err(response.status().as_u16())
            }
            Err(error) => panic!("unexpected handshake error: {error}"),
        }
    }

    /// A logged-in viewer with the right Origin.
    fn viewer(&self, user_agent: &str) -> (Socket, String) {
        let cookie = self.login();
        let socket = self
            .open_viewer(Some(&cookie), Some(&self.origin()), user_agent)
            .expect("the viewer connects");
        (socket, cookie)
    }

    fn stop(&mut self) {
        let server = self.server.take().unwrap();
        let (done, finished) = mpsc::channel();
        thread::spawn(move || {
            server.stop();
            let _ = done.send(());
        });
        finished
            .recv_timeout(WAIT)
            .expect("stop() returns after closing every connection");
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.stop();
        }
    }
}

fn client_config(config_dir: &std::path::Path) -> Arc<rustls::ClientConfig> {
    let cert = CertificateDer::from_pem_file(config_dir.join("virtual/cert.pem"))
        .expect("the server stored its certificate");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).expect("trust the self-signed certificate");
    Arc::new(
        rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth(),
    )
}

#[derive(Debug)]
struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("a JSON body")
    }
}

fn read_response(stream: &mut impl Read) -> Response {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(error) if error.kind() == ErrorKind::UnexpectedEof && !raw.is_empty() => break,
            Err(error) => panic!("reading the response failed: {error}"),
        }
    }
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a complete response head");
    let head = String::from_utf8(raw[..split].to_vec()).unwrap();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.trim().to_string(), value.trim().to_string())
        })
        .collect();
    Response {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

fn session_cookie(response: &Response) -> String {
    let set_cookie = response.header("Set-Cookie").expect("a session cookie");
    set_cookie.split(';').next().unwrap().trim().to_string()
}

/// The next JSON text frame, skipping control frames.
fn next_json(socket: &mut Socket) -> Value {
    loop {
        match socket.read().expect("a frame from the server") {
            Message::Text(text) => return serde_json::from_str(&text).expect("JSON frame"),
            Message::Ping(_) | Message::Pong(_) => {}
            other => panic!("unexpected message {other:?}"),
        }
    }
}

/// Reads until the server closes the WebSocket; returns whether it did.
fn closed_by_server(socket: &mut Socket) -> bool {
    loop {
        match socket.read() {
            Ok(Message::Close(_)) | Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
            Ok(other) => panic!("unexpected message after bye: {other:?}"),
            Err(tungstenite::Error::ConnectionClosed)
            | Err(tungstenite::Error::AlreadyClosed)
            | Err(tungstenite::Error::Protocol(_)) => return true,
            Err(tungstenite::Error::Io(error)) => {
                return !matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
            }
            Err(other) => panic!("unexpected error: {other}"),
        }
    }
}

fn send_key(socket: &mut Socket, key: u8, pressed: bool) {
    let text = json!({"type": "key", "key": key, "pressed": pressed}).to_string();
    socket.send(Message::text(text)).unwrap();
}

fn wait_viewers_changed(events: &mpsc::Receiver<ServerEvent>) {
    match events.recv_timeout(WAIT).expect("a server event") {
        ServerEvent::ViewersChanged => {}
        ServerEvent::Error(error) => panic!("server error: {error}"),
    }
}

fn sha256_fingerprint(der: &[u8]) -> String {
    let hex: Vec<String> = Sha256::digest(der)
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    format!("SHA-256 {}", hex.join(":"))
}

// ---- password ----

#[test]
fn hash_password_rejects_seven_characters_with_the_exact_message() {
    assert_eq!(
        hash_password("1234567"),
        Err("At least 8 characters.".to_string())
    );
    // Characters, not bytes: seven two-byte letters are still too short.
    assert_eq!(
        hash_password("ñññññññ"),
        Err("At least 8 characters.".to_string())
    );
}

#[test]
fn hash_password_returns_a_salted_argon2id_phc_string() {
    let first = hash_password("12345678").unwrap();
    let second = hash_password("12345678").unwrap();
    assert!(first.starts_with("$argon2id$"), "{first}");
    assert_ne!(first, second, "every hash has its own salt");
    assert!(!first.contains("12345678"), "the password is never stored");
}

#[test]
fn start_rejects_a_value_that_is_not_an_argon2id_hash() {
    let scratch = Scratch::new();
    let (sender, _inputs) = mpsc::channel();
    let result = VirtualDeckServer::start(
        config(&scratch.0, 0),
        PASSWORD.to_string(),
        sender,
        Box::new(Vec::new),
    );
    assert!(result.is_err());
}

// ---- login ----

#[test]
fn a_wrong_password_answers_401_with_the_tries_left() {
    let harness = Harness::start();
    let response = harness.login_with("wrong password");
    assert_eq!(response.status, 401);
    assert_eq!(
        response.json(),
        json!({"error": "Wrong password.", "tries_left": 4})
    );
    assert_eq!(response.header("Set-Cookie"), None);
}

#[test]
fn the_sixth_try_after_five_failures_answers_429_even_with_the_right_password() {
    let harness = Harness::start();
    for tries_left in (0..5).rev() {
        let response = harness.login_with("wrong password");
        assert_eq!(response.status, 401);
        assert_eq!(response.json()["tries_left"], json!(tries_left));
    }
    let response = harness.login_with(PASSWORD);
    assert_eq!(response.status, 429);
    assert_eq!(
        response.json(),
        json!({"error": "Too many tries.", "retry_after": 30})
    );
    assert_eq!(response.header("Retry-After"), Some("30"));
    assert_eq!(response.header("Set-Cookie"), None);
}

#[test]
fn the_right_password_sets_a_private_session_cookie_and_redirects_home() {
    let harness = Harness::start();
    let response = harness.login_with(PASSWORD);
    assert_eq!(response.status, 303);
    assert_eq!(response.header("Location"), Some("/"));
    let set_cookie = response.header("Set-Cookie").unwrap();
    let (cookie, attributes) = set_cookie.split_once(';').unwrap();
    let token = cookie.strip_prefix("sdeck_session=").unwrap();
    assert_eq!(token.len(), 64, "32 random bytes as hex");
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let attributes: Vec<&str> = attributes.split(';').map(str::trim).collect();
    for wanted in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
        assert!(attributes.contains(&wanted), "{set_cookie}");
    }
    // A JSON login body works too.
    let body = json!({"password": PASSWORD}).to_string();
    let json_login = harness.request(&format!(
        "POST /login HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{body}",
        harness.port,
        body.len()
    ));
    assert_eq!(json_login.status, 303);
}

#[test]
fn home_serves_the_login_page_without_a_session_and_the_deck_with_one() {
    let harness = Harness::start();
    let login_page = fs::read("src/virtual_deck/page/login.html").unwrap();
    let deck_page = fs::read("src/virtual_deck/page/index.html").unwrap();

    let anonymous = harness.get("/", None);
    assert_eq!(anonymous.status, 200);
    assert_eq!(anonymous.body, login_page);
    assert_eq!(
        anonymous.header("Content-Type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(
        anonymous.header("Content-Security-Policy"),
        Some(csp(harness.port).as_str())
    );
    assert_eq!(anonymous.header("X-Frame-Options"), Some("DENY"));
    assert_eq!(anonymous.header("Cache-Control"), Some("no-store"));
    assert_eq!(anonymous.header("Referrer-Policy"), Some("no-referrer"));

    let cookie = harness.login();
    let logged_in = harness.get("/", Some(&cookie));
    assert_eq!(logged_in.status, 200);
    assert_eq!(logged_in.body, deck_page);

    let forged = harness.get("/", Some(&format!("sdeck_session={}", "ab".repeat(32))));
    assert_eq!(forged.body, login_page, "an unknown token is no session");

    assert_eq!(harness.get("/missing", None).status, 404);
}

#[test]
fn logout_drops_the_session_and_redirects_home() {
    let harness = Harness::start();
    let cookie = harness.login();
    let response = harness.post("/logout", "", Some(&cookie));
    assert_eq!(response.status, 303);
    assert_eq!(response.header("Location"), Some("/"));
    assert!(response.header("Set-Cookie").unwrap().contains("Max-Age=0"));
    assert_eq!(
        harness
            .open_viewer(Some(&cookie), Some(&harness.origin()), ANDROID_CHROME)
            .err(),
        Some(403)
    );
}

#[test]
fn an_oversized_request_head_is_refused() {
    let harness = Harness::start();
    let response = harness.request(&format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Padding: {}\r\n\r\n",
        harness.port,
        "a".repeat(16 * 1024)
    ));
    assert_eq!(response.status, 431);
}

#[test]
fn an_oversized_login_body_is_refused_without_counting_a_try() {
    let harness = Harness::start();
    let response = harness.request(&format!(
        "POST /login HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 5000\r\n\r\n",
        harness.port
    ));
    assert_eq!(response.status, 413);
    assert_eq!(
        harness.login_with("wrong password").json()["tries_left"],
        json!(4)
    );
}

// ---- WebSocket admission ----

#[test]
fn a_websocket_without_the_session_cookie_is_forbidden() {
    let harness = Harness::start();
    let status = harness
        .open_viewer(None, Some(&harness.origin()), ANDROID_CHROME)
        .err();
    assert_eq!(status, Some(403));
    assert!(harness.server().viewers().is_empty());
    assert!(harness.inputs.try_recv().is_err(), "no DeckInput emitted");
}

#[test]
fn a_websocket_from_a_foreign_or_missing_origin_is_forbidden() {
    let harness = Harness::start();
    let cookie = harness.login();
    let foreign = harness.open_viewer(Some(&cookie), Some("https://evil.example"), ANDROID_CHROME);
    assert_eq!(foreign.err(), Some(403));
    let plain_http = harness.open_viewer(
        Some(&cookie),
        Some(&format!("http://127.0.0.1:{}", harness.port)),
        ANDROID_CHROME,
    );
    assert_eq!(plain_http.err(), Some(403));
    let missing = harness.open_viewer(Some(&cookie), None, ANDROID_CHROME);
    assert_eq!(missing.err(), Some(403));
    assert!(harness.server().viewers().is_empty());
    assert!(harness.inputs.try_recv().is_err(), "no DeckInput emitted");
}

#[test]
fn at_most_eight_viewers_connect() {
    let harness = Harness::start();
    let cookie = harness.login();
    let viewers: Vec<Socket> = (0..8)
        .map(|_| {
            harness
                .open_viewer(Some(&cookie), Some(&harness.origin()), ANDROID_CHROME)
                .expect("viewers one to eight connect")
        })
        .collect();
    let ninth = harness.open_viewer(Some(&cookie), Some(&harness.origin()), ANDROID_CHROME);
    assert_eq!(ninth.err(), Some(503));
    assert_eq!(harness.server().viewers().len(), 8);
    drop(viewers);
}

// ---- WebSocket protocol ----

#[test]
fn a_key_message_becomes_a_deck_input_and_bad_keys_are_ignored() {
    let harness = Harness::start();
    let (mut socket, _) = harness.viewer(ANDROID_CHROME);
    send_key(&mut socket, 16, true);
    send_key(&mut socket, 0, true);
    socket
        .send(Message::text(r#"{"type":"key","key":"3"}"#))
        .unwrap();
    send_key(&mut socket, 3, true);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 3,
            pressed: true
        }
    );
    send_key(&mut socket, 3, false);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 3,
            pressed: false
        }
    );
    send_key(&mut socket, 15, true);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 15,
            pressed: true
        }
    );
}

#[test]
fn the_snapshot_is_sent_on_connect_then_published_frames_follow() {
    let harness = Harness::with_snapshot(vec![
        DeckFrame::Brightness(60),
        DeckFrame::Background { jpeg: vec![9] },
        DeckFrame::Key {
            key: 1,
            jpeg: vec![1, 2, 3],
        },
    ]);
    let (mut socket, _) = harness.viewer(ANDROID_CHROME);
    assert_eq!(
        next_json(&mut socket),
        json!({"type": "brightness", "percent": 60})
    );
    assert_eq!(
        next_json(&mut socket),
        json!({"type": "background", "jpeg": "CQ=="})
    );
    assert_eq!(
        next_json(&mut socket),
        json!({"type": "key", "key": 1, "jpeg": "AQID"})
    );

    harness.server().publish(DeckFrame::Key {
        key: 2,
        jpeg: vec![0xff, 0xd8],
    });
    assert_eq!(
        next_json(&mut socket),
        json!({"type": "key", "key": 2, "jpeg": "/9g="})
    );
    harness.server().publish(DeckFrame::Reset);
    assert_eq!(next_json(&mut socket), json!({"type": "reset"}));
}

#[test]
fn viewers_lists_each_client_with_its_address_browser_and_start() {
    let harness = Harness::start();
    let events = harness.server().events();
    let before = SystemTime::now();
    let (_android, _) = harness.viewer(ANDROID_CHROME);
    wait_viewers_changed(&events);
    let (_mac, _) = harness.viewer(MAC_SAFARI);
    wait_viewers_changed(&events);
    let (_other, _) = harness.viewer("curl/8.11.0");
    wait_viewers_changed(&events);
    let after = SystemTime::now();

    let viewers = harness.server().viewers();
    let browsers: Vec<&str> = viewers
        .iter()
        .map(|viewer| viewer.browser.as_str())
        .collect();
    assert_eq!(
        browsers,
        [
            "Android · Chrome 131",
            "macOS · Safari 18",
            "Unknown browser"
        ]
    );
    for viewer in &viewers {
        assert_eq!(viewer.address, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(viewer.since >= before && viewer.since <= after);
    }
    let mut ids: Vec<u64> = viewers.iter().map(|viewer| viewer.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 3, "ids are unique");
}

#[test]
fn disconnect_sends_bye_removes_the_viewer_and_drops_its_session() {
    let harness = Harness::start();
    let events = harness.server().events();
    let (mut socket, cookie) = harness.viewer(ANDROID_CHROME);
    wait_viewers_changed(&events);
    send_key(&mut socket, 5, true);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 5,
            pressed: true
        }
    );

    let id = harness.server().viewers()[0].id;
    harness.server().disconnect(id);
    assert!(harness.server().viewers().is_empty());
    wait_viewers_changed(&events);
    assert_eq!(
        next_json(&mut socket),
        json!({"type": "bye", "reason": "disconnected"})
    );
    assert!(closed_by_server(&mut socket));
    // A key held by a viewer that leaves is released.
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 5,
            pressed: false
        }
    );
    assert_eq!(
        harness
            .open_viewer(Some(&cookie), Some(&harness.origin()), ANDROID_CHROME)
            .err(),
        Some(403),
        "the disconnected viewer's session is gone"
    );
}

#[test]
fn set_password_disconnects_every_viewer_and_requires_the_new_password() {
    let harness = Harness::start();
    let events = harness.server().events();
    let (mut first, first_cookie) = harness.viewer(ANDROID_CHROME);
    wait_viewers_changed(&events);
    let (mut second, _) = harness.viewer(MAC_SAFARI);
    wait_viewers_changed(&events);

    let new_password = "another secret";
    harness
        .server()
        .set_password(hash_password(new_password).unwrap())
        .unwrap();
    assert!(harness.server().viewers().is_empty());
    wait_viewers_changed(&events);
    for socket in [&mut first, &mut second] {
        assert_eq!(
            next_json(socket),
            json!({"type": "bye", "reason": "password-changed"})
        );
        assert!(closed_by_server(socket));
    }
    assert_eq!(
        harness
            .open_viewer(Some(&first_cookie), Some(&harness.origin()), ANDROID_CHROME)
            .err(),
        Some(403)
    );
    assert_eq!(harness.login_with(PASSWORD).status, 401);
    assert_eq!(harness.login_with(new_password).status, 303);
    assert!(harness.server().set_password("not a hash".into()).is_err());
    assert_eq!(harness.login_with(new_password).status, 303);
}

#[test]
fn a_client_that_drops_its_connection_releases_the_keys_it_held() {
    let harness = Harness::start();
    let events = harness.server().events();
    let (mut socket, _) = harness.viewer(ANDROID_CHROME);
    wait_viewers_changed(&events);
    send_key(&mut socket, 7, true);
    send_key(&mut socket, 12, true);
    let mut pressed = vec![
        harness.inputs.recv_timeout(WAIT).unwrap(),
        harness.inputs.recv_timeout(WAIT).unwrap(),
    ];
    pressed.sort_by_key(|input| input.key);
    assert_eq!(
        pressed,
        [
            DeckInput {
                key: 7,
                pressed: true
            },
            DeckInput {
                key: 12,
                pressed: true
            }
        ]
    );

    // No WebSocket close, no TLS close_notify: the phone just vanishes.
    drop(socket);
    let mut released = vec![
        harness.inputs.recv_timeout(WAIT).unwrap(),
        harness.inputs.recv_timeout(WAIT).unwrap(),
    ];
    released.sort_by_key(|input| input.key);
    assert_eq!(
        released,
        [
            DeckInput {
                key: 7,
                pressed: false
            },
            DeckInput {
                key: 12,
                pressed: false
            }
        ]
    );
    wait_viewers_changed(&events);
    assert!(harness.server().viewers().is_empty());
    assert!(
        harness.inputs.try_recv().is_err(),
        "each key is released once"
    );
}

#[test]
fn a_key_held_by_two_viewers_goes_up_when_the_last_one_leaves() {
    let harness = Harness::start();
    let (mut first, _) = harness.viewer(ANDROID_CHROME);
    let (mut second, _) = harness.viewer(MAC_SAFARI);
    send_key(&mut first, 4, true);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 4,
            pressed: true
        }
    );
    send_key(&mut second, 4, true);
    // A marker key proves the second press of key 4 was handled and hidden.
    send_key(&mut second, 1, true);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 1,
            pressed: true
        }
    );

    drop(first);
    send_key(&mut second, 1, false);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 1,
            pressed: false
        },
        "key 4 is still held by the second viewer"
    );
    // The marker can arrive before the first viewer's thread has ended, so
    // wait for it to be gone before dropping the second one.
    let events = harness.server().events();
    while harness.server().viewers().len() > 1 {
        wait_viewers_changed(&events);
    }
    assert!(harness.inputs.try_recv().is_err(), "key 4 stayed down");

    drop(second);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 4,
            pressed: false
        }
    );
}

#[test]
fn an_oversized_page_message_ends_the_viewer() {
    let harness = Harness::start();
    let events = harness.server().events();
    let (mut socket, _) = harness.viewer(ANDROID_CHROME);
    wait_viewers_changed(&events);
    socket.send(Message::text("x".repeat(2048))).unwrap();
    assert!(closed_by_server(&mut socket));
    wait_viewers_changed(&events);
    assert!(harness.server().viewers().is_empty());
    assert!(harness.inputs.try_recv().is_err(), "no DeckInput emitted");
}

#[test]
fn a_viewer_that_closes_is_removed() {
    let harness = Harness::start();
    let events = harness.server().events();
    let (mut socket, _) = harness.viewer(ANDROID_CHROME);
    wait_viewers_changed(&events);
    socket.close(None).unwrap();
    while socket.read().is_ok() {}
    wait_viewers_changed(&events);
    assert!(harness.server().viewers().is_empty());
}

// ---- stop ----

#[test]
fn stop_says_bye_closes_every_connection_and_joins_every_thread() {
    let mut harness = Harness::start();
    let (mut socket, _) = harness.viewer(ANDROID_CHROME);
    send_key(&mut socket, 9, true);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 9,
            pressed: true
        }
    );
    // A client that never starts its TLS handshake must not keep stop() waiting.
    let mut idle = TcpStream::connect((Ipv4Addr::LOCALHOST, harness.port)).unwrap();
    idle.set_read_timeout(Some(WAIT)).unwrap();
    // Nor one that stops in the middle of its request head.
    let mut half = harness.connect();
    half.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").unwrap();
    half.flush().unwrap();

    harness.stop();

    assert_eq!(
        next_json(&mut socket),
        json!({"type": "bye", "reason": "stopped"})
    );
    assert!(closed_by_server(&mut socket));
    let mut byte = [0u8; 1];
    match idle.read(&mut byte) {
        Ok(0) => {}
        Err(error) if error.kind() == ErrorKind::ConnectionReset => {}
        other => panic!("the idle connection stays open: {other:?}"),
    }
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, harness.port)).is_err(),
        "the listener is closed"
    );
    // The key the viewer held went up before its thread ended.
    assert_eq!(
        harness.inputs.recv_timeout(WAIT),
        Ok(DeckInput {
            key: 9,
            pressed: false
        })
    );
    // Every thread holding the input sender has ended.
    assert_eq!(
        harness.inputs.recv_timeout(WAIT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}

// ---- certificate ----

#[test]
fn the_certificate_and_its_private_key_are_stored_privately() {
    let harness = Harness::start();
    let dir = harness.scratch.0.join("virtual");
    let mode = |path: PathBuf| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(dir.clone()), 0o700);
    assert_eq!(mode(dir.join("key.pem")), 0o600);
    assert!(dir.join("cert.pem").is_file());
    let key = fs::read_to_string(dir.join("key.pem")).unwrap();
    assert!(key.contains("PRIVATE KEY"));
}

#[test]
fn the_fingerprint_is_the_sha256_of_the_served_certificate() {
    let harness = Harness::start();
    let mut stream = harness.connect();
    while stream.conn.is_handshaking() {
        stream.conn.complete_io(&mut stream.sock).unwrap();
    }
    let served = stream.conn.peer_certificates().unwrap()[0].to_vec();
    let fingerprint = harness.server().fingerprint();
    assert_eq!(fingerprint, sha256_fingerprint(&served));
    assert_eq!(fingerprint.len(), "SHA-256 ".len() + 32 * 3 - 1);
}

#[test]
fn the_certificate_is_kept_while_the_names_match_and_replaced_when_they_change() {
    let scratch = Scratch::new();
    let start = |dir: &std::path::Path| {
        let (sender, _inputs) = mpsc::channel();
        VirtualDeckServer::start(config(dir, 0), password_hash(), sender, Box::new(Vec::new))
            .unwrap()
    };
    let first = start(&scratch.0);
    let fingerprint = first.fingerprint();
    first.stop();
    let second = start(&scratch.0);
    assert_eq!(
        second.fingerprint(),
        fingerprint,
        "same names, same certificate"
    );
    second.stop();

    // A certificate for other names (an address the computer no longer has).
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let stale = rcgen::CertificateParams::new(vec!["10.255.255.254".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let dir = scratch.0.join("virtual");
    fs::write(dir.join("cert.pem"), stale.pem()).unwrap();
    fs::write(dir.join("key.pem"), key.serialize_pem()).unwrap();
    let third = start(&scratch.0);
    assert_ne!(third.fingerprint(), sha256_fingerprint(stale.der()));
    assert_ne!(third.fingerprint(), fingerprint);
    third.stop();
    let mode = fs::metadata(dir.join("key.pem"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn url_candidates_name_the_bound_address_and_port() {
    let harness = Harness::start();
    assert_eq!(
        harness.server().url_candidates(),
        vec![format!("https://127.0.0.1:{}/", harness.port)]
    );
}

#[test]
fn a_port_in_use_is_reported() {
    let harness = Harness::start();
    let scratch = Scratch::new();
    let (sender, _inputs) = mpsc::channel();
    let error = VirtualDeckServer::start(
        config(&scratch.0, harness.port),
        password_hash(),
        sender,
        Box::new(Vec::new),
    )
    .err()
    .expect("the port is taken");
    assert_eq!(error, format!("Port {} is already in use.", harness.port));
}
