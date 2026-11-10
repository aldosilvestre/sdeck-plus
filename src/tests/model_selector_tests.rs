//! The header's 'Current' selector: when it exists, what it lists, how the
//! shown model is chosen, and that a window with one model (or none) looks
//! exactly as it did before the selector existed. No real device, HOME or
//! network is involved.
use super::model_session_tests::{header_label, remember, start, wait_until};
use super::preview_model_tests::{model_fixture, ExplicitLook};
use super::two_device_tests::{start_pair, start_pair_with, Pair, PLUS_PATH, XL_PATH};
use super::*;
use feature_runtime::choose_shown;
use sdeck_plus::{
    device_model::{DeviceModel, PLUS, PLUS_XL},
    event_api::{test_support::SimDevice, ConnectionState, DeviceId},
};

const P: &DeviceModel = &PLUS;
const X: &DeviceModel = &PLUS_XL;
const STATUS_SUFFIX: &str = "connected — keys, dials and touch strip are live";

#[test]
fn choose_shown_follows_the_selection_rules_for_every_combination() {
    // (connected, last used, current, user chose) -> shown
    #[rustfmt::skip]
    let table: [(&[&DeviceModel], &DeviceModel, &DeviceModel, bool, &DeviceModel); 32] = [
        // Nothing connected: the window keeps what it shows.
        (&[], P, P, false, P), (&[], P, P, true, P), (&[], P, X, false, X), (&[], P, X, true, X),
        (&[], X, P, false, P), (&[], X, P, true, P), (&[], X, X, false, X), (&[], X, X, true, X),
        // Only a +: it is shown whatever was used or picked.
        (&[P], P, P, false, P), (&[P], P, P, true, P), (&[P], P, X, false, P), (&[P], P, X, true, P),
        (&[P], X, P, false, P), (&[P], X, P, true, P), (&[P], X, X, false, P), (&[P], X, X, true, P),
        // Only an XL.
        (&[X], P, P, false, X), (&[X], P, P, true, X), (&[X], P, X, false, X), (&[X], P, X, true, X),
        (&[X], X, P, false, X), (&[X], X, P, true, X), (&[X], X, X, false, X), (&[X], X, X, true, X),
        // Both: automatic shows the last used model, a pick sticks.
        (&[P, X], P, P, false, P), (&[P, X], P, P, true, P), (&[P, X], P, X, false, P), (&[P, X], P, X, true, X),
        (&[P, X], X, P, false, X), (&[P, X], X, P, true, P), (&[P, X], X, X, false, X), (&[P, X], X, X, true, X),
    ];
    for (connected, last, current, chose, expected) in table {
        assert_eq!(
            choose_shown(connected, last, current, chose).id,
            expected.id,
            "connected {:?}, last used {}, shown {}, picked {chose}",
            connected.iter().map(|model| model.id).collect::<Vec<_>>(),
            last.id,
            current.id
        );
    }
}

/// Every label text, tooltip and drop-down entry below `widget`.
fn texts(widget: &gtk::Widget, found: &mut Vec<String>) {
    if let Some(label) = widget.downcast_ref::<Label>() {
        found.push(label.text().to_string());
    }
    if let Some(tooltip) = widget.tooltip_text() {
        found.push(tooltip.to_string());
    }
    if let Some(dropdown) = widget.downcast_ref::<gtk::DropDown>() {
        if let Some(model) = dropdown.model() {
            for index in 0..model.n_items() {
                if let Some(entry) = model
                    .item(index)
                    .and_then(|item| item.downcast::<gtk::StringObject>().ok())
                {
                    found.push(entry.string().to_string());
                }
            }
        }
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        texts(&current, found);
        child = current.next_sibling();
    }
}

fn selector_visible(pair: &Pair) -> bool {
    pair.registry.selector().root.is_visible()
}

fn entries(pair: &Pair) -> Vec<String> {
    let names = &pair.registry.selector().names;
    (0..names.n_items())
        .filter_map(|index| names.string(index))
        .map(String::from)
        .collect()
}

fn wait_shown(pair: &Pair, model: &'static DeviceModel) {
    let registry = pair.registry.clone();
    wait_until(model.name, move || registry.model() == model);
}

fn select(pair: &Pair, model: &DeviceModel) {
    let index = entries(pair)
        .iter()
        .position(|name| name == model.name)
        .expect("the model is listed");
    pair.registry.selector().dropdown.set_selected(index as u32);
}

fn status_text(pair: &Pair) -> String {
    header_label(&pair.window, "muted").text().to_string()
}

fn remembered(pair: &Pair) -> String {
    std::fs::read_to_string(&pair.roots.preference).unwrap()
}

#[test]
fn there_is_no_selector_with_no_device_one_device_or_two_of_one_model() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-hidden") else {
            return;
        };
        assert!(!selector_visible(&pair), "no device");
        assert!(!pair.registry.selector().dropdown.is_focusable());
        let first = SimDevice::new(&PLUS);
        pair.bus.plug(PLUS_PATH, &first);
        pair.wait_devices(1);
        assert!(!selector_visible(&pair), "one + connected");
        assert_eq!(entries(&pair), ["Stream Deck +"]);
        let second = SimDevice::new(&PLUS);
        pair.bus.plug(XL_PATH, &second);
        pair.wait_devices(2);
        assert_eq!(pair.registry.session_count(), 1);
        assert!(!selector_visible(&pair), "two + connected");
        assert!(!pair.registry.selector().dropdown.is_focusable());
        assert_eq!(
            status_text(&pair),
            format!("Stream Deck + x2 {STATUS_SUFFIX}")
        );
    });
}

#[test]
fn a_lone_xl_shows_no_selector_and_the_original_status() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-xl") else {
            return;
        };
        pair.bus.plug(XL_PATH, &SimDevice::new(&PLUS_XL));
        pair.wait_devices(1);
        wait_shown(&pair, &PLUS_XL);
        assert!(!selector_visible(&pair));
        assert_eq!(
            status_text(&pair),
            format!("Stream Deck + XL {STATUS_SUFFIX}")
        );
        assert_eq!(header_label(&pair.window, "status").text(), "Connected");
    });
}

#[test]
fn the_selector_appears_with_two_models_and_says_current() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-two") else {
            return;
        };
        let (_plus, _xl) = pair.plug_both();
        assert!(selector_visible(&pair));
        assert_eq!(entries(&pair), ["Stream Deck +", "Stream Deck + XL"]);
        let selector = pair.registry.selector();
        assert!(selector.dropdown.is_focusable());
        assert_eq!(
            selector.dropdown.tooltip_text().as_deref(),
            Some("Choose the Stream Deck to configure")
        );
        let caption = selector.root.first_child().unwrap();
        assert_eq!(caption.downcast_ref::<Label>().unwrap().text(), "Current");
        let mut found = Vec::new();
        texts(pair.window.upcast_ref(), &mut found);
        assert!(found.iter().any(|text| text == "Current"));
        assert!(
            found.iter().all(|text| !text.contains("Preview")),
            "no 'Preview' anywhere: {found:?}"
        );
        assert_eq!(
            status_text(&pair),
            format!("Stream Deck + and Stream Deck + XL {STATUS_SUFFIX}")
        );
        assert_eq!(header_label(&pair.window, "status").text(), "Connected");
    });
}

#[test]
fn the_selector_is_reachable_and_operable_from_the_keyboard() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-keyboard") else {
            return;
        };
        let (_plus, _xl) = pair.plug_both();
        let dropdown = pair.registry.selector().dropdown.clone();
        assert!(dropdown.is_focusable() && dropdown.is_sensitive());
        assert!(dropdown.grab_focus(), "the drop-down takes keyboard focus");
        assert!(dropdown.has_focus() || dropdown.focus_child().is_some());
        // The stock drop-down opens its list on activation, as Enter does.
        let popover = dropdown
            .first_child()
            .and_then(|button| button.last_child())
            .and_then(|child| child.downcast::<gtk::Popover>().ok());
        dropdown.activate();
        assert!(popover.is_none_or(|popover| popover.is_visible()));
    });
}

#[test]
fn choosing_a_model_shows_its_session_remembers_it_and_keeps_the_other_alive() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-choose") else {
            return;
        };
        let (_plus, _xl) = pair.plug_both();
        let plus = pair.runtime(&PLUS);
        let xl = pair.runtime(&PLUS_XL);
        assert_eq!(pair.registry.model().id, "plus");
        // A draft in the + survives it being hidden; nothing asks.
        let ui = plus.binding_ui();
        ui.text_entry.set_text("echo draft");
        ui.action_picker.set_selected(1);
        assert!(ui.is_dirty());

        select(&pair, &PLUS_XL);
        assert_eq!(pair.registry.model().id, "plus-xl");
        assert_eq!(pair.registry.runtime().session_model().id, "plus-xl");
        assert_eq!(
            header_label(&pair.window, "hardware-brand").text(),
            "STREAM DECK + XL",
            "the XL's editor is the visible stack child"
        );
        assert_eq!(xl.session_model().id, "plus-xl");
        assert_eq!(remembered(&pair), "{\"model\":\"plus-xl\"}\n");
        assert!(gtk::Window::list_toplevels()
            .iter()
            .filter_map(|window| window.clone().downcast::<gtk::MessageDialog>().ok())
            .all(|dialog| !dialog.is_visible()));
        assert_eq!(pair.registry.session_count(), 2);
        assert_eq!(pair.registry.live_devices(&PLUS), 1);
        assert!(plus.binding_ui().is_dirty(), "the hidden + keeps its draft");
        assert!(!status_text(&pair).contains("Preview"));

        select(&pair, &PLUS);
        assert_eq!(pair.registry.model().id, "plus");
        assert_eq!(
            header_label(&pair.window, "hardware-brand").text(),
            "STREAM DECK +"
        );
        assert!(plus.binding_ui().is_dirty());
        assert_eq!(remembered(&pair), "{\"model\":\"plus\"}\n");
    });
}

#[test]
fn unplugging_the_shown_models_device_hides_the_selector_and_shows_the_other() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-unplug") else {
            return;
        };
        let (_plus, _xl) = pair.plug_both();
        assert_eq!(pair.registry.model().id, "plus");
        pair.bus.unplug(PLUS_PATH);
        pair.wait_devices(1);
        wait_shown(&pair, &PLUS_XL);
        assert!(!selector_visible(&pair));
        assert_eq!(
            status_text(&pair),
            format!("Stream Deck + XL {STATUS_SUFFIX}")
        );
        assert_eq!(remembered(&pair), "{\"model\":\"plus-xl\"}\n");
        // With nothing left the window keeps what it shows.
        pair.bus.unplug(XL_PATH);
        pair.wait_devices(0);
        assert_eq!(pair.registry.model().id, "plus-xl");
        assert!(!selector_visible(&pair));
        assert!(!status_text(&pair).contains("Preview"));
    });
}

#[test]
fn a_manual_pick_sticks_until_its_model_has_no_device_left() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-sticks") else {
            return;
        };
        let (_plus, _xl) = pair.plug_both();
        select(&pair, &PLUS_XL);
        // Duplicates and reconnects of the other model change nothing.
        pair.bus.plug("/dev/hidraw3", &SimDevice::new(&PLUS));
        pair.wait_devices(3);
        pair.settle();
        assert_eq!(pair.registry.model().id, "plus-xl");
        pair.bus.unplug(PLUS_PATH);
        pair.wait_devices(2);
        pair.settle();
        assert_eq!(pair.registry.model().id, "plus-xl");
        assert!(selector_visible(&pair));
        // The picked model's last device goes: the remaining model is shown.
        pair.bus.unplug(XL_PATH);
        pair.wait_devices(1);
        wait_shown(&pair, &PLUS);
        assert!(!selector_visible(&pair));
        // The pick is over: the XL coming back does not take the window.
        pair.bus.plug(XL_PATH, &SimDevice::new(&PLUS_XL));
        pair.wait_devices(2);
        pair.settle();
        assert_eq!(pair.registry.model().id, "plus");
    });
}

#[test]
fn a_last_used_model_that_connects_second_is_shown_when_it_connects() {
    run_gtk_test(|| {
        let Some(pair) = start_pair_with("selector-second", |roots| remember(roots, &PLUS_XL))
        else {
            return;
        };
        assert_eq!(pair.registry.model().id, "plus-xl");
        pair.bus.plug(PLUS_PATH, &SimDevice::new(&PLUS));
        pair.wait_devices(1);
        wait_shown(&pair, &PLUS);
        assert_eq!(remembered(&pair), "{\"model\":\"plus\"}\n");
        pair.bus.plug(XL_PATH, &SimDevice::new(&PLUS_XL));
        pair.wait_devices(2);
        wait_shown(&pair, &PLUS_XL);
        assert_eq!(remembered(&pair), "{\"model\":\"plus-xl\"}\n");
        assert!(selector_visible(&pair));
    });
}

#[test]
fn a_model_that_cannot_open_is_not_offered_and_nothing_is_remembered() {
    run_gtk_test(|| {
        let Some(pair) = start_pair_with("selector-failed", |roots| {
            std::fs::write(
                roots.root_for(&PLUS_XL).join("layers.conf"),
                b"this is not a layers file\n",
            )
            .unwrap();
        }) else {
            return;
        };
        let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
        pair.bus.plug(PLUS_PATH, &plus);
        pair.bus.plug(XL_PATH, &xl);
        let opened = xl.clone();
        wait_until("the hub to open the XL", move || opened.opened() >= 1);
        pair.wait_devices(1);
        pair.settle();
        assert!(!selector_visible(&pair));
        assert_eq!(entries(&pair), ["Stream Deck +"]);
        assert_eq!(pair.registry.model().id, "plus");
        assert!(!pair.roots.preference.exists());
    });
}

#[test]
fn the_header_without_a_device_names_no_preview() {
    run_gtk_test(|| {
        let Some(harness) = start("selector-none", |_| {}) else {
            return;
        };
        let mut found = Vec::new();
        texts(harness.window.upcast_ref(), &mut found);
        assert!(found.iter().all(|text| !text.contains("Preview")));
        assert!(found
            .iter()
            .any(|text| text == "Stream Deck + or Stream Deck + XL not connected — plug one in"));
        assert_eq!(
            header_label(&harness.window, "status").text(),
            "Not connected"
        );
    });
}

// ---- A window with one model (or none) is the window without a selector ----

fn hash(pixels: &[u8]) -> u64 {
    pixels.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn window_hash(window: &ApplicationWindow) -> (i32, i32, u64) {
    wait_for_frame(window);
    wait_for_frame(window);
    let content = window.child().expect("the window content");
    (
        content.width(),
        content.height(),
        hash(&rendered_pixels(window, &content)),
    )
}

/// Rendered from the unmodified code of commit 606f5d1 (before any XL work) over
/// the same fixture, the bundled icons and [`ExplicitLook`], 1000x700: (connected, dark, hash) per
/// GDK backend, as the pixels depend on it. Without a device the header line
/// now names both models, so the test puts the original sentence back before
/// rendering; every other pixel is the original's.
/// (connected, dark, hash) for the four renders of one backend.
type Hashes = [(bool, bool, u64); 4];

const GOLDEN: [(&str, Hashes); 2] = [
    (
        "GdkX11Display",
        [
            (false, false, 0xf95e7711644dcbce),
            (false, true, 0xb8a1e4269955d503),
            (true, false, 0xfd020e56d4362dc2),
            (true, true, 0xab654ea2ea41e121),
        ],
    ),
    (
        "GdkWaylandDisplay",
        [
            (false, false, 0x958086dd042ce2dc),
            (false, true, 0xa9e2fe7f4b9922ee),
            (true, false, 0x561a53911d5b16b2),
            (true, true, 0xc852c3512248e473),
        ],
    ),
];

#[test]
fn the_window_with_a_plus_only_is_byte_identical_to_the_one_before_the_xl_work() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        // Production registers the bundled icons before any window.
        app_icon::register().unwrap();
        let backend = gtk::gdk::Display::default().unwrap().type_().name();
        let Some((_, goldens)) = GOLDEN.iter().find(|(name, _)| *name == backend) else {
            eprintln!("no golden for the {backend} backend: pixel identity not checked");
            return;
        };
        for &(connected, dark, expected) in goldens {
            let _look = ExplicitLook::apply_with(dark);
            let Some(fixture) = model_fixture("selector-golden", &PLUS, |_, _| {}) else {
                return;
            };
            if connected {
                show_connection_state(
                    &fixture.widgets.status,
                    &fixture.widgets.connection_detail,
                    &ConnectionState::Connected {
                        device: DeviceId::from_path("sim"),
                        model: &PLUS,
                    },
                    None,
                );
            } else {
                fixture
                    .widgets
                    .connection_detail
                    .set_text("Stream Deck + not connected — plug it in");
            }
            wait_for_frame(&fixture.window);
            wait_for_frame(&fixture.window);
            let root = &fixture.widgets.root;
            assert_eq!((root.width(), root.height()), (1000, 700));
            let found = hash(&rendered_pixels(&fixture.window, root));
            eprintln!("selector golden {backend} connected={connected} dark={dark}: {found:#018x}");
            assert_eq!(
                found, expected,
                "the + window (connected {connected}, dark {dark}) must render exactly as before"
            );
        }
    });
}

/// The real window (stack, registry, selector built and hidden) renders the same
/// pixels as the very same window without the selector widget in its header.
fn selector_adds_no_pixels(model: &'static DeviceModel, connected: bool, dark: bool) {
    let _look = ExplicitLook::apply_with(dark);
    let Some(harness) = start("selector-pixels", |roots| remember(roots, model)) else {
        return;
    };
    if connected {
        harness.show_model(model);
        let detail = header_label(&harness.window, "muted");
        wait_until("the connected text", move || {
            detail.text().ends_with(STATUS_SUFFIX)
        });
    }
    let selector = harness.session.selector().root.clone();
    assert!(!selector.is_visible());
    let with = window_hash(&harness.window);
    let header = selector
        .parent()
        .and_then(|parent| parent.downcast::<GtkBox>().ok())
        .expect("the header box");
    header.remove(&selector);
    let without = window_hash(&harness.window);
    assert_eq!(
        with, without,
        "{} connected {connected} dark {dark}: the hidden selector changes pixels",
        model.id
    );
    // Put it back for the registry's own bookkeeping.
    header.insert_child_after(
        &selector,
        header.first_child().and_then(|c| c.next_sibling()).as_ref(),
    );
}

#[test]
fn a_hidden_selector_adds_no_pixel_to_the_real_window_with_one_model_or_none() {
    run_gtk_test(|| {
        for model in [&PLUS, &PLUS_XL] {
            for (connected, dark) in [(false, false), (true, false), (false, true), (true, true)] {
                selector_adds_no_pixels(model, connected, dark);
            }
        }
    });
}

#[test]
fn the_header_with_two_models_renders_at_the_minimum_width_in_both_themes() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-render") else {
            return;
        };
        let (_plus, _xl) = pair.plug_both();
        let directory = std::env::var_os("STREAMDECK_QA_DIR");
        for (dark, width) in [(false, 1000), (true, 1000), (false, 560), (true, 560)] {
            let _look = ExplicitLook::apply_with(dark);
            pair.window.set_default_size(width, 700);
            wait_for_frame(&pair.window);
            let selector = pair.registry.selector();
            assert!(selector.root.is_visible() && selector.root.is_mapped());
            let content = pair.window.child().unwrap();
            let bounds = selector.root.compute_bounds(&content).unwrap();
            assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
            assert!(
                bounds.x() >= 0.0 && bounds.x() + bounds.width() <= content.width() as f32,
                "the selector stays inside the window at {width} px (dark {dark})"
            );
            let status = header_label(&pair.window, "status");
            let status_bounds = status.compute_bounds(&content).unwrap();
            assert!(
                status_bounds.x() + status_bounds.width() <= bounds.x() + 1.0,
                "the badge and the selector do not overlap"
            );
            if let Some(directory) = &directory {
                let texture = rendered_texture(&pair.window, &content);
                let _ = texture.save_to_png(std::path::Path::new(directory).join(format!(
                    "header-two-models-{}-{width}.png",
                    if dark { "dark" } else { "light" }
                )));
            }
        }
    });
}
