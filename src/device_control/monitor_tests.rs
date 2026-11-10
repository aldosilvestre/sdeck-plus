use super::*;
use std::collections::VecDeque;
use std::sync::mpsc::channel;

struct ShutdownDevice {
    entered: Sender<()>,
    released: Receiver<()>,
    reports: std::cell::RefCell<Vec<Vec<u8>>>,
    dropped: Sender<Vec<Vec<u8>>>,
}

impl Device for ShutdownDevice {
    fn read(&self, _: &mut [u8]) -> Result<usize, String> {
        self.entered.send(()).unwrap();
        self.released.recv_timeout(Duration::from_secs(5)).unwrap();
        Ok(0)
    }

    fn write(&self, report: &[u8]) -> Result<usize, String> {
        self.reports.borrow_mut().push(report.to_vec());
        Ok(report.len())
    }

    fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
        self.reports.borrow_mut().push(report.to_vec());
        Ok(report.len())
    }
}

impl Drop for ShutdownDevice {
    fn drop(&mut self) {
        self.dropped.send(self.reports.take()).unwrap();
    }
}

#[test]
fn shutdown_turns_off_and_clears_every_display_before_releasing_device() {
    // Given: an idle connected worker, with pending artwork and controls.
    let (events, _receiver) = channel();
    let (entered, entry) = channel();
    let (release, released) = channel();
    let (dropped, reports) = channel();
    let mut device = Some(ShutdownDevice {
        entered,
        released,
        reports: Default::default(),
        dropped,
    });
    let mut controller = tests::single(
        events,
        move |_| Ok(device.take().unwrap()),
        |_, _| panic!("pending artwork must not be prepared"),
    );
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    controller
        .send_artwork(
            &PLUS,
            ArtworkCommand::Assign {
                target: ArtworkTarget::Keypad(1),
                path: "must-not-upload.png".into(),
            },
        )
        .unwrap();
    controller.send_control(&PLUS, DeviceAction::Wake).unwrap();

    // When: full shutdown cancels ordinary work, then joins the device owner.
    controller.request_stop();
    release.send(()).unwrap();
    controller.shutdown();
    controller.shutdown();

    // Then: brightness goes off and nine complete black JPEGs precede device drop.
    let reports = reports.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(reports[0][..3], [0x03, 0x08, 100]);
    assert_eq!(reports[1][..3], [0x03, 0x08, 0]);
    let mut completed = Vec::new();
    let mut jpeg = Vec::new();
    for report in &reports[2..] {
        assert_eq!(report.len(), image_output::REPORT_SIZE);
        let size = usize::from(u16::from_le_bytes([report[4], report[5]]));
        jpeg.extend_from_slice(&report[8..8 + size]);
        if report[3] == 1 {
            let image =
                gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(std::mem::take(&mut jpeg)))
                    .unwrap();
            let dimensions = if report[1] == 0x07 {
                (120, 120)
            } else {
                (800, 100)
            };
            assert_eq!((image.width(), image.height()), dimensions);
            assert!(image
                .read_pixel_bytes()
                .as_ref()
                .iter()
                .all(|byte| *byte < 8));
            completed.push((report[1], report[2]));
        }
    }
    assert!(jpeg.is_empty());
    assert_eq!(
        completed,
        (0..8)
            .map(|key| (0x07, key))
            .chain(std::iter::once((0x0b, 0)))
            .collect::<Vec<_>>()
    );
    assert!(controller.hub.is_none());
}

struct FeatureDevice {
    entered: Sender<()>,
    released: Receiver<Result<usize, String>>,
    features: Sender<Vec<u8>>,
}

impl Device for FeatureDevice {
    fn read(&self, _: &mut [u8]) -> Result<usize, String> {
        self.entered.send(()).unwrap();
        self.released.recv_timeout(Duration::from_secs(5)).unwrap()
    }

    fn write(&self, _: &[u8]) -> Result<usize, String> {
        Ok(image_output::REPORT_SIZE)
    }

    fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
        self.features.send(report.to_vec()).unwrap();
        Ok(report.len())
    }
}

#[test]
fn worker_replays_disconnected_adjustment_on_the_same_device_owner() {
    // Given: two device generations, with reads controlled by exact signals.
    let (events, receiver) = channel();
    let (features, reports) = channel();
    let (entered, entry) = channel();
    let (release, released) = channel();
    let (entered2, entry2) = channel();
    let (release2, released2) = channel();
    let mut devices = VecDeque::from([
        FeatureDevice {
            entered,
            released,
            features: features.clone(),
        },
        FeatureDevice {
            entered: entered2,
            released: released2,
            features,
        },
    ]);
    let mut hid = tests::single(
        events,
        move |_| Ok(devices.pop_front().unwrap()),
        |_, _| panic!("unexpected artwork preparation"),
    );
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        reports.recv_timeout(Duration::from_secs(5)).unwrap()[2],
        100
    );
    hid.send_control(&PLUS, DeviceAction::Brightness(44))
        .unwrap();
    release.send(Ok(0)).unwrap();
    let first = reports.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first.len(), 32);
    assert_eq!(first[2], 44);
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    hid.send_control(&PLUS, DeviceAction::AdjustBrightness(-4))
        .unwrap();
    // When: a pending relative command crosses the physical disconnection.
    release.send(Err("unplugged".into())).unwrap();
    let replayed = reports.recv_timeout(Duration::from_secs(5)).unwrap();
    entry2.recv_timeout(Duration::from_secs(5)).unwrap();
    hid.request_stop();
    release2.send(Ok(0)).unwrap();
    hid.shutdown();
    // Then: the second HID generation gets the final state once, not the step twice.
    assert_eq!(replayed.len(), 32);
    assert_eq!(replayed[2], 40);
    assert_eq!(reports.try_recv().unwrap()[..3], [0x03, 0x08, 0]);
    assert!(reports.try_recv().is_err());
    let results: Vec<_> = receiver
        .try_iter()
        .filter_map(|event| match event {
            MonitorEvent::DeviceResult { action, result, .. } => Some((action, result)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0], (DeviceAction::Brightness(44), Ok(())));
    assert_eq!(results[1].0, DeviceAction::AdjustBrightness(-4));
    assert!(results[1].1.is_err());
    assert_eq!(results[2], (DeviceAction::AdjustBrightness(-4), Ok(())));
    assert!(hid.send_control(&PLUS, DeviceAction::Wake).is_err());
}
