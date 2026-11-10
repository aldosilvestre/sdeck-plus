//! One worker per attached device: its own stop signal, marquees, controls,
//! artwork queue and a 100 ms read, on a thread the hub owns and joins.

use super::{
    blank_device, device_controls::Controls, hub::Shared, marquee, process_artwork_with,
    send_marquee_frames, ArtworkCommand, ConnectionState, Device, DeviceId, MonitorEvent,
    StopSignal,
};
use crate::{device_model::DeviceModel, image_output, protocol::decode_report_for};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
        Arc,
    },
    time::Instant,
};

pub(super) type Prepare = Arc<
    dyn Fn(&DeviceModel, ArtworkCommand) -> Result<Vec<image_output::OutputReport>, String>
        + Send
        + Sync,
>;

pub(super) struct DeviceWorker<D> {
    pub(super) id: DeviceId,
    pub(super) path: String,
    pub(super) model: &'static DeviceModel,
    pub(super) device: D,
    /// A child of the hub's signal: the hub stopping stops every worker.
    pub(super) stop: StopSignal,
    pub(super) hub_stop: StopSignal,
    pub(super) sender: Sender<MonitorEvent>,
    pub(super) artwork: Receiver<ArtworkCommand>,
    pub(super) controls: Controls,
    pub(super) prepare: Prepare,
    pub(super) shared: Arc<Shared>,
    pub(super) ended: Arc<AtomicBool>,
}

impl<D: Device> DeviceWorker<D> {
    /// Serve the device until it goes or stop is requested. Only a requested
    /// stop blanks the device, and only this one; an unplugged one is left alone.
    pub(super) fn run(mut self) {
        self.serve();
        let unplugged = self.stop.live();
        if !unplugged {
            blank_device(self.model, &self.device);
        }
        // Commands sent from here on are held for the next device of the model.
        self.shared.detach(self.id, &self.artwork);
        if unplugged {
            self.controls.drain(&self.stop, &self.sender, None);
        }
        let Self {
            device,
            path,
            sender,
            hub_stop,
            shared,
            ended,
            ..
        } = self;
        // The OS handle goes before the path is released for reopening.
        drop(device);
        shared.release(&path);
        // A vanished consumer ends the whole hub, not just this device.
        if sender.send(MonitorEvent::Heartbeat).is_err() {
            hub_stop.request();
        }
        ended.store(true, Ordering::Release);
    }

    fn serve(&mut self) {
        let model = self.model;
        let mut buffer = vec![0u8; model.input_report_len];
        // Scrolling titles belong to this connection; a reconnect redraws.
        let mut marquees = marquee::Marquees::new(model);
        self.controls.replay(&self.stop, &self.sender, &self.device);
        while self.stop.live() {
            // A heartbeat detects consumer loss even when there is no input.
            if self.sender.send(MonitorEvent::Heartbeat).is_err() {
                self.stop.request();
                return;
            }
            self.controls
                .drain(&self.stop, &self.sender, Some(&self.device));
            process_artwork_with(
                &self.artwork,
                &self.stop,
                (&self.sender, self.id),
                |command| (self.prepare)(model, command),
                |report| self.device.write(report),
                &mut marquees,
            );
            send_marquee_frames(
                &mut marquees,
                Instant::now(),
                &self.stop,
                (&self.sender, self.id),
                |command, elapsed| super::prepare_artwork_at_for(model, command, elapsed),
                |report| self.device.write(report),
            );
            if !self.stop.live() {
                return;
            }
            match self.device.read(&mut buffer) {
                Ok(0) => {}
                Ok(size) => {
                    for event in decode_report_for(model, &buffer[..size]) {
                        let input = MonitorEvent::Input {
                            device: self.id,
                            model,
                            event,
                        };
                        if self.sender.send(input).is_err() {
                            self.stop.request();
                            return;
                        }
                    }
                }
                Err(error) => {
                    let _ =
                        self.sender
                            .send(MonitorEvent::Connection(ConnectionState::Disconnected {
                                device: self.id,
                                model,
                                reason: format!("HID read ended: {error}"),
                            }));
                    return;
                }
            }
        }
    }
}
