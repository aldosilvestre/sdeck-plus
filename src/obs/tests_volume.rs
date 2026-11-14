use super::*;
use crate::desktop_actions::VolumeDirection;
use std::io;

fn volume(direction: VolumeDirection, db: u8) -> ObsAction {
    ObsAction::InputVolume {
        input: "Mic".into(),
        direction,
        db,
    }
}

/// A server that reports `current` dB and expects the `target` dB to be set.
fn volume_server(current: Value, target: f64) -> (ObsConfig, JoinHandle<()>) {
    server(false, move |mut socket| {
        let get = read(&mut socket);
        assert_eq!(get["d"]["requestType"], "GetInputVolume");
        assert_eq!(get["d"]["requestData"], json!({"inputName":"Mic"}));
        respond(&mut socket, &get, current);
        let set = read(&mut socket);
        assert_ne!(get["d"]["requestId"], set["d"]["requestId"]);
        assert_eq!(set["d"]["requestType"], "SetInputVolume");
        assert_eq!(
            set["d"]["requestData"],
            json!({"inputName":"Mic","inputVolumeDb":target})
        );
        respond(&mut socket, &set, json!({}));
    })
}

#[test]
fn input_volume_moves_by_the_step_from_the_current_level() {
    for (direction, db, current, target) in [
        (VolumeDirection::Up, 2, -10.0, -8.0),
        (VolumeDirection::Down, 3, -10.5, -13.5),
        (VolumeDirection::Up, 20, 0.0, 20.0),
    ] {
        let (config, peer) = volume_server(
            json!({"inputVolumeDb":current,"inputVolumeMul":0.5}),
            target,
        );
        Client::new(config).execute(&volume(direction, db)).unwrap();
        peer.join().unwrap();
    }
}

#[test]
fn input_volume_stays_inside_the_range_obs_accepts() {
    for (direction, current, target) in [
        (VolumeDirection::Up, 25.0, 26.0),
        (VolumeDirection::Up, 26.0, 26.0),
        (VolumeDirection::Down, -99.0, -100.0),
        (VolumeDirection::Down, -100.0, -100.0),
    ] {
        let (config, peer) = volume_server(json!({"inputVolumeDb":current}), target);
        Client::new(config).execute(&volume(direction, 5)).unwrap();
        peer.join().unwrap();
    }
}

#[test]
fn input_volume_fails_when_obs_reports_no_level_and_sets_nothing() {
    let (config, peer) = server(false, |mut socket| {
        let get = read(&mut socket);
        assert_eq!(get["d"]["requestType"], "GetInputVolume");
        respond(&mut socket, &get, json!({"inputVolumeMul":1.0}));
        // The client closes the connection without a SetInputVolume request.
        loop {
            match socket.read() {
                Err(_) | Ok(Message::Close(_)) => break,
                Ok(message) => assert!(!message.to_string().contains("SetInputVolume")),
            }
        }
    });
    let error = Client::new(config)
        .execute(&volume(VolumeDirection::Up, 2))
        .unwrap_err();
    assert!(!error.to_string().is_empty());
    peer.join().unwrap();
}

#[test]
fn invalid_input_volume_actions_are_rejected_before_connecting() {
    let config = ObsConfig {
        endpoint: "ws://127.0.0.1:1".into(),
        password: String::new(),
    };
    let mut cases = vec![
        volume(VolumeDirection::Up, 0),
        volume(VolumeDirection::Down, 21),
    ];
    for name in ["", "  ", "Mi\nc", &"x".repeat(513)] {
        cases.push(ObsAction::InputVolume {
            input: name.into(),
            direction: VolumeDirection::Up,
            db: 2,
        });
    }
    for action in cases {
        assert_eq!(
            action.validate().unwrap_err().kind(),
            io::ErrorKind::InvalidInput,
            "{action:?}"
        );
        // Validation fails before any connection attempt, so the refused port is never used.
        let error = Client::new(config.clone()).execute(&action).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{action:?}");
    }
    volume(VolumeDirection::Up, 1).validate().unwrap();
    volume(VolumeDirection::Down, 20).validate().unwrap();
}

#[test]
fn input_volume_has_a_stable_shape_and_summary() {
    let action = volume(VolumeDirection::Up, 2);
    let json = r#"{"InputVolume":{"input":"Mic","direction":"Up","db":2}}"#;
    assert_eq!(serde_json::to_string(&action).unwrap(), json);
    assert_eq!(serde_json::from_str::<ObsAction>(json).unwrap(), action);
    assert_eq!(action.summary(), "OBS input volume up 2 dB: Mic");
    assert_eq!(
        volume(VolumeDirection::Down, 5).summary(),
        "OBS input volume down 5 dB: Mic"
    );
}
