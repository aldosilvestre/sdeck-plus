//! The header menu's virtual deck (design/virtual-classic/04-all-in-menu.html)
//! in the real window over the real HID hub: off at start, the first-time
//! password form, on and off, the viewers with Disconnect, a new password, a
//! new port, the Current selector and the header lines.
//!
//! L40: the password lives in memory for one session. The first turn-on of
//! each window asks for it, turning off and on again reuses it, Change…
//! replaces it, and a restart forgets it; the settings file keeps the port
//! only, and the virtual Classic's own data stays in its root.
//!
//! L45: asked for the password, the switch stays on with the form below it;
//! nothing runs until Save and turn on, and Cancel (or the switch) turns it
//! back off.
//!
//! The server is real, plain HTTP, and listens on 127.0.0.1 only (the test
//! binary's `LAN`); the config folder is scratch; the test binary records the
//! browser address instead of starting a browser; viewers log in with the
//! page's challenge-response (computed here with `ring`) and watch through a
//! tungstenite client.
use super::model_session_tests::{header_label, scratch, stored_binding, wait_until};
use super::two_device_tests::{active, start_pair_with, wait_layer, Pair, PLUS_PATH};
use super::*;
use crate::virtual_deck_ui::{self, Phase, VirtualDeckPanel};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use ring::{digest, hmac, pbkdf2};
use sdeck_plus::{
    device_model::{PLUS, VIRTUAL_CLASSIC},
    event_api::test_support::SimDevice,
    local_artwork::KEYPAD_BACKGROUND_FILE,
    virtual_deck::settings::{SettingsStore, FILE_NAME},
};
use std::{
    collections::HashMap,
    io::{ErrorKind, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    num::NonZeroU32,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
};
use tungstenite::{client::IntoClientRequest, Message};

const PASSWORD: &str = "correct horse";
const NEW_PASSWORD: &str = "another secret";
/// The first-time form's warning (L40), word for word.
const SESSION_MESSAGE: &str =
    "Set a password for this session. It is forgotten when sdeck-plus closes.";
const ANDROID_CHROME: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/131.0.6778.135 Mobile Safari/537.36";
const MAC_SAFARI: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15";
/// Upper bound of every wait on a real signal; never a fixed sleep.
const WAIT: Duration = Duration::from_secs(20);

/// Ports nothing listens on right now, all different.
pub(super) fn free_ports<const N: usize>() -> [u16; N] {
    let listeners: Vec<TcpListener> = (0..N)
        .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap())
        .collect();
    std::array::from_fn(|index| listeners[index].local_addr().unwrap().port())
}

fn listening(port: u16) -> bool {
    TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok()
}

fn config_dir(roots: &feature_runtime::SessionRoots) -> PathBuf {
    roots.preference.parent().unwrap().to_path_buf()
}

/// Remembers `port`, as an earlier run would have. Only the port: the
/// password is never remembered.
pub(super) fn remember(port: u16) -> impl FnOnce(&feature_runtime::SessionRoots) {
    move |roots| {
        SettingsStore::open(&config_dir(roots))
            .set_port(port)
            .unwrap();
    }
}

/// Types `password` twice in the first-time form and presses Save and turn on.
fn save_and_turn_on(panel: &VirtualDeckPanel, password: &str) {
    let form = &panel.test_widgets().first_form;
    form.new.set_text(password);
    form.repeat.set_text(password);
    form.submit.emit_clicked();
}

/// The settings file holds the port and nothing about a password, owner-only.
fn assert_port_only(settings: &Path, port: u16) {
    let text = std::fs::read_to_string(settings).unwrap();
    let stored: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(stored, serde_json::json!({ "port": port }), "{text}");
    for secret in [PASSWORD, NEW_PASSWORD] {
        assert!(
            !text.contains(secret) && !text.contains(&STANDARD.encode(secret)),
            "{text}"
        );
    }
    assert_eq!(mode(settings), 0o600);
}

fn panel(pair: &Pair) -> VirtualDeckPanel {
    pair.registry
        .menu()
        .virtual_deck()
        .expect("the window's menu has the virtual deck")
}

fn wait_phase(panel: &VirtualDeckPanel, phase: Phase) {
    let watched = panel.clone();
    wait_until(&format!("the virtual deck to be {phase:?}"), move || {
        watched.test_phase() == phase
    });
}

/// Turns the deck on, typing [`PASSWORD`] when the menu asks for this
/// session's password (the switch stays on meanwhile), and waits until the
/// hub serves it.
pub(super) fn turn_on_and_wait(pair: &Pair) -> VirtualDeckPanel {
    let panel = panel(pair);
    panel.test_widgets().switch.set_active(true);
    if panel.test_phase() == Phase::NeedsPassword {
        assert!(panel.test_widgets().switch.is_active());
        assert!(panel.test_widgets().first_time.get_visible());
        save_and_turn_on(&panel, PASSWORD);
    }
    wait_phase(&panel, Phase::On);
    let registry = pair.registry.clone();
    wait_until("the hub to serve the virtual Classic", move || {
        registry.live_devices(&VIRTUAL_CLASSIC) == 1
    });
    panel
}

/// The header's "Virtual deck on · N viewers" label.
fn badge(pair: &Pair) -> Label {
    fn find(widget: &gtk::Widget) -> Option<Label> {
        if widget.has_css_class("status-virtual") {
            return widget.downcast_ref::<Label>().cloned();
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find(&current) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
    find(pair.window.upcast_ref()).expect("the header has the virtual deck badge")
}

/// Nothing of the virtual deck runs: no server on `port`, no device on the
/// hub, no header badge or address, no running details.
fn assert_nothing_runs(pair: &Pair, panel: &VirtualDeckPanel, port: u16, why: &str) {
    assert!(!listening(port), "{why}: no server");
    assert_eq!(
        pair.registry.live_devices(&VIRTUAL_CLASSIC),
        0,
        "{why}: no device"
    );
    assert!(!badge(pair).is_visible(), "{why}: no header badge");
    assert_eq!(virtual_deck_ui::served_at(), None, "{why}: no address");
    assert!(!panel.test_widgets().running.get_visible(), "{why}");
}

/// Runs what is ready on the main loop, a bounded number of times, so an
/// outcome a finished worker sent is handled.
fn handle_pending_events() {
    let context = glib::MainContext::default();
    for _ in 0..1000 {
        if !context.iteration(false) {
            break;
        }
    }
}

/// The texts of every label below `widget`, in order.
fn label_texts(widget: &gtk::Widget) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(label) = widget.downcast_ref::<Label>() {
        found.push(label.text().to_string());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        found.extend(label_texts(&current));
        child = current.next_sibling();
    }
    found
}

fn buttons_in(widget: &gtk::Widget) -> Vec<Button> {
    let mut found = Vec::new();
    if let Some(button) = widget.downcast_ref::<Button>() {
        found.push(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        found.extend(buttons_in(&current));
        child = current.next_sibling();
    }
    found
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// A browser page of the deck: logs in, opens the WebSocket and keeps reading
/// on a thread of its own; key presses go out through it.
struct ViewerClient {
    messages: mpsc::Receiver<serde_json::Value>,
    keys: mpsc::Sender<(u8, bool)>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

fn connect(port: u16) -> TcpStream {
    let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    tcp.set_read_timeout(Some(WAIT)).unwrap();
    tcp.set_write_timeout(Some(WAIT)).unwrap();
    tcp
}

/// A POST with a JSON body: the status, the headers and the body.
fn post(port: u16, path: &str, body: &str) -> (u16, String, serde_json::Value) {
    let mut stream = connect(port);
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => raw.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == ErrorKind::ConnectionReset && !raw.is_empty() => break,
            Err(error) => panic!("reading the answer failed: {error}"),
        }
    }
    let raw = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = raw.split_once("\r\n\r\n").expect("a whole answer");
    let status: u16 = head.split(' ').nth(1).unwrap().parse().unwrap();
    (
        status,
        head.to_string(),
        serde_json::from_str(body).unwrap_or_default(),
    )
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data);
    tag.as_ref().try_into().unwrap()
}

/// SaltedPasswords by password and salt.
type SaltedCache = Mutex<HashMap<(String, Vec<u8>), [u8; 32]>>;

/// SaltedPassword, derived once per password and salt for the whole binary.
fn salted_password(password: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    static SALTED: OnceLock<SaltedCache> = OnceLock::new();
    let cache = SALTED.get_or_init(Mutex::default);
    let key = (password.to_string(), salt.to_vec());
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

/// The page's login: POST /login/start, the proof computed here, POST
/// /login/finish. The session cookie, or the refusing status.
fn login(port: u16, password: &str) -> Result<String, u16> {
    let (status, _, challenge) = post(port, "/login/start", "");
    if status != 200 {
        return Err(status);
    }
    let salt = STANDARD
        .decode(challenge["salt"].as_str().unwrap())
        .unwrap();
    let iterations = u32::try_from(challenge["iterations"].as_u64().unwrap()).unwrap();
    let nonce = challenge["nonce"].as_str().unwrap();
    let salted = salted_password(password, &salt, iterations);
    let client_key = hmac_sha256(&salted, b"Client Key");
    let stored_key = digest::digest(&digest::SHA256, &client_key);
    let client_nonce = "0123456789abcdef0123456789abcdef";
    let auth_message = format!("{client_nonce}{nonce}");
    let signature = hmac_sha256(stored_key.as_ref(), auth_message.as_bytes());
    let mut proof = client_key;
    for (byte, signature) in proof.iter_mut().zip(signature) {
        *byte ^= signature;
    }
    let body = serde_json::json!({
        "nonce": nonce,
        "client_nonce": client_nonce,
        "proof": STANDARD.encode(proof),
    });
    let (status, head, answer) = post(port, "/login/finish", &body.to_string());
    if status != 200 {
        return Err(status);
    }
    let server_key = hmac_sha256(&salted, b"Server Key");
    assert_eq!(
        answer["server_signature"],
        STANDARD.encode(hmac_sha256(&server_key, auth_message.as_bytes())),
        "the server proves it knows the password"
    );
    let cookie = head
        .lines()
        .find_map(|line| line.strip_prefix("Set-Cookie: "))
        .expect("a session cookie");
    Ok(cookie.split(';').next().unwrap().trim().to_string())
}

impl ViewerClient {
    /// A page that logs in with [`PASSWORD`].
    fn connect(port: u16, user_agent: &str) -> Self {
        Self::connect_with(port, user_agent, PASSWORD)
    }

    fn connect_with(port: u16, user_agent: &str, password: &str) -> Self {
        let cookie = login(port, password).expect("the right password logs in");
        let mut request = format!("ws://127.0.0.1:{port}/ws")
            .into_client_request()
            .unwrap();
        let headers = request.headers_mut();
        headers.insert("User-Agent", user_agent.parse().unwrap());
        headers.insert("Cookie", cookie.parse().unwrap());
        headers.insert(
            "Origin",
            format!("http://127.0.0.1:{port}").parse().unwrap(),
        );
        let (mut socket, _) =
            tungstenite::client(request, connect(port)).expect("the page connects");
        // Short reads, so the thread also sends presses while nothing arrives.
        socket
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let (sender, messages) = mpsc::channel();
        let (keys, presses) = mpsc::channel::<(u8, bool)>();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                for (key, pressed) in presses.try_iter() {
                    let text = serde_json::json!({"type": "key", "key": key, "pressed": pressed});
                    if socket.send(Message::text(text.to_string())).is_err() {
                        return;
                    }
                }
                match socket.read() {
                    Ok(Message::Text(text)) => {
                        let _ = sender.send(serde_json::from_str(&text).unwrap());
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(error))
                        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                    Err(_) => return,
                }
            }
        });
        Self {
            messages,
            keys,
            stop,
            handle: Some(handle),
        }
    }

    /// The next message of type `kind`, skipping others.
    fn next_of(&self, kind: &str) -> serde_json::Value {
        loop {
            let message = self
                .messages
                .recv_timeout(WAIT)
                .unwrap_or_else(|_| panic!("a {kind:?} message from the server"));
            if message["type"] == kind {
                return message;
            }
        }
    }

    fn press(&self, key: u8, pressed: bool) {
        self.keys.send((key, pressed)).unwrap();
    }
}

impl Drop for ViewerClient {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn hidden_passwords_show_small_bullets_in_every_password_field() {
    run_gtk_test(|| {
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-bullets", remember(port)) else {
            return;
        };
        let panel = panel(&pair);
        let w = panel.test_widgets();
        assert_eq!(virtual_deck_ui::HIDDEN_CHAR, '\u{2022}');
        for entry in [
            &w.first_form.new,
            &w.first_form.repeat,
            &w.change_form.new,
            &w.change_form.repeat,
        ] {
            assert!(entry.has_css_class("virtual-deck-password"));
            let mut texts = Vec::new();
            let mut child = entry.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Ok(text) = widget.downcast::<gtk::Text>() {
                    texts.push(text);
                }
            }
            assert_eq!(texts.len(), 1, "one text inside each password entry");
            assert_eq!(texts[0].invisible_char(), virtual_deck_ui::HIDDEN_CHAR);
        }
    });
}

#[test]
fn the_menu_offers_the_switch_below_export_configuration_and_it_starts_off() {
    run_gtk_test(|| {
        let [port] = free_ports();
        // A port remembered from an earlier run changes nothing at start.
        let Some(pair) = start_pair_with("virtual-menu", remember(port)) else {
            return;
        };
        let _ = virtual_deck_ui::take_opened();
        let menu = pair.registry.menu();
        let panel = panel(&pair);
        let list = menu.button().popover().unwrap().child().unwrap();
        let mut children = Vec::new();
        let mut child = list.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            children.push(current);
        }
        let captions: Vec<String> = children
            .iter()
            .filter_map(|row| row.downcast_ref::<Button>())
            .filter_map(button_caption)
            .map(|caption| caption.to_string())
            .collect();
        assert_eq!(
            captions,
            [
                "Help",
                "Updates",
                "Import configuration...",
                "Export configuration..."
            ]
        );
        assert_eq!(children.len(), 6, "four rows, a hairline, the virtual deck");
        assert!(children[4].is::<gtk::Separator>());
        assert_eq!(&children[5], panel.section().upcast_ref::<gtk::Widget>());
        let w = panel.test_widgets();
        assert_eq!(
            label_texts(panel.section().first_child().unwrap().upcast_ref()),
            [virtual_deck_ui::TITLE]
        );
        assert!(!w.switch.is_active(), "off on every launch (S1)");
        assert!(w.switch.is_sensitive() && w.switch.is_focusable());
        assert!(!w.first_time.get_visible() && !w.running.get_visible());
        assert_eq!(panel.test_phase(), Phase::Off);
        assert!(!badge(&pair).is_visible());
        assert!(!listening(port), "nothing listens until the switch is on");
        assert_eq!(virtual_deck_ui::served_at(), None);
        assert_eq!(pair.registry.live_devices(&VIRTUAL_CLASSIC), 0);
        assert!(virtual_deck_ui::take_opened().is_empty());
    });
}

#[test]
fn the_first_turn_on_of_a_session_asks_for_a_password_and_checks_it_first() {
    run_gtk_test(|| {
        let Some(pair) = start_pair_with("virtual-first-time", |_| {}) else {
            return;
        };
        let _ = virtual_deck_ui::take_opened();
        let settings = config_dir(&pair.roots).join(FILE_NAME);
        let panel = panel(&pair);
        let w = panel.test_widgets();
        w.switch.set_active(true);
        assert!(
            w.switch.is_active() && w.switch.is_sensitive(),
            "no password for this session: the switch stays on and asks"
        );
        assert_eq!(panel.test_phase(), Phase::NeedsPassword);
        assert!(w.first_time.get_visible());
        assert!(!w.running.get_visible());
        assert_eq!(
            label_texts(w.first_time.upcast_ref())[..3],
            [SESSION_MESSAGE, "New password", "Repeat it"]
        );
        assert_eq!(w.first_notice.text(), SESSION_MESSAGE);
        assert_eq!(virtual_deck_ui::SESSION_PASSWORD, SESSION_MESSAGE);
        let form = &w.first_form;
        assert_eq!(button_caption(&form.cancel).as_deref(), Some("Cancel"));
        assert_eq!(
            button_caption(&form.submit).as_deref(),
            Some("Save and turn on")
        );
        assert!(form.submit.has_css_class("suggested-action"));
        let shows = |form: &crate::virtual_deck_ui::PasswordForm, text: &'static str| {
            let (error, submit) = (form.error.clone(), form.submit.clone());
            wait_until(text, move || {
                error.get_visible() && error.text() == text && submit.is_sensitive()
            });
        };
        form.new.set_text("short");
        form.submit.emit_clicked();
        shows(form, "At least 8 characters.");
        form.new.set_text("long enough");
        form.repeat.set_text("long enougj");
        form.submit.emit_clicked();
        shows(form, virtual_deck_ui::MISMATCH);
        // Nothing was stored, started, attached or opened; the switch stays on.
        assert!(!settings.exists());
        assert_eq!(panel.test_phase(), Phase::NeedsPassword);
        assert!(w.switch.is_active() && w.switch.is_sensitive());
        assert_eq!(pair.registry.live_devices(&VIRTUAL_CLASSIC), 0);
        assert!(!badge(&pair).is_visible());
        assert!(virtual_deck_ui::take_opened().is_empty());
        form.cancel.emit_clicked();
        assert_eq!(panel.test_phase(), Phase::Off);
        assert!(!w.switch.is_active() && w.switch.is_sensitive());
        assert!(!w.first_time.get_visible());
        assert_eq!(form.new.text(), "");
        assert_eq!(form.repeat.text(), "");
        assert!(!form.error.get_visible());
        assert!(!settings.exists());
    });
}

/// What an earlier build wrote: an Argon2id PHC string.
const OLD_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2FsdA$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g";

/// Settings files earlier builds wrote with a password in them on `port`: an
/// Argon2id hash, and the verifier of [`PASSWORD`] (salt, iterations,
/// StoredKey, ServerKey), which those builds would have logged in with.
fn files_with_a_password(port: u16) -> [(serde_json::Value, &'static str); 2] {
    let salt = [7u8; 16];
    let iterations = 10_000;
    let salted = salted_password(PASSWORD, &salt, iterations);
    let client_key = hmac_sha256(&salted, b"Client Key");
    let stored_key = digest::digest(&digest::SHA256, &client_key);
    let server_key = hmac_sha256(&salted, b"Server Key");
    [
        (
            serde_json::json!({"port": port, "password_hash": OLD_HASH}),
            "an Argon2id hash",
        ),
        (
            serde_json::json!({
                "port": port,
                "salt": STANDARD.encode(salt),
                "iterations": iterations,
                "stored_key": STANDARD.encode(stored_key.as_ref()),
                "server_key": STANDARD.encode(server_key),
            }),
            "the verifier of a password",
        ),
    ]
}

/// L40: the password lives in memory for the session. Whatever password an
/// earlier build left in the settings file, the first turn-on asks for this
/// session's password; the file stays as it is until Save and turn on, which
/// keeps its port and drops the old password, which no longer logs in.
#[test]
fn the_first_turn_on_asks_for_the_sessions_password_even_with_one_on_disk() {
    run_gtk_test(|| {
        for index in 0..2 {
            let [port] = free_ports();
            let (contents, why) = files_with_a_password(port)[index].clone();
            let text = contents.to_string();
            let written = text.clone();
            let Some(pair) = start_pair_with("virtual-old-password", move |roots| {
                let dir = config_dir(roots);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join(FILE_NAME), &written).unwrap();
            }) else {
                return;
            };
            let _ = virtual_deck_ui::take_opened();
            let settings = config_dir(&pair.roots).join(FILE_NAME);
            let panel = panel(&pair);
            let w = panel.test_widgets();
            w.switch.set_active(true);
            assert!(
                w.switch.is_active(),
                "{why}: no password for this session, the switch stays on and asks"
            );
            assert_eq!(panel.test_phase(), Phase::NeedsPassword, "{why}");
            assert!(w.first_time.get_visible(), "{why}");
            assert_eq!(w.first_notice.text(), SESSION_MESSAGE, "{why}");
            assert!(!w.feedback.get_visible(), "{why}: the file is not an error");
            assert!(!listening(port), "{why}");
            assert_eq!(
                std::fs::read_to_string(&settings).unwrap(),
                text,
                "{why}: kept as it is until saved"
            );
            // Cancel and try again: still kept, still asked.
            w.first_form.cancel.emit_clicked();
            assert!(!w.switch.is_active(), "{why}: Cancel turns it off");
            w.switch.set_active(true);
            assert_eq!(panel.test_phase(), Phase::NeedsPassword, "{why}");
            assert!(w.first_time.get_visible(), "{why}");
            assert_eq!(std::fs::read_to_string(&settings).unwrap(), text, "{why}");
            save_and_turn_on(&panel, NEW_PASSWORD);
            wait_phase(&panel, Phase::On);
            assert_eq!(panel.test_port(), Some(port), "{why}: the remembered port");
            assert_port_only(&settings, port);
            assert!(login(port, NEW_PASSWORD).is_ok(), "{why}");
            assert_eq!(
                login(port, PASSWORD),
                Err(401),
                "{why}: a password on disk never logs in"
            );
            assert!(virtual_deck_ui::take_opened().is_empty(), "{why}");
        }
    });
}

#[test]
fn save_and_turn_on_serves_the_virtual_classic_and_turning_off_removes_it() {
    run_gtk_test(|| {
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-on-off", move |roots| {
            SettingsStore::open(&config_dir(roots))
                .set_port(port)
                .unwrap();
        }) else {
            return;
        };
        let _ = virtual_deck_ui::take_opened();
        let config = config_dir(&pair.roots);
        let panel = panel(&pair);
        let w = panel.test_widgets();
        w.switch.set_active(true);
        let form = &w.first_form;
        form.new.set_text(PASSWORD);
        form.repeat.set_text(PASSWORD);
        form.submit.emit_clicked();
        wait_phase(&panel, Phase::On);
        let url = format!("http://127.0.0.1:{port}/");
        // S4 (L40): nothing about the password is written; the file keeps
        // the port only, owner-only.
        let settings = config.join(FILE_NAME);
        assert_port_only(&settings, port);
        assert!(!w.first_time.get_visible());
        // S5: the server listens on the remembered port over plain HTTP: no
        // certificate, no fingerprint.
        assert!(listening(port));
        assert_eq!(panel.test_port(), Some(port));
        assert!(!config.join("virtual").exists(), "no certificate folder");
        let shown = label_texts(w.running.upcast_ref()).join("\n");
        assert!(
            !shown.contains("ertificate") && !shown.contains("ingerprint"),
            "{shown}"
        );
        assert!(login(port, PASSWORD).is_ok());
        // S6: the address and its QR code, never the password.
        assert_eq!(panel.test_url().as_deref(), Some(url.as_str()));
        assert_eq!(w.address.text(), url);
        assert_eq!(w.qr.text(), url);
        assert!(w.qr.matrix().is_some());
        assert!(!w.qr.text().contains(PASSWORD));
        // S1: turning it on opens no browser window; only Open does, once.
        assert!(virtual_deck_ui::take_opened().is_empty());
        w.open.emit_clicked();
        assert_eq!(virtual_deck_ui::take_opened(), std::slice::from_ref(&url));
        // S1: the hub serves the virtual Classic like a plugged-in device.
        let registry = pair.registry.clone();
        wait_until("the virtual Classic connects", move || {
            registry.live_devices(&VIRTUAL_CLASSIC) == 1
        });
        assert!(w.switch.is_active() && w.switch.is_sensitive());
        let header = badge(&pair);
        assert!(header.is_visible());
        assert!(header.has_css_class("status"));
        assert_eq!(header.text(), "Virtual deck on · 0 viewers");
        assert_eq!(w.viewers_heading.text(), "Viewers (0)");
        assert!(w.no_viewers.get_visible());
        assert_eq!(w.no_viewers.text(), virtual_deck_ui::NO_VIEWERS);
        assert!(!w.viewers_scroll.get_visible());
        let line = format!("Virtual Stream Deck Classic — served at {url}");
        let window = pair.window.clone();
        let expected = line.clone();
        wait_until("the header names the address", move || {
            header_label(&window, "muted").text() == expected
        });
        let tooltip = header_label(&pair.window, "muted")
            .tooltip_text()
            .unwrap()
            .to_string();
        assert!(!tooltip.contains("PID"), "no USB ids: {tooltip}");
        assert!(tooltip.contains(&line), "{tooltip}");
        // Off: the server stops, the device goes and the header says so.
        w.switch.set_active(false);
        assert!(!badge(&pair).is_visible());
        assert_eq!(virtual_deck_ui::served_at(), None);
        wait_phase(&panel, Phase::Off);
        let registry = pair.registry.clone();
        wait_until("the virtual Classic disconnects", move || {
            registry.live_devices(&VIRTUAL_CLASSIC) == 0
        });
        assert!(!listening(port));
        assert!(!w.switch.is_active() && w.switch.is_sensitive());
        assert!(!w.running.get_visible());
        let watched = panel.clone();
        wait_until("every worker is joined", move || watched.test_idle());
        // Disconnected, then nothing found, as after an unplug.
        let window = pair.window.clone();
        wait_until("the header stops naming the address", move || {
            !header_label(&window, "muted").text().contains("served at")
        });
        assert!(virtual_deck_ui::take_opened().is_empty());
    });
}

#[test]
fn the_connection_line_of_the_virtual_classic_names_its_address_not_usb_ids() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let (status, detail) = (Label::new(None), Label::new(None));
        let device =
            sdeck_plus::event_api::DeviceId::from_path(sdeck_plus::event_api::VIRTUAL_DECK_PATH);
        let model = &VIRTUAL_CLASSIC;
        let connected = ConnectionState::Connected { device, model };
        assert!(show_connection_state(
            &status,
            &detail,
            &connected,
            Some(model)
        ));
        assert_eq!(status.text(), "Connected");
        assert_eq!(
            detail.text(),
            "Virtual Stream Deck Classic — served by this app in a web page"
        );
        let tooltip = detail.tooltip_text().unwrap().to_string();
        assert_eq!(
            tooltip,
            "Virtual Stream Deck Classic connected — keys are live\n\
             Virtual Stream Deck Classic — served by this app in a web page · \
             web page input and artwork active"
        );
        let gone = ConnectionState::Disconnected {
            device,
            model,
            reason: "virtual deck detached".into(),
        };
        assert!(!show_connection_state(&status, &detail, &gone, Some(model)));
        assert_eq!(status.text(), "Disconnected");
        assert_eq!(
            detail.text(),
            "Virtual Stream Deck Classic turned off — turn it on from the Menu"
        );
        let tooltip = detail.tooltip_text().unwrap().to_string();
        assert!(!tooltip.contains("PID"), "{tooltip}");
        let failed = ConnectionState::Unavailable("broken".into());
        show_connection_state(&status, &detail, &failed, Some(model));
        let tooltip = detail.tooltip_text().unwrap().to_string();
        assert_eq!(
            tooltip,
            "broken\nVirtual Stream Deck Classic — served by this app in a web page"
        );
        // Hardware keeps its USB ids.
        let plus = ConnectionState::Disconnected {
            device,
            model: &PLUS,
            reason: "unplugged".into(),
        };
        show_connection_state(&status, &detail, &plus, Some(&PLUS));
        assert_eq!(
            detail.text(),
            "Stream Deck + disconnected — plug it back in"
        );
        assert_eq!(
            detail.tooltip_text().unwrap().as_str(),
            "unplugged\nUSB device VID 0FD9 / PID 0084"
        );
    });
}

/// The virtual Classic's Base page: key 1 goes to page "A".
fn seed_virtual_classic(roots: &feature_runtime::SessionRoots) {
    let root = roots.root_for(&VIRTUAL_CLASSIC);
    std::fs::create_dir_all(&root).unwrap();
    let mut layers = LayerStore::open_at_model(&root, &VIRTUAL_CLASSIC).unwrap();
    layers.create("A").unwrap();
    layers
        .bind(
            "Base",
            Trigger::KeyPress(1),
            Action::SwitchLayer("A".into()),
        )
        .unwrap();
    layers.persist().unwrap();
}

#[test]
fn a_viewer_is_listed_presses_keys_sees_pictures_and_disconnect_ends_it() {
    run_gtk_test(|| {
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-viewers", move |roots| {
            remember(port)(roots);
            seed_virtual_classic(roots);
        }) else {
            return;
        };
        let panel = turn_on_and_wait(&pair);
        let w = panel.test_widgets();
        let viewer = ViewerClient::connect(port, ANDROID_CHROME);
        // A new page gets the deck as it is, starting blank.
        viewer.next_of("reset");
        let heading = w.viewers_heading.clone();
        wait_until("the viewer is listed", move || {
            heading.text() == "Viewers (1)"
        });
        assert!(w.viewers_scroll.get_visible());
        assert!(!w.no_viewers.get_visible());
        let row = label_texts(w.viewers.upcast_ref());
        assert_eq!(row[0], "Android · Chrome 131");
        assert!(row[1].starts_with("127.0.0.1 · since "), "{row:?}");
        assert_eq!(badge(&pair).text(), "Virtual deck on · 1 viewer");
        // A press on the page runs the virtual Classic's binding like the real key.
        let runtime = pair.registry.runtime_of(&VIRTUAL_CLASSIC).unwrap();
        assert_eq!(active(&runtime), "Base");
        while viewer.messages.try_recv().is_ok() {}
        viewer.press(1, true);
        viewer.press(1, false);
        wait_layer("the page press switches to A", &runtime, "A");
        // The new page's pictures reach the page.
        let key = viewer.next_of("key");
        assert!(key["key"]
            .as_u64()
            .is_some_and(|key| (1..=15).contains(&key)));
        assert!(key["jpeg"].as_str().is_some_and(|jpeg| !jpeg.is_empty()));
        // Disconnect says bye to that page and the list empties.
        let disconnect = buttons_in(w.viewers.upcast_ref());
        assert_eq!(disconnect.len(), 1);
        assert_eq!(
            button_caption(&disconnect[0]).as_deref(),
            Some("Disconnect")
        );
        disconnect[0].emit_clicked();
        assert_eq!(viewer.next_of("bye")["reason"], "disconnected");
        let heading = w.viewers_heading.clone();
        wait_until("the list empties", move || heading.text() == "Viewers (0)");
        assert!(w.no_viewers.get_visible());
        assert_eq!(badge(&pair).text(), "Virtual deck on · 0 viewers");
    });
}

/// L40: the session's password stays in memory: turned off and on again in
/// the same session, the deck starts at once without the form, viewers log in
/// with that password, and nothing about it is on disk.
#[test]
fn off_and_on_again_reuses_the_sessions_password_without_asking() {
    run_gtk_test(|| {
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-session", remember(port)) else {
            return;
        };
        let settings = config_dir(&pair.roots).join(FILE_NAME);
        let panel = turn_on_and_wait(&pair);
        let w = panel.test_widgets();
        assert!(login(port, PASSWORD).is_ok());
        assert_port_only(&settings, port);
        w.switch.set_active(false);
        wait_phase(&panel, Phase::Off);
        assert!(!listening(port));
        // On again: no form, it starts with the same password.
        w.switch.set_active(true);
        assert!(!w.first_time.get_visible(), "no password form this time");
        assert!(w.switch.is_active());
        assert_eq!(panel.test_phase(), Phase::Starting);
        wait_phase(&panel, Phase::On);
        assert!(!w.first_time.get_visible());
        let registry = pair.registry.clone();
        wait_until("the virtual Classic is served again", move || {
            registry.live_devices(&VIRTUAL_CLASSIC) == 1
        });
        let viewer = ViewerClient::connect(port, ANDROID_CHROME);
        viewer.next_of("reset");
        let heading = w.viewers_heading.clone();
        wait_until("the viewer is listed", move || {
            heading.text() == "Viewers (1)"
        });
        assert_eq!(login(port, NEW_PASSWORD), Err(401));
        assert_port_only(&settings, port);
    });
}

#[test]
fn changing_the_password_asks_first_then_disconnects_every_viewer() {
    run_gtk_test(|| {
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-password", remember(port)) else {
            return;
        };
        let config = config_dir(&pair.roots);
        let settings = config.join(FILE_NAME);
        let panel = turn_on_and_wait(&pair);
        let w = panel.test_widgets();
        let viewer = ViewerClient::connect(port, MAC_SAFARI);
        let heading = w.viewers_heading.clone();
        wait_until("the viewer is listed", move || {
            heading.text() == "Viewers (1)"
        });
        assert_eq!(button_caption(&w.change).as_deref(), Some("Change…"));
        w.change.emit_clicked();
        let form = &w.change_form;
        assert!(form.root.get_visible());
        assert_eq!(
            button_caption(&form.submit).as_deref(),
            Some("Change password")
        );
        let before = std::fs::read(&settings).unwrap();
        let ask = |panel: &VirtualDeckPanel| {
            form.new.set_text("another secret");
            form.repeat.set_text("another secret");
            form.submit.emit_clicked();
            let watched = panel.clone();
            wait_until("the confirmation", move || {
                watched.test_confirmation().is_some()
            });
            panel.test_confirmation().unwrap()
        };
        let dialog = ask(&panel);
        assert_eq!(
            dialog.property::<Option<String>>("text").as_deref(),
            Some(virtual_deck_ui::CHANGE_TITLE)
        );
        assert_eq!(
            dialog
                .property::<Option<String>>("secondary-text")
                .as_deref(),
            Some(virtual_deck_ui::CHANGE_WARNING)
        );
        dialog.response(gtk::ResponseType::Cancel);
        assert_eq!(
            std::fs::read(&settings).unwrap(),
            before,
            "Cancel changes nothing"
        );
        assert_eq!(w.viewers_heading.text(), "Viewers (1)");
        let dialog = ask(&panel);
        dialog.response(gtk::ResponseType::Accept);
        assert_eq!(viewer.next_of("bye")["reason"], "password-changed");
        let heading = w.viewers_heading.clone();
        wait_until("every viewer is gone", move || {
            heading.text() == "Viewers (0)"
        });
        assert!(!form.root.get_visible());
        // L40: the new password replaces the old one in memory only.
        assert_eq!(
            std::fs::read(&settings).unwrap(),
            before,
            "nothing about the password is written"
        );
        assert_port_only(&settings, port);
        assert_eq!(login(port, PASSWORD), Err(401), "the old one is refused");
        assert!(login(port, NEW_PASSWORD).is_ok());
        // Off and on again, the session keeps the new password.
        w.switch.set_active(false);
        wait_phase(&panel, Phase::Off);
        w.switch.set_active(true);
        assert!(!w.first_time.get_visible(), "no password form");
        wait_phase(&panel, Phase::On);
        assert_eq!(login(port, PASSWORD), Err(401));
        assert!(login(port, NEW_PASSWORD).is_ok());
    });
}

#[test]
fn a_new_port_restarts_the_server_and_a_busy_one_is_reported() {
    run_gtk_test(|| {
        let [first, second] = free_ports();
        let Some(pair) = start_pair_with("virtual-port", remember(first)) else {
            return;
        };
        let config = config_dir(&pair.roots);
        let panel = turn_on_and_wait(&pair);
        let _ = virtual_deck_ui::take_opened();
        let w = panel.test_widgets();
        assert_eq!(w.port.text(), first.to_string());
        assert!(label_texts(w.running.upcast_ref()).contains(&"Restarts the server".to_string()));
        let stored_port = || {
            let store = SettingsStore::open(&config);
            store.settings().port
        };
        // Out of range: said under the field, nothing restarts.
        w.port.set_text("80");
        w.port.emit_activate();
        assert!(w.port_feedback.get_visible());
        assert_eq!(w.port_feedback.text(), "Use a port from 1024 to 65535.");
        assert_eq!(panel.test_phase(), Phase::On);
        // In use: the server stays where it was.
        let blocker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let busy = blocker.local_addr().unwrap().port();
        w.port.set_text(&busy.to_string());
        w.port.emit_activate();
        let (feedback, watched) = (w.port_feedback.clone(), panel.clone());
        let message = format!("Port {busy} is already in use.");
        let expected = message.clone();
        wait_until("the busy port is reported", move || {
            feedback.text() == expected && watched.test_phase() == Phase::On
        });
        assert!(w.port_feedback.get_visible());
        assert_eq!(panel.test_port(), Some(first));
        assert!(listening(first));
        assert_eq!(stored_port(), first);
        // A free port: the server moves there and the port is remembered.
        w.port.set_text(&second.to_string());
        w.port.emit_activate();
        let watched = panel.clone();
        wait_until("the server moves", move || {
            watched.test_port() == Some(second) && watched.test_phase() == Phase::On
        });
        let url = format!("http://127.0.0.1:{second}/");
        assert_eq!(w.address.text(), url);
        assert_eq!(w.qr.text(), url);
        assert!(!w.port_feedback.get_visible());
        assert_eq!(stored_port(), second);
        assert!(listening(second));
        wait_until("the old server stops", move || !listening(first));
        let registry = pair.registry.clone();
        wait_until("the virtual Classic is served again", move || {
            registry.live_devices(&VIRTUAL_CLASSIC) == 1
        });
        let window = pair.window.clone();
        let line = format!("Virtual Stream Deck Classic — served at {url}");
        wait_until("the header names the new address", move || {
            header_label(&window, "muted").text() == line
        });
        assert!(
            virtual_deck_ui::take_opened().is_empty(),
            "a new port opens no browser"
        );
        // Quitting stops the server and drops the deck.
        panel.shutdown();
        assert!(!listening(second));
        assert!(!badge(&pair).is_visible());
        let registry = pair.registry.clone();
        wait_until("the virtual Classic goes", move || {
            registry.live_devices(&VIRTUAL_CLASSIC) == 0
        });
        drop(blocker);
    });
}

fn wait_port(panel: &VirtualDeckPanel, port: u16) {
    let watched = panel.clone();
    wait_until(&format!("the server to run on {port}"), move || {
        watched.test_port() == Some(port) && watched.test_phase() == Phase::On
    });
}

fn accept_button(dialog: &gtk::MessageDialog) -> Button {
    dialog
        .widget_for_response(gtk::ResponseType::Accept)
        .and_downcast::<Button>()
        .expect("the confirmation's Change password button")
}

fn secondary_text(dialog: &gtk::MessageDialog) -> Option<String> {
    dialog.property::<Option<String>>("secondary-text")
}

/// L21 minor 1: a new password confirmed while the server moves to another
/// port must not leave the new server on the old password. The confirmation
/// acts only while the deck is on: it waits, says why, then reaches the new
/// server, which disconnects every viewer.
#[test]
fn a_password_confirmed_while_the_server_restarts_waits_then_reaches_the_new_server() {
    run_gtk_test(|| {
        let [first, second] = free_ports();
        let Some(pair) = start_pair_with("virtual-password-restart", remember(first)) else {
            return;
        };
        let config = config_dir(&pair.roots);
        let panel = turn_on_and_wait(&pair);
        let w = panel.test_widgets();
        // The new password was hashed before the restart and waits for confirmation.
        w.change.emit_clicked();
        let form = &w.change_form;
        form.new.set_text(NEW_PASSWORD);
        form.repeat.set_text(NEW_PASSWORD);
        form.submit.emit_clicked();
        let watched = panel.clone();
        wait_until("the confirmation", move || {
            watched.test_confirmation().is_some()
        });
        let dialog = panel.test_confirmation().unwrap();
        let accept = accept_button(&dialog);
        assert!(accept.is_sensitive());
        // The server moves to another port and its new server is held starting.
        let hold = panel.test_hold_servers();
        w.port.set_text(&second.to_string());
        w.port.emit_activate();
        assert_eq!(panel.test_phase(), Phase::Restarting);
        let while_restarting = (
            w.change.is_sensitive(),
            accept.is_sensitive(),
            secondary_text(&dialog),
        );
        dialog.response(gtk::ResponseType::Accept);
        hold.release();
        wait_port(&panel, second);
        // Nothing was confirmed while restarting: the new server takes the
        // session's password, still the first one.
        assert!(
            login(second, PASSWORD).is_ok(),
            "the new server takes the session's password"
        );
        assert_eq!(
            login(second, NEW_PASSWORD),
            Err(401),
            "nothing was confirmed while restarting"
        );
        // Change… and the confirmation waited and said why.
        assert_eq!(
            while_restarting,
            (
                false,
                false,
                Some(virtual_deck_ui::CONFIRM_WAIT.to_string())
            )
        );
        assert_eq!(
            panel.test_confirmation().as_ref(),
            Some(&dialog),
            "the confirmation still asks"
        );
        // On again: it can be confirmed, and it disconnects every viewer of the new server.
        assert!(w.change.is_sensitive());
        assert!(accept.is_sensitive());
        assert_eq!(
            secondary_text(&dialog).as_deref(),
            Some(virtual_deck_ui::CHANGE_WARNING)
        );
        let viewer = ViewerClient::connect(second, MAC_SAFARI);
        let heading = w.viewers_heading.clone();
        wait_until("the viewer is listed", move || {
            heading.text() == "Viewers (1)"
        });
        accept.emit_clicked();
        assert_eq!(viewer.next_of("bye")["reason"], "password-changed");
        assert!(panel.test_confirmation().is_none());
        assert!(!form.root.get_visible());
        assert_eq!(login(second, PASSWORD), Err(401), "the old one is refused");
        assert!(login(second, NEW_PASSWORD).is_ok());
        assert_port_only(&config.join(FILE_NAME), second);
    });
}

/// A form submitted while the server restarts is refused with the reason.
#[test]
fn a_new_password_is_refused_while_the_server_restarts() {
    run_gtk_test(|| {
        let [first, second] = free_ports();
        let Some(pair) = start_pair_with("virtual-password-wait", remember(first)) else {
            return;
        };
        let config = config_dir(&pair.roots);
        let panel = turn_on_and_wait(&pair);
        let w = panel.test_widgets();
        w.change.emit_clicked();
        let form = &w.change_form;
        let hold = panel.test_hold_servers();
        w.port.set_text(&second.to_string());
        w.port.emit_activate();
        assert_eq!(panel.test_phase(), Phase::Restarting);
        assert!(form.root.get_visible(), "the form stays open");
        form.new.set_text(NEW_PASSWORD);
        form.repeat.set_text(NEW_PASSWORD);
        form.submit.emit_clicked();
        assert!(form.error.get_visible());
        assert_eq!(form.error.text(), virtual_deck_ui::CHANGE_WAIT);
        assert!(panel.test_confirmation().is_none());
        hold.release();
        wait_port(&panel, second);
        let watched = panel.clone();
        wait_until("every worker is joined", move || watched.test_idle());
        assert!(panel.test_confirmation().is_none());
        // The session keeps the password it had.
        assert!(login(second, PASSWORD).is_ok());
        assert_eq!(login(second, NEW_PASSWORD), Err(401));
        assert_port_only(&config.join(FILE_NAME), second);
    });
}

/// A server that restarts on another port takes the session's current
/// password, the one Change… set, never an older one.
#[test]
fn a_server_that_restarts_takes_the_sessions_current_password() {
    run_gtk_test(|| {
        let [first, second] = free_ports();
        let Some(pair) = start_pair_with("virtual-password-resync", remember(first)) else {
            return;
        };
        let config = config_dir(&pair.roots);
        let panel = turn_on_and_wait(&pair);
        let w = panel.test_widgets();
        w.change.emit_clicked();
        let form = &w.change_form;
        form.new.set_text(NEW_PASSWORD);
        form.repeat.set_text(NEW_PASSWORD);
        form.submit.emit_clicked();
        let watched = panel.clone();
        wait_until("the confirmation", move || {
            watched.test_confirmation().is_some()
        });
        accept_button(&panel.test_confirmation().unwrap()).emit_clicked();
        assert!(login(first, NEW_PASSWORD).is_ok());
        w.port.set_text(&second.to_string());
        w.port.emit_activate();
        wait_port(&panel, second);
        assert_eq!(
            login(second, PASSWORD),
            Err(401),
            "the restarted server refuses the older password"
        );
        assert!(login(second, NEW_PASSWORD).is_ok());
        assert_port_only(&config.join(FILE_NAME), second);
    });
}

/// A scratch folder outside every window's roots, removed when dropped.
struct Kept(PathBuf);

impl Drop for Kept {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Copies the folder `from` to `to`: folders, files (with their modes) and links.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_tree(&source, &target);
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&source).unwrap(), &target).unwrap();
        } else {
            std::fs::copy(&source, &target).unwrap();
        }
    }
}

/// The user: "que el password expire no significa que la configuracion del
/// virtual no se persista." A restart forgets only the session's password:
/// the port and the virtual Classic's own data (a binding and the page's
/// keypad background, changed while it ran) are there in the next run, which
/// asks for a password again and runs that data.
#[test]
fn a_restart_forgets_the_password_but_keeps_the_port_and_the_virtual_classics_data() {
    run_gtk_test(|| {
        let [first, second] = free_ports();
        let kept = Kept(scratch("virtual-restart-kept"));
        {
            let Some(pair) = start_pair_with("virtual-restart-before", move |roots| {
                remember(first)(roots);
                seed_virtual_classic(roots);
            }) else {
                return;
            };
            let panel = turn_on_and_wait(&pair);
            let w = panel.test_widgets();
            let root = pair.roots.root_for(&VIRTUAL_CLASSIC);
            // While it runs: key 2 goes to page "A", and the page gets a
            // keypad background.
            let runtime = pair.runtime(&VIRTUAL_CLASSIC);
            let snapshot = {
                let mut layers = runtime.layers().borrow_mut();
                layers
                    .bind(
                        "Base",
                        Trigger::KeyPress(2),
                        Action::SwitchLayer("A".into()),
                    )
                    .unwrap();
                layers.clone()
            };
            runtime.persist(snapshot);
            let saved = root.clone();
            wait_until("the binding is saved", move || {
                stored_binding(&saved, &VIRTUAL_CLASSIC, 2)
            });
            let picture = pair.base.join("background.png");
            let image = Pixbuf::new(Colorspace::Rgb, false, 8, 480, 272).unwrap();
            image.fill(0xff0000ff);
            image.savev(&picture, "png", &[]).unwrap();
            let artwork = runtime.artwork_ui();
            artwork
                .selected_target
                .set(ArtworkTarget::TouchStripBackground);
            artwork.import(&artwork.capture_owner(), &picture).unwrap();
            assert!(root.join(KEYPAD_BACKGROUND_FILE).is_file());
            // A new port, remembered.
            w.port.set_text(&second.to_string());
            w.port.emit_activate();
            wait_port(&panel, second);
            // Quit: the server stops and every pending save lands.
            pair.app.emit_by_name::<()>("shutdown", &[]);
            assert!(!listening(second));
            let settings = config_dir(&pair.roots).join(FILE_NAME);
            assert_port_only(&settings, second);
            std::fs::copy(&settings, kept.0.join(FILE_NAME)).unwrap();
            copy_tree(&root, &kept.0.join("virtual-classic"));
        }
        // The next run, over the same files.
        let source = kept.0.clone();
        let Some(pair) = start_pair_with("virtual-restart-after", move |roots| {
            std::fs::copy(source.join(FILE_NAME), config_dir(roots).join(FILE_NAME)).unwrap();
            copy_tree(
                &source.join("virtual-classic"),
                &roots.root_for(&VIRTUAL_CLASSIC),
            );
        }) else {
            return;
        };
        let settings = config_dir(&pair.roots).join(FILE_NAME);
        assert_port_only(&settings, second);
        let panel = panel(&pair);
        let w = panel.test_widgets();
        assert!(!w.switch.is_active(), "off at launch");
        // The password did not survive: this run asks for one.
        w.switch.set_active(true);
        assert_eq!(
            panel.test_phase(),
            Phase::NeedsPassword,
            "no password for this session yet"
        );
        assert!(w.switch.is_active());
        assert!(w.first_time.get_visible());
        assert_eq!(w.first_notice.text(), SESSION_MESSAGE);
        assert!(!listening(second));
        save_and_turn_on(&panel, NEW_PASSWORD);
        // The port did.
        wait_port(&panel, second);
        assert_eq!(
            login(second, PASSWORD),
            Err(401),
            "the last run's password is gone"
        );
        assert!(login(second, NEW_PASSWORD).is_ok());
        assert_port_only(&settings, second);
        // So did the virtual Classic's data, and it runs.
        let registry = pair.registry.clone();
        wait_until("the virtual Classic connects", move || {
            registry.live_devices(&VIRTUAL_CLASSIC) == 1
        });
        let runtime = pair.runtime(&VIRTUAL_CLASSIC);
        assert_eq!(
            runtime
                .layers()
                .borrow()
                .binding("Base", &Trigger::KeyPress(2)),
            Some(Action::SwitchLayer("A".into()))
        );
        let root = pair.roots.root_for(&VIRTUAL_CLASSIC);
        let background = runtime
            .artwork_ui()
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Base", ArtworkTarget::TouchStripBackground)
            .expect("the page keeps its keypad background");
        assert!(
            background.starts_with(&root) && background.is_file(),
            "{background:?}"
        );
        assert_eq!(active(&runtime), "Base");
        let viewer = ViewerClient::connect_with(second, ANDROID_CHROME, NEW_PASSWORD);
        viewer.next_of("reset");
        viewer.press(2, true);
        viewer.press(2, false);
        wait_layer("the kept binding switches to A", &runtime, "A");
    });
}

/// Makes a folder read-only (0500) until dropped, then owner-writable again.
struct ReadOnly(PathBuf);

impl ReadOnly {
    fn new(path: &Path) -> Self {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500)).unwrap();
        Self(path.to_path_buf())
    }
}

impl Drop for ReadOnly {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
    }
}

/// L21 minor 2: the server moved but the settings could not remember the
/// port; the menu says so and keeps showing the port it serves.
#[test]
fn a_new_port_that_cannot_be_remembered_is_still_the_one_shown_and_served() {
    run_gtk_test(|| {
        let [first, second] = free_ports();
        let Some(pair) = start_pair_with("virtual-port-unsaved", remember(first)) else {
            return;
        };
        let config = config_dir(&pair.roots);
        let panel = turn_on_and_wait(&pair);
        let w = panel.test_widgets();
        // The settings file can be read but no longer replaced.
        let _read_only = ReadOnly::new(&config);
        let probe = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(config.join(".writable-probe"));
        if probe.is_ok() {
            eprintln!("The config folder stays writable (root?): nothing to check.");
            return;
        }
        w.port.set_text(&second.to_string());
        w.port.emit_activate();
        wait_port(&panel, second);
        let url = format!("http://127.0.0.1:{second}/");
        assert!(listening(second));
        assert_eq!(w.port.text(), second.to_string());
        assert_eq!(w.address.text(), url);
        assert_eq!(panel.test_url().as_deref(), Some(url.as_str()));
        assert!(w.port_feedback.get_visible());
        let said = w.port_feedback.text().to_string();
        let expected = virtual_deck_ui::port_not_remembered(
            second,
            &format!("Could not save {}: ", config.join(FILE_NAME).display()),
        );
        assert!(said.starts_with(&expected), "{said}");
        assert_eq!(
            SettingsStore::open(&config).settings().port,
            first,
            "the old port is still the remembered one"
        );
        wait_until("the old server stops", move || !listening(first));
    });
}

/// An unusable settings file leaves the switch off with the reason. A busy
/// port after Save and turn on keeps the switch on and the form open with the
/// reason, so it can be tried again; with the session's password set, the
/// switch goes back off with the reason, as before.
#[test]
fn an_unusable_settings_file_or_a_busy_port_says_why() {
    run_gtk_test(|| {
        let Some(pair) = start_pair_with("virtual-refused", |roots| {
            let dir = config_dir(roots);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(FILE_NAME), b"edited by hand").unwrap();
        }) else {
            return;
        };
        let _ = virtual_deck_ui::take_opened();
        let config = config_dir(&pair.roots);
        let panel = panel(&pair);
        let w = panel.test_widgets();
        w.switch.set_active(true);
        assert!(!w.switch.is_active());
        assert_eq!(panel.test_phase(), Phase::Off);
        assert!(w.feedback.get_visible());
        let said = w.feedback.text().to_string();
        assert!(said.contains(FILE_NAME) && said.contains("kept"), "{said}");
        assert!(
            !w.first_time.get_visible(),
            "no password can be saved over it"
        );
        assert_eq!(
            std::fs::read(config.join(FILE_NAME)).unwrap(),
            b"edited by hand"
        );
        // Repaired, but its port is taken: Save and turn on keeps the switch
        // on and the form open, with the reason.
        std::fs::remove_file(config.join(FILE_NAME)).unwrap();
        let blocker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let busy = blocker.local_addr().unwrap().port();
        remember(busy)(&pair.roots);
        w.switch.set_active(true);
        assert_eq!(panel.test_phase(), Phase::NeedsPassword);
        assert!(w.first_time.get_visible(), "the session's password first");
        assert!(!w.feedback.get_visible(), "the earlier reason is gone");
        save_and_turn_on(&panel, PASSWORD);
        let reason = format!("Port {busy} is already in use.");
        let form = &w.first_form;
        let (error, watched, expected) = (form.error.clone(), panel.clone(), reason.clone());
        wait_until("the busy port is said in the form", move || {
            watched.test_phase() == Phase::NeedsPassword
                && error.get_visible()
                && error.text() == expected
        });
        assert!(form.error.has_css_class("feedback-error"));
        assert!(
            w.switch.is_active() && w.switch.is_sensitive(),
            "the switch stays on"
        );
        assert!(w.first_time.get_visible());
        assert!(form.submit.is_sensitive() && form.cancel.is_sensitive());
        assert_eq!(form.new.text(), PASSWORD, "kept to try again");
        assert_eq!(form.repeat.text(), PASSWORD, "kept to try again");
        assert!(!w.feedback.get_visible());
        // The blocker holds the port: everything else shows nothing runs.
        assert_eq!(pair.registry.live_devices(&VIRTUAL_CLASSIC), 0);
        assert!(!badge(&pair).is_visible());
        assert_eq!(virtual_deck_ui::served_at(), None);
        assert!(!w.running.get_visible());
        // Tried again once the port is free: it turns on.
        drop(blocker);
        form.submit.emit_clicked();
        wait_phase(&panel, Phase::On);
        assert!(listening(busy));
        assert!(!w.first_time.get_visible());
        assert!(!form.error.get_visible());
        assert_eq!(form.new.text(), "");
        assert!(login(busy, PASSWORD).is_ok());
        // Off, and the port taken again: with the session's password the
        // switch starts at once and goes back off with the reason.
        w.switch.set_active(false);
        wait_phase(&panel, Phase::Off);
        let registry = pair.registry.clone();
        wait_until("the virtual Classic goes", move || {
            registry.live_devices(&VIRTUAL_CLASSIC) == 0
        });
        let blocker = TcpListener::bind((Ipv4Addr::LOCALHOST, busy)).unwrap();
        w.switch.set_active(true);
        assert!(!w.first_time.get_visible(), "no password form");
        assert_eq!(panel.test_phase(), Phase::Starting);
        wait_phase(&panel, Phase::Off);
        assert!(!w.switch.is_active() && w.switch.is_sensitive());
        assert_eq!(w.feedback.text(), reason);
        assert!(w.feedback.get_visible());
        assert!(!w.first_time.get_visible());
        assert_eq!(pair.registry.live_devices(&VIRTUAL_CLASSIC), 0);
        assert!(!badge(&pair).is_visible());
        assert!(virtual_deck_ui::take_opened().is_empty());
        drop(blocker);
    });
}

/// L45: "primero se enciende el switch para mostrar que hay que setear el
/// password, luego cuando haga el saveAndOn recien que corra el servidor."
/// Turned on without this session's password, the switch stays on with the
/// form below it (its notice neutral, not a warning) and nothing runs; a
/// mismatch is said in the form; Save and turn on starts the server and the
/// device.
#[test]
fn turning_on_without_the_sessions_password_keeps_the_switch_on_and_starts_only_on_save() {
    run_gtk_test(|| {
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-needs-password", remember(port)) else {
            return;
        };
        let _ = virtual_deck_ui::take_opened();
        let settings = config_dir(&pair.roots).join(FILE_NAME);
        let before = std::fs::read(&settings).unwrap();
        let panel = panel(&pair);
        let w = panel.test_widgets();
        let form = &w.first_form;
        w.switch.set_active(true);
        assert_eq!(panel.test_phase(), Phase::NeedsPassword);
        assert!(w.switch.is_active() && w.switch.is_sensitive());
        assert!(w.first_time.get_visible() && form.root.get_visible());
        assert_eq!(w.first_notice.text(), SESSION_MESSAGE);
        // A neutral note, not a warning or an error.
        assert!(w.first_notice.has_css_class("helper"));
        for tone in ["feedback-warning", "feedback-error", "feedback-success"] {
            assert!(!w.first_notice.has_css_class(tone), "{tone}");
        }
        assert!(!w.feedback.get_visible(), "nothing failed");
        assert_nothing_runs(&pair, &panel, port, "asking for the password");
        // A mismatch: said in the form, the switch stays on, nothing starts.
        form.new.set_text(PASSWORD);
        form.repeat.set_text("correct horsf");
        form.submit.emit_clicked();
        let (error, submit) = (form.error.clone(), form.submit.clone());
        wait_until("the mismatch", move || {
            error.get_visible() && submit.is_sensitive()
        });
        assert_eq!(form.error.text(), virtual_deck_ui::MISMATCH);
        assert!(form.error.has_css_class("feedback-error"));
        assert_eq!(panel.test_phase(), Phase::NeedsPassword);
        assert!(w.switch.is_active() && w.switch.is_sensitive());
        assert!(w.first_time.get_visible());
        let watched = panel.clone();
        wait_until("every worker is joined", move || watched.test_idle());
        assert_nothing_runs(&pair, &panel, port, "after a mismatch");
        assert_eq!(std::fs::read(&settings).unwrap(), before, "nothing saved");
        // Save and turn on: only now the server starts. Held while it
        // starts, the switch stays on and the form waits.
        let hold = panel.test_hold_servers();
        form.repeat.set_text(PASSWORD);
        form.submit.emit_clicked();
        let watched = panel.clone();
        wait_until("the server starting", move || {
            watched.test_phase() == Phase::Starting
        });
        assert!(w.switch.is_active() && !w.switch.is_sensitive());
        assert!(w.first_time.get_visible() && !form.submit.is_sensitive());
        assert!(!badge(&pair).is_visible());
        hold.release();
        wait_phase(&panel, Phase::On);
        assert!(listening(port));
        assert!(w.switch.is_active() && w.switch.is_sensitive());
        assert!(!w.first_time.get_visible());
        assert_eq!(form.new.text(), "");
        assert_eq!(form.repeat.text(), "");
        assert!(w.running.get_visible());
        let header = badge(&pair);
        assert!(header.is_visible());
        assert_eq!(header.text(), "Virtual deck on · 0 viewers");
        let registry = pair.registry.clone();
        wait_until("the virtual Classic connects", move || {
            registry.live_devices(&VIRTUAL_CLASSIC) == 1
        });
        assert!(login(port, PASSWORD).is_ok());
        assert_port_only(&settings, port);
        assert!(virtual_deck_ui::take_opened().is_empty());
    });
}

/// L45: Cancel, or turning the switch off while the form asks, closes the
/// form and turns the switch off; nothing ran, so nothing stops. A password
/// still being checked then starts nothing.
#[test]
fn cancel_or_the_switch_while_the_password_is_asked_turns_off_and_starts_nothing() {
    run_gtk_test(|| {
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-needs-password-cancel", remember(port)) else {
            return;
        };
        let settings = config_dir(&pair.roots).join(FILE_NAME);
        let before = std::fs::read(&settings).unwrap();
        let panel = panel(&pair);
        let w = panel.test_widgets();
        let form = &w.first_form;
        let assert_off = |why: &str| {
            assert_eq!(panel.test_phase(), Phase::Off, "{why}");
            assert!(!w.switch.is_active() && w.switch.is_sensitive(), "{why}");
            assert!(!w.first_time.get_visible(), "{why}: the form closes");
            assert_eq!(form.new.text(), "", "{why}: cleared");
            assert_eq!(form.repeat.text(), "", "{why}: cleared");
            assert!(!form.error.get_visible(), "{why}");
            assert_nothing_runs(&pair, &panel, port, why);
        };
        // Cancel.
        w.switch.set_active(true);
        assert_eq!(panel.test_phase(), Phase::NeedsPassword);
        form.new.set_text(PASSWORD);
        form.repeat.set_text("typo");
        form.cancel.emit_clicked();
        assert_off("Cancel");
        // The switch turned off while the form asks.
        w.switch.set_active(true);
        assert_eq!(panel.test_phase(), Phase::NeedsPassword);
        assert!(w.first_time.get_visible());
        assert_eq!(form.new.text(), "", "the form opens empty");
        form.new.set_text(PASSWORD);
        w.switch.set_active(false);
        assert_off("the switch off");
        // Off and on again while a password is being checked: that password
        // is no longer asked for and starts nothing.
        w.switch.set_active(true);
        save_and_turn_on(&panel, PASSWORD);
        w.switch.set_active(false);
        w.switch.set_active(true);
        assert_eq!(panel.test_phase(), Phase::NeedsPassword);
        assert!(form.new.is_sensitive() && form.submit.is_sensitive());
        let watched = panel.clone();
        wait_until("every worker is joined", move || watched.test_idle());
        handle_pending_events();
        assert_eq!(
            panel.test_phase(),
            Phase::NeedsPassword,
            "the earlier password starts nothing"
        );
        assert!(w.switch.is_active() && w.first_time.get_visible());
        assert_eq!(form.new.text(), "");
        assert!(panel.test_idle());
        assert_nothing_runs(&pair, &panel, port, "a dropped password");
        assert_eq!(std::fs::read(&settings).unwrap(), before, "nothing saved");
        form.cancel.emit_clicked();
        assert_off("Cancel at last");
        assert!(panel.test_idle());
    });
}

#[test]
fn the_current_selector_lists_the_virtual_classic_while_it_is_on() {
    run_gtk_test(|| {
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-selector", remember(port)) else {
            return;
        };
        let plus = SimDevice::new(&PLUS);
        pair.bus.plug(PLUS_PATH, &plus);
        pair.wait_devices(1);
        let entries = |pair: &Pair| {
            let names = &pair.registry.selector().names;
            (0..names.n_items())
                .filter_map(|index| names.string(index))
                .map(String::from)
                .collect::<Vec<_>>()
        };
        assert!(!pair.registry.selector().root.is_visible());
        let panel = turn_on_and_wait(&pair);
        pair.wait_devices(2);
        assert!(pair.registry.selector().root.is_visible());
        assert_eq!(
            entries(&pair),
            ["Stream Deck +", "Virtual Stream Deck Classic"]
        );
        let detail = header_label(&pair.window, "muted");
        let sentence = "Stream Deck + and Virtual Stream Deck Classic connected — \
                        keys, dials and touch strip are live";
        assert_eq!(detail.text(), sentence);
        assert_eq!(
            detail.tooltip_text().unwrap().as_str(),
            format!(
                "{sentence}\nUSB device VID 0FD9 / PID 0084 · HID input and artwork active\n\
                 Virtual Stream Deck Classic — served at http://127.0.0.1:{port}/"
            )
        );
        panel.test_widgets().switch.set_active(false);
        wait_phase(&panel, Phase::Off);
        pair.wait_devices(1);
        assert!(!pair.registry.selector().root.is_visible());
        assert_eq!(entries(&pair), ["Stream Deck +"]);
    });
}

/// Opt-in captures (STREAMDECK_VIRTUAL_QA_DIR): the menu off, first time and
/// running with two viewers, and the window header while on.
#[test]
fn virtual_deck_menu_capture() {
    run_gtk_test(|| {
        let Some(directory) = std::env::var_os("STREAMDECK_VIRTUAL_QA_DIR") else {
            return;
        };
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-capture", |_| {}) else {
            return;
        };
        let config = config_dir(&pair.roots);
        let menu = pair.registry.menu().clone();
        let panel = panel(&pair);
        let w = panel.test_widgets();
        let popover = menu.button().popover().unwrap();
        popover.set_autohide(false);
        let settings = gtk::Settings::default().unwrap();
        let original = settings.is_gtk_application_prefer_dark_theme();
        let capture = |name: &str| {
            for (theme, dark) in [("dark", true), ("light", false)] {
                settings.set_gtk_application_prefer_dark_theme(dark);
                let anchor = menu.button().clone();
                // Closed first, so opening it publishes a fresh frame.
                anchor.popdown();
                rendered_text_popup(&popover, || anchor.popup())
                    .save_to_png(directory.join(format!("{name}-{theme}.png")))
                    .unwrap();
            }
        };
        capture("virtual-deck-menu-off");
        SettingsStore::open(&config).set_port(port).unwrap();
        w.switch.set_active(true);
        w.first_form.new.set_text("1234");
        w.first_form.submit.emit_clicked();
        let error = w.first_form.error.clone();
        wait_until("the first-time error", move || error.get_visible());
        capture("virtual-deck-menu-first-time");
        w.first_form.new.set_text(PASSWORD);
        w.first_form.repeat.set_text(PASSWORD);
        w.first_form.submit.emit_clicked();
        wait_phase(&panel, Phase::On);
        let _phone = ViewerClient::connect(port, ANDROID_CHROME);
        let _mac = ViewerClient::connect(port, MAC_SAFARI);
        let heading = w.viewers_heading.clone();
        wait_until("two viewers", move || heading.text() == "Viewers (2)");
        capture("virtual-deck-menu-running");
        menu.button().popdown();
        settings.set_gtk_application_prefer_dark_theme(true);
        rendered_texture(&pair.window, &pair.window.child().unwrap())
            .save_to_png(directory.join("virtual-deck-window-dark.png"))
            .unwrap();
        settings.set_gtk_application_prefer_dark_theme(original);
        let _ = virtual_deck_ui::take_opened();
    });
}
