use super::*;
use std::collections::VecDeque;
use std::sync::mpsc::channel;

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
        panic!("unexpected artwork write")
    }

    fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
        self.features.send(report.to_vec()).unwrap();
        Ok(report.len())
    }
}

#[test]
fn worker_replays_disconnected_adjustment_on_the_same_device_owner() {
    // Given: two device generations, with reads controlled by exact signals.
    let stop = StopSignal::default();
    let worker_stop = stop.clone();
    let (events, receiver) = channel();
    let (_artwork, artwork) = channel();
    let (controls, control_commands) = channel();
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
    let worker = thread::spawn(move || {
        monitor(
            events,
            (artwork, control_commands),
            worker_stop,
            |_| Ok(devices.pop_front().unwrap()),
            |_| panic!("unexpected artwork preparation"),
        );
    });
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    controls.send(DeviceAction::Brightness(44)).unwrap();
    release.send(Ok(0)).unwrap();
    let first = reports.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first.len(), 32);
    assert_eq!(first[2], 44);
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    controls.send(DeviceAction::AdjustBrightness(-4)).unwrap();
    // When: a pending relative command crosses the physical disconnection.
    release.send(Err("unplugged".into())).unwrap();
    let replayed = reports.recv_timeout(Duration::from_secs(5)).unwrap();
    entry2.recv_timeout(Duration::from_secs(5)).unwrap();
    stop.request();
    release2.send(Ok(0)).unwrap();
    worker.join().unwrap();
    // Then: the second HID generation gets the final state once, not the step twice.
    assert_eq!(replayed.len(), 32);
    assert_eq!(replayed[2], 40);
    assert!(reports.try_recv().is_err());
    let results: Vec<_> = receiver
        .try_iter()
        .filter_map(|event| match event {
            MonitorEvent::DeviceResult { action, result } => Some((action, result)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0], (DeviceAction::Brightness(44), Ok(())));
    assert_eq!(results[1].0, DeviceAction::AdjustBrightness(-4));
    assert!(results[1].1.is_err());
    assert_eq!(results[2], (DeviceAction::AdjustBrightness(-4), Ok(())));
    assert!(controls.send(DeviceAction::Wake).is_err());
}
