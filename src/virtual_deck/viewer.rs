//! One connected page: its WebSocket loop, its outgoing queue and the JSON
//! protocol shared with `page/index.html`.
use super::{access::SessionId, server::Shared, socket, DeckFrame, DeckInput, KEYS};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use serde_json::json;
use std::{
    io,
    net::IpAddr,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError},
    },
    time::{Duration, Instant, SystemTime},
};
use tungstenite::{
    protocol::{frame::coding::CloseCode, CloseFrame, Role, WebSocketConfig},
    Bytes, Message, Utf8Bytes, WebSocket,
};

/// Largest message a page may send: a key message is about 40 bytes.
const MAX_MESSAGE: usize = 1024;
/// Frames waiting for one viewer; a viewer this far behind gets a fresh snapshot.
pub(super) const OUTBOX: usize = 256;
/// How long the loop waits for page input before it serves the outgoing queue.
pub(super) const POLL: Duration = Duration::from_millis(20);
const PING_EVERY: Duration = Duration::from_secs(15);
/// A page that sends nothing, not even a pong, for this long is gone.
const SILENCE_LIMIT: Duration = Duration::from_secs(45);
const MAX_WRITE_BUFFER: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ByeReason {
    Disconnected = 1,
    PasswordChanged = 2,
    Stopped = 3,
}

impl ByeReason {
    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Disconnected),
            2 => Some(Self::PasswordChanged),
            3 => Some(Self::Stopped),
            _ => None,
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::PasswordChanged => "password-changed",
            Self::Stopped => "stopped",
        }
    }
}

/// The registry's view of a viewer; its thread owns the socket.
pub(super) struct ViewerHandle {
    pub id: u64,
    pub address: IpAddr,
    pub browser: String,
    pub since: SystemTime,
    pub session: SessionId,
    outbox: SyncSender<Utf8Bytes>,
    resync: AtomicBool,
    closing: AtomicU8,
}

impl ViewerHandle {
    pub fn new(
        id: u64,
        address: IpAddr,
        browser: String,
        session: SessionId,
        outbox: SyncSender<Utf8Bytes>,
    ) -> Self {
        Self {
            id,
            address,
            browser,
            since: SystemTime::now(),
            session,
            outbox,
            resync: AtomicBool::new(false),
            closing: AtomicU8::new(0),
        }
    }

    /// Queues a frame; a full queue drops it and asks for a snapshot instead.
    pub fn offer(&self, frame: &Utf8Bytes) {
        if let Err(TrySendError::Full(_)) = self.outbox.try_send(frame.clone()) {
            self.resync.store(true, Ordering::SeqCst);
        }
    }

    /// Asks the viewer's thread to say bye and close; the first reason wins.
    pub fn close(&self, reason: ByeReason) {
        let _ = self
            .closing
            .compare_exchange(0, reason as u8, Ordering::SeqCst, Ordering::SeqCst);
    }

    fn closing(&self) -> Option<ByeReason> {
        ByeReason::from_code(self.closing.load(Ordering::SeqCst))
    }
}

/// `{"type":"key","key":n,"jpeg":"<base64>"}` and the other server frames.
pub(super) fn frame_text(frame: &DeckFrame) -> Utf8Bytes {
    let value = match frame {
        DeckFrame::Key { key, jpeg } => {
            json!({"type": "key", "key": key, "jpeg": STANDARD.encode(jpeg)})
        }
        DeckFrame::Background { jpeg } => {
            json!({"type": "background", "jpeg": STANDARD.encode(jpeg)})
        }
        DeckFrame::Brightness(percent) => {
            json!({"type": "brightness", "percent": (*percent).min(100)})
        }
        DeckFrame::Reset => json!({"type": "reset"}),
    };
    Utf8Bytes::from(value.to_string())
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum PageMessage {
    Key { key: u8, pressed: bool },
}

/// A valid key message from the page; anything else is ignored.
fn parse_input(text: &str) -> Option<DeckInput> {
    match serde_json::from_str(text).ok()? {
        PageMessage::Key { key, pressed } => (1..=KEYS as u8)
            .contains(&key)
            .then_some(DeckInput { key, pressed }),
    }
}

pub(super) fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE))
        .max_write_buffer_size(MAX_WRITE_BUFFER)
}

pub(super) fn upgrade(stream: socket::Tls, leftover: Vec<u8>) -> WebSocket<socket::Tls> {
    WebSocket::from_partially_read(stream, leftover, Role::Server, Some(websocket_config()))
}

/// Serves one viewer until it leaves, fails, or is asked to close; lets go
/// of the keys it held, says bye when asked to close, and returns the
/// connection for the caller to finish.
pub(super) fn run(
    mut socket: WebSocket<socket::Tls>,
    handle: &ViewerHandle,
    outbox: &Receiver<Utf8Bytes>,
    shared: &Shared,
) -> socket::Tls {
    let mut held = [false; KEYS];
    let reason = serve(&mut socket, handle, outbox, shared, &mut held);
    for (index, pressed) in held.iter_mut().enumerate() {
        shared.key(pressed, index, false);
    }
    if let Some(reason) = reason {
        let bye = json!({"type": "bye", "reason": reason.text()}).to_string();
        let _ = socket.send(Message::text(bye));
        let code = match reason {
            ByeReason::Stopped => CloseCode::Away,
            ByeReason::Disconnected | ByeReason::PasswordChanged => CloseCode::Policy,
        };
        let _ = socket.close(Some(CloseFrame {
            code,
            reason: Utf8Bytes::from_static(""),
        }));
    }
    let _ = socket.flush();
    socket.into_inner()
}

/// Returns the reason to say bye with, or None when the page went away.
fn serve(
    socket: &mut WebSocket<socket::Tls>,
    handle: &ViewerHandle,
    outbox: &Receiver<Utf8Bytes>,
    shared: &Shared,
    held: &mut [bool; KEYS],
) -> Option<ByeReason> {
    if send_snapshot(socket, shared).is_err() {
        return None;
    }
    let mut heard = Instant::now();
    let mut pinged = Instant::now();
    loop {
        if let Some(reason) = handle.closing() {
            return Some(reason);
        }
        match socket.read() {
            Ok(message) => {
                heard = Instant::now();
                match message {
                    Message::Text(text) => {
                        if let Some(input) = parse_input(text.as_str()) {
                            let index = usize::from(input.key - 1);
                            shared.key(&mut held[index], index, input.pressed);
                        }
                    }
                    Message::Close(_) => return None,
                    _ => {}
                }
            }
            Err(tungstenite::Error::Io(error)) if waiting(&error) => {}
            Err(_) => return None,
        }
        if handle.resync.swap(false, Ordering::SeqCst) {
            while outbox.try_recv().is_ok() {}
            if send_snapshot(socket, shared).is_err() {
                return None;
            }
        }
        loop {
            match outbox.try_recv() {
                Ok(text) => {
                    if socket.send(Message::Text(text)).is_err() {
                        return None;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return None,
            }
        }
        if heard.elapsed() > SILENCE_LIMIT {
            return None;
        }
        if pinged.elapsed() > PING_EVERY {
            pinged = Instant::now();
            if socket.send(Message::Ping(Bytes::new())).is_err() {
                return None;
            }
        }
    }
}

fn send_snapshot(socket: &mut WebSocket<socket::Tls>, shared: &Shared) -> tungstenite::Result<()> {
    for frame in shared.snapshot() {
        socket.write(Message::Text(frame_text(&frame)))?;
    }
    socket.flush()
}

/// A read that only ran into the poll timeout.
fn waiting(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_well_formed_key_messages_become_input() {
        assert_eq!(
            parse_input(r#"{"type":"key","key":15,"pressed":false}"#),
            Some(DeckInput {
                key: 15,
                pressed: false
            })
        );
        for text in [
            r#"{"type":"key","key":0,"pressed":true}"#,
            r#"{"type":"key","key":16,"pressed":true}"#,
            r#"{"type":"key","key":300,"pressed":true}"#,
            r#"{"type":"key","key":"3","pressed":true}"#,
            r#"{"type":"key","key":3}"#,
            r#"{"type":"dial","key":3,"pressed":true}"#,
            "not json",
        ] {
            assert_eq!(parse_input(text), None, "{text}");
        }
    }

    #[test]
    fn frames_follow_the_page_protocol() {
        let value = |frame| -> serde_json::Value {
            serde_json::from_str(frame_text(&frame).as_str()).unwrap()
        };
        assert_eq!(
            value(DeckFrame::Key {
                key: 2,
                jpeg: vec![1, 2, 3]
            }),
            json!({"type": "key", "key": 2, "jpeg": "AQID"})
        );
        assert_eq!(
            value(DeckFrame::Background { jpeg: vec![9] }),
            json!({"type": "background", "jpeg": "CQ=="})
        );
        assert_eq!(
            value(DeckFrame::Brightness(140)),
            json!({"type": "brightness", "percent": 100})
        );
        assert_eq!(value(DeckFrame::Reset), json!({"type": "reset"}));
    }
}
