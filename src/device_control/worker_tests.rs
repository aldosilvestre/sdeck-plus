use super::super::{pending_artwork, ArtworkCommand};
use super::*;
use crate::local_artwork::ArtworkTarget;
use std::{cell::RefCell, sync::mpsc::channel};

struct RecordingDevice {
    reports: RefCell<Vec<Vec<u8>>>,
    outcome: Result<usize, String>,
    stop_on_write: Option<StopSignal>,
}

impl Device for RecordingDevice {
    fn read(&self, _: &mut [u8]) -> Result<usize, String> {
        panic!("unexpected input read")
    }

    fn write(&self, _: &[u8]) -> Result<usize, String> {
        panic!("controls must not use output reports")
    }

    fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
        self.reports.borrow_mut().push(report.to_vec());
        if let Some(stop) = &self.stop_on_write {
            stop.request();
        }
        self.outcome.clone()
    }
}

fn device(outcome: Result<usize, String>) -> RecordingDevice {
    RecordingDevice {
        reports: RefCell::default(),
        outcome,
        stop_on_write: None,
    }
}

fn results(events: Receiver<MonitorEvent>) -> Vec<(DeviceAction, Result<(), String>)> {
    events
        .try_iter()
        .filter_map(|event| match event {
            MonitorEvent::DeviceResult { action, result } => Some((action, result)),
            _ => None,
        })
        .collect()
}

#[test]
fn startup_does_not_write_when_no_control_was_issued() {
    // Given
    let (_sender, receiver) = channel();
    let controls = Controls::new(receiver);
    let device = device(Ok(32));
    let (events, _receiver) = channel();
    // When
    controls.replay(&StopSignal::default(), &events, &device);
    // Then
    assert!(device.reports.borrow().is_empty());
}

#[test]
fn disconnected_controls_replay_final_intent_without_reapplying_steps() {
    // Given
    let (sender, receiver) = channel();
    let mut controls = Controls::new(receiver);
    let stop = StopSignal::default();
    let (events, receiver) = channel();
    for action in [
        DeviceAction::Brightness(80),
        DeviceAction::AdjustBrightness(-15),
    ] {
        sender.send(action).unwrap();
    }
    controls.drain(&stop, &events, None);
    let disconnected = results(receiver);
    assert_eq!(disconnected.len(), 2);
    assert!(disconnected.iter().all(|(_, result)| result.is_err()));
    let device = device(Ok(32));
    let (events, receiver) = channel();
    // When
    controls.replay(&stop, &events, &device);
    controls.replay(&stop, &events, &device);
    // Then
    assert_eq!(
        device
            .reports
            .borrow()
            .iter()
            .map(|r| r[2])
            .collect::<Vec<_>>(),
        [65, 65]
    );
    assert_eq!(
        results(receiver),
        [
            (DeviceAction::AdjustBrightness(-15), Ok(())),
            (DeviceAction::AdjustBrightness(-15), Ok(())),
        ]
    );
}

#[test]
fn sleep_and_wake_reports_preserve_desired_brightness() {
    // Given
    let (sender, receiver) = channel();
    let mut controls = Controls::new(receiver);
    for action in [
        DeviceAction::Brightness(37),
        DeviceAction::Sleep,
        DeviceAction::Wake,
    ] {
        sender.send(action).unwrap();
    }
    let device = device(Ok(32));
    let (events, receiver) = channel();
    // When
    controls.drain(&StopSignal::default(), &events, Some(&device));
    // Then
    assert_eq!(
        device
            .reports
            .borrow()
            .iter()
            .map(|r| r[2])
            .collect::<Vec<_>>(),
        [37, 0, 37]
    );
    assert_eq!(results(receiver).len(), 3);
}

#[test]
fn artwork_replacement_cannot_discard_pending_controls() {
    // Given
    let (sender, receiver) = channel();
    let mut controls = Controls::new(receiver);
    sender.send(DeviceAction::Sleep).unwrap();
    let (artwork, snapshots) = channel();
    artwork.send(ArtworkCommand::Replace(vec![])).unwrap();
    artwork
        .send(ArtworkCommand::Replace(vec![ArtworkCommand::Clear {
            target: ArtworkTarget::Keypad(1),
        }]))
        .unwrap();
    let device = device(Ok(32));
    let (events, receiver) = channel();
    // When
    pending_artwork(&snapshots);
    controls.drain(&StopSignal::default(), &events, Some(&device));
    // Then
    assert_eq!(results(receiver), [(DeviceAction::Sleep, Ok(()))]);
    assert_eq!(device.reports.borrow()[0][2], 0);
}

#[test]
fn stopped_worker_cancels_pending_controls_and_replay() {
    // Given
    let (sender, receiver) = channel();
    let mut controls = Controls::new(receiver);
    let stop = StopSignal::default();
    let (events, receiver) = channel();
    sender.send(DeviceAction::Sleep).unwrap();
    controls.drain(&stop, &events, None);
    sender.send(DeviceAction::Wake).unwrap();
    let device = device(Ok(32));
    stop.request();
    // When
    controls.drain(&stop, &events, Some(&device));
    controls.replay(&stop, &events, &device);
    // Then
    assert!(device.reports.borrow().is_empty());
    assert_eq!(results(receiver).len(), 1);
}

#[test]
fn stop_from_feature_callback_prevents_remaining_control_writes() {
    // Given
    let (sender, receiver) = channel();
    let mut controls = Controls::new(receiver);
    sender.send(DeviceAction::Sleep).unwrap();
    sender.send(DeviceAction::Wake).unwrap();
    let stop = StopSignal::default();
    let mut device = device(Ok(32));
    device.stop_on_write = Some(stop.clone());
    let (events, receiver) = channel();
    // When
    controls.drain(&stop, &events, Some(&device));
    // Then
    assert_eq!(device.reports.borrow().len(), 1);
    assert!(results(receiver).is_empty());
}

#[test]
fn consumer_loss_prevents_feature_admission() {
    // Given
    let (sender, receiver) = channel();
    let mut controls = Controls::new(receiver);
    sender.send(DeviceAction::Sleep).unwrap();
    let (events, receiver) = channel();
    drop(receiver);
    let stop = StopSignal::default();
    let device = device(Ok(32));
    // When
    controls.drain(&stop, &events, Some(&device));
    // Then
    assert!(device.reports.borrow().is_empty());
    assert!(!stop.live());
}

#[test]
fn failed_and_short_feature_writes_report_failure_but_retain_intent() {
    // Given
    for outcome in [Ok(31), Err("device unplugged".into())] {
        let (sender, receiver) = channel();
        let mut controls = Controls::new(receiver);
        sender.send(DeviceAction::Brightness(61)).unwrap();
        let failed_device = device(outcome);
        let stop = StopSignal::default();
        let (events, receiver) = channel();
        // When
        controls.drain(&stop, &events, Some(&failed_device));
        // Then
        let reported = results(receiver);
        assert_eq!(reported[0].0, DeviceAction::Brightness(61));
        assert!(reported[0].1.is_err());
        let reconnected = device(Ok(32));
        let (events, receiver) = channel();
        controls.replay(&stop, &events, &reconnected);
        assert_eq!(reconnected.reports.borrow()[0][2], 61);
        assert_eq!(results(receiver), [(DeviceAction::Brightness(61), Ok(()))]);
    }
}
