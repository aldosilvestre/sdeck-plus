//! Typed actions shared by persistence, the editor and runtime adapters.

use crate::{
    desktop_actions::DesktopAction, device_control::DeviceAction, obs::ObsAction,
    organization::NavigationAction,
};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureAction {
    Desktop(DesktopAction),
    Navigation(NavigationAction),
    Device(DeviceAction),
    Obs(ObsAction),
    Timer { seconds: u32 },
    CancelTimer,
}

impl FeatureAction {
    pub fn validate(&self) -> io::Result<()> {
        match self {
            Self::Desktop(action) => action.validate(),
            Self::Navigation(action) => action.validate(),
            Self::Device(action) => action.validate(),
            Self::Obs(action) => action.validate(),
            Self::Timer { seconds } if !(1..=86_400).contains(seconds) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Timer duration must be between 1 second and 24 hours",
            )),
            Self::Timer { .. } | Self::CancelTimer => Ok(()),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Desktop(action) => action.summary(),
            Self::Navigation(action) => action.summary(),
            Self::Device(action) => action.summary(),
            Self::Obs(action) => action.summary(),
            Self::Timer { seconds } => format!("Timer {seconds}s"),
            Self::CancelTimer => "Cancel timer".into(),
        }
    }

    pub fn encode(&self) -> io::Result<String> {
        self.validate()?;
        let value = serde_json::to_string(self).map_err(io::Error::other)?;
        if value.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Action settings exceed the 4096-byte storage limit",
            ));
        }
        Ok(value)
    }

    pub fn decode(value: &str) -> io::Result<Self> {
        let action: Self = serde_json::from_str(value)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        action.validate()?;
        Ok(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_actions::{MediaCommand, ScrollDirection};

    #[test]
    fn typed_actions_roundtrip_without_losing_user_fields() {
        let actions = [
            FeatureAction::Desktop(DesktopAction::Text {
                value: "  Unicode\n\u{65e5}\u{672c}\u{8a9e}  ".into(),
                enter: true,
            }),
            FeatureAction::Desktop(DesktopAction::ShortcutToggle {
                first: "CTRL + A".into(),
                second: "SUPER + D".into(),
            }),
            FeatureAction::Desktop(DesktopAction::Open {
                target: "/home/user/a file.txt".into(),
            }),
            FeatureAction::Desktop(DesktopAction::Media(MediaCommand::VolumeUp)),
            FeatureAction::Desktop(DesktopAction::CycleAudioOutput),
            FeatureAction::Desktop(DesktopAction::MouseScroll {
                direction: ScrollDirection::Down,
                ticks: 2,
            }),
            FeatureAction::Navigation(NavigationAction::GoToPage(2)),
            FeatureAction::Device(DeviceAction::Brightness(50)),
            FeatureAction::Device(DeviceAction::AdjustBrightness(-5)),
            FeatureAction::Device(DeviceAction::Sleep),
            FeatureAction::Obs(ObsAction::SourceVisibility {
                scene: "Main scene".into(),
                source: "Camera".into(),
                visible: false,
            }),
            FeatureAction::Timer { seconds: 3 },
            FeatureAction::CancelTimer,
        ];
        for action in actions {
            let encoded = action.encode().unwrap();
            let decoded = FeatureAction::decode(&encoded).unwrap();
            assert_eq!(decoded, action);
        }
    }

    #[test]
    fn invalid_serialized_actions_are_rejected_at_the_boundary() {
        assert!(FeatureAction::decode(r#"{"Timer":{"seconds":0}}"#).is_err());
        assert!(FeatureAction::decode(r#"{"Device":{"Brightness":101}}"#).is_err());
        assert!(FeatureAction::decode(r#"{"Unknown":{}}"#).is_err());
        assert!(FeatureAction::decode("not json").is_err());
    }
}
