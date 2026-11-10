//! Typed actions shared by persistence, the editor and runtime adapters.

use crate::{
    desktop_actions::DesktopAction, device_control::DeviceAction, obs::ObsAction,
    organization::NavigationAction,
};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TouchscreenAction {
    NextPage,
    PreviousPage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureAction {
    Desktop(DesktopAction),
    Navigation(NavigationAction),
    Touchscreen(TouchscreenAction),
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
            Self::Timer { .. } | Self::CancelTimer | Self::Touchscreen(_) => Ok(()),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Desktop(action) => action.summary(),
            Self::Navigation(action) => action.summary(),
            Self::Touchscreen(TouchscreenAction::NextPage) => "Next touchscreen page".into(),
            Self::Touchscreen(TouchscreenAction::PreviousPage) => {
                "Previous touchscreen page".into()
            }
            Self::Device(action) => action.summary(),
            Self::Obs(action) => action.summary(),
            Self::Timer { seconds } => format!("Timer {seconds}s"),
            Self::CancelTimer => "Cancel timer".into(),
        }
    }

    pub fn encode(&self) -> io::Result<String> {
        self.validate()?;
        let value = match self {
            Self::Desktop(action) => serde_json::to_string(&Self::Desktop(action.canonical())),
            other => serde_json::to_string(other),
        }
        .map_err(io::Error::other)?;
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
        Ok(match action {
            Self::Desktop(desktop) => Self::Desktop(desktop.canonical()),
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_actions::{MediaCommand, ScrollDirection, VolumeDirection};

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
            FeatureAction::Desktop(DesktopAction::volume(VolumeDirection::Down, 10)),
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
    fn default_volume_step_encodes_as_the_legacy_shape_byte_for_byte() {
        let up = FeatureAction::Desktop(DesktopAction::volume(VolumeDirection::Up, 5));
        let down = FeatureAction::Desktop(DesktopAction::volume(VolumeDirection::Down, 5));
        assert_eq!(up.encode().unwrap(), r#"{"Desktop":{"Media":"VolumeUp"}}"#);
        assert_eq!(
            down.encode().unwrap(),
            r#"{"Desktop":{"Media":"VolumeDown"}}"#
        );
        // A hand-written default-step variant is stored in the legacy form too.
        let explicit = FeatureAction::Desktop(DesktopAction::VolumeStep {
            direction: VolumeDirection::Up,
            percent: 5,
        });
        assert_eq!(explicit.encode().unwrap(), up.encode().unwrap());
    }

    #[test]
    fn legacy_volume_shapes_decode_as_step_five() {
        assert_eq!(
            FeatureAction::decode(r#"{"Desktop":{"Media":"VolumeUp"}}"#)
                .unwrap()
                .summary(),
            "Media VolumeUp"
        );
        for (json, direction) in [
            (r#"{"Desktop":{"Media":"VolumeUp"}}"#, VolumeDirection::Up),
            (
                r#"{"Desktop":{"Media":"VolumeDown"}}"#,
                VolumeDirection::Down,
            ),
        ] {
            let FeatureAction::Desktop(action) = FeatureAction::decode(json).unwrap() else {
                panic!("desktop action expected");
            };
            assert_eq!(action.volume_step(), Some((direction, 5)));
        }
    }

    #[test]
    fn every_volume_step_roundtrips_and_non_default_uses_the_new_shape() {
        for direction in [VolumeDirection::Up, VolumeDirection::Down] {
            for percent in 1..=100u8 {
                let action = FeatureAction::Desktop(DesktopAction::volume(direction, percent));
                let encoded = action.encode().unwrap();
                assert_eq!(FeatureAction::decode(&encoded).unwrap(), action);
                assert_eq!(encoded.contains("VolumeStep"), percent != 5, "{encoded}");
            }
        }
        assert_eq!(
            FeatureAction::Desktop(DesktopAction::volume(VolumeDirection::Up, 10))
                .encode()
                .unwrap(),
            r#"{"Desktop":{"VolumeStep":{"direction":"Up","percent":10}}}"#
        );
    }

    #[test]
    fn out_of_range_volume_steps_are_rejected_on_encode_and_decode() {
        for percent in [0u8, 101, 255] {
            let action =
                FeatureAction::Desktop(DesktopAction::volume(VolumeDirection::Up, percent));
            assert!(action.encode().is_err(), "{percent}");
            let json = format!(
                r#"{{"Desktop":{{"VolumeStep":{{"direction":"Down","percent":{percent}}}}}}}"#
            );
            assert!(FeatureAction::decode(&json).is_err(), "{percent}");
        }
        assert!(FeatureAction::decode(
            r#"{"Desktop":{"VolumeStep":{"direction":"Up","percent":256}}}"#
        )
        .is_err());
        assert!(FeatureAction::decode(
            r#"{"Desktop":{"VolumeStep":{"direction":"Sideways","percent":10}}}"#
        )
        .is_err());
    }

    #[test]
    fn invalid_serialized_actions_are_rejected_at_the_boundary() {
        assert!(FeatureAction::decode(r#"{"Timer":{"seconds":0}}"#).is_err());
        assert!(FeatureAction::decode(r#"{"Device":{"Brightness":101}}"#).is_err());
        assert!(FeatureAction::decode(r#"{"Unknown":{}}"#).is_err());
        assert!(FeatureAction::decode("not json").is_err());
    }
}
