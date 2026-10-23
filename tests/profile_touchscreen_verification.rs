use sdeck_plus::{
    features::{FeatureAction, TouchscreenAction},
    info::InfoSource,
    layers::{Action, LayerStore, Trigger},
    local_artwork::ArtworkStore,
    organization::Organization,
    protocol::Event,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sdeck-dial-public-{}-{name}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn all_four_dials_keep_exact_precedence_and_blank_page_isolation() {
    let fixture = Fixture::new("all-four");
    let p = fixture.0.clone();
    let mut s = LayerStore::open_at(&p).unwrap();
    let mut o = Organization::open_at(&p, &s).unwrap();
    let keypad = o.create_page(&mut s).unwrap();
    let folder = o.create_folder(&mut s, "Tools").unwrap();
    let mut a = ArtworkStore::open_at_for_layer(&p, Some("Base")).unwrap();
    s.initialize_touchscreens(&o, &mut a).unwrap();
    let first = s.touchscreen_owner("Base").unwrap().to_owned();
    for n in 1..=4 {
        for t in [
            Trigger::EncoderPress(n),
            Trigger::EncoderClockwise(n),
            Trigger::EncoderCounterClockwise(n),
            Trigger::Rotation(n, 2),
            Trigger::Rotation(n, -2),
        ] {
            s.bind("Base", t.clone(), Action::Emit(format!("{t:?}")))
                .unwrap();
        }
        s.set_info("Base", &Trigger::EncoderPress(n), Some(InfoSource::Clock))
            .unwrap();
    }
    for key in ["Base", keypad.as_str(), folder.as_str()] {
        s.select(key);
        for n in 1..=4 {
            assert_eq!(s.zone_info_source(key, n), Some(InfoSource::Clock));
            for ticks in [-3, -2, 2, 3] {
                let t = if ticks == -2 || ticks == 2 {
                    Trigger::Rotation(n, ticks)
                } else if ticks > 0 {
                    Trigger::EncoderClockwise(n)
                } else {
                    Trigger::EncoderCounterClockwise(n)
                };
                assert_eq!(
                    s.matching_action(&Event::Rotation {
                        index: usize::from(n - 1),
                        ticks
                    }),
                    Some(Action::Emit(format!("{t:?}")))
                );
                assert_eq!(s.binding_layer(key, &t).unwrap().name, first);
            }
            assert_eq!(
                s.matching_action(&Event::EncoderButton {
                    index: usize::from(n - 1),
                    pressed: true
                }),
                Some(Action::Emit(format!("{:?}", Trigger::EncoderPress(n))))
            );
        }
    }
    let second = s.create_touchscreen_page("Base").unwrap();
    for n in 1..=4 {
        assert_eq!(s.zone_info_source(&folder, n), None);
        for t in [
            Trigger::EncoderPress(n),
            Trigger::EncoderClockwise(n),
            Trigger::EncoderCounterClockwise(n),
            Trigger::Rotation(n, 2),
            Trigger::Rotation(n, -2),
        ] {
            assert_eq!(s.binding(&folder, &t), None);
        }
    }
    s.persist().unwrap();
    for (t, msg) in [
        (Trigger::EncoderPress(0), "encoder index out of range"),
        (Trigger::EncoderClockwise(5), "encoder index out of range"),
        (Trigger::Rotation(4, 0), "rotation ticks cannot be zero"),
    ] {
        let disk = fs::read(p.join("touchscreen.json")).unwrap();
        let state = format!("{s:?}");
        assert_eq!(
            s.bind("Base", t, Action::None).unwrap_err().to_string(),
            msg
        );
        assert_eq!(format!("{s:?}"), state);
        assert_eq!(fs::read(p.join("touchscreen.json")).unwrap(), disk);
    }
    let nav = Action::Feature(FeatureAction::Touchscreen(TouchscreenAction::NextPage));
    for t in [
        Trigger::EncoderPress(4),
        Trigger::EncoderClockwise(4),
        Trigger::KeyPress(1),
    ] {
        let state = format!("{s:?}");
        let disk = fs::read(p.join("touchscreen.json")).unwrap();
        assert_eq!(
            s.bind("Base", t, nav.clone()).unwrap_err().to_string(),
            "touchscreen page actions require a touchscreen gesture"
        );
        assert_eq!(format!("{s:?}"), state);
        assert_eq!(fs::read(p.join("touchscreen.json")).unwrap(), disk);
    }
    s.bind("Base", Trigger::TouchTap, nav).unwrap();
    assert_eq!(s.touchscreen_owner("Base"), Some(second.as_str()));
}

#[test]
fn pending_v1_rejections_preserve_every_byte_and_in_memory_state() {
    let fixture = Fixture::new("v1");
    let p = fixture.0.clone();
    let mut s = LayerStore::open_at(&p).unwrap();
    let o = Organization::open_at(&p, &s).unwrap();
    s.bind(
        "Base",
        Trigger::EncoderPress(4),
        Action::Emit("legacy".into()),
    )
    .unwrap();
    s.persist().unwrap();
    o.persist().unwrap();
    fs::write(p.join("touchscreen.json"), br#"{"version":1,"profiles":[{"name":"Base","selected":0,"pages":[{"owner":"@touchscreen:1","bindings":[]}]}]}"#).unwrap();
    let mut s = LayerStore::open_at(&p).unwrap();
    assert!(s.diagnostics().is_empty());
    for t in [
        Trigger::EncoderPress(4),
        Trigger::EncoderClockwise(4),
        Trigger::EncoderCounterClockwise(4),
        Trigger::Rotation(4, -2),
    ] {
        let disk = ["layers.conf", "touchscreen.json", "organization.json"]
            .map(|f| fs::read(p.join(f)).unwrap());
        let state = format!("{s:?}");
        assert_eq!(
            s.bind("@touchscreen:1", t, Action::None)
                .unwrap_err()
                .to_string(),
            "unknown layer"
        );
        assert_eq!(format!("{s:?}"), state);
        assert_eq!(
            ["layers.conf", "touchscreen.json", "organization.json"]
                .map(|f| fs::read(p.join(f)).unwrap()),
            disk
        );
    }
    assert_eq!(
        s.binding("Base", &Trigger::EncoderPress(4)),
        Some(Action::Emit("legacy".into()))
    );
    let mut a = ArtworkStore::open_at_for_layer(&p, Some("Base")).unwrap();
    s.initialize_touchscreens(&o, &mut a).unwrap();
    assert_eq!(
        s.binding("@touchscreen:1", &Trigger::EncoderPress(4)),
        Some(Action::Emit("legacy".into()))
    );
}
