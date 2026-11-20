//! The Legacy Mini protocol of the Stream Deck Mini, checked byte for byte
//! against Elgato's Mini page (https://docs.elgato.com/streamdeck/hid/mini):
//! input report 0x01 with one state byte per key, 1024-byte image reports
//! with a 16-byte `02 01` header, and 80x80 BMP key pictures transposed as
//! the open-source drivers send them. No hardware is involved.

use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use sdeck_plus::{
    binding_icons::BindingIcon,
    button_text::ButtonText,
    device_model::MINI,
    image_output,
    info::InfoSource,
    local_artwork::ArtworkTarget,
    protocol::{decode_report_for, try_decode_report_for, Event},
};
use std::{path::PathBuf, time::Duration};

/// Size of a cleared 80x80 key: 54 header bytes plus 80 rows of 240 bytes.
const KEY_BMP_LEN: usize = 54 + 80 * 80 * 3;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sdeck-mini-protocol-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A 65-byte Mini input report: id 0x01, then `states`, zero padded.
fn mini_report(states: &[u8]) -> Vec<u8> {
    let mut report = vec![0x01];
    report.extend_from_slice(states);
    report.resize(65, 0);
    report
}

fn key(index: usize, pressed: bool) -> Event {
    Event::Key { index, pressed }
}

#[test]
fn mini_key_reports_decode_each_of_the_six_keys_from_byte_one() {
    assert_eq!(
        decode_report_for(&MINI, &mini_report(&[1, 0, 0, 0, 0, 1])),
        [
            key(0, true),
            key(1, false),
            key(2, false),
            key(3, false),
            key(4, false),
            key(5, true),
        ]
    );
    assert_eq!(
        try_decode_report_for(&MINI, &mini_report(&[0, 0, 1, 0, 0, 0])),
        Ok(vec![
            key(0, false),
            key(1, false),
            key(2, true),
            key(3, false),
            key(4, false),
            key(5, false),
        ])
    );
    // Only the first 6 payload bytes are keys; the rest is ignored.
    let mut trailing = mini_report(&[0; 6]);
    trailing[7..].fill(1);
    assert!(decode_report_for(&MINI, &trailing)
        .iter()
        .all(|event| matches!(event, Event::Key { pressed: false, index } if *index < 6)));
    // A state other than 0 or 1 is not a key event.
    assert_eq!(
        decode_report_for(&MINI, &mini_report(&[2, 1, 0, 0, 0, 0])),
        [
            key(1, true),
            key(2, false),
            key(3, false),
            key(4, false),
            key(5, false),
        ]
    );
}

#[test]
fn mini_reports_with_another_id_or_too_short_decode_to_nothing() {
    let mut other = mini_report(&[1; 6]);
    other[0] = 0x02;
    assert_eq!(try_decode_report_for(&MINI, &other), Ok(Vec::new()));
    // The report id plus fewer than six key bytes.
    assert!(decode_report_for(&MINI, &[0x01, 1, 1, 1, 1, 1]).is_empty());
    assert!(decode_report_for(&MINI, &[]).is_empty());
    // Exactly the id and six keys is enough.
    assert_eq!(
        decode_report_for(&MINI, &[0x01, 0, 0, 0, 1, 0, 0])[3],
        key(3, true)
    );
}

/// The 16-byte header Elgato's page gives: report 0x02, command 0x01, chunk
/// index, 0x00, Show Image flag, key index, ten zero bytes.
fn header(chunk: u8, last: bool, key_index: u8) -> [u8; 16] {
    let mut header = [0; 16];
    header[..6].copy_from_slice(&[0x02, 0x01, chunk, 0x00, u8::from(last), key_index]);
    header
}

#[test]
fn mini_image_reports_carry_the_16_byte_header_of_elgatos_mini_page() {
    let payload: Vec<u8> = (0..2017u32).map(|n| (n % 251 + 1) as u8).collect();
    for (number, key_index) in [(1u8, 1u8), (4, 4), (6, 6)] {
        let reports =
            image_output::encode_reports_for(&MINI, ArtworkTarget::Keypad(number), &payload)
                .unwrap();
        assert_eq!(reports.len(), 3, "1008 payload bytes per report");
        for (chunk, report) in reports.iter().enumerate() {
            let report = &report.0;
            assert_eq!(report.len(), 1024);
            assert_eq!(
                report[..16],
                header(chunk as u8, chunk == 2, key_index),
                "key {number} chunk {chunk}"
            );
        }
        assert_eq!(reports[0].0[16..], payload[..1008]);
        assert_eq!(reports[1].0[16..], payload[1008..2016]);
        assert_eq!(reports[2].0[16], payload[2016]);
        assert!(reports[2].0[17..].iter().all(|byte| *byte == 0));
    }
    // A payload that fills one report exactly is a single, final chunk.
    let full =
        image_output::encode_reports_for(&MINI, ArtworkTarget::Keypad(2), &[7; 1008]).unwrap();
    assert_eq!(full.len(), 1);
    assert_eq!(full[0].0[..16], header(0, true, 2));
    assert!(full[0].0[16..].iter().all(|byte| *byte == 7));
}

#[test]
fn mini_image_reports_refuse_controls_it_lacks_empty_and_oversized_images() {
    for target in [
        ArtworkTarget::Keypad(0),
        ArtworkTarget::Keypad(7),
        ArtworkTarget::TouchStripBackground,
        ArtworkTarget::EncoderZone(1),
    ] {
        assert_eq!(
            image_output::encode_reports_for(&MINI, target, &[1]),
            Err("unsupported artwork target".to_owned()),
            "{target:?}"
        );
    }
    assert_eq!(
        image_output::encode_reports_for(&MINI, ArtworkTarget::Keypad(1), &[]),
        Err("image payload must not be empty".to_owned())
    );
    // The chunk index is one byte: 256 chunks at most.
    let most = vec![1; 256 * 1008];
    let reports = image_output::encode_reports_for(&MINI, ArtworkTarget::Keypad(1), &most).unwrap();
    assert_eq!(reports.len(), 256);
    assert_eq!(reports[255].0[..16], header(255, true, 1));
    assert_eq!(
        image_output::encode_reports_for(
            &MINI,
            ArtworkTarget::Keypad(1),
            &[most, vec![1]].concat()
        ),
        Err("image requires too many chunks".to_owned())
    );
}

/// The 54-byte header of an 80x80, 24-bit, bottom-up BMP with no palette.
fn key_bmp_header() -> Vec<u8> {
    let mut header = Vec::new();
    header.extend_from_slice(b"BM");
    header.extend_from_slice(&(KEY_BMP_LEN as u32).to_le_bytes());
    header.extend_from_slice(&[0, 0, 0, 0]);
    header.extend_from_slice(&54u32.to_le_bytes());
    header.extend_from_slice(&40u32.to_le_bytes());
    header.extend_from_slice(&80i32.to_le_bytes());
    header.extend_from_slice(&80i32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&24u16.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&(80u32 * 80 * 3).to_le_bytes());
    header.extend_from_slice(&2835i32.to_le_bytes());
    header.extend_from_slice(&2835i32.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header
}

#[test]
fn a_cleared_mini_key_is_an_80_by_80_black_24_bit_bmp() {
    for number in 1..=6 {
        let bmp =
            image_output::prepare_clear_jpeg_for(&MINI, ArtworkTarget::Keypad(number)).unwrap();
        assert_eq!(bmp.len(), KEY_BMP_LEN);
        assert_eq!(bmp[..54], key_bmp_header());
        assert!(bmp[54..].iter().all(|byte| *byte == 0));
    }
    assert!(image_output::prepare_clear_jpeg_for(&MINI, ArtworkTarget::Keypad(7)).is_err());
}

/// Source pixel `(x, y)` of the coordinate picture: red 3x, green 3y, blue 0x80.
fn coordinate_pixel(x: usize, y: usize) -> [u8; 3] {
    [(3 * x) as u8, (3 * y) as u8, 0x80]
}

#[test]
fn mini_key_pictures_are_transposed_and_stored_bottom_up_in_bgr() {
    let scratch = Scratch::new("coordinates");
    let source = Pixbuf::new(Colorspace::Rgb, false, 8, 80, 80).unwrap();
    for y in 0..80 {
        for x in 0..80 {
            let [red, green, blue] = coordinate_pixel(x, y);
            source.put_pixel(x as u32, y as u32, red, green, blue, 255);
        }
    }
    let path = scratch.0.join("coordinates.png");
    source.savev(&path, "png", &[]).unwrap();
    let bmp = image_output::prepare_jpeg_for(&MINI, ArtworkTarget::Keypad(1), &path).unwrap();
    assert_eq!(bmp.len(), KEY_BMP_LEN);
    assert_eq!(bmp[..54], key_bmp_header());
    // Transposed (as the open-source drivers send it), the picture shows
    // source (y', x') at (x', y'). BMP rows run bottom-up, so file row r is
    // picture row 79 - r: file pixel (column c, row r) is source (79 - r, c).
    for row in 0..80 {
        for column in 0..80 {
            let at = 54 + row * 240 + column * 3;
            let [red, green, blue] = coordinate_pixel(79 - row, column);
            assert_eq!(
                bmp[at..at + 3],
                [blue, green, red],
                "file row {row} column {column}"
            );
        }
    }
    // The source's top-left pixel stays top-left: the first pixel of the last
    // file row. Its bottom-left one ends up top-right: the last pixel.
    let first_of_top = 54 + 79 * 240;
    assert_eq!(bmp[first_of_top..first_of_top + 3], [0x80, 0, 0]);
    let last = 54 + 79 * 240 + 79 * 3;
    assert_eq!(bmp[last..last + 3], [0x80, 3 * 79, 0]);
}

#[test]
fn every_mini_key_picture_path_produces_a_bmp_of_the_key_size() {
    let text = ButtonText {
        text: "Mini".into(),
        ..ButtonText::default()
    };
    let pictures = [
        (
            "button text",
            image_output::prepare_button_jpeg_for(&MINI, None, &text),
        ),
        (
            "binding icon",
            image_output::prepare_icon_jpeg_for(
                &MINI,
                ArtworkTarget::Keypad(2),
                BindingIcon::Timer,
                "Timer",
                None,
                None,
                (false, false),
            ),
        ),
        (
            "live information",
            image_output::prepare_info_jpeg_for(
                &MINI,
                ArtworkTarget::Keypad(3),
                &InfoSource::Clock.placeholder(),
                (None, None),
                Duration::ZERO,
            ),
        ),
    ];
    for (name, picture) in pictures {
        let bmp = picture.unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(bmp.len(), KEY_BMP_LEN, "{name}");
        assert_eq!(bmp[..54], key_bmp_header(), "{name}");
        assert!(
            bmp[54..].iter().any(|byte| *byte > 200),
            "{name} drew nothing"
        );
    }
}

#[test]
fn a_mini_key_bmp_is_split_into_twenty_reports_that_reassemble_it() {
    let bmp = image_output::prepare_clear_jpeg_for(&MINI, ArtworkTarget::Keypad(4)).unwrap();
    let reports = image_output::encode_reports_for(&MINI, ArtworkTarget::Keypad(4), &bmp).unwrap();
    // 19254 bytes in chunks of 1008: nineteen full ones and 102 bytes.
    assert_eq!(reports.len(), 20);
    let mut joined = Vec::new();
    for (chunk, report) in reports.iter().enumerate() {
        assert_eq!(report.0[..16], header(chunk as u8, chunk == 19, 4));
        joined.extend_from_slice(&report.0[16..]);
    }
    assert_eq!(joined[..KEY_BMP_LEN], bmp);
    assert!(joined[KEY_BMP_LEN..].iter().all(|byte| *byte == 0));
}
