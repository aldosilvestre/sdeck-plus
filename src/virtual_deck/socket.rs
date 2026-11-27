//! The TCP side of one connection: a whole-request deadline while the HTTP
//! request is read, then short read timeouts while it serves a viewer.
use std::{
    io::{self, Read, Write},
    net::{Shutdown, TcpStream},
    time::{Duration, Instant},
};

/// Longest a closing connection waits for the client to finish sending.
const CLOSE_DRAIN: Duration = Duration::from_secs(1);
const CLOSE_DRAIN_BYTES: usize = 64 * 1024;

pub(super) type Tls = rustls::StreamOwned<rustls::ServerConnection, Socket>;

pub(super) struct Socket {
    tcp: TcpStream,
    deadline: Option<Instant>,
}

impl Socket {
    pub fn new(tcp: TcpStream, deadline: Instant) -> Self {
        Self {
            tcp,
            deadline: Some(deadline),
        }
    }

    /// No overall deadline any more; every read waits at most `every`.
    pub fn poll_every(&mut self, every: Duration) -> io::Result<()> {
        self.deadline = None;
        self.tcp.set_read_timeout(Some(every))
    }
}

impl Read for Socket {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if let Some(deadline) = self.deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the request took too long",
                ));
            }
            self.tcp.set_read_timeout(Some(left))?;
        }
        self.tcp.read(buffer)
    }
}

impl Write for Socket {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.tcp.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

/// Ends a connection cleanly: TLS close_notify, no more writes, then reads
/// what the client still sends (bounded), so closing never turns into a reset
/// that would discard the last response before the client reads it.
pub(super) fn finish(mut stream: Tls) {
    stream.conn.send_close_notify();
    let _ = stream.flush();
    let socket = &mut stream.sock;
    let _ = socket.tcp.shutdown(Shutdown::Write);
    socket.deadline = Some(Instant::now() + CLOSE_DRAIN);
    let mut sink = [0u8; 4096];
    let mut drained = 0;
    while drained < CLOSE_DRAIN_BYTES {
        match socket.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(read) => drained += read,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Socket;
    use std::{
        io::{ErrorKind, Read},
        net::{Ipv4Addr, TcpListener, TcpStream},
        time::{Duration, Instant},
    };

    #[test]
    fn a_silent_client_cannot_outlast_the_request_deadline() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server_side, _) = listener.accept().unwrap();
        let started = Instant::now();
        let mut socket = Socket::new(server_side, started + Duration::from_millis(50));
        let error = socket.read(&mut [0u8; 16]).unwrap_err();
        assert!(
            matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
            "{error}"
        );
        let error = socket.read(&mut [0u8; 16]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut, "the deadline has passed");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
