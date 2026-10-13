use super::*;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "obs-test-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn actions_round_trip_when_serialized() {
    // Given every action shape.
    for action in [
        ObsAction::Scene("Main".into()),
        ObsAction::ToggleRecording,
        ObsAction::ToggleStreaming,
        ObsAction::ToggleMute("Mic".into()),
        ObsAction::SourceVisibility {
            scene: "Main".into(),
            source: "Camera".into(),
            visible: true,
        },
    ] {
        // When persisted as JSON.
        let saved = serde_json::to_string(&action).unwrap();
        // Then deserialization preserves the contract.
        assert_eq!(serde_json::from_str::<ObsAction>(&saved).unwrap(), action);
    }
}

#[test]
fn names_are_rejected_when_invalid() {
    // Given invalid name classes.
    for name in ["", " \t", "mic\n", &"x".repeat(513)] {
        let actions = [
            ObsAction::Scene(name.into()),
            ObsAction::ToggleMute(name.into()),
            ObsAction::SourceVisibility {
                scene: name.into(),
                source: "Camera".into(),
                visible: true,
            },
            ObsAction::SourceVisibility {
                scene: "Main".into(),
                source: name.into(),
                visible: true,
            },
        ];
        // When validation is requested.
        for action in actions {
            let result = action.validate();
            // Then each invalid field fails before connection.
            assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        }
    }
}

#[test]
fn endpoint_is_rejected_when_outside_local_scope() {
    // Given remote, ambiguous, credential-bearing, or unsupported URLs.
    for endpoint in [
        "ws://example.com:4455",
        "ws://192.168.1.1:4455",
        "wss://127.0.0.1:4455",
        "ws://user:secret@localhost:4455",
        "ws://localhost:0",
        "ws://localhost:65536",
        "ws://localhost/other",
        "ws://localhost/?password=secret",
        "ws://localhost:4455#secret",
        "ws://localhost.evil:4455",
        "http://localhost:4455",
        "localhost:4455",
    ] {
        let config = ObsConfig {
            endpoint: endpoint.into(),
            password: "secret".into(),
        };
        // When validated.
        let result = config.validate();
        // Then the boundary rejects it without echoing credentials.
        let error = result.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!error.to_string().contains("secret"));
    }
}

#[test]
fn endpoint_is_accepted_when_explicitly_loopback() {
    // Given IPv4, IPv6, and localhost endpoints.
    for endpoint in [
        "ws://127.0.0.1:4455",
        "ws://127.23.1.2:4455",
        "ws://[::1]:4455",
        "ws://localhost:4455/",
    ] {
        let config = ObsConfig {
            endpoint: endpoint.into(),
            password: String::new(),
        };
        // When validated.
        let result = config.validate();
        // Then every local endpoint is supported.
        result.unwrap();
    }
}

#[test]
fn configuration_is_private_when_saved_and_replaced() {
    // Given an explicit application root and a password.
    let root = Root::new();
    let config = ObsConfig {
        endpoint: "ws://localhost:4456".into(),
        password: "secret".into(),
    };
    // When atomically saving over existing private settings.
    config.save_at(&root.0).unwrap();
    let replacement = ObsConfig {
        password: "replacement".into(),
        ..config
    };
    replacement.save_at(&root.0).unwrap();
    // Then load preserves settings, the file is 0600, and no temps remain.
    assert_eq!(ObsConfig::load_at(&root.0).unwrap(), replacement);
    assert_eq!(
        fs::metadata(root.0.join("obs.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    assert!(!format!("{replacement:?}").contains("replacement"));
}

#[test]
fn missing_configuration_returns_default_when_loaded() {
    // Given a new root.
    let root = Root::new();
    // When loading absent settings.
    let result = ObsConfig::load_at(&root.0);
    // Then the documented local default is used.
    assert_eq!(result.unwrap(), ObsConfig::default());
}

#[test]
fn insecure_or_malformed_configuration_is_rejected_when_loaded() {
    // Given a private valid file.
    for case in ["permissions", "symlink", "json", "oversize"] {
        let root = Root::new();
        ObsConfig::default().save_at(&root.0).unwrap();
        let path = root.0.join("obs.json");
        match case {
            "permissions" => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            "symlink" => {
                fs::rename(&path, root.0.join("target")).unwrap();
                symlink(root.0.join("target"), &path).unwrap();
            }
            "json" => fs::write(&path, br#"{"password":"secret","endpoint":}"#).unwrap(),
            "oversize" => fs::write(&path, vec![b' '; 16_385]).unwrap(),
            _ => unreachable!(),
        }
        // When loading the invalid file.
        let result = ObsConfig::load_at(&root.0);
        // Then validation fails without exposing serialized secrets.
        assert!(!result.unwrap_err().to_string().contains("secret"));
    }
}

#[test]
fn insecure_root_is_rejected_when_saving() {
    // Given a writable-by-others application root.
    let root = Root::new();
    fs::set_permissions(&root.0, fs::Permissions::from_mode(0o777)).unwrap();
    // When saving credentials.
    let result = ObsConfig::default().save_at(&root.0);
    // Then no credentials are written to the untrusted root.
    assert_eq!(
        result.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
}
