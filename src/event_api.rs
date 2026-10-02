//! Typed in-process boundary from the HID producer to application consumers.
//! External transports can be attached behind this API in a later phase.

use crate::{
    image_output,
    local_artwork::ArtworkTarget,
    protocol::{decode_report, Event},
};
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    thread,
    time::Duration,
};

const VID: u16 = 0x0FD9;
const PID: u16 = 0x0084;

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
}

#[derive(Debug, Clone)]
pub enum ArtworkCommand {
    Assign {
        target: ArtworkTarget,
        path: PathBuf,
    },
    Clear {
        target: ArtworkTarget,
    },
}

/// Start the blocking HID producer on a dedicated thread. Consumers receive typed events.
pub fn start_hid_monitor(sender: Sender<MonitorEvent>) -> Sender<ArtworkCommand> {
    let (artwork_sender, artwork_receiver) = std::sync::mpsc::channel();
    thread::Builder::new()
        .name("streamdeck-hid".into())
        .spawn(move || monitor(sender, artwork_receiver))
        .expect("failed to start HID reader thread");
    artwork_sender
}

fn monitor(sender: Sender<MonitorEvent>, commands: Receiver<ArtworkCommand>) {
    loop {
        let _ = sender.send(MonitorEvent::Connection(ConnectionState::Searching));
        let api = match hidapi::HidApi::new() {
            Ok(api) => api,
            Err(error) => {
                let _ = sender.send(MonitorEvent::Connection(ConnectionState::Unavailable(
                    format!("HID initialization failed: {error}"),
                )));
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        let Some(info) = api
            .device_list()
            .find(|device| device.vendor_id() == VID && device.product_id() == PID)
        else {
            let _ = sender.send(MonitorEvent::Connection(ConnectionState::Unavailable(
                "Device not found · connect Stream Deck + (VID 0FD9 / PID 0084)".into(),
            )));
            thread::sleep(Duration::from_secs(2));
            continue;
        };
        let device = match info.open_device(&api) {
            Ok(device) => device,
            Err(error) => {
                let _ = sender.send(MonitorEvent::Connection(ConnectionState::Unavailable(
                    format!("Device found but could not open: {error}"),
                )));
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        if sender
            .send(MonitorEvent::Connection(ConnectionState::Connected))
            .is_err()
        {
            return;
        }
        let mut buffer = [0u8; 64];
        loop {
            for command in commands.try_iter() {
                let target = match &command {
                    ArtworkCommand::Assign { target, .. } => *target,
                    ArtworkCommand::Clear { target } => *target,
                };
                let result = (|| {
                    let jpeg = match command {
                        ArtworkCommand::Assign { target, path } => {
                            image_output::prepare_jpeg(target, path)?
                        }
                        ArtworkCommand::Clear { target } => {
                            image_output::prepare_clear_jpeg(target)?
                        }
                    };
                    let reports = image_output::encode_reports(target, &jpeg)?;
                    transmit_reports(&reports, |report| {
                        device.write(report).map_err(|error| error.to_string())
                    })
                })();
                let _ = sender.send(MonitorEvent::ArtworkResult { target, result });
            }
            match device.read_timeout(&mut buffer, 100) {
                Ok(0) => {}
                Ok(size) => {
                    for event in decode_report(&buffer[..size]) {
                        if sender.send(MonitorEvent::Input(event)).is_err() {
                            return;
                        }
                    }
                }
                Err(error) => {
                    let _ = sender.send(MonitorEvent::Connection(ConnectionState::Disconnected(
                        format!("HID read ended: {error}"),
                    )));
                    break;
                }
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn transmit_reports<E: std::fmt::Display>(
    reports: &[image_output::OutputReport],
    mut write: impl FnMut(&[u8]) -> Result<usize, E>,
) -> Result<(), String> {
    for (index, report) in reports.iter().enumerate() {
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
    use super::*;

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
        transmit_reports(&reports, |bytes| {
            seen.push(bytes.to_vec());
            Ok::<_, &'static str>(bytes.len())
        })
        .unwrap();
        assert_eq!(seen, [vec![1, 2], vec![3, 4]]);

        assert!(transmit_reports(&reports, |_| Ok::<_, &'static str>(1))
            .unwrap_err()
            .contains("short HID write"));
        assert!(transmit_reports(&reports, |_| Err("device gone"))
            .unwrap_err()
            .contains("device gone"));
    }
}
