//! The one-session adapter. Until the app keeps a session per model it serves a
//! single device: the first one that connected. Everything about other devices
//! is ignored; when the active one goes, the next connected one takes over.

use super::{ConnectionState, DeviceId, MonitorEvent};
use crate::device_model::DeviceModel;

#[derive(Default)]
pub struct ActiveDevice {
    /// Connected devices in the order they connected; the first is active.
    connected: Vec<(DeviceId, &'static DeviceModel)>,
}

impl ActiveDevice {
    /// The events the single session sees for `event`.
    pub fn accept(&mut self, event: MonitorEvent) -> Vec<MonitorEvent> {
        match event {
            MonitorEvent::Connection(ConnectionState::Connected { device, model }) => {
                // A device that reports again (replugged as another model before
                // anyone saw it go) keeps its place in line.
                let at = match self.connected.iter().position(|(id, _)| *id == device) {
                    Some(at) => {
                        self.connected[at].1 = model;
                        at
                    }
                    None => {
                        self.connected.push((device, model));
                        self.connected.len() - 1
                    }
                };
                if at == 0 {
                    vec![event]
                } else {
                    Vec::new()
                }
            }
            MonitorEvent::Connection(ConnectionState::Disconnected { device, .. }) => {
                let Some(at) = self.connected.iter().position(|(id, _)| *id == device) else {
                    return Vec::new();
                };
                self.connected.remove(at);
                if at != 0 {
                    return Vec::new();
                }
                let mut events = vec![event];
                if let Some((device, model)) = self.connected.first() {
                    events.push(MonitorEvent::Connection(ConnectionState::Connected {
                        device: *device,
                        model,
                    }));
                }
                events
            }
            MonitorEvent::Input { device, .. } | MonitorEvent::ArtworkResult { device, .. }
                if !self.is_active(device) =>
            {
                Vec::new()
            }
            MonitorEvent::DeviceResult {
                device: Some(device),
                ..
            } if !self.is_active(device) => Vec::new(),
            MonitorEvent::Connection(
                ConnectionState::Searching | ConnectionState::Unavailable(_),
            ) => {
                self.connected.clear();
                vec![event]
            }
            event => vec![event],
        }
    }

    fn is_active(&self, device: DeviceId) -> bool {
        // Before any Connected report nothing contradicts the sender.
        self.connected.first().is_none_or(|(id, _)| *id == device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        device_model::{PLUS, PLUS_XL},
        protocol::Event,
    };

    fn connected(path: &str, model: &'static DeviceModel) -> MonitorEvent {
        MonitorEvent::Connection(ConnectionState::Connected {
            device: DeviceId::from_path(path),
            model,
        })
    }

    fn disconnected(path: &str, model: &'static DeviceModel) -> MonitorEvent {
        MonitorEvent::Connection(ConnectionState::Disconnected {
            device: DeviceId::from_path(path),
            model,
            reason: "gone".into(),
        })
    }

    fn key(path: &str, model: &'static DeviceModel) -> MonitorEvent {
        MonitorEvent::Input {
            device: DeviceId::from_path(path),
            model,
            event: Event::Key {
                index: 0,
                pressed: true,
            },
        }
    }

    #[test]
    fn only_the_first_connected_device_is_seen() {
        let mut active = ActiveDevice::default();
        assert_eq!(active.accept(connected("a", &PLUS)).len(), 1);
        assert!(active.accept(connected("b", &PLUS_XL)).is_empty());
        assert_eq!(active.accept(key("a", &PLUS)).len(), 1);
        assert!(active.accept(key("b", &PLUS_XL)).is_empty());
    }

    #[test]
    fn the_next_connected_device_takes_over_when_the_active_one_goes() {
        let mut active = ActiveDevice::default();
        active.accept(connected("a", &PLUS));
        active.accept(connected("b", &PLUS_XL));
        assert!(active.accept(disconnected("b", &PLUS_XL)).is_empty());
        active.accept(connected("b", &PLUS_XL));
        let handover = active.accept(disconnected("a", &PLUS));
        assert_eq!(
            handover,
            [disconnected("a", &PLUS), connected("b", &PLUS_XL)]
        );
        assert_eq!(active.accept(key("b", &PLUS_XL)).len(), 1);
        assert!(active.accept(key("a", &PLUS)).is_empty());
    }

    #[test]
    fn global_states_pass_and_forget_every_device() {
        let mut active = ActiveDevice::default();
        active.accept(connected("a", &PLUS));
        let searching = MonitorEvent::Connection(ConnectionState::Searching);
        assert_eq!(active.accept(searching.clone()), [searching]);
        assert_eq!(active.accept(connected("b", &PLUS_XL)).len(), 1);
        assert_eq!(active.accept(MonitorEvent::Heartbeat).len(), 1);
    }

    #[test]
    fn a_device_reporting_again_keeps_its_place_and_follows_its_new_model() {
        let mut active = ActiveDevice::default();
        active.accept(connected("a", &PLUS));
        assert_eq!(active.accept(connected("a", &PLUS_XL)).len(), 1);
        active.accept(connected("b", &PLUS));
        assert!(active.accept(connected("b", &PLUS_XL)).is_empty());
        let handover = active.accept(disconnected("a", &PLUS_XL));
        assert_eq!(handover[1], connected("b", &PLUS_XL));
    }

    #[test]
    fn input_before_any_connection_is_not_contradicted() {
        let mut active = ActiveDevice::default();
        assert_eq!(active.accept(key("a", &PLUS)).len(), 1);
    }
}
