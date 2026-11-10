//! Independent rotation and layout probes: corner markers of distinct
//! colours show where each corner of the composed picture lands.

use super::sim_tests::Scratch;
use super::*;
use crate::device_model::{Rotation, PLUS_XL};
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};

/// Colours of the top-left, top-right, bottom-right and bottom-left markers.
const CORNERS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [255, 255, 0], [255, 0, 255]];

fn corner_picture(
    scratch: &Scratch,
    name: &str,
    (width, height): (i32, i32),
) -> std::path::PathBuf {
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, width, height).unwrap();
    image.fill(0x202020ff);
    for (corner, [r, g, b]) in CORNERS.iter().enumerate() {
        let x = if corner == 1 || corner == 2 {
            width - 24
        } else {
            0
        };
        let y = if corner >= 2 { height - 24 } else { 0 };
        image
            .new_subpixbuf(x, y, 24, 24)
            .fill(u32::from_be_bytes([*r, *g, *b, 255]));
    }
    let path = scratch.0.join(name);
    image.savev(&path, "png", &[]).unwrap();
    path
}

fn near(image: &Pixbuf, x: i32, y: i32, [r, g, b]: [u8; 3]) -> bool {
    let at = (y * image.rowstride() + x * image.n_channels()) as usize;
    let pixels = image.read_pixel_bytes();
    let pixel = &pixels.as_ref()[at..at + 3];
    [(pixel[0], r), (pixel[1], g), (pixel[2], b)]
        .iter()
        .all(|(got, want)| got.abs_diff(*want) < 90)
}

/// Source corner `n` must sit at `(x, y)` of the decoded upload.
fn assert_corners(jpeg: &[u8], size: (i32, i32), places: [(i32, i32); 4]) {
    let image = Pixbuf::from_read(std::io::Cursor::new(jpeg.to_vec())).unwrap();
    assert_eq!((image.width(), image.height()), size);
    for (corner, (x, y)) in places.into_iter().enumerate() {
        assert!(
            near(&image, x, y, CORNERS[corner]),
            "corner {corner} at ({x}, {y})"
        );
    }
}

/// Where the four source corners land after a counterclockwise quarter turn
/// of a `width` x `height` picture.
fn ccw_places(width: i32, height: i32) -> [(i32, i32); 4] {
    [
        (8, height - 9),
        (8, 8),
        (width - 9, 8),
        (width - 9, height - 9),
    ]
}

#[test]
fn xl_key_strip_and_zone_corners_land_where_a_ccw_turn_puts_them() {
    let scratch = Scratch::new("rotation-corners");
    for (target, size, name) in [
        (ArtworkTarget::Keypad(1), (112, 112), "key"),
        (ArtworkTarget::TouchStripBackground, (1200, 100), "strip"),
        (ArtworkTarget::EncoderZone(1), (200, 100), "zone"),
    ] {
        let path = corner_picture(&scratch, name, size);
        let jpeg = image_output::prepare_jpeg_for(&PLUS_XL, target, path).unwrap();
        // After the turn the picture is `height` wide and `width` tall.
        assert_corners(&jpeg, (size.1, size.0), ccw_places(size.1, size.0));
    }
}

#[test]
fn key_flip_y_mirrors_the_rotated_key_vertically() {
    static FLIPPED: DeviceModel = DeviceModel {
        key_flip_y: true,
        ..PLUS_XL
    };
    let scratch = Scratch::new("rotation-flip");
    let path = corner_picture(&scratch, "key", (112, 112));
    let jpeg = image_output::prepare_jpeg_for(&FLIPPED, ArtworkTarget::Keypad(1), path).unwrap();
    // Rotated placement is [BL, TL, TR, BR]; the vertical mirror swaps top and bottom.
    assert_corners(&jpeg, (112, 112), [(8, 8), (8, 103), (103, 103), (103, 8)]);
}

#[test]
fn partial_window_headers_are_logical_or_swapped_by_the_model_field() {
    static SWAPPED: DeviceModel = DeviceModel {
        partial_window_axis_swap: true,
        ..PLUS_XL
    };
    for (model, zone, expected) in [
        (&PLUS_XL, 1u8, (0, 0, 200, 100)),
        (&PLUS_XL, 6, (1000, 0, 200, 100)),
        (&SWAPPED, 1, (0, 0, 100, 200)),
        (&SWAPPED, 6, (0, 1000, 100, 200)),
    ] {
        let reports =
            image_output::encode_reports_for(model, ArtworkTarget::EncoderZone(zone), &[1; 3])
                .unwrap();
        let first = &reports[0].0;
        assert_eq!(first.len(), 1024);
        let word = |at: usize| u16::from_le_bytes([first[at], first[at + 1]]);
        assert_eq!((word(2), word(4), word(6), word(8)), expected);
        assert_eq!((first[0], first[1], first[10]), (2, 0x0c, 1));
        assert_eq!((word(11), word(13)), (0, 3));
    }
}

#[test]
fn plus_composition_is_never_rotated() {
    assert_eq!(
        (PLUS.key_rotation, PLUS.strip_rotation, PLUS.key_flip_y),
        (Rotation::None, Rotation::None, false)
    );
}

#[test]
fn every_xl_encode_path_produces_device_sized_rotated_pictures() {
    let size = |jpeg: Vec<u8>| {
        let image = Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
        (image.width(), image.height())
    };
    let text = crate::button_text::ButtonText {
        text: "Hi".into(),
        ..Default::default()
    };
    let info = crate::info::InfoValue {
        glyph: crate::info::Glyph::Speaker,
        level: Some(50),
        headline: "50%".into(),
        detail: String::new(),
        inactive: false,
        app_icon: None,
    };
    let icon = crate::binding_icons::BindingIcon::Command;
    let key = ArtworkTarget::Keypad(36);
    let zone = ArtworkTarget::EncoderZone(6);
    let m = &PLUS_XL;
    assert_eq!(
        size(image_output::prepare_button_jpeg_for(m, None, &text).unwrap()),
        (112, 112)
    );
    assert_eq!(
        size(image_output::prepare_clear_jpeg_for(m, key).unwrap()),
        (112, 112)
    );
    assert_eq!(
        size(image_output::prepare_clear_jpeg_for(m, zone).unwrap()),
        (100, 200)
    );
    assert_eq!(
        size(
            image_output::prepare_info_jpeg_for(m, key, &info, (None, None), Duration::ZERO)
                .unwrap()
        ),
        (112, 112)
    );
    assert_eq!(
        size(
            image_output::prepare_info_jpeg_for(
                m,
                zone,
                &info,
                (None, None),
                Duration::from_secs(3)
            )
            .unwrap()
        ),
        (100, 200)
    );
    assert_eq!(
        size(
            image_output::prepare_icon_jpeg_for(m, key, icon, "Run", None, None, (false, false))
                .unwrap()
        ),
        (112, 112)
    );
    assert_eq!(
        size(
            image_output::prepare_icon_jpeg_for(m, zone, icon, "Run", None, None, (true, true))
                .unwrap()
        ),
        (100, 200)
    );
    assert_eq!(
        size(
            image_output::prepare_touchscreen_zone_jpeg_for(
                m,
                6,
                (None, None),
                None,
                (true, false),
                Duration::ZERO
            )
            .unwrap()
        ),
        (100, 200)
    );
}

#[test]
fn xl_marquee_frames_keep_the_rotated_zone_size() {
    let info = crate::info::InfoValue {
        glyph: crate::info::Glyph::Speaker,
        level: None,
        headline: "A very long scrolling title that cannot fit".into(),
        detail: String::new(),
        inactive: false,
        app_icon: None,
    };
    for elapsed in [Duration::ZERO, Duration::from_millis(900)] {
        let reports = prepare_artwork_at_for(
            &PLUS_XL,
            ArtworkCommand::ComposeInfo {
                target: ArtworkTarget::EncoderZone(3),
                path: None,
                strip_path: None,
                value: info.clone(),
            },
            elapsed,
        )
        .unwrap();
        assert!(reports.iter().all(|report| report.0.len() == 1024));
        assert_eq!(&reports[0].0[..2], &[2, 0x0c]);
    }
}
