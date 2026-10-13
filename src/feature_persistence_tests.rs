use crate::{
    desktop_actions::{DesktopAction, ScrollDirection},
    device_control::DeviceAction,
    features::FeatureAction,
    layers::{Action, LayerStore, Trigger},
    obs::ObsAction,
    organization::NavigationAction,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "streamdeck-feature-storage-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).expect("remove test fixture");
        }
    }
}

#[test]
fn feature_bindings_survive_layer_storage_and_reload() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    let features = [
        FeatureAction::Desktop(DesktopAction::Text {
            value: "  exact text\nwith spaces  ".into(),
            enter: false,
        }),
        FeatureAction::Navigation(NavigationAction::NextPage),
        FeatureAction::Device(DeviceAction::AdjustBrightness(-5)),
        FeatureAction::Obs(ObsAction::Scene("Live camera".into())),
        FeatureAction::Timer { seconds: 60 },
        FeatureAction::Desktop(DesktopAction::PlayAudio {
            path: "/tmp/recorded sound.wav".into(),
        }),
        FeatureAction::Desktop(DesktopAction::PlayAudio {
            path: "/tmp/recorded sound.mp3".into(),
        }),
        FeatureAction::Desktop(DesktopAction::PlayAudio {
            path: "/tmp/recorded sound.m4a".into(),
        }),
    ];
    for (index, feature) in features.iter().enumerate() {
        store
            .bind(
                "Base",
                Trigger::KeyPress(u8::try_from(index + 1).unwrap()),
                Action::Feature(feature.clone()),
            )
            .unwrap();
    }
    store.persist().unwrap();

    let reloaded = LayerStore::open_at(&fixture.0).unwrap();

    assert!(reloaded.diagnostics().is_empty());
    assert_eq!(
        reloaded.layers()["Base"].bindings,
        store.layers()["Base"].bindings
    );
}

#[test]
fn explicit_obs_controls_survive_layer_storage_and_reload() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    let actions = [
        ObsAction::StartRecording,
        ObsAction::StopRecording,
        ObsAction::PauseRecording,
        ObsAction::ResumeRecording,
        ObsAction::StartStreaming,
        ObsAction::StopStreaming,
    ];
    for (index, action) in actions.iter().enumerate() {
        store
            .bind(
                "Base",
                Trigger::KeyPress(u8::try_from(index + 1).unwrap()),
                Action::Feature(FeatureAction::Obs(action.clone())),
            )
            .unwrap();
    }
    store.persist().unwrap();
    let reloaded = LayerStore::open_at(&fixture.0).unwrap();
    assert!(reloaded.diagnostics().is_empty());
    for (index, action) in actions.into_iter().enumerate() {
        assert_eq!(
            reloaded.layers()["Base"].bindings
                [&Trigger::KeyPress(u8::try_from(index + 1).unwrap())],
            Action::Feature(FeatureAction::Obs(action))
        );
    }
}

#[test]
fn previous_brightness_and_scroll_records_load_as_typed_actions() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(
        fixture.0.join("layers.conf"),
        concat!(
            "streamdeck-layers-v1\n",
            "active\t42617365\n",
            "layer\t42617365\n",
            "binding\t42617365\tk:1\tbrightness-v1:50\n",
            "binding\t42617365\tr:1:+\tscroll-v1:down:2\n"
        ),
    )
    .unwrap();

    let store = LayerStore::open_at(&fixture.0).unwrap();

    assert!(store.diagnostics().is_empty());
    assert_eq!(
        store.layers()["Base"].bindings[&Trigger::KeyPress(1)],
        Action::Feature(FeatureAction::Device(DeviceAction::Brightness(50)))
    );
    assert_eq!(
        store.layers()["Base"].bindings[&Trigger::EncoderClockwise(1)],
        Action::Feature(FeatureAction::Desktop(DesktopAction::MouseScroll {
            direction: ScrollDirection::Down,
            ticks: 2,
        }))
    );
}

#[test]
fn malformed_extended_records_never_overwrite_saved_configuration() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.0).unwrap();
    let original = concat!(
        "streamdeck-layers-v1\n",
        "layer\t42617365\n",
        "binding\t42617365\tk:1\tbrightness-v1:101\n"
    );
    fs::write(fixture.0.join("layers.conf"), original).unwrap();

    let store = LayerStore::open_at(&fixture.0).unwrap();

    assert!(!store.diagnostics().is_empty());
    assert_eq!(
        fs::read_to_string(fixture.0.join("layers.conf")).unwrap(),
        original
    );
}
