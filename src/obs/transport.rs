use super::ObsConfig;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::{
    client::client_with_config, protocol::WebSocketConfig, HandshakeError, Message, WebSocket,
};

pub(super) const TIMEOUT: Duration = Duration::from_secs(3);

/// The deadline is checked before every underlying I/O, including partial frames.
pub(super) struct DeadlineStream {
    stream: TcpStream,
    deadline: Instant,
}

impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stream
            .set_read_timeout(Some(remaining(self.deadline)?))?;
        self.stream.read(bytes)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream
            .set_write_timeout(Some(remaining(self.deadline)?))?;
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        remaining(self.deadline)?;
        self.stream.flush()
    }
}

#[derive(Deserialize)]
struct Frame<T> {
    op: u8,
    d: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hello {
    rpc_version: u32,
    authentication: Option<Authentication>,
}

#[derive(Deserialize)]
struct Authentication {
    salt: String,
    challenge: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identified {
    negotiated_rpc_version: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Identify {
    rpc_version: u32,
    event_subscriptions: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    authentication: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response<T> {
    request_type: String,
    request_id: String,
    request_status: RequestStatus,
    response_data: Option<T>,
}

#[derive(Deserialize)]
struct RequestStatus {
    result: bool,
    code: u16,
}

pub(super) struct Session {
    socket: WebSocket<DeadlineStream>,
    sequence: u32,
}

impl Session {
    pub(super) fn connect(config: &ObsConfig) -> io::Result<Self> {
        config.validate()?;
        let deadline = Instant::now() + TIMEOUT;
        let stream = TcpStream::connect_timeout(&config.address()?, remaining(deadline)?)?;
        stream.set_nodelay(true)?;
        let limits = WebSocketConfig::default()
            .max_message_size(Some(1_048_576))
            .max_frame_size(Some(1_048_576))
            .max_write_buffer_size(1_048_576);
        let (socket, _) = client_with_config(
            config.endpoint.as_str(),
            DeadlineStream { stream, deadline },
            Some(limits),
        )
        .map_err(|error| match error {
            HandshakeError::Failure(error) => socket_error(error),
            // A blocking TcpStream returns WouldBlock only on its I/O timeout.
            HandshakeError::Interrupted(_) => io::ErrorKind::TimedOut.into(),
        })?;
        let mut session = Self {
            socket,
            sequence: 0,
        };
        let hello: Hello = session.receive(0)?;
        if hello.rpc_version == 0 {
            return Err(protocol_error("OBS RPC version is unsupported"));
        }
        let authentication = match hello.authentication {
            None => None,
            Some(auth) => {
                if auth.salt.is_empty() || auth.challenge.is_empty() {
                    return Err(protocol_error("OBS authentication challenge is invalid"));
                }
                let mut hash = Sha256::new();
                hash.update(config.password.as_bytes());
                hash.update(auth.salt.as_bytes());
                let secret = STANDARD.encode(hash.finalize());
                let mut hash = Sha256::new();
                hash.update(secret.as_bytes());
                hash.update(auth.challenge.as_bytes());
                Some(STANDARD.encode(hash.finalize()))
            }
        };
        session.send(
            1,
            Identify {
                rpc_version: 1,
                event_subscriptions: 0,
                authentication,
            },
        )?;
        let identified: Identified = session.receive(2)?;
        if identified.negotiated_rpc_version != 1 {
            return Err(protocol_error("OBS RPC version negotiation failed"));
        }
        Ok(session)
    }

    pub(super) fn request<T: DeserializeOwned>(
        &mut self,
        request_type: &str,
        data: impl Serialize,
    ) -> io::Result<Option<T>> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| protocol_error("OBS request sequence exhausted"))?;
        let id = self.sequence.to_string();
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a, T> {
            request_type: &'a str,
            request_id: &'a str,
            request_data: T,
        }
        self.send(
            6,
            Request {
                request_type,
                request_id: &id,
                request_data: data,
            },
        )?;
        let response: Response<T> = self.receive(7)?;
        if response.request_id != id || response.request_type != request_type {
            return Err(protocol_error("OBS response does not match the request"));
        }
        let status = response.request_status;
        if status.result != (status.code == 100) {
            return Err(protocol_error("OBS response status is inconsistent"));
        }
        if !status.result {
            // Never expose untrusted server comments, which may echo credentials.
            return Err(io::Error::other(format!(
                "OBS rejected request (code {})",
                status.code
            )));
        }
        Ok(response.response_data)
    }

    pub(super) fn query<T: DeserializeOwned>(
        &mut self,
        request_type: &str,
        data: impl Serialize,
    ) -> io::Result<T> {
        self.request(request_type, data)?
            .ok_or_else(|| protocol_error("OBS response data is missing"))
    }

    fn send(&mut self, op: u8, d: impl Serialize) -> io::Result<()> {
        #[derive(Serialize)]
        struct Outgoing<T> {
            op: u8,
            d: T,
        }
        let text = serde_json::to_string(&Outgoing { op, d })
            .map_err(|_| protocol_error("OBS request encoding failed"))?;
        self.socket
            .send(Message::Text(text.into()))
            .map_err(socket_error)
    }

    fn receive<T: DeserializeOwned>(&mut self, expected: u8) -> io::Result<T> {
        // Control frames have a finite budget as well as the I/O deadline.
        for _ in 0..64 {
            remaining(self.socket.get_ref().deadline)?;
            match self.socket.read().map_err(socket_error)? {
                Message::Text(text) => {
                    let frame: Frame<T> = serde_json::from_str(&text)
                        .map_err(|_| protocol_error("malformed OBS message"))?;
                    if frame.op != expected {
                        return Err(protocol_error("unexpected OBS opcode"));
                    }
                    return Ok(frame.d);
                }
                Message::Ping(_) => self.socket.flush().map_err(socket_error)?,
                Message::Pong(_) => {}
                Message::Close(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "OBS closed the connection",
                    ));
                }
                Message::Binary(_) | Message::Frame(_) => {
                    return Err(protocol_error("OBS sent a non-JSON message"));
                }
            }
        }
        Err(protocol_error("OBS control-frame limit exceeded"))
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "OBS operation timed out"))
}

pub(super) fn protocol_error(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn socket_error(error: tungstenite::Error) -> io::Error {
    match error {
        tungstenite::Error::Io(error) => match error.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {
                io::Error::new(io::ErrorKind::TimedOut, "OBS operation timed out")
            }
            _ => io::Error::new(error.kind(), "OBS network I/O failed"),
        },
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "OBS closed the connection",
        ),
        _ => protocol_error("OBS WebSocket protocol error"),
    }
}
