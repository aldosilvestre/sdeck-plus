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
            // Lit in any accent color (icons) or white (captions).
            let lit = bytes[offset..offset + 3]
                .iter()
                .any(|channel| *channel > 180);
            if alpha > 200 && lit {
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
                p[3] > 200 && p[..3].iter().any(|channel| *channel > 180)
            })
            .count()
    };
    assert!(bright_in(6..78) > 250, "a large icon on the left");
    assert!(bright_in(84..194) > 60, "the caption to its right");
    // A translucent graphite tile, not transparent and not black, with
    // rounded transparent corners so the strip shows around it.
    let tile = pixel(80, 92);
    assert!(tile[3] > 8 && tile[3] < 60, "see-through tile: {tile:?}");
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
            .filter(|&o| bytes[o + 3] > 200 && bytes[o..o + 3].iter().any(|c| *c > 180))
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
        (Glyph::Play, Some(BindingIcon::Play)),
        (Glyph::Pause, Some(BindingIcon::Pause)),
        (Glyph::MediaIdle, Some(BindingIcon::MediaIdle)),
    ] {
        assert_eq!(BindingIcon::for_glyph(glyph), icon, "{glyph:?}");
        if let Some(icon) = icon {
            assert!(BindingIcon::ALL.contains(&icon), "{icon:?} is in ALL");
        }
    }
}

#[test]
fn long_captions_wrap_between_words_and_ellipsize_instead_of_breaking_words() {
    let input = "Website https://example.com/a/very/long/path and more words beyond";
    let words: Vec<&str> = input.split_whitespace().collect();
    for (size, width, lines) in [(14.0, 112, 2), (17.0, 100, 2), (17.0, 60, 1)] {
        let shown = caption_lines(input, size, width, lines);
        assert!(!shown.is_empty() && shown.len() <= lines);
        // Every line holds whole words, in order; nothing is cut mid-word.
        let joined: Vec<&str> = shown.iter().flat_map(|line| line.split(' ')).collect();
        assert_eq!(joined, words[..joined.len()], "{shown:?}");
        let layout = caption_layout(input, size, width, lines as i32);
        assert_eq!(
            layout.line_count() as usize,
            shown.len(),
            "no extra wrapping"
        );
        assert!(
            layout.pixel_size().0 <= width,
            "too wide lines are ellipsized"
        );
    }
    // Words that fit stay whole on the first line.
    assert_eq!(caption_lines("Media Next", 14.0, 112, 2), ["Media Next"]);
    assert_eq!(
        caption_lines("OBS scene: Camera", 17.0, 100, 2),
        ["OBS scene:", "Camera"]
    );
}

fn tile_alpha(alpha: f64, x: usize, y: usize) -> u8 {
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 200, 100).unwrap();
    {
        let context = cairo::Context::new(&surface).unwrap();
        draw_zone_tile(&context, alpha).unwrap();
    }
    let stride = surface.stride() as usize;
    let data = surface.data().unwrap();
    let offset = y * stride + x * 4;
    (u32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap()) >> 24) as u8
}

#[test]
fn section_tiles_are_clearly_see_through() {
    // About 0.15 over the plain strip and 0.10 over section artwork, fading
    // softly downwards; never zero, so the section still reads as a tile.
    for (alpha, most) in [(ZONE_TILE_ALPHA, 46), (ZONE_TILE_ARTWORK_ALPHA, 31)] {
        for y in [12, 50, 88] {
            let shown = tile_alpha(alpha, 100, y);
            assert!(shown > 5 && shown <= most, "{alpha} at y={y}: {shown}");
        }
        assert!(
            tile_alpha(alpha, 100, 88) < tile_alpha(alpha, 100, 12),
            "the fill fades towards the bottom"
        );
    }
    const { assert!(ZONE_TILE_ARTWORK_ALPHA < ZONE_TILE_ALPHA) };
    // The fine light edge stays subtle.
    assert!(tile_alpha(ZONE_TILE_ALPHA, 100, 4) < 50);
}

fn alphas(icon: BindingIcon) -> Vec<u8> {
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 48, 48).unwrap();
    {
        let context = cairo::Context::new(&surface).unwrap();
        paint(&context, icon, (24.0, 24.0), 48.0, (1.0, 1.0, 1.0, 1.0)).unwrap();
    }
    let data = surface.data().unwrap();
    data.as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| (u32::from_ne_bytes(*pixel) >> 24) as u8)
        .collect()
}

#[test]
fn every_icon_is_a_duotone_of_a_soft_fill_under_a_crisp_stroke() {
    // The soft layer is SOFT_ALPHA of the ink (89/255 for a white ink).
    let soft = (SOFT_ALPHA * 255.0).round() as u8;
    for icon in BindingIcon::ALL {
        let alphas = alphas(icon);
        let soft_layer = alphas.iter().filter(|a| a.abs_diff(soft) <= 2).count();
        let primary = alphas.iter().filter(|a| **a > 230).count();
        assert!(soft_layer > 40, "{}: soft layer {soft_layer}", icon.id());
        assert!(primary > 40, "{}: primary layer {primary}", icon.id());
    }
}

#[test]
fn hardware_icons_sit_on_a_soft_drop_shadow_not_a_hard_outline() {
    let draw = |shadowed: bool| {
        let mut surface = cairo::ImageSurface::create(cairo::Format::Rgb24, 64, 64).unwrap();
        {
            let context = cairo::Context::new(&surface).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
            let white = (1.0, 1.0, 1.0, 1.0);
            if shadowed {
                paint_outlined(&context, BindingIcon::Record, (32.0, 32.0), 40.0, white)
            } else {
                paint(&context, BindingIcon::Record, (32.0, 32.0), 40.0, white)
            }
            .unwrap();
        }
        let data = surface.data().unwrap();
        data.as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| (u32::from_ne_bytes(*pixel) & 0xff) as u8)
            .collect::<Vec<u8>>()
    };
    // Over plain white the glyph itself is invisible; what darkens is shadow.
    let (shadowed, plain) = (draw(true), draw(false));
    assert!(plain.iter().all(|value| *value == 255));
    let dark = shadowed.iter().filter(|value| **value < 60).count();
    let soft = shadowed
        .iter()
        .filter(|value| (90..235).contains(*value))
        .count();
    assert!(soft > 150, "a visible halo: {soft}");
    assert!(soft > dark * 2, "fades softly ({soft} soft, {dark} dark)");
    // It drops slightly downwards.
    let darkness = |rows: std::ops::Range<usize>| -> u32 {
        rows.flat_map(|y| (0..64).map(move |x| y * 64 + x))
            .map(|offset| u32::from(255 - shadowed[offset]))
            .sum()
    };
    assert!(darkness(32..64) > darkness(0..32));
}

#[test]
fn hardware_accents_follow_the_icon_family() {
    use IconCategory as C;
    for (icon, category) in [
        (BindingIcon::PlayPause, C::Media),
        (BindingIcon::NextTrack, C::Media),
        (BindingIcon::PlaySound, C::Media),
        (BindingIcon::Play, C::Media),
        (BindingIcon::MediaIdle, C::Media),
        (BindingIcon::VolumeUp, C::Audio),
        (BindingIcon::Mute, C::Audio),
        (BindingIcon::AudioOutput, C::Audio),
        (BindingIcon::Record, C::Obs),
        (BindingIcon::Stream, C::Obs),
        (BindingIcon::ObsScene, C::Obs),
        (BindingIcon::NextPage, C::Navigation),
        (BindingIcon::Folder, C::Navigation),
        (BindingIcon::GoToPage, C::Navigation),
        (BindingIcon::Command, C::System),
        (BindingIcon::Application, C::System),
        (BindingIcon::Brightness, C::System),
        (BindingIcon::Website, C::Web),
        (BindingIcon::Rain, C::Weather),
        (BindingIcon::Cpu, C::Monitor),
    ] {
        assert_eq!(icon.category(), category, "{}", icon.id());
        assert_eq!(icon.accent(), category.accent());
    }
    // Navigation stays neutral white; the families are told apart by hue.
    let (r, g, b, _) = C::Navigation.accent();
    assert!(r > 0.9 && g > 0.9 && b > 0.9);
    let accents: Vec<_> = [C::Media, C::Audio, C::Obs, C::System, C::Web]
        .iter()
        .map(|c| c.accent())
        .collect();
    for (index, a) in accents.iter().enumerate() {
        for b in &accents[index + 1..] {
            let distance = (a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs();
            assert!(distance > 0.25, "{a:?} vs {b:?}");
        }
    }
    // Keys carry the accent: a red record icon, a white page icon.
    let reddish = |icon: BindingIcon| {
        let image = compose_key_image(icon, "", None).unwrap().unwrap();
        let bytes = image.read_pixel_bytes();
        bytes
            .chunks(3)
            .filter(|p| p[0] > 200 && u16::from(p[1]) + 70 < u16::from(p[0]))
            .count()
    };
    assert!(reddish(BindingIcon::Record) > 100);
    assert_eq!(reddish(BindingIcon::NextPage), 0);
}

/// Opt-in visual review: `STREAMDECK_ICON_SHEET=/path/sheet.png` writes every
/// icon as a dark key, a colored key, a key over light artwork and a section
/// over strip artwork at device size, then key and section at preview size
/// and the themed menu icon on dark and light rows. Unset, nothing is written.
#[test]
fn icon_contact_sheet_when_explicitly_requested() {
    let Some(path) = std::env::var_os("STREAMDECK_ICON_SHEET") else {
        return;
    };
    const GROUP: f64 = 860.0;
    const ROW: f64 = 128.0;
    let per_row = 2;
    let rows = BindingIcon::ALL.len().div_ceil(per_row);
    let (width, height) = (
        GROUP as i32 * per_row as i32,
        (rows as f64 * ROW) as i32 + 8,
    );
    let sheet = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).unwrap();
    let cairo = cairo::Context::new(&sheet).unwrap();
    cairo.set_source_rgb(0.08, 0.10, 0.12);
    cairo.paint().unwrap();
    let place = |image: &gtk::gdk_pixbuf::Pixbuf, x: f64, y: f64, scale: f64| {
        let surface = pixbuf_surface(image);
        cairo.save().unwrap();
        cairo.translate(x, y);
        cairo.scale(scale, scale);
        cairo.set_source_surface(&surface, 0.0, 0.0).unwrap();
        cairo.source().set_filter(cairo::Filter::Good);
        cairo.paint().unwrap();
        cairo.restore().unwrap();
    };
    let backdrop = |x: f64, y: f64, w: f64, h: f64, stops: &[(f64, f64, f64)]| {
        let fill = cairo::LinearGradient::new(x, y, x + w, y + h);
        for (index, (r, g, b)) in stops.iter().enumerate() {
            fill.add_color_stop_rgb(index as f64 / (stops.len() - 1) as f64, *r, *g, *b);
        }
        cairo.rectangle(x, y, w, h);
        cairo.set_source(&fill).unwrap();
        cairo.fill().unwrap();
    };
    let colored = [(0.20, 0.25, 0.62), (0.42, 0.17, 0.56)];
    let light = [(0.99, 0.93, 0.75), (0.80, 0.90, 0.98), (1.0, 1.0, 1.0)];
    let strip = [(0.95, 0.55, 0.25), (0.80, 0.30, 0.55), (0.20, 0.65, 0.70)];
    for (index, icon) in BindingIcon::ALL.into_iter().enumerate() {
        let x = (index % per_row) as f64 * GROUP + 8.0;
        let y = (index / per_row) as f64 * ROW + 8.0;
        let caption = icon.id().replace('-', " ");
        let key = compose_key_image(icon, &caption, None).unwrap().unwrap();
        place(&key, x, y, 1.0);
        for (column, stops) in [(1.0, &colored[..]), (2.0, &light[..])] {
            let left = x + column * 124.0;
            backdrop(left, y, 120.0, 120.0, stops);
            // The clip keeps the shadow's groups key-sized.
            cairo.save().unwrap();
            cairo.rectangle(left, y, 120.0, 120.0);
            cairo.clip();
            paint_outlined(&cairo, icon, (left + 60.0, y + 39.0), 50.0, icon.accent()).unwrap();
            let layout = caption_layout(&caption, 14.0, KEY - 8, 2);
            draw_caption(&cairo, &layout, (left + 4.0, y + 72.0)).unwrap();
            cairo.restore().unwrap();
        }
        let zone = compose_zone_image(icon, &caption, false, index % 3 == 0).unwrap();
        backdrop(x + 372.0, y + 10.0, 200.0, 100.0, &strip);
        place(&zone, x + 372.0, y + 10.0, 1.0);
        place(&key, x + 580.0, y + 30.0, 0.5);
        cairo.rectangle(x + 646.0, y + 35.0, 100.0, 50.0);
        cairo.set_source_rgb(0.0, 0.0, 0.0);
        cairo.fill().unwrap();
        place(&zone, x + 646.0, y + 35.0, 0.5);
        for (row, background, ink) in [
            (0.0, (0.13, 0.15, 0.18), (0.95, 0.96, 0.97, 1.0)),
            (1.0, (0.96, 0.97, 0.98), (0.13, 0.15, 0.18, 1.0)),
        ] {
            let top = y + 22.0 + row * 40.0;
            cairo.rectangle(x + 754.0, top, 88.0, 36.0);
            cairo.set_source_rgb(background.0, background.1, background.2);
            cairo.fill().unwrap();
            paint(&cairo, icon, (x + 772.0, top + 18.0), 16.0, ink).unwrap();
            paint(&cairo, icon, (x + 812.0, top + 18.0), 24.0, ink).unwrap();
        }
    }
    drop(cairo);
    let path = std::path::PathBuf::from(path);
    surface_pixbuf(sheet).savev(&path, "png", &[]).unwrap();
    // Readable crops of eight rows each, next to the sheet.
    let image = gtk::gdk_pixbuf::Pixbuf::from_file(&path).unwrap();
    for (part, first) in (0..rows).step_by(8).enumerate() {
        let top = (first as f64 * ROW) as i32;
        let tall = ((8.0 * ROW) as i32 + 8).min(height - top);
        let crop = image.new_subpixbuf(0, top, width, tall);
        let name = format!("{}-part{}.png", path.with_extension("").display(), part + 1);
        crop.savev(name, "png", &[]).unwrap();
    }
}

fn pixbuf_surface(image: &gtk::gdk_pixbuf::Pixbuf) -> cairo::ImageSurface {
    let (width, height) = (image.width(), image.height());
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).unwrap();
    let stride = surface.stride() as usize;
    let bytes = image.read_pixel_bytes();
    let (source, channels) = (image.rowstride() as usize, image.n_channels() as usize);
    {
        let mut data = surface.data().unwrap();
        for y in 0..height as usize {
            for x in 0..width as usize {
                let p = &bytes[y * source + x * channels..];
                let alpha = if channels == 4 { u32::from(p[3]) } else { 255 };
                let pre = |c: u8| u32::from(c) * alpha / 255;
                let pixel = alpha << 24 | pre(p[0]) << 16 | pre(p[1]) << 8 | pre(p[2]);
                data[y * stride + x * 4..y * stride + x * 4 + 4]
                    .copy_from_slice(&pixel.to_ne_bytes());
            }
        }
    }
    surface
}

fn surface_pixbuf(mut surface: cairo::ImageSurface) -> gtk::gdk_pixbuf::Pixbuf {
    let (width, height) = (surface.width(), surface.height());
    let stride = surface.stride() as usize;
    let data = surface.data().unwrap();
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let offset = y * stride + x * 4;
            let pixel = u32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap());
            rgb.extend_from_slice(&pixel.to_be_bytes()[1..]);
        }
    }
    gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &gtk::glib::Bytes::from_owned(rgb),
        gtk::gdk_pixbuf::Colorspace::Rgb,
        false,
        8,
        width,
        height,
        width * 3,
    )
}
