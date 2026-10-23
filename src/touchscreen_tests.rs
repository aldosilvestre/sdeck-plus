use crate::{
    info::InfoSource,
    layers::{Action, LayerStore, PageReturn, Trigger},
    local_artwork::{ArtworkStore, ArtworkTarget},
    organization::Organization,
    protocol::Event,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sdeck-touch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn stores(&self) -> (LayerStore, Organization, ArtworkStore) {
        let layers = LayerStore::open_at(&self.0).unwrap();
        let organization = Organization::open_at(&self.0, &layers).unwrap();
        let artwork = ArtworkStore::open_at_for_layer(&self.0, Some(layers.active())).unwrap();
        (layers, organization, artwork)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn touch_configuration_is_profile_owned_across_pages_and_folders() {
    let fixture = Fixture::new();
    let (mut layers, mut organization, mut artwork) = fixture.stores();
    let page = organization.create_page(&mut layers).unwrap();
    let folder = organization.create_folder(&mut layers, "Tools").unwrap();
    layers.create("Studio").unwrap();
    organization.sync(&layers);
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    layers
        .bind("Base", Trigger::ZoneTap(1), Action::Emit("shared".into()))
        .unwrap();
    layers
        .set_info("Base", &Trigger::ZoneTap(1), Some(InfoSource::Clock))
        .unwrap();
    layers
        .set_return(
            "Base",
            &Trigger::ZoneTap(1),
            Some(PageReturn {
                layer: "Studio".into(),
                seconds: 3,
            }),
        )
        .unwrap();
    let owner = layers.touchscreen_owner("Base").unwrap().to_owned();
    for key in ["Base", page.as_str(), folder.as_str()] {
        layers.select(key);
        assert_eq!(
            layers.binding(key, &Trigger::ZoneTap(1)),
            Some(Action::Emit("shared".into()))
        );
        assert_eq!(
            layers.info_source(key, &Trigger::ZoneTap(1)),
            Some(InfoSource::Clock)
        );
        assert_eq!(
            layers.page_return(key, &Trigger::ZoneTap(1)).unwrap().layer,
            "Studio"
        );
        assert_eq!(layers.touchscreen_owner(key), Some(owner.as_str()));
        assert_eq!(
            layers.matching_action(&Event::TouchTap { x: 20, y: 20 }),
            Some(Action::Emit("shared".into()))
        );
    }
    assert_eq!(layers.binding("Studio", &Trigger::ZoneTap(1)), None);
    layers
        .bind(&page, Trigger::KeyPress(1), Action::Emit("keypad".into()))
        .unwrap();
    assert_eq!(layers.binding("Base", &Trigger::KeyPress(1)), None);
    assert_eq!(
        layers.binding(&page, &Trigger::KeyPress(1)),
        Some(Action::Emit("keypad".into()))
    );
    layers
        .bind(&page, Trigger::EncoderPress(1), Action::None)
        .unwrap();
    assert_eq!(
        layers.binding("Base", &Trigger::EncoderPress(1)),
        Some(Action::None)
    );
}

#[test]
fn dials_follow_touch_pages_across_keypad_navigation_and_profiles() {
    let fixture = Fixture::new();
    let (mut layers, mut organization, mut artwork) = fixture.stores();
    let keypad = organization.create_page(&mut layers).unwrap();
    let folder = organization.create_folder(&mut layers, "Tools").unwrap();
    layers.create("Studio").unwrap();
    organization.sync(&layers);
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    let first = layers.touchscreen_owner("Base").unwrap().to_owned();
    let triggers = [
        Trigger::EncoderPress(1),
        Trigger::EncoderClockwise(1),
        Trigger::EncoderCounterClockwise(1),
        Trigger::Rotation(1, 1),
    ];
    for trigger in &triggers {
        layers
            .bind("Base", trigger.clone(), Action::Emit("first".into()))
            .unwrap();
    }
    layers
        .set_info("Base", &triggers[0], Some(InfoSource::Clock))
        .unwrap();
    layers
        .set_return(
            "Base",
            &triggers[0],
            Some(PageReturn {
                layer: "Studio".into(),
                seconds: 3,
            }),
        )
        .unwrap();
    for key in ["Base", keypad.as_str(), folder.as_str()] {
        layers.select(key);
        for trigger in &triggers {
            assert_eq!(
                layers.binding(key, trigger),
                Some(Action::Emit("first".into()))
            );
            assert_eq!(layers.binding_layer(key, trigger).unwrap().name, first);
        }
        assert_eq!(layers.zone_info_source(key, 1), Some(InfoSource::Clock));
        assert_eq!(
            layers.page_return(key, &triggers[0]).unwrap().layer,
            "Studio"
        );
        assert_eq!(
            layers.matching_action(&Event::Rotation { index: 0, ticks: 1 }),
            Some(Action::Emit("first".into()))
        );
    }
    let second = layers.create_touchscreen_page("Base").unwrap();
    assert_eq!(layers.active(), folder);
    assert_eq!(layers.binding(&folder, &triggers[0]), None);
    assert_eq!(layers.zone_info_source(&folder, 1), None);
    assert_eq!(layers.page_return(&folder, &triggers[0]), None);
    layers
        .bind(&folder, triggers[0].clone(), Action::None)
        .unwrap();
    layers
        .bind(
            &folder,
            triggers[1].clone(),
            Action::Emit("directional".into()),
        )
        .unwrap();
    layers
        .bind(&folder, triggers[3].clone(), Action::Emit("exact".into()))
        .unwrap();
    assert_eq!(
        layers.matching_action(&Event::Rotation { index: 0, ticks: 1 }),
        Some(Action::Emit("exact".into()))
    );
    assert_eq!(
        layers.matching_action(&Event::Rotation { index: 0, ticks: 2 }),
        Some(Action::Emit("directional".into()))
    );
    layers.unbind(&folder, &triggers[3]).unwrap();
    assert_eq!(
        layers.matching_action(&Event::Rotation { index: 0, ticks: 1 }),
        Some(Action::Emit("directional".into()))
    );
    assert_eq!(layers.binding("Studio", &triggers[0]), None);
    layers
        .bind("Studio", triggers[0].clone(), Action::Emit("studio".into()))
        .unwrap();
    layers.persist().unwrap();
    organization.persist().unwrap();
    let (mut loaded, organization, mut artwork) = fixture.stores();
    loaded
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(loaded.touchscreen_owner("Base"), Some(second.as_str()));
    assert_eq!(loaded.binding("Base", &triggers[0]), Some(Action::None));
    assert_eq!(
        loaded.binding("Studio", &triggers[0]),
        Some(Action::Emit("studio".into()))
    );
    assert!(loaded.select_touchscreen_page("Base", &first));
    assert_eq!(
        loaded.binding("Base", &triggers[0]),
        Some(Action::Emit("first".into()))
    );
    assert_eq!(loaded.zone_info_source("Base", 1), Some(InfoSource::Clock));
}

#[test]
fn v1_pages_seed_dials_from_profile_root_once_without_changing_touch_or_legacy() {
    let fixture = Fixture::new();
    let (mut layers, mut organization, _artwork) = fixture.stores();
    let keypad = organization.create_page(&mut layers).unwrap();
    layers.create("Studio").unwrap();
    organization.sync(&layers);
    for (key, text) in [
        ("Base", "root"),
        (keypad.as_str(), "keypad"),
        ("Studio", "studio"),
    ] {
        layers
            .bind(key, Trigger::EncoderPress(1), Action::Emit(text.into()))
            .unwrap();
    }
    layers
        .set_info("Base", &Trigger::EncoderPress(1), Some(InfoSource::Clock))
        .unwrap();
    layers
        .set_return(
            "Base",
            &Trigger::EncoderPress(1),
            Some(PageReturn {
                layer: "Studio".into(),
                seconds: 2,
            }),
        )
        .unwrap();
    layers.persist().unwrap();
    organization.persist().unwrap();
    let legacy = fs::read(fixture.0.join("layers.conf")).unwrap();
    let document = serde_json::json!({"version":1,"profiles":[
        {"name":"Base","selected":1,"pages":[
            {"owner":"@touchscreen:1","bindings":[{"trigger":"tap","action":"emit:6564697465642d6669727374"}]},
            {"owner":"@touchscreen:2","bindings":[{"trigger":"tap","action":"emit:6564697465642d7365636f6e64"}]}]},
        {"name":"Studio","selected":0,"pages":[{"owner":"@touchscreen:3","bindings":[]}]}
    ]});
    fs::write(
        fixture.0.join("touchscreen.json"),
        serde_json::to_vec(&document).unwrap(),
    )
    .unwrap();
    let (mut loaded, organization, mut artwork) = fixture.stores();
    assert!(
        loaded.diagnostics().is_empty(),
        "{:?}",
        loaded.diagnostics()
    );
    loaded.select(&keypad);
    assert_eq!(
        loaded.binding("Base", &Trigger::EncoderPress(1)),
        Some(Action::Emit("root".into())),
        "v1 dials remain legacy-owned until initialization completes"
    );
    assert!(
        loaded
            .bind("@touchscreen:1", Trigger::EncoderPress(2), Action::None)
            .is_err(),
        "do not allow encoder rows to be written into an unupgraded v1 page"
    );
    loaded
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(loaded.active(), keypad);
    assert_eq!(loaded.touchscreen_owner("Base"), Some("@touchscreen:2"));
    for (owner, text) in [
        ("@touchscreen:1", "edited-first"),
        ("@touchscreen:2", "edited-second"),
    ] {
        assert_eq!(
            loaded.binding(owner, &Trigger::TouchTap),
            Some(Action::Emit(text.into()))
        );
        assert_eq!(
            loaded.binding(owner, &Trigger::EncoderPress(1)),
            Some(Action::Emit("root".into()))
        );
        assert_eq!(loaded.zone_info_source(owner, 1), Some(InfoSource::Clock));
        assert_eq!(
            loaded
                .page_return(owner, &Trigger::EncoderPress(1))
                .unwrap()
                .seconds,
            2
        );
    }
    assert_eq!(
        loaded.binding("Studio", &Trigger::EncoderPress(1)),
        Some(Action::Emit("studio".into()))
    );
    assert_eq!(fs::read(fixture.0.join("layers.conf")).unwrap(), legacy);
    let upgraded: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.0.join("touchscreen.json")).unwrap()).unwrap();
    assert_eq!(upgraded["version"], 2);
    loaded
        .bind("Base", Trigger::EncoderPress(1), Action::None)
        .unwrap();
    loaded.persist().unwrap();
    let before = fs::read(fixture.0.join("touchscreen.json")).unwrap();
    let (mut reopened, organization, mut artwork) = fixture.stores();
    reopened
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(
        reopened.binding("Base", &Trigger::EncoderPress(1)),
        Some(Action::None)
    );
    assert_eq!(
        fs::read(fixture.0.join("touchscreen.json")).unwrap(),
        before
    );
}

#[test]
fn fresh_migration_distinguishes_dial_variants_with_identical_touch() {
    let fixture = Fixture::new();
    let (mut layers, mut organization, mut artwork) = fixture.stores();
    let keypad = organization.create_page(&mut layers).unwrap();
    layers
        .bind("Base", Trigger::EncoderPress(1), Action::None)
        .unwrap();
    layers
        .bind(
            &keypad,
            Trigger::EncoderPress(1),
            Action::Emit("different".into()),
        )
        .unwrap();
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(layers.touchscreen_pages("Base").len(), 2);
    assert_eq!(
        layers.binding("Base", &Trigger::EncoderPress(1)),
        Some(Action::Emit("different".into()))
    );
}

#[test]
fn independent_touch_pages_survive_restart_without_changing_keypad_selection() {
    let fixture = Fixture::new();
    let (mut layers, mut organization, mut artwork) = fixture.stores();
    let keypad = organization.create_page(&mut layers).unwrap();
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    let first = layers.touchscreen_owner(&keypad).unwrap().to_owned();
    layers
        .bind(&keypad, Trigger::TouchTap, Action::Emit("first".into()))
        .unwrap();
    let second = layers.create_touchscreen_page("Base").unwrap();
    assert_eq!(layers.active(), keypad);
    assert_eq!(layers.binding(&keypad, &Trigger::TouchTap), None);
    layers
        .bind(&keypad, Trigger::TouchTap, Action::Emit("second".into()))
        .unwrap();
    assert!(layers.step_touchscreen_page("Base", false));
    assert_eq!(layers.touchscreen_owner(&keypad), Some(first.as_str()));
    assert!(!layers.step_touchscreen_page("Base", false));
    assert!(layers.step_touchscreen_page("Base", true));
    assert!(!layers.step_touchscreen_page("Base", true));
    assert_eq!(layers.touchscreen_owner(&keypad), Some(second.as_str()));
    assert_eq!(layers.active(), keypad);
    layers.persist().unwrap();
    organization.persist().unwrap();
    let (mut loaded, organization, mut artwork) = fixture.stores();
    loaded
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(loaded.touchscreen_pages("Base").len(), 2);
    assert_eq!(loaded.touchscreen_owner(&keypad), Some(second.as_str()));
    assert_eq!(
        loaded.binding(&keypad, &Trigger::TouchTap),
        Some(Action::Emit("second".into()))
    );
    assert!(loaded.select_touchscreen_page("Base", &first));
    assert_eq!(
        loaded.binding(&keypad, &Trigger::TouchTap),
        Some(Action::Emit("first".into()))
    );
}

#[test]
fn migration_deduplicates_complete_configurations_and_keeps_legacy_data_and_images() {
    use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
    let fixture = Fixture::new();
    let (mut layers, mut organization, mut artwork) = fixture.stores();
    layers
        .bind("Base", Trigger::TouchTap, Action::None)
        .unwrap();
    let page = organization.create_page(&mut layers).unwrap();
    layers.bind(&page, Trigger::TouchTap, Action::None).unwrap();
    layers.set_info(&page, &Trigger::ZoneTap(1), None).unwrap();
    let folder = organization.create_folder(&mut layers, "Tools").unwrap();
    layers
        .bind(&folder, Trigger::TouchTap, Action::None)
        .unwrap();
    layers
        .bind(
            &folder,
            Trigger::ZoneTap(1),
            Action::Emit("different".into()),
        )
        .unwrap();
    layers
        .set_info(&folder, &Trigger::ZoneTap(1), Some(InfoSource::Clock))
        .unwrap();
    layers
        .set_return(
            &folder,
            &Trigger::ZoneTap(1),
            Some(PageReturn {
                layer: "Base".into(),
                seconds: 7,
            }),
        )
        .unwrap();
    let image = fixture.0.join("image.png");
    let pixbuf = Pixbuf::new(Colorspace::Rgb, false, 8, 8, 8).unwrap();
    pixbuf.fill(0x112233ff);
    pixbuf.savev(&image, "png", &[]).unwrap();
    let asset = artwork
        .assign_in_layer(&folder, ArtworkTarget::TouchStripBackground, &image)
        .unwrap();
    layers.persist().unwrap();
    let legacy = fs::read(fixture.0.join("layers.conf")).unwrap();
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(layers.touchscreen_pages("Base").len(), 2);
    assert_eq!(fs::read(fixture.0.join("layers.conf")).unwrap(), legacy);
    let owner = layers.touchscreen_owner(&folder).unwrap().to_owned();
    assert_eq!(
        artwork.assigned_path_in_layer(&owner, ArtworkTarget::TouchStripBackground),
        Some(asset.clone())
    );
    assert_eq!(
        layers.binding("Base", &Trigger::ZoneTap(1)),
        Some(Action::Emit("different".into()))
    );
    assert_eq!(
        layers
            .page_return("Base", &Trigger::ZoneTap(1))
            .unwrap()
            .seconds,
        7
    );
    assert_eq!(
        layers.info_source("Base", &Trigger::ZoneTap(1)),
        Some(InfoSource::Clock)
    );
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(layers.touchscreen_pages("Base").len(), 2);
    artwork.remove_layer(&folder).unwrap();
    assert!(asset.exists());
    layers.persist().unwrap();
    organization.persist().unwrap();
    let (mut loaded, organization, mut artwork) = fixture.stores();
    loaded
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(loaded.touchscreen_pages("Base").len(), 2);
    assert_eq!(loaded.touchscreen_owner("Base"), Some(owner.as_str()));
    assert_eq!(
        loaded.binding("Base", &Trigger::ZoneTap(1)),
        Some(Action::Emit("different".into()))
    );
}

#[test]
fn interrupted_paired_save_recovers_before_loading_either_store() {
    for step in [1, 2] {
        let fixture = Fixture::new();
        let (mut layers, mut organization, mut artwork) = fixture.stores();
        let page = organization.create_page(&mut layers).unwrap();
        layers
            .initialize_touchscreens(&organization, &mut artwork)
            .unwrap();
        layers
            .bind("Base", Trigger::ZoneTap(1), Action::None)
            .unwrap();
        layers
            .set_return(
                "Base",
                &Trigger::ZoneTap(1),
                Some(PageReturn {
                    layer: page.clone(),
                    seconds: 2,
                }),
            )
            .unwrap();
        layers.persist().unwrap();
        let snapshots = || {
            ["layers.conf", "touchscreen.json"].map(|name| fs::read(fixture.0.join(name)).unwrap())
        };
        let previous = snapshots();
        layers.remove(&page).unwrap();
        crate::layers::INTERRUPT_TOUCH_SAVE.with(|flag| flag.set(step));
        // Unwind skips the ordinary returned-error rollback, as process death does.
        assert!(std::panic::catch_unwind(|| layers.persist()).is_err());
        let expected = if step == 1 { previous } else { snapshots() };
        for _ in 0..2 {
            let reopened = LayerStore::open_at(&fixture.0).unwrap();
            assert!(
                reopened.diagnostics().is_empty(),
                "{:?}",
                reopened.diagnostics()
            );
            assert_eq!(reopened.names().contains(&page), step == 1);
            assert_eq!(snapshots(), expected);
        }
    }
}

#[test]
fn failed_layers_save_does_not_poison_touchscreen_restart() {
    let fixture = Fixture::new();
    let (mut layers, mut organization, mut artwork) = fixture.stores();
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    layers.persist().unwrap();
    organization.persist().unwrap();
    let old_layers = fs::read(fixture.0.join("layers.conf")).unwrap();
    let old_touch = fs::read(fixture.0.join("touchscreen.json")).unwrap();
    let page = organization.create_page(&mut layers).unwrap();
    layers
        .bind("Base", Trigger::ZoneTap(1), Action::None)
        .unwrap();
    layers
        .set_return(
            "Base",
            &Trigger::ZoneTap(1),
            Some(PageReturn {
                layer: page,
                seconds: 2,
            }),
        )
        .unwrap();
    fs::create_dir(fixture.0.join("layers.conf.tmp")).unwrap();
    assert!(layers.persist().is_err());
    assert_eq!(fs::read(fixture.0.join("layers.conf")).unwrap(), old_layers);
    assert_eq!(
        fs::read(fixture.0.join("touchscreen.json")).unwrap(),
        old_touch
    );
    let reopened = LayerStore::open_at(&fixture.0).unwrap();
    assert!(
        reopened.diagnostics().is_empty(),
        "failed save poisoned restart: {:?}",
        reopened.diagnostics()
    );
}

#[test]
fn failed_touch_staging_preserves_layers_and_existing_return_targets() {
    let fixture = Fixture::new();
    let (mut layers, mut organization, mut artwork) = fixture.stores();
    let page = organization.create_page(&mut layers).unwrap();
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    layers
        .bind("Base", Trigger::ZoneTap(1), Action::None)
        .unwrap();
    layers
        .set_return(
            "Base",
            &Trigger::ZoneTap(1),
            Some(PageReturn {
                layer: page.clone(),
                seconds: 2,
            }),
        )
        .unwrap();
    layers.persist().unwrap();
    let old_layers = fs::read(fixture.0.join("layers.conf")).unwrap();
    let old_touch = fs::read(fixture.0.join("touchscreen.json")).unwrap();
    layers.remove(&page).unwrap();
    fs::create_dir(fixture.0.join("touchscreen.json.tmp")).unwrap();
    assert!(layers.persist().is_err());
    assert_eq!(fs::read(fixture.0.join("layers.conf")).unwrap(), old_layers);
    assert_eq!(
        fs::read(fixture.0.join("touchscreen.json")).unwrap(),
        old_touch
    );
    assert!(LayerStore::open_at(&fixture.0)
        .unwrap()
        .diagnostics()
        .is_empty());
}

#[test]
fn failed_touch_publication_rolls_back_the_prepared_layer_replacement() {
    let fixture = Fixture::new();
    let (mut layers, organization, mut artwork) = fixture.stores();
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    layers.persist().unwrap();
    let old_layers = fs::read(fixture.0.join("layers.conf")).unwrap();
    let old_touch = fs::read(fixture.0.join("touchscreen.json")).unwrap();
    layers.create("Unsaved").unwrap();
    // A destination directory allows staging but makes the final rename fail.
    fs::rename(
        fixture.0.join("touchscreen.json"),
        fixture.0.join("original-touchscreen.json"),
    )
    .unwrap();
    fs::create_dir(fixture.0.join("touchscreen.json")).unwrap();
    assert!(layers.persist().is_err());
    assert_eq!(fs::read(fixture.0.join("layers.conf")).unwrap(), old_layers);
    assert_eq!(
        fs::read(fixture.0.join("original-touchscreen.json")).unwrap(),
        old_touch
    );
    fs::remove_dir(fixture.0.join("touchscreen.json")).unwrap();
    fs::rename(
        fixture.0.join("original-touchscreen.json"),
        fixture.0.join("touchscreen.json"),
    )
    .unwrap();
    assert!(LayerStore::open_at(&fixture.0)
        .unwrap()
        .diagnostics()
        .is_empty());
}

#[test]
fn malformed_touch_storage_is_not_replaced_and_rejected_changes_do_not_mutate_state() {
    let fixture = Fixture::new();
    let bad = b"{broken";
    fs::write(fixture.0.join("touchscreen.json"), bad).unwrap();
    let (mut layers, organization, mut artwork) = fixture.stores();
    assert!(!layers.diagnostics().is_empty());
    assert!(layers
        .initialize_touchscreens(&organization, &mut artwork)
        .is_err());
    assert!(layers.persist().is_err());
    assert_eq!(fs::read(fixture.0.join("touchscreen.json")).unwrap(), bad);
    assert!(!fixture.0.join("layers.conf").exists());
}
