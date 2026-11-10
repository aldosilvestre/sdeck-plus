//! A simulated Stream Deck + / + XL for tests. It scripts 0x01 input reports,
//! captures every 0x02 output report and feature report, reassembles the JPEG
//! chunks of each image, and can be unplugged so a different model is found
//! next. A [`Bus`] lists the devices that are plugged in right now, and a
//! [`Rig`] runs the real hub over it, so several devices can come and go at
//! runtime. No hardware, HID stack or display server is involved.

use super::*;
use gtk::gdk_pixbuf::Pixbuf;
use std::collections::VecDeque;

const WAIT: Duration = Duration::from_secs(10);

enum Scripted {
    Report(Vec<u8>),
    Unplug,
}

#[derive(Default)]
struct State {
    inputs: VecDeque<Scripted>,
    writes: Vec<Vec<u8>>,
    features: Vec<Vec<u8>>,
    /// The cable was pulled: a read has already failed.
    gone: bool,
    /// Times the hub opened this unit.
    opened: usize,
    /// The next write blocks this long, as a stalled USB transfer does.
    stall: Option<Duration>,
}

/// One simulated unit. Clones share the same wire, so the test keeps a handle
/// while the monitor owns another.
#[derive(Clone)]
pub(super) struct SimDevice {
    pub(super) model: &'static DeviceModel,
    state: Arc<(Mutex<State>, Condvar)>,
}

impl Device for SimDevice {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, String> {
        let (state, wake) = &*self.state;
        let mut state = state.lock().unwrap();
        if state.inputs.is_empty() {
            state = wake
                .wait_timeout(state, Duration::from_millis(20))
                .unwrap()
                .0;
        }
        match state.inputs.pop_front() {
            None => Ok(0),
            Some(Scripted::Unplug) => {
                state.gone = true;
                Err("unplugged".into())
            }
            Some(Scripted::Report(report)) => {
                let size = report.len().min(buffer.len());
                buffer[..size].copy_from_slice(&report[..size]);
                Ok(size)
            }
        }
    }

    fn write(&self, report: &[u8]) -> Result<usize, String> {
        let (state, wake) = &*self.state;
        let stall = {
            let mut state = state.lock().unwrap();
            state.writes.push(report.to_vec());
            state.stall.take()
        };
        wake.notify_all();
        if let Some(stall) = stall {
            thread::sleep(stall);
        }
        Ok(report.len())
    }

    fn write_feature(&self, report: &[u8]) -> Result<usize, String> {
        let (state, wake) = &*self.state;
        state.lock().unwrap().features.push(report.to_vec());
        wake.notify_all();
        Ok(report.len())
    }
}

/// An image the host uploaded, reassembled from its chunk reports.
pub(super) struct CapturedImage {
    /// 0x07 key image, 0x0b full window, 0x0c partial window.
    pub(super) command: u8,
    /// Zero-based key for 0x07, otherwise 0.
    pub(super) slot: u8,
    /// `(x, y, width, height)` of a 0x0c partial window.
    pub(super) window: Option<(u16, u16, u16, u16)>,
    pub(super) chunks: usize,
    pub(super) jpeg: Vec<u8>,
}

impl CapturedImage {
    pub(super) fn decode(&self) -> Pixbuf {
        Pixbuf::from_read(std::io::Cursor::new(self.jpeg.clone())).unwrap()
    }
}

impl SimDevice {
    pub(super) fn new(model: &'static DeviceModel) -> Self {
        Self {
            model,
            state: Arc::default(),
        }
    }

    /// Queue one raw input report, as the device would send it.
    pub(super) fn send(&self, report: Vec<u8>) {
        self.script(Scripted::Report(report));
    }

    /// The next read fails, as it does when the cable is pulled.
    pub(super) fn unplug(&self) {
        self.script(Scripted::Unplug);
    }

    fn script(&self, item: Scripted) {
        let (state, wake) = &*self.state;
        state.lock().unwrap().inputs.push_back(item);
        wake.notify_all();
    }

    /// Make the next write block for `time`.
    pub(super) fn stall_next_write(&self, time: Duration) {
        self.state.0.lock().unwrap().stall = Some(time);
    }

    /// How many times the hub opened this unit.
    pub(super) fn opened(&self) -> usize {
        self.state.0.lock().unwrap().opened
    }

    fn mark_opened(&self) {
        self.state.0.lock().unwrap().opened += 1;
    }

    /// Handles to this unit alive now: the test's own, plus the bus's and the
    /// worker's while they hold it.
    pub(super) fn handles(&self) -> usize {
        Arc::strong_count(&self.state)
    }

    /// The newest finished key image of zero-based `slot`.
    pub(super) fn last_key_jpeg(&self, slot: u8) -> Option<Vec<u8>> {
        self.assemble()
            .0
            .into_iter()
            .rev()
            .find(|image| image.command == 0x07 && image.slot == slot)
            .map(|image| image.jpeg)
    }

    /// Whether a read already failed because of an unplug.
    fn is_gone(&self) -> bool {
        self.state.0.lock().unwrap().gone
    }

    /// Every 0x02 output report written so far.
    pub(super) fn writes(&self) -> Vec<Vec<u8>> {
        self.state.0.lock().unwrap().writes.clone()
    }

    /// Every feature report written so far.
    pub(super) fn features(&self) -> Vec<Vec<u8>> {
        self.state.0.lock().unwrap().features.clone()
    }

    /// Wait until `done` holds for the captured traffic, within a bounded time.
    pub(super) fn wait_until(&self, mut done: impl FnMut(&SimDevice) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !done(self) {
            assert!(Instant::now() < deadline, "simulated device wait timed out");
            let (state, wake) = &*self.state;
            let _ = wake
                .wait_timeout(state.lock().unwrap(), Duration::from_millis(20))
                .unwrap();
        }
    }

    /// Finished images and the number of chunk sequences still open.
    pub(super) fn assemble(&self) -> (Vec<CapturedImage>, usize) {
        let mut open: Vec<CapturedImage> = Vec::new();
        let mut done = Vec::new();
        for report in self.writes() {
            assert_eq!(report.len(), self.model.image_report_len, "report size");
            assert_eq!(report[0], 0x02, "output report id");
            let command = report[1];
            let (slot, window, last, index, length, header) = match command {
                0x07 | 0x0b => (
                    report[2],
                    None,
                    report[3] == 1,
                    u16::from_le_bytes([report[6], report[7]]),
                    usize::from(u16::from_le_bytes([report[4], report[5]])),
                    self.model.key_header_len,
                ),
                0x0c => {
                    let word = |at: usize| u16::from_le_bytes([report[at], report[at + 1]]);
                    (
                        0,
                        Some((word(2), word(4), word(6), word(8))),
                        report[10] == 1,
                        word(11),
                        usize::from(word(13)),
                        self.model.zone_header_len,
                    )
                }
                other => panic!("unexpected output command {other:#04x}"),
            };
            let at = open.iter().position(|image| {
                image.command == command && image.slot == slot && image.window == window
            });
            let at = at.unwrap_or_else(|| {
                open.push(CapturedImage {
                    command,
                    slot,
                    window,
                    chunks: 0,
                    jpeg: Vec::new(),
                });
                open.len() - 1
            });
            let image = &mut open[at];
            assert_eq!(
                usize::from(index),
                image.chunks,
                "chunk index is sequential"
            );
            assert!(
                length <= report.len() - header,
                "chunk length fits the report"
            );
            assert!(
                report[header + length..].iter().all(|byte| *byte == 0),
                "chunk padding is zero"
            );
            image
                .jpeg
                .extend_from_slice(&report[header..header + length]);
            image.chunks += 1;
            if last {
                done.push(open.remove(at));
            }
        }
        (done, open.len())
    }

    /// Finished images; panics if an upload was left half-sent.
    pub(super) fn images(&self) -> Vec<CapturedImage> {
        let (images, open) = self.assemble();
        assert_eq!(open, 0, "an image upload was left incomplete");
        images
    }

    pub(super) fn image_count(&self) -> usize {
        self.assemble().0.len()
    }
}

/// An input report: id 1, command, little-endian payload length, payload.
fn input_report(model: &DeviceModel, command: u8, payload: &[u8]) -> Vec<u8> {
    let mut report = vec![1, command];
    report.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    report.extend_from_slice(payload);
    report.resize(report.len().max(model.input_report_len), 0);
    report
}

/// Key states, one byte per key, for the zero-based keys in `down`.
pub(super) fn key_report(model: &DeviceModel, down: &[usize]) -> Vec<u8> {
    // Sensors a model appends after its keys are the last entries.
    let mut states = vec![0; usize::from(model.key_inputs())];
    for key in down {
        states[*key] = 1;
    }
    input_report(model, 0x00, &states)
}

/// Encoder buttons (type 0) for the zero-based dials in `down`.
pub(super) fn encoder_button_report(model: &DeviceModel, down: &[usize]) -> Vec<u8> {
    let mut payload = vec![0; usize::from(model.dials) + 1];
    for dial in down {
        payload[1 + dial] = 1;
    }
    input_report(model, 0x03, &payload)
}

/// Encoder rotation (type 1) as `(zero-based dial, signed ticks)`.
pub(super) fn rotation_report(model: &DeviceModel, turns: &[(usize, i8)]) -> Vec<u8> {
    let mut payload = vec![0; usize::from(model.dials) + 1];
    payload[0] = 1;
    for (dial, ticks) in turns {
        payload[1 + dial] = *ticks as u8;
    }
    input_report(model, 0x03, &payload)
}

/// A touch report: `kind` 1 tap, 2 press, 3 flick; `points` are x, y (and the
/// end point of a flick) as little-endian words. `length` is the payload size.
pub(super) fn touch_report(
    model: &DeviceModel,
    kind: u8,
    length: usize,
    points: &[u16],
) -> Vec<u8> {
    let mut payload = vec![kind, 0];
    for point in points {
        payload.extend_from_slice(&point.to_le_bytes());
    }
    payload.resize(length, 0);
    input_report(model, 0x02, &payload)
}

struct Plugged {
    path: String,
    serial: Option<String>,
    vendor: u16,
    product: u16,
    /// Listed as the device's own model, whatever the registry says of its
    /// product id; otherwise it is matched by id like a real listing.
    as_model: bool,
    device: SimDevice,
}

#[derive(Default)]
struct BusState {
    plugged: Vec<Plugged>,
    /// Devices that take the place of the one on `SEQUENCE_PATH` once it is gone.
    sequence: VecDeque<SimDevice>,
    opens: std::collections::HashMap<String, usize>,
    enumerations: usize,
    failing: std::collections::HashSet<String>,
    /// Hand the hub its candidates in the opposite order.
    reversed: bool,
}

/// The path of the one-after-another devices of [`Rig::start`].
pub(super) const SEQUENCE_PATH: &str = "sim";

/// What the USB bus lists right now. Clones share it.
#[derive(Clone, Default)]
pub(super) struct Bus(Arc<Mutex<BusState>>);

impl Bus {
    /// Plug a device in on `path`; it is listed from the next enumeration.
    pub(super) fn plug(&self, path: &str, device: &SimDevice) {
        self.add(path, (VID, device.model.pid()), device, true);
    }

    /// Plug in whatever has these USB ids, supported or not.
    pub(super) fn plug_ids(&self, path: &str, ids: (u16, u16), device: &SimDevice) {
        self.add(path, ids, device, false);
    }

    fn add(&self, path: &str, (vendor, product): (u16, u16), device: &SimDevice, as_model: bool) {
        self.0.lock().unwrap().plugged.push(Plugged {
            path: path.to_owned(),
            serial: Some(format!("SERIAL-{path}")),
            vendor,
            product,
            as_model,
            device: device.clone(),
        });
    }

    /// Pull the cable: the device stops being listed and its next read fails.
    pub(super) fn unplug(&self, path: &str) {
        let mut state = self.0.lock().unwrap();
        state.plugged.retain(|plugged| {
            let gone = plugged.path == path;
            if gone {
                plugged.device.unplug();
            }
            !gone
        });
    }

    /// Make opening `path` fail, as a device another program holds does.
    pub(super) fn refuse_open(&self, path: &str) {
        self.0.lock().unwrap().failing.insert(path.to_owned());
    }

    pub(super) fn reverse_order(&self) {
        self.0.lock().unwrap().reversed = true;
    }

    /// How many times `path` was opened.
    pub(super) fn opens(&self, path: &str) -> usize {
        self.0.lock().unwrap().opens.get(path).copied().unwrap_or(0)
    }

    /// How many times the hub has listed the bus.
    pub(super) fn enumerations(&self) -> usize {
        self.0.lock().unwrap().enumerations
    }

    fn enumerate(&self) -> Result<Vec<hub::Candidate<SimDevice>>, String> {
        let mut state = self.0.lock().unwrap();
        state.enumerations += 1;
        state.plugged.retain(|plugged| !plugged.device.is_gone());
        let sequenced = state
            .plugged
            .iter()
            .any(|plugged| plugged.path == SEQUENCE_PATH);
        if !sequenced {
            if let Some(next) = state.sequence.pop_front() {
                state.plugged.push(Plugged {
                    path: SEQUENCE_PATH.into(),
                    serial: None,
                    vendor: VID,
                    product: next.model.pid(),
                    as_model: true,
                    device: next,
                });
            }
        }
        let mut keys: Vec<DeviceKey> = state
            .plugged
            .iter()
            .filter(|plugged| plugged.as_model)
            .map(|plugged| DeviceKey {
                path: plugged.path.clone(),
                serial: plugged.serial.clone(),
                model: plugged.device.model,
            })
            .collect();
        let mut listing: Vec<_> = state
            .plugged
            .iter()
            .filter(|plugged| !plugged.as_model)
            .map(|plugged| Listing {
                path: plugged.path.clone(),
                serial: plugged.serial.clone(),
                vendor: plugged.vendor,
                product: plugged.product,
            })
            .collect();
        // The OS order is not a contract.
        listing.reverse();
        keys.extend(supported_devices(listing));
        if state.reversed {
            keys.reverse();
        }
        Ok(keys
            .into_iter()
            .map(|key| {
                let device = state
                    .plugged
                    .iter()
                    .find(|plugged| plugged.path == key.path)
                    .map(|plugged| plugged.device.clone())
                    .unwrap();
                let bus = self.clone();
                let path = key.path.clone();
                hub::Candidate {
                    key,
                    open: Box::new(move || {
                        let mut state = bus.0.lock().unwrap();
                        *state.opens.entry(path.clone()).or_default() += 1;
                        if state.failing.contains(&path) {
                            return Err("Device found but could not open: busy".into());
                        }
                        device.mark_opened();
                        Ok(device)
                    }),
                }
            })
            .collect())
    }
}

/// Faster than production, so a test does not wait seconds to see a replug.
pub(super) fn quick() -> hub::Cadence {
    hub::Cadence {
        search: Duration::from_millis(4),
        after_unplug: Duration::from_millis(2),
    }
}

/// The real hub over a [`Bus`].
pub(super) struct Rig {
    events: Receiver<MonitorEvent>,
    /// The one-session adapter's inputs: they reach the first device's model.
    pub(super) artwork: Sender<ArtworkCommand>,
    pub(super) controls: Sender<DeviceAction>,
    /// Everything received so far, heartbeats excluded.
    pub(super) seen: Vec<MonitorEvent>,
    pub(super) bus: Bus,
    hid: HidController,
}

impl Rig {
    /// Devices plugged in one after another: each appears once the one before
    /// it has been unplugged, all on the same path.
    pub(super) fn start(devices: &[&SimDevice]) -> Self {
        let bus = Bus::default();
        bus.0.lock().unwrap().sequence = devices.iter().map(|d| (*d).clone()).collect();
        // Slow enough that a test reacts to an unplug before the next device
        // shows up.
        Self::over(
            bus,
            hub::Cadence {
                search: Duration::from_millis(60),
                after_unplug: Duration::from_millis(60),
            },
        )
    }

    /// Nothing plugged in; use [`Rig::bus`] to plug devices at runtime.
    pub(super) fn empty() -> Self {
        Self::over(Bus::default(), quick())
    }

    pub(super) fn over(bus: Bus, cadence: hub::Cadence) -> Self {
        let (sender, events) = std::sync::mpsc::channel();
        let listing = bus.clone();
        let hid = hub::spawn(
            sender,
            move |_| listing.enumerate(),
            prepare_artwork_for,
            cadence,
        );
        Self {
            events,
            artwork: hid.artwork_sender(),
            controls: hid.control_sender(),
            seen: Vec::new(),
            bus,
            hid,
        }
    }

    /// What a send says once the hub has been stopped.
    pub(super) fn closed_send(&self) -> Result<(), HubClosed> {
        self.hid.send_control(&PLUS, DeviceAction::Wake)
    }

    pub(super) fn handle(&self) -> HidHandle {
        self.hid.handle()
    }

    /// Artwork for every live device of `model`.
    pub(super) fn send_artwork(&self, model: &DeviceModel, command: ArtworkCommand) {
        self.hid.send_artwork(model, command).unwrap();
    }

    pub(super) fn send_control(&self, model: &DeviceModel, action: DeviceAction) {
        self.hid.send_control(model, action).unwrap();
    }

    /// Consume events until `pick` accepts one; bounded in time.
    pub(super) fn wait_event<T>(&mut self, mut pick: impl FnMut(&MonitorEvent) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self.events.recv_timeout(left).expect("monitor event");
            if event == MonitorEvent::Heartbeat {
                continue;
            }
            self.seen.push(event.clone());
            if let Some(found) = pick(&event) {
                return found;
            }
        }
    }

    pub(super) fn wait_connection(&mut self, wanted: ConnectionState) {
        self.wait_event(|event| (*event == MonitorEvent::Connection(wanted.clone())).then_some(()));
    }

    /// The next `count` decoded input events, from any device.
    pub(super) fn inputs(&mut self, count: usize) -> Vec<Event> {
        self.inputs_tagged(count)
            .into_iter()
            .map(|(_, _, event)| event)
            .collect()
    }

    /// The next `count` input events with the device and model they came from.
    pub(super) fn inputs_tagged(
        &mut self,
        count: usize,
    ) -> Vec<(DeviceId, &'static DeviceModel, Event)> {
        let mut found = Vec::new();
        self.wait_event(|event| {
            if let MonitorEvent::Input {
                device,
                model,
                event,
            } = event
            {
                found.push((*device, *model, event.clone()));
            }
            (found.len() == count).then_some(())
        });
        found
    }

    /// Stop the hub; each worker blanks its own device before it returns.
    pub(super) fn stop(&mut self) {
        self.hid.shutdown();
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop();
    }
}
