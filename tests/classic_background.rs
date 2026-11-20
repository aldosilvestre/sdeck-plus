//! The Stream Deck Classic's keypad background (S6): one picture per keypad
//! page that covers the 480x272 LCD behind the 15 keys. It is sent as Elgato's
//! Classic page describes a "Full LCD Image" (480 x 272, rotated 180 degrees,
//! JPEG, Update Full Screen Image `02 08`), and stored beside artwork.conf in
//! a file of its own, so a reader that does not know it (this branch before
//! the background, or main) never meets a row it would refuse.
//!
//! Only the public library API is used; nothing reaches a device.
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use sdeck_plus::{
    config_bundle,
    device_model::{DeviceModel, CLASSIC, MINI, NEO, PLUS, PLUS_XL, XL},
    image_output,
    local_artwork::{ArtworkStore, ArtworkTarget},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const BACKGROUND: ArtworkTarget = ArtworkTarget::TouchStripBackground;
/// The file of the Classic's backgrounds, beside artwork.conf.
const BACKGROUND_FILE: &str = "keypad-background.conf";

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A scratch directory under the system temp dir, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "classic-background-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// A blue picture with a red 16 px square at its top-left corner.
    fn marker(&self, name: &str, (width, height): (i32, i32)) -> PathBuf {
        let image = Pixbuf::new(Colorspace::Rgb, false, 8, width, height).unwrap();
        image.fill(0x0000ffff);
        for y in 0..16 {
            for x in 0..16 {
                image.put_pixel(x, y, 255, 0, 0, 255);
            }
        }
        let path = self.path(name);
        image.savev(&path, "png", &[]).unwrap();
        path
    }

    /// A small picture of one color.
    fn plain(&self, name: &str, color: u32) -> PathBuf {
        let image = Pixbuf::new(Colorspace::Rgb, false, 8, 8, 8).unwrap();
        image.fill(color);
        let path = self.path(name);
        image.savev(&path, "png", &[]).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn pixel(image: &Pixbuf, x: i32, y: i32) -> [u8; 3] {
    let bytes = image.read_pixel_bytes();
    let at = y as usize * image.rowstride() as usize + x as usize * image.n_channels() as usize;
    [bytes[at], bytes[at + 1], bytes[at + 2]]
}

fn is_red(image: &Pixbuf, x: i32, y: i32) -> bool {
    let [red, green, blue] = pixel(image, x, y);
    red > 200 && green < 60 && blue < 60
}

fn classic_root(scratch: &Scratch) -> PathBuf {
    scratch.path("data/devices/classic")
}

/// Elgato's Classic page, "Full LCD Image": prepare a 480 x 272 picture,
/// rotate it 180 degrees, export it as JPEG.
#[test]
fn the_classic_background_is_a_480x272_jpeg_turned_half_way() {
    let scratch = Scratch::new("jpeg");
    let source = scratch.marker("background.png", (480, 272));
    assert_eq!(
        image_output::dimensions_for(&CLASSIC, BACKGROUND).map(|size| (size.width, size.height)),
        Ok((480, 272))
    );
    let jpeg = image_output::prepare_jpeg_for(&CLASSIC, BACKGROUND, &source).unwrap();
    assert_eq!(jpeg[..2], [0xFF, 0xD8], "JPEG");
    let decoded = Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (480, 272));
    // The source's top-left marker lands at the bottom right.
    assert!(is_red(&decoded, 471, 263), "marker not turned half way");
    assert!(!is_red(&decoded, 8, 8), "marker left at the top left");
}

/// Elgato's Classic page, "Update Full Screen Image": report id 0x02,
/// command 0x08, a reserved byte, the Transfer is Done flag, the UINT16 chunk
/// contents size and the UINT16 zero-based chunk index, then the data, in
/// 1024-byte reports padded with zeroes.
#[test]
fn full_screen_reports_carry_the_header_of_elgatos_classic_page() {
    let payload: Vec<u8> = (0..2500u32).map(|byte| (byte % 251) as u8 + 1).collect();
    let reports = image_output::encode_reports_for(&CLASSIC, BACKGROUND, &payload).unwrap();
    assert_eq!(reports.len(), 3, "1016 data bytes per report");
    for (index, report) in reports.iter().enumerate() {
        let report = &report.0;
        let start = index * 1016;
        let length = (payload.len() - start).min(1016);
        let last = index + 1 == reports.len();
        let mut header = [0x02, 0x08, 0x00, u8::from(last), 0, 0, 0, 0];
        header[4..6].copy_from_slice(&(length as u16).to_le_bytes());
        header[6..8].copy_from_slice(&(index as u16).to_le_bytes());
        assert_eq!(report.len(), 1024, "report {index}");
        assert_eq!(report[..8], header, "report {index}");
        assert_eq!(report[8..8 + length], payload[start..start + length]);
        assert!(report[8 + length..].iter().all(|byte| *byte == 0));
    }
    // Every other model keeps what it had: the Neo's window and the strips of
    // the + and + XL use 0x0B; the XL and the Mini have no such picture.
    for model in [&PLUS, &PLUS_XL, &NEO] {
        let reports = image_output::encode_reports_for(model, BACKGROUND, &[7]).unwrap();
        assert_eq!(
            reports[0].0[..8],
            [2, 0x0B, 0, 1, 1, 0, 0, 0],
            "{}",
            model.id
        );
    }
    for model in [&XL, &MINI] {
        assert_eq!(
            image_output::encode_reports_for(model, BACKGROUND, &[7]).unwrap_err(),
            "unsupported artwork target",
            "{}",
            model.id
        );
    }
}

#[test]
fn only_the_classic_offers_a_keypad_background() {
    assert_eq!(ArtworkTarget::touch_for(&CLASSIC), [BACKGROUND]);
    assert!(BACKGROUND.is_valid_for(&CLASSIC));
    assert_eq!(BACKGROUND.id_in(&CLASSIC), "touch-strip-background");
    // It belongs to the keypad page: the Classic has no touch pages to follow.
    assert!(!BACKGROUND.follows_touch_page(&CLASSIC));
    for model in [&XL, &MINI] {
        assert!(ArtworkTarget::touch_for(model).is_empty(), "{}", model.id);
        assert!(!BACKGROUND.is_valid_for(model), "{}", model.id);
    }
    // The + and + XL keep their strip and sections, the Neo its window.
    assert_eq!(ArtworkTarget::touch_for(&PLUS), ArtworkTarget::TOUCH);
    assert_eq!(ArtworkTarget::touch_for(&NEO), [BACKGROUND]);
    assert!(BACKGROUND.follows_touch_page(&PLUS));
    assert!(BACKGROUND.follows_touch_page(&PLUS_XL));
    assert!(!BACKGROUND.follows_touch_page(&NEO));
    let classic_targets = ArtworkTarget::all_for(&CLASSIC);
    assert_eq!(classic_targets.len(), 16);
    assert_eq!(classic_targets[15], BACKGROUND);
}

/// A page's background lives in keypad-background.conf; artwork.conf keeps
/// exactly the rows an older reader accepts for a Classic.
#[test]
fn the_background_is_stored_per_page_beside_artwork_conf() {
    let scratch = Scratch::new("store");
    let root = classic_root(&scratch);
    let key = scratch.plain("key.png", 0x00ff00ff);
    let first = scratch.marker("first.png", (480, 272));
    let second = scratch.marker("second.png", (480, 272));
    let mut store = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC).unwrap();
    store
        .assign_in_layer("Base", ArtworkTarget::Keypad(15), &key)
        .unwrap();
    let artwork = fs::read(root.join("artwork.conf")).unwrap();
    assert!(!root.join(BACKGROUND_FILE).exists());

    let base = store.assign_in_layer("Base", BACKGROUND, &first).unwrap();
    let page = store
        .assign_in_layer("Page 2", BACKGROUND, &second)
        .unwrap();
    assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), artwork);
    let name = |path: &Path| path.file_name().unwrap().to_str().unwrap().to_owned();
    assert!(name(&base).starts_with("touch-strip-background-"));
    assert_eq!(
        fs::read_to_string(root.join(BACKGROUND_FILE)).unwrap(),
        format!(
            "streamdeck-keypad-background-v1\n\
             42617365\ttouch-strip-background\t{}\n\
             506167652032\ttouch-strip-background\t{}\n",
            name(&base),
            name(&page)
        )
    );

    let reopened = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC).unwrap();
    assert!(reopened.diagnostics().is_empty());
    assert_eq!(
        reopened.assigned_path_in_layer("Base", BACKGROUND),
        Some(base.clone())
    );
    assert_eq!(
        reopened.assigned_path_in_layer("Page 2", BACKGROUND),
        Some(page.clone())
    );
    assert!(reopened
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(15))
        .is_some());

    // Deleting a page takes its background; the other page keeps its own.
    store.remove_layers(&["Page 2".to_owned()]).unwrap();
    assert!(!page.exists());
    assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), artwork);
    assert_eq!(
        fs::read_to_string(root.join(BACKGROUND_FILE)).unwrap(),
        format!(
            "streamdeck-keypad-background-v1\n42617365\ttouch-strip-background\t{}\n",
            name(&base)
        )
    );
    // Clearing the last background leaves the root as an older reader wrote it.
    store.clear_in_layer("Base", BACKGROUND).unwrap();
    assert!(!base.exists());
    assert!(!root.join(BACKGROUND_FILE).exists());
    assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), artwork);
    assert!(store
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(15))
        .is_some());
}

/// A Classic background row in artwork.conf is refused exactly like the
/// readers that do not know the background refuse it.
#[test]
fn artwork_conf_never_carries_a_classic_background_row() {
    let scratch = Scratch::new("conf-row");
    let root = classic_root(&scratch);
    fs::create_dir_all(root.join("assets")).unwrap();
    let asset = scratch.marker("data/devices/classic/assets/bg.png", (480, 272));
    assert!(asset.is_file());
    let text = "streamdeck-artwork-v3\n42617365\ttouch-strip-background\tbg.png\n";
    fs::write(root.join("artwork.conf"), text).unwrap();
    let Err(error) = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC) else {
        panic!("a Classic background row in artwork.conf must be refused");
    };
    assert_eq!(error.to_string(), "unknown artwork target");
    assert_eq!(fs::read_to_string(root.join("artwork.conf")).unwrap(), text);
}

#[test]
fn an_invalid_background_file_is_kept_and_disables_writes() {
    let scratch = Scratch::new("invalid");
    let root = classic_root(&scratch);
    let key = scratch.plain("key.png", 0x00ff00ff);
    ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC)
        .unwrap()
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &key)
        .unwrap();
    let artwork = fs::read(root.join("artwork.conf")).unwrap();
    for broken in [
        "something else\n",
        "streamdeck-keypad-background-v1\n42617365\ttouch-strip-background\tmissing.png\n",
        "streamdeck-keypad-background-v1\n42617365\tkeypad-1\tkey.png\n",
        "streamdeck-keypad-background-v1\nzz\ttouch-strip-background\tbg.png\n",
        "streamdeck-keypad-background-v1\n42617365\ttouch-strip-background\t../bg.png\n",
    ] {
        fs::write(root.join(BACKGROUND_FILE), broken).unwrap();
        assert!(
            ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC).is_err(),
            "{broken:?}"
        );
        let mut lenient = ArtworkStore::open_at_model(&root, &CLASSIC).unwrap();
        assert!(!lenient.diagnostics().is_empty(), "{broken:?}");
        assert!(lenient.assign_in_layer("Base", BACKGROUND, &key).is_err());
        assert!(lenient.clear_in_layer("Base", BACKGROUND).is_err());
        assert_eq!(
            fs::read_to_string(root.join(BACKGROUND_FILE)).unwrap(),
            broken
        );
        assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), artwork);
    }
}

/// Models without a keypad background never read or write the file.
#[test]
fn other_models_ignore_and_refuse_a_keypad_background() {
    let scratch = Scratch::new("others");
    let picture = scratch.marker("bg.png", (480, 272));
    for model in [&XL, &MINI] {
        let root = scratch.path(&format!("data/devices/{}", model.id));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(BACKGROUND_FILE), "anything\n").unwrap();
        let mut store = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), model).unwrap();
        assert!(store.diagnostics().is_empty(), "{}", model.id);
        assert!(store.assign_in_layer("Base", BACKGROUND, &picture).is_err());
        assert_eq!(
            fs::read_to_string(root.join(BACKGROUND_FILE)).unwrap(),
            "anything\n"
        );
    }
}

/// A Classic bundle carries the background file with a portable reference;
/// a Classic without a background exports what it always did.
#[test]
fn a_classic_bundle_carries_its_background() {
    let scratch = Scratch::new("bundle");
    let root = classic_root(&scratch);
    let picture = scratch.marker("bg.png", (480, 272));
    let plain = scratch.path("plain.sdeck-plus.json");
    {
        let mut store =
            ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC).unwrap();
        store
            .assign_in_layer("Base", ArtworkTarget::Keypad(2), &picture)
            .unwrap();
    }
    sdeck_plus::layers::LayerStore::open_at_model(&root, &CLASSIC)
        .unwrap()
        .persist()
        .unwrap();
    config_bundle::export_for(&root, &plain, &CLASSIC).unwrap();
    assert!(!fs::read_to_string(&plain)
        .unwrap()
        .contains(BACKGROUND_FILE));

    let asset = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC)
        .unwrap()
        .assign_in_layer("Base", BACKGROUND, &picture)
        .unwrap();
    let name = asset.file_name().unwrap().to_str().unwrap().to_owned();
    let bundle = scratch.path("classic.sdeck-plus.json");
    config_bundle::export_for(&root, &bundle, &CLASSIC).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&bundle).unwrap()).unwrap();
    let entry = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == BACKGROUND_FILE)
        .expect("the bundle carries the background file");
    assert_eq!(
        entry["data"],
        format!(
            "streamdeck-keypad-background-v1\n42617365\ttouch-strip-background\t./imagenes/{name}\n"
        )
    );

    config_bundle::import_for(&bundle, &root, &CLASSIC).unwrap();
    let store = ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &CLASSIC).unwrap();
    let restored = store
        .assigned_path_in_layer("Base", BACKGROUND)
        .expect("the background is back");
    assert_eq!(fs::read(restored).unwrap(), fs::read(&picture).unwrap());
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(root.join(BACKGROUND_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "installed configuration is private");
    }

    // A bundle of another model never brings the file into its root.
    let xl_root = scratch.path("data/devices/xl");
    let foreign = scratch.path("foreign.sdeck-plus.json");
    let mut json = json;
    json["model"] = "xl".into();
    fs::write(&foreign, serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(
        config_bundle::import_for(&foreign, &xl_root, &XL)
            .unwrap_err()
            .to_string(),
        "unsafe, duplicate or oversized bundle entry"
    );
    assert!(!xl_root.join(BACKGROUND_FILE).exists());
    // Import leaves the previous root as a sibling backup.
    for entry in fs::read_dir(root.parent().unwrap()).unwrap() {
        let path = entry.unwrap().path();
        if path != root {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// The registry entries keep their ids; only the Classic changes here.
#[test]
fn the_models_in_question_are_the_registry_entries() {
    assert_eq!(DeviceModel::by_id("classic"), Some(&CLASSIC));
    assert_eq!(DeviceModel::by_id("xl"), Some(&XL));
    assert_eq!(DeviceModel::by_id("neo"), Some(&NEO));
    assert_eq!(DeviceModel::by_id("mini"), Some(&MINI));
}
