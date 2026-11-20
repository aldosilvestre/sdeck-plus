//! The real HID hub over a simulated Stream Deck +, Classic and XL connected
//! together, feeding the real window: each model runs its own session and
//! configuration, the 'Current' selector lists every connected model,
//! unplugging one keeps the others running, and shutdown blanks all of them.
//! The models without dials or a touch strip get key pictures only, and
//! nothing they lack is reported as failing. No hardware, HOME, network or HID
//! stack is involved; the window is built over scratch directories.
use super::model_sections_tests::help_texts;
use super::model_session_tests::{header_label, live_threads, wait_until};
use super::two_device_tests::{
    active, decode, reset, start_pair_with, tap_key, wait_device, wait_layer, Pair, PLUS_PATH,
};
use super::*;
use feature_runtime::{SessionRoots, SwitchReason};
use sdeck_plus::{
    device_model::{DeviceModel, CLASSIC, PLUS, XL},
    event_api::test_support::SimDevice,
};
use std::time::Instant;

const CLASSIC_PATH: &str = "/dev/hidraw3";
const XL_PATH: &str = "/dev/hidraw4";

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

/// Every picture sent to `device` so far is a key picture of `model`'s size,
/// for one of its keys.
fn only_key_pictures(device: &SimDevice, model: &DeviceModel) {
    let images = device.images();
    assert!(!images.is_empty(), "{} received pictures", model.id);
    for image in images {
        assert_eq!(image.command, 0x07, "{}: key pictures only", model.id);
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
            .any(|text| text == "Click a key on the preview."));
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
