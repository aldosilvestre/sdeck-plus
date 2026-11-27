//! The virtual Stream Deck Classic's HTTPS + WSS server (S4, S5, S1 clean stop).
//!
//! Every test starts a real server on 127.0.0.1 with an ephemeral port and a
//! scratch config directory, then talks to it with a real rustls client and a
//! tungstenite WebSocket client, through the public `sdeck_plus::virtual_deck`
//! API only. Nothing reaches a device or the network beyond loopback; a
//! second client address is 127.0.0.2, which Linux routes over loopback too.
use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};
use sdeck_plus::virtual_deck::{
    hash_password, DeckFrame, DeckInput, ServerConfig, ServerEvent, VirtualDeckServer,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, ErrorKind, Read, Write},
    net::{IpAddr, Ipv4Addr, TcpStream},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::PermissionsExt,
    },
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime},
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
/// Another computer on the LAN, as far as the server can tell.
const OTHER_ADDRESS: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);
/// Connections one address may have open before they become viewers.
const PENDING_PER_ADDRESS: usize = 8;
/// Key presses one viewer may make per second.
const PRESSES_PER_SECOND: u64 = 50;

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
        tls_over(&self.tls, tcp)
    }

    /// A TLS connection whose source address is `source`.
    fn connect_from(&self, source: Ipv4Addr) -> Tls {
        tls_over(&self.tls, tcp_from(source, self.port))
    }

    fn origin(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    fn request(&self, raw: &str) -> Response {
        send_request(self.connect(), raw)
    }

    fn request_from(&self, source: Ipv4Addr, raw: &str) -> Response {
        send_request(self.connect_from(source), raw)
    }

    fn login_from(&self, source: Ipv4Addr, password: &str) -> Response {
        self.request_from(source, &login_request(self.port, password))
    }

    /// A POST with an `Origin` header.
    fn post_from_origin(
        &self,
        path: &str,
        body: &str,
        cookie: Option<&str>,
        origin: &str,
    ) -> Response {
        let cookie = cookie
            .map(|c| format!("Cookie: {c}\r\n"))
            .unwrap_or_default();
        self.request(&format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\n\
             Content-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\n{cookie}\r\n{body}",
            body.len(),
            port = self.port
        ))
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
        self.request(&login_request(self.port, password))
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

fn tls_over(tls: &Arc<rustls::ClientConfig>, tcp: TcpStream) -> Tls {
    tcp.set_read_timeout(Some(WAIT)).unwrap();
    tcp.set_write_timeout(Some(WAIT)).unwrap();
    let name = ServerName::from(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let connection = rustls::ClientConnection::new(tls.clone(), name).unwrap();
    rustls::StreamOwned::new(connection, tcp)
}

/// A TCP connection to 127.0.0.1:`port` from `source`; every 127.0.0.0/8
/// address is a loopback address on Linux, so the server sees a different
/// client address without any network beyond loopback.
fn tcp_from(source: Ipv4Addr, port: u16) -> TcpStream {
    fn address(ip: Ipv4Addr, port: u16) -> libc::sockaddr_in {
        libc::sockaddr_in {
            sin_family: libc::AF_INET as libc::sa_family_t,
            sin_port: port.to_be(),
            sin_addr: libc::in_addr {
                s_addr: u32::from(ip).to_be(),
            },
            sin_zero: [0; 8],
        }
    }
    let length = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
    // SAFETY: socket() reads no memory of ours; a non-negative result is a
    // new descriptor that nothing else owns, so OwnedFd may close it.
    let descriptor =
        unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    assert!(descriptor >= 0, "socket: {}", io::Error::last_os_error());
    // SAFETY: see above; `descriptor` is open and owned by nobody else.
    let socket = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let local = address(source, 0);
    // SAFETY: `local` is a valid sockaddr_in of `length` bytes that outlives
    // the call, and the descriptor is open.
    let bound = unsafe { libc::bind(socket.as_raw_fd(), std::ptr::addr_of!(local).cast(), length) };
    assert_eq!(bound, 0, "bind {source}: {}", io::Error::last_os_error());
    let remote = address(Ipv4Addr::LOCALHOST, port);
    // SAFETY: as for bind.
    let connected = unsafe {
        libc::connect(
            socket.as_raw_fd(),
            std::ptr::addr_of!(remote).cast(),
            length,
        )
    };
    assert_eq!(
        connected,
        0,
        "connect from {source}: {}",
        io::Error::last_os_error()
    );
    TcpStream::from(socket)
}

fn login_request(port: u16, password: &str) -> String {
    let body = format!("password={}", password.replace(' ', "+"));
    format!(
        "POST /login HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn send_request(mut stream: Tls, raw: &str) -> Response {
    stream.write_all(raw.as_bytes()).unwrap();
    stream.flush().unwrap();
    read_response(&mut stream)
}

/// Every byte the server sends before it closes (a reset ends it too).
fn read_until_closed(stream: &mut impl Read) -> Vec<u8> {
    let mut received = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => return received,
            Ok(read) => received.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => {
                assert!(
                    !matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
                    "the server kept the connection open"
                );
                return received;
            }
        }
    }
}

/// Bytes that hold no HTTP answer: nothing, or a TLS alert record.
fn assert_no_http_reply(received: &[u8], what: &str) {
    assert!(
        !received.windows(5).any(|window| window == b"HTTP/"),
        "{what}: {received:?}"
    );
    if let Some(first) = received.first() {
        assert_eq!(*first, 0x15, "{what}: only a TLS alert, got {received:?}");
    }
}

/// A ClientHello that offers only `version` (no supported_versions
/// extension) with a suite, groups and signature schemes the server could
/// use with TLS 1.2, so only the version can make it fail.
fn client_hello(version: [u8; 2]) -> Vec<u8> {
    let mut hello = version.to_vec();
    hello.extend_from_slice(&[0x5a; 32]); // random
    hello.push(0); // no session id
                   // TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 and _256_GCM_SHA384.
    hello.extend_from_slice(&[0x00, 0x04, 0xc0, 0x2b, 0xc0, 0x2c]);
    hello.extend_from_slice(&[0x01, 0x00]); // null compression
    let extensions: &[u8] = &[
        0x00, 0x0d, 0x00, 0x06, 0x00, 0x04, 0x04, 0x03, 0x08, 0x04, // signature_algorithms
        0x00, 0x0a, 0x00, 0x06, 0x00, 0x04, 0x00, 0x1d, 0x00, 0x17, // supported_groups
        0x00, 0x0b, 0x00, 0x02, 0x01, 0x00, // ec_point_formats: uncompressed
    ];
    hello.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    hello.extend_from_slice(extensions);
    let mut handshake = vec![0x01, 0x00];
    handshake.extend_from_slice(&(hello.len() as u16).to_be_bytes());
    handshake.extend_from_slice(&hello);
    let mut record = vec![0x16, 0x03, 0x01];
    record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
    record.extend_from_slice(&handshake);
    record
}

/// Whether a request on `stream` gets any HTTP answer at all.
fn answered(mut stream: Tls, raw: &str) -> bool {
    if stream
        .write_all(raw.as_bytes())
        .and_then(|()| stream.flush())
        .is_err()
    {
        return false;
    }
    read_until_closed(&mut stream).starts_with(b"HTTP/")
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
fn hash_password_uses_argon2id_with_19_mib_two_passes_and_one_lane() {
    let hash = hash_password("12345678").unwrap();
    let parsed = argon2::password_hash::PasswordHash::new(&hash).expect("a PHC string");
    assert_eq!(parsed.algorithm.as_str(), "argon2id");
    assert_eq!(parsed.version, Some(19));
    let params = argon2::Params::try_from(&parsed).expect("Argon2 parameters");
    assert_eq!(
        (params.m_cost(), params.t_cost(), params.p_cost()),
        (19_456, 2, 1)
    );
    assert!(
        hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "{hash}"
    );
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
fn a_locked_address_does_not_lock_another_one() {
    let harness = Harness::start();
    for _ in 0..5 {
        assert_eq!(harness.login_with("wrong password").status, 401);
    }
    assert_eq!(harness.login_with(PASSWORD).status, 429);

    let other = harness.login_from(OTHER_ADDRESS, "wrong password");
    assert_eq!(other.status, 401);
    assert_eq!(
        other.json(),
        json!({"error": "Wrong password.", "tries_left": 4})
    );
    let other = harness.login_from(OTHER_ADDRESS, PASSWORD);
    assert_eq!(other.status, 303, "{other:?}");
    assert!(other.header("Set-Cookie").is_some());

    let first = harness.login_with(PASSWORD);
    assert_eq!(first.status, 429, "the first address is still locked");
}

#[test]
fn a_foreign_origin_login_is_forbidden_and_never_counts_as_a_try() {
    let harness = Harness::start();
    for password in ["wrong+password", "correct+horse"] {
        let response = harness.post_from_origin(
            "/login",
            &format!("password={password}"),
            None,
            "https://evil.example",
        );
        assert_eq!(response.status, 403, "{password}");
        assert_eq!(response.json(), json!({"error": "Forbidden."}));
        assert_eq!(response.header("Set-Cookie"), None, "{password}");
    }
    let next = harness.login_with("wrong password");
    assert_eq!(next.status, 401);
    assert_eq!(
        next.json(),
        json!({"error": "Wrong password.", "tries_left": 4}),
        "the forbidden tries were not counted"
    );
}

#[test]
fn a_login_form_posted_with_a_null_origin_still_logs_in() {
    // Browsers send `Origin: null` for the login form's own post, because the
    // page's Referrer-Policy is no-referrer.
    let harness = Harness::start();
    let deck_page = fs::read("src/virtual_deck/page/index.html").unwrap();
    let wrong = harness.post_from_origin("/login", "password=wrong+password", None, "null");
    assert_eq!(wrong.status, 401);
    assert_eq!(wrong.json()["tries_left"], json!(4));
    let response = harness.post_from_origin("/login", "password=correct+horse", None, "null");
    assert_eq!(response.status, 303, "{response:?}");
    let cookie = session_cookie(&response);
    assert_eq!(harness.get("/", Some(&cookie)).body, deck_page);
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
fn logout_with_a_null_or_foreign_origin_is_forbidden_and_keeps_the_session() {
    let harness = Harness::start();
    let deck_page = fs::read("src/virtual_deck/page/index.html").unwrap();
    let cookie = harness.login();
    let plain_http = format!("http://127.0.0.1:{}", harness.port);
    for origin in ["null", "https://evil.example", plain_http.as_str()] {
        let response = harness.post_from_origin("/logout", "", Some(&cookie), origin);
        assert_eq!(response.status, 403, "{origin}");
        assert_eq!(response.json(), json!({"error": "Forbidden."}), "{origin}");
        assert_eq!(response.header("Set-Cookie"), None, "{origin}");
        assert_eq!(
            harness.get("/", Some(&cookie)).body,
            deck_page,
            "{origin}: the session is still live"
        );
    }
    // The page's own logout is a fetch(), which sends its real origin.
    let response = harness.post_from_origin("/logout", "", Some(&cookie), &harness.origin());
    assert_eq!(response.status, 303);
    assert!(response.header("Set-Cookie").unwrap().contains("Max-Age=0"));
    assert_ne!(harness.get("/", Some(&cookie)).body, deck_page);
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
    let null = harness.open_viewer(Some(&cookie), Some("null"), ANDROID_CHROME);
    assert_eq!(null.err(), Some(403));
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

// ---- transport ----

#[test]
fn plaintext_http_gets_no_http_reply() {
    let harness = Harness::start();
    let mut tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, harness.port)).unwrap();
    tcp.set_read_timeout(Some(WAIT)).unwrap();
    tcp.write_all(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n", harness.port).as_bytes())
        .unwrap();
    assert_no_http_reply(&read_until_closed(&mut tcp), "plaintext HTTP");
}

#[test]
fn tls_older_than_1_2_gets_no_http_reply() {
    let harness = Harness::start();
    let connect = || {
        let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, harness.port)).unwrap();
        tcp.set_read_timeout(Some(WAIT)).unwrap();
        tcp
    };
    for (version, name) in [([3, 1], "TLS 1.0"), ([3, 2], "TLS 1.1")] {
        let mut tcp = connect();
        tcp.write_all(&client_hello(version)).unwrap();
        tcp.write_all(
            format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n", harness.port).as_bytes(),
        )
        .unwrap();
        let received = read_until_closed(&mut tcp);
        assert_no_http_reply(&received, name);
        // A fatal protocol_version alert, then the connection ends.
        assert_eq!(received, [0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 70], "{name}");
    }
    // The same hello offering TLS 1.2 gets a ServerHello: only the version
    // was refused.
    let mut tcp = connect();
    tcp.write_all(&client_hello([3, 3])).unwrap();
    let mut header = [0u8; 6];
    tcp.read_exact(&mut header).unwrap();
    assert_eq!(
        &header[..3],
        [0x16, 0x03, 0x03],
        "a TLS 1.2 handshake record"
    );
    assert_eq!(header[5], 0x02, "a ServerHello");
    // A TLS 1.2+ client is still served.
    assert_eq!(harness.get("/", None).status, 200);
}

#[test]
fn one_address_cannot_hold_more_than_eight_unfinished_connections() {
    let harness = Harness::start();
    let request = format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n", harness.port);
    // Connections that never start their TLS handshake.
    let stalled: Vec<TcpStream> = (0..PENDING_PER_ADDRESS)
        .map(|_| TcpStream::connect((Ipv4Addr::LOCALHOST, harness.port)).unwrap())
        .collect();
    assert!(
        !answered(harness.connect(), &request),
        "a ninth unfinished connection from the same address is closed unanswered"
    );
    assert!(
        answered(harness.connect_from(OTHER_ADDRESS), &request),
        "another address is still served"
    );
    // The eight were accepted and are served once they speak.
    for tcp in stalled {
        assert!(answered(tls_over(&harness.tls, tcp), &request));
    }
    // Their slots free up again once they are done.
    let deadline = Instant::now() + WAIT;
    while !answered(harness.connect(), &request) {
        assert!(Instant::now() < deadline, "the address stays shut out");
        thread::yield_now();
    }
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
fn a_viewer_that_floods_key_presses_is_throttled_but_its_releases_always_arrive() {
    let harness = Harness::start();
    let (mut flooder, _) = harness.viewer(ANDROID_CHROME);
    let (mut other, _) = harness.viewer(MAC_SAFARI);
    let started = Instant::now();
    send_key(&mut flooder, 2, true);
    send_key(&mut flooder, 4, true);
    const SENT: usize = 500;
    for _ in 0..SENT {
        send_key(&mut flooder, 1, true);
        send_key(&mut flooder, 1, false);
    }
    // A release always gets through, even in a throttled second; it also
    // marks the end of the flood.
    send_key(&mut flooder, 2, false);
    let released_two = DeckInput {
        key: 2,
        pressed: false,
    };
    let mut delivered = Vec::new();
    loop {
        let input = harness
            .inputs
            .recv_timeout(WAIT)
            .expect("the release of key 2 arrives");
        delivered.push(input);
        if input == released_two {
            break;
        }
    }
    let elapsed = started.elapsed();

    let presses = delivered.iter().filter(|input| input.pressed).count() as u64;
    // A new second of budget starts at most once per elapsed second.
    let allowed = PRESSES_PER_SECOND * (elapsed.as_secs() + 1);
    assert!(
        presses <= allowed,
        "{presses} presses of {} reached the deck in {elapsed:?}; at most {allowed}",
        SENT + 2
    );
    assert_eq!(
        delivered[..2],
        [
            DeckInput {
                key: 2,
                pressed: true
            },
            DeckInput {
                key: 4,
                pressed: true
            }
        ]
    );
    let key_one: Vec<bool> = delivered
        .iter()
        .filter(|input| input.key == 1)
        .map(|input| input.pressed)
        .collect();
    assert!(!key_one.is_empty(), "presses within the budget get through");
    assert!(
        key_one.len().is_multiple_of(2) && key_one.chunks(2).all(|pair| pair == [true, false]),
        "every press of key 1 that got through was released: {key_one:?}"
    );

    // Another viewer has a budget of its own.
    send_key(&mut other, 9, true);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 9,
            pressed: true
        }
    );
    // The keys a throttled viewer holds go up when it leaves.
    drop(flooder);
    assert_eq!(
        harness.inputs.recv_timeout(WAIT).unwrap(),
        DeckInput {
            key: 4,
            pressed: false
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
fn no_session_from_the_old_password_survives_a_password_change() {
    let harness = Harness::start();
    let login_page = fs::read("src/virtual_deck/page/login.html").unwrap();
    let before = harness.login();
    let new_password = "another secret";
    let new_hash = hash_password(new_password).unwrap();

    // A login with the old password, in flight while the password changes:
    // whichever finishes first, it must not leave a live session behind.
    let stream = harness.connect();
    let raw = login_request(harness.port, PASSWORD);
    let (sent, ready) = mpsc::channel();
    let racing = thread::spawn(move || {
        let mut stream = stream;
        stream.write_all(raw.as_bytes()).unwrap();
        stream.flush().unwrap();
        sent.send(()).unwrap();
        read_response(&mut stream)
    });
    ready.recv_timeout(WAIT).expect("the login was sent");
    harness.server().set_password(new_hash).unwrap();
    let raced = racing.join().unwrap();

    let mut cookies = vec![before];
    match raced.status {
        303 => cookies.push(session_cookie(&raced)),
        401 => assert_eq!(raced.header("Set-Cookie"), None),
        other => panic!("unexpected login answer {other}: {raced:?}"),
    }
    for cookie in &cookies {
        assert_eq!(
            harness.get("/", Some(cookie)).body,
            login_page,
            "the old session shows the login page"
        );
        assert_eq!(
            harness
                .open_viewer(Some(cookie), Some(&harness.origin()), ANDROID_CHROME)
                .err(),
            Some(403)
        );
    }
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
fn a_private_key_that_others_can_read_is_replaced_by_a_private_one() {
    let scratch = Scratch::new();
    let start = |dir: &std::path::Path| {
        let (sender, _inputs) = mpsc::channel();
        VirtualDeckServer::start(config(dir, 0), password_hash(), sender, Box::new(Vec::new))
            .unwrap()
    };
    let mode = |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let first = start(&scratch.0);
    let fingerprint = first.fingerprint();
    first.stop();
    let key_path = scratch.0.join("virtual/key.pem");
    let exposed = fs::read(&key_path).unwrap();
    fs::set_permissions(&key_path, fs::Permissions::from_mode(0o644)).unwrap();

    let second = start(&scratch.0);
    assert_ne!(
        second.fingerprint(),
        fingerprint,
        "a new key means a new certificate"
    );
    second.stop();
    assert_eq!(mode(&key_path), 0o600);
    assert_ne!(
        fs::read(&key_path).unwrap(),
        exposed,
        "the key was replaced"
    );
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
