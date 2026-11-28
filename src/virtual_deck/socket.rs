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

pub(super) struct Socket {
    tcp: TcpStream,
    deadline: Option<Instant>,
}

impl Socket {
    /// Takes over a new connection: no Nagle delay, each write done within
    /// `write_timeout`, the whole request read by `deadline`.
    pub fn new(tcp: TcpStream, deadline: Instant, write_timeout: Duration) -> io::Result<Self> {
        let socket = Self {
            tcp,
            deadline: Some(deadline),
        };
        let _ = socket.tcp.set_nodelay(true);
        socket.tcp.set_write_timeout(Some(write_timeout))?;
        Ok(socket)
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

/// The server keeps a second handle on every connection, to cut it short on
/// stop, so closing this one alone would leave the connection open until the
/// next accept: every way out of a connection shuts it down, so the client
/// sees the end at once.
impl Drop for Socket {
    fn drop(&mut self) {
        let _ = self.tcp.shutdown(Shutdown::Both);
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

/// Ends a connection cleanly: no more writes, then reads what the client
/// still sends (bounded), so closing never turns into a reset that would
/// discard the last response before the client reads it.
pub(super) fn finish(mut socket: Socket) {
    let _ = socket.flush();
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

    const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

    #[test]
    fn a_silent_client_cannot_outlast_the_request_deadline() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server_side, _) = listener.accept().unwrap();
        let started = Instant::now();
        let mut socket = Socket::new(
            server_side,
            started + Duration::from_millis(50),
            WRITE_TIMEOUT,
        )
        .unwrap();
        let error = socket.read(&mut [0u8; 16]).unwrap_err();
        assert!(
            matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
            "{error}"
        );
        let error = socket.read(&mut [0u8; 16]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut, "the deadline has passed");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn dropping_a_socket_ends_the_connection_even_with_another_handle_open() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client.set_read_timeout(Some(WRITE_TIMEOUT)).unwrap();
        let (server_side, _) = listener.accept().unwrap();
        let _watch = server_side.try_clone().unwrap();
        drop(Socket::new(server_side, Instant::now() + WRITE_TIMEOUT, WRITE_TIMEOUT).unwrap());
        assert_eq!(
            client.read(&mut [0u8; 1]).unwrap(),
            0,
            "the client sees the end"
        );
    }
}
