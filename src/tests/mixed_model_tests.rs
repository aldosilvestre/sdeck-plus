//! The real HID hub over simulated Stream Decks connected together (a + with a
//! Classic and an XL, or a + with a Neo and a Mini), feeding the real window:
//! each model runs its own session and configuration, the 'Current' selector
//! lists every connected model, unplugging one keeps the others running, and
//! shutdown blanks all of them. The models without dials or a touch strip get
//! key pictures only (and the Neo its window's), and nothing they lack is
//! reported as failing. No hardware, HOME, network or HID stack is involved;
//! the window is built over scratch directories.
use super::model_sections_tests::help_texts;
use super::model_session_tests::{header_label, live_threads, wait_until};
use super::two_device_tests::{
    active, decode, reset, start_pair_with, tap_key, wait_device, wait_layer, Pair, PLUS_PATH,
};
use super::*;
use feature_runtime::{SessionRoots, SwitchReason};
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use sdeck_plus::{
    device_model::{DeviceModel, CLASSIC, MINI, NEO, PLUS, XL},
    event_api::test_support::SimDevice,
    image_output,
};
use std::time::Instant;

const CLASSIC_PATH: &str = "/dev/hidraw3";
const XL_PATH: &str = "/dev/hidraw4";
const NEO_PATH: &str = "/dev/hidraw5";
const MINI_PATH: &str = "/dev/hidraw6";

/// The Neo's information window, a page's picture like a + strip's.
const WINDOW: ArtworkTarget = ArtworkTarget::TouchStripBackground;

/// Layers A and B on the Classic and the XL. Their first key leads to a
/// different layer on each model (+: A, Classic: B, XL: A) and their last key
/// to the other one, so a stray event shows which model ran it.
fn seed_keys_only(roots: &SessionRoots) {
    for (model, first, last) in [(&CLASSIC, "B", "A"), (&XL, "A", "B")] {
        let root = roots.root_for(model);
        std::fs::create_dir_all(&root).unwrap();
        let mut store = LayerStore::open_at_model(&root, model).unwrap();
        for name in ["A", "B"] {
            store.create(name).unwrap();
        }
        store
            .bind(
                "Base",
                Trigger::KeyPress(1),
                Action::SwitchLayer(first.into()),
            )
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::KeyPress(model.keys()),
                Action::SwitchLayer(last.into()),
            )
            .unwrap();
        store.persist().unwrap();
    }
}

/// Layers A and B on the Neo and the Mini, each input leading to a different
/// layer per model (+ key 1: A; Neo key 1: B, touch button 2: A; Mini key 1:
/// B, key 6: A), and a red picture in the Neo's window on the Base page.
fn seed_neo_and_mini(roots: &SessionRoots) {
    for (model, bindings) in [
        (
            &NEO,
            [(Trigger::KeyPress(1), "B"), (Trigger::TouchButton(2), "A")],
        ),
        (
            &MINI,
            [(Trigger::KeyPress(1), "B"), (Trigger::KeyPress(6), "A")],
        ),
    ] {
        let root = roots.root_for(model);
        std::fs::create_dir_all(&root).unwrap();
        let mut store = LayerStore::open_at_model(&root, model).unwrap();
        for name in ["A", "B"] {
            store.create(name).unwrap();
        }
        for (trigger, layer) in bindings {
            store
                .bind("Base", trigger, Action::SwitchLayer(layer.into()))
                .unwrap();
        }
        store.persist().unwrap();
    }
    let root = roots.root_for(&NEO);
    let source = root.join("window-source.png");
    let (width, height) = NEO.strip_px();
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, width as i32, height as i32).unwrap();
    image.fill(0xff0000ff);
    image.savev(&source, "png", &[]).unwrap();
    let mut artwork =
        sdeck_plus::local_artwork::ArtworkStore::open_at_for_layer_model(&root, Some("Base"), &NEO)
            .unwrap();
    artwork.assign_in_layer("Base", WINDOW, &source).unwrap();
    std::fs::remove_file(&source).unwrap();
}

/// Whether the pixel at `(x, y)` is clearly red.
fn is_red(image: &Pixbuf, x: i32, y: i32) -> bool {
    let at = (y * image.rowstride() + x * image.n_channels()) as usize;
    let pixels = image.read_pixel_bytes();
    let pixel = &pixels.as_ref()[at..at + 3];
    pixel[0] > 180 && pixel[1] < 90 && pixel[2] < 90
}

/// Whether the middle of `image` is black, as a blanked display is.
fn is_black(image: &Pixbuf) -> bool {
    let (x, y) = (image.width() / 2, image.height() / 2);
    let at = (y * image.rowstride() + x * image.n_channels()) as usize;
    let pixels = image.read_pixel_bytes();
    pixels.as_ref()[at..at + 3]
        .iter()
        .all(|channel| *channel < 30)
}

struct Three {
    plus: SimDevice,
    classic: SimDevice,
    xl: SimDevice,
}

/// A +, a Classic and an XL, plugged in and connected to the window.
fn plug_three(pair: &Pair) -> Three {
    let three = Three {
        plus: SimDevice::new(&PLUS),
        classic: SimDevice::new(&CLASSIC),
        xl: SimDevice::new(&XL),
    };
    pair.bus.plug(PLUS_PATH, &three.plus);
    pair.bus.plug(CLASSIC_PATH, &three.classic);
    pair.bus.plug(XL_PATH, &three.xl);
    pair.wait_devices(3);
    three
}

fn entries(pair: &Pair) -> Vec<String> {
    let names = &pair.registry.selector().names;
    (0..names.n_items())
        .filter_map(|index| names.string(index))
        .map(String::from)
        .collect()
}

fn detail(pair: &Pair) -> String {
    header_label(&pair.window, "muted").text().to_string()
}

/// The side panel's tab captions of the shown session that the user can see.
fn shown_tabs(pair: &Pair) -> Vec<String> {
    let switcher = super::preview_model_tests::class_of(pair.window.upcast_ref(), "sidebar-tabs");
    let mut tabs = Vec::new();
    let mut child = switcher.first_child();
    while let Some(button) = child {
        if button.is_visible() {
            if let Some(label) = descendant_label(&button) {
                tabs.push(label.text().to_string());
            }
        }
        child = button.next_sibling();
    }
    tabs
}

/// Every picture sent to `device` so far is a key picture of `model`'s size
/// and format, for one of its keys.
fn only_key_pictures(device: &SimDevice, model: &DeviceModel) {
    let images = device.images();
    assert!(!images.is_empty(), "{} received pictures", model.id);
    for image in images {
        assert_eq!(
            image.command,
            device.key_command(),
            "{}: key pictures only",
            model.id
        );
        assert!(
            image.slot < model.keys(),
            "{}: slot {}",
            model.id,
            image.slot
        );
        let picture = image.decode();
        assert_eq!(
            (picture.width(), picture.height()),
            (model.key_px as i32, model.key_px as i32),
            "{}",
            model.id
        );
    }
}

#[test]
fn a_plus_a_classic_and_an_xl_each_run_their_own_configuration() {
    run_gtk_test(|| {
        let Some(pair) = start_pair_with("mixed-sessions", seed_keys_only) else {
            return;
        };
        let three = plug_three(&pair);
        assert_eq!(pair.registry.session_count(), 3);
        for model in [&PLUS, &CLASSIC, &XL] {
            assert_eq!(pair.registry.live_devices(model), 1, "{}", model.id);
            assert_eq!(
                pair.runtime(model).layers().borrow().root(),
                pair.roots.root_for(model),
                "{} keeps its own data root",
                model.id
            );
        }
        // The selector lists every connected model in the registry's order.
        assert!(pair.registry.selector().root.is_visible());
        assert_eq!(
            entries(&pair),
            ["Stream Deck +", "Stream Deck Classic", "Stream Deck XL"]
        );
        assert_eq!(pair.registry.model().id, "plus", "the window keeps the +");
        // Each device gets the full picture of its own model, and the models
        // without a strip get no strip picture.
        wait_device("the + keys", &three.plus, |device| {
            device.last_key_jpeg(7).is_some()
        });
        wait_device("the Classic keys", &three.classic, |device| {
            device.last_key_jpeg(14).is_some()
        });
        wait_device("the XL keys", &three.xl, |device| {
            device.last_key_jpeg(31).is_some()
        });
        pair.settle();
        only_key_pictures(&three.classic, &CLASSIC);
        only_key_pictures(&three.xl, &XL);
        let plus_key = decode(three.plus.last_key_jpeg(0).unwrap());
        assert_eq!((plus_key.width(), plus_key.height()), (120, 120));
        // The header names the three, and no picture of theirs failed.
        assert_eq!(
            detail(&pair),
            "Stream Deck +, Stream Deck Classic and Stream Deck XL connected — keys, dials and touch strip are live"
        );
        // Input on each device runs only its own model's bindings.
        let runtimes = [
            pair.runtime(&PLUS),
            pair.runtime(&CLASSIC),
            pair.runtime(&XL),
        ];
        let devices = [&three.plus, &three.classic, &three.xl];
        let cases: [(usize, &DeviceModel, usize, &'static str); 4] = [
            (0, &PLUS, 0, "A"),
            (1, &CLASSIC, 0, "B"),
            (1, &CLASSIC, 14, "A"),
            (2, &XL, 31, "B"),
        ];
        for (at, model, key, layer) in cases {
            tap_key(devices[at], model, key);
            wait_layer(
                &format!("{} key {}", model.id, key + 1),
                &runtimes[at],
                layer,
            );
            pair.settle();
            for (other, runtime) in runtimes.iter().enumerate() {
                if other != at {
                    assert_eq!(
                        active(runtime),
                        "Base",
                        "{} key {} must not run another model's binding",
                        model.id,
                        key + 1
                    );
                }
            }
            reset(&runtimes[at]);
        }
        // Picking the Classic shows its session: keys only, no Dials tab.
        assert_eq!(shown_tabs(&pair), ["Actions", "Dials", "Binding"]);
        let index = entries(&pair)
            .iter()
            .position(|name| name == "Stream Deck Classic")
            .unwrap();
        pair.registry.selector().dropdown.set_selected(index as u32);
        let registry = pair.registry.clone();
        wait_until("the Classic to be shown", move || {
            registry.model() == &CLASSIC
        });
        assert_eq!(shown_tabs(&pair), ["Actions", "Binding"]);
        assert_eq!(
            header_label(&pair.window, "hardware-brand").text(),
            "STREAM DECK CLASSIC"
        );
        // The window's one setup help follows the shown model.
        let help = || help_texts(&pair.registry.menu().help_for_test());
        assert!(help()
            .iter()
            .any(|text| text == "Click a key or the background on the preview."));
        assert!(!help().iter().any(|text| text == "Touch strip pages"));
        pair.registry.show(&XL, SwitchReason::Selector);
        assert_eq!(shown_tabs(&pair), ["Actions", "Binding"]);
        pair.registry.show(&PLUS, SwitchReason::Selector);
        assert_eq!(shown_tabs(&pair), ["Actions", "Dials", "Binding"]);
        assert!(help().iter().any(|text| text == "Touch strip pages"));
        assert!(help()
            .iter()
            .any(|text| text.starts_with("Click a key, dial or the touch strip")));
    });
}

#[test]
fn unplugging_the_classic_keeps_the_plus_and_the_xl_running() {
    run_gtk_test(|| {
        let Some(pair) = start_pair_with("mixed-unplug", seed_keys_only) else {
            return;
        };
        let three = plug_three(&pair);
        let (plus, classic, xl) = (
            pair.runtime(&PLUS),
            pair.runtime(&CLASSIC),
            pair.runtime(&XL),
        );
        pair.bus.unplug(CLASSIC_PATH);
        pair.wait_devices(2);
        assert_eq!(pair.registry.live_devices(&CLASSIC), 0);
        assert_eq!(pair.registry.session_count(), 3, "its session is kept");
        assert_eq!(entries(&pair), ["Stream Deck +", "Stream Deck XL"]);
        assert!(pair.registry.selector().root.is_visible());
        assert_eq!(
            detail(&pair),
            "Stream Deck + and Stream Deck XL connected — keys, dials and touch strip are live"
        );
        for (device, model, runtime, layer) in
            [(&three.plus, &PLUS, &plus, "A"), (&three.xl, &XL, &xl, "A")]
        {
            tap_key(device, model, 0);
            wait_layer(&format!("the {} still works", model.id), runtime, layer);
            reset(runtime);
        }
        assert_eq!(active(&classic), "Base");
        // The shown Classic goes too while another model remains: that one is shown.
        let again = SimDevice::new(&CLASSIC);
        pair.bus.plug(CLASSIC_PATH, &again);
        pair.wait_devices(3);
        pair.registry.show(&CLASSIC, SwitchReason::Selector);
        tap_key(&again, &CLASSIC, 0);
        wait_layer("the replugged Classic works", &classic, "B");
        reset(&classic);
        pair.bus.unplug(CLASSIC_PATH);
        pair.wait_devices(2);
        let registry = pair.registry.clone();
        wait_until("a connected model to be shown", move || {
            registry.model() != &CLASSIC
        });
        // The XL leaves: the + alone is shown and the selector goes away.
        pair.bus.unplug(XL_PATH);
        pair.wait_devices(1);
        assert_eq!(pair.registry.model().id, "plus");
        assert!(!pair.registry.selector().root.is_visible());
        assert_eq!(
            detail(&pair),
            "Stream Deck + connected — keys, dials and touch strip are live"
        );
        tap_key(&three.plus, &PLUS, 0);
        wait_layer("the + alone still works", &plus, "A");
    });
}

#[test]
fn a_lone_classic_or_xl_is_shown_with_its_own_status() {
    run_gtk_test(|| {
        for (model, path, name) in [
            (&CLASSIC, CLASSIC_PATH, "Stream Deck Classic"),
            (&XL, XL_PATH, "Stream Deck XL"),
        ] {
            let Some(pair) = start_pair_with("mixed-lone", seed_keys_only) else {
                return;
            };
            let device = SimDevice::new(model);
            pair.bus.plug(path, &device);
            pair.wait_devices(1);
            let registry = pair.registry.clone();
            wait_until(name, move || registry.model() == model);
            wait_device(name, &device, |device| {
                device.last_key_jpeg(model.keys() - 1).is_some()
            });
            pair.settle();
            only_key_pictures(&device, model);
            assert!(!pair.registry.selector().root.is_visible());
            assert_eq!(detail(&pair), format!("{name} connected — keys are live"));
            assert_eq!(header_label(&pair.window, "status").text(), "Connected");
            assert_eq!(shown_tabs(&pair), ["Actions", "Binding"]);
            tap_key(&device, model, usize::from(model.keys()) - 1);
            wait_layer(
                name,
                &pair.runtime(model),
                if model == &CLASSIC { "A" } else { "B" },
            );
        }
    });
}

#[test]
fn shutdown_blanks_the_plus_the_classic_and_the_xl() {
    run_gtk_test(|| {
        let (hubs, workers) = (live_threads("streamdeck-hid"), live_threads("layer-worker"));
        let Some(pair) = start_pair_with("mixed-shutdown", seed_keys_only) else {
            return;
        };
        let three = plug_three(&pair);
        for (device, model) in [
            (&three.plus, &PLUS),
            (&three.classic, &CLASSIC),
            (&three.xl, &XL),
        ] {
            wait_device(model.id, device, |device| {
                device.last_key_jpeg(model.keys() - 1).is_some()
            });
        }
        let started = Instant::now();
        pair.app.emit_by_name::<()>("shutdown", &[]);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "shutdown took {:?}",
            started.elapsed()
        );
        for (device, model) in [
            (&three.plus, &PLUS),
            (&three.classic, &CLASSIC),
            (&three.xl, &XL),
        ] {
            assert_eq!(
                device.features().last().unwrap()[..3],
                [0x03, 0x08, 0],
                "{}: the display is turned off",
                model.id
            );
            // Every upload, the blanking ones included, was complete.
            device.images();
        }
        assert!(live_threads("layer-worker") <= workers);
        assert!(live_threads("streamdeck-hid") <= hubs);
    });
}

/// A +, a Neo and a Mini, plugged in and connected to the window.
fn plug_plus_neo_and_mini(pair: &Pair) -> (SimDevice, SimDevice, SimDevice) {
    let (plus, neo, mini) = (
        SimDevice::new(&PLUS),
        SimDevice::new(&NEO),
        SimDevice::new(&MINI),
    );
    pair.bus.plug(PLUS_PATH, &plus);
    pair.bus.plug(NEO_PATH, &neo);
    pair.bus.plug(MINI_PATH, &mini);
    pair.wait_devices(3);
    (plus, neo, mini)
}

/// The newest finished picture of the Neo's window sent to `device`.
fn last_window(device: &SimDevice) -> Option<Pixbuf> {
    device
        .assemble()
        .0
        .into_iter()
        .rev()
        .find(|image| image.command == 0x0b)
        .map(|image| image.decode())
}

/// Whether the Neo `device` shows the red picture of its window.
fn shows_the_red_window(device: &SimDevice) -> bool {
    let (width, height) = NEO.strip_px();
    last_window(device).is_some_and(|window| is_red(&window, width as i32 / 2, height as i32 / 2))
}

/// Every picture sent to the Neo `device` so far is a key picture of its size,
/// for one of its keys, or a picture of its whole window.
fn only_neo_pictures(device: &SimDevice) {
    let images = device.images();
    assert!(images.iter().any(|image| image.command == 0x0b));
    for image in images {
        let picture = image.decode();
        let size = (picture.width() as u32, picture.height() as u32);
        match image.command {
            0x07 => {
                assert!(image.slot < NEO.keys(), "neo: slot {}", image.slot);
                assert_eq!(size, (NEO.key_px, NEO.key_px), "neo key");
            }
            0x0b => assert_eq!(size, NEO.strip_px(), "neo window"),
            other => panic!("neo: unexpected picture command {other:#04x}"),
        }
    }
}

#[test]
fn a_plus_a_neo_and_a_mini_each_run_their_own_configuration() {
    run_gtk_test(|| {
        let Some(pair) = start_pair_with("mixed-neo-mini", seed_neo_and_mini) else {
            return;
        };
        let (plus, neo, mini) = plug_plus_neo_and_mini(&pair);
        assert_eq!(pair.registry.session_count(), 3);
        for model in [&PLUS, &NEO, &MINI] {
            assert_eq!(pair.registry.live_devices(model), 1, "{}", model.id);
            assert_eq!(
                pair.runtime(model).layers().borrow().root(),
                pair.roots.root_for(model),
                "{} keeps its own data root",
                model.id
            );
        }
        // The selector lists every connected model in the registry's order.
        assert!(pair.registry.selector().root.is_visible());
        assert_eq!(
            entries(&pair),
            ["Stream Deck +", "Stream Deck Neo", "Stream Deck Mini"]
        );
        assert_eq!(pair.registry.model().id, "plus", "the window keeps the +");
        // Each device gets the pictures of its own model: the Neo JPEG keys
        // and its page's red window picture, the Mini BMP keys in its own
        // image reports.
        wait_device("the + keys", &plus, |device| {
            device.last_key_jpeg(7).is_some()
        });
        wait_device("the Neo keys", &neo, |device| {
            device.last_key_jpeg(7).is_some()
        });
        wait_device("the Neo window", &neo, shows_the_red_window);
        wait_device("the Mini keys", &mini, |device| {
            device.last_key_jpeg(5).is_some()
        });
        pair.settle();
        only_neo_pictures(&neo);
        only_key_pictures(&mini, &MINI);
        assert!(mini
            .writes()
            .iter()
            .all(|report| report[..2] == [0x02, 0x01]));
        assert_eq!(mini.last_key_jpeg(0).unwrap()[..2], *b"BM");
        assert_eq!(neo.last_key_jpeg(0).unwrap()[..2], [0xFF, 0xD8]);
        // The header names the three and every control any of them has.
        assert_eq!(
            detail(&pair),
            "Stream Deck +, Stream Deck Neo and Stream Deck Mini connected — keys, dials, touch buttons, touch strip and info window are live"
        );
        // Input on each device runs only its own model's bindings: the Neo's
        // second touch button follows its eight keys in the input report.
        let runtimes = [pair.runtime(&PLUS), pair.runtime(&NEO), pair.runtime(&MINI)];
        let devices = [&plus, &neo, &mini];
        let cases: [(usize, &DeviceModel, usize, &'static str); 5] = [
            (0, &PLUS, 0, "A"),
            (1, &NEO, 0, "B"),
            (1, &NEO, 9, "A"),
            (2, &MINI, 0, "B"),
            (2, &MINI, 5, "A"),
        ];
        for (at, model, index, layer) in cases {
            tap_key(devices[at], model, index);
            wait_layer(
                &format!("{} input {}", model.id, index + 1),
                &runtimes[at],
                layer,
            );
            pair.settle();
            for (other, runtime) in runtimes.iter().enumerate() {
                if other != at {
                    assert_eq!(
                        active(runtime),
                        "Base",
                        "{} input {} must not run another model's binding",
                        model.id,
                        index + 1
                    );
                }
            }
            reset(&runtimes[at]);
        }
        // Picking the Neo or the Mini shows its own session: no Dials tab, no
        // touch strip pages, and the setup help names its own controls.
        assert_eq!(shown_tabs(&pair), ["Actions", "Dials", "Binding"]);
        let help = || help_texts(&pair.registry.menu().help_for_test());
        for (model, brand, select) in [
            (
                &NEO,
                "STREAM DECK NEO",
                "Click a key, the info window or a touch button on the preview.",
            ),
            (&MINI, "STREAM DECK MINI", "Click a key on the preview."),
        ] {
            let index = entries(&pair)
                .iter()
                .position(|name| name == model.name)
                .unwrap();
            pair.registry.selector().dropdown.set_selected(index as u32);
            let registry = pair.registry.clone();
            wait_until(model.name, move || registry.model() == model);
            assert_eq!(shown_tabs(&pair), ["Actions", "Binding"], "{}", model.id);
            assert_eq!(header_label(&pair.window, "hardware-brand").text(), brand);
            assert!(help().iter().any(|text| text == select), "{}", model.id);
            assert!(
                !help().iter().any(|text| text == "Touch strip pages"),
                "{}",
                model.id
            );
        }
    });
}

#[test]
fn a_lone_neo_or_mini_is_shown_with_its_own_status() {
    run_gtk_test(|| {
        for (model, path, status) in [
            (
                &NEO,
                NEO_PATH,
                "Stream Deck Neo connected — keys, touch buttons and info window are live",
            ),
            (
                &MINI,
                MINI_PATH,
                "Stream Deck Mini connected — keys are live",
            ),
        ] {
            let Some(pair) = start_pair_with("mixed-lone-neo-mini", seed_neo_and_mini) else {
                return;
            };
            let device = SimDevice::new(model);
            pair.bus.plug(path, &device);
            pair.wait_devices(1);
            let registry = pair.registry.clone();
            wait_until(model.name, move || registry.model() == model);
            wait_device(model.name, &device, |device| {
                device.last_key_jpeg(model.keys() - 1).is_some()
            });
            pair.settle();
            assert!(!pair.registry.selector().root.is_visible());
            assert_eq!(detail(&pair), status);
            assert_eq!(header_label(&pair.window, "status").text(), "Connected");
            assert_eq!(shown_tabs(&pair), ["Actions", "Binding"]);
            tap_key(&device, model, 0);
            wait_layer(model.name, &pair.runtime(model), "B");
        }
    });
}

#[test]
fn shutdown_blanks_the_plus_the_neo_and_the_mini() {
    run_gtk_test(|| {
        let (hubs, workers) = (live_threads("streamdeck-hid"), live_threads("layer-worker"));
        let Some(pair) = start_pair_with("mixed-neo-mini-shutdown", seed_neo_and_mini) else {
            return;
        };
        let (plus, neo, mini) = plug_plus_neo_and_mini(&pair);
        for (device, model) in [(&plus, &PLUS), (&neo, &NEO), (&mini, &MINI)] {
            wait_device(model.id, device, |device| {
                device.last_key_jpeg(model.keys() - 1).is_some()
            });
        }
        wait_device("the Neo window", &neo, shows_the_red_window);
        let started = Instant::now();
        pair.app.emit_by_name::<()>("shutdown", &[]);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "shutdown took {:?}",
            started.elapsed()
        );
        // The Expanded models are turned off with `03 08 00`, the Mini with
        // its own `05 55 AA D1 01 00`; every key of each ends black.
        let expanded_off: &[u8] = &[0x03, 0x08, 0];
        let mini_off: &[u8] = &[0x05, 0x55, 0xAA, 0xD1, 0x01, 0];
        for (device, model, off) in [
            (&plus, &PLUS, expanded_off),
            (&neo, &NEO, expanded_off),
            (&mini, &MINI, mini_off),
        ] {
            let features = device.features();
            let last = features.last().unwrap();
            assert_eq!(
                &last[..off.len()],
                off,
                "{}: the display is turned off",
                model.id
            );
            // Every upload, the blanking ones included, was complete.
            device.images();
            for slot in 0..model.keys() {
                let key = device.last_key_jpeg(slot).unwrap();
                let key = Pixbuf::from_read(std::io::Cursor::new(key)).unwrap();
                assert!(is_black(&key), "{} key {} is blanked", model.id, slot + 1);
            }
        }
        assert!(
            is_black(&last_window(&neo).unwrap()),
            "the Neo's window is blanked"
        );
        let black = image_output::prepare_clear_jpeg_for(&MINI, ArtworkTarget::Keypad(1)).unwrap();
        for slot in 0..MINI.keys() {
            assert_eq!(
                mini.last_key_jpeg(slot).unwrap(),
                black,
                "the Mini's key {} is a black BMP",
                slot + 1
            );
        }
        assert!(live_threads("layer-worker") <= workers);
        assert!(live_threads("streamdeck-hid") <= hubs);
    });
}
