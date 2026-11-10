//! Independent probes of the hub with several simulated devices: tagging of
//! every control, fan-out, unplug under load, replug policies, no double open,
//! isolation of failures, exact shutdown blanking, a stalled write and a
//! concurrent stress run. Only what `sim_hub` does not already pin.

use super::sim_device::{
    encoder_button_report, key_report, quick, rotation_report, touch_report, Bus, Rig, SimDevice,
};
use super::sim_tests::Scratch;
use super::*;
use crate::device_model::PLUS_XL;
use std::sync::atomic::{AtomicBool, Ordering};

fn id(path: &str) -> DeviceId {
    DeviceId::from_path(path)
}

fn up(path: &str, model: &'static DeviceModel) -> ConnectionState {
    ConnectionState::Connected {
        device: id(path),
        model,
    }
}

fn gone(rig: &mut Rig) {
    rig.wait_event(|event| {
        matches!(
            event,
            MonitorEvent::Connection(ConnectionState::Disconnected { .. })
        )
        .then_some(())
    });
}

fn assign(path: &std::path::Path, slot: u8) -> ArtworkCommand {
    ArtworkCommand::Assign {
        target: ArtworkTarget::Keypad(slot),
        path: path.to_owned(),
    }
}

fn jpeg_of(model: &'static DeviceModel, slot: u8, path: &std::path::Path) -> Vec<u8> {
    image_output::prepare_jpeg_for(model, ArtworkTarget::Keypad(slot), path).unwrap()
}

/// Wait, bounded, until every unit is held only by the test.
fn wait_released(devices: &[SimDevice]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while devices.iter().any(|device| device.handles() > 1) {
        assert!(Instant::now() < deadline, "a device handle leaked");
        thread::sleep(Duration::from_millis(2));
    }
}

fn wait_last(device: &SimDevice, slot: u8, expected: &[u8]) {
    device.wait_until(|device| device.last_key_jpeg(slot).as_deref() == Some(expected));
}

#[test]
fn p1_every_key_dial_and_touch_is_tagged_with_its_device_and_model() {
    let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
    let mut rig = Rig::empty();
    rig.bus.plug("/p", &plus);
    rig.bus.plug("/x", &xl);
    rig.wait_connection(up("/p", &PLUS));
    rig.wait_connection(up("/x", &PLUS_XL));
    for (path, device, model) in [("/p", &plus, &PLUS), ("/x", &xl, &PLUS_XL)] {
        let tagged = |rig: &mut Rig, count| {
            let found = rig.inputs_tagged(count);
            assert!(found.iter().all(|(d, m, _)| (*d, *m) == (id(path), model)));
            found
        };
        for key in 0..usize::from(model.keys()) {
            device.send(key_report(model, &[key]));
            let found = tagged(&mut rig, usize::from(model.key_inputs()));
            assert!(found.iter().any(|(_, _, e)| *e
                == Event::Key {
                    index: key,
                    pressed: true
                }));
        }
        for dial in 0..usize::from(model.dials) {
            device.send(encoder_button_report(model, &[dial]));
            tagged(&mut rig, usize::from(model.dials));
            device.send(rotation_report(model, &[(dial, 2)]));
            assert_eq!(
                tagged(&mut rig, 1)[0].2,
                Event::Rotation {
                    index: dial,
                    ticks: 2
                }
            );
        }
        let width = model.zone_px().0 as u16 * u16::from(model.zones());
        for x in [0, width / 2, width - 1] {
            device.send(touch_report(model, 1, 0x10, &[x, 5]));
            assert_eq!(tagged(&mut rig, 1)[0].2, Event::TouchTap { x, y: 5 });
            device.send(touch_report(model, 2, 6, &[x, 5]));
            assert_eq!(tagged(&mut rig, 1)[0].2, Event::TouchPress { x, y: 5 });
            device.send(touch_report(model, 3, 10, &[x, 5, 0, 5]));
            assert!(matches!(tagged(&mut rig, 1)[0].2, Event::TouchFlick { .. }));
        }
    }
}

#[test]
fn p2_two_plus_and_one_xl_get_exactly_their_own_reports() {
    let scratch = Scratch::new("probe-fan");
    let (a, b, xl) = (
        SimDevice::new(&PLUS),
        SimDevice::new(&PLUS),
        SimDevice::new(&PLUS_XL),
    );
    let mut rig = Rig::empty();
    for (path, device) in [("/a", &a), ("/b", &b), ("/x", &xl)] {
        rig.bus.plug(path, device);
    }
    for (path, model) in [("/a", &PLUS), ("/b", &PLUS), ("/x", &PLUS_XL)] {
        rig.wait_connection(up(path, model));
    }
    rig.send_artwork(
        &PLUS,
        assign(&scratch.marker("p.png", (120, 120), false), 1),
    );
    rig.send_artwork(
        &PLUS_XL,
        assign(&scratch.marker("x.png", (112, 112), false), 1),
    );
    rig.send_control(&PLUS, DeviceAction::Sleep);
    rig.send_control(&PLUS_XL, DeviceAction::Brightness(70));
    for device in [&a, &b] {
        device.wait_until(|d| d.image_count() == 1 && d.features().iter().any(|r| r[2] == 0));
    }
    xl.wait_until(|d| d.image_count() == 1 && d.features().iter().any(|r| r[2] == 70));
    assert_eq!(a.writes(), b.writes());
    assert_eq!(a.features(), b.features());
    let image = xl.images().remove(0).decode();
    assert_eq!((image.width(), image.height()), (112, 112));
    assert!(
        xl.features().iter().all(|r| r[2] != 0),
        "the + sleep reached the XL"
    );
    assert!(
        a.features().iter().all(|r| r[2] != 70),
        "the XL level reached a +"
    );
}

#[test]
fn p3_unplugging_a_plus_under_load_loses_nothing_on_the_others() {
    let scratch = Scratch::new("probe-load");
    let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
    let mut rig = Rig::empty();
    rig.bus.plug("/p", &plus);
    rig.bus.plug("/x", &xl);
    rig.wait_connection(up("/p", &PLUS));
    rig.wait_connection(up("/x", &PLUS_XL));
    let flow = scratch.marker("flow.png", (120, 120), false);
    let last = scratch.marker("last.png", (120, 120), true);
    for round in 0..40u8 {
        for slot in 1..=4 {
            rig.send_artwork(&PLUS, assign(&flow, slot));
            rig.send_artwork(&PLUS_XL, assign(&flow, slot));
        }
        if round == 10 {
            rig.bus.unplug("/p");
        }
    }
    gone(&mut rig);
    let again = SimDevice::new(&PLUS);
    rig.bus.plug("/p", &again);
    rig.wait_connection(up("/p", &PLUS));
    for slot in 1..=4 {
        rig.send_artwork(&PLUS, assign(&last, slot));
        rig.send_artwork(&PLUS_XL, assign(&last, slot));
    }
    for slot in 1..=4u8 {
        wait_last(&xl, slot - 1, &jpeg_of(&PLUS_XL, slot, &last));
        wait_last(&again, slot - 1, &jpeg_of(&PLUS, slot, &last));
    }
    // Every upload on the survivor was whole, and none was repeated: the
    // 40 rounds of four slots all arrived exactly once.
    let (images, open) = xl.assemble();
    assert_eq!(open, 0);
    assert_eq!(images.len(), 40 * 4 + 4);
}

#[test]
fn p4_a_new_path_without_a_live_peer_gets_the_whole_picture_once_and_the_brightness() {
    let scratch = Scratch::new("probe-newpath");
    let first = SimDevice::new(&PLUS);
    let mut rig = Rig::empty();
    rig.bus.plug("/old", &first);
    rig.wait_connection(up("/old", &PLUS));
    let picture = scratch.marker("pic.png", (120, 120), false);
    rig.send_artwork(
        &PLUS,
        ArtworkCommand::Replace((1..=3).map(|slot| assign(&picture, slot)).collect()),
    );
    rig.send_control(&PLUS, DeviceAction::Brightness(42));
    first.wait_until(|d| d.image_count() == 3);
    rig.bus.unplug("/old");
    gone(&mut rig);
    let fresh = SimDevice::new(&PLUS);
    rig.bus.plug("/new", &fresh);
    rig.wait_connection(up("/new", &PLUS));
    // A marker after the replay proves nothing else follows it.
    rig.send_artwork(
        &PLUS,
        ArtworkCommand::Clear {
            target: ArtworkTarget::Keypad(8),
        },
    );
    fresh.wait_until(|d| d.image_count() == 4);
    assert_eq!(fresh.features()[0][..3], [0x03, 0x08, 42]);
    let slots: Vec<_> = fresh.images().iter().map(|image| image.slot).collect();
    assert_eq!(slots, [0, 1, 2, 7]);
}

#[test]
fn p5_a_mirror_attaching_during_artwork_ends_identical_to_its_peer() {
    let scratch = Scratch::new("probe-late");
    let flow = scratch.marker("flow.png", (120, 120), false);
    let last = scratch.marker("last.png", (120, 120), true);
    let first = SimDevice::new(&PLUS);
    let mut rig = Rig::empty();
    rig.bus.plug("/a", &first);
    rig.wait_connection(up("/a", &PLUS));
    let expected = jpeg_of(&PLUS, 1, &last);
    for round in 0..20 {
        let late = SimDevice::new(&PLUS);
        rig.bus.plug("/b", &late);
        for _ in 0..5 {
            rig.send_artwork(
                &PLUS,
                ArtworkCommand::Replace(vec![assign(&flow, 1), assign(&flow, 2)]),
            );
        }
        rig.send_artwork(
            &PLUS,
            ArtworkCommand::Replace(vec![assign(&last, 1), assign(&last, 2)]),
        );
        rig.wait_connection(up("/b", &PLUS));
        for device in [&first, &late] {
            wait_last(device, 0, &expected);
            wait_last(device, 1, &expected);
            assert_eq!(
                device.last_key_jpeg(0),
                device.last_key_jpeg(1),
                "round {round}"
            );
        }
        rig.bus.unplug("/b");
        gone(&mut rig);
    }
}

#[test]
fn p6_same_path_replugs_never_open_a_path_twice_or_leak_a_handle() {
    let mut rig = Rig::empty();
    let mut devices = Vec::new();
    for _ in 0..120 {
        let device = SimDevice::new(&PLUS);
        rig.bus.plug("/p", &device);
        rig.wait_connection(up("/p", &PLUS));
        rig.bus.unplug("/p");
        gone(&mut rig);
        devices.push(device);
    }
    assert_eq!(rig.bus.opens("/p"), 120);
    assert!(devices.iter().all(|device| device.opened() == 1));
    wait_released(&devices);
}

#[test]
fn p6_replugs_faster_than_the_cadence_and_shuffled_listings_stay_single_open() {
    let bus = Bus::default();
    bus.reverse_order();
    let mut rig = Rig::over(bus, quick());
    let mut devices = Vec::new();
    for _ in 0..50 {
        let device = SimDevice::new(&PLUS_XL);
        rig.bus.plug("/p", &device);
        rig.bus.unplug("/p");
        devices.push(device);
    }
    let last = SimDevice::new(&PLUS_XL);
    rig.bus.plug("/p", &last);
    rig.wait_connection(up("/p", &PLUS_XL));
    assert!(devices.iter().all(|device| device.opened() <= 1));
    assert_eq!(last.opened(), 1);
    rig.stop();
    wait_released(&devices);
}

#[test]
fn p7_a_refused_device_and_a_failing_reader_do_not_stop_the_others() {
    let scratch = Scratch::new("probe-isolation");
    let (busy, broken, healthy) = (
        SimDevice::new(&PLUS),
        SimDevice::new(&PLUS),
        SimDevice::new(&PLUS_XL),
    );
    let mut rig = Rig::empty();
    rig.bus.refuse_open("/a");
    rig.bus.plug("/a", &busy);
    rig.bus.plug("/b", &broken);
    rig.bus.plug("/c", &healthy);
    rig.wait_connection(up("/b", &PLUS));
    rig.wait_connection(up("/c", &PLUS_XL));
    broken.unplug();
    gone(&mut rig);
    rig.send_artwork(
        &PLUS_XL,
        assign(&scratch.marker("x.png", (112, 112), false), 1),
    );
    healthy.wait_until(|d| d.image_count() == 1);
    healthy.send(key_report(&PLUS_XL, &[3]));
    assert_eq!(rig.inputs_tagged(36)[0].0, id("/c"));
    assert!(busy.writes().is_empty() && busy.opened() == 0);
}

#[test]
fn p8_shutdown_blanks_each_live_device_exactly_once_and_not_an_unplugged_one() {
    let (plus, xl, lost) = (
        SimDevice::new(&PLUS),
        SimDevice::new(&PLUS_XL),
        SimDevice::new(&PLUS),
    );
    let mut rig = Rig::empty();
    for (path, device) in [("/p", &plus), ("/x", &xl), ("/l", &lost)] {
        rig.bus.plug(path, device);
    }
    for (path, model) in [("/l", &PLUS), ("/p", &PLUS), ("/x", &PLUS_XL)] {
        rig.wait_connection(up(path, model));
    }
    rig.bus.unplug("/l");
    gone(&mut rig);
    rig.stop();
    for (device, keys) in [(&plus, 8u8), (&xl, 36)] {
        let images = device.images();
        for slot in 0..keys {
            let clears = images
                .iter()
                .filter(|image| image.command == 0x07 && image.slot == slot)
                .count();
            assert_eq!(clears, 1, "key {slot}");
        }
        assert_eq!(
            images.iter().filter(|image| image.command == 0x0b).count(),
            1
        );
        let dark = device.features().iter().filter(|r| r[2] == 0).count();
        assert_eq!(dark, 1);
    }
    assert!(lost.writes().is_empty());
    assert!(lost.features().iter().all(|report| report[2] != 0));
}

#[test]
fn p9_a_blocked_write_delays_shutdown_by_its_own_length_only() {
    let device = SimDevice::new(&PLUS);
    let mut rig = Rig::empty();
    rig.bus.plug("/p", &device);
    rig.wait_connection(up("/p", &PLUS));
    device.stall_next_write(Duration::from_millis(1000));
    for key in 1..=5 {
        let target = ArtworkTarget::Keypad(key);
        rig.send_artwork(&PLUS, ArtworkCommand::Clear { target });
    }
    device.wait_until(|d| !d.writes().is_empty());
    let started = Instant::now();
    rig.stop();
    assert!(
        started.elapsed() < Duration::from_millis(2500),
        "{:?}",
        started.elapsed()
    );
    // The admitted write finished and cut its image short; the four queued
    // uploads were cancelled, so only that image and the eight blank keys began.
    let begun = device
        .writes()
        .iter()
        .filter(|r| r[1] == 0x07 && u16::from_le_bytes([r[6], r[7]]) == 0)
        .count();
    assert_eq!(begun, 1 + 8);
    assert_eq!(rig.closed_send(), Err(HubClosed), "later sends are refused");
}

#[test]
fn p10_concurrent_senders_under_repeated_plugging_stay_consistent() {
    let scratch = Scratch::new("probe-stress");
    let picture = scratch.marker("s.png", (120, 120), false);
    let last = scratch.marker("l.png", (120, 120), true);
    let mut rig = Rig::empty();
    let stop = AtomicBool::new(false);
    let mut devices = Vec::new();
    thread::scope(|scope| {
        for lane in 0..4u8 {
            let handle = rig.handle();
            let (artwork, controls) = (rig.artwork.clone(), rig.controls.clone());
            let (stop, picture) = (&stop, &picture);
            scope.spawn(move || {
                let mut n = 0u8;
                while !stop.load(Ordering::Acquire) {
                    let slot = 1 + n % 8;
                    match lane {
                        0 => drop(artwork.send(assign(picture, slot))),
                        1 => drop(controls.send(DeviceAction::AdjustBrightness(1))),
                        2 => drop(handle.send_artwork(&PLUS_XL, assign(picture, slot))),
                        _ => drop(handle.send_control(&PLUS, DeviceAction::AdjustBrightness(-1))),
                    }
                    n = n.wrapping_add(1);
                    thread::sleep(Duration::from_millis(2));
                }
            });
        }
        for _ in 0..150 {
            let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
            rig.bus.plug("/p", &plus);
            rig.bus.plug("/x", &xl);
            rig.wait_connection(up("/p", &PLUS));
            rig.wait_connection(up("/x", &PLUS_XL));
            rig.bus.unplug("/p");
            rig.bus.unplug("/x");
            gone(&mut rig);
            gone(&mut rig);
            devices.extend([plus, xl]);
        }
        stop.store(true, Ordering::Release);
    });
    let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
    rig.bus.plug("/p", &plus);
    rig.bus.plug("/x", &xl);
    rig.wait_connection(up("/p", &PLUS));
    rig.wait_connection(up("/x", &PLUS_XL));
    rig.send_artwork(&PLUS, ArtworkCommand::Replace(vec![assign(&last, 1)]));
    rig.send_artwork(&PLUS_XL, ArtworkCommand::Replace(vec![assign(&last, 1)]));
    wait_last(&plus, 0, &jpeg_of(&PLUS, 1, &last));
    wait_last(&xl, 0, &jpeg_of(&PLUS_XL, 1, &last));
    assert!(devices.iter().all(|device| device.opened() <= 1));
    rig.stop();
    wait_released(&devices);
}
