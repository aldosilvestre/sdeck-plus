//! Device intent, independent of artwork snapshots and HID ownership.

use serde::{Deserialize, Serialize};
use std::io;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum DeviceAction {
    /// Set the remembered percentage; while asleep the display stays dark.
    Brightness(u8),
    /// Adjust the remembered percentage, clamped to 0..=100.
    AdjustBrightness(i8),
    /// Turn brightness off without discarding the remembered percentage.
    Sleep,
    /// Restore the remembered percentage (initially 100).
    Wake,
}

impl DeviceAction {
    /// Reject absolute brightness outside the device's percentage range.
    pub fn validate(&self) -> io::Result<()> {
        match self {
            Self::Brightness(value) if *value > 100 => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "brightness must be between 0 and 100",
            )),
            Self::Brightness(_) | Self::AdjustBrightness(_) | Self::Sleep | Self::Wake => Ok(()),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Brightness(value) => format!("Brightness {value}%"),
            Self::AdjustBrightness(step) => format!("Adjust brightness {step:+}%"),
            Self::Sleep => "Sleep".into(),
            Self::Wake => "Wake".into(),
        }
    }
}

/// Remember intent even after a failed write. No intent means no startup report.
pub(crate) struct DesiredState {
    brightness: u8,
    sleeping: bool,
    pub(crate) action: Option<DeviceAction>,
}

impl Default for DesiredState {
    fn default() -> Self {
        Self {
            // There is no brightness readback in this protocol. This baseline is
            // only used once a user requests relative adjustment or wake.
            brightness: 100,
            sleeping: false,
            action: None,
        }
    }
}

impl DesiredState {
    pub(crate) fn apply(&mut self, action: &DeviceAction) -> io::Result<()> {
        action.validate()?;
        match action {
            DeviceAction::Brightness(value) => self.brightness = *value,
            DeviceAction::AdjustBrightness(step) => {
                self.brightness = self.brightness.saturating_add_signed(*step).min(100);
            }
            DeviceAction::Sleep => self.sleeping = true,
            DeviceAction::Wake => self.sleeping = false,
        }
        self.action = Some(action.clone());
        Ok(())
    }

    pub(crate) fn report(&self) -> [u8; 32] {
        // StreamDeckPlus.set_brightness in python-elgato-streamdeck:
        // https://github.com/abcminiuser/python-elgato-streamdeck/blob/master/src/StreamDeck/Devices/StreamDeckPlus.py
        let mut report = [0; 32];
        report[0] = 0x03;
        report[1] = 0x08;
        report[2] = if self.sleeping { 0 } else { self.brightness };
        report
    }
}

#[cfg(test)]
#[path = "device_control/tests.rs"]
mod tests;
