//! The device registry: one table describes every documented Elgato model, and
//! these checks run on every entry (Supported or Planned) with no per-model
//! code, so a new table row is covered as soon as it is added.

use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use sdeck_plus::{
    binding_editor::{controls_for, trigger_from_selection_for, ControlKind, EventKind},
    config_bundle,
    device_model::{DeviceModel, ImageFormat, ProtocolFamily, Rotation, Support, PLUS, PLUS_XL},
    device_roots::{data_root_in, owner_of_root},
    image_output,
    layers::{Action, LayerStore, SwipeDirection, Trigger},
    local_artwork::{ArtworkStore, ArtworkTarget},
    organization::Organization,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sdeck-registry-{}-{name}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn picture(dir: &Path, name: &str, width: i32, height: i32) -> PathBuf {
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, width, height).unwrap();
    for y in 0..height {
        for x in 0..width {
            image.put_pixel(
                x as u32,
                y as u32,
                (x * 255 / width) as u8,
                (y * 255 / height) as u8,
                77,
                255,
            );
        }
    }
    let path = dir.join(name);
    image.savev(&path, "png", &[]).unwrap();
    path
}

/// Output-report bytes of the Stream Deck + XL, captured before the registry
/// was generalized, so extending the table cannot move them.
const XL_OUTPUT_GOLDENS: [(&str, usize, u64); 4] = [
    ("key 1", 3, 0xe27f5c5b840aff43),
    ("key 36", 3, 0x1753398d28f257c0),
    ("strip", 18, 0xac2b43a0a8c37d7e),
    ("zone 6", 4, 0x781767bbe8c1338c),
];

#[test]
fn xl_output_reports_stay_byte_identical() {
    let scratch = Scratch::new("xl-golden");
    let source = picture(&scratch.0, "source.png", 1300, 130);
    for ((name, count, hash), target) in XL_OUTPUT_GOLDENS.into_iter().zip([
        ArtworkTarget::Keypad(1),
        ArtworkTarget::Keypad(36),
        ArtworkTarget::TouchStripBackground,
        ArtworkTarget::EncoderZone(6),
    ]) {
        let jpeg = image_output::prepare_jpeg_for(&PLUS_XL, target, &source).unwrap();
        let reports = image_output::encode_reports_for(&PLUS_XL, target, &jpeg).unwrap();
        let bytes: Vec<u8> = reports.iter().flat_map(|r| r.0.iter().copied()).collect();
        assert_eq!((reports.len(), fnv(&bytes)), (count, hash), "{name}");
    }
}

fn each() -> impl Iterator<Item = &'static DeviceModel> {
    DeviceModel::all().iter().copied()
}

fn emit() -> Action {
    Action::Emit("x".into())
}

/// The documented facts, kept apart from the table so a typo in either shows.
/// `(id, family, pids, cols, rows, key px, key rotation, format, sensors)`.
#[allow(clippy::type_complexity)]
const DOCUMENTED: [(
    &str,
    ProtocolFamily,
    &[u16],
    u8,
    u8,
    u32,
    Rotation,
    ImageFormat,
    u8,
); 6] = [
    (
        "plus",
        ProtocolFamily::Expanded,
        &[0x0084],
        4,
        2,
        120,
        Rotation::None,
        ImageFormat::Jpeg,
        0,
    ),
    (
        "plus-xl",
        ProtocolFamily::Expanded,
        &[0x00C6],
        9,
        4,
        112,
        Rotation::Ccw90,
        ImageFormat::Jpeg,
        0,
    ),
    (
        "classic",
        ProtocolFamily::Expanded,
        &[0x006D, 0x0080, 0x00A5, 0x00B9],
        5,
        3,
        72,
        Rotation::Rot180,
        ImageFormat::Jpeg,
        0,
    ),
    (
        "xl",
        ProtocolFamily::Expanded,
        &[0x006C, 0x008F, 0x00BA],
        8,
        4,
        96,
        Rotation::Rot180,
        ImageFormat::Jpeg,
        0,
    ),
    (
        "neo",
        ProtocolFamily::Expanded,
        &[0x009A],
        4,
        2,
        96,
        Rotation::Rot180,
        ImageFormat::Jpeg,
        2,
    ),
    (
        "mini",
        ProtocolFamily::LegacyMini,
        &[0x0063, 0x0090, 0x00B3, 0x00B8],
        3,
        2,
        80,
        Rotation::Cw90,
        ImageFormat::Bmp,
        0,
    ),
];

#[test]
fn the_table_matches_the_documented_facts() {
    assert_eq!(DeviceModel::all().len(), DOCUMENTED.len());
    for (id, family, pids, cols, rows, key_px, rotation, format, sensors) in DOCUMENTED {
        let model = DeviceModel::by_id(id).unwrap_or_else(|| panic!("{id} missing"));
        assert_eq!(model.family, family, "{id}");
        assert_eq!(model.pids, pids, "{id}");
        assert_eq!((model.key_cols, model.key_rows), (cols, rows), "{id}");
        assert_eq!(model.key_px, key_px, "{id}");
        assert_eq!(model.key_rotation, rotation, "{id}");
        assert_eq!(model.image_format, format, "{id}");
        assert_eq!(model.touch_buttons, sensors, "{id}");
    }
    let neo = DeviceModel::by_id("neo").unwrap();
    assert_eq!(neo.strip_px(), (248, 58));
    assert!(neo.has_strip() && !neo.has_touch());
    let mini = DeviceModel::by_id("mini").unwrap();
    assert_eq!((mini.input_report_len, mini.key_header_len), (65, 16));
    assert!(!DeviceModel::by_id("xl").unwrap().has_strip());
    assert!(!DeviceModel::by_id("classic").unwrap().has_touch());
}

#[test]
fn exactly_the_plus_and_the_plus_xl_are_supported_today() {
    let supported: Vec<_> = DeviceModel::supported().map(|model| model.id).collect();
    assert_eq!(supported, ["plus", "plus-xl"]);
    for model in each() {
        assert_eq!(
            model.is_supported(),
            model.support == Support::Supported,
            "{}",
            model.id
        );
    }
    assert_eq!(DeviceModel::default_model(), &PLUS);
}

#[test]
fn ids_are_unique_and_safe_as_one_path_component() {
    let mut seen = std::collections::BTreeSet::new();
    for model in each() {
        assert!(!model.name.is_empty());
        assert!(
            !model.id.is_empty()
                && model
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "{}",
            model.id
        );
        assert!(!model.id.starts_with('-'), "{}", model.id);
        assert!(seen.insert(model.id), "duplicate id {}", model.id);
        assert_eq!(DeviceModel::by_id(model.id), Some(model));
    }
}

#[test]
fn product_ids_are_unique_across_models_and_discovery_sees_supported_ones_only() {
    let mut seen = std::collections::BTreeMap::new();
    for model in each() {
        assert!(!model.pids.is_empty(), "{}", model.id);
        assert_eq!(model.pid(), model.pids[0]);
        for pid in model.pids {
            assert_eq!(seen.insert(*pid, model.id), None, "pid {pid:04X} repeated");
            assert_eq!(DeviceModel::by_pid_any(*pid), Some(model));
            let found = DeviceModel::by_pid_supported(*pid);
            assert_eq!(found, model.is_supported().then_some(model), "{pid:04X}");
            assert_eq!(DeviceModel::by_pid(*pid), found);
        }
    }
}

#[test]
fn every_entry_cites_the_elgato_hid_documentation() {
    for model in each() {
        assert!(
            model
                .doc_url
                .starts_with("https://docs.elgato.com/streamdeck/hid/")
                && model.doc_url.len() > "https://docs.elgato.com/streamdeck/hid/".len(),
            "{}: {}",
            model.id,
            model.doc_url
        );
    }
}

#[test]
fn counts_ranges_and_geometry_agree_with_the_accessors() {
    for model in each() {
        let id = model.id;
        assert!(model.key_cols > 0 && model.key_rows > 0, "{id}");
        assert_eq!(model.keys(), model.key_cols * model.key_rows, "{id}");
        assert_eq!(model.key_inputs(), model.keys() + model.touch_buttons);
        assert_eq!(model.key_numbers().count(), usize::from(model.keys()));
        assert_eq!(model.key_numbers().next(), Some(1));
        assert_eq!(model.key_numbers().last(), Some(model.keys()));
        assert_eq!(model.dial_numbers().count(), usize::from(model.dials));
        assert_eq!(model.zone_numbers().count(), usize::from(model.zones()));
        assert!(!model.is_valid_key(0) && !model.is_valid_key(model.keys() + 1));
        assert!(!model.is_valid_dial(0) && !model.is_valid_dial(model.dials + 1));
        assert!(!model.is_valid_zone(0) && !model.is_valid_zone(model.zones() + 1));
        assert!(model.key_px > 0, "{id}");
        assert!(model.input_report_len > 0 && model.image_report_len > model.key_header_len);
        assert_eq!(model.has_strip(), model.strip.is_some());
        match model.strip {
            None => {
                assert_eq!((model.zones(), model.has_touch()), (0, false), "{id}");
                assert_eq!((model.strip_px(), model.zone_px()), ((0, 0), (0, 0)));
            }
            Some(strip) => {
                assert!(strip.px.0 > 0 && strip.px.1 > 0, "{id}");
                assert_eq!(model.has_touch(), strip.touch);
                assert_eq!(strip.touch, strip.zones > 0, "{id}");
                if strip.zones > 0 {
                    assert_eq!(u32::from(strip.zones) * strip.zone_px.0, strip.px.0, "{id}");
                    assert_eq!(strip.zone_px.1, strip.px.1, "{id}");
                    let last = model.zones();
                    assert_eq!(model.zone_x(last) + strip.zone_px.0, strip.px.0);
                    assert_eq!(model.zone_index_from_x(0), Some(1));
                    assert_eq!(model.zone_index_from_x(strip.px.0 - 1), Some(last));
                }
                assert_eq!(model.zone_index_from_x(strip.px.0), None, "{id}");
            }
        }
        if model.family == ProtocolFamily::Expanded {
            assert_eq!(model.input_header_len, 4, "{id}");
            assert_eq!(model.key_header_len, 8, "{id}");
            assert_eq!(model.zone_header_len, 16, "{id}");
            assert!(usize::from(model.key_inputs()) + 4 <= model.input_report_len);
        }
    }
}

#[test]
fn artwork_targets_follow_the_shape_of_every_model() {
    for model in each() {
        let all = ArtworkTarget::all_for(model);
        let strip = usize::from(model.has_strip());
        assert_eq!(
            all.len(),
            usize::from(model.keys()) + strip + usize::from(model.zones()),
            "{}",
            model.id
        );
        assert_eq!(
            ArtworkTarget::touch_for(model).len(),
            strip + usize::from(model.zones())
        );
        let ids: std::collections::BTreeSet<_> = all.iter().map(|t| t.id_in(model)).collect();
        assert_eq!(ids.len(), all.len(), "{}", model.id);
        for target in &all {
            assert!(target.is_valid_for(model));
            assert_eq!(
                ArtworkTarget::from_id_in(model, &target.id_in(model)),
                Some(*target)
            );
            let size = image_output::dimensions_for(model, *target).unwrap();
            assert!(size.width > 0 && size.height > 0);
        }
        for outside in [
            ArtworkTarget::Keypad(model.keys() + 1),
            ArtworkTarget::EncoderZone(model.zones() + 1),
        ] {
            assert!(!outside.is_valid_for(model));
            assert_eq!(outside.id_in(model), "");
            assert!(image_output::dimensions_for(model, outside).is_err());
        }
        assert_eq!(
            ArtworkTarget::TouchStripBackground.is_valid_for(model),
            model.has_strip()
        );
        assert_eq!(
            image_output::dimensions_for(model, ArtworkTarget::TouchStripBackground).is_ok(),
            model.has_strip()
        );
    }
}

#[test]
fn data_roots_are_one_per_model_and_only_the_plus_keeps_the_legacy_root() {
    let base = Path::new("/data/sdeck-plus");
    let mut roots = std::collections::BTreeSet::new();
    for model in each() {
        let root = data_root_in(base, model);
        assert!(roots.insert(root.clone()), "{}", model.id);
        if model.id == "plus" {
            assert_eq!(root, base);
            assert_eq!(owner_of_root(&root), None);
        } else {
            assert_eq!(root, base.join("devices").join(model.id));
            assert_eq!(owner_of_root(&root), Some(model));
        }
    }
}

#[test]
fn every_model_round_trips_its_extreme_controls_and_rejects_one_past_the_end() {
    let scratch = Scratch::new("stores");
    for model in each() {
        let root = data_root_in(&scratch.0.join("sdeck-plus"), model);
        let mut layers = LayerStore::open_at_model(&root, model).unwrap();
        assert_eq!(layers.model(), model);
        let mut valid = vec![Trigger::KeyPress(model.keys()), Trigger::KeyHold(1)];
        let mut invalid = vec![
            Trigger::KeyPress(0),
            Trigger::KeyPress(model.keys() + 1),
            Trigger::KeyHold(model.keys() + 1),
            Trigger::EncoderPress(model.dials + 1),
            Trigger::EncoderClockwise(model.dials + 1),
            Trigger::Rotation(model.dials + 1, 1),
            Trigger::ZoneTap(model.zones() + 1),
            Trigger::ZonePress(model.zones() + 1),
        ];
        if model.dials > 0 {
            valid.extend([
                Trigger::EncoderPress(model.dials),
                Trigger::EncoderCounterClockwise(model.dials),
                Trigger::Rotation(model.dials, -1),
            ]);
        }
        if model.zones() > 0 {
            valid.extend([
                Trigger::ZoneTap(model.zones()),
                Trigger::ZonePress(model.zones()),
            ]);
        }
        let touch = [
            Trigger::TouchTap,
            Trigger::TouchPress,
            Trigger::TouchFlick,
            Trigger::TouchSwipe(SwipeDirection::Left),
        ];
        if model.has_touch() {
            valid.extend(touch);
        } else {
            invalid.extend(touch);
        }
        for trigger in &valid {
            layers
                .bind("Base", trigger.clone(), emit())
                .unwrap_or_else(|e| panic!("{} {trigger:?}: {e}", model.id));
        }
        for trigger in &invalid {
            assert!(
                layers.bind("Base", trigger.clone(), emit()).is_err(),
                "{} accepted {trigger:?}",
                model.id
            );
        }
        layers.persist().unwrap();
        let reopened = LayerStore::open_at_model(&root, model).unwrap();
        assert!(reopened.diagnostics().is_empty(), "{}", model.id);
        for trigger in &valid {
            assert!(
                reopened.layers()["Base"].bindings.contains_key(trigger),
                "{} lost {trigger:?}",
                model.id
            );
        }
        assert_eq!(reopened.layers()["Base"].bindings.len(), valid.len());
    }
}

#[test]
fn every_model_artwork_store_accepts_its_targets_and_rejects_the_rest() {
    let scratch = Scratch::new("artwork");
    let image = picture(&scratch.0, "picture.png", 64, 64);
    for model in each() {
        let root = data_root_in(&scratch.0.join("sdeck-plus"), model);
        let mut store = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), model).unwrap();
        assert_eq!(store.model(), model);
        for target in ArtworkTarget::all_for(model) {
            store
                .assign_in_layer("Base", target, &image)
                .unwrap_or_else(|e| panic!("{} {target:?}: {e}", model.id));
        }
        for outside in [
            ArtworkTarget::Keypad(model.keys() + 1),
            ArtworkTarget::EncoderZone(model.zones() + 1),
        ] {
            assert!(
                store.assign_in_layer("Base", outside, &image).is_err(),
                "{} accepted {outside:?}",
                model.id
            );
        }
        if !model.has_strip() {
            assert!(store
                .assign_in_layer("Base", ArtworkTarget::TouchStripBackground, &image)
                .is_err());
        }
        assert!(store
            .swap_keys_in_layer("Base", 1, model.keys() + 1)
            .is_err());
        let reopened = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), model).unwrap();
        for target in ArtworkTarget::all_for(model) {
            assert!(
                reopened.assigned_path_in_layer("Base", target).is_some(),
                "{} {target:?}",
                model.id
            );
        }
    }
}

#[test]
fn every_model_bundle_round_trips_and_refuses_another_model() {
    let scratch = Scratch::new("bundles");
    for model in each() {
        let root = data_root_in(&scratch.0.join("source"), model);
        let mut layers = LayerStore::open_at_model(&root, model).unwrap();
        layers
            .bind("Base", Trigger::KeyPress(model.keys()), emit())
            .unwrap();
        layers.persist().unwrap();
        Organization::open_at(&root, &layers)
            .unwrap()
            .persist()
            .unwrap();
        ArtworkStore::open_at_for_layer_model(&root, Some("Base"), model).unwrap();
        let bundle = scratch.0.join(format!("{}.sdeck-plus.json", model.id));
        config_bundle::export_for(&root, &bundle, model).unwrap();
        let target = data_root_in(&scratch.0.join("target"), model);
        config_bundle::import_for(&bundle, &target, model).unwrap();
        let restored = LayerStore::open_at_model(&target, model).unwrap();
        assert!(restored.diagnostics().is_empty(), "{}", model.id);
        assert!(restored.layers()["Base"]
            .bindings
            .contains_key(&Trigger::KeyPress(model.keys())));
        for other in each().filter(|other| other.id != model.id) {
            let elsewhere = data_root_in(&scratch.0.join("refused"), other);
            assert!(
                config_bundle::import_for(&bundle, &elsewhere, other).is_err(),
                "{} bundle imported as {}",
                model.id,
                other.id
            );
        }
    }
}

#[test]
fn the_editor_offers_only_the_controls_a_model_has() {
    for model in each() {
        let controls = controls_for(model);
        assert!(controls.contains(&ControlKind::Button));
        assert_eq!(controls.contains(&ControlKind::Encoder), model.dials > 0);
        assert_eq!(controls.contains(&ControlKind::Touch), model.has_touch());
        let encoder = trigger_from_selection_for(
            model,
            ControlKind::Encoder,
            model.dials.max(1),
            EventKind::EncoderPress,
        );
        assert_eq!(encoder.is_ok(), model.dials > 0, "{}", model.id);
        for event in [
            EventKind::TouchTap,
            EventKind::TouchFlick,
            EventKind::TouchSwipeUp,
        ] {
            let touch = trigger_from_selection_for(model, ControlKind::Touch, 0, event);
            assert_eq!(touch.is_ok(), model.has_touch(), "{} {event:?}", model.id);
        }
        assert!(trigger_from_selection_for(
            model,
            ControlKind::Button,
            model.keys(),
            EventKind::ButtonPress
        )
        .is_ok());
        assert!(trigger_from_selection_for(
            model,
            ControlKind::Button,
            model.keys() + 1,
            EventKind::ButtonPress
        )
        .is_err());
    }
}

/// A 3x2 picture whose six pixels all differ.
fn marker_pixbuf() -> Pixbuf {
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, 3, 2).unwrap();
    for y in 0..2 {
        for x in 0..3 {
            image.put_pixel(x, y, (x * 40 + 10) as u8, (y * 100 + 20) as u8, 7, 255);
        }
    }
    image
}

fn read(image: &Pixbuf, x: i32, y: i32) -> [u8; 2] {
    let at = (y * image.rowstride() + x * image.n_channels()) as usize;
    let bytes = image.read_pixel_bytes();
    [bytes[at], bytes[at + 1]]
}

#[test]
fn rotations_move_every_pixel_to_the_place_its_name_says() {
    let source = marker_pixbuf();
    let (w, h) = (3, 2);
    for rotation in [
        Rotation::None,
        Rotation::Cw90,
        Rotation::Ccw90,
        Rotation::Rot180,
    ] {
        let turned = image_output::rotated(&source, rotation).unwrap();
        let swapped = matches!(rotation, Rotation::Cw90 | Rotation::Ccw90);
        assert_eq!(
            (turned.width(), turned.height()),
            if swapped { (h, w) } else { (w, h) },
            "{rotation:?}"
        );
        for y in 0..h {
            for x in 0..w {
                let (nx, ny) = match rotation {
                    Rotation::None => (x, y),
                    Rotation::Cw90 => (h - 1 - y, x),
                    Rotation::Ccw90 => (y, w - 1 - x),
                    Rotation::Rot180 => (w - 1 - x, h - 1 - y),
                };
                assert_eq!(read(&turned, nx, ny), read(&source, x, y), "{rotation:?}");
            }
        }
    }
}

#[test]
fn rotations_are_a_bijection_with_the_expected_inverses() {
    let source = marker_pixbuf();
    let bytes = |image: &Pixbuf| {
        (
            image.width(),
            image.height(),
            image.read_pixel_bytes().to_vec(),
        )
    };
    let turn = |image: &Pixbuf, rotation| image_output::rotated(image, rotation).unwrap();
    assert_eq!(
        bytes(&turn(&turn(&source, Rotation::Cw90), Rotation::Ccw90)),
        bytes(&source)
    );
    assert_eq!(
        bytes(&turn(&turn(&source, Rotation::Rot180), Rotation::Rot180)),
        bytes(&source)
    );
    let mut around = source.clone();
    for _ in 0..4 {
        around = turn(&around, Rotation::Cw90);
    }
    assert_eq!(bytes(&around), bytes(&source));
    assert_eq!(
        bytes(&turn(&turn(&source, Rotation::Cw90), Rotation::Cw90)),
        bytes(&turn(&source, Rotation::Rot180))
    );
}

#[test]
fn every_expanded_model_clears_every_target_at_its_device_size() {
    for model in each().filter(|model| model.family == ProtocolFamily::Expanded) {
        for target in ArtworkTarget::all_for(model) {
            let size = image_output::dimensions_for(model, target).unwrap();
            let jpeg = image_output::prepare_clear_jpeg_for(model, target).unwrap();
            let decoded = Pixbuf::from_read(std::io::Cursor::new(jpeg.clone())).unwrap();
            let rotation = match target {
                ArtworkTarget::Keypad(_) => model.key_rotation,
                _ => model.strip_rotation(),
            };
            let expected = if matches!(rotation, Rotation::Cw90 | Rotation::Ccw90) {
                (size.height, size.width)
            } else {
                (size.width, size.height)
            };
            assert_eq!(
                (decoded.width() as u32, decoded.height() as u32),
                expected,
                "{} {target:?}",
                model.id
            );
            let reports = image_output::encode_reports_for(model, target, &jpeg).unwrap();
            assert!(reports.iter().all(|r| r.0.len() == model.image_report_len));
            assert_eq!(reports[0].0[0], 0x02);
        }
        assert!(image_output::encode_reports_for(
            model,
            ArtworkTarget::Keypad(model.keys() + 1),
            &[1]
        )
        .is_err());
    }
}

#[test]
fn plus_tables_keep_their_values() {
    assert_eq!((PLUS.keys(), PLUS.dials, PLUS.zones()), (8, 4, 4));
    assert_eq!((PLUS_XL.keys(), PLUS_XL.dials, PLUS_XL.zones()), (36, 6, 6));
    assert_eq!((PLUS.pids, PLUS_XL.pids), (&[0x0084][..], &[0x00C6][..]));
}
