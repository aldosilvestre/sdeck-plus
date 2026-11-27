use super::*;
use crate::{
    device_model::{MINI, PLUS, PLUS_XL},
    event_api::{
        hub, prepare_artwork_for, sim_device::SimDevice, ConnectionState, DeviceId, MonitorEvent,
    },
    local_artwork::ArtworkTarget,
};
use gtk::gdk_pixbuf::Colorspace;
use std::{
    path::PathBuf,
    sync::mpsc::{channel, Receiver},
    time::Instant,
};

const WAIT: Duration = Duration::from_secs(10);

/// Consume events until `wanted` accepts one; bounded in time.
fn wait(events: &Receiver<MonitorEvent>, wanted: impl Fn(&MonitorEvent) -> bool) {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = events.recv_timeout(left).expect("the awaited event");
        if wanted(&event) {
            return;
        }
    }
}

fn virtual_connected(event: &MonitorEvent) -> bool {
    *event
        == MonitorEvent::Connection(ConnectionState::Connected {
            device: DeviceId::from_path(VIRTUAL_DECK_PATH),
            model: &VIRTUAL_CLASSIC,
        })
}

/// A cadence that would keep the hub from listing again during the test.
fn never() -> hub::Cadence {
    hub::Cadence {
        search: Duration::from_secs(3600),
        after_unplug: Duration::from_secs(3600),
    }
}

#[test]
fn an_attach_is_served_without_waiting_for_the_search_interval() {
    let (sender, events) = channel();
    let mut hid = hub::spawn(
        sender,
        |_| Ok(Vec::<hub::Candidate<SimDevice>>::new()),
        prepare_artwork_for,
        never(),
    );
    // The first listing found nothing; the next is an hour away.
    wait(&events, |event| {
        matches!(
            event,
            MonitorEvent::Connection(ConnectionState::Unavailable(_))
        )
    });
    let (deck, _link) = virtual_deck_pair();
    hid.attach_virtual(deck);
    wait(&events, virtual_connected);
    hid.shutdown();
}

#[test]
fn a_failing_hid_listing_does_not_hide_the_attached_deck() {
    let (sender, events) = channel();
    let mut hid = hub::spawn(
        sender,
        |_| Err::<Vec<hub::Candidate<SimDevice>>, _>("HID initialization failed: denied".into()),
        prepare_artwork_for,
        never(),
    );
    wait(&events, |event| {
        *event
            == MonitorEvent::Connection(ConnectionState::Unavailable(
                "HID initialization failed: denied".into(),
            ))
    });
    let (deck, _link) = virtual_deck_pair();
    hid.attach_virtual(deck);
    wait(&events, virtual_connected);
    hid.shutdown();
}

#[test]
fn shutdown_blanks_the_deck_then_unplugs_it() {
    let (sender, events) = channel();
    let mut hid = hub::spawn(
        sender,
        |_| Ok(Vec::<hub::Candidate<SimDevice>>::new()),
        prepare_artwork_for,
        never(),
    );
    let (deck, link) = virtual_deck_pair();
    let frames = link.frames();
    hid.attach_virtual(deck);
    wait(&events, virtual_connected);
    hid.shutdown();
    let seen: Vec<_> = frames.try_iter().collect();
    // Opened blank, brightness restored, then blanked like a real Classic.
    assert_eq!(seen.first(), Some(&VirtualFrame::Reset));
    assert!(seen.contains(&VirtualFrame::Brightness(0)));
    let blanked: Vec<u8> = seen
        .iter()
        .filter_map(|frame| match frame {
            VirtualFrame::Key { key, .. } => Some(*key),
            _ => None,
        })
        .collect();
    assert_eq!(blanked, (1..=15).collect::<Vec<u8>>());
    assert_eq!(seen.last(), Some(&VirtualFrame::Reset));
    assert_eq!(link.snapshot(), [VirtualFrame::Reset]);
}

#[test]
fn a_second_frames_call_takes_the_stream_over() {
    let (_deck, link) = virtual_deck_pair();
    let first = link.frames();
    let second = link.clone().frames();
    assert!(first.recv_timeout(WAIT).is_err(), "the first stream ended");
    drop(_deck);
    assert_eq!(second.recv_timeout(WAIT), Ok(VirtualFrame::Reset));
}

#[test]
fn chunks_out_of_order_or_for_other_displays_are_dropped() {
    let model = &VIRTUAL_CLASSIC;
    let picture: Vec<u8> = (0..3000u32).map(|byte| byte as u8).collect();
    let reports =
        image_output::encode_reports_for(model, ArtworkTarget::Keypad(2), &picture).unwrap();
    assert_eq!(reports.len(), 3);

    let mut assembly = Assembly::default();
    let finished: Vec<_> = reports
        .iter()
        .filter_map(|report| assembly.add(model, &report.0))
        .collect();
    assert_eq!(finished, [(0x07, 1, picture.clone())]);

    // A missing middle chunk drops the whole picture.
    let mut assembly = Assembly::default();
    assert_eq!(assembly.add(model, &reports[0].0), None);
    assert_eq!(assembly.add(model, &reports[2].0), None);
    assert!(assembly.0.is_empty());
    // A strip report and a key report from another family are not pictures here.
    let strip =
        image_output::encode_reports_for(&PLUS, ArtworkTarget::EncoderZone(1), &picture).unwrap();
    assert_eq!(assembly.add(model, &strip[0].0), None);
    assert_eq!(assembly.add(model, &[0x01, 0x00, 0x00]), None);

    let background =
        image_output::encode_reports_for(model, ArtworkTarget::TouchStripBackground, &picture)
            .unwrap();
    let finished: Vec<_> = background
        .iter()
        .filter_map(|report| assembly.add(model, &report.0))
        .collect();
    assert_eq!(finished, [(0x08, 0, picture)]);
}

#[test]
fn brightness_is_read_in_the_layout_of_the_model_family() {
    let expanded = protocol_for(VIRTUAL_CLASSIC.family);
    for percent in [0, 40, 100] {
        assert_eq!(
            brightness_in(&VIRTUAL_CLASSIC, &expanded.brightness_report(percent)),
            Some(percent)
        );
    }
    assert_eq!(
        brightness_in(&VIRTUAL_CLASSIC, &expanded.sleep_timer_report(300)),
        None
    );
    assert_eq!(
        brightness_in(&VIRTUAL_CLASSIC, &expanded.show_logo_report()),
        None
    );
    assert_eq!(brightness_in(&VIRTUAL_CLASSIC, &[0x03, 0x08, 40]), None);
    let mini = protocol_for(MINI.family);
    assert_eq!(brightness_in(&MINI, &mini.brightness_report(7)), Some(7));
    assert_eq!(brightness_in(&MINI, &expanded.brightness_report(7)), None);
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether only the top-left quarter of the decoded `jpeg` is light.
fn top_left_only(jpeg: &[u8]) -> bool {
    let image = Pixbuf::from_read(std::io::Cursor::new(jpeg.to_vec())).unwrap();
    let pixels = image.read_pixel_bytes();
    let light = |x: i32, y: i32| {
        let at = (y * image.height() / 4 * image.rowstride()
            + x * image.width() / 4 * image.n_channels()) as usize;
        pixels[at] > 200
    };
    light(1, 1) && !light(3, 1) && !light(1, 3) && !light(3, 3)
}

#[test]
fn key_pictures_are_turned_back_for_every_rotation_and_flip() {
    let scratch = Scratch(std::env::temp_dir().join(format!(
        "sdeck-virtual-upright-{}-{:?}",
        std::process::id(),
        Instant::now()
    )));
    std::fs::create_dir_all(&scratch.0).unwrap();
    let source = scratch.0.join("corner.png");
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, 200, 200).unwrap();
    image.fill(0x000000ff);
    for y in 0..100 {
        for x in 0..100 {
            image.put_pixel(x, y, 255, 255, 255, 255);
        }
    }
    image.savev(&source, "png", &[]).unwrap();
    // Rot180 (Classic), Ccw90 (+ XL), Ccw90 then a flip in BMP (Mini), none (+).
    for model in [&VIRTUAL_CLASSIC, &PLUS_XL, &MINI, &PLUS] {
        let sent =
            image_output::prepare_jpeg_for(model, ArtworkTarget::Keypad(1), &source).unwrap();
        let frame = upright_frame(model, 0x07, 0, sent).unwrap();
        let VirtualFrame::Key { key: 1, jpeg } = frame else {
            panic!("{}: not a picture of key 1", model.id);
        };
        assert!(top_left_only(&jpeg), "{}", model.id);
    }
    assert!(upright_frame(&VIRTUAL_CLASSIC, 0x07, 15, vec![1]).is_err());
}
