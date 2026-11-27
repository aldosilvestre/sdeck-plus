//! T6e (L30): a keypad background keeps the Stream Deck Classic's preview the
//! size of the device. A picture of any size or shape (a wide 1920x1080 photo,
//! a tall 600x1200 one, or the LCD's own 480x272) fills only the Background
//! bar's 42x24 thumbnail and each key's share: the bar, the keys, the chassis,
//! the card and the window keep the geometry they have without a background.
//! The virtual Classic, shown through the real hub while the menu's switch is
//! on, and the real Classic (simulated) share the preview, so both are
//! checked; the models without a keypad background keep theirs.
//!
//! Opt-in captures: with `STREAMDECK_VIRTUAL_QA_DIR` set, the window content
//! is written before and after each background as `<model id>-<picture>.png`.
use super::model_session_tests::{header_label, wait_until};
use super::preview_model_tests::{bounds_in, chassis_of, class_of, model_fixture};
use super::two_device_tests::{start_pair_with, Pair};
use super::virtual_deck_ui_tests::{free_ports, remember, turn_on_and_wait};
use super::*;
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use sdeck_plus::{
    device_model::{DeviceModel, CLASSIC, VIRTUAL_CLASSIC},
    event_api::test_support::SimDevice,
    image_output,
    preview_layout::{ControlRef, PreviewLayout},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const BACKGROUND: ArtworkTarget = ArtworkTarget::TouchStripBackground;

/// The bar's thumbnail: the LCD's 480:272 shape at the bar's height.
const THUMBNAIL: (i32, i32) = (42, 24);

const RED: u32 = 0xff0000ff;
const BLUE: u32 = 0x0000ffff;
const GREEN: u32 = 0x00ff00ff;
const YELLOW: u32 = 0xffff00ff;
const MAGENTA: u32 = 0xff00ffff;
const CYAN: u32 = 0x00ffffff;

/// A background picture of two halves: left and right when it is at least as
/// wide as tall, top and bottom otherwise. `first` shows on one key, `second`
/// on another, after the cover crop to 480x272.
struct Sample {
    name: &'static str,
    size: (i32, i32),
    colors: (u32, u32),
    keys: (u8, u8),
}

const WIDE: Sample = Sample {
    name: "wide-1920x1080",
    size: (1920, 1080),
    colors: (RED, BLUE),
    keys: (1, 5),
};
const TALL: Sample = Sample {
    name: "tall-600x1200",
    size: (600, 1200),
    colors: (GREEN, YELLOW),
    keys: (1, 11),
};
const LCD: Sample = Sample {
    name: "lcd-480x272",
    size: (480, 272),
    colors: (MAGENTA, CYAN),
    keys: (1, 15),
};

impl Sample {
    /// The picture, outside the stores.
    fn write(&self) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "virtual-background-preview-{}-{}-{}.png",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            self.name
        ));
        let (width, height) = self.size;
        let image = Pixbuf::new(Colorspace::Rgb, false, 8, width, height).unwrap();
        image.fill(self.colors.0);
        if width >= height {
            image
                .new_subpixbuf(width / 2, 0, width - width / 2, height)
                .fill(self.colors.1);
        } else {
            image
                .new_subpixbuf(0, height / 2, width, height - height / 2)
                .fill(self.colors.1);
        }
        image.savev(&path, "png", &[]).unwrap();
        path
    }
}

/// Removes the pictures made by [`Sample::write`] however the test ends.
struct Pictures(Vec<PathBuf>);

impl Drop for Pictures {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

type Bounds = (i32, i32, i32, i32);

/// Where the preview and the window are, as laid out on screen.
#[derive(Debug, PartialEq)]
struct Geometry {
    window: (i32, i32),
    /// The preview (its ScaledBin): allocated width and height, and the
    /// natural width it asks of the window.
    preview: (i32, i32, i32),
    /// The chassis inside the preview.
    chassis: Bounds,
    /// Every control of the model, on the chassis, in creation order.
    controls: Vec<(ControlRef, Bounds)>,
}

fn geometry(window: &ApplicationWindow, preview: &preview::PreviewView) -> Geometry {
    wait_for_frame(window);
    let bin: gtk::Widget = preview.widget().clone().upcast();
    let chassis = chassis_of(&bin);
    Geometry {
        window: (window.width(), window.height()),
        preview: (
            bin.width(),
            bin.height(),
            bin.measure(gtk::Orientation::Horizontal, -1).1,
        ),
        chassis: bounds_in(&chassis, &bin),
        controls: preview
            .layout()
            .controls()
            .into_iter()
            .map(|control| {
                let widget = preview
                    .control(control)
                    .unwrap_or_else(|| panic!("{control:?} is on the preview"));
                (control, bounds_in(&widget, &chassis))
            })
            .collect(),
    }
}

/// The chassis has the model's size and every control sits at its rectangle.
fn assert_at_layout(geometry: &Geometry, layout: &PreviewLayout, what: &str) {
    assert_eq!(
        (geometry.chassis.2, geometry.chassis.3),
        layout.size,
        "{what}: chassis size"
    );
    for (control, bounds) in &geometry.controls {
        let rect = layout.rect_of(*control).unwrap();
        assert_eq!(
            *bounds,
            (rect.x, rect.y, rect.w, rect.h),
            "{what}: {control:?}"
        );
    }
}

/// The preview picture registered for `target` on this preview's chassis.
fn picture_of(
    artwork: &ArtworkUi,
    preview: &preview::PreviewView,
    target: ArtworkTarget,
) -> Picture {
    let chassis = chassis_of(preview.widget().upcast_ref());
    artwork
        .pictures
        .borrow()
        .iter()
        .find(|(shown, picture)| *shown == target && picture.is_ancestor(&chassis))
        .map(|(_, picture)| picture.clone())
        .unwrap_or_else(|| panic!("{target:?} has a picture on the chassis"))
}

/// The colour at the centre of what `picture` shows.
fn centre_of(picture: &Picture) -> [u8; 3] {
    let texture = picture
        .paintable()
        .and_downcast::<gtk::gdk::Texture>()
        .expect("the picture shows a texture");
    let (width, height) = (texture.width(), texture.height());
    let mut bytes = vec![0; (width * height * 4) as usize];
    texture.download(&mut bytes, width as usize * 4);
    // GDK's default memory format: premultiplied BGRA, opaque here.
    let at = ((height / 2) * width + width / 2) as usize * 4;
    [bytes[at + 2], bytes[at + 1], bytes[at]]
}

fn centre_of_pixbuf(image: &Pixbuf) -> [u8; 3] {
    let at =
        (image.height() / 2 * image.rowstride() + image.width() / 2 * image.n_channels()) as usize;
    let bytes = image.read_pixel_bytes();
    [bytes[at], bytes[at + 1], bytes[at + 2]]
}

fn rgb(color: u32) -> [u8; 3] {
    [(color >> 24) as u8, (color >> 16) as u8, (color >> 8) as u8]
}

fn near(left: [u8; 3], right: [u8; 3]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| left.abs_diff(right) <= 8)
}

/// The window content, when captures were asked for.
fn capture(window: &ApplicationWindow, model: &DeviceModel, name: &str) {
    let Some(directory) = std::env::var_os("STREAMDECK_VIRTUAL_QA_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    wait_for_frame(window);
    rendered_texture(window, &window.child().unwrap())
        .save_to_png(directory.join(format!("{}-{name}.png", model.id)))
        .unwrap();
}

/// Choose each sample in turn as the shown page's background, as Appearance's
/// Image… does, and check that the preview keeps its geometry and shows the
/// picture only in the bar's thumbnail and each key's share.
fn check_backgrounds(
    window: &ApplicationWindow,
    preview: &preview::PreviewView,
    artwork: &ArtworkUi,
    samples: &[&Sample],
) {
    let model = preview.model();
    assert!(model.has_keypad_background(), "{}", model.id);
    let layout = PreviewLayout::for_model(model);
    let bar = preview
        .control(ControlRef::Background)
        .expect("the Background bar")
        .downcast::<Button>()
        .unwrap();
    // Selected as when its picture is chosen: Appearance points at it.
    bar.emit_clicked();
    assert_eq!(artwork.selected_target.get(), BACKGROUND);
    let without = geometry(window, preview);
    assert_at_layout(
        &without,
        &layout,
        &format!("{} without background", model.id),
    );
    assert!(!picture_of(artwork, preview, BACKGROUND).is_mapped());
    let bar_rect = layout.rect_of(ControlRef::Background).unwrap();
    // Without a picture the caption starts at the bar's 8 px padding.
    let caption = class_of(bar.upcast_ref(), "background-caption");
    let caption_x = || bounds_in(&caption, &chassis_of(preview.widget().upcast_ref())).0;
    assert_eq!(caption_x(), bar_rect.x + 8, "{}: caption", model.id);
    capture(window, model, "none");
    let pictures = Pictures(samples.iter().map(|sample| sample.write()).collect());
    for (sample, path) in samples.iter().zip(&pictures.0) {
        let what = format!("{} with the {} background", model.id, sample.name);
        artwork.import(&artwork.capture_owner(), path).unwrap();
        let asset = artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Base", BACKGROUND)
            .expect("the page has its background");
        let with = geometry(window, preview);
        capture(window, model, sample.name);
        // Nothing moves or grows: not the window, the card, the chassis nor
        // any control.
        assert_eq!(with, without, "{what}");
        // The bar shows the whole picture in its thumbnail, inside the bar.
        let thumbnail = picture_of(artwork, preview, BACKGROUND);
        assert!(thumbnail.is_mapped(), "{what}: the thumbnail shows");
        let chassis = chassis_of(preview.widget().upcast_ref());
        let (x, y, w, h) = bounds_in(&thumbnail, &chassis);
        assert_eq!((w, h), THUMBNAIL, "{what}: thumbnail size");
        assert!(
            x >= bar_rect.x
                && y >= bar_rect.y
                && x + w <= bar_rect.x + bar_rect.w
                && y + h <= bar_rect.y + bar_rect.h,
            "{what}: thumbnail {:?} inside the bar {bar_rect:?}",
            (x, y, w, h)
        );
        assert_eq!(caption_x(), x + w + 8, "{what}: caption beside it");
        // Each key shows its share of the picture, as the device does.
        for key in model.key_numbers() {
            let picture = picture_of(artwork, preview, ArtworkTarget::Keypad(key));
            assert!(picture.is_mapped(), "{what}: key {key} shows");
            let paintable = picture.paintable().unwrap();
            assert_eq!(
                (paintable.intrinsic_width(), paintable.intrinsic_height()),
                (72, 72),
                "{what}: key {key} shows its 72 px share"
            );
            let share = image_output::keypad_background_crop(model, &asset, key).unwrap();
            let expected = centre_of_pixbuf(&image_output::decode_picture(&share).unwrap());
            assert!(
                near(centre_of(&picture), expected),
                "{what}: key {key} shows {:?}, its share is {expected:?}",
                centre_of(&picture)
            );
        }
        let (first, second) = sample.keys;
        let key_colour = |key| centre_of(&picture_of(artwork, preview, ArtworkTarget::Keypad(key)));
        assert!(
            near(key_colour(first), rgb(sample.colors.0)),
            "{what}: key {first}"
        );
        assert!(
            near(key_colour(second), rgb(sample.colors.1)),
            "{what}: key {second}"
        );
    }
}

#[test]
fn a_background_of_any_size_keeps_the_classic_previews_the_size_of_the_device() {
    run_gtk_test(|| {
        for model in [&CLASSIC, &VIRTUAL_CLASSIC] {
            let Some(f) = model_fixture("background-geometry", model, |_, _| {}) else {
                return;
            };
            check_backgrounds(
                &f.window,
                &f.widgets.preview,
                &f.artwork,
                &[&WIDE, &TALL, &LCD],
            );
        }
    });
}

/// The shown session of `model` in `pair`'s window, once the window shows it.
fn shown_session(pair: &Pair, model: &'static DeviceModel) -> feature_runtime::FeatureRuntime {
    let registry = pair.registry.clone();
    wait_until(
        &format!("the window to show the {}", model.name),
        move || {
            registry.model().id == model.id
                && registry
                    .runtime_of(model)
                    .and_then(|runtime| runtime.preview())
                    .is_some_and(|preview| preview.widget().is_mapped())
        },
    );
    pair.runtime(model)
}

#[test]
fn the_virtual_classic_shown_over_the_real_hub_keeps_its_preview_with_a_background() {
    run_gtk_test(|| {
        let [port] = free_ports();
        let Some(pair) = start_pair_with("virtual-background-preview", remember(port)) else {
            return;
        };
        let _panel = turn_on_and_wait(&pair);
        let runtime = shown_session(&pair, &VIRTUAL_CLASSIC);
        // L33: the preview is titled "VIRTUAL DECK"; the header keeps the
        // model's name.
        assert_eq!(
            header_label(&pair.window, "hardware-brand").text(),
            "VIRTUAL DECK"
        );
        let window = pair.window.clone();
        wait_until("the header names the virtual Classic", move || {
            header_label(&window, "muted")
                .text()
                .starts_with("Virtual Stream Deck Classic — served at ")
        });
        let preview = runtime.preview().unwrap();
        check_backgrounds(
            &pair.window,
            &preview,
            runtime.artwork_ui(),
            &[&WIDE, &TALL],
        );
    });
}

#[test]
fn the_simulated_classic_over_the_real_hub_keeps_its_preview_with_a_background() {
    run_gtk_test(|| {
        let Some(pair) = start_pair_with("classic-background-preview", |_| {}) else {
            return;
        };
        let classic = SimDevice::new(&CLASSIC);
        pair.bus.plug("/dev/hidraw3", &classic);
        pair.wait_devices(1);
        let runtime = shown_session(&pair, &CLASSIC);
        let preview = runtime.preview().unwrap();
        check_backgrounds(
            &pair.window,
            &preview,
            runtime.artwork_ui(),
            &[&WIDE, &TALL],
        );
    });
}

/// L33: "tambien al preview podrias sacarle el titulo de 'VIRTUAL STREAM DECK
/// CLASSIC', con que diga 'VIRTUAL DECK' suficiente." The virtual Classic's
/// preview reads "VIRTUAL DECK"; every real model keeps its name in capitals,
/// and the title keeps its place.
#[test]
fn the_virtual_classics_preview_reads_virtual_deck_and_real_models_keep_their_names() {
    run_gtk_test(|| {
        for model in DeviceModel::connectable() {
            let Some(f) = model_fixture("preview-title", model, |_, _| {}) else {
                return;
            };
            let chassis = chassis_of(f.widgets.preview.widget().upcast_ref());
            let brand = class_of(&chassis, "hardware-brand")
                .downcast::<Label>()
                .unwrap();
            let expected = if model.is_virtual() {
                "VIRTUAL DECK".to_owned()
            } else {
                model.name.to_uppercase()
            };
            assert_eq!(brand.text(), expected, "{}", model.id);
            let rect = PreviewLayout::for_model(model).brand_rect;
            assert_eq!(
                bounds_in(&brand, &chassis),
                (rect.x, rect.y, rect.w, rect.h),
                "{}",
                model.id
            );
        }
    });
}

/// Risk item 3: the preview of every model keeps its chassis and controls at
/// the model's layout.
#[test]
fn every_model_keeps_its_preview_at_its_layout() {
    run_gtk_test(|| {
        for model in DeviceModel::connectable() {
            let Some(f) = model_fixture("layout-geometry", model, |_, _| {}) else {
                return;
            };
            let geometry = geometry(&f.window, &f.widgets.preview);
            assert_at_layout(&geometry, &PreviewLayout::for_model(model), model.id);
        }
    });
}
