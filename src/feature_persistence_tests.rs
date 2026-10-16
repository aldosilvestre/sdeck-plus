use crate::{
    desktop_actions::{DesktopAction, ScrollDirection},
    device_control::DeviceAction,
    features::FeatureAction,
    info::InfoSource,
    layers::{Action, LayerStore, PageReturn, Trigger},
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
fn none_binding_survives_reload_and_blocks_whole_strip_fallback() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store
        .bind("Base", Trigger::ZoneTap(2), Action::None)
        .unwrap();
    store
        .bind(
            "Base",
            Trigger::TouchTap,
            Action::Command("must not run".into()),
        )
        .unwrap();
    store.persist().unwrap();

    let reloaded = LayerStore::open_at(&fixture.0).unwrap();
    assert!(reloaded.conflicts().is_empty());
    assert_eq!(
        reloaded.matching_action(&crate::protocol::Event::TouchTap { x: 250, y: 0 }),
        Some(Action::None),
    );
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
fn page_return_survives_reload_and_follows_its_binding() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    store.create("Studio").unwrap();
    let value = PageReturn {
        layer: "Studio".into(),
        seconds: 15,
    };
    let first = Trigger::KeyPress(1);
    store
        .bind("Base", first.clone(), Action::Command("run".into()))
        .unwrap();
    store
        .set_return("Base", &first, Some(value.clone()))
        .unwrap();
    assert!(store
        .set_return("Base", &Trigger::KeyPress(2), Some(value.clone()))
        .is_err());
    for (layer, seconds) in [("Missing", 5), ("Studio", 0), ("Studio", 86_401)] {
        let invalid = PageReturn {
            layer: layer.into(),
            seconds,
        };
        assert!(store.set_return("Base", &first, Some(invalid)).is_err());
    }
    store.persist().unwrap();

    let mut reloaded = LayerStore::open_at(&fixture.0).unwrap();
    assert!(reloaded.diagnostics().is_empty());
    assert_eq!(reloaded.page_return("Base", &first), Some(value.clone()));
    reloaded.swap_keys("Base", 1, 2).unwrap();
    assert_eq!(reloaded.page_return("Base", &first), None);
    assert_eq!(
        reloaded.page_return("Base", &Trigger::KeyPress(2)),
        Some(value)
    );
    reloaded.unbind("Base", &Trigger::KeyPress(2)).unwrap();
    assert_eq!(reloaded.page_return("Base", &Trigger::KeyPress(2)), None);

    reloaded
        .bind("Base", first.clone(), Action::Command("run".into()))
        .unwrap();
    reloaded
        .set_return(
            "Base",
            &first,
            Some(PageReturn {
                layer: "Studio".into(),
                seconds: 9,
            }),
        )
        .unwrap();
    reloaded.remove("Studio").unwrap();
    assert_eq!(reloaded.page_return("Base", &first), None);
}

#[test]
fn numeric_page_return_rows_from_earlier_builds_do_not_discard_the_configuration() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    let key = Trigger::KeyPress(1);
    store
        .bind("Base", key.clone(), Action::Command("run".into()))
        .unwrap();
    store.persist().unwrap();
    let path = fixture.0.join("layers.conf");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("return\t42617365\tk:1\t2\t5\n");
    fs::write(&path, text).unwrap();

    let reloaded = LayerStore::open_at(&fixture.0).unwrap();

    assert!(
        reloaded.diagnostics().is_empty(),
        "{:?}",
        reloaded.diagnostics()
    );
    assert_eq!(
        reloaded.layers()["Base"].bindings[&key],
        Action::Command("run".into())
    );
    assert_eq!(reloaded.page_return("Base", &key), None);
}

#[test]
fn live_information_survives_reload_and_follows_its_control() {
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    let key = Trigger::KeyPress(1);
    store
        .bind("Base", key.clone(), Action::Command("run".into()))
        .unwrap();
    store
        .set_info("Base", &key, Some(InfoSource::Volume))
        .unwrap();
    assert!(store
        .set_info("Base", &Trigger::KeyPress(2), Some(InfoSource::Clock))
        .is_err());
    store
        .bind(
            "Base",
            Trigger::Rotation(1, 2),
            Action::Command("run".into()),
        )
        .unwrap();
    assert!(store
        .set_info("Base", &Trigger::Rotation(1, 2), Some(InfoSource::Clock))
        .is_err());
    store
        .bind("Base", Trigger::TouchTap, Action::Command("run".into()))
        .unwrap();
    assert!(store
        .set_info("Base", &Trigger::TouchTap, Some(InfoSource::Clock))
        .is_err());
    let section = Trigger::ZoneTap(4);
    store
        .bind("Base", section.clone(), Action::Command("run".into()))
        .unwrap();
    store
        .set_info("Base", &section, Some(InfoSource::Clock))
        .unwrap();
    assert_eq!(store.zone_info_source("Base", 4), Some(InfoSource::Clock));
    assert!(store
        .bind("Base", Trigger::ZoneTap(5), Action::Command("run".into()))
        .is_err());
    let dial = Trigger::EncoderClockwise(2);
    store
        .bind("Base", dial.clone(), Action::Command("run".into()))
        .unwrap();
    store
        .set_info("Base", &dial, Some(InfoSource::Cpu))
        .unwrap();
    assert_eq!(store.zone_info_source("Base", 2), Some(InfoSource::Cpu));
    assert_eq!(store.zone_info_source("Base", 1), None);
    store.persist().unwrap();

    let mut reloaded = LayerStore::open_at(&fixture.0).unwrap();
    assert!(reloaded.conflicts().is_empty());
    assert_eq!(reloaded.info_source("Base", &key), Some(InfoSource::Volume));
    assert_eq!(reloaded.zone_info_source("Base", 2), Some(InfoSource::Cpu));
    assert_eq!(
        reloaded.info_sources().into_iter().collect::<Vec<_>>(),
        [InfoSource::Volume, InfoSource::Clock, InfoSource::Cpu]
    );
    reloaded.swap_keys("Base", 1, 2).unwrap();
    assert_eq!(reloaded.info_source("Base", &key), None);
    assert_eq!(
        reloaded.info_source("Base", &Trigger::KeyPress(2)),
        Some(InfoSource::Volume)
    );
    reloaded.unbind("Base", &Trigger::KeyPress(2)).unwrap();
    reloaded.unbind("Base", &dial).unwrap();
    assert_eq!(
        reloaded.zone_info_source("Base", 4),
        Some(InfoSource::Clock)
    );
    reloaded.unbind("Base", &Trigger::ZoneTap(4)).unwrap();
    assert!(reloaded.info_sources().is_empty());
}

#[test]
fn touch_taps_choose_their_section_before_the_whole_strip() {
    use crate::protocol::Event;
    let fixture = Fixture::new();
    let mut store = LayerStore::open_at(&fixture.0).unwrap();
    for (trigger, name) in [
        (Trigger::ZoneTap(2), "second"),
        (Trigger::ZonePress(1), "held"),
        (Trigger::TouchTap, "strip"),
    ] {
        store
            .bind("Base", trigger, Action::Command(name.into()))
            .unwrap();
    }
    let command = |event: Event| match store.matching_action(&event) {
        Some(Action::Command(name)) => Some(name),
        _ => None,
    };
    assert_eq!(
        command(Event::TouchTap { x: 250, y: 10 }).as_deref(),
        Some("second")
    );
    assert_eq!(
        command(Event::TouchTap { x: 399, y: 10 }).as_deref(),
        Some("second")
    );
    assert_eq!(
        command(Event::TouchTap { x: 450, y: 10 }).as_deref(),
        Some("strip")
    );
    assert_eq!(
        command(Event::TouchTap { x: 900, y: 10 }).as_deref(),
        Some("strip")
    );
    assert_eq!(
        command(Event::TouchPress { x: 10, y: 10 }).as_deref(),
        Some("held")
    );
    assert_eq!(command(Event::TouchPress { x: 450, y: 10 }), None);
    store.persist().unwrap();
    let reloaded = LayerStore::open_at(&fixture.0).unwrap();
    assert!(reloaded.conflicts().is_empty());
    assert_eq!(reloaded.layers()["Base"].bindings.len(), 3);
}

#[test]
fn unknown_information_rows_are_removed_as_conflicts() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(
        fixture.0.join("layers.conf"),
        concat!(
            "streamdeck-layers-v1\n",
            "layer\t42617365\n",
            "binding\t42617365\tk:1\tcommand:72756e\n",
            "info\t42617365\tk:1\tno-such-source\n"
        ),
    )
    .unwrap();

    let store = LayerStore::open_at(&fixture.0).unwrap();

    assert_eq!(store.conflicts().len(), 1);
    assert_eq!(store.info_source("Base", &Trigger::KeyPress(1)), None);
    assert_eq!(store.layers()["Base"].bindings.len(), 1);
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
fn malformed_extended_records_are_removed_and_logged_without_blocking_startup() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(
        fixture.0.join("layers.conf"),
        concat!(
            "streamdeck-layers-v1\n",
            "layer\t42617365\n",
            "binding\t42617365\tk:1\tbrightness-v1:101\n",
            "binding\t42617365\tk:2\tcommand:72756e\n",
            "binding\t42617365\tk:2\tcommand:6f74686572\n",
            "binding\t42617365\tk:3\tswitch:6e6f7065\n",
            "binding\t6e6f7065\tk:4\tcommand:72756e\n",
            "mystery\trow\n"
        ),
    )
    .unwrap();

    let store = LayerStore::open_at(&fixture.0).unwrap();

    assert!(store.diagnostics().is_empty());
    assert_eq!(store.conflicts().len(), 5);
    let base = &store.layers()["Base"].bindings;
    assert_eq!(base.len(), 1);
    assert_eq!(base[&Trigger::KeyPress(2)], Action::Command("run".into()));
    let log = fs::read_to_string(fixture.0.join(crate::layers::CONFLICT_LOG)).unwrap();
    assert_eq!(log.lines().count(), 5);
    assert!(log.contains("control=k:1") && log.contains("reason=duplicate binding"));
    assert!(log.contains("missing target layer") && log.contains("brightness-v1:101"));
    let rewritten = LayerStore::open_at(&fixture.0).unwrap();
    assert!(rewritten.conflicts().is_empty());
    assert_eq!(rewritten.layers()["Base"].bindings.len(), 1);
}
