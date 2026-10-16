use super::*;
use std::{cell::Cell, os::unix::process::ExitStatusExt, process::ExitStatus, rc::Rc};

fn snapshot(ids: &[u32]) -> Vec<u8> {
    serde_json::to_vec(
        &ids.iter()
            .map(|id| {
                serde_json::json!({
                    "id": id,
                    "type": "PipeWire:Interface:Node",
                    "info": {"props": {
                        "media.class": "Audio/Sink",
                        "node.name": format!("sink_{id}")
                    }}
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

fn inspect(stdout: &[u8], code: i32) -> Output {
    Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: stdout.to_vec(),
        stderr: b"inspect failure".to_vec(),
    }
}

struct Runner {
    current: Rc<Cell<u32>>,
    selected: Vec<u32>,
    fail: bool,
}

impl ProcessRunner for Runner {
    fn run(&mut self, argv: &[String]) -> io::Result<()> {
        assert_eq!(&argv[..2], ["wpctl", "set-default"]);
        assert_eq!(argv.len(), 3);
        if self.fail {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        }
        let id = argv[2].parse().unwrap();
        self.current.set(id);
        self.selected.push(id);
        Ok(())
    }
}

struct Fixture {
    runner: Runner,
    ids: Vec<u32>,
    queries: usize,
    discoveries: usize,
}

impl Fixture {
    fn new(ids: &[u32], current: u32) -> Self {
        Self {
            runner: Runner {
                current: Rc::new(Cell::new(current)),
                selected: Vec::new(),
                fail: false,
            },
            ids: ids.to_vec(),
            queries: 0,
            discoveries: 0,
        }
    }

    fn press(&mut self) -> io::Result<()> {
        let current = self.runner.current.clone();
        cycle_output(
            &mut self.runner,
            || {
                self.discoveries += 1;
                Ok(snapshot(&self.ids))
            },
            || {
                self.queries += 1;
                inspect_default_output(inspect(
                    format!(
                        "id {}, type PipeWire:Interface:Node\n  node.name = sink",
                        current.get()
                    )
                    .as_bytes(),
                    0,
                ))
            },
        )
    }
}

#[test]
fn repeated_presses_follow_actual_default_and_wrap() {
    // Given three outputs with a mutable default updated by successful selection.
    let mut host = Fixture::new(&[1, 2, 3], 1);
    // When the rotation is pressed through a complete cycle.
    for _ in 0..3 {
        host.press().unwrap();
    }
    // Then each press rediscovers outputs and queries the actual current default.
    assert_eq!(host.runner.selected, [2, 3, 1]);
    assert_eq!(host.runner.current.get(), 1);
    assert_eq!((host.discoveries, host.queries), (3, 3));
}

#[test]
fn hotplug_and_reordering_follow_fresh_picker_order() {
    // Given a previous press and a newly connected sink in a reordered snapshot.
    let mut host = Fixture::new(&[1, 2, 3], 1);
    host.press().unwrap();
    host.ids = vec![4, 2, 1];
    // When the next press discovers that snapshot.
    host.press().unwrap();
    // Then it follows the current sink in the new order, not a cached index.
    assert_eq!(host.runner.selected, [2, 1]);
    assert_eq!((host.discoveries, host.queries), (2, 2));
}

#[test]
fn external_default_change_is_observed_on_next_press() {
    // Given a previous selection followed by an external default change.
    let mut host = Fixture::new(&[1, 2, 3], 1);
    host.press().unwrap();
    host.runner.current.set(3);
    // When another binding presses rotate.
    host.press().unwrap();
    // Then rotation follows the external default instead of binding history.
    assert_eq!(host.runner.selected, [2, 1]);
    assert_eq!(host.queries, 2);
}

#[test]
fn missing_current_sink_selects_first_in_snapshot() {
    // Given a resolved default absent from the current discovery.
    let mut host = Fixture::new(&[3, 1, 2], 99);
    // When rotation is pressed.
    host.press().unwrap();
    // Then the first discovered sink is selected.
    assert_eq!(host.runner.selected, [3]);
}

#[test]
fn single_sink_is_selected_again() {
    // Given exactly one sink which is already default.
    let mut host = Fixture::new(&[58], 58);
    // When rotation is pressed.
    host.press().unwrap();
    // Then the same sink is explicitly selected.
    assert_eq!(host.runner.selected, [58]);
    assert_eq!(host.queries, 1);
}

#[test]
fn no_sinks_returns_error_without_default_query_or_selection() {
    // Given no available outputs.
    let mut host = Fixture::new(&[], 58);
    // When rotation is pressed.
    let error = host.press().unwrap_err();
    // Then empty discovery is explicit and no mutation is attempted.
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(host.queries, 0);
    assert!(host.runner.selected.is_empty());
}

#[test]
fn discovery_and_default_failures_prevent_selection() {
    // Given failures at each read boundary, including malformed discovery.
    for failure in 0..3 {
        let mut host = Fixture::new(&[1, 2], 1);
        // When the failing boundary is reached.
        let error = cycle_output(
            &mut host.runner,
            || match failure {
                0 => Err(io::Error::new(io::ErrorKind::NotFound, "pw-dump missing")),
                1 => Ok(b"invalid".to_vec()),
                _ => Ok(snapshot(&[1, 2])),
            },
            || Err(io::Error::new(io::ErrorKind::BrokenPipe, "inspect failed")),
        )
        .unwrap_err();
        // Then its failure propagates without any selection.
        assert_eq!(
            error.kind(),
            [
                io::ErrorKind::NotFound,
                io::ErrorKind::InvalidData,
                io::ErrorKind::BrokenPipe
            ][failure]
        );
        assert!(host.runner.selected.is_empty());
        assert_eq!(host.runner.current.get(), 1);
    }
}

#[test]
fn selection_failure_propagates_without_advancing_default() {
    // Given a successful read followed by a failed set-default.
    let mut host = Fixture::new(&[1, 2], 1);
    host.runner.fail = true;
    // When rotation is pressed.
    let error = host.press().unwrap_err();
    // Then the command error is returned and the actual default stays unchanged.
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(host.runner.current.get(), 1);
    assert_eq!(host.queries, 1);
}

#[test]
fn inspect_header_resolves_installed_node_format() {
    // Given the installed wpctl inspect header format.
    let output = inspect(
        b"id 58, type PipeWire:Interface:Node\n  node.name = \"sink\"\n",
        0,
    );
    // When the default is parsed.
    let id = inspect_default_output(output).unwrap();
    // Then its numeric node ID is returned.
    assert_eq!(id, 58);
}

#[test]
fn invalid_inspect_headers_are_explicit_errors() {
    // Given malformed headers, non-node objects, a status tree, and invalid UTF-8.
    for bytes in [
        b"".as_slice(),
        b"id nope, type PipeWire:Interface:Node",
        b"id 4294967296, type PipeWire:Interface:Node",
        b"id 58, type PipeWire:Interface:Device",
        b"Audio\n Sinks:\n * 58. Speakers",
        b"\xff",
    ] {
        // When inspect output is parsed.
        let error = inspect_default_output(inspect(bytes, 0)).unwrap_err();
        // Then invalid output cannot silently select a sink.
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}

#[test]
fn unsuccessful_inspect_exit_is_not_accepted_as_default() {
    // Given a failed command which still printed a valid header.
    let output = inspect(b"id 58, type PipeWire:Interface:Node", 1);
    // When the command output is handled.
    let error = inspect_default_output(output).unwrap_err();
    // Then its exit failure and stderr are surfaced.
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(error.to_string().contains("inspect failure"));
}
