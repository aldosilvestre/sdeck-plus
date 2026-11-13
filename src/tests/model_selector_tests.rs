//! The header's 'Current' selector: when it exists, what it lists, how the
//! shown model is chosen, and that a window with one model (or none) looks
//! exactly as it did before the selector existed. No real device, HOME or
//! network is involved.
use super::model_session_tests::{header_label, remember, start, tree, wait_until};
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
        let sentence = format!("Stream Deck + and Stream Deck + XL {STATUS_SUFFIX}");
        assert_eq!(status_text(&pair), sentence);
        let tooltip = header_label(&pair.window, "muted")
            .tooltip_text()
            .unwrap()
            .to_string();
        assert_eq!(
            tooltip,
            format!(
                "{sentence}\nUSB device VID 0FD9 / PID 0084 and 00C6 · HID input and artwork active"
            ),
            "the tooltip starts with the whole sentence"
        );
        assert_eq!(header_label(&pair.window, "status").text(), "Connected");
    });
}

#[test]
fn the_two_model_status_follows_the_supported_order_not_the_plug_order() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-order") else {
            return;
        };
        // The XL plugs in first.
        pair.bus.plug(XL_PATH, &SimDevice::new(&PLUS_XL));
        pair.wait_devices(1);
        assert_eq!(
            status_text(&pair),
            format!("Stream Deck + XL {STATUS_SUFFIX}"),
            "one model keeps today's text"
        );
        assert_eq!(
            header_label(&pair.window, "muted")
                .tooltip_text()
                .unwrap()
                .as_str(),
            "USB device VID 0FD9 / PID 00C6 · HID input and artwork active",
        );
        pair.bus.plug(PLUS_PATH, &SimDevice::new(&PLUS));
        pair.wait_devices(2);
        assert_eq!(
            status_text(&pair),
            format!("Stream Deck + and Stream Deck + XL {STATUS_SUFFIX}")
        );
        assert_eq!(entries(&pair), ["Stream Deck +", "Stream Deck + XL"]);
    });
}

/// The widgets Tab visits, in order, starting from `window`'s first stop.
fn tab_stops(window: &ApplicationWindow) -> Vec<gtk::Widget> {
    gtk::prelude::GtkWindowExt::set_focus(window, None::<&gtk::Widget>);
    let mut stops: Vec<gtk::Widget> = Vec::new();
    for _ in 0..400 {
        if !window.child_focus(gtk::DirectionType::TabForward) {
            break;
        }
        let Some(current) = gtk::prelude::GtkWindowExt::focus(window) else {
            break;
        };
        if stops.contains(&current) {
            break;
        }
        stops.push(current);
    }
    stops
}

fn in_selector(pair: &Pair, widget: &gtk::Widget) -> bool {
    let selector = pair.registry.selector();
    *widget == selector.dropdown
        || widget.is_ancestor(&selector.dropdown)
        || widget.is_ancestor(&selector.root)
}

#[test]
fn the_selector_is_a_tab_stop_only_while_it_is_visible() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-tab") else {
            return;
        };
        pair.window.present();
        pair.bus.plug(PLUS_PATH, &SimDevice::new(&PLUS));
        pair.wait_devices(1);
        let stops = tab_stops(&pair.window);
        assert!(!stops.is_empty(), "Tab visits something");
        assert!(stops.iter().all(|stop| !in_selector(&pair, stop)));
        pair.bus.plug(XL_PATH, &SimDevice::new(&PLUS_XL));
        pair.wait_devices(2);
        let forward = tab_stops(&pair.window);
        assert!(
            forward.iter().any(|stop| in_selector(&pair, stop)),
            "Tab reaches the selector"
        );
        // The selector takes focus, and Tab and Shift+Tab both leave it. (GTK
        // orders the header by geometry, so which neighbour Shift+Tab reaches
        // is not pinned down here.)
        let dropdown = pair.registry.selector().dropdown.clone();
        for direction in [
            gtk::DirectionType::TabForward,
            gtk::DirectionType::TabBackward,
        ] {
            assert!(dropdown.grab_focus());
            assert!(pair.window.child_focus(direction));
            assert!(gtk::prelude::GtkWindowExt::focus(&pair.window)
                .is_some_and(|focus| !in_selector(&pair, &focus)));
        }
        pair.bus.unplug(XL_PATH);
        pair.wait_devices(1);
        assert!(tab_stops(&pair.window)
            .iter()
            .all(|stop| !in_selector(&pair, stop)));
    });
}

fn first_list_view(widget: &gtk::Widget) -> Option<gtk::ListView> {
    if let Some(list) = widget.downcast_ref::<gtk::ListView>() {
        return Some(list.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = first_list_view(&current) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

/// gtk4-rs 0.9 cannot synthesize key events in-process (no
/// `gtk_test_widget_send_key` binding, and `gdk::KeyEvent` has no constructor),
/// so this drives what a key press ends in: keyboard focus on the drop-down and
/// its toggle button's `clicked` (Space/Enter), which opens the list of
/// entries; then the selection.
#[test]
fn the_selector_opens_with_activate_and_a_row_chosen_with_enter_switches_the_model() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-keyboard") else {
            return;
        };
        pair.window.present();
        let (_plus, _xl) = pair.plug_both();
        let dropdown = pair.registry.selector().dropdown.clone();
        assert!(dropdown.is_focusable() && dropdown.is_sensitive());
        assert!(dropdown.grab_focus(), "the drop-down takes keyboard focus");
        // Space/Enter on the focused toggle button clicks it, which opens the list.
        let toggle = dropdown
            .first_child()
            .and_then(|child| child.downcast::<gtk::ToggleButton>().ok())
            .expect("the drop-down's toggle button");
        toggle.emit_clicked();
        pair.settle();
        let list = first_list_view(dropdown.upcast_ref()).expect("the list of entries");
        assert!(list.is_mapped(), "activating opens the list");
        assert_eq!(dropdown.selected(), 0);
        assert_eq!(list.model().map(|model| model.n_items()), Some(2));
        // Emitting `activate` on the list or moving its selection model does not
        // reach the drop-down in-process (tried both), so the final choice goes
        // through the drop-down's own selection, as the list's handler does.
        dropdown.set_selected(1);
        pair.settle();
        assert_eq!(dropdown.selected(), 1);
        assert_eq!(pair.registry.model().id, "plus-xl");
        assert_eq!(remembered(&pair), "{\"model\":\"plus-xl\"}\n");
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

/// (connected, dark, hash) for the four renders of one backend.
type Hashes = [(bool, bool, u64); 4];

/// Pixel hashes of the unmodified code of commit 606f5d1 (before any XL work)
/// rendered on the author's host. They depend on fonts, themes and GDK
/// backend, so they only mean something on that host: see
/// [`the_window_with_a_plus_only_is_byte_identical_to_the_one_before_the_xl_work`].
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

/// Manual check against the code before the XL work; never part of the suite.
///
/// To regenerate the goldens for this host: extract commit 606f5d1 with
/// `git archive 606f5d1 | tar -x -C <empty scratch dir>`, add a test there that
/// builds the window as `model_fixture` does here (`build_widgets` over a
/// `LayerStore::open_at` and `ArtworkStore::open_at`, 1000x700, the bundled
/// icons registered, the same explicit look, and the connection state set to
/// the same sentence), render `widgets.root` with `rendered_pixels` and print
/// the FNV-1a hash this file's `hash` computes. Run it with its own
/// `CARGO_TARGET_DIR` for light/dark x searching/connected on the backend you
/// use (`GDK_BACKEND=x11` or Wayland), paste the four hashes into `GOLDEN`
/// under that backend and run this test with `-- --ignored`.
#[test]
#[ignore = "host-specific pixel hashes; see the doc comment to regenerate them"]
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

/// Host-independent: the + window's header has no 'Current' or 'Preview' text
/// and nothing of the selector can take focus.
#[test]
fn the_plus_window_has_no_selector_text_and_no_selector_in_the_tab_order() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-structure") else {
            return;
        };
        pair.bus.plug(PLUS_PATH, &SimDevice::new(&PLUS));
        pair.wait_devices(1);
        let selector = pair.registry.selector();
        assert!(!selector.root.is_visible() && !selector.dropdown.is_focusable());
        // Hidden widgets are skipped by Tab: walk the focusable visible widgets.
        fn focusable(widget: &gtk::Widget, found: &mut Vec<gtk::Widget>) {
            if !widget.is_visible() {
                return;
            }
            if widget.is_focusable() {
                found.push(widget.clone());
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                focusable(&current, found);
                child = current.next_sibling();
            }
        }
        let mut tab = Vec::new();
        focusable(pair.window.upcast_ref(), &mut tab);
        assert!(!tab.is_empty());
        assert!(
            tab.iter()
                .all(|widget| !widget.is_ancestor(&selector.root) && *widget != selector.dropdown),
            "no selector widget is reachable with Tab"
        );
        let mut found = Vec::new();
        let header = selector.root.parent().expect("the header");
        texts(&header, &mut found);
        // The caption exists but is not shown; no visible header text says it.
        fn visible_texts(widget: &gtk::Widget, found: &mut Vec<String>) {
            if !widget.is_visible() {
                return;
            }
            if let Some(label) = widget.downcast_ref::<Label>() {
                found.push(label.text().to_string());
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                visible_texts(&current, found);
                child = current.next_sibling();
            }
        }
        let mut visible = Vec::new();
        visible_texts(&header, &mut visible);
        assert!(visible
            .iter()
            .all(|text| text != "Current" && !text.contains("Preview")));
        assert!(found.iter().all(|text| !text.contains("Preview")));
    });
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

// ---- Visibility, sequences and isolation ----

fn flip_counter(pair: &Pair) -> Rc<Cell<u32>> {
    let flips = Rc::new(Cell::new(0));
    let counter = flips.clone();
    pair.registry
        .selector()
        .root
        .connect_visible_notify(move |_| counter.set(counter.get() + 1));
    flips
}

#[test]
fn the_selector_never_flashes_for_one_model_and_toggles_once_per_second_model_plug() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-flips") else {
            return;
        };
        let flips = flip_counter(&pair);
        let hidden = |pair: &Pair| {
            !selector_visible(pair) && !pair.registry.selector().dropdown.is_focusable()
        };
        assert!(hidden(&pair));
        for (model, first, second) in [(&PLUS, PLUS_PATH, XL_PATH), (&PLUS_XL, XL_PATH, PLUS_PATH)]
        {
            pair.bus.plug(first, &SimDevice::new(model));
            pair.wait_devices(1);
            pair.bus.plug(second, &SimDevice::new(model));
            pair.wait_devices(2);
            pair.settle();
            assert!(hidden(&pair), "{} twice", model.name);
            pair.bus.unplug(second);
            pair.bus.unplug(first);
            pair.wait_devices(0);
        }
        assert_eq!(flips.get(), 0, "same-model plugs never show the selector");
        pair.bus.plug(XL_PATH, &SimDevice::new(&PLUS_XL));
        pair.wait_devices(1);
        for cycle in 0..10 {
            pair.bus.plug(PLUS_PATH, &SimDevice::new(&PLUS));
            pair.wait_devices(2);
            pair.settle();
            assert!(selector_visible(&pair), "cycle {cycle}");
            assert!(pair.registry.selector().dropdown.is_focusable());
            assert_eq!(entries(&pair), ["Stream Deck +", "Stream Deck + XL"]);
            pair.bus.unplug(PLUS_PATH);
            pair.wait_devices(1);
            pair.settle();
            assert!(hidden(&pair), "cycle {cycle}");
        }
        assert_eq!(flips.get(), 20);
        // A duplicate while both models are connected adds no entry.
        pair.bus.plug(PLUS_PATH, &SimDevice::new(&PLUS));
        pair.bus.plug("/dev/hidraw3", &SimDevice::new(&PLUS));
        pair.wait_devices(3);
        pair.settle();
        assert!(selector_visible(&pair));
        assert_eq!(entries(&pair), ["Stream Deck +", "Stream Deck + XL"]);
    });
}

#[test]
fn random_plug_unplug_and_pick_sequences_follow_an_independent_model() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-random") else {
            return;
        };
        let paths = [PLUS_PATH, XL_PATH, "/dev/hidraw3", "/dev/hidraw4"];
        let mut devices: Vec<(&str, &'static DeviceModel)> = Vec::new();
        let (mut shown, mut last, mut chose) = (P, P, false);
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut random = move |below: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % below
        };
        let connected = |devices: &[(&str, &'static DeviceModel)]| -> Vec<&'static DeviceModel> {
            DeviceModel::supported()
                .filter(|model| devices.iter().any(|(_, known)| known == model))
                .collect()
        };
        for step in 0..160 {
            let mut unplugged = false;
            match random(3) {
                0 if devices.len() < paths.len() => {
                    let path = *paths
                        .iter()
                        .find(|path| devices.iter().all(|(used, _)| used != *path))
                        .unwrap();
                    let model = if random(2) == 0 { P } else { X };
                    pair.bus.plug(path, &SimDevice::new(model));
                    devices.push((path, model));
                    pair.wait_devices(devices.len());
                }
                1 if !devices.is_empty() => {
                    let (path, _) = devices.remove(random(devices.len() as u64) as usize);
                    pair.bus.unplug(path);
                    unplugged = true;
                    pair.wait_devices(devices.len());
                }
                2 if connected(&devices).len() >= 2 => {
                    let model = if random(2) == 0 { P } else { X };
                    select(&pair, model);
                    if model != shown {
                        (chose, last, shown) = (true, model, model);
                    }
                    pair.settle();
                    assert_eq!(pair.registry.model().id, shown.id, "step {step} pick");
                    continue;
                }
                _ => continue,
            }
            let now = connected(&devices);
            if !now.contains(&shown) {
                chose = false;
            }
            let target = if now.is_empty() || (chose && now.contains(&shown)) {
                shown
            } else if now.contains(&last) {
                last
            } else {
                now[0]
            };
            if target != shown {
                if unplugged {
                    last = target;
                }
                shown = target;
            }
            pair.settle();
            assert_eq!(
                pair.registry.model().id,
                shown.id,
                "step {step} {devices:?}"
            );
            assert_eq!(selector_visible(&pair), now.len() >= 2, "step {step}");
            assert_eq!(
                entries(&pair),
                now.iter().map(|model| model.name).collect::<Vec<_>>(),
                "step {step}"
            );
            if !now.is_empty() && pair.roots.preference.exists() {
                assert!(remembered(&pair).contains(shown.id), "step {step}");
            }
        }
    });
}

#[test]
fn a_malformed_device_json_is_kept_byte_identical_even_after_a_pick() {
    run_gtk_test(|| {
        let Some(pair) = start_pair_with("selector-badpref", |roots| {
            std::fs::create_dir_all(roots.preference.parent().unwrap()).unwrap();
            std::fs::write(&roots.preference, b"{not json").unwrap();
        }) else {
            return;
        };
        assert_eq!(pair.registry.model().id, "plus");
        let (_plus, _xl) = pair.plug_both();
        select(&pair, &PLUS_XL);
        pair.settle();
        assert_eq!(pair.registry.model().id, "plus-xl");
        assert_eq!(std::fs::read(&pair.roots.preference).unwrap(), b"{not json");
    });
}

#[test]
fn switching_back_and_forth_leaves_every_root_byte_identical_and_asks_nothing() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("selector-roots") else {
            return;
        };
        let (_plus, _xl) = pair.plug_both();
        pair.settle();
        let plus = pair.runtime(&PLUS);
        let ui = plus.binding_ui();
        ui.text_entry.set_text("echo draft");
        ui.action_picker.set_selected(1);
        assert!(ui.is_dirty());
        let (plus_root, xl_root) = (pair.roots.data_dir.clone(), pair.roots.root_for(&PLUS_XL));
        let (plus_before, xl_before) = (tree(&plus_root), tree(&xl_root));
        for model in [&PLUS_XL, &PLUS] {
            select(&pair, model);
            pair.settle();
            assert!(plus.binding_ui().is_dirty(), "the + keeps its draft");
            assert!(gtk::Window::list_toplevels()
                .iter()
                .filter_map(|window| window.clone().downcast::<gtk::MessageDialog>().ok())
                .all(|dialog| !dialog.is_visible()));
        }
        assert_eq!(tree(&plus_root), plus_before);
        assert_eq!(tree(&xl_root), xl_before);
    });
}

#[test]
fn the_rail_badge_mirrors_the_connection_label_and_turns_into_a_dot_when_compact() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let source = Label::new(None);
        source.add_css_class("status");
        let badge = ui::mirror_badge(&source);
        // The mirror starts as the label is and follows every later change.
        source.set_text("Not connected");
        source.add_css_class("status-idle");
        source.set_tooltip_text(Some("Plug a Stream Deck in"));
        assert_eq!(badge.text(), "Not connected");
        assert_eq!(
            badge.tooltip_text().as_deref(),
            Some("Plug a Stream Deck in")
        );
        assert!(badge.has_css_class("status") && badge.has_css_class("status-idle"));
        assert!(badge.has_css_class("rail-badge"));
        // A tone change replaces the old one.
        source.remove_css_class("status-idle");
        source.add_css_class("status-online");
        source.set_text("Connected");
        assert!(badge.has_css_class("status-online") && !badge.has_css_class("status-idle"));
        // The compact marker belongs to the rail and survives updates.
        badge.add_css_class("rail-compact");
        source.set_text("Stream Deck + connected");
        assert!(badge.has_css_class("rail-compact"));
        assert_eq!(badge.text(), "Stream Deck + connected");
    });
}

fn descendants_with_class(widget: &gtk::Widget, class: &str, found: &mut Vec<gtk::Widget>) {
    if widget.has_css_class(class) {
        found.push(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        descendants_with_class(&current, class, found);
        child = current.next_sibling();
    }
}

#[test]
fn confirm_dialogs_read_left_aligned_and_their_cancel_button_shows_an_escape_hint() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let dialog = gtk::MessageDialog::builder()
            .message_type(gtk::MessageType::Question)
            .text("Delete \"Studio\"?")
            .secondary_text("Its pages and bindings go with it.")
            .build();
        ui::style_confirm_dialog(&dialog);
        let cancel = dialog.add_button("Keep", gtk::ResponseType::Cancel);
        ui::add_key_hint(&cancel, "Esc");
        let destroy = dialog.add_button("Delete", gtk::ResponseType::Accept);
        assert!(dialog.has_css_class("confirm-dialog"));
        // The title and the explanation start at the left edge, as a dialog reads.
        let mut labels = Vec::new();
        let mut child = dialog.message_area().first_child();
        while let Some(widget) = child {
            if let Some(label) = widget.downcast_ref::<Label>() {
                assert_eq!(label.justify(), gtk::Justification::Left);
                assert_eq!(label.xalign(), 0.0);
                labels.push(label.text().to_string());
            }
            child = widget.next_sibling();
        }
        assert_eq!(
            labels,
            ["Delete \"Studio\"?", "Its pages and bindings go with it."]
        );
        // Only the cancel button has the hint, and its accessible name stays "Keep".
        let mut hints = Vec::new();
        descendants_with_class(dialog.upcast_ref(), "key-hint", &mut hints);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].downcast_ref::<Label>().unwrap().text(), "Esc");
        assert!(cancel.is_ancestor(dialog.upcast_ref::<gtk::Widget>()));
        assert!(hints[0].is_ancestor(&cancel));
        assert!(!hints[0].is_ancestor(&destroy));
        assert_eq!(
            cancel.downcast_ref::<Button>().unwrap().label().as_deref(),
            None,
            "the caption moved into the button's content"
        );
        let mut captions = Vec::new();
        // The button holds one box with the caption and the hint.
        let mut inside = cancel
            .first_child()
            .and_then(|content| content.first_child());
        while let Some(widget) = inside {
            captions.push(widget.downcast_ref::<Label>().map(|l| l.text().to_string()));
            inside = widget.next_sibling();
        }
        assert_eq!(
            captions,
            [Some("Keep".to_string()), Some("Esc".to_string())]
        );
    });
}
