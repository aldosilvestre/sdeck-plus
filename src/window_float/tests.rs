use super::*;
use std::os::unix::net::UnixListener;
use std::sync::mpsc;

const ME: Identity<'static> = Identity {
    pid: 4242,
    class: "dev.streamdeck.EventMonitor",
    title: "sdeck-plus",
};

fn client(address: &str, pid: u32, class: &str, title: &str, floating: bool) -> String {
    format!(
        r#"{{"address":"{address}","mapped":true,"floating":{floating},"class":"{class}","title":"{title}","pid":{pid},"monitor":1}}"#
    )
}

fn clients(items: &[String]) -> String {
    format!("[{}]", items.join(","))
}

const MONITORS: &str = r#"[{"id":0,"x":0,"y":0,"width":800,"height":600,"scale":1.0,"reserved":[0,0,0,0],"transform":0},
{"id":1,"x":1920,"y":0,"width":3840,"height":2160,"scale":2.0,"reserved":[0,30,0,0],"transform":0}]"#;

#[test]
fn socket_is_none_without_signature() {
    assert_eq!(command_socket(Some(OsStr::new("/run/user/1")), None), None);
    assert_eq!(
        command_socket(Some(OsStr::new("/run/user/1")), Some(OsStr::new(""))),
        None
    );
}

#[test]
fn socket_path_uses_runtime_dir_and_signature() {
    assert_eq!(
        command_socket(Some(OsStr::new("/run/user/1")), Some(OsStr::new("abc_1"))),
        Some(PathBuf::from("/run/user/1/hypr/abc_1/.socket.sock"))
    );
}

#[test]
fn finds_own_tiled_window_by_pid_class_and_title() {
    let json = clients(&[
        client("0x1", 1, "dev.streamdeck.EventMonitor", "sdeck-plus", false),
        client("0x2", 4242, "kitty", "sdeck-plus", false),
        client("0x3", 4242, "dev.streamdeck.EventMonitor", "Help", false),
        client(
            "0x4",
            4242,
            "dev.streamdeck.EventMonitor",
            "sdeck-plus",
            false,
        ),
    ]);
    assert_eq!(
        find_window(&json, ME),
        Ok(Lookup::Tiled {
            address: "0x4".into(),
            monitor: 1
        })
    );
}

#[test]
fn already_floating_window_is_left_alone() {
    let json = clients(&[client("0x4", 4242, ME.class, ME.title, true)]);
    assert_eq!(find_window(&json, ME), Ok(Lookup::AlreadyFloating));
}

#[test]
fn unlisted_or_unmapped_window_is_missing() {
    let unmapped = r#"[{"address":"0x4","mapped":false,"floating":false,"class":"dev.streamdeck.EventMonitor","title":"sdeck-plus","pid":4242,"monitor":0}]"#;
    assert_eq!(find_window(unmapped, ME), Ok(Lookup::Missing));
    assert_eq!(find_window("[]", ME), Ok(Lookup::Missing));
}

#[test]
fn malformed_clients_json_is_an_error() {
    assert!(find_window("nope", ME).is_err());
}

#[test]
fn batch_text_is_exact() {
    let area = Area {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    assert_eq!(
        batch("0xabc", area, 1120, 700),
        "[[BATCH]]/dispatch setfloating address:0xabc;\
         /dispatch resizewindowpixel exact 1120 700,address:0xabc;\
         /dispatch movewindowpixel exact 400 190,address:0xabc"
    );
}

#[test]
fn batch_clamps_to_a_smaller_area_and_centers_inside_it() {
    let area = Area {
        x: 100,
        y: 30,
        width: 800,
        height: 570,
    };
    assert!(batch("0x1", area, 1120, 700).contains("exact 800 570,address:0x1;"));
    assert!(batch("0x1", area, 1120, 700).ends_with("exact 100 30,address:0x1"));
}

#[test]
fn usable_area_accounts_for_scale_and_reserved_space() {
    assert_eq!(
        usable_area(MONITORS, 1),
        Ok(Area {
            x: 1920,
            y: 30,
            width: 1920,
            height: 1050
        })
    );
}

#[test]
fn usable_area_swaps_sides_for_rotated_monitors() {
    let json = r#"[{"id":0,"width":1080,"height":1920,"transform":1}]"#;
    let area = usable_area(json, 0).unwrap();
    assert_eq!((area.width, area.height), (1920, 1080));
}

#[test]
fn unknown_monitor_is_an_error() {
    assert!(usable_area(MONITORS, 9).is_err());
}

#[test]
fn batch_replies_are_judged_by_ok() {
    assert!(batch_succeeded("okokok"));
    assert!(!batch_succeeded("okinvalid dispatcher"));
    assert!(!batch_succeeded(""));
}

struct Fake {
    dir: PathBuf,
    requests: mpsc::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Fake {
    /// Answers each connection with the next canned reply, then stops.
    fn start(replies: Vec<&'static str>) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sdeck-float-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let (tx, requests) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = [0u8; 4096];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buffer[..read]).into_owned());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        Self {
            dir,
            requests,
            thread: Some(thread),
        }
    }

    fn socket(&self) -> PathBuf {
        self.dir.join(".socket.sock")
    }

    fn finish(mut self) -> Vec<String> {
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
        let seen = self.requests.try_iter().collect();
        let _ = std::fs::remove_dir_all(&self.dir);
        seen
    }
}

const FAST: Duration = Duration::from_millis(5);

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

#[test]
fn fake_socket_receives_clients_monitors_then_the_batch() {
    let tiled = leak(clients(&[client("0x9", 4242, ME.class, ME.title, false)]));
    let monitors = r#"[{"id":1,"x":0,"y":0,"width":1920,"height":1080}]"#;
    let fake = Fake::start(vec![tiled, monitors, "okokok"]);
    let outcome = float_window(
        &fake.socket(),
        ME,
        (1120, 700),
        Duration::from_secs(3),
        FAST,
    );
    let seen = fake.finish();
    assert!(matches!(outcome, Ok(Outcome::Floated(_))), "{outcome:?}");
    assert_eq!(
        seen,
        [
            "j/clients",
            "j/monitors",
            "[[BATCH]]/dispatch setfloating address:0x9;\
             /dispatch resizewindowpixel exact 1120 700,address:0x9;\
             /dispatch movewindowpixel exact 400 190,address:0x9"
        ]
    );
}

#[test]
fn window_listed_late_is_found_by_polling() {
    let tiled = leak(clients(&[client("0x9", 4242, ME.class, ME.title, false)]));
    let monitors = r#"[{"id":1,"width":1920,"height":1080}]"#;
    let fake = Fake::start(vec!["[]", "[]", tiled, monitors, "okokok"]);
    let outcome = float_window(
        &fake.socket(),
        ME,
        (1120, 700),
        Duration::from_secs(3),
        FAST,
    );
    assert!(matches!(outcome, Ok(Outcome::Floated(_))), "{outcome:?}");
    assert_eq!(fake.finish().len(), 5);
}

#[test]
fn floating_window_sends_no_batch() {
    let floating = leak(clients(&[client("0x9", 4242, ME.class, ME.title, true)]));
    let fake = Fake::start(vec![floating]);
    let outcome = float_window(
        &fake.socket(),
        ME,
        (1120, 700),
        Duration::from_secs(3),
        FAST,
    );
    assert_eq!(outcome, Ok(Outcome::AlreadyFloating));
    assert_eq!(fake.finish(), ["j/clients"]);
}

#[test]
fn never_listed_window_gives_up_at_the_deadline() {
    let fake = Fake::start(vec!["[]"; 200]);
    let outcome = float_window(
        &fake.socket(),
        ME,
        (1120, 700),
        Duration::from_millis(60),
        FAST,
    );
    assert_eq!(outcome, Ok(Outcome::NotFound));
    // The server thread stays parked in accept until the process ends.
    let _ = std::fs::remove_dir_all(&fake.dir);
}

#[test]
fn silent_socket_times_out_instead_of_hanging() {
    let fake = Fake::start(vec!["unused"]);
    let listener_dir = fake.dir.join("silent");
    std::fs::create_dir_all(&listener_dir).unwrap();
    let silent = UnixListener::bind(listener_dir.join(".socket.sock")).unwrap();
    let holder = std::thread::spawn(move || {
        let (stream, _) = silent.accept().unwrap();
        std::thread::sleep(Duration::from_millis(900));
        drop(stream);
    });
    let started = Instant::now();
    let outcome = float_window(
        &listener_dir.join(".socket.sock"),
        ME,
        (1120, 700),
        Duration::from_secs(3),
        FAST,
    );
    assert!(outcome.is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    holder.join().unwrap();
    let _ = std::fs::remove_dir_all(&fake.dir);
}

#[test]
fn missing_socket_is_an_error_not_a_panic() {
    let outcome = float_window(
        Path::new("/nonexistent/sdeck/.socket.sock"),
        ME,
        (1120, 700),
        Duration::from_millis(50),
        FAST,
    );
    assert!(outcome.is_err());
}

#[test]
fn rejected_batch_is_an_error() {
    let tiled = leak(clients(&[client("0x9", 4242, ME.class, ME.title, false)]));
    let fake = Fake::start(vec![tiled, r#"[{"id":1,"width":800,"height":600}]"#, "bad"]);
    let outcome = float_window(
        &fake.socket(),
        ME,
        (1120, 700),
        Duration::from_secs(3),
        FAST,
    );
    fake.finish();
    assert!(outcome.is_err());
}
