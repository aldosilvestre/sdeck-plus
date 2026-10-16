//! PipeWire sink discovery and stable-name default output selection.

use crate::layers::ProcessRunner;
use serde::Deserialize;
use std::{
    io,
    process::{Command, Output},
};

/// An available sink, identified by its persistent PipeWire `node.name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioOutput {
    /// Stable `node.name` to persist in `DesktopAction::SetAudioOutput`.
    pub name: String,
    /// Display label (`node.description`, or the stable name when absent).
    pub description: String,
}

/// Lists the current PipeWire audio sinks using `pw-dump`.
/// Only `PipeWire:Interface:Node` objects with `media.class = Audio/Sink`
/// are returned, in dump order. Numeric IDs are deliberately not exposed:
/// execution resolves the saved name against a fresh dump.
///
/// # Errors
/// Returns command failures and `InvalidData` for malformed dump data.
pub fn list_outputs() -> io::Result<Vec<AudioOutput>> {
    Ok(parse_outputs(&dump()?)?
        .into_iter()
        .map(|(_, output)| output)
        .collect())
}

pub(super) fn dump() -> io::Result<Vec<u8>> {
    dump_output(Command::new("pw-dump").output()?)
}

fn dump_output(output: Output) -> io::Result<Vec<u8>> {
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "pw-dump exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

#[derive(Deserialize)]
struct Object {
    id: u32,
    #[serde(rename = "type")]
    kind: String,
    info: Option<Info>,
}

#[derive(Deserialize)]
struct Info {
    props: Option<Props>,
}

#[derive(Deserialize)]
struct Props {
    #[serde(rename = "media.class")]
    media_class: Option<String>,
    #[serde(rename = "node.name")]
    name: Option<String>,
    #[serde(rename = "node.description")]
    description: Option<String>,
}

fn parse_outputs(bytes: &[u8]) -> io::Result<Vec<(u32, AudioOutput)>> {
    let objects: Vec<Object> = serde_json::from_slice(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut outputs = Vec::new();
    for object in objects {
        if object.kind != "PipeWire:Interface:Node" {
            continue;
        }
        let Some(props) = object.info.and_then(|info| info.props) else {
            continue;
        };
        if props.media_class.as_deref() != Some("Audio/Sink") {
            continue;
        }
        let name = props.name.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "audio sink has no node.name")
        })?;
        super::printable_target(&name)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let description = props.description.unwrap_or_else(|| name.clone());
        outputs.push((object.id, AudioOutput { name, description }));
    }
    Ok(outputs)
}

pub(super) fn set_output(
    name: &str,
    process: &mut impl ProcessRunner,
    read_dump: impl FnOnce() -> io::Result<Vec<u8>>,
) -> io::Result<()> {
    let outputs = parse_outputs(&read_dump()?)?;
    let (id, _) = outputs
        .iter()
        .find(|(_, output)| output.name == name)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("audio output is disconnected: {name}"),
            )
        })?;
    process.run(&["wpctl".into(), "set-default".into(), id.to_string()])
}

pub(super) fn default_sink() -> io::Result<u32> {
    inspect_default_output(
        Command::new("wpctl")
            .args(["inspect", "@DEFAULT_AUDIO_SINK@"])
            .output()?,
    )
}

fn inspect_default_output(output: Output) -> io::Result<u32> {
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "wpctl inspect exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    // Inspect's object header is not the human-readable status tree.
    let id = text
        .lines()
        .next()
        .and_then(|header| header.strip_prefix("id "))
        .and_then(|header| header.strip_suffix(", type PipeWire:Interface:Node"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid default sink inspect header",
            )
        })?;
    id.parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(super) fn cycle_output(
    process: &mut impl ProcessRunner,
    read_dump: impl FnOnce() -> io::Result<Vec<u8>>,
    read_default: impl FnOnce() -> io::Result<u32>,
) -> io::Result<()> {
    let outputs = parse_outputs(&read_dump()?)?;
    let first = outputs
        .first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no audio outputs available"))?;
    let current = read_default()?;
    let next = outputs
        .iter()
        .position(|(id, _)| *id == current)
        .and_then(|index| outputs.get(index + 1))
        .unwrap_or(first);
    process.run(&["wpctl".into(), "set-default".into(), next.0.to_string()])
}

#[cfg(test)]
#[path = "audio_output_cycle_tests.rs"]
mod cycle_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    fn sink(id: u32) -> Vec<u8> {
        format!(
            r#"[{{"id":{id},"type":"PipeWire:Interface:Node","info":{{"props":{{
                "media.class":"Audio/Sink","node.name":"alsa_output.usb",
                "node.description":"USB Speakers"
            }}}}}}]"#
        )
        .into_bytes()
    }

    #[derive(Default)]
    struct Runner {
        calls: Vec<Vec<String>>,
        fail: bool,
    }

    impl ProcessRunner for Runner {
        fn run(&mut self, argv: &[String]) -> io::Result<()> {
            self.calls.push(argv.to_vec());
            if self.fail {
                Err(io::Error::new(io::ErrorKind::NotFound, "wpctl missing"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn parser_keeps_only_sink_nodes_and_exposes_stable_names() {
        let mut objects: Vec<serde_json::Value> = serde_json::from_slice(&sink(41)).unwrap();
        objects.extend(
            serde_json::from_str::<Vec<serde_json::Value>>(
                r#"[
                {"id":1,"type":"PipeWire:Interface:Core","info":{"props":{}}},
                {"id":2,"type":"PipeWire:Interface:Device","info":{"props":{
                    "media.class":"Audio/Sink","node.name":"not-a-node"}}},
                {"id":3,"type":"PipeWire:Interface:Node","info":{"props":{
                    "media.class":"Audio/Source","node.name":"microphone"}}},
                {"id":4,"type":"PipeWire:Interface:Node"},
                {"id":5,"type":"PipeWire:Interface:Node","info":{"props":{
                    "media.class":"Audio/Sink","node.name":"virtual_sink"}}}
            ]"#,
            )
            .unwrap(),
        );
        let outputs = parse_outputs(&serde_json::to_vec(&objects).unwrap()).unwrap();
        assert_eq!(
            outputs,
            [
                (
                    41,
                    AudioOutput {
                        name: "alsa_output.usb".into(),
                        description: "USB Speakers".into(),
                    }
                ),
                (
                    5,
                    AudioOutput {
                        name: "virtual_sink".into(),
                        description: "virtual_sink".into(),
                    }
                ),
            ]
        );
    }

    #[test]
    fn malformed_dump_and_sink_data_are_errors() {
        for data in [
            "",
            "{}",
            "[",
            r#"[{"id":"41","type":"PipeWire:Interface:Node"}]"#,
            r#"[{"id":1,"type":"PipeWire:Interface:Node","info":{"props":{
                "media.class":"Audio/Sink"}}}]"#,
            r#"[{"id":1,"type":"PipeWire:Interface:Node","info":{"props":{
                "media.class":"Audio/Sink","node.name":""}}}]"#,
            r#"[{"id":1,"type":"PipeWire:Interface:Node","info":{"props":{
                "media.class":"Audio/Sink","node.name":"bad\nname"}}}]"#,
            r#"[{"id":1,"type":"PipeWire:Interface:Node","info":{"props":{
                "media.class":"Audio/Sink","node.name":123}}}]"#,
        ] {
            assert_eq!(
                parse_outputs(data.as_bytes()).unwrap_err().kind(),
                io::ErrorKind::InvalidData,
                "{data}"
            );
        }
        assert!(parse_outputs(b"[]").unwrap().is_empty());
    }

    #[test]
    fn selection_resolves_current_id_each_time_without_persisting_it() {
        let mut runner = Runner::default();
        for id in [41, 109] {
            set_output("alsa_output.usb", &mut runner, || Ok(sink(id))).unwrap();
        }
        assert_eq!(
            runner.calls,
            [
                ["wpctl", "set-default", "41"].map(String::from).to_vec(),
                ["wpctl", "set-default", "109"].map(String::from).to_vec(),
            ]
        );
    }

    #[test]
    fn disconnected_target_does_not_run_wpctl() {
        let mut runner = Runner::default();
        let error = set_output("disconnected", &mut runner, || Ok(sink(41))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn discovery_and_invalid_json_errors_prevent_selection() {
        let mut runner = Runner::default();
        let error = set_output("alsa_output.usb", &mut runner, || {
            Err(io::Error::new(io::ErrorKind::NotFound, "pw-dump missing"))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        let error =
            set_output("alsa_output.usb", &mut runner, || Ok(b"invalid".to_vec())).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn wpctl_errors_propagate_after_resolution() {
        let mut runner = Runner {
            fail: true,
            ..Runner::default()
        };
        let error = set_output("alsa_output.usb", &mut runner, || Ok(sink(41))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(runner.calls.len(), 1);
    }

    #[test]
    fn unsuccessful_dump_exit_is_not_parsed_as_empty_discovery() {
        let error = dump_output(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: b"[]".to_vec(),
            stderr: b"connection refused".to_vec(),
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("connection refused"));
    }
}
