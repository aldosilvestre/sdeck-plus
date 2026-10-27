use super::*;
use crate::desktop_actions::{DesktopAction, MediaCommand, ScrollDirection};
use crate::device_control::DeviceAction;
use crate::features::{FeatureAction, TouchscreenAction};
use crate::obs::ObsAction;
use crate::organization::NavigationAction;
use std::path::PathBuf;

fn feature(action: FeatureAction) -> Action {
    Action::Feature(action)
}

fn desktop(action: DesktopAction) -> Action {
    feature(FeatureAction::Desktop(action))
}

#[test]
fn every_action_family_maps_to_a_named_icon() {
    let cases = [
        (Action::None, "none"),
        (Action::SwitchLayer("Studio".into()), "go-to-page"),
        (Action::Command("make".into()), "command"),
        (
            Action::Launch {
                executable: PathBuf::from("/usr/bin/firefox"),
                args: Vec::new(),
            },
            "application",
        ),
        (Action::KeyPress { key: "a".into() }, "shortcut"),
        (
            Action::Shortcut {
                modifiers: vec!["CTRL".into()],
                key: "c".into(),
            },
            "shortcut",
        ),
        (Action::Emit("lights".into()), "named-event"),
        (
            desktop(DesktopAction::PlayAudio {
                path: PathBuf::from("/tmp/a.wav"),
            }),
            "play-sound",
        ),
        (desktop(DesktopAction::StopAudio), "stop"),
        (
            desktop(DesktopAction::Media(MediaCommand::PlayPause)),
            "play-pause",
        ),
        (desktop(DesktopAction::Media(MediaCommand::Stop)), "stop"),
        (
            desktop(DesktopAction::Media(MediaCommand::Next)),
            "next-track",
        ),
        (
            desktop(DesktopAction::Media(MediaCommand::Previous)),
            "previous-track",
        ),
        (
            desktop(DesktopAction::Media(MediaCommand::VolumeUp)),
            "volume-up",
        ),
        (
            desktop(DesktopAction::Media(MediaCommand::VolumeDown)),
            "volume-down",
        ),
        (desktop(DesktopAction::Media(MediaCommand::Mute)), "mute"),
        (desktop(DesktopAction::CycleAudioOutput), "audio-output"),
        (
            desktop(DesktopAction::SetAudioOutput {
                name: "hdmi".into(),
            }),
            "audio-output",
        ),
        (
            desktop(DesktopAction::MouseScroll {
                direction: ScrollDirection::Down,
                ticks: 3,
            }),
            "scroll",
        ),
        (
            desktop(DesktopAction::Text {
                value: "hi".into(),
                enter: false,
            }),
            "text",
        ),
        (
            desktop(DesktopAction::Open {
                target: "/tmp".into(),
            }),
            "open-file",
        ),
        (
            desktop(DesktopAction::Website {
                url: "https://example.com".into(),
            }),
            "website",
        ),
        (
            desktop(DesktopAction::CloseApplication {
                executable: "firefox".into(),
            }),
            "close-application",
        ),
        (
            desktop(DesktopAction::ShortcutToggle {
                first: "a".into(),
                second: "b".into(),
            }),
            "toggle",
        ),
        (
            feature(FeatureAction::Navigation(NavigationAction::NextPage)),
            "next-page",
        ),
        (
            feature(FeatureAction::Navigation(NavigationAction::PreviousPage)),
            "previous-page",
        ),
        (
            feature(FeatureAction::Navigation(NavigationAction::GoToPage(2))),
            "go-to-page",
        ),
        (
            feature(FeatureAction::Navigation(NavigationAction::OpenFolder(
                "x".into(),
            ))),
            "folder",
        ),
        (
            feature(FeatureAction::Navigation(NavigationAction::ParentFolder)),
            "parent-folder",
        ),
        (
            feature(FeatureAction::Touchscreen(TouchscreenAction::NextPage)),
            "next-page",
        ),
        (
            feature(FeatureAction::Touchscreen(TouchscreenAction::PreviousPage)),
            "previous-page",
        ),
        (
            feature(FeatureAction::Device(DeviceAction::Brightness(50))),
            "brightness",
        ),
        (
            feature(FeatureAction::Device(DeviceAction::AdjustBrightness(-10))),
            "brightness",
        ),
        (
            feature(FeatureAction::Device(DeviceAction::Wake)),
            "brightness",
        ),
        (feature(FeatureAction::Device(DeviceAction::Sleep)), "sleep"),
        (feature(FeatureAction::Timer { seconds: 5 }), "timer"),
        (feature(FeatureAction::CancelTimer), "cancel-timer"),
        (
            feature(FeatureAction::Obs(ObsAction::Scene("Cam".into()))),
            "obs-scene",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::ToggleRecording)),
            "record",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::StartRecording)),
            "record",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::ResumeRecording)),
            "record",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::StopRecording)),
            "stop",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::PauseRecording)),
            "pause",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::ToggleStreaming)),
            "stream",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::StartStreaming)),
            "stream",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::StopStreaming)),
            "stop",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::ToggleMute("Mic".into()))),
            "microphone-mute",
        ),
        (
            feature(FeatureAction::Obs(ObsAction::SourceVisibility {
                scene: "a".into(),
                source: "b".into(),
                visible: true,
            })),
            "visibility",
        ),
    ];
    for (action, id) in cases {
        assert_eq!(BindingIcon::for_action(&action).id(), id, "{action:?}");
    }
}

#[test]
fn action_type_menu_rows_have_icons_and_unknown_rows_fall_back() {
    let rows = [
        "go-to-page",
        "command",
        "application",
        "shortcut",
        "named-event",
        "more",
        "play-sound",
        "none",
    ];
    for (index, id) in rows.iter().enumerate() {
        assert_eq!(BindingIcon::for_action_type(index as u32).id(), *id);
    }
    assert_eq!(BindingIcon::for_action_type(99), BindingIcon::Generic);
    assert_eq!(BindingIcon::Generic.id(), "generic");
}

#[test]
fn icon_ids_are_unique_and_every_icon_draws_visible_ink() {
    let mut seen = std::collections::BTreeSet::new();
    for icon in BindingIcon::ALL {
        assert!(seen.insert(icon.id()), "duplicate id {}", icon.id());
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 48, 48).unwrap();
        {
            let context = cairo::Context::new(&surface).unwrap();
            paint(&context, icon, (24.0, 24.0), 40.0, (1.0, 1.0, 1.0, 1.0)).unwrap();
        }
        let mut surface = surface;
        let data = surface.data().unwrap();
        let inked = data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| u32::from_ne_bytes(**pixel) >> 24 > 0x80)
            .count();
        assert!(inked > 20, "{} draws almost nothing ({inked})", icon.id());
    }
}

#[test]
fn only_actions_with_a_purpose_are_shown_on_hardware() {
    assert!(!BindingIcon::for_action(&Action::None).shown_on_controls());
    assert!(BindingIcon::for_action(&Action::Command("x".into())).shown_on_controls());
}
