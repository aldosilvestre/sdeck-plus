//! What each key of a Stream Deck Classic shows of its page's background,
//! and the facts of the registry entry behind it. Elgato's Classic page draws
//! the 15 keys as 72 px squares on the 480 x 272 LCD ("Unit Pixel Layout"):
//! 11 px from the left edge, 5 px from the top, 25 px apart. A key without a
//! picture of its own shows the part of the background under it, with its
//! icon, text or live information drawn over that part.
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use sdeck_plus::{
    button_text::ButtonText,
    device_model::{DeviceModel, KeypadBackground, Rotation, CLASSIC, PLUS},
    image_output,
    local_artwork::{ArtworkStore, ArtworkTarget, KEYPAD_BACKGROUND_FILE},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "classic-crop-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The color key `key` gets in [`background`].
fn color(key: u8) -> [u8; 3] {
    [key * 16, 255 - key * 16, 96]
}

/// A 480 x 272 grey background with each key's square in its own color.
fn background(scratch: &Scratch) -> PathBuf {
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, 480, 272).unwrap();
    image.fill(0x808080ff);
    for key in 1..=15u8 {
        let (column, row) = (u32::from((key - 1) % 5), u32::from((key - 1) / 5));
        let [red, green, blue] = color(key);
        for y in 0..72 {
            for x in 0..72 {
                image.put_pixel(
                    11 + column * 97 + x,
                    5 + row * 97 + y,
                    red,
                    green,
                    blue,
                    255,
                );
            }
        }
    }
    let path = scratch.0.join("background.png");
    image.savev(&path, "png", &[]).unwrap();
    path
}

fn pixel(image: &Pixbuf, x: i32, y: i32) -> [u8; 3] {
    let bytes = image.read_pixel_bytes();
    let at = y as usize * image.rowstride() as usize + x as usize * image.n_channels() as usize;
    [bytes[at], bytes[at + 1], bytes[at + 2]]
}

fn near(actual: [u8; 3], expected: [u8; 3], tolerance: u8) -> bool {
    actual
        .iter()
        .zip(expected)
        .all(|(a, e)| a.abs_diff(e) <= tolerance)
}

#[test]
fn the_classic_registry_entry_follows_elgatos_full_lcd_image() {
    assert_eq!(
        CLASSIC.keypad_background,
        Some(KeypadBackground {
            px: (480, 272),
            rotation: Rotation::Rot180,
            first_key: (11, 5),
            pitch: (97, 97),
        })
    );
    assert!(CLASSIC.has_keypad_background());
    assert_eq!(CLASSIC.key_on_background(1), Some((11, 5, 72, 72)));
    assert_eq!(CLASSIC.key_on_background(5), Some((399, 5, 72, 72)));
    assert_eq!(CLASSIC.key_on_background(11), Some((11, 199, 72, 72)));
    assert_eq!(CLASSIC.key_on_background(15), Some((399, 199, 72, 72)));
    assert_eq!(CLASSIC.key_on_background(0), None);
    assert_eq!(CLASSIC.key_on_background(16), None);
    // Every key lies on the LCD.
    for key in CLASSIC.key_numbers() {
        let (x, y, width, height) = CLASSIC.key_on_background(key).unwrap();
        assert!(x + width <= 480 && y + height <= 272, "key {key}");
    }
    // The Classic is the only model that offers one (S6).
    for model in DeviceModel::all()
        .iter()
        .filter(|model| model.id != "classic")
    {
        assert_eq!(model.keypad_background, None, "{}", model.id);
        assert!(!model.has_keypad_background());
        assert_eq!(model.key_on_background(1), None);
    }
}

#[test]
fn each_key_shows_its_own_share_of_the_background() {
    let scratch = Scratch::new("share");
    let source = background(&scratch);
    for key in CLASSIC.key_numbers() {
        let share = image_output::keypad_background_crop(&CLASSIC, &source, key).unwrap();
        assert!(image_output::is_keypad_background_crop(&share));
        // A key picture composed from it, as the preview draws it.
        let image =
            image_output::compose_button_image_for(&CLASSIC, Some(&share), &ButtonText::default())
                .unwrap();
        assert_eq!((image.width(), image.height()), (72, 72));
        for (x, y) in [(1, 1), (36, 36), (70, 70)] {
            assert_eq!(pixel(&image, x, y), color(key), "key {key} at {x},{y}");
        }
        let decoded = image_output::decode_picture(&share).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (72, 72));
        assert_eq!(pixel(&decoded, 36, 36), color(key));
        // And as the device gets it: a 72 px JPEG of the same part.
        let jpeg =
            image_output::prepare_jpeg_for(&CLASSIC, ArtworkTarget::Keypad(key), &share).unwrap();
        let sent = Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
        assert_eq!((sent.width(), sent.height()), (72, 72));
        assert!(near(pixel(&sent, 36, 36), color(key), 12), "key {key}");
    }
    assert!(!image_output::is_keypad_background_crop(&source));
    assert_eq!(
        image_output::keypad_background_crop(&CLASSIC, &source, 0),
        None
    );
    assert_eq!(
        image_output::keypad_background_crop(&CLASSIC, &source, 16),
        None
    );
    assert_eq!(
        image_output::keypad_background_crop(&PLUS, &source, 1),
        None
    );
}

/// A key's share of a background whose file is gone is an error, never a
/// panic and never another picture.
#[test]
fn a_share_of_a_missing_background_is_an_error() {
    let scratch = Scratch::new("missing");
    let source = background(&scratch);
    let share = image_output::keypad_background_crop(&CLASSIC, &source, 3).unwrap();
    fs::remove_file(&source).unwrap();
    assert!(image_output::decode_picture(&share).is_err());
    assert!(image_output::prepare_jpeg_for(&CLASSIC, ArtworkTarget::Keypad(3), &share).is_err());
    assert!(
        image_output::compose_button_image_for(&CLASSIC, Some(&share), &ButtonText::default())
            .is_err()
    );
}

/// The bytes of a removed background become an owned asset again (the
/// toast's Undo): a picture that does not decode changes nothing.
#[test]
fn a_removed_background_comes_back_from_its_bytes() {
    let scratch = Scratch::new("undo");
    let root = scratch.0.join("data/devices/classic");
    let source = background(&scratch);
    let mut store = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC).unwrap();
    let target = ArtworkTarget::TouchStripBackground;
    let first = store.assign_in_layer("Base", target, &source).unwrap();
    let bytes = fs::read(&first).unwrap();
    store.clear_in_layer("Base", target).unwrap();
    assert!(!first.exists());
    assert!(!root.join(KEYPAD_BACKGROUND_FILE).exists());

    assert!(store
        .assign_bytes_in_layer("Base", target, b"not a picture", "png")
        .is_err());
    assert_eq!(store.assigned_path_in_layer("Base", target), None);
    assert!(!root.join(KEYPAD_BACKGROUND_FILE).exists());
    assert_eq!(fs::read_dir(root.join("assets")).unwrap().count(), 0);

    let restored = store
        .assign_bytes_in_layer("Base", target, &bytes, "png")
        .unwrap();
    assert_eq!(fs::read(&restored).unwrap(), bytes);
    assert_eq!(restored.extension().unwrap(), "png");
    let reopened = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC).unwrap();
    assert_eq!(
        reopened.assigned_path_in_layer("Base", target),
        Some(restored)
    );
    // An extension that is no plain word is not trusted.
    let odd = store
        .assign_bytes_in_layer("Page", target, &bytes, "../x")
        .unwrap();
    assert_eq!(odd.extension().unwrap(), "img");
}
