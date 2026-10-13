use super::*;
use std::collections::BTreeMap;

#[derive(Debug)]
struct Call {
    destination: String,
    path: String,
    interface: String,
    method: String,
    parameters: Option<glib::Variant>,
    timeout_ms: i32,
}

#[derive(Default)]
struct FakeBus {
    names: Vec<String>,
    statuses: BTreeMap<String, String>,
    calls: Vec<Call>,
    fail_method: Option<&'static str>,
    malformed_method: Option<&'static str>,
}

impl Bus for FakeBus {
    fn call(
        &mut self,
        destination: &str,
        path: &str,
        interface: &str,
        method: &str,
        parameters: Option<&glib::Variant>,
        timeout_ms: i32,
    ) -> io::Result<glib::Variant> {
        self.calls.push(Call {
            destination: destination.into(),
            path: path.into(),
            interface: interface.into(),
            method: method.into(),
            parameters: parameters.cloned(),
            timeout_ms,
        });
        if self.fail_method == Some(method) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "bus call timed out",
            ));
        }
        if self.malformed_method == Some(method) {
            return Ok(42_u32.to_variant());
        }
        match method {
            "ListNames" => Ok((self.names.clone(),).to_variant()),
            "Get" => {
                let property = parameters.and_then(|value| value.get::<(String, String)>());
                if property != Some((PLAYER_INTERFACE.into(), "PlaybackStatus".into())) {
                    return Err(super::super::invalid("invalid property request"));
                }
                let status = self
                    .statuses
                    .get(destination)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "player vanished"))?;
                Ok((status.to_variant(),).to_variant())
            }
            "PlayPause" | "Stop" | "Next" | "Previous" => Ok(().to_variant()),
            _ => Err(super::super::invalid("unsupported bus method")),
        }
    }
}

fn fixture(players: &[(&str, &str)]) -> FakeBus {
    let statuses: BTreeMap<_, _> = players
        .iter()
        .map(|(suffix, status)| (format!("{SERVICE_PREFIX}{suffix}"), (*status).into()))
        .collect();
    FakeBus {
        names: statuses.keys().rev().cloned().collect(),
        statuses,
        ..FakeBus::default()
    }
}

#[test]
fn playing_player_wins_over_first_paused_service() {
    // Given unsorted services, with the first alphabetical player paused.
    let mut bus = fixture(&[("alpha", "Paused"), ("zeta", "Playing")]);
    bus.names.push("org.example.Unrelated".into());
    // When transport is dispatched.
    execute_on(&mut bus, MediaCommand::PlayPause).unwrap();
    // Then the Playing player is selected, excluding unrelated services.
    assert_eq!(
        bus.calls.last().unwrap().destination,
        format!("{SERVICE_PREFIX}zeta")
    );
    assert_eq!(bus.calls.len(), 4);
}

#[test]
fn alphabetical_first_player_wins_when_none_are_playing() {
    // Given unsorted stopped and paused services.
    let mut bus = fixture(&[("zeta", "Paused"), ("alpha", "Stopped")]);
    // When transport is dispatched.
    execute_on(&mut bus, MediaCommand::Next).unwrap();
    // Then selection is deterministic, independent of ListNames order.
    assert_eq!(
        bus.calls.last().unwrap().destination,
        format!("{SERVICE_PREFIX}alpha")
    );
}

#[test]
fn alphabetical_first_playing_player_wins_when_several_play() {
    // Given multiple Playing players in reversed discovery order.
    let mut bus = fixture(&[("zeta", "Playing"), ("alpha", "Playing")]);
    // When transport is dispatched.
    execute_on(&mut bus, MediaCommand::Stop).unwrap();
    // Then the first sorted Playing player is chosen.
    assert_eq!(
        bus.calls.last().unwrap().destination,
        format!("{SERVICE_PREFIX}alpha")
    );
    assert_eq!(bus.calls.len(), 3);
}

#[test]
fn transport_uses_exact_mpris_method_and_bounded_bus_calls() {
    // Given every requested transport operation and its wire method.
    for (command, method) in [
        (MediaCommand::PlayPause, "PlayPause"),
        (MediaCommand::Stop, "Stop"),
        (MediaCommand::Next, "Next"),
        (MediaCommand::Previous, "Previous"),
    ] {
        let mut bus = fixture(&[("fixture", "Playing")]);
        // When each transport operation is dispatched.
        execute_on(&mut bus, command).unwrap();
        // Then discovery, status, and transport calls have correct wire fields.
        let discovery = &bus.calls[0];
        assert_eq!(discovery.destination, "org.freedesktop.DBus");
        assert_eq!(discovery.path, "/org/freedesktop/DBus");
        assert_eq!(discovery.interface, "org.freedesktop.DBus");
        assert_eq!(discovery.method, "ListNames");
        assert!(discovery.parameters.is_none());
        let property = &bus.calls[1];
        assert_eq!(property.path, PLAYER_PATH);
        assert_eq!(property.interface, "org.freedesktop.DBus.Properties");
        assert_eq!(property.method, "Get");
        assert_eq!(
            property
                .parameters
                .as_ref()
                .unwrap()
                .get::<(String, String)>(),
            Some((PLAYER_INTERFACE.into(), "PlaybackStatus".into()))
        );
        let transport = &bus.calls[2];
        assert_eq!(transport.destination, format!("{SERVICE_PREFIX}fixture"));
        assert_eq!(transport.path, PLAYER_PATH);
        assert_eq!(transport.interface, PLAYER_INTERFACE);
        assert_eq!(transport.method, method);
        assert!(transport.parameters.is_none());
        assert!(bus.calls.iter().all(|call| call.timeout_ms == 3000));
    }
}

#[test]
fn no_player_returns_visible_not_found_without_control_call() {
    // Given only unrelated names and an invalid empty MPRIS suffix.
    let mut bus = FakeBus {
        names: vec!["org.freedesktop.DBus".into(), SERVICE_PREFIX.into()],
        ..FakeBus::default()
    };
    // When a media operation is dispatched.
    let error = execute_on(&mut bus, MediaCommand::Previous).unwrap_err();
    // Then no player transport is attempted and missing-player failure is explicit.
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(bus.calls.len(), 1);
}

#[test]
fn bus_boundary_failures_are_returned_without_key_fallback() {
    // Given timeout failures at discovery, property, or control boundaries.
    for method in ["ListNames", "Get", "PlayPause"] {
        let mut bus = fixture(&[("fixture", "Playing")]);
        bus.fail_method = Some(method);
        // When transport encounters the failed boundary.
        let error = execute_on(&mut bus, MediaCommand::PlayPause).unwrap_err();
        // Then its exact failure is returned, with no subsequent bus calls.
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(bus.calls.last().unwrap().method, method);
    }
}

#[test]
fn malformed_bus_replies_are_rejected_at_the_boundary() {
    // Given malformed discovery, status, or transport response payloads.
    for method in ["ListNames", "Get", "PlayPause"] {
        let mut bus = fixture(&[("fixture", "Playing")]);
        bus.malformed_method = Some(method);
        // When transport decodes the response.
        let error = execute_on(&mut bus, MediaCommand::PlayPause).unwrap_err();
        // Then invalid wire data is reported, not mistaken for success.
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(bus.calls.last().unwrap().method, method);
    }
}

#[test]
fn vanished_player_is_reported_without_controlling_another() {
    // Given a player disappearing between discovery and status read.
    let mut bus = fixture(&[("fixture", "Playing")]);
    bus.statuses.clear();
    // When transport selects a player.
    let error = execute_on(&mut bus, MediaCommand::Next).unwrap_err();
    // Then the disappearance is visible and no control call is sent.
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(bus.calls.len(), 2);
}

#[test]
fn unknown_playback_status_is_not_treated_as_paused() {
    // Given an invalid status value from a claimed MPRIS player.
    let mut bus = fixture(&[("fixture", "Invalid")]);
    // When selection decodes PlaybackStatus.
    let error = execute_on(&mut bus, MediaCommand::Stop).unwrap_err();
    // Then status validation fails before transport dispatch.
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(bus.calls.len(), 2);
}

#[test]
fn pipewire_controls_do_not_call_the_mpris_bus() {
    // Given output controls owned by the PipeWire adapter.
    for command in [
        MediaCommand::VolumeUp,
        MediaCommand::VolumeDown,
        MediaCommand::Mute,
    ] {
        let mut bus = FakeBus::default();
        // When these are incorrectly sent to the transport adapter.
        let error = execute_on(&mut bus, command).unwrap_err();
        // Then the boundary rejects them without any bus calls.
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(bus.calls.is_empty());
    }
}
