//! The virtual Stream Deck Classic's plain HTTP + WebSocket server (S4, S5,
//! S1 clean stop).
//!
//! Every test starts a real server on 127.0.0.1 with an ephemeral port, then
//! talks to it over plain TCP and with a tungstenite WebSocket client, through
//! the public `sdeck_plus::virtual_deck` API only. The login is the page's
//! challenge-response, implemented here from its definition with `ring`, so
//! the tests also pin the protocol the page speaks. Nothing reaches a device
//! or the network beyond loopback; a second client address is 127.0.0.2,
//! which Linux routes over loopback too.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ring::{digest, hmac, pbkdf2};
use sdeck_plus::virtual_deck::{
    build_verifier, DeckFrame, DeckInput, ServerConfig, ServerEvent, Verifier, VirtualDeckServer,
    ITERATIONS,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{self, ErrorKind, Read, Write},
    net::{IpAddr, Ipv4Addr, TcpStream},
    num::NonZeroU32,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{mpsc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime},
};
use tungstenite::{client::IntoClientRequest, handshake::HandshakeError, Message, WebSocket};

const PASSWORD: &str = "correct horse";
const NEW_PASSWORD: &str = "another secret";
/// Upper bound of every wait on a real signal; never a fixed sleep.
const WAIT: Duration = Duration::from_secs(20);
const EXPIRED: &str = "The login expired. Try again.";
/// The page policy; `connect-src` also names `ws://<Host>` for older Safari.
fn csp(port: u16) -> String {
    format!(
        "default-src 'self'; img-src 'self' data: blob:; \
         connect-src 'self' ws://127.0.0.1:{port}; \
         style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; form-action 'none'"
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

type Socket = WebSocket<TcpStream>;

/// One verifier per password for the whole binary: deriving is slow on purpose.
fn verifier_of(password: &'static str) -> Verifier {
    static VERIFIERS: OnceLock<Mutex<HashMap<&'static str, Verifier>>> = OnceLock::new();
    let verifiers = VERIFIERS.get_or_init(Mutex::default);
    let mut verifiers = verifiers.lock().unwrap();
    verifiers
        .entry(password)
        .or_insert_with(|| build_verifier(password).expect("a verifier of the test password"))
        .clone()
}

fn config(port: u16) -> ServerConfig {
    ServerConfig {
        port,
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
    }
}

// ---- the page's side of the login, from its definition ----

type Key = [u8; 32];

fn hmac_sha256(key: &[u8], data: &[u8]) -> Key {
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data);
    tag.as_ref().try_into().unwrap()
}

fn sha256(data: &[u8]) -> Key {
    digest::digest(&digest::SHA256, data)
        .as_ref()
        .try_into()
        .unwrap()
}

/// SaltedPasswords by password, salt and iterations.
type SaltedCache = Mutex<HashMap<(String, Vec<u8>, u32), Key>>;

/// PBKDF2-HMAC-SHA-256(password, salt, iterations), remembered per input so
/// the many logins of this binary derive each password once.
fn salted_password(password: &str, salt: &[u8], iterations: u32) -> Key {
    static SALTED: OnceLock<SaltedCache> = OnceLock::new();
    let cache = SALTED.get_or_init(Mutex::default);
    let key = (password.to_string(), salt.to_vec(), iterations);
    if let Some(salted) = cache.lock().unwrap().get(&key) {
        return *salted;
    }
    let mut salted = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(iterations).unwrap(),
        salt,
        password.as_bytes(),
        &mut salted,
    );
    cache.lock().unwrap().insert(key, salted);
    salted
}

/// The server's challenge: `{"salt","iterations","nonce"}`.
#[derive(Debug, Clone)]
struct Challenge {
    salt: Vec<u8>,
    iterations: u32,
    nonce: String,
}

impl Challenge {
    fn of(response: &Response) -> Self {
        assert_eq!(response.status, 200, "{response:?}");
        let body = response.json();
        Self {
            salt: STANDARD.decode(body["salt"].as_str().unwrap()).unwrap(),
            iterations: u32::try_from(body["iterations"].as_u64().unwrap()).unwrap(),
            nonce: body["nonce"].as_str().unwrap().to_string(),
        }
    }
}

/// What the page sends for `password` and what it expects back.
struct Answer {
    body: String,
    server_signature: String,
}

fn answer(password: &str, challenge: &Challenge, client_nonce: &str) -> Answer {
    let salted = salted_password(password, &challenge.salt, challenge.iterations);
    let client_key = hmac_sha256(&salted, b"Client Key");
    let stored_key = sha256(&client_key);
    let auth_message = format!("{client_nonce}{}", challenge.nonce);
    let signature = hmac_sha256(&stored_key, auth_message.as_bytes());
    let mut proof = client_key;
    for (byte, signature) in proof.iter_mut().zip(signature) {
        *byte ^= signature;
    }
    let server_key = hmac_sha256(&salted, b"Server Key");
    Answer {
        body: json!({
            "nonce": challenge.nonce,
            "client_nonce": client_nonce,
            "proof": STANDARD.encode(proof),
        })
        .to_string(),
        server_signature: STANDARD.encode(hmac_sha256(&server_key, auth_message.as_bytes())),
    }
}

/// A fresh client nonce, as the page makes one: 32 random bytes in hex.
fn client_nonce() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let bytes = sha256(format!("{}-{count}", std::process::id()).as_bytes());
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A POST with an optional JSON body, `Origin` and cookie.
fn post_request(
    port: u16,
    path: &str,
    body: &str,
    origin: Option<&str>,
    cookie: Option<&str>,
) -> String {
    let origin = origin
        .map(|origin| format!("Origin: {origin}\r\n"))
        .unwrap_or_default();
    let cookie = cookie
        .map(|cookie| format!("Cookie: {cookie}\r\n"))
        .unwrap_or_default();
    format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{origin}{cookie}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

struct Harness {
    server: Option<VirtualDeckServer>,
    inputs: mpsc::Receiver<DeckInput>,
    port: u16,
}

impl Harness {
    fn start() -> Self {
        Self::with_snapshot(Vec::new())
    }

    fn with_snapshot(frames: Vec<DeckFrame>) -> Self {
        let (sender, inputs) = mpsc::channel();
        let server = VirtualDeckServer::start(
            config(0),
            verifier_of(PASSWORD),
            sender,
            Box::new(move || frames.clone()),
        )
        .expect("start the virtual deck server");
        let port = server.local_port();
        assert_ne!(port, 0, "port 0 binds an ephemeral port");
        Self {
            server: Some(server),
            inputs,
            port,
        }
    }

    fn server(&self) -> &VirtualDeckServer {
        self.server.as_ref().unwrap()
    }

    fn connect(&self) -> TcpStream {
        let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).unwrap();
        with_timeouts(tcp)
    }

    /// A connection whose source address is `source`.
    fn connect_from(&self, source: Ipv4Addr) -> TcpStream {
        with_timeouts(tcp_from(source, self.port))
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn request(&self, raw: &str) -> Response {
        send_request(self.connect(), raw)
    }

    fn request_from(&self, source: Ipv4Addr, raw: &str) -> Response {
        send_request(self.connect_from(source), raw)
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

    fn post(&self, path: &str, body: &str, origin: Option<&str>, cookie: Option<&str>) -> Response {
        self.request(&post_request(self.port, path, body, origin, cookie))
    }

    fn post_from(&self, source: Ipv4Addr, path: &str, body: &str) -> Response {
        self.request_from(source, &post_request(self.port, path, body, None, None))
    }

    /// POST /login/start from `source`.
    fn challenge_from(&self, source: Ipv4Addr) -> Challenge {
        Challenge::of(&self.post_from(source, "/login/start", ""))
    }

    fn challenge(&self) -> Challenge {
        self.challenge_from(Ipv4Addr::LOCALHOST)
    }

    /// The whole login from `source`: the answer to /login/finish.
    fn login_from(&self, source: Ipv4Addr, password: &str) -> Response {
        let start = self.post_from(source, "/login/start", "");
        if start.status != 200 {
            return start;
        }
        let answer = answer(password, &Challenge::of(&start), &client_nonce());
        self.post_from(source, "/login/finish", &answer.body)
    }

    fn login_with(&self, password: &str) -> Response {
        self.login_from(Ipv4Addr::LOCALHOST, password)
    }

    /// Logs in with the right password and returns the `Cookie` header value.
    fn login(&self) -> String {
        let response = self.login_with(PASSWORD);
        assert_eq!(response.status, 200, "{response:?}");
        session_cookie(&response)
    }

    fn open_viewer(
        &self,
        cookie: Option<&str>,
        origin: Option<&str>,
        user_agent: &str,
    ) -> Result<Socket, u16> {
        let mut request = format!("ws://127.0.0.1:{}/ws", self.port)
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

fn with_timeouts(tcp: TcpStream) -> TcpStream {
    tcp.set_read_timeout(Some(WAIT)).unwrap();
    tcp.set_write_timeout(Some(WAIT)).unwrap();
    tcp
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

fn send_request(mut stream: TcpStream, raw: &str) -> Response {
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

/// Whether a request on `stream` gets any HTTP answer at all.
fn answered(mut stream: TcpStream, raw: &str) -> bool {
    if stream
        .write_all(raw.as_bytes())
        .and_then(|()| stream.flush())
        .is_err()
    {
        return false;
    }
    read_until_closed(&mut stream).starts_with(b"HTTP/")
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
            Err(error) if error.kind() == ErrorKind::ConnectionReset && !raw.is_empty() => break,
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

fn wrong_password(tries_left: u32) -> Value {
    json!({"error": "Wrong password.", "tries_left": tries_left})
}

// ---- password verifier ----

#[test]
fn build_verifier_counts_characters_with_the_exact_messages() {
    assert_eq!(
        build_verifier("1234567").err().as_deref(),
        Some("At least 8 characters.")
    );
    // Characters, not bytes: seven two-byte letters are still too short.
    assert_eq!(
        build_verifier("ñññññññ").err().as_deref(),
        Some("At least 8 characters.")
    );
    assert_eq!(
        build_verifier(&"a".repeat(129)).err().as_deref(),
        Some("At most 128 characters.")
    );
    assert!(build_verifier(&"ñ".repeat(128)).is_ok());
}

#[test]
fn build_verifier_salts_every_verifier_and_uses_200000_iterations() {
    let first = build_verifier("12345678").unwrap();
    let second = build_verifier("12345678").unwrap();
    assert_eq!(ITERATIONS, 200_000);
    assert_eq!(first.iterations(), ITERATIONS);
    assert_eq!(first.salt().len(), 16);
    assert_ne!(
        first.salt(),
        second.salt(),
        "every verifier has its own salt"
    );
    assert_ne!(first, second);
    assert!(!format!("{first:?}").contains("12345678"));
}

// ---- login ----

#[test]
fn the_login_starts_with_a_single_use_challenge_from_the_password_verifier() {
    let harness = Harness::start();
    let verifier = verifier_of(PASSWORD);
    let response = harness.post("/login/start", "", None, None);
    assert_eq!(response.status, 200);
    assert_eq!(response.header("Content-Type"), Some("application/json"));
    assert_eq!(response.header("Set-Cookie"), None);
    let body = response.json();
    assert_eq!(
        body.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["iterations", "nonce", "salt"]
    );
    let challenge = Challenge::of(&response);
    assert_eq!(challenge.salt, verifier.salt());
    assert_eq!(challenge.iterations, ITERATIONS);
    assert_eq!(challenge.nonce.len(), 64, "32 random bytes as hex");
    assert!(challenge
        .nonce
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert_ne!(harness.challenge().nonce, challenge.nonce);
    assert_eq!(harness.get("/login/start", None).status, 405);
}

#[test]
fn the_right_proof_sets_a_private_session_cookie_and_proves_the_server() {
    let harness = Harness::start();
    let challenge = harness.challenge();
    let answer = answer(PASSWORD, &challenge, &client_nonce());
    let response = harness.post("/login/finish", &answer.body, None, None);
    assert_eq!(response.status, 200, "{response:?}");
    assert_eq!(
        response.json(),
        json!({"server_signature": answer.server_signature}),
        "the server proves it knows the verifier"
    );
    let set_cookie = response.header("Set-Cookie").unwrap();
    let (cookie, attributes) = set_cookie.split_once(';').unwrap();
    let token = cookie.strip_prefix("sdeck_session=").unwrap();
    assert_eq!(token.len(), 64, "32 random bytes as hex");
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let attributes: Vec<&str> = attributes.split(';').map(str::trim).collect();
    assert_eq!(
        attributes,
        ["HttpOnly", "SameSite=Strict", "Path=/"],
        "no Secure: the deck is plain HTTP"
    );
    let deck_page = fs::read("src/virtual_deck/page/index.html").unwrap();
    assert_eq!(
        harness.get("/", Some(&session_cookie(&response))).body,
        deck_page
    );
}

#[test]
fn the_password_never_travels_in_any_login_request() {
    let harness = Harness::start();
    let challenge = harness.challenge();
    let start = post_request(harness.port, "/login/start", "", None, None);
    let answer = answer(PASSWORD, &challenge, &client_nonce());
    let finish = post_request(harness.port, "/login/finish", &answer.body, None, None);
    let finished = harness.request(&finish);
    assert_eq!(finished.status, 200);
    let hex: String = PASSWORD.bytes().map(|byte| format!("{byte:02x}")).collect();
    let forms = [
        PASSWORD.to_string(),
        PASSWORD.replace(' ', "+"),
        PASSWORD.replace(' ', "%20"),
        STANDARD.encode(PASSWORD),
        hex,
        "correct".to_string(),
        "password".to_string(),
    ];
    for request in [&start, &finish] {
        for form in &forms {
            assert!(!request.contains(form.as_str()), "{form:?} in {request}");
        }
    }
    // The old form login is gone: a password posted to /login logs nobody in.
    let old = harness.request(&format!(
        "POST /login HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 21\r\n\r\n\
         password=correct+horse",
        harness.port
    ));
    assert_eq!(old.status, 405);
    assert_eq!(old.header("Allow"), Some("GET"));
    assert_eq!(old.header("Set-Cookie"), None);
    assert_eq!(harness.get("/login", None).status, 303);
}

#[test]
fn a_wrong_proof_answers_401_with_the_tries_left() {
    let harness = Harness::start();
    let response = harness.login_with("wrong password");
    assert_eq!(response.status, 401);
    assert_eq!(response.json(), wrong_password(4));
    assert_eq!(response.header("Set-Cookie"), None);
}

#[test]
fn the_sixth_try_after_five_failures_answers_429_even_with_the_right_password() {
    let harness = Harness::start();
    // A challenge taken before the lockout is no way around it.
    let early = harness.challenge();
    for tries_left in (0..5).rev() {
        let response = harness.login_with("wrong password");
        assert_eq!(response.status, 401);
        assert_eq!(response.json(), wrong_password(tries_left));
    }
    let too_many = json!({"error": "Too many tries.", "retry_after": 30});
    let start = harness.post("/login/start", "", None, None);
    assert_eq!(start.status, 429);
    assert_eq!(start.json(), too_many);
    assert_eq!(start.header("Retry-After"), Some("30"));
    let right = answer(PASSWORD, &early, &client_nonce());
    let finish = harness.post("/login/finish", &right.body, None, None);
    assert_eq!(finish.status, 429);
    assert_eq!(finish.json(), too_many);
    assert_eq!(finish.header("Retry-After"), Some("30"));
    assert_eq!(finish.header("Set-Cookie"), None);
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
    assert_eq!(other.json(), wrong_password(4));
    let other = harness.login_from(OTHER_ADDRESS, PASSWORD);
    assert_eq!(other.status, 200, "{other:?}");
    assert!(other.header("Set-Cookie").is_some());

    let first = harness.login_with(PASSWORD);
    assert_eq!(first.status, 429, "the first address is still locked");
}

#[test]
fn a_replayed_answer_is_rejected_and_never_counts_as_a_try() {
    let harness = Harness::start();
    let challenge = harness.challenge();
    let right = answer(PASSWORD, &challenge, &client_nonce());
    assert_eq!(
        harness
            .post("/login/finish", &right.body, None, None)
            .status,
        200
    );
    for _ in 0..2 {
        let replayed = harness.post("/login/finish", &right.body, None, None);
        assert_eq!(replayed.status, 400);
        assert_eq!(replayed.json(), json!({"error": EXPIRED}));
        assert_eq!(replayed.header("Set-Cookie"), None);
    }
    // A wrong answer uses its challenge up too.
    let challenge = harness.challenge();
    let wrong = answer("wrong password", &challenge, &client_nonce());
    assert_eq!(
        harness
            .post("/login/finish", &wrong.body, None, None)
            .status,
        401
    );
    let again = harness.post("/login/finish", &wrong.body, None, None);
    assert_eq!(again.status, 400);
    assert_eq!(again.json(), json!({"error": EXPIRED}));
    // An unknown nonce is no challenge either.
    let unknown = Challenge {
        nonce: "ab".repeat(32),
        ..challenge
    };
    let made_up = answer(PASSWORD, &unknown, &client_nonce());
    assert_eq!(
        harness
            .post("/login/finish", &made_up.body, None, None)
            .status,
        400
    );
    assert_eq!(
        harness.login_with("wrong password").json(),
        wrong_password(3),
        "only the one wrong proof counted"
    );
}

#[test]
fn a_nonce_from_another_address_is_rejected_and_stays_usable_by_its_own() {
    let harness = Harness::start();
    let challenge = harness.challenge();
    let right = answer(PASSWORD, &challenge, &client_nonce());
    let elsewhere = harness.post_from(OTHER_ADDRESS, "/login/finish", &right.body);
    assert_eq!(elsewhere.status, 400);
    assert_eq!(elsewhere.json(), json!({"error": EXPIRED}));
    assert_eq!(elsewhere.header("Set-Cookie"), None);
    let own = harness.post("/login/finish", &right.body, None, None);
    assert_eq!(own.status, 200, "{own:?}");
    assert_eq!(
        harness.login_from(OTHER_ADDRESS, "wrong password").json(),
        wrong_password(4),
        "the other address's attempt counted as no try"
    );
}

#[test]
fn a_malformed_answer_is_a_bad_request_and_uses_up_nothing() {
    let harness = Harness::start();
    let challenge = harness.challenge();
    let right = answer(PASSWORD, &challenge, &client_nonce());
    let fields: Value = serde_json::from_str(&right.body).unwrap();
    let with = |key: &str, value: Value| {
        let mut body = fields.clone();
        body[key] = value;
        body.to_string()
    };
    for body in [
        "not json".to_string(),
        json!({"nonce": challenge.nonce, "client_nonce": "abcdefghijklmnop"}).to_string(),
        with("proof", json!(STANDARD.encode([1u8; 31]))),
        with("proof", json!("not base64!")),
        with("client_nonce", json!("short")),
        with("client_nonce", json!("a,b".repeat(10))),
        with("password", json!(PASSWORD)),
    ] {
        let response = harness.post("/login/finish", &body, None, None);
        assert_eq!(response.status, 400, "{body}");
        assert_eq!(response.json(), json!({"error": "Bad request."}), "{body}");
    }
    assert_eq!(
        harness
            .post("/login/finish", &right.body, None, None)
            .status,
        200,
        "the challenge is still there"
    );
    assert_eq!(
        harness.login_with("wrong password").json(),
        wrong_password(4)
    );
}

#[test]
fn a_foreign_or_null_origin_login_is_forbidden_and_never_counts_as_a_try() {
    let harness = Harness::start();
    let https = format!("https://127.0.0.1:{}", harness.port);
    for origin in ["https://evil.example", "null", https.as_str()] {
        let start = harness.post("/login/start", "", Some(origin), None);
        assert_eq!(start.status, 403, "{origin}");
        assert_eq!(start.json(), json!({"error": "Forbidden."}));
        for password in ["wrong password", PASSWORD] {
            let answer = answer(password, &harness.challenge(), &client_nonce());
            let finish = harness.post("/login/finish", &answer.body, Some(origin), None);
            assert_eq!(finish.status, 403, "{origin}");
            assert_eq!(finish.header("Set-Cookie"), None, "{origin}");
        }
    }
    // The page's own origin logs in.
    let challenge = Challenge::of(&harness.post("/login/start", "", Some(&harness.origin()), None));
    let right = answer(PASSWORD, &challenge, &client_nonce());
    let response = harness.post("/login/finish", &right.body, Some(&harness.origin()), None);
    assert_eq!(response.status, 200);
    assert_eq!(
        harness.login_with("wrong password").json(),
        wrong_password(4),
        "the forbidden tries were not counted"
    );
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
    let response = harness.post("/logout", "", Some(&harness.origin()), Some(&cookie));
    assert_eq!(response.status, 303);
    assert_eq!(response.header("Location"), Some("/"));
    assert_eq!(
        response.header("Set-Cookie"),
        Some("sdeck_session=; Max-Age=0; HttpOnly; SameSite=Strict; Path=/")
    );
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
    let https = format!("https://127.0.0.1:{}", harness.port);
    for origin in ["null", "https://evil.example", https.as_str()] {
        let response = harness.post("/logout", "", Some(origin), Some(&cookie));
        assert_eq!(response.status, 403, "{origin}");
        assert_eq!(response.json(), json!({"error": "Forbidden."}), "{origin}");
        assert_eq!(response.header("Set-Cookie"), None, "{origin}");
        assert_eq!(
            harness.get("/", Some(&cookie)).body,
            deck_page,
            "{origin}: the session is still live"
        );
    }
    let response = harness.post("/logout", "", Some(&harness.origin()), Some(&cookie));
    assert_eq!(response.status, 303);
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
fn an_oversized_login_answer_is_refused_without_counting_a_try() {
    let harness = Harness::start();
    let response = harness.request(&format!(
        "POST /login/finish HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 5000\r\n\r\n",
        harness.port
    ));
    assert_eq!(response.status, 413);
    assert_eq!(
        harness.login_with("wrong password").json(),
        wrong_password(4)
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
    let https = format!("https://127.0.0.1:{}", harness.port);
    for origin in [
        Some("http://evil.example"),
        Some("https://evil.example"),
        Some(https.as_str()),
        Some("http://127.0.0.1"),
        Some("null"),
        None,
    ] {
        let refused = harness.open_viewer(Some(&cookie), origin, ANDROID_CHROME);
        assert_eq!(refused.err(), Some(403), "{origin:?}");
    }
    assert!(harness.server().viewers().is_empty());
    assert!(harness.inputs.try_recv().is_err(), "no DeckInput emitted");
    assert!(harness
        .open_viewer(Some(&cookie), Some(&harness.origin()), ANDROID_CHROME)
        .is_ok());
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
fn one_address_cannot_hold_more_than_eight_unfinished_connections() {
    let harness = Harness::start();
    let request = format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n", harness.port);
    // Connections that never send their request.
    let stalled: Vec<TcpStream> = (0..PENDING_PER_ADDRESS)
        .map(|_| harness.connect())
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
        assert!(answered(tcp, &request));
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

    harness.server().set_password(verifier_of(NEW_PASSWORD));
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
    assert_eq!(
        harness.challenge().salt,
        verifier_of(NEW_PASSWORD).salt(),
        "challenges carry the new salt"
    );
    assert_eq!(harness.login_with(PASSWORD).status, 401);
    assert_eq!(harness.login_with(NEW_PASSWORD).status, 200);
}

#[test]
fn no_session_or_challenge_from_the_old_password_survives_a_password_change() {
    let harness = Harness::start();
    let login_page = fs::read("src/virtual_deck/page/login.html").unwrap();
    let before = harness.login();
    // A login with the old password, in flight while the password changes.
    let challenge = harness.challenge();
    harness.server().set_password(verifier_of(NEW_PASSWORD));
    let late = answer(PASSWORD, &challenge, &client_nonce());
    let response = harness.post("/login/finish", &late.body, None, None);
    assert_eq!(
        response.status, 400,
        "the challenge went with the old password"
    );
    assert_eq!(response.json(), json!({"error": EXPIRED}));
    assert_eq!(response.header("Set-Cookie"), None);
    assert_eq!(
        harness.get("/", Some(&before)).body,
        login_page,
        "the old session shows the login page"
    );
    assert_eq!(
        harness
            .open_viewer(Some(&before), Some(&harness.origin()), ANDROID_CHROME)
            .err(),
        Some(403)
    );
    assert_eq!(harness.login_with(NEW_PASSWORD).status, 200);
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

    // No WebSocket close: the phone just vanishes.
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
    // A client that never sends its request must not keep stop() waiting.
    let mut idle = harness.connect();
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

// ---- addresses ----

#[test]
fn url_candidates_name_the_bound_address_and_port_over_plain_http() {
    let harness = Harness::start();
    assert_eq!(
        harness.server().url_candidates(),
        vec![format!("http://127.0.0.1:{}/", harness.port)]
    );
}

#[test]
fn the_default_config_listens_on_every_interface_at_8765() {
    assert_eq!(
        ServerConfig::default(),
        ServerConfig {
            port: 8765,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        }
    );
}

#[test]
fn a_port_in_use_is_reported() {
    let harness = Harness::start();
    let (sender, _inputs) = mpsc::channel();
    let error = VirtualDeckServer::start(
        config(harness.port),
        verifier_of(PASSWORD),
        sender,
        Box::new(Vec::new),
    )
    .err()
    .expect("the port is taken");
    assert_eq!(error, format!("Port {} is already in use.", harness.port));
}
