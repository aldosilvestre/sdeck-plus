//! The simulator driven by the registry table instead of one hand-picked
//! model. These runs do not make a Planned model supported: they prove the
//! machinery of each protocol family (Expanded, Legacy Mini) works for every
//! documented shape, so promoting a model later is a table edit plus
//! verification.

use super::sim_device::{key_report, Rig, SimDevice};
use super::sim_tests::{connected, is_red, listed, Scratch};
use super::*;
use crate::device_model::{ProtocolFamily, Rotation, Support, MINI};
use crate::layers::Trigger;

/// Expanded-family models, Supported or Planned.
fn expanded() -> impl Iterator<Item = &'static DeviceModel> {
    DeviceModel::all()
        .iter()
        .copied()
        .filter(|model| model.family == ProtocolFamily::Expanded)
}

/// Where the red corner marker of a `(width, height)` source lands once the
/// picture is turned by `rotation` (the marker sits near the top-left).
fn marker_at(rotation: Rotation, (width, height): (i32, i32)) -> (i32, i32) {
    match rotation {
        Rotation::None => (8, 8),
        Rotation::Cw90 => (height - 9, 8),
        Rotation::Ccw90 => (8, width - 9),
        Rotation::Rot180 => (width - 9, height - 9),
    }
}

fn turned((width, height): (u32, u32), rotation: Rotation) -> (u32, u32) {
    match rotation {
        Rotation::Cw90 | Rotation::Ccw90 => (height, width),
        _ => (width, height),
    }
}

fn upload(
    model: &'static DeviceModel,
    name: &str,
    target: ArtworkTarget,
    size: (u32, u32),
) -> super::sim_device::CapturedImage {
    let scratch = Scratch::new(&format!("registry-{}-{name}", model.id));
    let path = scratch.marker("picture.png", size, false);
    let device = SimDevice::new(model);
    let mut rig = Rig::start(&[&device]);
    rig.wait_connection(connected(model));
    rig.artwork
        .send(ArtworkCommand::Assign { target, path })
        .unwrap();
    device.wait_until(|device| device.image_count() == 1);
    rig.stop();
    device.images().remove(0)
}

#[test]
fn discovery_never_matches_a_planned_model() {
    for model in DeviceModel::all()
        .iter()
        .filter(|model| model.support == Support::Planned)
    {
        for pid in model.pids {
            assert_eq!(DeviceModel::by_pid(*pid), None, "{} {pid:04X}", model.id);
            assert_eq!(DeviceModel::by_pid_supported(*pid), None);
            assert!(supported_devices([listed(VID, *pid, "a")]).is_empty());
            assert_eq!(DeviceModel::by_pid_any(*pid), Some(*model));
        }
    }
    let found = supported_devices(
        [0x0060, 0x009A, 0x0063, 0x00C6]
            .into_iter()
            .enumerate()
            .map(|(at, pid)| listed(VID, pid, &format!("path{at}"))),
    );
    assert_eq!(
        found
            .iter()
            .map(|key| (key.path.as_str(), key.model))
            .collect::<Vec<_>>(),
        [("path3", &crate::device_model::PLUS_XL)]
    );
}

#[test]
fn every_expanded_model_decodes_its_keys_and_appended_sensors() {
    for model in expanded() {
        let device = SimDevice::new(model);
        let mut rig = Rig::start(&[&device]);
        rig.wait_connection(connected(model));
        let inputs = usize::from(model.key_inputs());
        for index in [0, inputs - 1] {
            device.send(key_report(model, &[index]));
            let events = rig.inputs(inputs);
            let pressed: Vec<_> = events
                .iter()
                .filter(|event| matches!(event, Event::Key { pressed: true, .. }))
                .collect();
            assert_eq!(
                pressed,
                [&Event::Key {
                    index,
                    pressed: true
                }],
                "{}",
                model.id
            );
            device.send(key_report(model, &[]));
            rig.inputs(inputs);
        }
    }
}

#[test]
fn sensors_press_without_touching_any_binding_or_key_state() {
    let model = DeviceModel::by_id("neo").unwrap();
    assert_eq!((model.keys(), model.touch_buttons), (8, 2));
    let device = SimDevice::new(model);
    let mut rig = Rig::start(&[&device]);
    rig.wait_connection(connected(model));
    device.send(key_report(model, &[8, 9]));
    let events = rig.inputs(10);
    assert!(events.contains(&Event::Key {
        index: 8,
        pressed: true
    }));
    assert!(events.contains(&Event::Key {
        index: 9,
        pressed: true
    }));
    assert!(events[..8]
        .iter()
        .all(|event| matches!(event, Event::Key { pressed: false, .. })));
}

#[test]
fn every_expanded_model_uploads_rotated_key_images_of_its_size() {
    for model in expanded() {
        let side = model.key_px;
        let last = model.keys();
        let image = upload(model, "key", ArtworkTarget::Keypad(last), (side, side));
        assert_eq!(
            (image.command, image.slot),
            (0x07, last - 1),
            "{}",
            model.id
        );
        let decoded = image.decode();
        assert_eq!(
            (decoded.width() as u32, decoded.height() as u32),
            (side, side)
        );
        let (x, y) = marker_at(model.key_rotation, (side as i32, side as i32));
        assert!(is_red(&decoded, x, y), "{} marker at {x},{y}", model.id);
    }
}

#[test]
fn every_expanded_window_is_uploaded_at_its_size_and_rotation() {
    for model in expanded().filter(|model| model.has_strip()) {
        let size = model.strip_px();
        let image = upload(model, "strip", ArtworkTarget::TouchStripBackground, size);
        assert_eq!((image.command, image.slot), (0x0b, 0), "{}", model.id);
        let decoded = image.decode();
        let expected = turned(size, model.strip_rotation());
        assert_eq!(
            (decoded.width() as u32, decoded.height() as u32),
            expected,
            "{}",
            model.id
        );
        let (x, y) = marker_at(model.strip_rotation(), (size.0 as i32, size.1 as i32));
        assert!(is_red(&decoded, x, y), "{} marker at {x},{y}", model.id);
    }
}

/// Elgato's Neo page, "Full Window Image": a 248 x 58 picture, "rotated
/// 180° before upload", as JPEG, through Update Window Image: `02 0B`, a
/// reserved byte, the done flag, the UINT16 chunk size and the UINT16 chunk
/// index (little-endian), then the data, in 1024-byte reports.
#[test]
fn the_neo_window_is_a_248x58_jpeg_turned_half_way_in_full_window_reports() {
    let model = DeviceModel::by_id("neo").unwrap();
    let scratch = Scratch::new("registry-neo-window");
    // Noise makes the picture take several chunks.
    let path = scratch.marker("window.png", (248, 58), true);
    let device = SimDevice::new(model);
    let mut rig = Rig::start(&[&device]);
    rig.wait_connection(connected(model));
    rig.artwork
        .send(ArtworkCommand::Assign {
            target: ArtworkTarget::TouchStripBackground,
            path: path.clone(),
        })
        .unwrap();
    device.wait_until(|device| device.image_count() == 1);
    let writes = device.writes();
    let jpeg =
        image_output::prepare_jpeg_for(model, ArtworkTarget::TouchStripBackground, &path).unwrap();
    let capacity = 1024 - 8;
    assert!(writes.len() >= 2, "{} reports", writes.len());
    assert_eq!(writes.len(), jpeg.len().div_ceil(capacity));
    for (index, report) in writes.iter().enumerate() {
        let start = index * capacity;
        let length = (jpeg.len() - start).min(capacity);
        let last = index + 1 == writes.len();
        let mut header = [0x02, 0x0B, 0x00, u8::from(last), 0, 0, 0, 0];
        header[4..6].copy_from_slice(&(length as u16).to_le_bytes());
        header[6..8].copy_from_slice(&(index as u16).to_le_bytes());
        assert_eq!(report.len(), 1024, "report {index}");
        assert_eq!(report[..8], header, "report {index}");
        assert_eq!(report[8..8 + length], jpeg[start..start + length]);
        assert!(report[8 + length..].iter().all(|byte| *byte == 0));
    }
    let image = device.images().remove(0);
    assert_eq!((image.command, image.slot, image.window), (0x0b, 0, None));
    assert_eq!(image.jpeg[..2], [0xFF, 0xD8], "JPEG");
    let decoded = image.decode();
    assert_eq!((decoded.width(), decoded.height()), (248, 58));
    // The marker of the source's top-left corner lands at the bottom right.
    let (x, y) = marker_at(Rotation::Rot180, (248, 58));
    assert_eq!((x, y), (239, 49));
    assert!(is_red(&decoded, x, y), "marker not turned half way");
    assert!(!is_red(&decoded, 8, 8), "marker left at the top left");
    rig.stop();
}

/// Elgato's Classic page, "Full LCD Image": a 480 x 272 picture, "rotated
/// 180° before upload", as JPEG, through Update Full Screen Image: `02 08`, a
/// reserved byte, the done flag, the UINT16 chunk contents size and the
/// UINT16 chunk index (little-endian), then the data, in 1024-byte reports.
/// A key of the page then shows its own 72 px share of that picture.
#[test]
fn the_classic_background_is_a_480x272_jpeg_turned_half_way_in_full_screen_reports() {
    let model = &crate::device_model::CLASSIC;
    let scratch = Scratch::new("registry-classic-background");
    // Noise makes the picture take several chunks.
    let path = scratch.marker("background.png", (480, 272), true);
    let device = SimDevice::new(model);
    let mut rig = Rig::start(&[&device]);
    rig.wait_connection(connected(model));
    rig.artwork
        .send(ArtworkCommand::Assign {
            target: ArtworkTarget::TouchStripBackground,
            path: path.clone(),
        })
        .unwrap();
    device.wait_until(|device| device.image_count() == 1);
    let writes = device.writes();
    let jpeg =
        image_output::prepare_jpeg_for(model, ArtworkTarget::TouchStripBackground, &path).unwrap();
    let capacity = 1024 - 8;
    assert!(writes.len() >= 2, "{} reports", writes.len());
    assert_eq!(writes.len(), jpeg.len().div_ceil(capacity));
    for (index, report) in writes.iter().enumerate() {
        let start = index * capacity;
        let length = (jpeg.len() - start).min(capacity);
        let last = index + 1 == writes.len();
        let mut header = [0x02, 0x08, 0x00, u8::from(last), 0, 0, 0, 0];
        header[4..6].copy_from_slice(&(length as u16).to_le_bytes());
        header[6..8].copy_from_slice(&(index as u16).to_le_bytes());
        assert_eq!(report.len(), 1024, "report {index}");
        assert_eq!(report[..8], header, "report {index}");
        assert_eq!(report[8..8 + length], jpeg[start..start + length]);
        assert!(report[8 + length..].iter().all(|byte| *byte == 0));
    }
    let image = device.images().remove(0);
    assert_eq!((image.command, image.slot, image.window), (0x08, 0, None));
    assert_eq!(image.jpeg[..2], [0xFF, 0xD8], "JPEG");
    let decoded = image.decode();
    assert_eq!((decoded.width(), decoded.height()), (480, 272));
    // The marker of the source's top-left corner lands at the bottom right.
    let (x, y) = marker_at(Rotation::Rot180, (480, 272));
    assert_eq!((x, y), (471, 263));
    assert!(is_red(&decoded, x, y), "marker not turned half way");
    assert!(!is_red(&decoded, 8, 8), "marker left at the top left");

    // Key 1 sits 11 px from the left and 5 px from the top of the LCD, so its
    // share holds the marker, turned with the key.
    let share = image_output::keypad_background_crop(model, &path, 1).unwrap();
    rig.artwork
        .send(ArtworkCommand::Assign {
            target: ArtworkTarget::Keypad(1),
            path: share,
        })
        .unwrap();
    device.wait_until(|device| device.image_count() == 2);
    let key = device.images().remove(1);
    assert_eq!((key.command, key.slot), (0x07, 0));
    let decoded = key.decode();
    assert_eq!((decoded.width(), decoded.height()), (72, 72));
    // Rows 5..24 and columns 11..24 of the LCD are red: after the half turn
    // they sit at the key's bottom right.
    assert!(is_red(&decoded, 64, 59), "the key's share of the marker");
    assert!(!is_red(&decoded, 8, 8));
    rig.stop();
}

/// A long Now playing title scrolls in the Neo's window on the device as it
/// does on keys and touch sections: after the first picture, the HID owner
/// sends later frames of it, each a whole 0x0B window.
#[test]
fn a_long_playing_title_scrolls_in_the_neo_window() {
    use crate::info::{Glyph, InfoValue};
    let model = DeviceModel::by_id("neo").unwrap();
    let device = SimDevice::new(model);
    let mut rig = Rig::start(&[&device]);
    rig.wait_connection(connected(model));
    let value = InfoValue {
        detail: "Queen".into(),
        ..InfoValue::text(Glyph::Play, "Bohemian Rhapsody (Remastered 2011)")
    };
    rig.artwork
        .send(ArtworkCommand::ComposeInfo {
            target: ArtworkTarget::TouchStripBackground,
            path: None,
            strip_path: None,
            value,
        })
        .unwrap();
    device.wait_until(|device| device.image_count() >= 3);
    let images = device.images();
    assert!(images.iter().all(|image| image.command == 0x0b));
    assert_ne!(
        images[0].jpeg, images[2].jpeg,
        "later frames move the title"
    );
    let decoded = images[2].decode();
    assert_eq!((decoded.width(), decoded.height()), (248, 58));
    rig.stop();
}

#[test]
fn every_touch_section_is_uploaded_to_its_window() {
    for model in expanded().filter(|model| model.has_touch()) {
        let zone = model.zones();
        let image = upload(
            model,
            "zone",
            ArtworkTarget::EncoderZone(zone),
            model.zone_px(),
        );
        assert_eq!(image.command, 0x0c, "{}", model.id);
        let (width, height) = model.zone_px();
        assert_eq!(
            image.window,
            Some((model.zone_x(zone) as u16, 0, width as u16, height as u16))
        );
    }
    assert!(DeviceModel::all()
        .iter()
        .filter(|model| !model.has_touch())
        .all(|model| model.zones() == 0));
}

#[test]
fn shutdown_blanks_every_key_and_the_window_of_any_expanded_model() {
    for model in expanded() {
        let device = SimDevice::new(model);
        let mut rig = Rig::start(&[&device]);
        rig.wait_connection(connected(model));
        rig.stop();
        let images = device.images();
        let mut slots: Vec<_> = images
            .iter()
            .filter(|image| image.command == 0x07)
            .map(|image| usize::from(image.slot))
            .collect();
        slots.sort_unstable();
        assert_eq!(
            slots,
            (0..usize::from(model.keys())).collect::<Vec<_>>(),
            "{}",
            model.id
        );
        let windows = images.iter().filter(|image| image.command == 0x0b).count();
        assert_eq!(windows, usize::from(model.has_strip()), "{}", model.id);
        assert_eq!(device.features().last().unwrap()[..3], [0x03, 0x08, 0]);
    }
}

#[test]
fn every_expanded_model_gets_the_plus_brightness_sleep_and_blank_feature_reports() {
    // `03 08 <level>` padded to 32 bytes, as sent to the + before families.
    let golden = |level: u8| {
        let mut report = vec![0u8; 32];
        report[..3].copy_from_slice(&[0x03, 0x08, level]);
        report
    };
    for model in expanded() {
        let device = SimDevice::new(model);
        let mut rig = Rig::start(&[&device]);
        rig.wait_connection(connected(model));
        rig.controls.send(DeviceAction::Brightness(40)).unwrap();
        device.wait_until(|device| device.features().last() == Some(&golden(40)));
        rig.controls.send(DeviceAction::Sleep).unwrap();
        device.wait_until(|device| device.features().last() == Some(&golden(0)));
        let before_shutdown = device.features().len();
        rig.stop();
        let features = device.features();
        assert_eq!(features[before_shutdown..], [golden(0)], "{}", model.id);
        assert!(
            features.iter().all(|report| *report == golden(report[2])),
            "{} sent {features:02x?}",
            model.id
        );
    }
}

/// Size of an 80x80 24-bit BMP: 54 header bytes and 80 rows of 240 bytes.
const MINI_BMP_LEN: usize = 54 + 80 * 80 * 3;

/// `05 55 AA D1 01 <level>` padded to 32 bytes (Elgato's Mini page).
fn mini_brightness(level: u8) -> Vec<u8> {
    let mut report = vec![0u8; 32];
    report[..6].copy_from_slice(&[0x05, 0x55, 0xAA, 0xD1, 0x01, level]);
    report
}

#[test]
fn a_legacy_mini_connects_and_reports_each_of_its_six_keys() {
    assert_eq!(MINI.family, ProtocolFamily::LegacyMini);
    assert_eq!(MINI.ensure_protocol(), Ok(()));
    let device = SimDevice::new(&MINI);
    let mut rig = Rig::start(&[&device]);
    rig.wait_connection(connected(&MINI));
    for index in 0..6 {
        let report = key_report(&MINI, &[index]);
        assert_eq!((report.len(), report[0], report[1 + index]), (65, 0x01, 1));
        device.send(report);
        let events = rig.inputs(6);
        for (at, event) in events.iter().enumerate() {
            assert_eq!(
                *event,
                Event::Key {
                    index: at,
                    pressed: at == index
                }
            );
        }
        assert_eq!(
            Trigger::from_event_for(&MINI, &events[index]),
            Some(Trigger::KeyPress(index as u8 + 1))
        );
        device.send(key_report(&MINI, &[]));
        assert!(rig
            .inputs(6)
            .iter()
            .all(|event| matches!(event, Event::Key { pressed: false, .. })));
    }
    assert!(!rig.seen.iter().any(|event| matches!(
        event,
        MonitorEvent::Connection(ConnectionState::Unavailable(_))
    )));
}

#[test]
fn a_legacy_mini_key_upload_is_an_80_square_transposed_bmp() {
    let image = upload(&MINI, "key", ArtworkTarget::Keypad(6), (80, 80));
    assert_eq!((image.command, image.slot), (0x01, 5));
    assert_eq!(image.chunks, 20, "19254 bytes in 1008-byte chunks");
    assert_eq!(image.jpeg.len(), MINI_BMP_LEN);
    assert_eq!(image.jpeg[..2], *b"BM");
    let decoded = image.decode();
    assert_eq!((decoded.width(), decoded.height()), (80, 80));
    // Transposed (Ccw90 then a vertical flip), the top-left marker stays at
    // the top-left; tests/mini_protocol.rs pins every pixel of the transpose.
    assert!(is_red(&decoded, 8, 8), "marker not at top-left");
    let (x, y) = marker_at(Rotation::Cw90, (80, 80));
    assert!(!is_red(&decoded, x, y), "marker turned clockwise");
}

#[test]
fn legacy_mini_image_reports_carry_the_header_of_elgatos_mini_page() {
    let scratch = Scratch::new("registry-mini-headers");
    let device = SimDevice::new(&MINI);
    let mut rig = Rig::start(&[&device]);
    rig.wait_connection(connected(&MINI));
    let path = scratch.marker("key.png", (80, 80), true);
    rig.artwork
        .send(ArtworkCommand::Assign {
            target: ArtworkTarget::Keypad(2),
            path: path.clone(),
        })
        .unwrap();
    device.wait_until(|device| device.image_count() == 1);
    let writes = device.writes();
    assert_eq!(writes.len(), 20);
    for (chunk, report) in writes.iter().enumerate() {
        assert_eq!(report.len(), 1024);
        let mut header = [0u8; 16];
        header[..6].copy_from_slice(&[0x02, 0x01, chunk as u8, 0x00, u8::from(chunk == 19), 2]);
        assert_eq!(report[..16], header, "chunk {chunk}");
    }
    // The upload reached the device untouched: the bytes of the encoder.
    let bmp = image_output::prepare_jpeg_for(&MINI, ArtworkTarget::Keypad(2), &path).unwrap();
    let expected = image_output::encode_reports_for(&MINI, ArtworkTarget::Keypad(2), &bmp).unwrap();
    assert_eq!(
        writes,
        expected
            .into_iter()
            .map(|report| report.0)
            .collect::<Vec<_>>()
    );
    rig.stop();
}

#[test]
fn a_legacy_mini_gets_its_own_brightness_and_is_blanked_with_bmp_keys_on_shutdown() {
    let device = SimDevice::new(&MINI);
    let mut rig = Rig::start(&[&device]);
    rig.wait_connection(connected(&MINI));
    rig.controls.send(DeviceAction::Brightness(40)).unwrap();
    device.wait_until(|device| device.features().last() == Some(&mini_brightness(40)));
    rig.controls.send(DeviceAction::Sleep).unwrap();
    device.wait_until(|device| device.features().last() == Some(&mini_brightness(0)));
    let before_shutdown = device.features().len();
    rig.stop();
    let features = device.features();
    assert_eq!(features[before_shutdown..], [mini_brightness(0)]);
    assert!(
        features
            .iter()
            .all(|report| *report == mini_brightness(report[5])),
        "sent {features:02x?}"
    );
    let images = device.images();
    let mut slots: Vec<_> = images.iter().map(|image| image.slot).collect();
    slots.sort_unstable();
    assert_eq!(slots, [0, 1, 2, 3, 4, 5]);
    let black = image_output::prepare_clear_jpeg_for(&MINI, ArtworkTarget::Keypad(1)).unwrap();
    for image in &images {
        assert_eq!(image.command, 0x01);
        assert_eq!(image.jpeg, black, "key {}", image.slot);
    }
}

#[test]
fn a_mini_and_a_plus_on_one_bus_each_get_their_own_image_format() {
    let (plus, mini) = (SimDevice::new(&PLUS), SimDevice::new(&MINI));
    let mut rig = Rig::empty();
    rig.bus.plug("/dev/hidraw1", &plus);
    rig.bus.plug("/dev/hidraw2", &mini);
    for (path, model) in [("/dev/hidraw1", &PLUS), ("/dev/hidraw2", &MINI)] {
        rig.wait_connection(ConnectionState::Connected {
            device: DeviceId::from_path(path),
            model,
        });
    }
    let clear = ArtworkCommand::Clear {
        target: ArtworkTarget::Keypad(1),
    };
    rig.send_artwork(&PLUS, clear.clone());
    rig.send_artwork(&MINI, clear);
    plus.wait_until(|device| device.last_key_jpeg(0).is_some());
    mini.wait_until(|device| device.last_key_jpeg(0).is_some());
    assert!(plus
        .writes()
        .iter()
        .all(|report| report[..2] == [0x02, 0x07]));
    assert!(mini
        .writes()
        .iter()
        .all(|report| report[..2] == [0x02, 0x01]));
    let jpeg = plus.last_key_jpeg(0).unwrap();
    assert_eq!(jpeg[..2], [0xFF, 0xD8], "the + keeps JPEG");
    let bmp = mini.last_key_jpeg(0).unwrap();
    assert_eq!(bmp.len(), MINI_BMP_LEN);
    assert_eq!(bmp[..2], *b"BM");
    mini.send(key_report(&MINI, &[3]));
    let events = rig.inputs_tagged(6);
    assert!(events.iter().all(
        |(device, model, _)| (*device, *model) == (DeviceId::from_path("/dev/hidraw2"), &MINI)
    ));
    assert!(events.iter().any(|(_, _, event)| *event
        == Event::Key {
            index: 3,
            pressed: true
        }));
}

#[test]
fn only_the_supported_product_ids_are_ever_discovered() {
    for pid in 0..=u16::MAX {
        let expected = match pid {
            0x0084 => Some("plus"),
            0x00C6 => Some("plus-xl"),
            0x006D | 0x0080 | 0x00A5 | 0x00B9 => Some("classic"),
            0x006C | 0x008F | 0x00BA => Some("xl"),
            _ => None,
        };
        let found = supported_devices([listed(VID, pid, "a")])
            .first()
            .map(|key| key.model.id);
        assert_eq!(found, expected, "{pid:04X}");
        assert_eq!(DeviceModel::by_pid(pid).map(|model| model.id), expected);
        assert!(supported_devices([listed(VID ^ 1, pid, "a")]).is_empty());
    }
    for model in DeviceModel::all().iter().filter(|m| !m.is_supported()) {
        for pid in model.pids {
            let found = supported_devices([listed(VID, *pid, "a"), listed(VID, 0x0084, "b")]);
            assert_eq!(
                found
                    .iter()
                    .map(|key| (key.path.as_str(), key.model.id))
                    .collect::<Vec<_>>(),
                [("b", "plus")]
            );
        }
    }
}
