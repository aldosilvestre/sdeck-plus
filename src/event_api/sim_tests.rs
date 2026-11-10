//! End-to-end checks of the HID monitor against the simulated device.

use super::sim_device::{
    encoder_button_report, key_report, rotation_report, touch_report, Rig, SimDevice,
};
use super::*;
use crate::device_model::PLUS_XL;
use crate::layers::Trigger;
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};

pub(super) struct Scratch(pub(super) std::path::PathBuf);

impl Scratch {
    pub(super) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sdeck-sim-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// A blue picture with a red square in its top-left corner. With `noise`
    /// the rest is pseudo-random, so the JPEG needs many chunks.
    pub(super) fn marker(
        &self,
        name: &str,
        (width, height): (u32, u32),
        noise: bool,
    ) -> std::path::PathBuf {
        let image = Pixbuf::new(Colorspace::Rgb, false, 8, width as i32, height as i32).unwrap();
        image.fill(0x0000ffff);
        if noise {
            let mut seed = 0x2545_f491_u32;
            for y in 0..height {
                for x in 0..width {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let [r, g, ..] = seed.to_le_bytes();
                    image.put_pixel(x, y, r, g, 255, 255);
                }
            }
        }
        image.new_subpixbuf(0, 0, 24, 24).fill(0xff0000ff);
        let path = self.0.join(name);
        image.savev(&path, "png", &[]).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn is_red(image: &Pixbuf, x: i32, y: i32) -> bool {
    let at = (y * image.rowstride() + x * image.n_channels()) as usize;
    let pixels = image.read_pixel_bytes();
    let pixel = &pixels.as_ref()[at..at + 3];
    pixel[0] > 180 && pixel[1] < 90 && pixel[2] < 90
}

pub(super) fn connected(model: &'static DeviceModel) -> ConnectionState {
    ConnectionState::Connected(model)
}

fn upload(device: &SimDevice, target: ArtworkTarget, path: std::path::PathBuf) -> CapturedImages {
    let mut rig = Rig::start(&[device]);
    rig.wait_connection(connected(device.model));
    rig.artwork
        .send(ArtworkCommand::Assign { target, path })
        .unwrap();
    device.wait_until(|device| device.image_count() == 1);
    rig.stop();
    CapturedImages(device.images())
}

struct CapturedImages(Vec<super::sim_device::CapturedImage>);

impl CapturedImages {
    /// The image sent for the command, ignoring the blanking on shutdown.
    fn first(&self) -> &super::sim_device::CapturedImage {
        &self.0[0]
    }
}

#[test]
fn first_supported_device_wins_and_other_vendors_are_ignored() {
    let ids = [
        (0x1234, 0x0084),
        (VID, 0x1111),
        (VID, 0x00C6),
        (VID, 0x0084),
    ];
    assert_eq!(first_supported(ids), Some((2, &PLUS_XL)));
    assert_eq!(first_supported([(0x1234, 0x0084)]), None);
    assert_eq!(first_supported([(VID, 0x0084)]), Some((0, &PLUS)));
}

#[test]
fn xl_key_press_and_release_for_first_and_last_key() {
    let xl = SimDevice::new(&PLUS_XL);
    let mut rig = Rig::start(&[&xl]);
    rig.wait_connection(connected(&PLUS_XL));
    for key in [0usize, 35] {
        xl.send(key_report(&PLUS_XL, &[key]));
        let events = rig.inputs(36);
        let pressed: Vec<_> = events
            .iter()
            .filter(|event| matches!(event, Event::Key { pressed: true, .. }))
            .collect();
        assert_eq!(
            pressed,
            [&Event::Key {
                index: key,
                pressed: true
            }]
        );
        assert_eq!(
            Trigger::from_event_for(&PLUS_XL, &events[key]),
            Some(Trigger::KeyPress(key as u8 + 1))
        );
        xl.send(key_report(&PLUS_XL, &[]));
        let released = rig.inputs(36);
        assert!(released
            .iter()
            .all(|event| matches!(event, Event::Key { pressed: false, .. })));
    }
}

#[test]
fn xl_encoder_buttons_and_rotations_report_each_of_the_six_dials() {
    let xl = SimDevice::new(&PLUS_XL);
    let mut rig = Rig::start(&[&xl]);
    rig.wait_connection(connected(&PLUS_XL));
    for dial in 0..6usize {
        xl.send(encoder_button_report(&PLUS_XL, &[dial]));
        let events = rig.inputs(6);
        for (index, event) in events.iter().enumerate() {
            assert_eq!(
                *event,
                Event::EncoderButton {
                    index,
                    pressed: index == dial
                }
            );
        }
        for ticks in [3i8, -2] {
            xl.send(rotation_report(&PLUS_XL, &[(dial, ticks)]));
            let events = rig.inputs(1);
            assert_eq!(events, [Event::Rotation { index: dial, ticks }]);
            assert_eq!(
                Trigger::from_event_for(&PLUS_XL, &events[0]),
                Some(Trigger::Rotation(dial as u8 + 1, ticks))
            );
        }
    }
}

#[test]
fn xl_touch_gestures_map_to_the_section_under_the_finger() {
    let xl = SimDevice::new(&PLUS_XL);
    let mut rig = Rig::start(&[&xl]);
    rig.wait_connection(connected(&PLUS_XL));
    for x in [
        0u16, 150, 199, 200, 399, 400, 599, 600, 799, 800, 999, 1000, 1199,
    ] {
        let zone = (x / 200 + 1) as u8;
        xl.send(touch_report(&PLUS_XL, 1, 0x10, &[x, 50]));
        let tap = rig.inputs(1);
        assert_eq!(tap, [Event::TouchTap { x, y: 50 }]);
        assert_eq!(
            Trigger::from_event_for(&PLUS_XL, &tap[0]),
            Some(Trigger::ZoneTap(zone))
        );
        xl.send(touch_report(&PLUS_XL, 2, 6, &[x, 50]));
        let press = rig.inputs(1);
        assert_eq!(press, [Event::TouchPress { x, y: 50 }]);
        assert_eq!(
            Trigger::from_event_for(&PLUS_XL, &press[0]),
            Some(Trigger::ZonePress(zone))
        );
    }
    xl.send(touch_report(&PLUS_XL, 3, 10, &[1100, 40, 100, 40]));
    assert_eq!(
        rig.inputs(1),
        [Event::TouchFlick {
            start_x: 1100,
            start_y: 40,
            end_x: 100,
            end_y: 40
        }]
    );
}

fn connection_log(rig: &Rig) -> Vec<ConnectionState> {
    rig.seen
        .iter()
        .filter_map(|event| match event {
            MonitorEvent::Connection(state) => Some(state.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn replugging_the_other_model_reports_it_and_drops_stale_artwork() {
    let plus = SimDevice::new(&PLUS);
    let xl = SimDevice::new(&PLUS_XL);
    let mut rig = Rig::start(&[&plus, &xl]);
    rig.wait_connection(connected(&PLUS));
    plus.send(key_report(&PLUS, &[7]));
    assert_eq!(rig.inputs(8).len(), 8);
    plus.unplug();
    rig.wait_event(|event| {
        matches!(
            event,
            MonitorEvent::Connection(ConnectionState::Disconnected(_))
        )
        .then_some(())
    });
    // Queued while nothing is attached, for the keypad of the old model.
    rig.artwork
        .send(ArtworkCommand::Clear {
            target: ArtworkTarget::Keypad(3),
        })
        .unwrap();
    rig.wait_connection(connected(&PLUS_XL));
    xl.send(key_report(&PLUS_XL, &[35]));
    assert_eq!(rig.inputs(36).len(), 36);
    assert_eq!(
        connection_log(&rig),
        [
            ConnectionState::Searching,
            connected(&PLUS),
            ConnectionState::Disconnected("HID read ended: unplugged".into()),
            ConnectionState::Searching,
            connected(&PLUS_XL),
        ]
    );
    assert!(
        xl.writes().is_empty(),
        "stale artwork reached the new model"
    );
    assert!(!rig
        .seen
        .iter()
        .any(|event| matches!(event, MonitorEvent::ArtworkResult { .. })));
}

#[test]
fn replugging_the_same_model_keeps_queued_artwork() {
    let first = SimDevice::new(&PLUS);
    let second = SimDevice::new(&PLUS);
    let mut rig = Rig::start(&[&first, &second]);
    rig.wait_connection(connected(&PLUS));
    first.unplug();
    rig.wait_event(|event| {
        matches!(
            event,
            MonitorEvent::Connection(ConnectionState::Disconnected(_))
        )
        .then_some(())
    });
    rig.artwork
        .send(ArtworkCommand::Clear {
            target: ArtworkTarget::Keypad(3),
        })
        .unwrap();
    rig.wait_event(|event| {
        matches!(
            event,
            MonitorEvent::ArtworkResult {
                target: ArtworkTarget::Keypad(3),
                result: Ok(())
            }
        )
        .then_some(())
    });
    assert_eq!(second.images().len(), 1);
}

#[test]
fn xl_key_upload_is_a_112_square_jpeg_rotated_counterclockwise() {
    let scratch = Scratch::new("xl-key");
    let xl = SimDevice::new(&PLUS_XL);
    let path = scratch.marker("key.png", (112, 112), false);
    let sent = upload(&xl, ArtworkTarget::Keypad(1), path);
    let image = sent.first();
    assert_eq!((image.command, image.slot), (0x07, 0));
    let decoded = image.decode();
    assert_eq!((decoded.width(), decoded.height()), (112, 112));
    // The red top-left corner of the source ends up bottom-left.
    assert!(is_red(&decoded, 8, 112 - 9), "marker not at bottom-left");
    assert!(!is_red(&decoded, 8, 8), "marker still at top-left");
}

#[test]
fn xl_strip_upload_is_100_by_1200_and_rotated() {
    let scratch = Scratch::new("xl-strip");
    let xl = SimDevice::new(&PLUS_XL);
    let path = scratch.marker("strip.png", (1200, 100), true);
    let sent = upload(&xl, ArtworkTarget::TouchStripBackground, path);
    let image = sent.first();
    assert_eq!((image.command, image.slot), (0x0b, 0));
    assert!(image.chunks > 1, "a busy strip needs several chunks");
    let decoded = image.decode();
    assert_eq!((decoded.width(), decoded.height()), (100, 1200));
    assert!(is_red(&decoded, 8, 1200 - 9), "marker not at bottom-left");
}

#[test]
fn xl_zone_upload_is_100_by_200_with_logical_window_coordinates() {
    let scratch = Scratch::new("xl-zone");
    let xl = SimDevice::new(&PLUS_XL);
    let path = scratch.marker("zone.png", (200, 100), false);
    let sent = upload(&xl, ArtworkTarget::EncoderZone(6), path);
    let image = sent.first();
    assert_eq!(image.command, 0x0c);
    assert_eq!(image.window, Some((1000, 0, 200, 100)));
    let decoded = image.decode();
    assert_eq!((decoded.width(), decoded.height()), (100, 200));
    assert!(is_red(&decoded, 8, 200 - 9), "marker not at bottom-left");
}

#[test]
fn swapped_axis_models_send_the_window_in_rotated_portrait_coordinates() {
    let scratch = Scratch::new("xl-swap");
    static SWAPPED: DeviceModel = DeviceModel {
        partial_window_axis_swap: true,
        ..PLUS_XL
    };
    let device = SimDevice::new(&SWAPPED);
    let path = scratch.marker("zone.png", (200, 100), false);
    let sent = upload(&device, ArtworkTarget::EncoderZone(6), path);
    assert_eq!(sent.first().window, Some((0, 1000, 100, 200)));
    let decoded = sent.first().decode();
    assert_eq!((decoded.width(), decoded.height()), (100, 200));
}

#[test]
fn xl_large_image_is_split_into_1024_byte_reports_with_a_final_flag() {
    let scratch = Scratch::new("xl-chunks");
    let xl = SimDevice::new(&PLUS_XL);
    let path = scratch.marker("strip.png", (1200, 100), true);
    upload(&xl, ArtworkTarget::TouchStripBackground, path);
    let strip: Vec<_> = xl.writes().into_iter().filter(|r| r[1] == 0x0b).collect();
    let first_upload = &strip[..strip
        .iter()
        .position(|report| report[3] == 1)
        .map(|last| last + 1)
        .unwrap()];
    assert!(first_upload.len() > 1);
    assert!(first_upload.iter().all(|report| report.len() == 1024));
    let (last, others) = first_upload.split_last().unwrap();
    assert!(others.iter().all(|report| report[3] == 0));
    assert_eq!(last[3], 1);
    for (index, report) in first_upload.iter().enumerate() {
        assert_eq!(u16::from_le_bytes([report[6], report[7]]) as usize, index);
    }
}

#[test]
fn plus_uploads_are_unrotated_and_byte_identical_to_the_legacy_encoding() {
    let scratch = Scratch::new("plus-key");
    let plus = SimDevice::new(&PLUS);
    let path = scratch.marker("key.png", (120, 120), false);
    let sent = upload(&plus, ArtworkTarget::Keypad(1), path.clone());
    let decoded = sent.first().decode();
    assert_eq!((decoded.width(), decoded.height()), (120, 120));
    assert!(is_red(&decoded, 8, 8), "marker moved on the Stream Deck +");
    let jpeg = image_output::prepare_jpeg(ArtworkTarget::Keypad(1), &path).unwrap();
    let expected = image_output::encode_reports(ArtworkTarget::Keypad(1), &jpeg).unwrap();
    let written: Vec<_> = plus.writes().into_iter().take(expected.len()).collect();
    assert_eq!(
        written,
        expected.into_iter().map(|r| r.0).collect::<Vec<_>>()
    );
}

#[test]
fn plus_zone_and_strip_keep_their_windows_and_sizes() {
    let scratch = Scratch::new("plus-zone");
    let plus = SimDevice::new(&PLUS);
    let zone = upload(
        &plus,
        ArtworkTarget::EncoderZone(4),
        scratch.marker("zone.png", (200, 100), false),
    );
    assert_eq!(zone.first().window, Some((600, 0, 200, 100)));
    let decoded = zone.first().decode();
    assert_eq!((decoded.width(), decoded.height()), (200, 100));
    assert!(is_red(&decoded, 8, 8));
    let plus = SimDevice::new(&PLUS);
    let strip = upload(
        &plus,
        ArtworkTarget::TouchStripBackground,
        scratch.marker("strip.png", (800, 100), false),
    );
    let decoded = strip.first().decode();
    assert_eq!((decoded.width(), decoded.height()), (800, 100));
}

#[test]
fn brightness_feature_report_is_the_same_for_both_models() {
    for model in [&PLUS, &PLUS_XL] {
        let device = SimDevice::new(model);
        let mut rig = Rig::start(&[&device]);
        rig.wait_connection(connected(model));
        rig.controls.send(DeviceAction::Brightness(40)).unwrap();
        device.wait_until(|device| device.features().iter().any(|report| report[2] == 40));
        let mut expected = [0u8; 32];
        expected[..3].copy_from_slice(&[0x03, 0x08, 40]);
        assert!(device.features().contains(&expected.to_vec()));
    }
}

#[test]
fn shutdown_blanks_every_key_and_the_strip_of_the_connected_model() {
    for (model, keys, strip) in [(&PLUS, 8usize, (800, 100)), (&PLUS_XL, 36, (100, 1200))] {
        let device = SimDevice::new(model);
        let mut rig = Rig::start(&[&device]);
        rig.wait_connection(connected(model));
        rig.stop();
        let images = device.images();
        let mut slots: Vec<_> = images
            .iter()
            .filter(|image| image.command == 0x07)
            .map(|image| image.slot as usize)
            .collect();
        slots.sort_unstable();
        assert_eq!(slots, (0..keys).collect::<Vec<_>>());
        let strips: Vec<_> = images
            .iter()
            .filter(|image| image.command == 0x0b)
            .collect();
        assert_eq!(strips.len(), 1);
        let decoded = strips[0].decode();
        assert_eq!((decoded.width(), decoded.height()), strip);
        assert_eq!(device.features().last().unwrap()[..3], [0x03, 0x08, 0]);
    }
}
