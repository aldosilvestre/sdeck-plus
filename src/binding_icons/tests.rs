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

fn bright(image: &gtk::gdk_pixbuf::Pixbuf, rows: std::ops::Range<i32>) -> usize {
    let bytes = image.read_pixel_bytes();
    let stride = image.rowstride() as usize;
    let channels = image.n_channels() as usize;
    let mut count = 0;
    for y in rows {
        for x in 0..image.width() {
            let offset = y as usize * stride + x as usize * channels;
            let alpha = if image.has_alpha() {
                bytes[offset + 3]
            } else {
                255
            };
            if alpha > 200 && bytes[offset] > 200 && bytes[offset + 1] > 200 {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn keys_without_text_show_the_icon_above_its_caption() {
    let image = compose_key_image(BindingIcon::Command, "Run make", None)
        .unwrap()
        .expect("an icon key");
    assert_eq!((image.width(), image.height()), (120, 120));
    assert!(bright(&image, 14..64) > 80, "icon in the upper band");
    assert!(bright(&image, 70..110) > 20, "caption in the lower band");
    // Black around, like an empty key.
    assert_eq!(bright(&image, 0..4), 0);
}

#[test]
fn user_text_keeps_its_placement_and_the_icon_uses_the_free_band() {
    use crate::button_text::{ButtonText, VerticalAlignment};
    let text = ButtonText {
        text: "Play".into(),
        vertical_alignment: VerticalAlignment::Bottom,
        ..ButtonText::default()
    };
    let plain = crate::image_output::compose_button_image(None, &text).unwrap();
    let image = compose_key_image(BindingIcon::PlayPause, "Media PlayPause", Some(&text))
        .unwrap()
        .expect("room above bottom text");
    assert!(bright(&image, 10..60) > 80);
    // The user's text pixels are untouched.
    let rows = 84..120;
    let crop = |image: &gtk::gdk_pixbuf::Pixbuf| {
        let bytes = image.read_pixel_bytes();
        let stride = image.rowstride() as usize;
        rows.clone()
            .flat_map(|y| bytes[y * stride..y * stride + 360].to_vec())
            .collect::<Vec<u8>>()
    };
    assert_eq!(crop(&image), crop(&plain));
    let centered = ButtonText {
        text: "Play".into(),
        ..ButtonText::default()
    };
    assert!(
        compose_key_image(BindingIcon::PlayPause, "x", Some(&centered))
            .unwrap()
            .is_none(),
        "centered user text leaves no room, so the key stays as before"
    );
}

#[test]
fn touch_sections_lay_the_icon_beside_its_caption_on_a_strip_tile() {
    let image =
        compose_zone_image(BindingIcon::ObsScene, "OBS scene: Camera", false, false).unwrap();
    assert_eq!((image.width(), image.height()), (200, 100));
    assert!(image.has_alpha());
    let bytes = image.read_pixel_bytes();
    let stride = image.rowstride() as usize;
    let pixel = |x: usize, y: usize| &bytes[y * stride + x * 4..y * stride + x * 4 + 4];
    let bright_in = |columns: std::ops::Range<usize>| {
        (12..88)
            .flat_map(|y| columns.clone().map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let p = pixel(x, y);
                p[3] > 200 && p[0] > 200 && p[1] > 200
            })
            .count()
    };
    assert!(bright_in(6..78) > 250, "a large icon on the left");
    assert!(bright_in(84..194) > 60, "the caption to its right");
    // A translucent graphite tile, not transparent and not black, with
    // rounded transparent corners so the strip shows around it.
    let tile = pixel(80, 92);
    assert!(tile[3] > 60 && tile[3] < 255, "translucent tile: {tile:?}");
    assert!(tile[0] > 20, "graphite, not black: {tile:?}");
    assert_eq!(pixel(0, 0)[3], 0);
    assert_eq!(pixel(199, 99)[3], 0);
}

#[test]
fn edge_page_indicators_keep_their_room_beside_the_section_icon() {
    let bright_columns = |image: &gtk::gdk_pixbuf::Pixbuf, columns: std::ops::Range<usize>| {
        let bytes = image.read_pixel_bytes();
        let stride = image.rowstride() as usize;
        (30..70)
            .flat_map(|y| columns.clone().map(move |x| y * stride + x * 4))
            .filter(|&o| bytes[o + 3] > 200 && bytes[o] > 200 && bytes[o + 1] > 200)
            .count()
    };
    let plain = compose_zone_image(BindingIcon::NextPage, "Next page", false, false).unwrap();
    let previous = compose_zone_image(BindingIcon::NextPage, "Next page", true, false).unwrap();
    let next = compose_zone_image(BindingIcon::Mute, "Media Mute Mute Mute", false, true).unwrap();
    assert!(bright_columns(&plain, 0..34) > 0 || bright_columns(&plain, 34..80) > 0);
    assert_eq!(
        bright_columns(&previous, 0..32),
        0,
        "left chevron area stays clear"
    );
    assert_eq!(
        bright_columns(&next, 168..200),
        0,
        "right chevron area stays clear"
    );
}

#[test]
fn target_markers_fill_exactly_their_control_on_a_mini_map() {
    use crate::local_artwork::ArtworkTarget;
    let filled = |target: ArtworkTarget, x: i32, y: i32| {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 48, 48).unwrap();
        {
            let context = cairo::Context::new(&surface).unwrap();
            paint_target(&context, target, (24.0, 24.0), 48.0, (1.0, 1.0, 1.0, 1.0)).unwrap();
        }
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let offset = y as usize * stride + x as usize * 4;
        u32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap()) >> 24 > 0xc0
    };
    // Centers on the 48px map: key 1 top-left, key 8 bottom-right, zone 3, strip.
    assert!(filled(ArtworkTarget::Keypad(1), 6, 12));
    assert!(!filled(ArtworkTarget::Keypad(2), 6, 12));
    assert!(filled(ArtworkTarget::Keypad(8), 42, 20));
    assert!(filled(ArtworkTarget::EncoderZone(3), 30, 40));
    assert!(!filled(ArtworkTarget::EncoderZone(3), 18, 40));
    assert!(filled(ArtworkTarget::TouchStripBackground, 18, 40));
    assert_eq!(
        target_marker_id(ArtworkTarget::EncoderZone(3)),
        "target-zone-3"
    );
    assert_eq!(
        target_marker_id(ArtworkTarget::TouchStripBackground),
        "target-strip"
    );
    assert_eq!(target_marker_id(ArtworkTarget::Keypad(5)), "target-key-5");
}

#[test]
fn live_information_glyphs_come_from_the_binding_vocabulary() {
    use crate::info::Glyph;
    // Shared meanings reuse the binding icon; the rest are new vocabulary icons.
    for (glyph, icon) in [
        (Glyph::None, None),
        (Glyph::Sun, Some(BindingIcon::Brightness)),
        (Glyph::SpeakerMuted, Some(BindingIcon::Mute)),
        (Glyph::Headphones, Some(BindingIcon::AudioOutput)),
        (Glyph::Record, Some(BindingIcon::Record)),
        (Glyph::Live, Some(BindingIcon::Stream)),
        (Glyph::ObsIdle, Some(BindingIcon::ObsScene)),
        (Glyph::Speaker, Some(BindingIcon::Volume)),
        (Glyph::Cloud, Some(BindingIcon::Cloud)),
        (Glyph::PartlyCloudy, Some(BindingIcon::PartlyCloudy)),
        (Glyph::Rain, Some(BindingIcon::Rain)),
        (Glyph::Snow, Some(BindingIcon::Snow)),
        (Glyph::Storm, Some(BindingIcon::Storm)),
        (Glyph::Fog, Some(BindingIcon::Fog)),
        (Glyph::Chip, Some(BindingIcon::Cpu)),
        (Glyph::Memory, Some(BindingIcon::Memory)),
        (Glyph::Thermometer, Some(BindingIcon::Temperature)),
    ] {
        assert_eq!(BindingIcon::for_glyph(glyph), icon, "{glyph:?}");
        if let Some(icon) = icon {
            assert!(BindingIcon::ALL.contains(&icon), "{icon:?} is in ALL");
        }
    }
}
