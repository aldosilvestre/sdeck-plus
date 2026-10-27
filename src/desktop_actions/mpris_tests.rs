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

/// Answers the read-only calls the Now playing reader makes.
#[derive(Default)]
struct StatusBus {
    names: Vec<String>,
    /// Bus name -> (PlaybackStatus, Metadata) as the player reports them.
    players: BTreeMap<String, (String, glib::Variant)>,
    /// Bus name -> (Identity, DesktopEntry) on the root interface.
    roots: BTreeMap<String, (String, String)>,
    calls: Vec<Call>,
    fail: Option<(&'static str, &'static str)>,
}

impl Bus for StatusBus {
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
        if let Some((failing, at)) = self.fail {
            if failing == method && (at.is_empty() || destination.ends_with(at)) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "bus call timed out",
                ));
            }
        }
        match method {
            "ListNames" => Ok((self.names.clone(),).to_variant()),
            "GetAll"
                if parameters.and_then(|value| value.get::<(String,)>())
                    == Some((ROOT_INTERFACE.into(),)) =>
            {
                let (identity, entry) = self
                    .roots
                    .get(destination)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no root"))?;
                let mut properties = std::collections::HashMap::new();
                properties.insert("Identity".to_owned(), identity.to_variant());
                properties.insert("DesktopEntry".to_owned(), entry.to_variant());
                Ok((properties,).to_variant())
            }
            "GetAll" => {
                assert_eq!(
                    parameters.and_then(|value| value.get::<(String,)>()),
                    Some((PLAYER_INTERFACE.into(),))
                );
                let (status, metadata) = self
                    .players
                    .get(destination)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "player vanished"))?;
                let mut properties = std::collections::HashMap::new();
                properties.insert("PlaybackStatus".to_owned(), status.to_variant());
                properties.insert("Metadata".to_owned(), metadata.clone());
                Ok((properties,).to_variant())
            }
            _ => panic!("the status reader must not call {method}"),
        }
    }
}

fn metadata(title: Option<&str>, artist: Option<glib::Variant>) -> glib::Variant {
    let mut fields = std::collections::HashMap::new();
    if let Some(title) = title {
        fields.insert("xesam:title".to_owned(), title.to_variant());
    }
    if let Some(artist) = artist {
        fields.insert("xesam:artist".to_owned(), artist);
    }
    fields.to_variant()
}

fn status_fixture(players: &[(&str, &str, glib::Variant)]) -> StatusBus {
    let players: BTreeMap<_, _> = players
        .iter()
        .map(|(suffix, status, metadata)| {
            (
                format!("{SERVICE_PREFIX}{suffix}"),
                ((*status).to_owned(), metadata.clone()),
            )
        })
        .collect();
    StatusBus {
        names: players
            .keys()
            .rev()
            .cloned()
            .chain(["org.freedesktop.Notifications".to_owned()])
            .collect(),
        players,
        ..StatusBus::default()
    }
}

#[test]
fn status_reader_reports_every_player_sorted_with_title_and_artists() {
    // Given two players in reversed discovery order and an unrelated service.
    let artists = vec!["Queen".to_owned(), "David Bowie".to_owned()].to_variant();
    let mut bus = status_fixture(&[
        (
            "zeta",
            "Paused",
            metadata(Some("Under Pressure"), Some(artists)),
        ),
        (
            "alpha",
            "Playing",
            metadata(Some("Song"), Some("Solo".to_variant())),
        ),
    ]);
    // When the reader runs.
    let players = read_players_on(&mut bus).unwrap();
    // Then players come sorted by bus name, artists joined, nothing controlled.
    assert_eq!(
        players,
        vec![
            PlayerState {
                bus_name: format!("{SERVICE_PREFIX}alpha"),
                status: PlaybackStatus::Playing,
                title: Some("Song".into()),
                artist: Some("Solo".into()),
                identity: None,
                desktop_entry: None,
            },
            PlayerState {
                bus_name: format!("{SERVICE_PREFIX}zeta"),
                status: PlaybackStatus::Paused,
                title: Some("Under Pressure".into()),
                artist: Some("Queen, David Bowie".into()),
                identity: None,
                desktop_entry: None,
            },
        ]
    );
    assert!(bus
        .calls
        .iter()
        .all(|call| ["ListNames", "GetAll"].contains(&call.method.as_str())));
    let read = &bus.calls[1];
    assert_eq!(read.path, PLAYER_PATH);
    assert_eq!(read.interface, "org.freedesktop.DBus.Properties");
    assert!(bus
        .calls
        .iter()
        .all(|call| call.timeout_ms == READ_TIMEOUT_MS));
}

#[test]
fn status_reader_reads_identity_and_desktop_entry_from_the_root_interface() {
    // Given a stopped player and a playing one without usable metadata.
    let mut bus = status_fixture(&[
        ("browser", "Playing", metadata(Some("  "), None)),
        ("idle", "Stopped", metadata(None, None)),
    ]);
    bus.roots.insert(
        format!("{SERVICE_PREFIX}browser"),
        ("Firefox".into(), "firefox".into()),
    );
    // When the reader runs.
    let players = read_players_on(&mut bus).unwrap();
    // Then blank titles are missing; Identity and DesktopEntry come in one call.
    assert_eq!(players[0].title, None);
    assert_eq!(players[0].artist, None);
    assert_eq!(players[0].identity.as_deref(), Some("Firefox"));
    assert_eq!(players[0].desktop_entry.as_deref(), Some("firefox"));
    assert_eq!(players[1].status, PlaybackStatus::Stopped);
    assert_eq!(players[1].identity, None, "a missing root is not an error");
    assert_eq!(players[1].desktop_entry, None);
    // Two bounded reads per player: its Player and its root properties.
    let reads: Vec<_> = bus
        .calls
        .iter()
        .filter(|call| call.destination.ends_with("browser"))
        .map(|call| {
            call.parameters
                .as_ref()
                .unwrap()
                .get::<(String,)>()
                .unwrap()
                .0
        })
        .collect();
    assert_eq!(reads, [PLAYER_INTERFACE, ROOT_INTERFACE]);
}

#[test]
fn status_reader_skips_broken_players_but_reports_bus_failures() {
    // Given one player that fails, one with an unknown status and one healthy.
    let mut bus = status_fixture(&[
        ("broken", "Playing", metadata(Some("x"), None)),
        ("odd", "Buffering", metadata(Some("y"), None)),
        ("ok", "Paused", metadata(Some("z"), None)),
    ]);
    bus.fail = Some(("GetAll", "broken"));
    // When the reader runs, a misbehaving player does not hide the others.
    let players = read_players_on(&mut bus).unwrap();
    assert_eq!(players.len(), 1);
    assert_eq!(players[0].title.as_deref(), Some("z"));
    // And no player at all is an empty, successful reading.
    let mut empty = StatusBus::default();
    assert_eq!(read_players_on(&mut empty).unwrap(), vec![]);
    // But a failing bus is an error, not "nothing playing".
    let mut down = status_fixture(&[("ok", "Playing", metadata(Some("z"), None))]);
    down.fail = Some(("ListNames", ""));
    assert_eq!(
        read_players_on(&mut down).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
}
