//! Behavior-preservation goldens for the Stream Deck + and generic-shape
//! checks that use the Stream Deck + XL table.

use sdeck_plus::{
    binding_editor::{
        control_summary, control_triggers, trigger_from_selection_for, ControlKind, EventKind,
    },
    config_bundle,
    device_model::{DeviceModel, Rotation, PLUS, PLUS_XL},
    features::{FeatureAction, TouchscreenAction},
    image_output::{self, ImageDimensions},
    info::InfoSource,
    key_hold::{KeyHoldRecognizer, HOLD_DURATION},
    layers::{Action, LayerStore, Trigger},
    local_artwork::{ArtworkStore, ArtworkTarget},
    organization::Organization,
    protocol::{decode_report, decode_report_for, Event},
};
use std::{
    fs,
    path::{Path, PathBuf},
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
            "sdeck-device-model-{}-{name}-{unique}",
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

/// FNV-1a, enough to pin bytes without storing them.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Remove the nanosecond stamp that names copied assets.
fn without_stamps(text: &str) -> String {
    text.split('\n')
        .map(|line| match line.rfind('-') {
            Some(dash)
                if line[dash + 1..]
                    .split('.')
                    .next()
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) =>
            {
                format!(
                    "{}-STAMP.{}",
                    &line[..dash],
                    line[dash + 1..].split_once('.').map_or("", |p| p.1)
                )
            }
            _ => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn picture(dir: &Path, name: &str, width: i32, height: i32, shade: u8) -> PathBuf {
    use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, width, height).unwrap();
    for y in 0..height {
        for x in 0..width {
            image.put_pixel(
                x as u32,
                y as u32,
                (x * 255 / width) as u8,
                (y * 255 / height) as u8,
                shade,
                255,
            );
        }
    }
    let path = dir.join(name);
    image.savev(&path, "png", &[]).unwrap();
    path
}

fn pixels(image: &gtk::gdk_pixbuf::Pixbuf) -> u64 {
    fnv(&image.read_pixel_bytes())
}

fn store_snapshot() -> Vec<(String, String)> {
    let fixture = Fixture::new("stores");
    let root = fixture.0.clone();
    let mut store = LayerStore::open_at(&root).unwrap();
    let mut organization = Organization::open_at(&root, &store).unwrap();
    let page = organization.create_page(&mut store).unwrap();
    organization.create_folder(&mut store, "Tools").unwrap();
    let mut artwork = ArtworkStore::open_at_for_layer(&root, Some("Base")).unwrap();
    store
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    store.create("Work").unwrap();
    for n in 1..=8 {
        store
            .bind(
                "Base",
                Trigger::KeyPress(n),
                Action::Emit(format!("key {n}")),
            )
            .unwrap();
    }
    store
        .bind("Base", Trigger::KeyHold(3), Action::Emit("hold".into()))
        .unwrap();
    for n in 1..=4 {
        store
            .bind(
                "Base",
                Trigger::EncoderPress(n),
                Action::Emit(format!("dial {n}")),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::EncoderClockwise(n),
                Action::Emit(format!("cw {n}")),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::Rotation(n, -2),
                Action::Emit(format!("rot {n}")),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::ZoneTap(n),
                Action::Emit(format!("tap {n}")),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::ZonePress(n),
                Action::Emit(format!("press {n}")),
            )
            .unwrap();
    }
    store
        .bind(
            "Base",
            Trigger::EncoderCounterClockwise(2),
            Action::SwitchLayer("Work".into()),
        )
        .unwrap();
    store
        .bind(
            "Base",
            Trigger::TouchSwipe(sdeck_plus::layers::SwipeDirection::Left),
            Action::Feature(FeatureAction::Touchscreen(TouchscreenAction::NextPage)),
        )
        .unwrap();
    store
        .set_info("Base", &Trigger::KeyPress(2), Some(InfoSource::Clock))
        .unwrap();
    store
        .set_info("Base", &Trigger::ZoneTap(4), Some(InfoSource::Clock))
        .unwrap();
    store.swap_keys("Base", 1, 8).unwrap();
    store
        .bind(&page, Trigger::KeyPress(5), Action::Emit("page".into()))
        .unwrap();
    organization.sync(&store);

    let art = fixture.0.join("art");
    fs::create_dir(&art).unwrap();
    let key = picture(&art, "key.png", 120, 120, 40);
    let strip = picture(&art, "strip.png", 800, 100, 90);
    let zone = picture(&art, "zone.png", 200, 100, 140);
    artwork
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &key)
        .unwrap();
    artwork
        .assign_in_layer("Base", ArtworkTarget::Keypad(8), &key)
        .unwrap();
    artwork
        .assign_in_layer("Base", ArtworkTarget::TouchStripBackground, &strip)
        .unwrap();
    artwork
        .assign_in_layer("Base", ArtworkTarget::EncoderZone(4), &zone)
        .unwrap();
    let text = sdeck_plus::button_text::ButtonText {
        text: "Hi".into(),
        ..Default::default()
    };
    artwork
        .set_button_text_in_layer("Base", ArtworkTarget::Keypad(3), text)
        .unwrap();
    artwork.swap_keys_in_layer("Base", 1, 3).unwrap();
    store.persist().unwrap();

    [
        "layers.conf",
        "touchscreen.json",
        "organization.json",
        "artwork.conf",
    ]
    .into_iter()
    .map(|name| {
        (
            name.to_owned(),
            without_stamps(
                &fs::read_to_string(root.join(name)).unwrap_or_else(|_| "<absent>".into()),
            ),
        )
    })
    .collect()
}

fn report(command: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![1, command, payload.len() as u8, (payload.len() >> 8) as u8];
    bytes.extend_from_slice(payload);
    bytes
}

fn decode_snapshot() -> String {
    let reports = [
        report(0, &[1, 0, 1, 0, 0, 0, 0, 1]),
        report(0, &[0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1]),
        report(0, &[2, 1, 1]),
        report(3, &[0, 1, 0, 1, 0]),
        report(3, &[0, 1, 1, 1, 1, 1, 1]),
        report(3, &[1, 2, 0xfe, 0, 1]),
        report(3, &[1, 0, 0, 0, 0]),
        report(3, &[2, 0, 0, 0, 0]),
        report(3, &[0, 1]),
        report(2, &[1, 1, 0x4b, 0, 0x23, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        report(2, &[1, 1, 0x4b, 0, 0x23, 0]),
        report(2, &[2, 0, 1, 0, 2, 0]),
        report(2, &[3, 0, 1, 0, 2, 0, 0x20, 3, 100, 0]),
        report(2, &[3, 0, 1, 0, 2, 0, 3, 0, 4]),
        report(9, &[1]),
        vec![2, 0, 1, 0, 1],
        vec![1, 0, 8, 0, 1],
    ];
    reports
        .iter()
        .map(|bytes| format!("{:?}", decode_report(bytes)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn image_snapshot() -> Vec<(String, String)> {
    use gtk::gdk_pixbuf::Pixbuf;
    let fixture = Fixture::new("images");
    let dir = fixture.0.clone();
    let key = picture(&dir, "key.png", 300, 200, 30);
    let strip = picture(&dir, "strip.png", 1600, 200, 60);
    let zone = picture(&dir, "zone.png", 400, 200, 120);
    let mut out = Vec::new();
    let mut add = |name: String, value: String| out.push((name, value));
    let plain = sdeck_plus::button_text::ButtonText::default();
    let key_image = image_output::compose_button_image(Some(&key), &plain).unwrap();
    add(
        "compose_button".into(),
        format!(
            "{}x{} {:016x}",
            key_image.width(),
            key_image.height(),
            pixels(&key_image)
        ),
    );
    let blank = image_output::compose_button_image(None, &plain).unwrap();
    add(
        "compose_button_blank".into(),
        format!("{:016x}", pixels(&blank)),
    );
    for (zone_n, previous, next) in [
        (1, true, false),
        (2, true, true),
        (4, false, true),
        (4, true, true),
    ] {
        let image: Pixbuf =
            image_output::compose_touchscreen_zone(zone_n, Some(&zone), None, previous, next)
                .unwrap();
        add(
            format!("zone {zone_n} {previous} {next}"),
            format!(
                "{}x{} {:016x}",
                image.width(),
                image.height(),
                pixels(&image)
            ),
        );
        let jpeg = image_output::prepare_touchscreen_zone_jpeg(
            zone_n,
            None,
            Some(&strip),
            None,
            previous,
            next,
        )
        .unwrap();
        add(
            format!("zone_jpeg {zone_n} {previous} {next}"),
            format!("{} {:016x}", jpeg.len(), fnv(&jpeg)),
        );
    }
    for target in [
        ArtworkTarget::Keypad(1),
        ArtworkTarget::Keypad(8),
        ArtworkTarget::TouchStripBackground,
        ArtworkTarget::EncoderZone(1),
        ArtworkTarget::EncoderZone(4),
    ] {
        let size = image_output::dimensions(target).unwrap();
        add(
            format!("dimensions {target:?}"),
            format!("{}x{}", size.width, size.height),
        );
        let source = match target {
            ArtworkTarget::Keypad(_) => &key,
            ArtworkTarget::TouchStripBackground => &strip,
            ArtworkTarget::EncoderZone(_) => &zone,
        };
        let jpeg = image_output::prepare_jpeg(target, source).unwrap();
        add(
            format!("jpeg {target:?}"),
            format!("{} {:016x}", jpeg.len(), fnv(&jpeg)),
        );
        let clear = image_output::prepare_clear_jpeg(target).unwrap();
        add(
            format!("clear {target:?}"),
            format!("{} {:016x}", clear.len(), fnv(&clear)),
        );
        let payload: Vec<u8> = (0..2500u32).map(|n| (n * 7 % 251) as u8).collect();
        let reports = image_output::encode_reports(target, &payload).unwrap();
        let bytes: Vec<u8> = reports.iter().flat_map(|r| r.0.clone()).collect();
        add(
            format!("reports {target:?}"),
            format!("{} {:016x}", reports.len(), fnv(&bytes)),
        );
    }
    for zone_n in 1..=4u8 {
        let jpeg = image_output::prepare_info_jpeg(
            ArtworkTarget::EncoderZone(zone_n),
            &InfoSource::Clock.placeholder(),
            None,
            Some(&strip),
        );
        add(
            format!("info_zone {zone_n}"),
            jpeg.map(|j| format!("{} {:016x}", j.len(), fnv(&j)))
                .unwrap_or_else(|e| e),
        );
    }
    let _ = ImageDimensions {
        width: 0,
        height: 0,
    };
    out
}

/// (file, length, FNV-1a) captured from the code before the device model existed.
const STORE_GOLDENS: [(&str, usize, u64); 4] = [
    ("layers.conf", 592, 0xfebd8870a63c11a6),
    ("touchscreen.json", 2640, 0x85827dc5189d9e6b),
    ("organization.json", 8, 0x6bf2329923a4d83c),
    ("artwork.conf", 372, 0x20b9baa42882f08f),
];

const DECODE_GOLDEN: u64 = 0x92decf52e2e0c44b;

/// (image name, observed value) captured before the device model existed.
const IMAGE_GOLDENS: [(&str, &str); 34] = [
    ("compose_button", "120x120 6ec57d0a2f1d6ed5"),
    ("compose_button_blank", "35e96d4cbd0af225"),
    ("zone 1 true false", "200x100 7ca86c1164897c9a"),
    ("zone_jpeg 1 true false", "4310 4d88e6375bceaddd"),
    ("zone 2 true true", "200x100 52aa9938a2e60de8"),
    ("zone_jpeg 2 true true", "3301 916f8f8bb3429566"),
    ("zone 4 false true", "200x100 72d005d8b3ac5db5"),
    ("zone_jpeg 4 false true", "4507 075d6d57a6e04293"),
    ("zone 4 true true", "200x100 72d005d8b3ac5db5"),
    ("zone_jpeg 4 true true", "4507 075d6d57a6e04293"),
    ("dimensions Keypad(1)", "120x120"),
    ("jpeg Keypad(1)", "2827 05d0720179c64c62"),
    ("clear Keypad(1)", "1021 e4f0a26789990b3f"),
    ("reports Keypad(1)", "3 755e123e71b2a5aa"),
    ("dimensions Keypad(8)", "120x120"),
    ("jpeg Keypad(8)", "2827 05d0720179c64c62"),
    ("clear Keypad(8)", "1021 e4f0a26789990b3f"),
    ("reports Keypad(8)", "3 1ce607a003fba165"),
    ("dimensions TouchStripBackground", "800x100"),
    ("jpeg TouchStripBackground", "11253 6362b51689510135"),
    ("clear TouchStripBackground", "2902 3d71bba8729d41e2"),
    ("reports TouchStripBackground", "3 d40878b34ecf1ac6"),
    ("dimensions EncoderZone(1)", "200x100"),
    ("jpeg EncoderZone(1)", "3802 0e7ec20ebe90e96b"),
    ("clear EncoderZone(1)", "1196 37d982041e60b36f"),
    ("reports EncoderZone(1)", "3 5256895a6a19930d"),
    ("dimensions EncoderZone(4)", "200x100"),
    ("jpeg EncoderZone(4)", "3802 0e7ec20ebe90e96b"),
    ("clear EncoderZone(4)", "1196 37d982041e60b36f"),
    ("reports EncoderZone(4)", "3 0800f64f84ffd73b"),
    ("info_zone 1", "4385 4a76faaca24d7215"),
    ("info_zone 2", "4393 34d9238e06207d90"),
    ("info_zone 3", "4486 f119590f90b1cfe8"),
    ("info_zone 4", "4581 2cd3ba4bc4011c97"),
];

#[test]
fn plus_store_files_stay_byte_identical() {
    let actual = store_snapshot();
    assert_eq!(actual.len(), STORE_GOLDENS.len());
    for ((name, text), (golden, length, hash)) in actual.iter().zip(STORE_GOLDENS) {
        assert_eq!(name, golden);
        assert_eq!((text.len(), fnv(text.as_bytes())), (length, hash), "{name}");
    }
}

#[test]
fn plus_reports_decode_to_the_same_events() {
    assert_eq!(fnv(decode_snapshot().as_bytes()), DECODE_GOLDEN);
}

#[test]
fn plus_images_and_output_reports_stay_byte_identical() {
    let actual = image_snapshot();
    assert_eq!(actual.len(), IMAGE_GOLDENS.len());
    for ((name, value), (golden, expected)) in actual.iter().zip(IMAGE_GOLDENS) {
        assert_eq!(name, golden);
        assert_eq!(value, expected, "{name}");
    }
}

fn xl_store(fixture: &Fixture) -> LayerStore {
    LayerStore::open_at_model(&fixture.0, &PLUS_XL).unwrap()
}

fn emit(text: &str) -> Action {
    Action::Emit(text.into())
}

#[test]
fn model_tables_describe_both_devices() {
    assert_eq!((PLUS.keys(), PLUS.dials, PLUS.zones), (8, 4, 4));
    assert_eq!((PLUS_XL.keys(), PLUS_XL.dials, PLUS_XL.zones), (36, 6, 6));
    assert_eq!(
        (PLUS.key_px, PLUS.strip_px, PLUS.zone_px),
        (120, (800, 100), (200, 100))
    );
    assert_eq!(
        (PLUS_XL.key_px, PLUS_XL.strip_px, PLUS_XL.zone_px),
        (112, (1200, 100), (200, 100))
    );
    assert_eq!(
        (PLUS.key_rotation, PLUS_XL.key_rotation),
        (Rotation::None, Rotation::Ccw90)
    );
    assert_eq!(PLUS_XL.strip_rotation, Rotation::Ccw90);
    assert_eq!((PLUS.id, PLUS_XL.id), ("plus", "plus-xl"));
    assert_eq!((PLUS.pid, PLUS_XL.pid), (0x0084, 0x00C6));
}

#[test]
fn models_are_found_by_pid_and_id() {
    assert_eq!(DeviceModel::by_pid(0x0084), Some(&PLUS));
    assert_eq!(DeviceModel::by_pid(0x00C6), Some(&PLUS_XL));
    assert_eq!(DeviceModel::by_pid(0x0080), None);
    assert_eq!(DeviceModel::by_id("plus-xl"), Some(&PLUS_XL));
    assert_eq!(DeviceModel::by_id("XL"), None);
    assert_eq!(DeviceModel::default_model(), &PLUS);
    assert_eq!(DeviceModel::all().len(), 2);
}

#[test]
fn control_ranges_and_zone_geometry_follow_the_model() {
    assert!(PLUS.is_valid_key(8) && !PLUS.is_valid_key(9) && !PLUS.is_valid_key(0));
    assert!(PLUS_XL.is_valid_key(36) && !PLUS_XL.is_valid_key(37));
    assert!(PLUS_XL.is_valid_dial(6) && !PLUS_XL.is_valid_dial(7));
    assert!(PLUS.is_valid_zone(4) && !PLUS.is_valid_zone(5));
    assert!(PLUS_XL.is_valid_zone(6) && !PLUS_XL.is_valid_zone(7));
    let offsets: Vec<_> = PLUS_XL.zone_numbers().map(|n| PLUS_XL.zone_x(n)).collect();
    assert_eq!(offsets, [0, 200, 400, 600, 800, 1000]);
    assert_eq!(PLUS_XL.zone_index_from_x(0), Some(1));
    assert_eq!(PLUS_XL.zone_index_from_x(1199), Some(6));
    assert_eq!(PLUS_XL.zone_index_from_x(1200), None);
    assert_eq!(PLUS.zone_index_from_x(799), Some(4));
    assert_eq!(PLUS.zone_index_from_x(800), None);
}

#[test]
fn xl_reports_decode_with_36_keys_and_6_dials() {
    let mut keys = vec![0u8; 36];
    keys[0] = 1;
    keys[35] = 1;
    let events = decode_report_for(&PLUS_XL, &report(0, &keys));
    assert_eq!(events.len(), 36);
    assert_eq!(
        events[35],
        Event::Key {
            index: 35,
            pressed: true
        }
    );
    // The Stream Deck + still reads only its own eight keys.
    assert_eq!(decode_report_for(&PLUS, &report(0, &keys)).len(), 8);
    assert_eq!(decode_report(&report(0, &keys)).len(), 8);

    let events = decode_report_for(&PLUS_XL, &report(3, &[0, 0, 0, 0, 0, 0, 1]));
    assert_eq!(events.len(), 6);
    assert_eq!(
        events[5],
        Event::EncoderButton {
            index: 5,
            pressed: true
        }
    );
    let events = decode_report_for(&PLUS_XL, &report(3, &[1, 0, 0, 0, 0, 0, 0xff]));
    assert_eq!(
        events,
        [Event::Rotation {
            index: 5,
            ticks: -1
        }]
    );
    // Four dials' worth of payload is too short for six dials.
    assert!(decode_report_for(&PLUS_XL, &report(3, &[0, 1, 1, 1, 1])).is_empty());
}

#[test]
fn xl_touch_reports_with_the_longer_payload_decode() {
    let mut tap = vec![1, 1, 0xa0, 0x04, 0x23, 0];
    tap.resize(0x10, 0);
    assert_eq!(
        decode_report_for(&PLUS_XL, &report(2, &tap)),
        [Event::TouchTap { x: 1184, y: 35 }]
    );
}

#[test]
fn xl_image_sizes_and_report_headers_follow_the_model() {
    let size = |target| image_output::dimensions_for(&PLUS_XL, target).unwrap();
    assert_eq!(
        (
            size(ArtworkTarget::Keypad(36)).width,
            size(ArtworkTarget::Keypad(36)).height
        ),
        (112, 112)
    );
    assert_eq!(
        size(ArtworkTarget::TouchStripBackground),
        ImageDimensions {
            width: 1200,
            height: 100
        }
    );
    assert_eq!(
        size(ArtworkTarget::EncoderZone(6)),
        ImageDimensions {
            width: 200,
            height: 100
        }
    );
    assert!(image_output::dimensions_for(&PLUS_XL, ArtworkTarget::Keypad(37)).is_err());
    assert!(image_output::dimensions_for(&PLUS_XL, ArtworkTarget::EncoderZone(7)).is_err());
    assert!(image_output::dimensions_for(&PLUS, ArtworkTarget::Keypad(9)).is_err());
    assert!(image_output::dimensions_for(&PLUS, ArtworkTarget::EncoderZone(5)).is_err());

    let key =
        image_output::encode_reports_for(&PLUS_XL, ArtworkTarget::Keypad(36), &[7; 3]).unwrap();
    assert_eq!(key[0].0.len(), 1024);
    assert_eq!(&key[0].0[..8], &[2, 7, 35, 1, 3, 0, 0, 0]);
    for (zone, x) in [
        (1u8, 0u16),
        (2, 200),
        (3, 400),
        (4, 600),
        (5, 800),
        (6, 1000),
    ] {
        let reports = image_output::encode_reports_for(
            &PLUS_XL,
            ArtworkTarget::EncoderZone(zone),
            &[9; 1009],
        )
        .unwrap();
        assert_eq!(reports.len(), 2);
        let first = &reports[0].0;
        assert_eq!(&first[..2], &[2, 0x0c]);
        assert_eq!(u16::from_le_bytes([first[2], first[3]]), x);
        assert_eq!(u16::from_le_bytes([first[6], first[7]]), 200);
        assert_eq!(u16::from_le_bytes([first[8], first[9]]), 100);
    }
    let strip =
        image_output::encode_reports_for(&PLUS_XL, ArtworkTarget::TouchStripBackground, &[1])
            .unwrap();
    assert_eq!(&strip[0].0[..8], &[2, 0x0b, 0, 1, 1, 0, 0, 0]);
    assert!(image_output::encode_reports_for(&PLUS_XL, ArtworkTarget::Keypad(37), &[1]).is_err());
    assert!(image_output::encode_reports_for(&PLUS, ArtworkTarget::EncoderZone(5), &[1]).is_err());
}

#[test]
fn xl_images_are_composed_at_the_model_sizes() {
    use gtk::gdk_pixbuf::Pixbuf;
    let plain = sdeck_plus::button_text::ButtonText::default();
    let key = image_output::compose_button_image_for(&PLUS_XL, None, &plain).unwrap();
    assert_eq!((key.width(), key.height()), (112, 112));
    let mut text = plain.clone();
    text.text = "Hi".into();
    let labelled = image_output::compose_button_image_for(&PLUS_XL, None, &text).unwrap();
    assert_eq!((labelled.width(), labelled.height()), (112, 112));
    assert_ne!(pixels(&labelled), pixels(&key));
    let icon = sdeck_plus::binding_icons::compose_key_image_for(
        &PLUS_XL,
        sdeck_plus::binding_icons::BindingIcon::Command,
        "Run",
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!((icon.width(), icon.height()), (112, 112));
    // The strip is rotated 90 degrees before upload, so its image is portrait.
    let clear = image_output::prepare_clear_jpeg_for(&PLUS_XL, ArtworkTarget::TouchStripBackground)
        .unwrap();
    let decoded = Pixbuf::from_read(std::io::Cursor::new(clear)).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (100, 1200));
    let clear = image_output::prepare_clear_jpeg_for(&PLUS_XL, ArtworkTarget::Keypad(36)).unwrap();
    let decoded = Pixbuf::from_read(std::io::Cursor::new(clear)).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (112, 112));
}

#[test]
fn page_indicators_sit_on_the_first_and_last_section_of_the_model() {
    let alpha = |model: &DeviceModel, zone: u8, x: u32| {
        let image = image_output::compose_touchscreen_zone_for(
            model,
            zone,
            None,
            None,
            (true, true),
            std::time::Duration::ZERO,
        )
        .unwrap();
        image.pixel_bytes().unwrap()[((50 * image.rowstride() as u32 + x * 4) + 3) as usize]
    };
    // The previous chevron sits near the left edge of the first section only.
    assert_eq!(alpha(&PLUS_XL, 1, 8), 255);
    assert_eq!(alpha(&PLUS_XL, 2, 8), 0);
    // The next chevron belongs to the last section: 4 on the +, 6 on the XL.
    assert_eq!(alpha(&PLUS, 4, 192), 255);
    assert_eq!(alpha(&PLUS_XL, 4, 192), 0);
    assert_eq!(alpha(&PLUS_XL, 6, 192), 255);
}

#[test]
fn layer_store_validates_controls_by_model() {
    let plus = Fixture::new("plus-store");
    let mut store = LayerStore::open_at(&plus.0).unwrap();
    assert_eq!(store.model(), &PLUS);
    assert!(store.bind("Base", Trigger::KeyPress(8), emit("ok")).is_ok());
    assert!(store
        .bind("Base", Trigger::KeyPress(9), emit("no"))
        .is_err());
    assert!(store
        .bind("Base", Trigger::EncoderPress(5), emit("no"))
        .is_err());
    assert!(store.bind("Base", Trigger::ZoneTap(5), emit("no")).is_err());
    assert!(store.swap_keys("Base", 1, 9).is_err());

    let xl = Fixture::new("xl-store");
    let mut store = xl_store(&xl);
    assert_eq!(store.model(), &PLUS_XL);
    for trigger in [
        Trigger::KeyPress(36),
        Trigger::KeyHold(36),
        Trigger::EncoderPress(6),
        Trigger::EncoderClockwise(6),
        Trigger::Rotation(6, 2),
        Trigger::ZoneTap(6),
        Trigger::ZonePress(6),
    ] {
        store
            .bind("Base", trigger.clone(), emit("ok"))
            .unwrap_or_else(|e| panic!("{trigger:?}: {e}"));
    }
    for trigger in [
        Trigger::KeyPress(37),
        Trigger::KeyHold(37),
        Trigger::EncoderPress(7),
        Trigger::Rotation(7, 1),
        Trigger::ZoneTap(7),
        Trigger::ZonePress(7),
    ] {
        assert!(
            store.bind("Base", trigger.clone(), emit("no")).is_err(),
            "{trigger:?}"
        );
    }
    store.swap_keys("Base", 36, 1).unwrap();
    store.persist().unwrap();

    let reopened = xl_store(&xl);
    assert!(
        reopened.diagnostics().is_empty(),
        "{:?}",
        reopened.diagnostics()
    );
    assert_eq!(
        reopened.matching_action(&Event::Key {
            index: 0,
            pressed: true
        }),
        Some(emit("ok"))
    );
    // The same files are not a valid Stream Deck + configuration. Opening a
    // root with the wrong model drops the rows that model cannot hold.
    let as_plus = LayerStore::open_at(&xl.0).unwrap();
    assert!(!as_plus.conflicts().is_empty());
}

#[test]
fn touch_events_map_to_the_sections_of_the_model() {
    let fixture = Fixture::new("xl-touch");
    let mut store = xl_store(&fixture);
    store
        .bind("Base", Trigger::ZoneTap(6), emit("six"))
        .unwrap();
    store
        .bind("Base", Trigger::ZonePress(5), emit("five"))
        .unwrap();
    assert_eq!(
        store.matching_action(&Event::TouchTap { x: 1100, y: 10 }),
        Some(emit("six"))
    );
    assert_eq!(
        store.matching_action(&Event::TouchPress { x: 900, y: 10 }),
        Some(emit("five"))
    );
    assert_eq!(
        store.matching_action(&Event::TouchTap { x: 1200, y: 10 }),
        None
    );
    assert_eq!(
        Trigger::from_event_for(&PLUS_XL, &Event::TouchTap { x: 1000, y: 0 }),
        Some(Trigger::ZoneTap(6))
    );
    assert_eq!(
        Trigger::from_event_for(&PLUS, &Event::TouchTap { x: 1000, y: 0 }),
        Some(Trigger::TouchTap)
    );
    assert_eq!(
        Trigger::from_event(&Event::TouchTap { x: 799, y: 0 }),
        Some(Trigger::ZoneTap(4))
    );
}

#[test]
fn artwork_targets_are_listed_per_model() {
    let plus = ArtworkTarget::all_for(&PLUS);
    assert_eq!(plus, ArtworkTarget::ALL);
    assert_eq!(ArtworkTarget::touch_for(&PLUS), ArtworkTarget::TOUCH);
    let ids: Vec<_> = plus.iter().map(|t| t.id_in(&PLUS)).collect();
    assert_eq!(ids[0], "keypad-1");
    assert_eq!(ids[7], "keypad-8");
    assert_eq!(ids[8], "touch-strip-background");
    assert_eq!(ids[12], "encoder-zone-4");

    let xl = ArtworkTarget::all_for(&PLUS_XL);
    assert_eq!(xl.len(), 36 + 1 + 6);
    assert_eq!(ArtworkTarget::touch_for(&PLUS_XL).len(), 7);
    assert_eq!(ArtworkTarget::Keypad(36).id_in(&PLUS_XL), "keypad-36");
    assert_eq!(
        ArtworkTarget::EncoderZone(6).id_in(&PLUS_XL),
        "encoder-zone-6"
    );
    assert_eq!(ArtworkTarget::Keypad(36).id_in(&PLUS), "");
    assert_eq!(
        ArtworkTarget::from_id_in(&PLUS_XL, "keypad-36"),
        Some(ArtworkTarget::Keypad(36))
    );
    assert_eq!(ArtworkTarget::from_id_in(&PLUS, "keypad-36"), None);
    assert_eq!(ArtworkTarget::from_id_in(&PLUS, "encoder-zone-5"), None);
    assert!(ArtworkTarget::Keypad(36).is_valid_for(&PLUS_XL));
    assert!(!ArtworkTarget::Keypad(9).is_valid_for(&PLUS));
}

#[test]
fn artwork_store_accepts_only_the_controls_of_its_model() {
    let fixture = Fixture::new("xl-artwork");
    let image = picture(&fixture.0, "key.png", 112, 112, 20);
    let root = fixture.0.join("data");
    let mut xl = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &PLUS_XL).unwrap();
    assert_eq!(xl.model(), &PLUS_XL);
    xl.assign_in_layer("Base", ArtworkTarget::Keypad(36), &image)
        .unwrap();
    xl.assign_in_layer("Base", ArtworkTarget::EncoderZone(6), &image)
        .unwrap();
    let text = sdeck_plus::button_text::ButtonText {
        text: "Hi".into(),
        ..Default::default()
    };
    xl.set_button_text_in_layer("Base", ArtworkTarget::Keypad(30), text)
        .unwrap();
    xl.swap_keys_in_layer("Base", 30, 36).unwrap();
    assert!(xl.swap_keys_in_layer("Base", 1, 37).is_err());
    assert!(xl
        .assign_in_layer("Base", ArtworkTarget::Keypad(37), &image)
        .is_err());
    let manifest = fs::read_to_string(root.join("artwork.conf")).unwrap();
    assert!(manifest.contains("keypad-36") && manifest.contains("encoder-zone-6"));

    let reopened = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &PLUS_XL).unwrap();
    assert!(reopened
        .assigned_path_in_layer("Base", ArtworkTarget::EncoderZone(6))
        .is_some());

    let mut plus = ArtworkStore::open_at_for_layer(fixture.0.join("plus"), Some("Base")).unwrap();
    assert!(plus
        .assign_in_layer("Base", ArtworkTarget::Keypad(9), &image)
        .is_err());
    assert!(plus.swap_keys_in_layer("Base", 1, 9).is_err());
    assert!(plus
        .assign_in_layer("Base", ArtworkTarget::Keypad(8), &image)
        .is_ok());
}

#[test]
fn key_hold_recognizer_is_sized_by_the_model() {
    let fixture = Fixture::new("xl-hold");
    let mut store = xl_store(&fixture);
    store
        .bind("Base", Trigger::KeyPress(36), emit("tap"))
        .unwrap();
    store
        .bind("Base", Trigger::KeyHold(36), emit("hold"))
        .unwrap();
    let mut xl = KeyHoldRecognizer::new(&PLUS_XL);
    let start = std::time::Duration::ZERO;
    assert!(xl.update(&store, 35, true, start).is_empty());
    assert_eq!(xl.next_deadline(), Some(HOLD_DURATION));
    let held = xl.update(&store, 35, false, HOLD_DURATION);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].trigger, Trigger::KeyHold(36));
    // Key 36 does not exist on the default (Stream Deck +) recognizer.
    let mut plus = KeyHoldRecognizer::default();
    assert!(plus.update(&store, 35, true, start).is_empty());
    assert_eq!(plus.next_deadline(), None);
}

#[test]
fn editor_triggers_and_summaries_follow_the_model() {
    let fixture = Fixture::new("xl-editor");
    let mut store = xl_store(&fixture);
    assert_eq!(
        trigger_from_selection_for(
            &PLUS_XL,
            ControlKind::Encoder,
            6,
            EventKind::EncoderClockwise
        )
        .unwrap(),
        Trigger::EncoderClockwise(6)
    );
    assert!(
        trigger_from_selection_for(&PLUS_XL, ControlKind::Encoder, 7, EventKind::EncoderPress)
            .is_err()
    );
    assert_eq!(
        trigger_from_selection_for(&PLUS_XL, ControlKind::Button, 36, EventKind::ButtonPress)
            .unwrap(),
        Trigger::KeyPress(36)
    );
    assert_eq!(
        trigger_from_selection_for(&PLUS_XL, ControlKind::Touch, 6, EventKind::TouchTap).unwrap(),
        Trigger::ZoneTap(6)
    );
    assert!(
        trigger_from_selection_for(&PLUS_XL, ControlKind::Touch, 7, EventKind::TouchTap).is_err()
    );
    assert!(trigger_from_selection_for(&PLUS, ControlKind::Touch, 5, EventKind::TouchTap).is_err());
    assert!(
        trigger_from_selection_for(&PLUS, ControlKind::Button, 9, EventKind::ButtonPress).is_err()
    );

    let triggers = control_triggers(&store, "Base", ControlKind::Encoder, 6).unwrap();
    assert!(triggers.contains(&Trigger::EncoderCounterClockwise(6)));
    assert!(control_triggers(&store, "Base", ControlKind::Encoder, 7).is_err());
    assert!(control_triggers(&store, "Base", ControlKind::Button, 36)
        .unwrap()
        .contains(&Trigger::KeyHold(36)));

    store
        .bind("Base", Trigger::ZoneTap(6), emit("six"))
        .unwrap();
    let summary = control_summary(&store, "Base", ControlKind::Touch, 0);
    assert!(summary.contains("Section 6 Tap"), "{summary}");
}

#[test]
fn bundles_are_validated_against_the_model_of_the_target_store() {
    let xl = Fixture::new("bundle-xl");
    let mut store = xl_store(&xl);
    store
        .bind("Base", Trigger::KeyPress(36), emit("tap"))
        .unwrap();
    store
        .bind("Base", Trigger::ZoneTap(6), emit("zone"))
        .unwrap();
    store.persist().unwrap();
    let bundle = xl.0.join("out.sdeck-plus.json");
    let source = xl.0.join("source");
    fs::create_dir(&source).unwrap();
    for name in ["layers.conf", "touchscreen.json"] {
        if xl.0.join(name).exists() {
            fs::copy(xl.0.join(name), source.join(name)).unwrap();
        }
    }
    config_bundle::export_for(&source, &bundle, &PLUS_XL).unwrap();
    // A Stream Deck + root refuses controls it does not have.
    assert!(config_bundle::export_for(&source, &xl.0.join("plus.json"), &PLUS).is_err());

    let target = Fixture::new("bundle-target");
    let imported = target.0.join("xl-root");
    config_bundle::import_for(&bundle, &imported, &PLUS_XL).unwrap();
    let store = LayerStore::open_at_model(&imported, &PLUS_XL).unwrap();
    assert_eq!(
        store.matching_action(&Event::Key {
            index: 35,
            pressed: true
        }),
        Some(emit("tap"))
    );
    let refused = target.0.join("plus-root");
    assert!(config_bundle::import_for(&bundle, &refused, &PLUS).is_err());
    assert!(config_bundle::import(&bundle, &refused).is_err());

    // A Stream Deck + bundle stays valid for the Stream Deck +.
    let plus = Fixture::new("bundle-plus");
    let mut store = LayerStore::open_at(&plus.0).unwrap();
    store
        .bind("Base", Trigger::KeyPress(8), emit("eight"))
        .unwrap();
    store.persist().unwrap();
    let bundle = plus.0.join("plus.sdeck-plus.json");
    let source = plus.0.join("source");
    fs::create_dir(&source).unwrap();
    fs::copy(plus.0.join("layers.conf"), source.join("layers.conf")).unwrap();
    config_bundle::export(&source, &bundle).unwrap();
    let restored = target.0.join("plus-restored");
    config_bundle::import(&bundle, &restored).unwrap();
    assert!(LayerStore::open_at(&restored)
        .unwrap()
        .diagnostics()
        .is_empty());
}

#[test]
fn plus_open_of_an_xl_root_drops_xl_rows_and_xl_open_keeps_them() {
    let fixture = Fixture::new("xl-root-hazard");
    let key_36 = Event::Key {
        index: 35,
        pressed: true,
    };
    let mut store = xl_store(&fixture);
    store
        .bind("Base", Trigger::KeyPress(36), emit("xl"))
        .unwrap();
    store.persist().unwrap();
    drop(store);
    let conf = fixture.0.join("layers.conf");
    assert!(fs::read_to_string(&conf).unwrap().contains("k:36"));

    let reopened = xl_store(&fixture);
    assert_eq!(reopened.matching_action(&key_36), Some(emit("xl")));
    drop(reopened);
    assert!(fs::read_to_string(&conf).unwrap().contains("k:36"));

    // Opening an XL root as a Stream Deck + loses the row for good.
    let plus = LayerStore::open_at(&fixture.0).unwrap();
    assert_eq!(plus.matching_action(&key_36), None);
    drop(plus);
    assert!(!fs::read_to_string(&conf).unwrap().contains("k:36"));
}

#[test]
fn xl_touch_tap_with_payload_length_0x10_decodes() {
    let mut bytes = vec![1, 2, 0x10, 0, 1, 0, 0x4c, 0x04, 0x32, 0];
    bytes.extend_from_slice(&[0; 10]);
    assert_eq!(
        decode_report_for(&PLUS_XL, &bytes),
        [Event::TouchTap { x: 1100, y: 50 }]
    );
}

#[test]
fn button_text_errors_name_the_keypad_range_of_the_model() {
    let fixture = Fixture::new("text-errors");
    let text = sdeck_plus::button_text::ButtonText {
        text: "Hi".into(),
        ..Default::default()
    };
    for (model, root, range) in [(&PLUS, "plus", "1..=8"), (&PLUS_XL, "xl", "1..=36")] {
        let mut store =
            ArtworkStore::open_at_for_layer_model(fixture.0.join(root), Some("Base"), model)
                .unwrap();
        let error = store
            .set_button_text_in_layer("Base", ArtworkTarget::TouchStripBackground, text.clone())
            .unwrap_err();
        assert_eq!(error.to_string(), format!("text requires keypad {range}"));
    }
}
