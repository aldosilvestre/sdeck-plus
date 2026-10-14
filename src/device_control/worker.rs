//! Controls use a separate FIFO but the same HID worker and cancellation gate.

use super::{Device, MonitorEvent, StopSignal};
use crate::device_control::{DesiredState, DeviceAction};
use std::sync::mpsc::{Receiver, Sender};

pub(super) struct Controls {
    commands: Receiver<DeviceAction>,
    desired: DesiredState,
}

impl Controls {
    pub(super) fn new(commands: Receiver<DeviceAction>) -> Self {
        Self {
            commands,
            desired: DesiredState::default(),
        }
    }

    pub(super) fn drain(
        &mut self,
        stop: &StopSignal,
        sender: &Sender<MonitorEvent>,
        device: Option<&dyn Device>,
    ) {
        while stop.live() {
            let action = match self.commands.try_recv() {
                Ok(action) => action,
                Err(_) => return,
            };
            let result = self
                .desired
                .apply(&action)
                .map_err(|error| error.to_string());
            let result = result.and_then(|()| match device {
                Some(device) => self.write(stop, sender, device),
                None => Err("HID device disconnected; control intent retained".into()),
            });
            self.finish(stop, sender, action, result);
        }
    }

    pub(super) fn replay(
        &self,
        stop: &StopSignal,
        sender: &Sender<MonitorEvent>,
        device: &dyn Device,
    ) {
        if let Some(action) = &self.desired.action {
            let result = self.write(stop, sender, device);
            self.finish(stop, sender, action.clone(), result);
        } else if let Err(error) = self.write(stop, sender, device) {
            eprintln!("Could not restore Stream Deck brightness: {error}");
        }
    }

    fn write(
        &self,
        stop: &StopSignal,
        sender: &Sender<MonitorEvent>,
        device: &dyn Device,
    ) -> Result<(), String> {
        if sender.send(MonitorEvent::Heartbeat).is_err() {
            stop.request();
            return Err("HID consumer disconnected".into());
        }
        // The same admission rule as artwork: an admitted call may finish,
        // but no lock is held during the HID call or a fake's stop callback.
        if !stop.live() {
            return Err("HID control cancelled".into());
        }
        let report = self.desired.report();
        let written = device
            .write_feature(&report)
            .map_err(|error| format!("HID feature write failed: {error}"))?;
        if written != report.len() {
            return Err(format!(
                "short HID feature write: wrote {written} of {} bytes",
                report.len()
            ));
        }
        Ok(())
    }

    fn finish(
        &self,
        stop: &StopSignal,
        sender: &Sender<MonitorEvent>,
        action: DeviceAction,
        result: Result<(), String>,
    ) {
        if stop.live()
            && sender
                .send(MonitorEvent::DeviceResult { action, result })
                .is_err()
        {
            stop.request();
        }
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
