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
    assert_eq!(layers.binding("Base", &Trigger::EncoderPress(1)), None);
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
