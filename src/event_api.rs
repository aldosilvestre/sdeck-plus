//! Typed in-process boundary from the HID producer to application consumers.
//! External transports can be attached behind this API in a later phase.

use crate::protocol::{decode_report, Event};
use std::{sync::mpsc::Sender, thread, time::Duration};

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
}

/// Start the blocking HID producer on a dedicated thread. Consumers receive typed events.
pub fn start_hid_monitor(sender: Sender<MonitorEvent>) {
    thread::Builder::new()
        .name("streamdeck-hid".into())
        .spawn(move || monitor(sender))
        .expect("failed to start HID reader thread");
}

fn monitor(sender: Sender<MonitorEvent>) {
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
}
