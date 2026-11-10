//! The simulator driven by the registry table instead of one hand-picked
//! model. These runs do not make a Planned model supported: they prove the
//! Expanded-family machinery works for every documented shape, so promoting a
//! model later is a table edit plus verification.

use super::sim_device::{key_report, Rig, SimDevice};
use super::sim_tests::{connected, is_red, Scratch};
use super::*;
use crate::device_model::{ProtocolFamily, Rotation, Support};

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
            assert_eq!(first_supported([(VID, *pid)]), None);
            assert_eq!(DeviceModel::by_pid_any(*pid), Some(*model));
        }
    }
    assert_eq!(
        first_supported([(VID, 0x006D), (VID, 0x009A), (VID, 0x0063), (VID, 0x00C6)]),
        Some((3, &crate::device_model::PLUS_XL))
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
fn a_legacy_mini_is_refused_clearly_and_never_driven() {
    let model = DeviceModel::by_id("mini").unwrap();
    assert_eq!(model.family, ProtocolFamily::LegacyMini);
    let message = model.ensure_protocol().unwrap_err();
    assert!(message.contains("Stream Deck Mini") && message.contains("not implemented"));

    let device = SimDevice::new(model);
    let mut rig = Rig::start(&[&device]);
    let reason = rig.wait_event(|event| match event {
        MonitorEvent::Connection(ConnectionState::Unavailable(reason))
            if reason.contains("not implemented") =>
        {
            Some(reason.clone())
        }
        _ => None,
    });
    assert_eq!(reason, message);
    rig.stop();
    assert!(device.writes().is_empty() && device.features().is_empty());
    assert!(!rig
        .seen
        .iter()
        .any(|event| *event == MonitorEvent::Connection(connected(model))));

    assert!(crate::protocol::try_decode_report_for(model, &[1, 0, 1, 0, 1]).is_err());
    assert!(crate::protocol::decode_report_for(model, &[1, 0, 1, 0, 1]).is_empty());
    assert!(image_output::encode_reports_for(model, ArtworkTarget::Keypad(1), &[1]).is_err());
    let scratch = Scratch::new("registry-mini-image");
    let path = scratch.marker("picture.png", (80, 80), false);
    let error = image_output::prepare_jpeg_for(model, ArtworkTarget::Keypad(1), &path).unwrap_err();
    assert!(error.contains("BMP") && error.contains("not implemented"));
}

#[test]
fn only_the_two_supported_product_ids_are_ever_discovered() {
    for pid in 0..=u16::MAX {
        let expected = match pid {
            0x0084 => Some("plus"),
            0x00C6 => Some("plus-xl"),
            _ => None,
        };
        let found = first_supported([(VID, pid)]).map(|(_, model)| model.id);
        assert_eq!(found, expected, "{pid:04X}");
        assert_eq!(DeviceModel::by_pid(pid).map(|model| model.id), expected);
        assert_eq!(first_supported([(VID ^ 1, pid)]), None, "{pid:04X}");
    }
    for model in DeviceModel::all().iter().filter(|m| !m.is_supported()) {
        for pid in model.pids {
            let found = first_supported([(VID, *pid), (VID, 0x0084)]);
            assert_eq!(found.map(|(at, m)| (at, m.id)), Some((1, "plus")));
        }
    }
}
