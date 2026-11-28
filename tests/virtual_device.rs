//! The virtual Stream Deck Classic served by the real HID hub: what a viewer
//! presses arrives as key input of the `virtual-classic` model, what the
//! application draws for that model comes back as upright pictures, and
//! detaching it is an unplug. The hub runs over an empty simulated bus (plus a
//! simulated real Classic where the two must coexist): no hardware, HID stack
//! or display server.

use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use sdeck_plus::{
    device_control::DeviceAction,
    device_model::{DeviceModel, CLASSIC, VIRTUAL_CLASSIC},
    event_api::{
        test_support::{key_report, start_hub, SimBus, SimDevice},
        virtual_deck_pair, ArtworkCommand, ConnectionState, DeviceId, HidController, MonitorEvent,
        VirtualDeckLink, VirtualFrame, VIRTUAL_DECK_PATH,
    },
    local_artwork::ArtworkTarget,
    protocol::Event,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const WAIT: Duration = Duration::from_secs(10);

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sdeck-virtual-device-{}-{name}-{unique}",
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

/// The hub over `bus`, with its event stream.
fn hub(bus: &SimBus) -> (HidController, Receiver<MonitorEvent>) {
    let (sender, events) = channel();
    (start_hub(bus, sender), events)
}

fn virtual_id() -> DeviceId {
    DeviceId::from_path(VIRTUAL_DECK_PATH)
}

/// Consume events until `pick` accepts one; bounded in time.
fn wait_event<T>(
    events: &Receiver<MonitorEvent>,
    mut pick: impl FnMut(&MonitorEvent) -> Option<T>,
) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = events
            .recv_timeout(left)
            .expect("the awaited monitor event");
        if let Some(found) = pick(&event) {
            return found;
        }
    }
}

fn wait_connected(events: &Receiver<MonitorEvent>, device: DeviceId, model: &DeviceModel) {
    wait_event(events, |event| match event {
        MonitorEvent::Connection(ConnectionState::Connected {
            device: found,
            model: connected,
        }) => (*found == device && *connected == model).then_some(()),
        _ => None,
    });
}

/// The next `count` inputs of `model`, in the order they arrived.
fn inputs_of(events: &Receiver<MonitorEvent>, model: &DeviceModel, count: usize) -> Vec<Event> {
    let mut found = Vec::new();
    wait_event(events, |event| {
        if let MonitorEvent::Input {
            model: from, event, ..
        } = event
        {
            if *from == model {
                found.push(event.clone());
            }
        }
        (found.len() == count).then_some(())
    });
    found
}

/// Consume frames until `pick` accepts one; bounded in time.
fn wait_frame<T>(
    frames: &Receiver<VirtualFrame>,
    mut pick: impl FnMut(&VirtualFrame) -> Option<T>,
) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let frame = frames.recv_timeout(left).expect("the awaited frame");
        if let Some(found) = pick(&frame) {
            return found;
        }
    }
}

/// A picture whose top-left quarter is white and the rest black, so a turned
/// or mirrored copy shows.
fn corner_picture(dir: &Path, name: &str, (width, height): (i32, i32)) -> PathBuf {
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, width, height).unwrap();
    image.fill(0x000000ff);
    for y in 0..height / 2 {
        for x in 0..width / 2 {
            image.put_pixel(x as u32, y as u32, 255, 255, 255, 255);
        }
    }
    let path = dir.join(name);
    image.savev(&path, "png", &[]).unwrap();
    path
}

/// The decoded size of `jpeg` and whether only its top-left quarter is light.
fn upright_corner(jpeg: &[u8]) -> ((i32, i32), bool) {
    let image = Pixbuf::from_read(std::io::Cursor::new(jpeg.to_vec())).expect("a decodable JPEG");
    let (width, height) = (image.width(), image.height());
    let light = |x: i32, y: i32| {
        let pixels = image.read_pixel_bytes();
        let at = (y * image.rowstride() + x * image.n_channels()) as usize;
        pixels[at] > 200 && pixels[at + 1] > 200 && pixels[at + 2] > 200
    };
    let quarter = |x: i32, y: i32| light(x * width / 4, y * height / 4);
    (
        (width, height),
        quarter(1, 1) && !quarter(3, 1) && !quarter(1, 3) && !quarter(3, 3),
    )
}

#[test]
fn the_link_is_cloneable_and_sendable() {
    fn shareable<T: Clone + Send + 'static>() {}
    shareable::<VirtualDeckLink>();
    fn sendable<T: Send + 'static>() {}
    sendable::<sdeck_plus::event_api::VirtualDeck>();
    sendable::<VirtualFrame>();
}

#[test]
fn a_press_on_the_link_reaches_the_hub_as_a_key_of_the_virtual_classic() {
    let bus = SimBus::default();
    let (hid, events) = hub(&bus);
    let (deck, link) = virtual_deck_pair();
    hid.attach_virtual(deck);
    wait_connected(&events, virtual_id(), &VIRTUAL_CLASSIC);

    link.press(7, true);
    // One Classic key report: the state of every one of the 15 keys.
    let down = inputs_of(&events, &VIRTUAL_CLASSIC, 15);
    for (index, event) in down.iter().enumerate() {
        assert_eq!(
            *event,
            Event::Key {
                index,
                pressed: index == 6
            }
        );
    }
    link.press(7, false);
    let up = inputs_of(&events, &VIRTUAL_CLASSIC, 15);
    assert!(up
        .iter()
        .all(|event| matches!(event, Event::Key { pressed: false, .. })));

    // Keys the Classic lacks are ignored: the next report is key 1's.
    link.press(0, true);
    link.press(16, true);
    link.press(1, true);
    let next = wait_event(&events, |event| match event {
        MonitorEvent::Input {
            device,
            model,
            event,
        } => {
            assert_eq!((*device, *model), (virtual_id(), &VIRTUAL_CLASSIC));
            Some(event.clone())
        }
        _ => None,
    });
    assert_eq!(
        next,
        Event::Key {
            index: 0,
            pressed: true
        }
    );
}

#[test]
fn artwork_for_the_virtual_classic_comes_back_upright_with_brightness_and_a_snapshot() {
    let scratch = Scratch::new("artwork");
    let bus = SimBus::default();
    let (hid, events) = hub(&bus);
    let (deck, link) = virtual_deck_pair();
    let frames = link.frames();
    hid.attach_virtual(deck);
    wait_connected(&events, virtual_id(), &VIRTUAL_CLASSIC);

    let key = corner_picture(&scratch.0, "key.png", (72, 72));
    hid.send_artwork(
        &VIRTUAL_CLASSIC,
        ArtworkCommand::Assign {
            target: ArtworkTarget::Keypad(3),
            path: key,
        },
    )
    .unwrap();
    wait_event(&events, |event| match event {
        MonitorEvent::ArtworkResult {
            device,
            target: ArtworkTarget::Keypad(3),
            result,
        } => {
            assert_eq!(*device, virtual_id());
            assert_eq!(*result, Ok(()));
            Some(())
        }
        _ => None,
    });
    let key_jpeg = wait_frame(&frames, |frame| match frame {
        VirtualFrame::Key { key: 3, jpeg } => Some(jpeg.clone()),
        VirtualFrame::Key { key, .. } => panic!("unexpected picture for key {key}"),
        _ => None,
    });
    assert_eq!(upright_corner(&key_jpeg), ((72, 72), true));

    let background = corner_picture(&scratch.0, "background.png", (480, 272));
    hid.send_artwork(
        &VIRTUAL_CLASSIC,
        ArtworkCommand::Assign {
            target: ArtworkTarget::TouchStripBackground,
            path: background,
        },
    )
    .unwrap();
    let background_jpeg = wait_frame(&frames, |frame| match frame {
        VirtualFrame::Background { jpeg } => Some(jpeg.clone()),
        _ => None,
    });
    assert_eq!(upright_corner(&background_jpeg), ((480, 272), true));

    hid.send_control(&VIRTUAL_CLASSIC, DeviceAction::Brightness(40))
        .unwrap();
    wait_frame(&frames, |frame| {
        (*frame == VirtualFrame::Brightness(40)).then_some(())
    });

    // A viewer that connects now is told the whole current state.
    let snapshot = link.snapshot();
    assert_eq!(snapshot.first(), Some(&VirtualFrame::Reset));
    assert!(snapshot.contains(&VirtualFrame::Brightness(40)));
    assert!(snapshot.contains(&VirtualFrame::Background {
        jpeg: background_jpeg
    }));
    assert!(snapshot.contains(&VirtualFrame::Key {
        key: 3,
        jpeg: key_jpeg
    }));
    assert!(!snapshot.iter().any(|frame| matches!(
        frame,
        VirtualFrame::Key { key, .. } if *key != 3
    )));
}

#[test]
fn detaching_the_virtual_classic_is_an_unplug() {
    let bus = SimBus::default();
    let (hid, events) = hub(&bus);
    // Nothing attached: detaching changes nothing.
    hid.detach_virtual();
    let (deck, link) = virtual_deck_pair();
    let frames = link.frames();
    hid.attach_virtual(deck);
    wait_connected(&events, virtual_id(), &VIRTUAL_CLASSIC);
    // The hub restores the brightness of every device it opens.
    wait_frame(&frames, |frame| {
        matches!(frame, VirtualFrame::Brightness(_)).then_some(())
    });

    hid.detach_virtual();
    wait_event(&events, |event| match event {
        MonitorEvent::Connection(ConnectionState::Disconnected { device, model, .. }) => {
            assert_eq!((*device, *model), (virtual_id(), &VIRTUAL_CLASSIC));
            Some(())
        }
        MonitorEvent::Input { .. } => panic!("no input after a detach"),
        _ => None,
    });
    wait_frame(&frames, |frame| {
        (*frame == VirtualFrame::Reset).then_some(())
    });
    assert_eq!(link.snapshot(), [VirtualFrame::Reset]);

    // A new deck takes the place; the detached link reaches nothing.
    let (again, fresh) = virtual_deck_pair();
    hid.attach_virtual(again);
    wait_connected(&events, virtual_id(), &VIRTUAL_CLASSIC);
    link.press(2, true);
    fresh.press(5, true);
    let report = inputs_of(&events, &VIRTUAL_CLASSIC, 15);
    assert_eq!(
        report[4],
        Event::Key {
            index: 4,
            pressed: true
        }
    );
    assert_eq!(
        report[1],
        Event::Key {
            index: 1,
            pressed: false
        }
    );
}

#[test]
fn attaching_a_second_deck_unplugs_the_first() {
    let bus = SimBus::default();
    let (hid, events) = hub(&bus);
    let (first, first_link) = virtual_deck_pair();
    hid.attach_virtual(first);
    wait_connected(&events, virtual_id(), &VIRTUAL_CLASSIC);
    let (second, second_link) = virtual_deck_pair();
    hid.attach_virtual(second);
    wait_event(&events, |event| {
        matches!(
            event,
            MonitorEvent::Connection(ConnectionState::Disconnected { device, .. })
                if *device == virtual_id()
        )
        .then_some(())
    });
    wait_connected(&events, virtual_id(), &VIRTUAL_CLASSIC);
    first_link.press(3, true);
    second_link.press(9, true);
    let report = inputs_of(&events, &VIRTUAL_CLASSIC, 15);
    assert!(matches!(report[8], Event::Key { pressed: true, .. }));
    assert!(matches!(report[2], Event::Key { pressed: false, .. }));
}

#[test]
fn a_real_classic_and_the_virtual_classic_run_side_by_side() {
    let scratch = Scratch::new("side-by-side");
    let bus = SimBus::default();
    let classic = SimDevice::new(&CLASSIC);
    bus.plug("classic-usb", &classic);
    let (hid, events) = hub(&bus);
    let (deck, link) = virtual_deck_pair();
    let frames = link.frames();
    hid.attach_virtual(deck);
    let mut waiting = vec![DeviceId::from_path("classic-usb"), virtual_id()];
    wait_event(&events, |event| {
        if let MonitorEvent::Connection(ConnectionState::Connected { device, model }) = event {
            let expected = if *device == virtual_id() {
                &VIRTUAL_CLASSIC
            } else {
                &CLASSIC
            };
            assert_eq!(*model, expected);
            waiting.retain(|id| id != device);
        }
        waiting.is_empty().then_some(())
    });

    // Input is tagged with the device and model it came from.
    classic.send(key_report(&CLASSIC, &[0]));
    let real = inputs_of(&events, &CLASSIC, 15);
    assert_eq!(
        real[0],
        Event::Key {
            index: 0,
            pressed: true
        }
    );
    link.press(15, true);
    let pressed = inputs_of(&events, &VIRTUAL_CLASSIC, 15);
    assert_eq!(
        pressed[14],
        Event::Key {
            index: 14,
            pressed: true
        }
    );

    // Artwork for the real Classic never reaches the virtual one.
    let picture = corner_picture(&scratch.0, "key.png", (72, 72));
    hid.send_artwork(
        &CLASSIC,
        ArtworkCommand::Assign {
            target: ArtworkTarget::Keypad(1),
            path: picture.clone(),
        },
    )
    .unwrap();
    hid.send_artwork(
        &VIRTUAL_CLASSIC,
        ArtworkCommand::Assign {
            target: ArtworkTarget::Keypad(2),
            path: picture,
        },
    )
    .unwrap();
    let first = wait_frame(&frames, |frame| match frame {
        VirtualFrame::Key { key, .. } => Some(*key),
        _ => None,
    });
    assert_eq!(first, 2);
    classic.wait_until(|device| device.last_key_jpeg(0).is_some());
    assert!(classic.last_key_jpeg(1).is_none());
}
