use super::*;
use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

fn node(id: u32, class: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": "PipeWire:Interface:Node",
        "info": {"props": {
            "media.class": class,
            "node.name": name,
            "node.description": format!("Device {name}")
        }}
    })
}

fn dump() -> Vec<u8> {
    serde_json::to_vec(&vec![
        node(1, "Audio/Sink", "sink_a"),
        node(2, "Audio/Source", "mic_a"),
        node(3, "Audio/Source", "mic_b"),
        node(4, "Audio/Sink", "sink_b"),
        node(5, "Stream/Input/Audio", "app"),
        node(6, "Audio/Source", "mic_c"),
    ])
    .unwrap()
}

#[derive(Default)]
struct Runner(Vec<Vec<String>>);
impl ProcessRunner for Runner {
    fn run(&mut self, argv: &[String]) -> io::Result<()> {
        self.0.push(argv.to_vec());
        Ok(())
    }
}

fn inspect(id: u32) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: format!("id {id}, type PipeWire:Interface:Node\n  node.name = mic").into_bytes(),
        stderr: Vec::new(),
    }
}

#[test]
fn inputs_list_only_audio_sources_in_dump_order() {
    let inputs = parse_inputs(&dump()).unwrap();
    let names: Vec<_> = inputs
        .iter()
        .map(|(id, i)| (*id, i.name.as_str()))
        .collect();
    assert_eq!(names, [(2, "mic_a"), (3, "mic_b"), (6, "mic_c")]);
    assert_eq!(inputs[0].1.description, "Device mic_a");
    // Outputs are unchanged by the new class.
    let outputs = parse_outputs(&dump()).unwrap();
    assert_eq!(
        outputs.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [1, 4]
    );
}

#[test]
fn set_input_resolves_the_stable_name_among_sources_only() {
    let mut runner = Runner::default();
    set_input("mic_b", &mut runner, || Ok(dump())).unwrap();
    assert_eq!(runner.0, [["wpctl", "set-default", "3"].map(String::from)]);
    // A sink name is not an input.
    let error = set_input("sink_a", &mut runner, || Ok(dump())).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains("audio input is disconnected"));
    assert_eq!(runner.0.len(), 1);
}

#[test]
fn input_rotation_follows_the_current_default_source_and_wraps() {
    for (current, next) in [(2, "3"), (3, "6"), (6, "2"), (99, "2")] {
        let mut runner = Runner::default();
        cycle_input(&mut runner, || Ok(dump()), || Ok(current)).unwrap();
        assert_eq!(runner.0, [["wpctl", "set-default", next].map(String::from)]);
    }
    let mut runner = Runner::default();
    let error = cycle_input(&mut runner, || Ok(b"[]".to_vec()), || Ok(1)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains("no audio inputs available"));
    assert!(runner.0.is_empty());
}

#[test]
fn default_source_id_is_read_from_the_inspect_header() {
    assert_eq!(inspect_default_node(inspect(42), "source").unwrap(), 42);
    let failed = Output {
        status: ExitStatus::from_raw(1 << 8),
        stdout: Vec::new(),
        stderr: b"boom".to_vec(),
    };
    assert!(inspect_default_node(failed, "source").is_err());
}
