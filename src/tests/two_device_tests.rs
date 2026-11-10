//! The real HID hub over a simulated Stream Deck + and + XL, feeding the real
//! window: sessions, routing by model, pictures per device, mirroring and
//! shutdown. No hardware, HOME, network or HID stack is involved; the window
//! is built over scratch directories.
use super::model_session_tests::{
    fs_set_writable, live_threads, roots_in, scratch, stored_binding, tree, wait_until,
};
use super::*;
use feature_runtime::{FeatureRuntime, ModelSession, SessionRoots, SwitchReason};
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use sdeck_plus::{
    device_control::DeviceAction,
    device_model::{DeviceModel, PLUS, PLUS_XL},
    event_api::test_support::{
        encoder_button_report, key_report, rotation_report, start_hub, touch_report, SimBus,
        SimDevice,
    },
    features::FeatureAction,
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

const PLUS_PATH: &str = "/dev/hidraw1";
const XL_PATH: &str = "/dev/hidraw2";

struct Pair {
    base: PathBuf,
    roots: SessionRoots,
    app: gtk::Application,
    window: ApplicationWindow,
    registry: Rc<ModelSession>,
    bus: SimBus,
    /// The channel the hub reports through, for events that come from outside.
    events: mpsc::Sender<MonitorEvent>,
}

impl Drop for Pair {
    fn drop(&mut self) {
        // Stops the hub (blanking every device) and the workers, as closing does.
        self.app.emit_by_name::<()>("shutdown", &[]);
        for window in gtk::Window::list_toplevels() {
            if let Ok(window) = window.downcast::<gtk::Window>() {
                if window.transient_for().as_ref() == Some(self.window.upcast_ref()) {
                    window.destroy();
                }
            }
        }
        self.window.destroy();
        let _ = fs_set_writable(&self.roots.data_dir.join("devices"));
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The input reports one gesture sends on a model.
type Script = fn(&DeviceModel) -> Vec<Vec<u8>>;

fn switch_to(layer: &str) -> Action {
    Action::SwitchLayer(layer.into())
}

/// Layers A and B on both models; each model's own bindings send its input to
/// a different layer, so a stray event shows which model ran it.
fn seed(roots: &SessionRoots) {
    let mut plus = LayerStore::open_at(&roots.data_dir).unwrap();
    for name in ["A", "B"] {
        plus.create(name).unwrap();
    }
    for trigger in [
        Trigger::KeyPress(1),
        Trigger::EncoderClockwise(1),
        Trigger::ZoneTap(1),
    ] {
        plus.bind("Base", trigger, switch_to("A")).unwrap();
    }
    plus.bind(
        "Base",
        Trigger::KeyPress(2),
        Action::Feature(FeatureAction::Device(DeviceAction::AdjustBrightness(-10))),
    )
    .unwrap();
    plus.persist().unwrap();
    let xl_root = roots.root_for(&PLUS_XL);
    std::fs::create_dir_all(&xl_root).unwrap();
    let mut xl = LayerStore::open_at_model(&xl_root, &PLUS_XL).unwrap();
    for name in ["A", "B"] {
        xl.create(name).unwrap();
    }
    for trigger in [
        Trigger::KeyPress(1),
        Trigger::EncoderClockwise(1),
        Trigger::ZoneTap(1),
    ] {
        xl.bind("Base", trigger, switch_to("B")).unwrap();
    }
    xl.bind("Base", Trigger::KeyPress(36), switch_to("A"))
        .unwrap();
    xl.bind("Base", Trigger::KeyHold(36), switch_to("B"))
        .unwrap();
    xl.persist().unwrap();
}

/// The window over the real hub and an empty simulated bus; `None` without a display.
fn start_pair(tag: &str) -> Option<Pair> {
    if !init_gtk_for_test() {
        return None;
    }
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let base = scratch(tag);
    let roots = roots_in(&base);
    let updates = base.join("config/sdeck-plus/updates.json");
    std::fs::create_dir_all(updates.parent().unwrap()).unwrap();
    // Automatic update checks are off: nothing may reach the network.
    std::fs::write(&updates, br#"{"automatic_check":false}"#).unwrap();
    seed(&roots);
    // An id of its own: earlier tests may still hold the anonymous application path.
    let app = gtk::Application::new(
        Some("dev.streamdeck.SimulatedPairTest"),
        gtk::gio::ApplicationFlags::NON_UNIQUE,
    );
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let bus = SimBus::default();
    let slot = Rc::new(RefCell::new(None));
    let (hub_bus, hub_slot) = (bus.clone(), slot.clone());
    let environment = AppEnvironment {
        roots: Ok(roots.clone()),
        start_monitor: Box::new(move |events| {
            *hub_slot.borrow_mut() = Some(events.clone());
            start_hub(&hub_bus, events)
        }),
        update_preference: Ok(updates),
        observe_focus: false,
    };
    let registry = build_ui_in(&app, true, || environment).expect("the window opens");
    let events = slot.borrow_mut().take().unwrap();
    let window = configuration_window(&app).unwrap();
    Some(Pair {
        base,
        roots,
        app,
        window,
        registry,
        bus,
        events,
    })
}

impl Pair {
    /// A + and a + XL, both plugged in and both connected to the window.
    fn plug_both(&self) -> (SimDevice, SimDevice) {
        let (plus, xl) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS_XL));
        self.bus.plug(PLUS_PATH, &plus);
        self.bus.plug(XL_PATH, &xl);
        self.wait_devices(2);
        (plus, xl)
    }

    fn wait_devices(&self, count: usize) {
        let registry = self.registry.clone();
        wait_until("the devices to connect", move || {
            registry.device_count() == count
        });
    }

    fn runtime(&self, model: &DeviceModel) -> FeatureRuntime {
        self.registry
            .runtime_of(model)
            .unwrap_or_else(|| panic!("{} has a session", model.name))
    }

    /// Let the window's polling run a few times, so anything an event was
    /// going to do has been done.
    fn settle(&self) {
        let ticks = Rc::new(Cell::new(0));
        let counter = ticks.clone();
        let source = glib::timeout_add_local(Duration::from_millis(40), move || {
            counter.set(counter.get() + 1);
            glib::ControlFlow::Continue
        });
        wait_until("the polling to run", move || ticks.get() >= 4);
        source.remove();
    }
}

/// Run the main loop until `done` holds for `device`, within a bounded time.
/// The window's polling only runs while the loop does, and it is what turns a
/// device's input into actions and a session's snapshot into pictures.
fn wait_device(what: &str, device: &SimDevice, mut done: impl FnMut(&SimDevice) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let context = glib::MainContext::default();
    while !done(device) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        context.iteration(true);
    }
}

fn active(runtime: &FeatureRuntime) -> String {
    runtime.layers().borrow().active().to_owned()
}

fn reset(runtime: &FeatureRuntime) {
    runtime.layers().borrow_mut().select("Base");
}

fn tap_key(device: &SimDevice, model: &DeviceModel, index: usize) {
    device.send(key_report(model, &[index]));
    device.send(key_report(model, &[]));
}

fn wait_layer(what: &str, runtime: &FeatureRuntime, layer: &'static str) {
    let runtime = runtime.clone();
    wait_until(what, move || active(&runtime) == layer);
}

fn marker_png(directory: &Path, name: &str, size: i32) -> PathBuf {
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, size, size).unwrap();
    image.fill(0x0000ffff);
    image.new_subpixbuf(0, 0, 24, 24).fill(0xff0000ff);
    let path = directory.join(name);
    image.savev(&path, "png", &[]).unwrap();
    path
}

fn is_red(image: &Pixbuf, x: i32, y: i32) -> bool {
    let at = (y * image.rowstride() + x * image.n_channels()) as usize;
    let pixels = image.read_pixel_bytes();
    let pixel = &pixels.as_ref()[at..at + 3];
    pixel[0] > 180 && pixel[1] < 90 && pixel[2] < 90
}

fn decode(jpeg: Vec<u8>) -> Pixbuf {
    Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap()
}

#[test]
fn both_models_connect_and_each_keeps_a_session_of_its_own() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-sessions") else {
            return;
        };
        let (_plus, _xl) = pair.plug_both();
        assert_eq!(pair.registry.session_count(), 2);
        assert_eq!(pair.registry.live_devices(&PLUS), 1);
        assert_eq!(pair.registry.live_devices(&PLUS_XL), 1);
        // Two devices connected: the window keeps what it showed, and says both.
        assert_eq!(pair.registry.model().id, "plus");
        let roots = (
            pair.runtime(&PLUS).layers().borrow().root().to_owned(),
            pair.runtime(&PLUS_XL).layers().borrow().root().to_owned(),
        );
        assert_eq!(
            roots,
            (pair.roots.root_for(&PLUS), pair.roots.root_for(&PLUS_XL))
        );
    });
}

#[test]
fn input_on_each_device_runs_only_its_own_models_bindings() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-input") else {
            return;
        };
        let (plus_device, xl_device) = pair.plug_both();
        let (plus, xl) = (pair.runtime(&PLUS), pair.runtime(&PLUS_XL));
        let scripts: [(&str, Script); 3] = [
            ("key", |model| {
                vec![key_report(model, &[0]), key_report(model, &[])]
            }),
            ("dial", |model| vec![rotation_report(model, &[(0, 1)])]),
            ("touch", |model| {
                vec![touch_report(model, 1, 0x10, &[100, 50])]
            }),
        ];
        for (what, script) in scripts {
            for report in script(&PLUS) {
                plus_device.send(report);
            }
            wait_layer(&format!("the + {what}"), &plus, "A");
            pair.settle();
            assert_eq!(
                active(&xl),
                "Base",
                "a + {what} must not run the XL's binding"
            );
            reset(&plus);
            for report in script(&PLUS_XL) {
                xl_device.send(report);
            }
            wait_layer(&format!("the XL {what}"), &xl, "B");
            pair.settle();
            assert_eq!(
                active(&plus),
                "Base",
                "an XL {what} must not run the +'s binding"
            );
            reset(&xl);
        }
        // The dial press has no binding on either: nothing runs.
        plus_device.send(encoder_button_report(&PLUS, &[0]));
        plus_device.send(encoder_button_report(&PLUS, &[]));
        pair.settle();
        assert_eq!((active(&plus), active(&xl)), ("Base".into(), "Base".into()));
    });
}

#[test]
fn each_device_receives_only_its_own_models_pictures() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-artwork") else {
            return;
        };
        let (plus_device, xl_device) = pair.plug_both();
        // The full snapshot of each model, at its own report size.
        wait_device("plus_device", &plus_device, |device| {
            device.last_key_jpeg(7).is_some()
        });
        wait_device("xl_device", &xl_device, |device| {
            device.last_key_jpeg(35).is_some()
        });
        let plus_slots: Vec<_> = plus_device
            .images()
            .iter()
            .filter(|image| image.command == 0x07)
            .map(|image| image.slot)
            .collect();
        assert!(plus_slots.iter().all(|slot| *slot < 8), "{plus_slots:?}");
        let (plus_before, xl_before) = (plus_device.image_count(), xl_device.image_count());
        // Each session's own output is addressed to its own model.
        for (model, runtime, size) in [
            (&PLUS, pair.runtime(&PLUS), 120),
            (&PLUS_XL, pair.runtime(&PLUS_XL), 112),
        ] {
            runtime
                .artwork_ui()
                .output
                .send(ArtworkCommand::Assign {
                    target: ArtworkTarget::Keypad(1),
                    path: marker_png(&pair.base, &format!("{}.png", model.id), size),
                })
                .unwrap();
        }
        wait_device("plus_device", &plus_device, |device| {
            device.image_count() > plus_before
        });
        wait_device("xl_device", &xl_device, |device| {
            device.image_count() > xl_before
        });
        pair.settle();
        let plus_image = decode(plus_device.last_key_jpeg(0).unwrap());
        assert_eq!((plus_image.width(), plus_image.height()), (120, 120));
        assert!(is_red(&plus_image, 8, 8), "the + is never rotated");
        let xl_image = decode(xl_device.last_key_jpeg(0).unwrap());
        assert_eq!((xl_image.width(), xl_image.height()), (112, 112));
        assert!(
            is_red(&xl_image, 8, 112 - 9),
            "the XL turns counterclockwise"
        );
    });
}

#[test]
fn saving_in_the_shown_session_writes_only_that_models_root() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-save") else {
            return;
        };
        let _devices = pair.plug_both();
        let (plus_root, xl_root) = (pair.roots.root_for(&PLUS), pair.roots.root_for(&PLUS_XL));
        let bind_and_save = |runtime: &FeatureRuntime, key: u8| {
            let snapshot = {
                let mut layers = runtime.layers().borrow_mut();
                layers
                    .bind(
                        "Base",
                        Trigger::KeyPress(key),
                        Action::Shortcut {
                            modifiers: vec!["CTRL".into()],
                            key: "m".into(),
                        },
                    )
                    .unwrap();
                layers.clone()
            };
            runtime.persist(snapshot);
        };
        let xl_before = tree(&xl_root);
        assert_eq!(pair.registry.model().id, "plus");
        bind_and_save(&pair.registry.runtime(), 5);
        wait_until("the + binding on disk", {
            let root = plus_root.clone();
            move || stored_binding(&root, &PLUS, 5)
        });
        assert_eq!(
            tree(&xl_root),
            xl_before,
            "the XL files stay byte-identical"
        );

        pair.registry.show(&PLUS_XL, SwitchReason::Device);
        let plus_before = tree(&plus_root);
        bind_and_save(&pair.registry.runtime(), 20);
        wait_until("the XL binding on disk", {
            let root = xl_root.clone();
            move || stored_binding(&root, &PLUS_XL, 20)
        });
        assert_eq!(
            tree(&plus_root),
            plus_before,
            "the + files stay byte-identical"
        );
        assert!(!stored_binding(&plus_root, &PLUS, 20));
    });
}

#[test]
fn unplugging_one_device_keeps_the_other_running_and_the_shown_model_follows() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-unplug") else {
            return;
        };
        let (plus_device, xl_device) = pair.plug_both();
        let (plus, xl) = (pair.runtime(&PLUS), pair.runtime(&PLUS_XL));
        // The XL goes: the + keeps working, receiving input and pictures.
        pair.bus.unplug(XL_PATH);
        pair.wait_devices(1);
        assert_eq!(pair.registry.live_devices(&PLUS_XL), 0);
        assert_eq!(pair.registry.model().id, "plus", "the shown model stays");
        let before = plus_device.image_count();
        plus.artwork_ui()
            .output
            .send(ArtworkCommand::Assign {
                target: ArtworkTarget::Keypad(1),
                path: marker_png(&pair.base, "still.png", 120),
            })
            .unwrap();
        wait_device("plus_device", &plus_device, |device| {
            device.image_count() > before
        });
        tap_key(&plus_device, &PLUS, 0);
        wait_layer("the + still works", &plus, "A");
        reset(&plus);
        assert!(xl_device.handles() >= 1);

        // The XL comes back (a new unit): its session is kept, nothing is rebuilt.
        let again = SimDevice::new(&PLUS_XL);
        pair.bus.plug(XL_PATH, &again);
        pair.wait_devices(2);
        assert_eq!(pair.registry.session_count(), 2);
        assert_eq!(pair.registry.model().id, "plus");
        tap_key(&again, &PLUS_XL, 0);
        wait_layer("the replugged XL works", &xl, "B");
        reset(&xl);

        // The shown model's last device goes while another is connected: that one is shown.
        pair.registry.show(&PLUS_XL, SwitchReason::Device);
        pair.bus.unplug(XL_PATH);
        pair.wait_devices(1);
        assert_eq!(pair.registry.model().id, "plus");
        // With no device left the window keeps what it shows.
        pair.bus.unplug(PLUS_PATH);
        pair.wait_devices(0);
        assert_eq!(pair.registry.model().id, "plus");
        assert_eq!(pair.registry.session_count(), 2);
        // Later: the XL alone connects, so it is shown (the + is not connected).
        pair.bus.plug(XL_PATH, &SimDevice::new(&PLUS_XL));
        pair.wait_devices(1);
        let registry = pair.registry.clone();
        wait_until("the XL to be shown", move || {
            registry.model().id == "plus-xl"
        });
        assert_eq!(
            std::fs::read_to_string(&pair.roots.preference).unwrap(),
            r#"{"model":"plus-xl"}"#.to_owned() + "\n",
            "the last shown model is remembered after a successful change"
        );
    });
}

#[test]
fn two_devices_of_one_model_mirror_pictures_and_run_an_action_once() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-mirror") else {
            return;
        };
        let (first, second) = (SimDevice::new(&PLUS), SimDevice::new(&PLUS));
        pair.bus.plug(PLUS_PATH, &first);
        pair.bus.plug(XL_PATH, &second);
        pair.wait_devices(2);
        assert_eq!(pair.registry.session_count(), 1, "one session serves both");
        assert_eq!(pair.registry.live_devices(&PLUS), 2);
        wait_device("first", &first, |device| device.last_key_jpeg(7).is_some());
        wait_device("second", &second, |device| {
            device.last_key_jpeg(7).is_some()
        });
        for slot in 0..8 {
            assert_eq!(
                first.last_key_jpeg(slot),
                second.last_key_jpeg(slot),
                "key {slot} is drawn the same on both"
            );
        }
        // Key 2 lowers the brightness by 10: pressed on either, it runs once
        // for both devices.
        let level = |device: &SimDevice, wanted: u8| {
            device
                .features()
                .iter()
                .filter(|report| report[..2] == [0x03, 0x08] && report[2] == wanted)
                .count()
        };
        tap_key(&first, &PLUS, 1);
        for device in [&first, &second] {
            wait_device("the first press", device, |device| level(device, 90) >= 1);
        }
        tap_key(&second, &PLUS, 1);
        for device in [&first, &second] {
            wait_device("the second press", device, |device| level(device, 80) >= 1);
        }
        pair.settle();
        for device in [&first, &second] {
            assert_eq!(level(device, 90), 1, "the first press ran once");
            assert_eq!(level(device, 80), 1, "the second press ran once");
            assert_eq!(level(device, 70), 0);
        }
    });
}

#[test]
fn shutdown_blanks_both_devices_and_joins_every_worker_promptly() {
    run_gtk_test(|| {
        // Libraries can start threads that inherit a worker's name and outlive
        // it; what counts is that this window leaves no more than it found.
        let (hubs, workers) = (live_threads("streamdeck-hid"), live_threads("layer-worker"));
        let Some(pair) = start_pair("pair-shutdown") else {
            return;
        };
        let (plus_device, xl_device) = pair.plug_both();
        wait_device("plus_device", &plus_device, |device| {
            device.last_key_jpeg(7).is_some()
        });
        wait_device("xl_device", &xl_device, |device| {
            device.last_key_jpeg(35).is_some()
        });
        let started = Instant::now();
        pair.app.emit_by_name::<()>("shutdown", &[]);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "shutdown took {:?}",
            started.elapsed()
        );
        for device in [&plus_device, &xl_device] {
            assert_eq!(
                device.features().last().unwrap()[..3],
                [0x03, 0x08, 0],
                "the display is turned off"
            );
        }
        assert!(live_threads("layer-worker") <= workers);
        assert!(live_threads("streamdeck-hid") <= hubs);
    });
}

#[test]
fn twenty_plug_cycles_of_the_second_device_leave_nothing_behind() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-cycles") else {
            return;
        };
        let (_plus_device, xl_device) = pair.plug_both();
        // One XL unplug/replug first, so everything a cycle creates exists.
        let mut xl_device = xl_device;
        let mut cycle = |pair: &Pair| {
            pair.bus.unplug(XL_PATH);
            pair.wait_devices(1);
            xl_device = SimDevice::new(&PLUS_XL);
            pair.bus.plug(XL_PATH, &xl_device);
            pair.wait_devices(2);
        };
        cycle(&pair);
        let widgets = super::model_session_tests::count_widgets(pair.window.upcast_ref());
        let counts = || {
            [
                "layer-worker",
                "info-reader",
                "hid-art-0084",
                "hid-ctl-0084",
                "hid-art-00c6",
                "hid-ctl-00c6",
                "streamdeck-hid",
                "streamdeck-hid-",
            ]
            .map(live_threads)
        };
        // Everything the first cycle created exists now: later cycles add nothing.
        pair.settle();
        let threads = counts();
        let mut actions = pair.app.list_actions();
        actions.sort();
        for _ in 0..20 {
            cycle(&pair);
        }
        // A worker that was unplugged ends right after it says so.
        wait_until("the device workers to settle", move || {
            counts()
                .iter()
                .zip(threads)
                .all(|(now, before)| *now <= before)
        });
        assert_eq!(pair.registry.session_count(), 2);
        assert_eq!(
            super::model_session_tests::count_widgets(pair.window.upcast_ref()),
            widgets
        );
        let mut after = pair.app.list_actions();
        after.sort();
        assert_eq!(after, actions, "no application action is added");
    });
}

#[test]
fn a_key_hold_on_the_hidden_xl_session_still_fires() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-hold") else {
            return;
        };
        let (_plus_device, xl_device) = pair.plug_both();
        assert_eq!(pair.registry.model().id, "plus", "the + is the one shown");
        let xl = pair.runtime(&PLUS_XL);
        // A short tap on key 36 runs the tap action.
        tap_key(&xl_device, &PLUS_XL, 35);
        wait_layer("the tap", &xl, "A");
        reset(&xl);
        // A hold fires at its boundary, once, with the XL hidden behind the +.
        xl_device.send(key_report(&PLUS_XL, &[35]));
        let pressed = xl.clone();
        wait_until("the key is down", move || pressed.key_hold_generation() > 0);
        xl.advance_key_clock(sdeck_plus::key_hold::HOLD_DURATION);
        wait_layer("the hold", &xl, "B");
        reset(&xl);
        xl_device.send(key_report(&PLUS_XL, &[]));
        pair.settle();
        assert_eq!(active(&xl), "Base", "the release after a hold is silent");
        // The hidden session's countdown keeps running too.
        xl.execute(
            "timer".into(),
            Action::Feature(FeatureAction::Timer { seconds: 60 }),
        );
        pair.registry.show(&PLUS, SwitchReason::Device);
        assert!(xl.timer_active());
    });
}

#[test]
fn focus_events_reach_every_session() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-focus") else {
            return;
        };
        let _devices = pair.plug_both();
        pair.events
            .send(MonitorEvent::ApplicationFocus(Err("no compositor".into())))
            .unwrap();
        for model in [&PLUS, &PLUS_XL] {
            let runtime = pair.runtime(model);
            wait_until("the focus report", move || {
                runtime
                    .status
                    .text()
                    .contains("Application detection unavailable: no compositor")
            });
        }
    });
}

#[test]
fn the_menu_and_updates_follow_the_shown_session_and_open_one_dialog() {
    run_gtk_test(|| {
        let Some(pair) = start_pair("pair-menu") else {
            return;
        };
        let _devices = pair.plug_both();
        let bound_to = |model: &DeviceModel| {
            let bound = pair.registry.menu().bound_runtime().unwrap();
            Rc::ptr_eq(bound.layers(), pair.runtime(model).layers())
        };
        assert!(bound_to(&PLUS));
        let open = || {
            gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Dialog>().ok())
                .filter(|d| d.is_visible() && d.title().as_deref() == Some("Updates"))
                .count()
        };
        for model in [&PLUS_XL, &PLUS, &PLUS_XL] {
            pair.registry.show(model, SwitchReason::Device);
            assert!(bound_to(model), "the menu acts on {}", model.name);
            pair.registry.menu().present_updates();
            pair.registry.menu().present_updates();
            assert_eq!(open(), 1, "one Updates dialog after showing {}", model.name);
        }
        assert_eq!(
            pair.app
                .list_actions()
                .iter()
                .filter(|name| *name == "check-updates-on-open")
                .count(),
            1
        );
    });
}
