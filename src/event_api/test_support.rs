//! Test support, not part of the supported API: the real HID hub over
//! simulated devices, so the binary's tests can plug a Stream Deck + and a
//! + XL into the window without hardware, a HID stack or a display server.
//!
//! Plug devices into a [`SimBus`], start the hub with [`start_hub`] and
//! script input through [`SimDevice`]; what the hub wrote comes back through
//! [`SimDevice::images`], [`SimDevice::writes`] and [`SimDevice::features`].
//! The hub runs at test cadence (milliseconds), not the two-second search of
//! production.

pub use super::sim_device::{
    encoder_button_report, key_report, rotation_report, touch_report, Bus as SimBus,
    CapturedImage as SimImage, SimDevice,
};
use super::{hub, prepare_artwork_for, HidController, MonitorEvent};
use std::sync::mpsc::Sender;

/// Start the real hub over `bus`; events go to `events` exactly as the
/// production monitor's do. Plug and unplug on `bus` at any time.
pub fn start_hub(bus: &SimBus, events: Sender<MonitorEvent>) -> HidController {
    let listing = bus.clone();
    hub::spawn(
        events,
        move |_| listing.enumerate(),
        prepare_artwork_for,
        super::sim_device::quick(),
    )
}
