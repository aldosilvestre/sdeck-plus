//! Typed in-process boundary from the HID producer to application consumers.
//! External transports can be attached behind this API in a later phase.

use crate::{
    device_control::DeviceAction, device_model::DeviceModel, image_output,
    local_artwork::ArtworkTarget, protocol::Event,
};
use std::{
    fmt,
    path::PathBuf,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const VID: u16 = 0x0FD9;

#[path = "device_control/worker.rs"]
mod device_controls;

#[cfg(test)]
#[path = "device_control/monitor_tests.rs"]
mod control_monitor_tests;

mod active;
mod device_worker;
mod hub;
mod marquee;

pub use active::ActiveDevice;

#[cfg(test)]
mod sim_device;

#[cfg(test)]
mod sim_tests;

#[cfg(test)]
mod sim_xl_probes;

#[cfg(test)]
mod sim_registry;

#[cfg(test)]
mod sim_hub;

#[cfg(test)]
mod rotation_probes;

/// Opaque identity of one attached device, derived from its HID path: the same
/// path always gives the same id, so a replug on the same port keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(u64);

impl DeviceId {
    pub fn from_path(path: &str) -> Self {
        // FNV-1a: stable across runs and releases, unlike `DefaultHasher`.
        Self(path.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        }))
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "hid-{:016x}", self.0)
    }
}

/// What enumeration knows about one supported device before it is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceKey {
    pub path: String,
    pub serial: Option<String>,
    pub model: &'static DeviceModel,
}

impl DeviceKey {
    pub fn id(&self) -> DeviceId {
        DeviceId::from_path(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// No device is open at all; the hub is looking.
    Searching,
    /// A supported device is open; everything about it follows its model.
    Connected {
        device: DeviceId,
        model: &'static DeviceModel,
    },
    /// No device is open and the hub could not use what it found.
    Unavailable(String),
    /// One device went away; the others keep running.
    Disconnected {
        device: DeviceId,
        model: &'static DeviceModel,
        reason: String,
    },
}

impl ConnectionState {
    /// The model of the device that just connected.
    pub fn connected_model(&self) -> Option<&'static DeviceModel> {
        match self {
            Self::Connected { model, .. } => Some(model),
            _ => None,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitorEvent {
    Connection(ConnectionState),
    Input {
        device: DeviceId,
        model: &'static DeviceModel,
        event: Event,
    },
    ArtworkResult {
        device: DeviceId,
        target: ArtworkTarget,
        result: Result<(), String>,
    },
    DeviceResult {
        /// `None` when the action was addressed to a model with no live device.
        device: Option<DeviceId>,
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

impl MonitorEvent {
    /// The device an event is about, when it is about one.
    pub fn device(&self) -> Option<DeviceId> {
        match self {
            Self::Connection(
                ConnectionState::Connected { device, .. }
                | ConnectionState::Disconnected { device, .. },
            )
            | Self::Input { device, .. }
            | Self::ArtworkResult { device, .. } => Some(*device),
            Self::DeviceResult { device, .. } => *device,
            _ => None,
        }
    }

    /// The decoded input, whichever device sent it.
    pub fn input(&self) -> Option<&Event> {
        match self {
            Self::Input { event, .. } => Some(event),
            _ => None,
        }
    }
}

/// Cooperative cancellation and report admission share one lock. The lock is never
/// held across HID calls: an admitted call may finish, and callbacks may request stop.
/// A child (see [`StopSignal::child`]) also stops when its parent does.
#[derive(Clone, Default)]
struct StopSignal(Arc<(Mutex<bool>, Condvar)>, Option<Arc<StopSignal>>);

impl StopSignal {
    /// A signal of its own that also reads as stopped once `self` is. Only the
    /// parent's condition variable wakes waiters, so children never `wait`.
    fn child(&self) -> Self {
        Self(Arc::default(), Some(Arc::new(self.clone())))
    }

    fn request(&self) {
        *self.0 .0.lock().unwrap() = true;
        self.0 .1.notify_all();
    }

    fn live(&self) -> bool {
        !*self.0 .0.lock().unwrap() && self.1.as_ref().is_none_or(|parent| parent.live())
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

/// The hub refused a command because it has shut down or its receiver is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubClosed;

impl fmt::Display for HubClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the HID hub is closed")
    }
}

impl std::error::Error for HubClosed {}

/// A cloneable way to address the devices by model. It outlives nothing: once
/// the controller shuts the hub down, every send fails with [`HubClosed`].
#[derive(Clone)]
pub struct HidHandle {
    /// The one-session adapter inputs: what is sent here goes to the model of
    /// the first connected device.
    artwork: Sender<ArtworkCommand>,
    controls: Sender<DeviceAction>,
    shared: Option<Arc<hub::Shared>>,
}

impl HidHandle {
    /// Send artwork to every live device of `model`. With none attached it is
    /// held for the next one (see `hub::Shared`).
    pub fn send_artwork(
        &self,
        model: &DeviceModel,
        command: ArtworkCommand,
    ) -> Result<(), HubClosed> {
        match &self.shared {
            Some(shared) => shared.send_artwork(Some(model.pid()), command),
            None => self.artwork.send(command).map_err(|_| HubClosed),
        }
    }

    /// Send a brightness or sleep action to every live device of `model`.
    pub fn send_control(&self, model: &DeviceModel, action: DeviceAction) -> Result<(), HubClosed> {
        match &self.shared {
            Some(shared) => shared.send_control(model.pid(), action),
            None => self.controls.send(action).map_err(|_| HubClosed),
        }
    }
}

/// Owns the HID hub; dropping it cancels, joins every device worker and
/// releases the devices.
///
/// Artwork and device controls are addressed to a MODEL. Every live device of
/// that model gets them, so two units of one model mirror each other.
pub struct HidController {
    stop: StopSignal,
    hub: Option<thread::JoinHandle<()>>,
    handle: HidHandle,
}

impl HidController {
    /// A sender whose commands reach the model of the first connected device.
    pub fn control_sender(&self) -> Sender<DeviceAction> {
        self.handle.controls.clone()
    }

    /// A sender whose commands reach the model of the first connected device.
    pub fn artwork_sender(&self) -> Sender<ArtworkCommand> {
        self.handle.artwork.clone()
    }

    pub fn handle(&self) -> HidHandle {
        self.handle.clone()
    }

    /// See [`HidHandle::send_artwork`].
    pub fn send_artwork(
        &self,
        model: &DeviceModel,
        command: ArtworkCommand,
    ) -> Result<(), HubClosed> {
        self.handle.send_artwork(model, command)
    }

    /// See [`HidHandle::send_control`].
    pub fn send_control(&self, model: &DeviceModel, action: DeviceAction) -> Result<(), HubClosed> {
        self.handle.send_control(model, action)
    }

    pub fn request_stop(&self) {
        self.stop.request();
    }

    /// A controller with no HID hub, for callers that inject connection
    /// events themselves. The receivers see everything sent to the device,
    /// whatever model it was addressed to.
    pub fn detached() -> (Self, Receiver<ArtworkCommand>, Receiver<DeviceAction>) {
        let (artwork, commands) = std::sync::mpsc::channel();
        let (controls, control_commands) = std::sync::mpsc::channel();
        (
            Self {
                stop: StopSignal::default(),
                hub: None,
                handle: HidHandle {
                    artwork,
                    controls,
                    shared: None,
                },
            },
            commands,
            control_commands,
        )
    }

    pub fn shutdown(&mut self) {
        self.request_stop();
        if let Some(hub) = self.hub.take() {
            let _ = hub.join();
        }
    }
}

impl Drop for HidController {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Start the HID hub on a dedicated, owned thread. It serves every supported
/// device it finds, each on a worker of its own, and follows plugs and unplugs.
pub fn start_hid_monitor(sender: Sender<MonitorEvent>) -> HidController {
    hub::spawn(
        sender,
        enumerate_devices,
        prepare_artwork_for,
        hub::Cadence::default(),
    )
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

/// One HID listing entry, before it is matched to a model.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listing {
    path: String,
    serial: Option<String>,
    vendor: u16,
    product: u16,
}

/// Every listed supported device with its model, in HID path order. Other
/// vendors, unknown products and Planned models never match.
fn supported_devices(listing: impl IntoIterator<Item = Listing>) -> Vec<DeviceKey> {
    let mut found: Vec<_> = listing
        .into_iter()
        .filter(|entry| entry.vendor == VID)
        .filter_map(|entry| {
            DeviceModel::by_pid_supported(entry.product).map(|model| DeviceKey {
                path: entry.path,
                serial: entry.serial,
                model,
            })
        })
        .collect();
    // hidapi's order is not a contract; the path is.
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// The message while no supported device is listed.
fn not_found_message() -> String {
    let wanted = DeviceModel::supported()
        .map(|model| format!("{} PID {:04X}", model.name, model.pid()))
        .collect::<Vec<_>>()
        .join(" or ");
    format!("Device not found · connect a Stream Deck + or + XL (VID 0FD9 / {wanted})")
}

fn enumerate_devices(stop: &StopSignal) -> Result<Vec<hub::Candidate<hidapi::HidDevice>>, String> {
    if !stop.live() {
        return Err("HID discovery cancelled".into());
    }
    let api =
        hidapi::HidApi::new().map_err(|error| format!("HID initialization failed: {error}"))?;
    let (listing, mut paths): (Vec<_>, std::collections::HashMap<_, _>) = api
        .device_list()
        .map(|info| {
            let path = info.path().to_string_lossy().into_owned();
            let listed = Listing {
                path: path.clone(),
                serial: info.serial_number().map(str::to_owned),
                vendor: info.vendor_id(),
                product: info.product_id(),
            };
            (listed, (path, info.path().to_owned()))
        })
        .unzip();
    let api = Arc::new(api);
    Ok(supported_devices(listing)
        .into_iter()
        .map(|key| {
            let api = api.clone();
            let path = paths.remove(&key.path);
            hub::Candidate {
                key,
                open: Box::new(move || {
                    let path = path.ok_or("Device found but could not open: path vanished")?;
                    api.open_path(&path)
                        .map_err(|error| format!("Device found but could not open: {error}"))
                }),
            }
        })
        .collect())
}

#[cfg(test)]
use crate::device_model::PLUS;

#[cfg(test)]
fn prepare_artwork(command: ArtworkCommand) -> Result<Vec<image_output::OutputReport>, String> {
    prepare_artwork_for(&PLUS, command)
}

fn prepare_artwork_for(
    model: &DeviceModel,
    command: ArtworkCommand,
) -> Result<Vec<image_output::OutputReport>, String> {
    prepare_artwork_at_for(model, command, Duration::ZERO)
}

#[cfg(test)]
fn prepare_artwork_at(
    command: ArtworkCommand,
    elapsed: Duration,
) -> Result<Vec<image_output::OutputReport>, String> {
    prepare_artwork_at_for(&PLUS, command, elapsed)
}

/// Device reports for a command, with a scrolling title `elapsed` into its marquee.
fn prepare_artwork_at_for(
    model: &DeviceModel,
    command: ArtworkCommand,
    elapsed: Duration,
) -> Result<Vec<image_output::OutputReport>, String> {
    let (target, jpeg) = match command {
        ArtworkCommand::Assign { target, path } => {
            (target, image_output::prepare_jpeg_for(model, target, path)?)
        }
        ArtworkCommand::Clear { target } => {
            (target, image_output::prepare_clear_jpeg_for(model, target)?)
        }
        ArtworkCommand::ComposeButton { key, path, text } => (
            ArtworkTarget::Keypad(key),
            image_output::prepare_button_jpeg_for(model, path.as_deref(), &text)?,
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
            image_output::prepare_touchscreen_zone_jpeg_for(
                model,
                zone,
                (path.as_deref(), strip_path.as_deref()),
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
            image_output::prepare_info_jpeg_for(
                model,
                target,
                &value,
                (path.as_deref(), strip_path.as_deref()),
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
            image_output::prepare_icon_jpeg_for(
                model,
                target,
                icon,
                &caption,
                text.as_ref(),
                strip_path.as_deref(),
                (previous, next),
            )?,
        ),
        ArtworkCommand::Replace(_) => unreachable!("snapshots contain target commands"),
    };
    image_output::encode_reports_for(model, target, &jpeg)
}

#[cfg(test)]
fn process_artwork(
    commands: &Receiver<ArtworkCommand>,
    stop: &StopSignal,
    sender: &Sender<MonitorEvent>,
    prepare: impl FnMut(ArtworkCommand) -> Result<Vec<image_output::OutputReport>, String>,
    write: impl FnMut(&[u8]) -> Result<usize, String>,
) {
    let mut marquees = marquee::Marquees::new(&PLUS);
    process_artwork_with(
        commands,
        stop,
        (sender, DeviceId::from_path("test")),
        prepare,
        write,
        &mut marquees,
    );
}

/// Send queued artwork, keeping `marquees` in step with what each target shows.
fn process_artwork_with(
    commands: &Receiver<ArtworkCommand>,
    stop: &StopSignal,
    (sender, device): (&Sender<MonitorEvent>, DeviceId),
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
            .send(MonitorEvent::ArtworkResult {
                device,
                target,
                result,
            })
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
    (sender, device): (&Sender<MonitorEvent>, DeviceId),
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
                    device,
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

fn blank_device(model: &DeviceModel, device: &impl Device) {
    let mut sleep = [0; 32];
    sleep[0] = 0x03;
    sleep[1] = 0x08;
    match device.write_feature(&sleep) {
        Ok(32) => {}
        Ok(size) => eprintln!("HID shutdown brightness write was short: {size} of 32 bytes"),
        Err(error) => eprintln!("Could not turn off Stream Deck displays: {error}"),
    }
    // Clearing the full strip also replaces all the overlaid touch zones.
    for target in model.key_numbers().map(ArtworkTarget::Keypad).chain(
        model
            .has_strip()
            .then_some(ArtworkTarget::TouchStripBackground),
    ) {
        let result =
            prepare_artwork_for(model, ArtworkCommand::Clear { target }).and_then(|reports| {
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
                super::MonitorEvent::ArtworkResult { target, result, .. } => Some((target, result)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            results,
            [(crate::local_artwork::ArtworkTarget::EncoderZone(4), Ok(()))]
        );
    }

    use super::*;

    /// A hub over one device on one path: `discover` runs each time the hub
    /// opens it, which it only does while no worker has the path.
    pub(super) fn single<D: Device + Send + 'static>(
        events: Sender<MonitorEvent>,
        discover: impl FnMut(&StopSignal) -> Result<D, String> + Send + 'static,
        prepare: impl Fn(&DeviceModel, ArtworkCommand) -> Result<Vec<image_output::OutputReport>, String>
            + Send
            + Sync
            + 'static,
    ) -> HidController {
        let discover = Arc::new(Mutex::new(discover));
        hub::spawn(
            events,
            move |stop| {
                let discover = discover.clone();
                let stop = stop.clone();
                Ok(vec![hub::Candidate {
                    key: DeviceKey {
                        path: "test".into(),
                        serial: None,
                        model: &PLUS,
                    },
                    open: Box::new(move || (discover.lock().unwrap())(&stop)),
                }])
            },
            prepare,
            hub::Cadence {
                search: Duration::from_millis(4),
                after_unplug: Duration::from_millis(2),
            },
        )
    }

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
            let (events, receiver) = std::sync::mpsc::channel();
            let (entered, entry) = std::sync::mpsc::channel();
            let (release, released) = std::sync::mpsc::channel();
            let mut hid = single::<FakeDevice>(
                events,
                move |_| {
                    entered.send(()).unwrap();
                    released.recv().unwrap();
                    Err("not found".into())
                },
                |_, _| panic!("unexpected preparation"),
            );
            entry.recv().unwrap();
            if during_discovery {
                hid.request_stop();
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
                hid.request_stop();
            }
            hid.shutdown();
        }
    }

    #[test]
    fn idle_worker_stops_and_controller_join_is_idempotent() {
        let (events, _receiver) = std::sync::mpsc::channel();
        let (entered, entry) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let mut device = Some(FakeDevice {
            read_started: entered,
            read_release: released,
        });
        let mut controller = single(
            events,
            move |_| Ok(device.take().unwrap()),
            |_, _| panic!("unexpected preparation"),
        );
        entry.recv().unwrap();
        controller.send_control(&PLUS, DeviceAction::Sleep).unwrap();
        controller.request_stop();
        release.send(Ok(0)).unwrap();
        controller.shutdown();
        controller.shutdown();
        assert!(controller.hub.is_none());
        assert_eq!(
            controller.send_control(&PLUS, DeviceAction::Wake),
            Err(HubClosed)
        );
    }

    #[test]
    fn reconnect_recovers_until_consumer_disappears() {
        let (events, receiver) = std::sync::mpsc::channel();
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
        let mut hid = single(
            events,
            move |_| Ok(devices.pop().unwrap()),
            |_, _| panic!("unexpected preparation"),
        );
        entry.recv().unwrap();
        release.send(Err("unplugged".into())).unwrap();
        entry2.recv().unwrap();
        drop(receiver);
        release2.send(Ok(0)).unwrap();
        // The hub ends by itself once nobody listens; nothing asks it to stop.
        hid.hub.take().unwrap().join().unwrap();
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
        let message = MonitorEvent::Input {
            device: DeviceId::from_path("test"),
            model: &PLUS,
            event: Event::Key {
                index: 2,
                pressed: true,
            },
        };
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

    #[derive(Default)]
    struct Recorder(std::cell::RefCell<Vec<Vec<u8>>>);

    impl Device for Recorder {
        fn read(&self, _: &mut [u8]) -> Result<usize, String> {
            Ok(0)
        }
        fn write(&self, report: &[u8]) -> Result<usize, String> {
            self.0.borrow_mut().push(report.to_vec());
            Ok(report.len())
        }
        fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
            Ok(report.len())
        }
    }

    fn cleared_commands(model: &DeviceModel) -> (usize, Vec<u8>) {
        let device = Recorder::default();
        blank_device(model, &device);
        let writes = device.0.borrow();
        let keys = writes
            .iter()
            .filter(|report| report[1] == 0x07 && report[3] == 1)
            .map(|report| report[2])
            .collect();
        (
            writes.iter().filter(|report| report[1] == 0x0b).count(),
            keys,
        )
    }

    #[test]
    fn shutdown_clears_every_key_and_the_strip_of_the_model() {
        let (strips, keys) = cleared_commands(&PLUS);
        assert_eq!(keys, (0..8).collect::<Vec<u8>>());
        assert!(strips >= 1);
        let (strips, keys) = cleared_commands(&crate::device_model::PLUS_XL);
        assert_eq!(keys, (0..36).collect::<Vec<u8>>());
        assert!(strips >= 1);
    }
}
