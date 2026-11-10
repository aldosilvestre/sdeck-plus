//! The hub serving several simulated devices at once: identity, tagged events,
//! per-model fan-out, plug and unplug at runtime, shutdown and a stress run.

use super::sim_device::{key_report, quick, Bus, Rig, SimDevice};
use super::sim_tests::{is_red, Scratch};
use super::*;
use crate::device_model::PLUS_XL;

fn id(path: &str) -> DeviceId {
    DeviceId::from_path(path)
}

fn connected_as(path: &str, model: &'static DeviceModel) -> ConnectionState {
    ConnectionState::Connected {
        device: id(path),
        model,
    }
}

/// A rig with two devices of different models, both connected.
fn pair() -> (Rig, SimDevice, SimDevice) {
    let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
    let mut rig = Rig::empty();
    rig.bus.plug("/dev/hidraw1", &plus);
    rig.bus.plug("/dev/hidraw2", &xl);
    rig.wait_connection(connected_as("/dev/hidraw1", &PLUS));
    rig.wait_connection(connected_as("/dev/hidraw2", &PLUS_XL));
    (rig, plus, xl)
}

fn assign_key(scratch: &Scratch, name: &str, size: u32) -> ArtworkCommand {
    ArtworkCommand::Assign {
        target: ArtworkTarget::Keypad(1),
        path: scratch.marker(name, (size, size), false),
    }
}

fn connections(rig: &Rig) -> Vec<ConnectionState> {
    rig.seen
        .iter()
        .filter_map(|event| match event {
            MonitorEvent::Connection(state) => Some(state.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn device_ids_come_from_the_path_and_keys_name_the_model() {
    assert_eq!(id("/dev/hidraw3"), id("/dev/hidraw3"));
    assert_ne!(id("/dev/hidraw3"), id("/dev/hidraw4"));
    assert_eq!(id(""), DeviceId::from_path(""));
    assert_eq!(id("a").to_string().len(), "hid-".len() + 16);
    let key = DeviceKey {
        path: "/dev/hidraw3".into(),
        serial: Some("CL1".into()),
        model: &PLUS,
    };
    assert_eq!(key.id(), id("/dev/hidraw3"));
}

#[test]
fn two_models_connect_together_each_with_its_own_id_and_model() {
    let (rig, _plus, _xl) = pair();
    assert_eq!(
        connections(&rig)
            .into_iter()
            .filter(ConnectionState::is_connected)
            .collect::<Vec<_>>(),
        [
            connected_as("/dev/hidraw1", &PLUS),
            connected_as("/dev/hidraw2", &PLUS_XL)
        ]
    );
}

#[test]
fn input_is_tagged_with_the_device_and_model_it_came_from() {
    let (mut rig, plus, xl) = pair();
    plus.send(key_report(&PLUS, &[7]));
    let from_plus = rig.inputs_tagged(8);
    assert!(from_plus
        .iter()
        .all(|(device, model, _)| (*device, *model) == (id("/dev/hidraw1"), &PLUS)));
    xl.send(key_report(&PLUS_XL, &[35]));
    let from_xl = rig.inputs_tagged(36);
    assert!(from_xl
        .iter()
        .all(|(device, model, _)| (*device, *model) == (id("/dev/hidraw2"), &PLUS_XL)));
    assert!(from_xl.iter().any(|(_, _, event)| *event
        == Event::Key {
            index: 35,
            pressed: true
        }));
}

#[test]
fn artwork_reaches_only_the_devices_of_the_addressed_model_at_their_own_size() {
    let scratch = Scratch::new("hub-sizes");
    let (rig, plus, xl) = pair();
    rig.send_artwork(&PLUS, assign_key(&scratch, "plus.png", 120));
    plus.wait_until(|device| device.image_count() == 1);
    assert!(xl.writes().is_empty(), "the + picture reached the XL");
    rig.send_artwork(&PLUS_XL, assign_key(&scratch, "xl.png", 112));
    xl.wait_until(|device| device.image_count() == 1);
    assert_eq!(plus.image_count(), 1, "the XL picture reached the +");
    let plus_image = plus.images().remove(0).decode();
    assert_eq!((plus_image.width(), plus_image.height()), (120, 120));
    assert!(is_red(&plus_image, 8, 8), "the + is never rotated");
    let xl_image = xl.images().remove(0).decode();
    assert_eq!((xl_image.width(), xl_image.height()), (112, 112));
    assert!(
        is_red(&xl_image, 8, 112 - 9),
        "the XL turns counterclockwise"
    );
}

#[test]
fn the_adapter_senders_reach_only_the_first_connected_model() {
    let scratch = Scratch::new("hub-adapter");
    let (rig, plus, xl) = pair();
    let ArtworkCommand::Assign { target, path } = assign_key(&scratch, "first.png", 120) else {
        unreachable!()
    };
    rig.artwork
        .send(ArtworkCommand::Assign { target, path })
        .unwrap();
    rig.controls.send(DeviceAction::Brightness(61)).unwrap();
    plus.wait_until(|device| {
        device.image_count() == 1 && device.features().iter().any(|report| report[2] == 61)
    });
    assert!(xl.writes().is_empty());
    assert!(xl.features().iter().all(|report| report[2] == 100));
}

#[test]
fn two_devices_of_one_model_mirror_artwork_and_controls() {
    let scratch = Scratch::new("hub-mirror");
    let (first, second) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS));
    let mut rig = Rig::empty();
    rig.bus.plug("/dev/hidraw1", &first);
    rig.bus.plug("/dev/hidraw2", &second);
    rig.wait_connection(connected_as("/dev/hidraw2", &PLUS));
    rig.send_artwork(&PLUS, assign_key(&scratch, "both.png", 120));
    rig.send_control(&PLUS, DeviceAction::Brightness(35));
    for device in [&first, &second] {
        device.wait_until(|device| {
            device.image_count() == 1 && device.features().iter().any(|report| report[2] == 35)
        });
    }
    assert_eq!(first.writes(), second.writes());
    assert_eq!(first.features(), second.features());
}

#[test]
fn unplugging_one_device_leaves_the_other_running() {
    let scratch = Scratch::new("hub-unplug");
    let (mut rig, plus, xl) = pair();
    rig.bus.unplug("/dev/hidraw2");
    rig.wait_event(|event| {
        matches!(
            event,
            MonitorEvent::Connection(ConnectionState::Disconnected { device, model, .. })
                if *device == id("/dev/hidraw2") && *model == &PLUS_XL
        )
        .then_some(())
    });
    rig.send_artwork(&PLUS, assign_key(&scratch, "still.png", 120));
    plus.wait_until(|device| device.image_count() == 1);
    plus.send(key_report(&PLUS, &[0]));
    assert_eq!(rig.inputs_tagged(8)[0].0, id("/dev/hidraw1"));
    assert!(
        connections(&rig)
            .iter()
            .filter(|state| **state == ConnectionState::Searching)
            .count()
            <= 1,
        "a live device means the hub is not searching"
    );
    // The unplugged device is not blanked; it is gone.
    rig.stop();
    assert!(xl.writes().is_empty());
}

#[test]
fn a_replugged_mirror_is_drawn_from_the_cached_snapshot_and_desired_brightness() {
    let scratch = Scratch::new("hub-replug");
    let (first, second) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS));
    let mut rig = Rig::empty();
    rig.bus.plug("/dev/hidraw1", &first);
    rig.bus.plug("/dev/hidraw2", &second);
    rig.wait_connection(connected_as("/dev/hidraw2", &PLUS));
    rig.send_artwork(
        &PLUS,
        ArtworkCommand::Replace(vec![assign_key(&scratch, "snap.png", 120)]),
    );
    rig.send_control(&PLUS, DeviceAction::Brightness(40));
    second.wait_until(|device| device.image_count() == 1);
    rig.bus.unplug("/dev/hidraw2");
    rig.wait_event(|event| {
        matches!(
            event,
            MonitorEvent::Connection(ConnectionState::Disconnected { .. })
        )
        .then_some(())
    });
    let replugged = SimDevice::new(&PLUS);
    rig.bus.plug("/dev/hidraw2", &replugged);
    replugged.wait_until(|device| {
        device.image_count() == 1 && device.features().iter().any(|report| report[2] == 40)
    });
    assert_eq!(replugged.writes(), second.writes());
    assert_eq!(first.image_count(), 1, "the other device is not redrawn");
}

#[test]
fn a_path_is_never_opened_twice_while_its_worker_lives() {
    let (rig, _plus, _xl) = pair();
    let seen = rig.bus.enumerations();
    let deadline = Instant::now() + Duration::from_secs(10);
    while rig.bus.enumerations() < seen + 40 {
        assert!(Instant::now() < deadline, "the hub stopped enumerating");
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(rig.bus.opens("/dev/hidraw1"), 1);
    assert_eq!(rig.bus.opens("/dev/hidraw2"), 1);
}

#[test]
fn the_enumeration_order_changes_nothing() {
    let mut orders = Vec::new();
    for reversed in [false, true] {
        let bus = Bus::default();
        if reversed {
            bus.reverse_order();
        }
        let devices = [
            ("/dev/hidraw3", SimDevice::new(&PLUS)),
            ("/dev/hidraw1", SimDevice::new(&PLUS_XL)),
            ("/dev/hidraw2", SimDevice::new(&PLUS)),
        ];
        for (path, device) in &devices {
            bus.plug(path, device);
        }
        let mut rig = Rig::over(bus, quick());
        for _ in 0..3 {
            rig.wait_event(|event| {
                matches!(
                    event,
                    MonitorEvent::Connection(ConnectionState::Connected { .. })
                )
                .then_some(())
            });
        }
        orders.push(
            connections(&rig)
                .into_iter()
                .filter(ConnectionState::is_connected)
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(orders[0], orders[1]);
    assert_eq!(
        orders[0],
        [
            connected_as("/dev/hidraw1", &PLUS_XL),
            connected_as("/dev/hidraw2", &PLUS),
            connected_as("/dev/hidraw3", &PLUS)
        ]
    );
}

#[test]
fn a_planned_product_id_next_to_a_supported_one_is_never_opened() {
    let (planned, plus) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS));
    let mut rig = Rig::empty();
    rig.bus.plug_ids("/dev/hidraw1", (VID, 0x006D), &planned);
    rig.bus.plug_ids("/dev/hidraw2", (VID, 0x0084), &plus);
    rig.wait_connection(connected_as("/dev/hidraw2", &PLUS));
    let seen = rig.bus.enumerations();
    while rig.bus.enumerations() < seen + 10 {
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(rig.bus.opens("/dev/hidraw1"), 0);
    assert!(planned.writes().is_empty() && planned.features().is_empty());
}

#[test]
fn shutdown_blanks_both_devices_and_joins_promptly() {
    let (mut rig, plus, xl) = pair();
    let started = Instant::now();
    rig.stop();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "shutdown took {:?}",
        started.elapsed()
    );
    for (device, keys, strip) in [(&plus, 8usize, (800, 100)), (&xl, 36, (100, 1200))] {
        let images = device.images();
        let mut slots: Vec<_> = images
            .iter()
            .filter(|image| image.command == 0x07)
            .map(|image| usize::from(image.slot))
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
    // Idempotent, as the single-device controller always was.
    rig.stop();
}

#[test]
fn a_device_that_fails_to_open_does_not_stop_the_others() {
    let (busy, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
    let mut rig = Rig::empty();
    rig.bus.refuse_open("/dev/hidraw1");
    rig.bus.plug("/dev/hidraw1", &busy);
    rig.bus.plug("/dev/hidraw2", &xl);
    rig.wait_connection(connected_as("/dev/hidraw2", &PLUS_XL));
    assert!(rig.bus.opens("/dev/hidraw1") >= 1);
    assert!(!connections(&rig).contains(&connected_as("/dev/hidraw1", &PLUS)));
    rig.stop();
    assert!(busy.writes().is_empty());
}

#[test]
fn artwork_held_for_a_replaced_model_never_reaches_the_new_one_on_that_port() {
    let scratch = Scratch::new("hub-stale");
    let (mut rig, plus, xl) = pair();
    rig.bus.unplug("/dev/hidraw2");
    rig.wait_event(|event| {
        matches!(
            event,
            MonitorEvent::Connection(ConnectionState::Disconnected { .. })
        )
        .then_some(())
    });
    // Held for an XL that is not there any more...
    rig.send_artwork(&PLUS_XL, assign_key(&scratch, "stale.png", 112));
    // ...and the same port comes back as a +.
    let replacement = SimDevice::new(&PLUS);
    rig.bus.plug("/dev/hidraw2", &replacement);
    rig.wait_connection(connected_as("/dev/hidraw2", &PLUS));
    rig.send_artwork(&PLUS, assign_key(&scratch, "fresh.png", 120));
    replacement.wait_until(|device| device.image_count() == 1);
    plus.wait_until(|device| device.image_count() == 1);
    assert_eq!(replacement.image_count(), 1, "only the fresh picture");
    assert!(xl.writes().is_empty());
}

#[test]
fn the_hub_with_no_devices_searches_and_recovers_when_one_appears() {
    let mut rig = Rig::empty();
    rig.wait_connection(ConnectionState::Searching);
    let reason = rig.wait_event(|event| match event {
        MonitorEvent::Connection(ConnectionState::Unavailable(reason)) => Some(reason.clone()),
        _ => None,
    });
    assert!(reason.starts_with("Device not found"), "{reason}");
    let late = SimDevice::new(&PLUS_XL);
    rig.bus.plug("/dev/hidraw9", &late);
    rig.wait_connection(connected_as("/dev/hidraw9", &PLUS_XL));
    rig.bus.unplug("/dev/hidraw9");
    rig.wait_connection(ConnectionState::Searching);
}

#[test]
fn controls_for_a_model_without_a_device_are_reported_and_replayed_on_attach() {
    let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
    let mut rig = Rig::empty();
    rig.bus.plug("/dev/hidraw1", &plus);
    rig.wait_connection(connected_as("/dev/hidraw1", &PLUS));
    rig.send_control(&PLUS_XL, DeviceAction::Brightness(30));
    let result = rig.wait_event(|event| match event {
        MonitorEvent::DeviceResult {
            device: None,
            result,
            ..
        } => Some(result.clone()),
        _ => None,
    });
    assert!(result.unwrap_err().contains("control intent retained"));
    rig.bus.plug("/dev/hidraw2", &xl);
    xl.wait_until(|device| device.features().iter().any(|report| report[2] == 30));
    assert!(plus.features().iter().all(|report| report[2] == 100));
}

#[test]
fn plugging_and_unplugging_two_devices_under_concurrent_artwork_stays_consistent() {
    let scratch = Scratch::new("hub-stress");
    let mut rig = Rig::empty();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let last = thread::scope(|scope| {
        let sender = {
            let stop = stop.clone();
            let hid = rig.handle();
            scope.spawn(move || {
                let mut sent = 0u32;
                while !stop.load(std::sync::atomic::Ordering::Acquire) {
                    for model in [&PLUS, &PLUS_XL] {
                        let target = ArtworkTarget::Keypad(1 + (sent % 8) as u8);
                        let _ = hid.send_artwork(model, ArtworkCommand::Clear { target });
                        if sent.is_multiple_of(10) {
                            let _ = hid.send_control(model, DeviceAction::AdjustBrightness(1));
                        }
                    }
                    sent += 1;
                    thread::sleep(Duration::from_millis(3));
                }
                sent
            })
        };
        for _ in 0..20 {
            let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
            rig.bus.plug("/dev/hidraw1", &plus);
            rig.bus.plug("/dev/hidraw2", &xl);
            let mut wanted = [
                connected_as("/dev/hidraw1", &PLUS),
                connected_as("/dev/hidraw2", &PLUS_XL),
            ]
            .to_vec();
            rig.wait_event(|event| {
                if let MonitorEvent::Connection(state) = event {
                    wanted.retain(|want| want != state);
                }
                wanted.is_empty().then_some(())
            });
            rig.bus.unplug("/dev/hidraw1");
            rig.bus.unplug("/dev/hidraw2");
            let mut gone = 2;
            rig.wait_event(|event| {
                if matches!(
                    event,
                    MonitorEvent::Connection(ConnectionState::Disconnected { .. })
                ) {
                    gone -= 1;
                }
                (gone == 0).then_some(())
            });
        }
        // The last pair stays; the final picture must reach both.
        let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
        rig.bus.plug("/dev/hidraw1", &plus);
        rig.bus.plug("/dev/hidraw2", &xl);
        stop.store(true, std::sync::atomic::Ordering::Release);
        let sent = sender.join().unwrap();
        assert!(sent > 0, "artwork was being sent throughout");
        rig.wait_event(|event| {
            matches!(
                event,
                MonitorEvent::Connection(ConnectionState::Connected { device, .. })
                    if *device == id("/dev/hidraw2")
            )
            .then_some(())
        });
        (plus, xl)
    });
    let (plus, xl) = last;
    rig.send_artwork(
        &PLUS,
        ArtworkCommand::Replace(vec![assign_key(&scratch, "end-plus.png", 120)]),
    );
    rig.send_artwork(
        &PLUS_XL,
        ArtworkCommand::Replace(vec![assign_key(&scratch, "end-xl.png", 112)]),
    );
    for (device, size) in [(&plus, 120), (&xl, 112)] {
        device.wait_until(|device| {
            device.assemble().0.iter().any(|image| {
                image.command == 0x07 && image.slot == 0 && {
                    let decoded = image.decode();
                    decoded.width() == size && decoded.height() == size
                }
            })
        });
    }
    rig.stop();
    assert_eq!(plus.assemble().1, 0);
    assert_eq!(xl.assemble().1, 0);
}

fn retained_result(rig: &mut Rig, wanted: DeviceAction) {
    let result = rig.wait_event(|event| match event {
        MonitorEvent::DeviceResult {
            device: None,
            action,
            result,
        } if *action == wanted => Some(result.clone()),
        _ => None,
    });
    assert_eq!(
        result,
        Err("HID device disconnected; control intent retained".to_owned())
    );
}

#[test]
fn a_control_sent_before_any_device_is_retained_and_applied_on_the_first_connect() {
    for (action, level) in [(DeviceAction::Brightness(50), 50), (DeviceAction::Sleep, 0)] {
        let mut rig = Rig::empty();
        rig.controls.send(action.clone()).unwrap();
        retained_result(&mut rig, action);
        let device = SimDevice::new(&PLUS);
        rig.bus.plug("/dev/hidraw1", &device);
        device.wait_until(|device| !device.features().is_empty());
        assert_eq!(device.features()[0][..3], [0x03, 0x08, level]);
    }
}

#[test]
fn held_work_of_a_departed_model_neither_leaks_nor_replays_stale() {
    let scratch = Scratch::new("hub-departed");
    let (mut rig, _plus, xl) = pair();
    rig.bus.unplug("/dev/hidraw2");
    rig.wait_event(|event| {
        matches!(
            event,
            MonitorEvent::Connection(ConnectionState::Disconnected { .. })
        )
        .then_some(())
    });
    rig.send_artwork(&PLUS_XL, assign_key(&scratch, "held.png", 112));
    rig.send_control(&PLUS_XL, DeviceAction::Brightness(20));
    // The port comes back as a +: nothing meant for the XL reaches it.
    let swapped = SimDevice::new(&PLUS);
    rig.bus.plug("/dev/hidraw2", &swapped);
    rig.wait_connection(connected_as("/dev/hidraw2", &PLUS));
    swapped.wait_until(|device| !device.features().is_empty());
    assert!(swapped.features().iter().all(|report| report[2] == 100));
    // The XL returns elsewhere much later: no stale picture or brightness.
    rig.bus.unplug("/dev/hidraw2");
    let back = SimDevice::new(&PLUS_XL);
    rig.bus.plug("/dev/hidraw3", &back);
    rig.wait_connection(connected_as("/dev/hidraw3", &PLUS_XL));
    back.wait_until(|device| !device.features().is_empty());
    assert!(back.features().iter().all(|report| report[2] == 100));
    assert!(back.writes().is_empty(), "a stale picture was replayed");
    assert!(xl.writes().is_empty());
}

#[test]
fn held_work_is_kept_when_another_device_of_the_departed_model_remains() {
    let scratch = Scratch::new("hub-kept");
    let (first, second) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS));
    let mut rig = Rig::empty();
    rig.bus.plug("/dev/hidraw1", &first);
    rig.bus.plug("/dev/hidraw2", &second);
    rig.wait_connection(connected_as("/dev/hidraw2", &PLUS));
    rig.send_artwork(&PLUS, assign_key(&scratch, "kept.png", 120));
    first.wait_until(|device| device.image_count() == 1);
    // One + leaves and its port returns as an XL; the other + still lives.
    rig.bus.unplug("/dev/hidraw2");
    let xl = SimDevice::new(&PLUS_XL);
    rig.bus.plug("/dev/hidraw2", &xl);
    rig.wait_connection(connected_as("/dev/hidraw2", &PLUS_XL));
    let third = SimDevice::new(&PLUS);
    rig.bus.plug("/dev/hidraw3", &third);
    third.wait_until(|device| device.image_count() == 1);
    assert!(xl.writes().is_empty());
}

#[test]
fn model_senders_reach_only_their_own_model_and_end_with_the_controller() {
    let scratch = Scratch::new("hub-model-senders");
    let (mut rig, plus, xl) = pair();
    let (plus_artwork, plus_controls) = rig.hid.model_senders(&PLUS);
    let (xl_artwork, _xl_controls) = rig.hid.model_senders(&PLUS_XL);
    plus_artwork
        .send(assign_key(&scratch, "p.png", 120))
        .unwrap();
    plus_controls.send(DeviceAction::Brightness(61)).unwrap();
    xl_artwork.send(assign_key(&scratch, "x.png", 112)).unwrap();
    plus.wait_until(|device| {
        device.image_count() == 1 && device.features().iter().any(|report| report[2] == 61)
    });
    xl.wait_until(|device| device.image_count() == 1);
    assert_eq!(plus.image_count(), 1);
    assert!(xl.features().iter().all(|report| report[2] == 100));
    let started = Instant::now();
    rig.stop();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "routers are joined promptly"
    );
    assert!(plus_artwork
        .send(assign_key(&scratch, "late.png", 120))
        .is_err());
}
