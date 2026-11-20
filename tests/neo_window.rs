//! The Stream Deck Neo's 248x58 information window shows each keypad page's
//! own content, like a touch strip section of the Stream Deck +: a picture
//! and live information (time, now playing, ...), without touch. The picture
//! is the page's `touch-strip-background` artwork; the live information is an
//! `info` row on the page's `window` control in layers.conf. Everything goes
//! through the public API.
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use sdeck_plus::{
    binding_editor::{
        control_number_error, control_summary, control_triggers, events_for_control,
        trigger_from_selection_for, ControlKind, EventKind,
    },
    config_bundle,
    device_model::{DeviceModel, CLASSIC, PLUS, PLUS_XL, XL},
    image_output,
    info::{Glyph, InfoSource, InfoValue},
    info_render,
    layers::{Action, LayerStore, Trigger},
    local_artwork::{ArtworkStore, ArtworkTarget},
    organization::Organization,
    protocol::Event,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sdeck-neo-window-{}-{name}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn neo() -> &'static DeviceModel {
    DeviceModel::by_id("neo").unwrap()
}

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// Models whose display under the keys is a touch strip, or that have none.
fn without_window() -> [&'static DeviceModel; 4] {
    [&PLUS, &PLUS_XL, &CLASSIC, &XL]
}

/// A picture whose left half is red and right half blue.
fn halves(dir: &Path, name: &str, (width, height): (i32, i32)) -> PathBuf {
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, width, height).unwrap();
    image.fill(0x0000ffff);
    image
        .new_subpixbuf(0, 0, width / 2, height)
        .fill(0xff0000ff);
    let path = dir.join(name);
    image.savev(&path, "png", &[]).unwrap();
    path
}

fn pixel(image: &Pixbuf, x: i32, y: i32) -> [u8; 4] {
    let bytes = image.read_pixel_bytes();
    let at = (y * image.rowstride() + x * image.n_channels()) as usize;
    let alpha = if image.has_alpha() {
        bytes[at + 3]
    } else {
        255
    };
    [bytes[at], bytes[at + 1], bytes[at + 2], alpha]
}

fn clock() -> InfoValue {
    InfoValue {
        detail: "Thu 19 Nov".into(),
        ..InfoValue::text(Glyph::None, "10:24")
    }
}

#[test]
fn the_window_is_a_control_of_the_neo_only() {
    let model = neo();
    assert!(model.has_info_window());
    assert_eq!(model.zones(), 0);
    for model in without_window() {
        assert!(!model.has_info_window(), "{}", model.id);
    }
    assert_eq!(
        events_for_control(ControlKind::Window),
        [EventKind::Display]
    );
    assert_eq!(
        trigger_from_selection_for(model, ControlKind::Window, 1, EventKind::Display),
        Ok(Trigger::Window)
    );
    assert!(trigger_from_selection_for(model, ControlKind::Window, 2, EventKind::Display).is_err());
    assert!(
        trigger_from_selection_for(model, ControlKind::Window, 1, EventKind::ButtonPress).is_err()
    );
    assert_eq!(control_number_error(model, ControlKind::Window, 1), None);
    assert_eq!(
        control_number_error(model, ControlKind::Window, 2).unwrap(),
        "The Stream Deck Neo has one information window."
    );
    for other in without_window() {
        assert!(
            trigger_from_selection_for(other, ControlKind::Window, 1, EventKind::Display).is_err(),
            "{}",
            other.id
        );
        assert_eq!(
            control_number_error(other, ControlKind::Window, 1).unwrap(),
            format!("The {} has no information window.", other.name)
        );
    }
}

#[test]
fn each_keypad_page_keeps_its_own_window_information_in_window_rows() {
    let fixture = Fixture::new("rows");
    let mut store = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    let mut organization = Organization::open_at(&fixture.0, &store).unwrap();
    let page = organization.create_page(&mut store).unwrap();
    for (layer, source) in [
        ("Base", InfoSource::Clock),
        (page.as_str(), InfoSource::NowPlaying),
    ] {
        store.bind(layer, Trigger::Window, Action::None).unwrap();
        store
            .set_info(layer, &Trigger::Window, Some(source))
            .unwrap();
    }
    store.persist().unwrap();
    organization.persist().unwrap();
    let text = fs::read_to_string(fixture.0.join("layers.conf")).unwrap();
    let base = hex("Base");
    for row in [
        format!("binding\t{base}\twindow\tnone"),
        format!("info\t{base}\twindow\tclock"),
        format!("binding\t{}\twindow\tnone", hex(&page)),
        format!("info\t{}\twindow\tnow-playing", hex(&page)),
    ] {
        assert!(text.lines().any(|line| line == row), "{row} in\n{text}");
    }
    let reopened = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    assert!(reopened.conflicts().is_empty() && reopened.diagnostics().is_empty());
    assert_eq!(
        reopened.info_source("Base", &Trigger::Window),
        Some(InfoSource::Clock)
    );
    assert_eq!(
        reopened.info_source(&page, &Trigger::Window),
        Some(InfoSource::NowPlaying)
    );
    // The reader polls what the windows show.
    let sources = reopened.info_sources();
    assert!(sources.contains(&InfoSource::Clock) && sources.contains(&InfoSource::NowPlaying));
    assert_eq!(
        control_summary(&reopened, &page, ControlKind::Window, 1),
        "Live information: Now playing"
    );
    let mut cleared = reopened.clone();
    cleared.set_info("Base", &Trigger::Window, None).unwrap();
    assert_eq!(
        control_summary(&cleared, "Base", ControlKind::Window, 1),
        "No live information"
    );
}

#[test]
fn the_window_runs_nothing_and_exists_only_on_a_model_with_one() {
    let fixture = Fixture::new("refusals");
    let mut store = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    assert_eq!(
        store
            .bind("Base", Trigger::Window, Action::Emit("x".into()))
            .unwrap_err()
            .to_string(),
        "the information window runs no action"
    );
    // Live information needs the window's own entry first, like a key's.
    assert!(store
        .set_info("Base", &Trigger::Window, Some(InfoSource::Clock))
        .is_err());
    assert_eq!(store.binding("Base", &Trigger::Window), None);
    for model in without_window() {
        let root = fixture.0.join(model.id);
        let mut other = LayerStore::open_at_model(&root, model).unwrap();
        assert_eq!(
            other
                .bind("Base", Trigger::Window, Action::None)
                .unwrap_err()
                .to_string(),
            "this device has no information window",
            "{}",
            model.id
        );
    }
    // No input ever reaches the window: no event names it.
    store.bind("Base", Trigger::Window, Action::None).unwrap();
    for index in 0..12 {
        for pressed in [true, false] {
            let event = Event::Key { index, pressed };
            assert_ne!(
                store.matching_binding(&event).map(|(trigger, _)| trigger),
                Some(Trigger::Window)
            );
        }
    }
}

#[test]
fn removing_the_window_clears_its_information_and_undo_restores_it() {
    let fixture = Fixture::new("remove");
    let mut store = LayerStore::open_at_model(&fixture.0, neo()).unwrap();
    store.bind("Base", Trigger::Window, Action::None).unwrap();
    store
        .set_info("Base", &Trigger::Window, Some(InfoSource::Weather))
        .unwrap();
    assert_eq!(
        control_triggers(&store, "Base", ControlKind::Window, 1).unwrap(),
        [Trigger::Window]
    );
    let removed = store
        .unbind_control("Base", ControlKind::Window, 1)
        .unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(store.binding("Base", &Trigger::Window), None);
    assert_eq!(store.info_source("Base", &Trigger::Window), None);
    store.restore_removed(&removed).unwrap();
    assert_eq!(
        store.info_source("Base", &Trigger::Window),
        Some(InfoSource::Weather)
    );
}

#[test]
fn the_window_picture_belongs_to_the_keypad_page_and_the_plus_strip_to_its_touch_page() {
    let fixture = Fixture::new("owners");
    for model in [neo(), &PLUS] {
        let root = fixture.0.join(model.id);
        let mut store = LayerStore::open_at_model(&root, model).unwrap();
        let mut organization = Organization::open_at(&root, &store).unwrap();
        let page = organization.create_page(&mut store).unwrap();
        let mut artwork =
            ArtworkStore::open_at_for_layer_model(&root, Some("Base"), model).unwrap();
        store
            .initialize_touchscreens(&organization, &mut artwork)
            .unwrap();
        for layer in ["Base", page.as_str()] {
            let owner = store.artwork_owner(layer, ArtworkTarget::TouchStripBackground);
            if model.has_info_window() {
                assert_eq!(owner, layer, "the window follows the keypad page");
            } else {
                assert!(owner.starts_with("@touchscreen:"), "{owner}");
            }
            assert_eq!(store.artwork_owner(layer, ArtworkTarget::Keypad(1)), layer);
        }
    }
}

#[test]
fn the_window_picture_draws_live_information_beside_its_icon_over_the_artwork() {
    let fixture = Fixture::new("render");
    let model = neo();
    let artwork = halves(&fixture.0, "window.png", (248, 58));
    let volume = sdeck_plus::info::parse_volume("Volume: 0.45").unwrap();
    let image =
        info_render::compose_window_image_for(model, &volume, Some(&artwork), Duration::ZERO)
            .unwrap();
    assert_eq!((image.width(), image.height()), (248, 58));
    // The artwork shows through around the reading.
    assert_eq!(pixel(&image, 1, 1), [255, 0, 0, 255]);
    assert_eq!(pixel(&image, 246, 56), [0, 0, 255, 255]);
    // The cyan speaker sits in the left part, the value and bar beside it.
    let cyan = |x0: i32, x1: i32| {
        (x0..x1)
            .flat_map(|x| (0..58).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                let [red, green, blue, _] = pixel(&image, x, y);
                red < 160 && green > 180 && blue > 200
            })
            .count()
    };
    assert!(cyan(0, 56) > 40, "icon at the left: {}", cyan(0, 56));
    let white = (60..248)
        .flat_map(|x| (0..58).map(move |y| (x, y)))
        .filter(|&(x, y)| {
            pixel(&image, x, y)[..3]
                .iter()
                .all(|channel| *channel > 220)
        })
        .count();
    assert!(white > 30, "value text beside the icon: {white}");
    // Without artwork the picture is transparent around the reading.
    let bare =
        info_render::compose_window_image_for(model, &clock(), None, Duration::ZERO).unwrap();
    assert_eq!(pixel(&bare, 0, 0)[3], 0);
    // A long playing title scrolls in the window, a short one does not.
    let title = |headline: &str| InfoValue {
        detail: "Queen".into(),
        ..InfoValue::text(Glyph::Play, headline)
    };
    let (full, column) =
        info_render::window_marquee_extent(model, &title("Bohemian Rhapsody (Remastered 2011)"))
            .expect("a long title scrolls");
    assert!(full > column && column > 100.0, "{full} {column}");
    assert_eq!(
        info_render::window_marquee_extent(model, &title("So What")),
        None
    );
    assert!(info_render::compose_window_image_for(&PLUS, &clock(), None, Duration::ZERO).is_err());
}

#[test]
fn live_information_reaches_the_window_as_a_turned_full_window_picture() {
    let fixture = Fixture::new("device");
    let model = neo();
    let artwork = halves(&fixture.0, "window.png", (248, 58));
    let target = ArtworkTarget::TouchStripBackground;
    let jpeg = image_output::prepare_info_jpeg_for(
        model,
        target,
        &clock(),
        (Some(&artwork), None),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(jpeg[..2], [0xFF, 0xD8]);
    let device = Pixbuf::from_read(std::io::Cursor::new(jpeg.clone())).unwrap();
    assert_eq!((device.width(), device.height()), (248, 58));
    // Turned half way: the red left half of the artwork is on the right.
    let [red, _, blue, _] = pixel(&device, 244, 4);
    assert!(red > 200 && blue < 60, "{red} {blue}");
    let [red, _, blue, _] = pixel(&device, 3, 53);
    assert!(red < 60 && blue > 200, "{red} {blue}");
    // The picture the window preview shows, turned back, is what the device shows.
    let preview =
        info_render::compose_window_image_for(model, &clock(), Some(&artwork), Duration::ZERO)
            .unwrap();
    let turned = image_output::rotated(&device, model.strip_rotation()).unwrap();
    for (x, y) in [(10, 10), (124, 29), (200, 45)] {
        let (shown, sent) = (pixel(&preview, x, y), pixel(&turned, x, y));
        for channel in 0..3 {
            assert!(
                shown[channel].abs_diff(sent[channel]) < 40,
                "{x},{y}: {shown:?} {sent:?}"
            );
        }
    }
    let reports = image_output::encode_reports_for(model, target, &jpeg).unwrap();
    assert!(reports.iter().all(|report| report.0[..2] == [0x02, 0x0B]));
    // A touch strip keeps refusing live information on its whole background.
    for model in [&PLUS, &PLUS_XL] {
        assert_eq!(
            image_output::prepare_info_jpeg_for(
                model,
                target,
                &clock(),
                (None, None),
                Duration::ZERO
            )
            .unwrap_err(),
            "live information cannot replace the touch strip background"
        );
    }
}

#[test]
fn bundles_carry_each_pages_window_picture_and_information() {
    let fixture = Fixture::new("bundle");
    let source = fixture.0.join("source");
    let model = neo();
    let mut store = LayerStore::open_at_model(&source, model).unwrap();
    let mut organization = Organization::open_at(&source, &store).unwrap();
    let page = organization.create_page(&mut store).unwrap();
    store.bind(&page, Trigger::Window, Action::None).unwrap();
    store
        .set_info(&page, &Trigger::Window, Some(InfoSource::Clock))
        .unwrap();
    store.persist().unwrap();
    organization.persist().unwrap();
    let mut artwork = ArtworkStore::open_at_for_layer_model(&source, Some("Base"), model).unwrap();
    let picture = halves(&fixture.0, "window.png", (248, 58));
    artwork
        .assign_in_layer(&page, ArtworkTarget::TouchStripBackground, &picture)
        .unwrap();
    let bundle = fixture.0.join("neo.sdeck-plus.json");
    config_bundle::export_for(&source, &bundle, model).unwrap();
    let target = fixture.0.join("target");
    config_bundle::import_for(&bundle, &target, model).unwrap();
    let imported = LayerStore::open_at_model(&target, model).unwrap();
    assert!(imported.conflicts().is_empty());
    assert_eq!(
        imported.info_source(&page, &Trigger::Window),
        Some(InfoSource::Clock)
    );
    let images = ArtworkStore::open_at_for_layer_model(&target, Some("Base"), model).unwrap();
    assert!(images
        .assigned_path_in_layer(&page, ArtworkTarget::TouchStripBackground)
        .is_some());
    assert!(images
        .assigned_path_in_layer("Base", ArtworkTarget::TouchStripBackground)
        .is_none());
}
