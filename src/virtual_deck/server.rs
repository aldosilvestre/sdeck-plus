//! The running server: the accept thread, one thread per connection, the
//! viewer registry and a clean, joined stop.
use super::{
    access::{Access, SESSION_COOKIE},
    browser,
    http::{self, BodyError, HeadError, Origin, Reply, Request},
    password,
    socket::{self, Socket, Tls},
    viewer::{self, ByeReason, ViewerHandle},
    DeckFrame, DeckInput, ServerEvent, Snapshot, Viewer, INDEX_PAGE, KEYS, LOGIN_PAGE, MAX_VIEWERS,
};
use serde_json::json;
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender},
        Arc, Mutex, MutexGuard, PoisonError,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tungstenite::Utf8Bytes;

/// Open connections at once (viewers included); more are closed at accept.
const MAX_CONNECTIONS: usize = 32;
/// Open connections one address may have that are not viewers (yet); more
/// are closed at accept, so one address cannot fill every slot with
/// handshakes it never finishes. Its viewers do not count: one computer may
/// hold every viewer slot and still load the page.
const MAX_PENDING_PER_ADDRESS: usize = 8;
/// TLS handshake, request head and body must all arrive within this time.
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);
/// A client that takes longer than this to take our bytes is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Wait after a failed accept (for example, out of file descriptors).
const ACCEPT_BACKOFF: Duration = Duration::from_millis(250);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// State every thread of one server shares.
pub(super) struct Shared {
    tls: Arc<rustls::ServerConfig>,
    registry: Mutex<Registry>,
    /// Argon2id checks run one at a time: bounded memory and CPU, and a
    /// lockout cannot be raced by parallel logins from one address.
    verifying: Mutex<()>,
    /// How many viewers hold each key down.
    pressed: Mutex<[u16; KEYS]>,
    inputs: Sender<DeckInput>,
    snapshot: Snapshot,
    subscribers: Mutex<Vec<Sender<ServerEvent>>>,
    stopping: AtomicBool,
}

struct Registry {
    access: Access,
    viewers: Vec<Arc<ViewerHandle>>,
    next_id: u64,
    stopping: bool,
}

impl Shared {
    pub fn new(
        tls: Arc<rustls::ServerConfig>,
        password_hash: String,
        inputs: Sender<DeckInput>,
        snapshot: Snapshot,
    ) -> Self {
        Self {
            tls,
            registry: Mutex::new(Registry {
                access: Access::new(password_hash),
                viewers: Vec::new(),
                next_id: 0,
                stopping: false,
            }),
            verifying: Mutex::new(()),
            pressed: Mutex::new([0; KEYS]),
            inputs,
            snapshot,
            subscribers: Mutex::new(Vec::new()),
            stopping: AtomicBool::new(false),
        }
    }

    fn registry(&self) -> MutexGuard<'_, Registry> {
        lock(&self.registry)
    }

    pub fn emit(&self, event: ServerEvent) {
        lock(&self.subscribers).retain(|subscriber| subscriber.send(event.clone()).is_ok());
    }

    pub fn subscribe(&self) -> Receiver<ServerEvent> {
        let (sender, receiver) = mpsc::channel();
        lock(&self.subscribers).push(sender);
        receiver
    }

    pub fn snapshot(&self) -> Vec<DeckFrame> {
        (self.snapshot)()
    }

    /// One viewer's key went down or up. The deck sees a key go down when the
    /// first viewer presses it and up when the last one lets go.
    pub fn key(&self, held: &mut bool, index: usize, pressed: bool) {
        if *held == pressed || index >= KEYS {
            return;
        }
        *held = pressed;
        let mut counts = lock(&self.pressed);
        let count = &mut counts[index];
        let changed = if pressed {
            *count += 1;
            *count == 1
        } else {
            *count = count.saturating_sub(1);
            *count == 0
        };
        if changed {
            let key = u8::try_from(index + 1).unwrap_or(u8::MAX);
            let _ = self.inputs.send(DeckInput { key, pressed });
        }
    }

    pub fn publish(&self, frame: &DeckFrame) {
        let text = viewer::frame_text(frame);
        for handle in &self.registry().viewers {
            handle.offer(&text);
        }
    }

    pub fn viewers(&self) -> Vec<Viewer> {
        self.registry()
            .viewers
            .iter()
            .map(|handle| Viewer {
                id: handle.id,
                address: handle.address,
                browser: handle.browser.clone(),
                since: handle.since,
            })
            .collect()
    }

    /// Closes viewer `id` and every other viewer of its login session, and
    /// ends that session, so the page has to log in again.
    pub fn disconnect(&self, id: u64) {
        let removed = {
            let mut registry = self.registry();
            let session = registry
                .viewers
                .iter()
                .find(|handle| handle.id == id)
                .map(|handle| handle.session);
            match session {
                Some(session) => {
                    registry.access.close_session(&session);
                    close_viewers(&mut registry.viewers, ByeReason::Disconnected, |handle| {
                        handle.session == session
                    })
                }
                None => false,
            }
        };
        if removed {
            self.emit(ServerEvent::ViewersChanged);
        }
    }

    pub fn set_password(&self, hash: String) -> Result<(), String> {
        password::check_hash(&hash)?;
        let removed = {
            let _turn = lock(&self.verifying);
            let mut registry = self.registry();
            registry.access.set_password(hash);
            close_viewers(&mut registry.viewers, ByeReason::PasswordChanged, |_| true)
        };
        if removed {
            self.emit(ServerEvent::ViewersChanged);
        }
        Ok(())
    }

    /// Refuses new viewers and asks every viewer to say bye; returns whether
    /// any viewer was removed.
    fn begin_stop(&self) -> bool {
        self.stopping.store(true, Ordering::SeqCst);
        let mut registry = self.registry();
        registry.stopping = true;
        close_viewers(&mut registry.viewers, ByeReason::Stopped, |_| true)
    }

    fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    fn has_session(&self, request: &Request) -> bool {
        self.registry()
            .access
            .find_session(request.cookies(SESSION_COOKIE))
            .is_some()
    }

    fn locked(&self, address: IpAddr) -> Option<Reply> {
        self.registry()
            .access
            .locked(address, Instant::now())
            .map(too_many_tries)
    }

    fn log_in(&self, address: IpAddr, password: Option<&str>) -> Reply {
        let _turn = lock(&self.verifying);
        let (hash, generation) = {
            let mut registry = self.registry();
            if let Some(wait) = registry.access.locked(address, Instant::now()) {
                return too_many_tries(wait);
            }
            registry.access.password()
        };
        let matches = password.is_some_and(|password| password::verify(password, &hash));
        let mut registry = self.registry();
        if !matches || registry.access.generation() != generation {
            let tries_left = registry.access.fail(address, Instant::now());
            return Reply::json(
                401,
                &json!({"error": "Wrong password.", "tries_left": tries_left}),
            );
        }
        registry.access.succeed(address);
        match registry.access.open_session(Instant::now()) {
            Ok((token, _)) => Reply::home().with(
                "Set-Cookie",
                format!("{SESSION_COOKIE}={token}; HttpOnly; Secure; SameSite=Strict; Path=/"),
            ),
            Err(error) => Reply::error(500, &error),
        }
    }

    fn log_out(&self, request: &Request) {
        let removed = {
            let mut registry = self.registry();
            match registry
                .access
                .find_session(request.cookies(SESSION_COOKIE))
            {
                Some(session) => {
                    registry.access.close_session(&session);
                    close_viewers(&mut registry.viewers, ByeReason::Disconnected, |handle| {
                        handle.session == session
                    })
                }
                None => false,
            }
        };
        if removed {
            self.emit(ServerEvent::ViewersChanged);
        }
    }

    /// Registers a viewer when its session is live and there is room.
    fn admit_viewer(
        &self,
        request: &Request,
        address: IpAddr,
        outbox: SyncSender<Utf8Bytes>,
        upgraded: &AtomicBool,
    ) -> Result<Arc<ViewerHandle>, Reply> {
        let browser = browser::describe(request.header("user-agent").unwrap_or_default());
        let mut registry = self.registry();
        if registry.stopping {
            return Err(Reply::error(503, "The virtual deck is stopping."));
        }
        let Some(session) = registry
            .access
            .find_session(request.cookies(SESSION_COOKIE))
        else {
            return Err(forbidden());
        };
        if registry.viewers.len() >= MAX_VIEWERS {
            return Err(Reply::error(503, "Too many viewers."));
        }
        registry.next_id += 1;
        let handle = Arc::new(ViewerHandle::new(
            registry.next_id,
            address,
            browser,
            session,
            outbox,
        ));
        registry.viewers.push(handle.clone());
        // Under the registry lock: stop() sees this connection as a viewer
        // (one that says bye itself) exactly when it is in the registry.
        upgraded.store(true, Ordering::SeqCst);
        Ok(handle)
    }

    /// Removes viewer `id`; returns whether it was still registered.
    fn remove_viewer(&self, id: u64) -> bool {
        let mut registry = self.registry();
        let before = registry.viewers.len();
        registry.viewers.retain(|handle| handle.id != id);
        registry.viewers.len() != before
    }
}

fn close_viewers(
    viewers: &mut Vec<Arc<ViewerHandle>>,
    reason: ByeReason,
    selected: impl Fn(&ViewerHandle) -> bool,
) -> bool {
    let before = viewers.len();
    viewers.retain(|handle| {
        if selected(handle) {
            handle.close(reason);
            false
        } else {
            true
        }
    });
    viewers.len() != before
}

fn too_many_tries(wait: Duration) -> Reply {
    let seconds = wait.as_millis().div_ceil(1000).max(1);
    Reply::json(
        429,
        &json!({"error": "Too many tries.", "retry_after": seconds}),
    )
    .with("Retry-After", seconds.to_string())
}

fn forbidden() -> Reply {
    Reply::error(403, "Forbidden.")
}

struct Connection {
    peer: IpAddr,
    thread: JoinHandle<()>,
    /// A second handle on the socket, to cut a request short on stop.
    tcp: TcpStream,
    upgraded: Arc<AtomicBool>,
}

/// The listener and its threads.
pub(super) struct Running {
    pub shared: Arc<Shared>,
    pub port: u16,
    acceptor: Option<JoinHandle<()>>,
    connections: Arc<Mutex<Vec<Connection>>>,
    stop_signal: Option<Sender<()>>,
    listener: TcpListener,
    wake: SocketAddr,
}

impl Running {
    pub fn start(bind: IpAddr, port: u16, shared: Arc<Shared>) -> Result<Self, String> {
        let listener =
            TcpListener::bind((bind, port)).map_err(|error| bind_error(&error, bind, port))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("Could not listen on port {port}: {error}"))?
            .port();
        let handle = listener
            .try_clone()
            .map_err(|error| format!("Could not listen on port {port}: {error}"))?;
        let wake_address = match bind {
            IpAddr::V4(address) if address.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(address) if address.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
            address => address,
        };
        let connections = Arc::new(Mutex::new(Vec::new()));
        let (stop_signal, stop) = mpsc::channel();
        let acceptor = thread::Builder::new()
            .name("virtual-deck-accept".into())
            .spawn({
                let shared = shared.clone();
                let connections = connections.clone();
                move || accept_loop(&listener, &shared, &connections, &stop)
            })
            .map_err(|error| format!("Could not start the virtual deck: {error}"))?;
        Ok(Self {
            shared,
            port,
            acceptor: Some(acceptor),
            connections,
            stop_signal: Some(stop_signal),
            listener: handle,
            wake: SocketAddr::new(wake_address, port),
        })
    }

    /// Says bye to every viewer, closes the listener and every connection
    /// and joins every thread. Safe to call twice.
    pub fn stop(&mut self) {
        let Some(acceptor) = self.acceptor.take() else {
            return;
        };
        let viewers_closed = self.shared.begin_stop();
        drop(self.stop_signal.take());
        wake_acceptor(&self.listener, self.wake);
        let _ = acceptor.join();
        let connections = std::mem::take(&mut *lock(&self.connections));
        for connection in &connections {
            // Viewers say bye and close by themselves; a plain request is cut.
            if !connection.upgraded.load(Ordering::SeqCst) {
                let _ = connection.tcp.shutdown(Shutdown::Both);
            }
        }
        for connection in connections {
            let _ = connection.thread.join();
        }
        if viewers_closed {
            self.shared.emit(ServerEvent::ViewersChanged);
        }
    }
}

fn bind_error(error: &io::Error, bind: IpAddr, port: u16) -> String {
    match error.kind() {
        io::ErrorKind::AddrInUse => format!("Port {port} is already in use."),
        io::ErrorKind::AddrNotAvailable => {
            format!("The address {bind} does not belong to this computer.")
        }
        io::ErrorKind::PermissionDenied => format!("Port {port} needs administrator rights."),
        _ => format!("Could not listen on port {port}: {error}"),
    }
}

/// Makes a blocked `accept` return.
#[cfg(target_os = "linux")]
fn wake_acceptor(listener: &TcpListener, _wake: SocketAddr) {
    use std::os::fd::AsRawFd;
    // SAFETY: the descriptor belongs to `listener`, which stays open for the
    // whole call; shutting a listening socket down makes Linux return EINVAL
    // from the accept blocked on it. No memory is involved.
    unsafe {
        libc::shutdown(listener.as_raw_fd(), libc::SHUT_RDWR);
    }
}

/// Makes a blocked `accept` return.
#[cfg(not(target_os = "linux"))]
fn wake_acceptor(_listener: &TcpListener, wake: SocketAddr) {
    let _ = TcpStream::connect_timeout(&wake, Duration::from_secs(1));
}

fn accept_loop(
    listener: &TcpListener,
    shared: &Arc<Shared>,
    connections: &Mutex<Vec<Connection>>,
    stop: &Receiver<()>,
) {
    loop {
        let accepted = listener.accept();
        if shared.is_stopping() {
            return;
        }
        match accepted {
            Ok((tcp, peer)) => admit(tcp, peer.ip().to_canonical(), shared, connections),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted
                        | io::ErrorKind::ConnectionAborted
                        | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => {
                shared.emit(ServerEvent::Error(format!(
                    "The virtual deck could not accept a connection: {error}"
                )));
                if let Err(RecvTimeoutError::Disconnected) = stop.recv_timeout(ACCEPT_BACKOFF) {
                    return;
                }
            }
        }
    }
}

fn admit(tcp: TcpStream, peer: IpAddr, shared: &Arc<Shared>, connections: &Mutex<Vec<Connection>>) {
    let mut open = lock(connections);
    let mut index = 0;
    while index < open.len() {
        if open[index].thread.is_finished() {
            let _ = open.swap_remove(index).thread.join();
        } else {
            index += 1;
        }
    }
    let pending_from_peer = open
        .iter()
        .filter(|connection| connection.peer == peer && !connection.upgraded.load(Ordering::SeqCst))
        .count();
    if open.len() >= MAX_CONNECTIONS || pending_from_peer >= MAX_PENDING_PER_ADDRESS {
        return;
    }
    let Ok(watch) = tcp.try_clone() else {
        return;
    };
    let upgraded = Arc::new(AtomicBool::new(false));
    let spawned = thread::Builder::new()
        .name("virtual-deck-conn".into())
        .spawn({
            let shared = shared.clone();
            let upgraded = upgraded.clone();
            move || serve_connection(tcp, peer, &shared, &upgraded)
        });
    match spawned {
        Ok(thread) => open.push(Connection {
            peer,
            thread,
            tcp: watch,
            upgraded,
        }),
        Err(error) => shared.emit(ServerEvent::Error(format!(
            "The virtual deck could not serve a connection: {error}"
        ))),
    }
}

fn serve_connection(tcp: TcpStream, peer: IpAddr, shared: &Shared, upgraded: &AtomicBool) {
    let Ok(socket) = Socket::new(tcp, Instant::now() + REQUEST_DEADLINE, WRITE_TIMEOUT) else {
        return;
    };
    let Ok(connection) = rustls::ServerConnection::new(shared.tls.clone()) else {
        return;
    };
    let mut stream = rustls::StreamOwned::new(connection, socket);
    let (request, leftover) = match http::read_head(&mut stream) {
        Ok(parts) => parts,
        Err(HeadError::Gone) => return,
        Err(HeadError::TooLarge) => {
            return reply(
                stream,
                Reply::error(431, "Request header is too large."),
                None,
            )
        }
        Err(HeadError::Malformed) => return reply(stream, Reply::error(400, "Bad request."), None),
    };
    let answer = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/ws") => return open_viewer(stream, &request, leftover, peer, shared, upgraded),
        ("GET", "/") if shared.has_session(&request) => Reply::html(INDEX_PAGE),
        ("GET", "/") => Reply::html(LOGIN_PAGE),
        ("GET", "/login") => Reply::home(),
        ("POST", "/login") => match log_in(&mut stream, &request, leftover, peer, shared) {
            Some(answer) => answer,
            None => return,
        },
        ("POST", "/logout") => log_out(&request, shared),
        (_, "/" | "/ws") => Reply::error(405, "Method not allowed.").with("Allow", "GET".into()),
        (_, "/logout") => Reply::error(405, "Method not allowed.").with("Allow", "POST".into()),
        (_, "/login") => Reply::error(405, "Method not allowed.").with("Allow", "GET, POST".into()),
        _ => Reply::error(404, "Not found."),
    };
    reply(stream, answer, request.header("host"));
}

/// Sends `answer` and closes; `host` is the request's `Host`, if any.
fn reply(mut stream: Tls, answer: Reply, host: Option<&str>) {
    if answer.write_to(&mut stream, host).is_ok() {
        socket::finish(stream);
    }
}

/// POST /login; None when the client went away mid-body. A foreign origin is
/// refused before anything else, so it never counts as a try; `null` is
/// allowed, because that is what the login form's own post sends.
fn log_in(
    stream: &mut Tls,
    request: &Request,
    leftover: Vec<u8>,
    peer: IpAddr,
    shared: &Shared,
) -> Option<Reply> {
    if request.origin() == Origin::Foreign {
        return Some(forbidden());
    }
    if let Some(answer) = shared.locked(peer) {
        return Some(answer);
    }
    let body = match http::read_body(stream, request, leftover) {
        Ok(body) => body,
        Err(BodyError::Gone) => return None,
        Err(BodyError::TooLarge) => return Some(Reply::error(413, "Request body is too large.")),
        Err(BodyError::LengthRequired) => return Some(Reply::error(411, "Length required.")),
    };
    let password = http::password_field(request.header("content-type"), &body);
    Some(shared.log_in(peer, password.as_deref()))
}

/// POST /logout: ends the session (and its viewers) and clears the cookie.
/// The page logs out with fetch(), which sends its own origin, so `null`
/// can only come from another page.
fn log_out(request: &Request, shared: &Shared) -> Reply {
    if matches!(request.origin(), Origin::Foreign | Origin::Null) {
        return forbidden();
    }
    shared.log_out(request);
    Reply::home().with(
        "Set-Cookie",
        format!("{SESSION_COOKIE}=; Max-Age=0; HttpOnly; Secure; SameSite=Strict; Path=/"),
    )
}

/// GET /ws: needs a live session cookie and `Origin: https://<Host>`.
fn open_viewer(
    mut stream: Tls,
    request: &Request,
    leftover: Vec<u8>,
    peer: IpAddr,
    shared: &Shared,
    upgraded: &AtomicBool,
) {
    if request.origin() != Origin::Same {
        return reply(stream, forbidden(), request.header("host"));
    }
    if !request.is_websocket_upgrade() {
        return reply(
            stream,
            Reply::error(400, "Bad request."),
            request.header("host"),
        );
    }
    let key = request
        .header("sec-websocket-key")
        .unwrap_or_default()
        .to_string();
    let (sender, outbox) = mpsc::sync_channel(viewer::OUTBOX);
    let handle = match shared.admit_viewer(request, peer, sender, upgraded) {
        Ok(handle) => handle,
        Err(answer) => return reply(stream, answer, request.header("host")),
    };
    let switched = http::write_switching_protocols(&mut stream, &key)
        .and_then(|()| stream.sock.poll_every(viewer::POLL));
    if switched.is_err() {
        shared.remove_viewer(handle.id);
        return;
    }
    shared.emit(ServerEvent::ViewersChanged);
    let stream = viewer::run(viewer::upgrade(stream, leftover), &handle, &outbox, shared);
    if shared.remove_viewer(handle.id) {
        shared.emit(ServerEvent::ViewersChanged);
    }
    socket::finish(stream);
}
