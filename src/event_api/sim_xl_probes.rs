//! Independent protocol probes: model selection, edge coordinates and the
//! replug cycle, checked from the wire rather than through the helpers
//! the production code uses.

use super::sim_device::{key_report, touch_report, Rig, SimDevice};
use super::sim_tests::connected;
use super::*;
use crate::device_model::PLUS_XL;
use crate::layers::Trigger;

#[test]
fn product_ids_select_their_model_and_unknown_ones_are_ignored() {
    assert_eq!(DeviceModel::by_pid(0x0084), Some(&PLUS));
    assert_eq!(DeviceModel::by_pid(0x00C6), Some(&PLUS_XL));
    assert_eq!(DeviceModel::by_pid(0x0080), None);
    assert_eq!(
        first_supported([(VID, 0x00C6), (VID, 0x0084)]),
        Some((0, &PLUS_XL))
    );
    assert_eq!(
        first_supported([(VID, 0x0084), (VID, 0x00C6)]),
        Some((0, &PLUS))
    );
}

#[test]
fn xl_touch_edges_map_to_sections_only_inside_the_strip() {
    let xl = SimDevice::new(&PLUS_XL);
    let mut rig = Rig::start(&[&xl]);
    rig.wait_connection(connected(&PLUS_XL));
    for (x, zone) in [
        (0u16, Some(1u8)),
        (199, Some(1)),
        (200, Some(2)),
        (1199, Some(6)),
        (1200, None),
        (65535, None),
    ] {
        xl.send(touch_report(&PLUS_XL, 1, 0x10, &[x, 5]));
        let event = rig.inputs(1).remove(0);
        assert_eq!(event, Event::TouchTap { x, y: 5 });
        let triggers = Trigger::from_event_for(&PLUS_XL, &event);
        assert_eq!(
            triggers,
            Some(zone.map_or(Trigger::TouchTap, Trigger::ZoneTap))
        );
    }
}

/// Every output report is one full-size report, whatever the model.
fn assert_report_sizes(device: &SimDevice) {
    for report in device.writes() {
        assert_eq!(report.len(), device.model.image_report_len);
    }
}

#[test]
fn plus_xl_plus_xl_replug_cycle_drops_stale_artwork_only_on_a_model_change() {
    let devices = [
        SimDevice::new(&PLUS),
        SimDevice::new(&PLUS_XL),
        SimDevice::new(&PLUS),
        SimDevice::new(&PLUS_XL),
    ];
    let mut rig = Rig::start(&devices.iter().collect::<Vec<_>>());
    rig.wait_connection(connected(devices[0].model));
    for (at, device) in devices.iter().enumerate() {
        let key = device.model.keys();
        device.send(key_report(device.model, &[usize::from(key) - 1]));
        let events = rig.inputs(usize::from(device.model.keys()));
        assert!(events.contains(&Event::Key {
            index: usize::from(key) - 1,
            pressed: true
        }));
        device.unplug();
        rig.wait_event(|event| {
            matches!(
                event,
                MonitorEvent::Connection(ConnectionState::Disconnected(_))
            )
            .then_some(())
        });
        if let Some(next) = devices.get(at + 1) {
            // Queued while unplugged, drawn only if the next device is the same model.
            rig.artwork
                .send(ArtworkCommand::Clear {
                    target: ArtworkTarget::Keypad(2),
                })
                .unwrap();
            rig.wait_connection(connected(next.model));
            if next.model == device.model {
                next.wait_until(|next| next.image_count() == 1);
            } else {
                assert!(next.writes().is_empty(), "artwork leaked across models");
            }
            assert_report_sizes(next);
        }
    }
}
