//! Typed in-process boundary from the HID producer to application consumers.
//! External transports can be attached behind this API in a later phase.

use crate::{
    device_control::DeviceAction,
    image_output,
    local_artwork::ArtworkTarget,
    protocol::{decode_report, Event},
};
use std::{
    path::PathBuf,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const VID: u16 = 0x0FD9;
const PID: u16 = 0x0084;

#[path = "device_control/worker.rs"]
mod device_controls;

#[cfg(test)]
#[path = "device_control/monitor_tests.rs"]
mod control_monitor_tests;

mod marquee;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Searching,
    Connected,
    Unavailable(String),
    Disconnected(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitorEvent {
    Connection(ConnectionState),
    Input(Event),
    ArtworkResult {
        target: ArtworkTarget,
        result: Result<(), String>,
    },
    DeviceResult {
        action: DeviceAction,
        result: Result<(), String>,
    },
    AppEvent {
        name: String,
    },
    LayerError(String),
    /// Result of an explicit Save binding request, after persistence finishes.
    BindingSaveResult(Result<(), String>),
    /// Result of an explicit profile save, after persistence finishes.
    ProfileSaveResult(Result<(), String>),
    ConfigurationExportResult {
        path: PathBuf,
        result: Result<(), String>,
    },
    ConfigurationImportResult {
        root: PathBuf,
        result: Result<Option<PathBuf>, String>,
    },
    KeySwapResult {
        layer: String,
        first: u8,
        second: u8,
        result: Result<(), String>,
    },
    ApplicationFocus(Result<Option<crate::app_focus::FocusedApplication>, String>),
    /// Latest reading of a live information source.
    Info {
        source: crate::info::InfoSource,
        value: crate::info::InfoValue,
    },
    /// An action finished running, so live readings it changes are stale.
    ActionDispatched,
    /// Liveness probe; consumers ignore this message.
    Heartbeat,
}

#[derive(Debug, Clone)]
pub enum ArtworkCommand {
    /// Complete desired image state. A newer pending snapshot supersedes older work.
    Replace(Vec<ArtworkCommand>),
    Assign {
        target: ArtworkTarget,
        path: PathBuf,
    },
    Clear {
        target: ArtworkTarget,
    },
    ComposeButton {
        key: u8,
        path: Option<PathBuf>,
        text: crate::button_text::ButtonText,
    },
    /// Complete stateless touch-zone foreground with page-availability indicators.
    ComposeTouchscreenZone {
        zone: u8,
        path: Option<PathBuf>,
        strip_path: Option<PathBuf>,
        value: Option<crate::info::InfoValue>,
        previous: bool,
        next: bool,
    },
    /// A key or touch section without artwork of its own showing its
    /// binding's icon, derived at render time and never stored. Touch
    /// sections draw over the strip crop with page indicators last.
    ComposeIcon {
        target: ArtworkTarget,
        icon: crate::binding_icons::BindingIcon,
        caption: String,
        /// The key's user button text, kept exactly where it was placed.
        text: Option<crate::button_text::ButtonText>,
        strip_path: Option<PathBuf>,
        previous: bool,
        next: bool,
    },
    /// A live information picture replaces a key or touch-zone image.
    ComposeInfo {
        target: ArtworkTarget,
        /// The target's own artwork, drawn under the picture.
        path: Option<PathBuf>,
        /// The strip underneath a section with no artwork of its own.
        strip_path: Option<PathBuf>,
        value: crate::info::InfoValue,
    },
}

/// Cooperative cancellation and report admission share one lock. The lock is never
/// held across HID calls: an admitted call may finish, and callbacks may request stop.
#[derive(Clone, Default)]
struct StopSignal(Arc<(Mutex<bool>, Condvar)>);

impl StopSignal {
    fn request(&self) {
        *self.0 .0.lock().unwrap() = true;
        self.0 .1.notify_all();
    }

    fn live(&self) -> bool {
        !*self.0 .0.lock().unwrap()
    }

    fn wait(&self, duration: Duration) -> bool {
        let (stopped, _) = self
            .0
             .1
            .wait_timeout_while(self.0 .0.lock().unwrap(), duration, |stopped| !*stopped)
            .unwrap();
        !*stopped
    }
}

/// Owns the HID worker; dropping it cancels, joins and releases the device.
pub struct HidController {
    stop: StopSignal,
    worker: Option<thread::JoinHandle<()>>,
    artwork: Sender<ArtworkCommand>,
    controls: Sender<DeviceAction>,
}

impl HidController {
    pub fn control_sender(&self) -> Sender<DeviceAction> {
        self.controls.clone()
    }

    pub fn artwork_sender(&self) -> Sender<ArtworkCommand> {
        self.artwork.clone()
    }

    pub fn request_stop(&self) {
        self.stop.request();
    }

    pub fn shutdown(&mut self) {
        self.request_stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for HidController {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Start the blocking HID producer on a dedicated, owned thread.
pub fn start_hid_monitor(sender: Sender<MonitorEvent>) -> HidController {
    let (artwork, commands) = std::sync::mpsc::channel();
    let (controls, control_commands) = std::sync::mpsc::channel();
    let stop = StopSignal::default();
    let worker_stop = stop.clone();
    let worker = thread::Builder::new()
        .name("streamdeck-hid".into())
        .spawn(move || {
            monitor(
                sender,
                (commands, control_commands),
                worker_stop,
                discover_device,
                prepare_artwork,
            )
        })
        .expect("failed to start HID reader thread");
    HidController {
        stop,
        worker: Some(worker),
        artwork,
        controls,
    }
}

/// Queued work, and whether a snapshot replaced everything shown before it.
fn pending_artwork(commands: &Receiver<ArtworkCommand>) -> (bool, Vec<ArtworkCommand>) {
    let mut pending = Vec::new();
    let mut replaced = false;
    for command in commands.try_iter() {
        match command {
            ArtworkCommand::Replace(snapshot) => {
                pending = snapshot;
                replaced = true;
            }
            command => pending.push(command),
        }
    }
    (replaced, pending)
}

/// The control a command draws on; snapshots have none of their own.
fn command_target(command: &ArtworkCommand) -> Option<ArtworkTarget> {
    match command {
        ArtworkCommand::Assign { target, .. }
        | ArtworkCommand::Clear { target }
        | ArtworkCommand::ComposeInfo { target, .. }
        | ArtworkCommand::ComposeIcon { target, .. } => Some(*target),
        ArtworkCommand::ComposeButton { key, .. } => Some(ArtworkTarget::Keypad(*key)),
        ArtworkCommand::ComposeTouchscreenZone { zone, .. } => {
            Some(ArtworkTarget::EncoderZone(*zone))
        }
        ArtworkCommand::Replace(_) => None,
    }
}

trait Device {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, String>;
    fn write(&self, report: &[u8]) -> Result<usize, String>;
    fn write_feature(&self, report: &[u8]) -> Result<usize, String>;
}

impl Device for hidapi::HidDevice {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, String> {
        self.read_timeout(buffer, 100)
            .map_err(|error| error.to_string())
    }

    fn write(&self, report: &[u8]) -> Result<usize, String> {
        self.write(report).map_err(|error| error.to_string())
    }

    fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
        // hidapi checks the native byte count and returns IncompleteSendError
        // for short sends; its safe API exposes success as ().
        self.send_feature_report(report)
            .map(|()| report.len())
            .map_err(|error| error.to_string())
    }
}

fn discover_device(stop: &StopSignal) -> Result<hidapi::HidDevice, String> {
    if !stop.live() {
        return Err("HID discovery cancelled".into());
    }
    let api =
        hidapi::HidApi::new().map_err(|error| format!("HID initialization failed: {error}"))?;
    let info = api
        .device_list()
        .find(|device| device.vendor_id() == VID && device.product_id() == PID)
        .ok_or_else(|| {
            "Device not found · connect Stream Deck + (VID 0FD9 / PID 0084)".to_string()
        })?;
    if !stop.live() {
        return Err("HID discovery cancelled".into());
    }
    info.open_device(&api)
        .map_err(|error| format!("Device found but could not open: {error}"))
}

fn prepare_artwork(command: ArtworkCommand) -> Result<Vec<image_output::OutputReport>, String> {
    prepare_artwork_at(command, Duration::ZERO)
}

/// Device reports for a command, with a scrolling title `elapsed` into its marquee.
fn prepare_artwork_at(
    command: ArtworkCommand,
    elapsed: Duration,
) -> Result<Vec<image_output::OutputReport>, String> {
    let (target, jpeg) = match command {
        ArtworkCommand::Assign { target, path } => {
            (target, image_output::prepare_jpeg(target, path)?)
        }
        ArtworkCommand::Clear { target } => (target, image_output::prepare_clear_jpeg(target)?),
        ArtworkCommand::ComposeButton { key, path, text } => (
            ArtworkTarget::Keypad(key),
            image_output::prepare_button_jpeg(path.as_deref(), &text)?,
        ),
        ArtworkCommand::ComposeTouchscreenZone {
            zone,
            path,
            strip_path,
            value,
            previous,
            next,
        } => (
            ArtworkTarget::EncoderZone(zone),
            image_output::prepare_touchscreen_zone_jpeg_at(
                zone,
                path.as_deref(),
                strip_path.as_deref(),
                value.as_ref(),
                (previous, next),
                elapsed,
            )?,
        ),
        ArtworkCommand::ComposeInfo {
            target,
            path,
            strip_path,
            value,
        } => (
            target,
            image_output::prepare_info_jpeg_at(
                target,
                &value,
                path.as_deref(),
                strip_path.as_deref(),
                elapsed,
            )?,
        ),
        ArtworkCommand::ComposeIcon {
            target,
            icon,
            caption,
            text,
            strip_path,
            previous,
            next,
        } => (
            target,
            image_output::prepare_icon_jpeg(
                target,
                icon,
                &caption,
                text.as_ref(),
                strip_path.as_deref(),
                previous,
                next,
            )?,
        ),
        ArtworkCommand::Replace(_) => unreachable!("snapshots contain target commands"),
    };
    image_output::encode_reports(target, &jpeg)
}

#[cfg(test)]
fn process_artwork(
    commands: &Receiver<ArtworkCommand>,
    stop: &StopSignal,
    sender: &Sender<MonitorEvent>,
    prepare: impl FnMut(ArtworkCommand) -> Result<Vec<image_output::OutputReport>, String>,
    write: impl FnMut(&[u8]) -> Result<usize, String>,
) {
    let mut marquees = marquee::Marquees::default();
    process_artwork_with(commands, stop, sender, prepare, write, &mut marquees);
}

/// Send queued artwork, keeping `marquees` in step with what each target shows.
fn process_artwork_with(
    commands: &Receiver<ArtworkCommand>,
    stop: &StopSignal,
    sender: &Sender<MonitorEvent>,
    mut prepare: impl FnMut(ArtworkCommand) -> Result<Vec<image_output::OutputReport>, String>,
    mut write: impl FnMut(&[u8]) -> Result<usize, String>,
    marquees: &mut marquee::Marquees,
) {
    if !stop.live() {
        return;
    }
    let (replaced, pending) = pending_artwork(commands);
    if replaced {
        marquees.reset();
    }
    for command in pending {
        if !stop.live() {
            return;
        }
        let Some(target) = command_target(&command) else {
            continue;
        };
        marquees.observe(&command, Instant::now());
        let result = prepare(command).and_then(|reports| {
            transmit_reports(&reports, stop, |report| {
                if sender.send(MonitorEvent::Heartbeat).is_err() {
                    stop.request();
                    return Err("HID consumer disconnected".into());
                }
                write(report)
            })
        });
        if !stop.live() {
            return;
        }
        if result.is_err() {
            marquees.stop(target);
        }
        if sender
            .send(MonitorEvent::ArtworkResult { target, result })
            .is_err()
        {
            stop.request();
            return;
        }
    }
}

/// Send the marquee frames due at `now`. Frames report nothing back unless
/// they fail, which also stops that target's marquee.
fn send_marquee_frames(
    marquees: &mut marquee::Marquees,
    now: Instant,
    stop: &StopSignal,
    sender: &Sender<MonitorEvent>,
    mut prepare: impl FnMut(ArtworkCommand, Duration) -> Result<Vec<image_output::OutputReport>, String>,
    mut write: impl FnMut(&[u8]) -> Result<usize, String>,
) {
    if marquees.is_idle() {
        return;
    }
    for (target, command, elapsed) in marquees.due(now) {
        if !stop.live() {
            return;
        }
        let result = prepare(command, elapsed)
            .and_then(|reports| transmit_reports(&reports, stop, &mut write));
        if !stop.live() {
            return;
        }
        if let Err(error) = result {
            marquees.stop(target);
            if sender
                .send(MonitorEvent::ArtworkResult {
                    target,
                    result: Err(error),
                })
                .is_err()
            {
                stop.request();
                return;
            }
        }
    }
}

fn monitor<D: Device>(
    sender: Sender<MonitorEvent>,
    commands: (Receiver<ArtworkCommand>, Receiver<DeviceAction>),
    stop: StopSignal,
    mut discover: impl FnMut(&StopSignal) -> Result<D, String>,
    mut prepare: impl FnMut(ArtworkCommand) -> Result<Vec<image_output::OutputReport>, String>,
) {
    let (commands, controls) = commands;
    let mut controls = device_controls::Controls::new(controls);
    while stop.live() {
        controls.drain(&stop, &sender, None);
        if !stop.live() {
            return;
        }
        if sender
            .send(MonitorEvent::Connection(ConnectionState::Searching))
            .is_err()
        {
            return;
        }
        let device = match discover(&stop) {
            Ok(device) => device,
            Err(error) => {
                if sender
                    .send(MonitorEvent::Connection(ConnectionState::Unavailable(
                        error,
                    )))
                    .is_err()
                {
                    return;
                }
                if !stop.wait(Duration::from_secs(2)) {
                    return;
                }
                continue;
            }
        };
        // Keep shutdown writes on the HID owner, after ordinary work is cancelled.
        // The session returns on unplug without blanking a disconnected device.
        (|| {
            if !stop.live() {
                return;
            }
            if sender
                .send(MonitorEvent::Connection(ConnectionState::Connected))
                .is_err()
            {
                stop.request();
                return;
            }
            let mut buffer = [0u8; 64];
            // Scrolling titles belong to this connection; a reconnect redraws.
            let mut marquees = marquee::Marquees::default();
            controls.replay(&stop, &sender, &device);
            while stop.live() {
                // A heartbeat detects consumer loss even when there is no input.
                if sender.send(MonitorEvent::Heartbeat).is_err() {
                    stop.request();
                    return;
                }
                controls.drain(&stop, &sender, Some(&device));
                process_artwork_with(
                    &commands,
                    &stop,
                    &sender,
                    &mut prepare,
                    |report| device.write(report),
                    &mut marquees,
                );
                send_marquee_frames(
                    &mut marquees,
                    Instant::now(),
                    &stop,
                    &sender,
                    prepare_artwork_at,
                    |report| device.write(report),
                );
                if !stop.live() {
                    return;
                }
                match device.read(&mut buffer) {
                    Ok(0) => {}
                    Ok(size) => {
                        for event in decode_report(&buffer[..size]) {
                            if sender.send(MonitorEvent::Input(event)).is_err() {
                                stop.request();
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(MonitorEvent::Connection(
                            ConnectionState::Disconnected(format!("HID read ended: {error}")),
                        ));
                        break;
                    }
                }
            }
        })();
        if !stop.live() {
            blank_device(&device);
            return;
        }
        drop(device);
        if !stop.wait(Duration::from_secs(1)) {
            return;
        }
    }
}

fn blank_device(device: &impl Device) {
    let mut sleep = [0; 32];
    sleep[0] = 0x03;
    sleep[1] = 0x08;
    match device.write_feature(&sleep) {
        Ok(32) => {}
        Ok(size) => eprintln!("HID shutdown brightness write was short: {size} of 32 bytes"),
        Err(error) => eprintln!("Could not turn off Stream Deck displays: {error}"),
    }
    // Clearing the full strip also replaces all four overlaid touch zones.
    for target in (1..=8)
        .map(ArtworkTarget::Keypad)
        .chain(std::iter::once(ArtworkTarget::TouchStripBackground))
    {
        let result = prepare_artwork(ArtworkCommand::Clear { target }).and_then(|reports| {
            for report in reports {
                let size = device.write(&report.0)?;
                if size != report.0.len() {
                    return Err(format!("short HID shutdown image write: {size} bytes"));
                }
            }
            Ok(())
        });
        if let Err(error) = result {
            eprintln!("Could not clear Stream Deck {target:?} during shutdown: {error}");
        }
    }
}

fn transmit_reports<E: std::fmt::Display>(
    reports: &[image_output::OutputReport],
    stop: &StopSignal,
    mut write: impl FnMut(&[u8]) -> Result<usize, E>,
) -> Result<(), String> {
    for (index, report) in reports.iter().enumerate() {
        // This locked check is admission; stop serializes with it. Do not hold
        // the lock during the admitted synchronous call (including fake callbacks).
        if !stop.live() {
            return Err("HID upload cancelled".into());
        }
        let written = write(&report.0)
            .map_err(|error| format!("HID write failed on report {}: {error}", index + 1))?;
        if written != report.0.len() {
            return Err(format!(
                "short HID write on report {}: wrote {written} of {} bytes",
                index + 1,
                report.0.len()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn touchscreen_indicator_commands_reach_only_the_requested_zone() {
        let (sender, commands) = std::sync::mpsc::channel();
        sender
            .send(super::ArtworkCommand::ComposeTouchscreenZone {
                zone: 4,
                path: None,
                strip_path: None,
                value: None,
                previous: false,
                next: true,
            })
            .unwrap();
        let (events, received) = std::sync::mpsc::channel();
        let mut reports = Vec::new();
        super::process_artwork(
            &commands,
            &super::StopSignal::default(),
            &events,
            super::prepare_artwork,
            |report| {
                reports.push(report.to_vec());
                Ok::<_, String>(report.len())
            },
        );
        let jpeg =
            crate::image_output::prepare_touchscreen_zone_jpeg(4, None, None, None, false, true)
                .unwrap();
        let expected = crate::image_output::encode_reports(
            crate::local_artwork::ArtworkTarget::EncoderZone(4),
            &jpeg,
        )
        .unwrap();
        assert_eq!(
            reports,
            expected
                .into_iter()
                .map(|report| report.0)
                .collect::<Vec<_>>()
        );
        let results = received
            .try_iter()
            .filter_map(|event| match event {
                super::MonitorEvent::ArtworkResult { target, result } => Some((target, result)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            results,
            [(crate::local_artwork::ArtworkTarget::EncoderZone(4), Ok(()))]
        );
    }

    use super::*;

    #[test]
    fn composed_text_is_encoded_for_the_requested_key() {
        // Given
        let text = crate::button_text::ButtonText {
            text: "Play".into(),
            ..Default::default()
        };
        // When
        let reports = prepare_artwork(ArtworkCommand::ComposeButton {
            key: 3,
            path: None,
            text,
        })
        .unwrap();
        // Then
        let mut jpeg = Vec::new();
        for report in &reports {
            assert_eq!(report.0[..3], [0x02, 0x07, 2]);
            let size = usize::from(u16::from_le_bytes([report.0[4], report.0[5]]));
            jpeg.extend_from_slice(&report.0[8..8 + size]);
        }
        let image = gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
        assert_eq!((image.width(), image.height()), (120, 120));
        let pixels = image.read_pixel_bytes();
        assert!(pixels.as_ref().iter().any(|byte| *byte > 240));
        assert!(pixels.as_ref().iter().any(|byte| *byte < 10));
    }

    #[test]
    fn stop_from_write_callback_prevents_remaining_reports() {
        let stop = StopSignal::default();
        let reports = vec![image_output::OutputReport(vec![1]); 3];
        let mut writes = 0;
        let result = transmit_reports(&reports, &stop, |bytes| {
            writes += 1;
            stop.request();
            Ok::<_, String>(bytes.len())
        });
        assert!(result.is_err());
        assert_eq!(writes, 1);
        stop.request();
        assert!(!stop.wait(Duration::from_secs(60)));
    }

    #[test]
    fn stopped_snapshot_is_discarded_without_preparing_or_writing() {
        let stop = StopSignal::default();
        stop.request();
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(ArtworkCommand::Clear {
                target: ArtworkTarget::Keypad(1),
            })
            .unwrap();
        process_artwork(
            &receiver,
            &stop,
            &std::sync::mpsc::channel().0,
            |_| panic!("must not prepare"),
            |_| panic!("must not write"),
        );
    }

    #[test]
    fn stop_during_preparation_prevents_upload_and_next_target() {
        let stop = StopSignal::default();
        let (sender, receiver) = std::sync::mpsc::channel();
        for index in 1..=2 {
            sender
                .send(ArtworkCommand::Clear {
                    target: ArtworkTarget::Keypad(index),
                })
                .unwrap();
        }
        let mut prepared = 0;
        process_artwork(
            &receiver,
            &stop,
            &std::sync::mpsc::channel().0,
            |_| {
                prepared += 1;
                stop.request();
                Ok(vec![image_output::OutputReport(vec![1])])
            },
            |_| panic!("must not write"),
        );
        assert_eq!(prepared, 1);
    }

    #[test]
    fn first_report_stop_or_consumer_loss_prevents_other_reports_and_targets() {
        for explicit_stop in [true, false] {
            let stop = StopSignal::default();
            let (sender, commands) = std::sync::mpsc::channel();
            sender
                .send(ArtworkCommand::Replace(vec![
                    ArtworkCommand::Clear {
                        target: ArtworkTarget::Keypad(1),
                    },
                    ArtworkCommand::Clear {
                        target: ArtworkTarget::Keypad(2),
                    },
                ]))
                .unwrap();
            let (events, receiver) = std::sync::mpsc::channel();
            let mut receiver = Some(receiver);
            let mut prepared = 0;
            let mut written = 0;
            process_artwork(
                &commands,
                &stop,
                &events,
                |_| {
                    prepared += 1;
                    Ok(vec![image_output::OutputReport(vec![1]); 3])
                },
                |bytes| {
                    written += 1;
                    if explicit_stop {
                        stop.request();
                    } else {
                        receiver.take();
                    }
                    Ok(bytes.len())
                },
            );
            assert_eq!(prepared, 1);
            assert_eq!(written, 1);
            assert!(!stop.live());
        }
    }

    struct FakeDevice {
        read_started: Sender<()>,
        read_release: Receiver<Result<usize, String>>,
    }

    impl Device for FakeDevice {
        fn read(&self, _: &mut [u8]) -> Result<usize, String> {
            self.read_started.send(()).unwrap();
            self.read_release.recv().unwrap()
        }
        fn write(&self, _: &[u8]) -> Result<usize, String> {
            Ok(image_output::REPORT_SIZE)
        }
        fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
            Ok(report.len())
        }
    }

    #[test]
    fn cancellation_interrupts_retry_wait_and_discovery() {
        for during_discovery in [false, true] {
            let stop = StopSignal::default();
            let worker_stop = stop.clone();
            let (events, receiver) = std::sync::mpsc::channel();
            let (_commands, commands) = std::sync::mpsc::channel();
            let (entered, entry) = std::sync::mpsc::channel();
            let (release, released) = std::sync::mpsc::channel();
            let worker = thread::spawn(move || {
                monitor::<FakeDevice>(
                    events,
                    (commands, std::sync::mpsc::channel().1),
                    worker_stop,
                    |_| {
                        entered.send(()).unwrap();
                        released.recv().unwrap();
                        Err("not found".into())
                    },
                    |_| panic!("unexpected preparation"),
                )
            });
            entry.recv().unwrap();
            if during_discovery {
                stop.request();
            }
            release.send(()).unwrap();
            if !during_discovery {
                loop {
                    if matches!(
                        receiver.recv().unwrap(),
                        MonitorEvent::Connection(ConnectionState::Unavailable(_))
                    ) {
                        break;
                    }
                }
                stop.request();
            }
            worker.join().unwrap();
        }
    }

    #[test]
    fn idle_worker_stops_and_controller_join_is_idempotent() {
        let stop = StopSignal::default();
        let worker_stop = stop.clone();
        let (events, _receiver) = std::sync::mpsc::channel();
        let (artwork, commands) = std::sync::mpsc::channel();
        let (controls, control_commands) = std::sync::mpsc::channel();
        let (entered, entry) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let mut device = Some(FakeDevice {
            read_started: entered,
            read_release: released,
        });
        let worker = thread::spawn(move || {
            monitor(
                events,
                (commands, control_commands),
                worker_stop,
                |_| Ok(device.take().unwrap()),
                |_| panic!("unexpected preparation"),
            )
        });
        let mut controller = HidController {
            stop,
            worker: Some(worker),
            artwork,
            controls,
        };
        entry.recv().unwrap();
        let controls = controller.control_sender();
        controls.send(DeviceAction::Sleep).unwrap();
        controller.request_stop();
        release.send(Ok(0)).unwrap();
        controller.shutdown();
        controller.shutdown();
        assert!(controller.worker.is_none());
        assert!(controls.send(DeviceAction::Wake).is_err());
    }

    #[test]
    fn reconnect_recovers_until_consumer_disappears() {
        let stop = StopSignal::default();
        let (events, receiver) = std::sync::mpsc::channel();
        let (_artwork, commands) = std::sync::mpsc::channel();
        let (entered, entry) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let (entered2, entry2) = std::sync::mpsc::channel();
        let (release2, released2) = std::sync::mpsc::channel();
        let mut devices = vec![
            FakeDevice {
                read_started: entered2,
                read_release: released2,
            },
            FakeDevice {
                read_started: entered,
                read_release: released,
            },
        ];
        let worker = thread::spawn(move || {
            monitor(
                events,
                (commands, std::sync::mpsc::channel().1),
                stop,
                |_| Ok(devices.pop().unwrap()),
                |_| panic!("unexpected preparation"),
            )
        });
        entry.recv().unwrap();
        release.send(Err("unplugged".into())).unwrap();
        entry2.recv().unwrap();
        drop(receiver);
        release2.send(Ok(0)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn disconnected_snapshots_keep_only_latest_complete_state() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(ArtworkCommand::Replace(vec![ArtworkCommand::Assign {
                target: ArtworkTarget::Keypad(1),
                path: "obsolete.png".into(),
            }]))
            .unwrap();
        sender
            .send(ArtworkCommand::Replace(vec![ArtworkCommand::Clear {
                target: ArtworkTarget::Keypad(1),
            }]))
            .unwrap();
        let (replaced, pending) = pending_artwork(&receiver);
        assert!(replaced, "a snapshot replaces everything shown before it");
        assert_eq!(pending.len(), 1);
        assert!(matches!(
            pending[0],
            ArtworkCommand::Clear {
                target: ArtworkTarget::Keypad(1)
            }
        ));
        let (replaced, pending) = pending_artwork(&receiver);
        assert!(!replaced && pending.is_empty());
    }

    #[test]
    fn api_messages_are_typed_and_cloneable() {
        let message = MonitorEvent::Input(Event::Key {
            index: 2,
            pressed: true,
        });
        assert_eq!(message.clone(), message);
        assert_eq!(
            MonitorEvent::Connection(ConnectionState::Searching),
            MonitorEvent::Connection(ConnectionState::Searching)
        );
    }

    #[test]
    fn transmit_writes_reports_in_order_and_rejects_short_or_failed_writes() {
        let reports = vec![
            image_output::OutputReport(vec![1, 2]),
            image_output::OutputReport(vec![3, 4]),
        ];
        let mut seen = Vec::new();
        transmit_reports(&reports, &StopSignal::default(), |bytes| {
            seen.push(bytes.to_vec());
            Ok::<_, &'static str>(bytes.len())
        })
        .unwrap();
        assert_eq!(seen, [vec![1, 2], vec![3, 4]]);

        assert!(
            transmit_reports(&reports, &StopSignal::default(), |_| Ok::<_, &'static str>(
                1
            ))
            .unwrap_err()
            .contains("short HID write")
        );
        assert!(
            transmit_reports(&reports, &StopSignal::default(), |_| Err("device gone"))
                .unwrap_err()
                .contains("device gone")
        );
    }
}
