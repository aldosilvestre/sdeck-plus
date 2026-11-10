//! A simulated Stream Deck + / + XL for tests. It scripts 0x01 input reports,
//! captures every 0x02 output report and feature report, reassembles the JPEG
//! chunks of each image, and can be unplugged so a different model is found
//! next. No hardware, HID stack or display server is involved.

use super::*;
use gtk::gdk_pixbuf::Pixbuf;
use std::{collections::VecDeque, thread::JoinHandle};

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
            Some(Scripted::Unplug) => Err("unplugged".into()),
            Some(Scripted::Report(report)) => {
                let size = report.len().min(buffer.len());
                buffer[..size].copy_from_slice(&report[..size]);
                Ok(size)
            }
        }
    }

    fn write(&self, report: &[u8]) -> Result<usize, String> {
        let (state, wake) = &*self.state;
        state.lock().unwrap().writes.push(report.to_vec());
        wake.notify_all();
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

/// A monitor wired to scripted devices, plugged in one after another.
pub(super) struct Rig {
    events: Receiver<MonitorEvent>,
    pub(super) artwork: Sender<ArtworkCommand>,
    pub(super) controls: Sender<DeviceAction>,
    /// Everything received so far, heartbeats excluded.
    pub(super) seen: Vec<MonitorEvent>,
    stop: StopSignal,
    worker: Option<JoinHandle<()>>,
}

impl Rig {
    pub(super) fn start(devices: &[&SimDevice]) -> Self {
        let mut plugged: VecDeque<SimDevice> = devices.iter().map(|d| (*d).clone()).collect();
        let (sender, events) = std::sync::mpsc::channel();
        let (artwork, commands) = std::sync::mpsc::channel();
        let (controls, control_commands) = std::sync::mpsc::channel();
        let stop = StopSignal::default();
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            monitor_for(
                sender,
                (commands, control_commands),
                worker_stop,
                move |_| {
                    plugged
                        .pop_front()
                        .map(|device| (device.clone(), device.model))
                        .ok_or_else(|| "Device not found".to_string())
                },
                prepare_artwork_for,
            )
        });
        Self {
            events,
            artwork,
            controls,
            seen: Vec::new(),
            stop,
            worker: Some(worker),
        }
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

    /// The next `count` decoded input events.
    pub(super) fn inputs(&mut self, count: usize) -> Vec<Event> {
        let mut found = Vec::new();
        self.wait_event(|event| {
            if let MonitorEvent::Input(input) = event {
                found.push(input.clone());
            }
            (found.len() == count).then_some(())
        });
        found
    }

    /// Stop the monitor; it blanks the connected device before returning.
    pub(super) fn stop(&mut self) {
        self.stop.request();
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop();
    }
}
