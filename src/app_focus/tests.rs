use super::*;
use std::os::unix::net::UnixListener;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{self, Receiver, Sender};

const WAIT: Duration = Duration::from_secs(3);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
type Update = Result<Option<FocusedApplication>, String>;

enum Reply {
    Json(String),
    Hold(Receiver<()>),
}

struct Source {
    directory: PathBuf,
    responses: Sender<Reply>,
    queries: Receiver<()>,
    subscribed: Receiver<UnixStream>,
    server: Option<JoinHandle<()>>,
    updates: Receiver<Update>,
    observer: Option<FocusObserver>,
    events: Option<UnixStream>,
}

impl Source {
    fn new(initial: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sdeck-focus-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let requests = UnixListener::bind(directory.join(".socket.sock")).unwrap();
        let events = UnixListener::bind(directory.join(".socket2.sock")).unwrap();
        events.set_nonblocking(true).unwrap();
        let (subscription_tx, subscribed) = mpsc::channel();
        let (responses, response_rx) = mpsc::channel::<Reply>();
        let (query_tx, queries) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut events = Some(events);
            for incoming in requests.incoming() {
                let mut socket = incoming.unwrap();
                socket.set_read_timeout(Some(WAIT)).unwrap();
                socket.set_write_timeout(Some(WAIT)).unwrap();
                let mut command = [0; 14];
                if socket.read_exact(&mut command).is_err() {
                    break;
                }
                assert_eq!(&command, b"j/activewindow");
                if let Some(listener) = events.take() {
                    // The event connection must already be queued when the
                    // first query arrives. Nonblocking accept proves ordering.
                    let (event_socket, _) = listener.accept().unwrap();
                    subscription_tx.send(event_socket).unwrap();
                }
                query_tx.send(()).unwrap();
                match response_rx.recv_timeout(WAIT).unwrap() {
                    Reply::Json(response) => socket.write_all(response.as_bytes()).unwrap(),
                    Reply::Hold(release) => release.recv_timeout(WAIT).unwrap(),
                }
                // Dropping the request socket terminates the JSON response.
            }
        });
        let (update_tx, updates) = mpsc::channel();
        responses.send(Reply::Json(initial.into())).unwrap();
        let observer = FocusObserver::start_at(
            &directory.join(".socket.sock"),
            &directory.join(".socket2.sock"),
            move |update| update_tx.send(update).unwrap(),
        )
        .unwrap();
        Self {
            directory,
            responses,
            queries,
            subscribed,
            server: Some(server),
            updates,
            observer: Some(observer),
            events: None,
        }
    }

    fn subscription(&mut self) {
        self.events = Some(self.subscribed.recv_timeout(WAIT).unwrap());
        self.queries.recv_timeout(WAIT).unwrap();
    }

    fn update(&self) -> Update {
        self.updates.recv_timeout(WAIT).unwrap()
    }

    fn event(&mut self, event: &str, response: &str) {
        self.responses.send(Reply::Json(response.into())).unwrap();
        self.events
            .as_mut()
            .unwrap()
            .write_all(format!("{event}\n").as_bytes())
            .unwrap();
        self.queries.recv_timeout(WAIT).unwrap();
    }

    fn disconnect(&mut self) {
        self.events
            .take()
            .unwrap()
            .shutdown(Shutdown::Both)
            .unwrap();
        assert!(self.update().is_err());
        self.observer.as_mut().unwrap().stop();
        assert!(self.observer.as_ref().unwrap().thread.is_none());
        assert!(self.updates.try_recv().is_err());
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.observer.take();
        self.events.take();
        // Wake a server blocked in accept. No polling, detached thread, or sleep.
        drop(UnixStream::connect(self.directory.join(".socket.sock")).unwrap());
        self.server.take().unwrap().join().unwrap();
        std::fs::remove_dir_all(&self.directory).unwrap();
        assert!(!self.directory.exists());
    }
}

fn window(address: &str, class: &str, pid: u32) -> String {
    serde_json::json!({"address": address, "class": class, "pid": pid, "title": "ignored"})
        .to_string()
}

fn external(address: &str, class: &str) -> String {
    window(address, class, u32::MAX)
}

fn identity(class: &str) -> Update {
    Ok(Some(FocusedApplication {
        window_class: class.into(),
        executable: None,
    }))
}

#[test]
fn initial_query_when_event_subscription_is_connected() {
    // Given a real request/event socket pair with a queued initial response.
    let mut source = Source::new(&external("0xa", "Terminal"));
    // When startup subscribes, the request is observable without any event.
    source.subscription();
    // Then it emits the focused external identity.
    assert_eq!(source.update(), identity("Terminal"));
    source.disconnect();
}

#[test]
fn executable_basename_when_external_pid_is_resolvable() {
    // Given a live external process (the test's parent).
    let parent = std::fs::read_to_string("/proc/self/status").unwrap();
    let pid = parent
        .lines()
        .find_map(|line| line.strip_prefix("PPid:\t"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let basename = std::fs::read_link(format!("/proc/{pid}/exe"))
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let mut source = Source::new(&window("0xa", "External", pid));
    // When startup queries that external window.
    source.subscription();
    // Then executable is resolved to a basename, not the full path.
    assert_eq!(
        source.update(),
        Ok(Some(FocusedApplication {
            window_class: "External".into(),
            executable: Some(basename),
        }))
    );
    source.disconnect();
}

#[test]
fn own_focus_when_no_external_context_exists() {
    // Given an own-process focused window at startup.
    let mut source = Source::new(&window("0xa", "Config", std::process::id()));
    // When it is queried.
    source.subscription();
    // Then there is no external application context.
    assert_eq!(source.update(), Ok(None));
    source.disconnect();
}

#[test]
fn external_identity_when_focus_event_arrives() {
    // Given an initially empty compositor.
    let mut source = Source::new("{}");
    source.subscription();
    assert_eq!(source.update(), Ok(None));
    // When activewindowv2 arrives; its payload is not the query identity.
    source.event("activewindowv2>>stale", &external("0xa", "Browser"));
    // Then actual current JSON defines the context.
    assert_eq!(source.update(), identity("Browser"));
    source.disconnect();
}

#[test]
fn retained_context_when_own_configuration_is_focused() {
    // Given a focused external application.
    let mut source = Source::new(&external("0xa", "Browser"));
    source.subscription();
    assert_eq!(source.update(), identity("Browser"));
    // When the own process becomes focused.
    source.event(
        "activewindowv2>>b",
        &window("0xb", "Config", std::process::id()),
    );
    // Then no replacement context is emitted before the disconnect receipt.
    source.disconnect();
}

#[test]
fn duplicate_identity_when_latest_window_address_changes() {
    // Given two windows of the same app.
    let mut source = Source::new(&external("0xa", "Browser"));
    source.subscription();
    assert_eq!(source.update(), identity("Browser"));
    // When focus changes to the second window, then its close event arrives
    // while configuration is focused. Closure must use the latest address.
    source.event("activewindowv2>>b", &external("0xb", "Browser"));
    source.event(
        "closewindow>>b",
        &window("0xc", "Config", std::process::id()),
    );
    // Then only the closure emits; duplicate app focus did not emit.
    assert_eq!(source.update(), Ok(None));
    source.disconnect();
}

#[test]
fn retained_window_closure_when_configuration_is_focused() {
    // Given a retained external context while own configuration has focus.
    let mut source = Source::new(&external("0xa", "Browser"));
    source.subscription();
    assert_eq!(source.update(), identity("Browser"));
    source.event(
        "activewindowv2>>b",
        &window("0xb", "Config", std::process::id()),
    );
    // When the retained window closes.
    source.event(
        "closewindow>>a",
        &window("0xb", "Config", std::process::id()),
    );
    // Then context clears even though configuration still has focus.
    assert_eq!(source.update(), Ok(None));
    source.disconnect();
}

#[test]
fn actual_external_context_when_retained_window_closes() {
    // Given an external window whose close event races another app's focus.
    let mut source = Source::new(&external("0xa", "Browser"));
    source.subscription();
    assert_eq!(source.update(), identity("Browser"));
    // When closure is delivered and JSON reports another external window.
    source.event("closewindow>>a", &external("0xb", "Editor"));
    // Then the actual focused app replaces the retained app.
    assert_eq!(source.update(), identity("Editor"));
    source.disconnect();
}

#[test]
fn empty_context_when_external_window_loses_focus() {
    // Given external focus.
    let mut source = Source::new(&external("0xa", "Browser"));
    source.subscription();
    assert_eq!(source.update(), identity("Browser"));
    // When current activewindow JSON is empty.
    source.event("activewindowv2>>", "{}");
    // Then no app is distinct from unavailable.
    assert_eq!(source.update(), Ok(None));
    source.disconnect();
}

#[test]
fn error_and_recovery_when_json_is_unavailable() {
    // Given invalid initial JSON.
    let mut source = Source::new("not json");
    source.subscription();
    assert!(source.update().is_err());
    // When a focus event provides a valid empty object.
    source.event("activewindowv2>>", "{}");
    // Then availability is restored as explicit no-app.
    assert_eq!(source.update(), Ok(None));
    source.disconnect();
}

#[test]
fn idempotent_stop_when_event_read_is_blocked() {
    // Given an observer waiting for events after its initial callback.
    let mut source = Source::new("{}");
    source.subscription();
    assert_eq!(source.update(), Ok(None));
    let mut observer = source.observer.take().unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    // When stop is called twice on an owned background thread.
    let stopper = thread::spawn(move || {
        observer.stop();
        observer.stop();
        assert!(observer.thread.is_none());
        assert!(observer.event_socket.is_none());
        done_tx.send(()).unwrap();
    });
    // Then it joins within a bounded signal and emits no spurious error.
    done_rx.recv_timeout(WAIT).unwrap();
    stopper.join().unwrap();
    assert!(source.updates.try_recv().is_err());
}

#[test]
fn drop_when_event_read_is_blocked() {
    // Given a live event subscription.
    let mut source = Source::new("{}");
    source.subscription();
    assert_eq!(source.update(), Ok(None));
    let observer = source.observer.take().unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    // When the observer is dropped.
    let dropper = thread::spawn(move || {
        drop(observer);
        done_tx.send(()).unwrap();
    });
    // Then its callback channel closes after its thread has joined.
    done_rx.recv_timeout(WAIT).unwrap();
    dropper.join().unwrap();
    assert!(matches!(
        source.updates.recv_timeout(WAIT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[test]
fn bounded_stop_when_request_read_is_blocked() {
    // Given an observer and a server holding the next JSON response open.
    let mut source = Source::new("{}");
    source.subscription();
    assert_eq!(source.update(), Ok(None));
    let (release_tx, release_rx) = mpsc::channel();
    source.responses.send(Reply::Hold(release_rx)).unwrap();
    source
        .events
        .as_mut()
        .unwrap()
        .write_all(b"activewindowv2>>a\n")
        .unwrap();
    source.queries.recv_timeout(WAIT).unwrap();
    let mut observer = source.observer.take().unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    // When stop runs while the request is blocked.
    let stopper = thread::spawn(move || {
        observer.stop();
        done_tx.send(()).unwrap();
    });
    // Then the request timeout bounds the join without a spurious error.
    done_rx.recv_timeout(WAIT).unwrap();
    stopper.join().unwrap();
    release_tx.send(()).unwrap();
    assert!(source.updates.try_recv().is_err());
}

#[test]
fn unavailable_when_request_socket_disconnects_without_json() {
    // Given a request socket that closes with an empty response.
    let mut source = Source::new("");
    // When the initial query reads that response.
    source.subscription();
    // Then unavailable is not represented as no-app.
    assert!(source.update().is_err());
    source.disconnect();
}

#[test]
fn closed_context_stays_cleared_when_query_recovers_to_own_focus() {
    // Given a retained external application.
    let mut source = Source::new(&external("0xa", "Browser"));
    source.subscription();
    assert_eq!(source.update(), identity("Browser"));
    // When closure cannot query, then the source recovers to own focus.
    source.event("closewindow>>a", "invalid");
    assert!(source.update().is_err());
    source.event(
        "activewindowv2>>b",
        &window("0xb", "Config", std::process::id()),
    );
    // Then the explicitly closed context never returns.
    assert_eq!(source.update(), Ok(None));
    source.disconnect();
}

#[test]
fn unrelated_events_when_external_context_is_retained() {
    // Given a focused external window.
    let mut source = Source::new(&external("0xa", "Browser"));
    source.subscription();
    assert_eq!(source.update(), identity("Browser"));
    // When legacy title and unrelated closure events precede a focus event.
    source
        .events
        .as_mut()
        .unwrap()
        .write_all(b"activewindow>>Other,title\nclosewindow>>b\n")
        .unwrap();
    source.event("activewindowv2>>c", &external("0xc", "Editor"));
    // Then only the actual focus event queries and emits the next identity.
    assert_eq!(source.update(), identity("Editor"));
    source.disconnect();
}
