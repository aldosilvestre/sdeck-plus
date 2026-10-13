use super::{Client, ObsAction, ObsConfig, ObsStatus};
use serde_json::{json, Value};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tungstenite::{Message, WebSocket};

#[path = "tests_config.rs"]
mod config;
#[path = "tests_errors.rs"]
mod errors;

fn raw_server(
    handler: impl FnOnce(WebSocket<TcpStream>) + Send + 'static,
) -> (ObsConfig, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("ws://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        handler(tungstenite::accept(stream).unwrap());
    });
    (
        ObsConfig {
            endpoint,
            password: "supersecretpassword".into(),
        },
        server,
    )
}

fn server(
    authenticated: bool,
    handler: impl FnOnce(WebSocket<TcpStream>) + Send + 'static,
) -> (ObsConfig, JoinHandle<()>) {
    raw_server(move |mut socket| {
        let mut hello = json!({"op":0,"d":{"rpcVersion":1}});
        if authenticated {
            hello["d"]["authentication"] = json!({
                "salt":"lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=",
                "challenge":"+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY="
            });
        }
        send(&mut socket, hello);
        let identify = read(&mut socket);
        assert_eq!(identify["op"], 1);
        assert_eq!(identify["d"]["rpcVersion"], 1);
        assert_eq!(identify["d"]["eventSubscriptions"], 0);
        if authenticated {
            assert_eq!(
                identify["d"]["authentication"],
                "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
            );
        } else {
            assert!(identify["d"].get("authentication").is_none());
        }
        send(&mut socket, json!({"op":2,"d":{"negotiatedRpcVersion":1}}));
        handler(socket);
    })
}

fn read(socket: &mut WebSocket<TcpStream>) -> Value {
    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap()
}

fn send(socket: &mut WebSocket<TcpStream>, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .unwrap();
}

fn respond(socket: &mut WebSocket<TcpStream>, request: &Value, data: Value) {
    send(
        socket,
        json!({"op":7,"d":{
            "requestType":request["d"]["requestType"],"requestId":request["d"]["requestId"],
            "requestStatus":{"result":true,"code":100},"responseData":data
        }}),
    );
}

#[test]
fn scene_is_executed_when_authenticated() {
    // Given a real local websocket using the published authentication vector.
    let (config, peer) = server(true, |mut socket| {
        let request = read(&mut socket);
        assert_eq!(request["op"], 6);
        assert_eq!(request["d"]["requestType"], "SetCurrentProgramScene");
        assert_eq!(request["d"]["requestData"], json!({"sceneName":"Main"}));
        respond(&mut socket, &request, json!({}));
    });
    // When the client executes a scene switch.
    let result = Client::new(config).execute(&ObsAction::Scene("Main".into()));
    // Then the server acknowledges the exact request and authenticated handshake.
    result.unwrap();
    peer.join().unwrap();
}

#[test]
fn actions_use_expected_requests_when_executed() {
    // Given the supported one-request action variants.
    for (action, kind, data) in [
        (ObsAction::ToggleRecording, "ToggleRecord", json!({})),
        (ObsAction::ToggleStreaming, "ToggleStream", json!({})),
        (ObsAction::StartRecording, "StartRecord", json!({})),
        (ObsAction::StopRecording, "StopRecord", json!({})),
        (ObsAction::PauseRecording, "PauseRecord", json!({})),
        (ObsAction::ResumeRecording, "ResumeRecord", json!({})),
        (ObsAction::StartStreaming, "StartStream", json!({})),
        (ObsAction::StopStreaming, "StopStream", json!({})),
        (
            ObsAction::ToggleMute("Mic".into()),
            "ToggleInputMute",
            json!({"inputName":"Mic"}),
        ),
    ] {
        let (config, peer) = server(false, move |mut socket| {
            let request = read(&mut socket);
            assert_eq!(request["d"]["requestType"], kind);
            assert_eq!(request["d"]["requestData"], data);
            // Successful action responses need not include responseData.
            send(
                &mut socket,
                json!({"op":7,"d":{
                    "requestType":kind,"requestId":request["d"]["requestId"],
                    "requestStatus":{"result":true,"code":100}
                }}),
            );
        });
        // When each action is executed.
        let result = Client::new(config).execute(&action);
        // Then its exact wire request succeeds.
        result.unwrap();
        peer.join().unwrap();
    }
}

#[test]
fn visibility_is_set_when_scene_item_is_resolved() {
    // Given a scene containing an item with ID 42.
    let (config, peer) = server(false, |mut socket| {
        let lookup = read(&mut socket);
        assert_eq!(lookup["d"]["requestType"], "GetSceneItemId");
        assert_eq!(
            lookup["d"]["requestData"],
            json!({"sceneName":"Main","sourceName":"Camera"})
        );
        respond(&mut socket, &lookup, json!({"sceneItemId":42}));
        let set = read(&mut socket);
        assert_ne!(lookup["d"]["requestId"], set["d"]["requestId"]);
        assert_eq!(set["d"]["requestType"], "SetSceneItemEnabled");
        assert_eq!(
            set["d"]["requestData"],
            json!({"sceneName":"Main","sceneItemId":42,"sceneItemEnabled":false})
        );
        respond(&mut socket, &set, json!({}));
    });
    // When visibility is set by source name.
    let result = Client::new(config).execute(&ObsAction::SourceVisibility {
        scene: "Main".into(),
        source: "Camera".into(),
        visible: false,
    });
    // Then the resolved numeric ID is used in the acknowledged mutation.
    result.unwrap();
    peer.join().unwrap();
}

#[test]
fn probe_returns_status_when_all_queries_succeed() {
    // Given OBS with two scenes and different recording/streaming states.
    let (config, peer) = server(false, |mut socket| {
        let mut ids = Vec::new();
        for (kind, response) in [
            (
                "GetSceneList",
                json!({"currentProgramSceneName":"Main",
                "scenes":[{"sceneName":"Main","sceneIndex":0},{"sceneName":"BRB","sceneIndex":1}]}),
            ),
            ("GetRecordStatus", json!({"outputActive":true})),
            ("GetStreamStatus", json!({"outputActive":false})),
        ] {
            let request = read(&mut socket);
            assert_eq!(request["d"]["requestType"], kind);
            assert_eq!(request["d"]["requestData"], json!({}));
            assert!(!ids.contains(&request["d"]["requestId"]));
            ids.push(request["d"]["requestId"].clone());
            respond(&mut socket, &request, response);
        }
    });
    // When the read-only probe runs.
    let result = Client::new(config).probe();
    // Then it reports every queried field, without issuing mutations.
    assert_eq!(
        result.unwrap(),
        ObsStatus {
            scenes: vec!["Main".into(), "BRB".into()],
            current_scene: "Main".into(),
            recording: true,
            streaming: false,
        }
    );
    peer.join().unwrap();
}

#[test]
fn ping_is_answered_when_waiting_for_hello() {
    // Given a server that requires pong before sending Hello.
    let (config, peer) = raw_server(|mut socket| {
        socket.send(Message::Ping(vec![1, 2, 3].into())).unwrap();
        assert_eq!(socket.read().unwrap(), Message::Pong(vec![1, 2, 3].into()));
        send(&mut socket, json!({"op":0,"d":{"rpcVersion":1}}));
        assert_eq!(read(&mut socket)["op"], 1);
        send(&mut socket, json!({"op":2,"d":{"negotiatedRpcVersion":1}}));
        let request = read(&mut socket);
        respond(&mut socket, &request, json!({}));
    });
    // When the client executes an action.
    let result = Client::new(config).execute(&ObsAction::ToggleRecording);
    // Then ping was handled without dropping the operation.
    result.unwrap();
    peer.join().unwrap();
}
