use super::*;
use std::io;
use std::sync::mpsc;
use std::time::Instant;

#[test]
fn response_is_rejected_when_correlation_or_status_is_invalid() {
    // Given wire responses with invalid correlation or status.
    for case in [
        "id",
        "type",
        "opcode",
        "status",
        "missing_status",
        "rejection",
    ] {
        let (config, peer) = server(false, move |mut socket| {
            let request = read(&mut socket);
            let mut response = json!({"op":7,"d":{
                "requestType":request["d"]["requestType"],"requestId":request["d"]["requestId"],
                "requestStatus":{"result":true,"code":100}
            }});
            match case {
                "id" => response["d"]["requestId"] = json!("wrong"),
                "type" => response["d"]["requestType"] = json!("Wrong"),
                "opcode" => response["op"] = json!(5),
                "status" => response["d"]["requestStatus"]["code"] = json!(500),
                "missing_status" => {
                    response["d"]
                        .as_object_mut()
                        .unwrap()
                        .remove("requestStatus");
                }
                "rejection" => {
                    response["d"]["requestStatus"] =
                        json!({"result":false,"code":608,"comment":"supersecretpassword"})
                }
                _ => unreachable!(),
            }
            send(&mut socket, response);
        });
        // When executing the request.
        let result = Client::new(config).execute(&ObsAction::ToggleRecording);
        // Then each error reaches the caller without untrusted server comments.
        let error = result.unwrap_err();
        assert_eq!(
            error.kind(),
            if case == "rejection" {
                io::ErrorKind::Other
            } else {
                io::ErrorKind::InvalidData
            }
        );
        assert!(!error.to_string().contains("supersecretpassword"));
        peer.join().unwrap();
    }
}

#[test]
fn handshake_is_rejected_when_hello_is_malformed() {
    // Given malformed JSON, opcode, version, or auth challenge in Hello.
    for message in [
        "{".into(),
        json!({"op":7,"d":{"rpcVersion":1}}).to_string(),
        json!({"op":0,"d":{"rpcVersion":0}}).to_string(),
        json!({"op":0,"d":{"rpcVersion":1,"authentication":{"salt":"s"}}}).to_string(),
        json!({"op":0,"d":{"rpcVersion":1,"authentication":{"salt":"","challenge":"c"}}})
            .to_string(),
    ] {
        let (config, peer) = raw_server(move |mut socket| {
            socket.send(Message::Text(message.into())).unwrap();
        });
        // When initiating an OBS connection.
        let result = Client::new(config).probe();
        // Then malformed handshake data fails, rather than being skipped.
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        peer.join().unwrap();
    }
}

#[test]
fn handshake_is_rejected_when_identified_is_invalid() {
    // Given an unsupported or malformed Identified message.
    for response in [
        json!({"op":2,"d":{"negotiatedRpcVersion":2}}),
        json!({"op":2,"d":{}}),
        json!({"op":0,"d":{"negotiatedRpcVersion":1}}),
    ] {
        let (config, peer) = raw_server(move |mut socket| {
            send(&mut socket, json!({"op":0,"d":{"rpcVersion":1}}));
            read(&mut socket);
            send(&mut socket, response);
        });
        // When negotiating the connection.
        let result = Client::new(config).probe();
        // Then no query is sent and negotiation errors are surfaced.
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        peer.join().unwrap();
    }
}

#[test]
fn close_is_reported_when_authentication_fails() {
    // Given a server closing after Identify.
    let (config, peer) = raw_server(|mut socket| {
        send(
            &mut socket,
            json!({"op":0,"d":{"rpcVersion":1,
            "authentication":{"salt":"salt","challenge":"challenge"}}}),
        );
        read(&mut socket);
        socket
            .close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::Library(4009),
                reason: "supersecretpassword".into(),
            }))
            .unwrap();
    });
    // When identifying with the server.
    let result = Client::new(config).probe();
    // Then the close is a connection failure, never a successful empty result.
    let error = result.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
    assert!(!error.to_string().contains("supersecretpassword"));
    peer.join().unwrap();
}

#[test]
fn probe_fails_when_response_data_is_missing_or_malformed() {
    // Given missing or mistyped scene data.
    for data in [
        Value::Null,
        json!({"scenes":[]}),
        json!({"currentProgramSceneName":3,"scenes":[]}),
    ] {
        let (config, peer) = server(false, move |mut socket| {
            let request = read(&mut socket);
            respond(&mut socket, &request, data);
        });
        // When probing.
        let result = Client::new(config).probe();
        // Then required response data is never fabricated.
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        peer.join().unwrap();
    }
}

#[test]
fn request_times_out_when_server_stalls() {
    // Given a server subscribed to the explicit client-completion signal.
    let (completed, completion) = mpsc::channel();
    let (config, peer) = server(false, move |mut socket| {
        read(&mut socket);
        completion.recv_timeout(Duration::from_secs(6)).unwrap();
    });
    // When executing without a response. Time itself is the behavior under test.
    let started = Instant::now();
    let result = Client::new(config).execute(&ObsAction::ToggleRecording);
    let elapsed = started.elapsed();
    completed.send(()).unwrap();
    // Then the single three-second operation deadline terminates the request.
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert!(elapsed < Duration::from_secs(5));
    peer.join().unwrap();
}

#[test]
fn hello_times_out_when_server_stalls() {
    // Given an upgraded websocket that waits for client completion, not sleeps.
    let (completed, completion) = mpsc::channel();
    let (config, peer) = raw_server(move |_socket| {
        completion.recv_timeout(Duration::from_secs(6)).unwrap();
    });
    // When probing a server that never sends Hello.
    let result = Client::new(config).probe();
    completed.send(()).unwrap();
    // Then the handshake is bounded by the same deadline.
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    peer.join().unwrap();
}

#[test]
fn websocket_upgrade_times_out_when_server_stalls() {
    // Given a TCP peer subscribed to completion but never upgrading to websocket.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = ObsConfig {
        endpoint: format!("ws://{}", listener.local_addr().unwrap()),
        password: String::new(),
    };
    let (completed, completion) = mpsc::channel();
    let peer = thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        completion.recv_timeout(Duration::from_secs(6)).unwrap();
    });
    // When probing the non-responsive HTTP upgrade.
    let result = Client::new(config).probe();
    completed.send(()).unwrap();
    // Then the initial websocket handshake also has a finite deadline.
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    peer.join().unwrap();
}

#[test]
fn control_frame_flood_is_rejected_when_budget_expires() {
    // Given more control messages than the client permits before Hello.
    let (config, peer) = raw_server(|mut socket| {
        for _ in 0..64 {
            socket.send(Message::Pong(Vec::new().into())).unwrap();
        }
    });
    // When probing.
    let result = Client::new(config).probe();
    // Then a control flood cannot create an infinite receive loop.
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    peer.join().unwrap();
}

#[test]
fn binary_or_oversized_message_is_rejected_when_received() {
    // Given forbidden binary or oversized JSON text frames.
    for message in [
        Message::Binary(vec![1, 2, 3].into()),
        Message::Text("x".repeat(1_048_577).into()),
    ] {
        let (config, peer) = raw_server(move |mut socket| {
            // The receiver may close while an oversized frame is being sent.
            match socket.send(message) {
                Ok(()) | Err(tungstenite::Error::Io(_)) => {}
                Err(error) => panic!("unexpected fake server error: {error}"),
            }
        });
        // When probing.
        let result = Client::new(config).probe();
        // Then decoding and memory limits are enforced.
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        peer.join().unwrap();
    }
}
