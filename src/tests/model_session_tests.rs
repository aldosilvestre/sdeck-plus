//! In-process model sessions: startup, hardware changes, 'Preview as' and the
//! data each model keeps. No real device, HOME or network is involved: the
//! window is built over scratch directories and a detached monitor.
use super::*;
use feature_runtime::{ModelSession, SessionRoots};
use sdeck_plus::{
    device_model::{DeviceModel, PLUS, PLUS_XL},
    event_api::{ConnectionState, HidController},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Harness {
    base: PathBuf,
    roots: SessionRoots,
    app: gtk::Application,
    window: ApplicationWindow,
    session: Rc<ModelSession>,
    /// Connection events enter the window through the monitor's own channel.
    events: mpsc::Sender<MonitorEvent>,
    artwork: mpsc::Receiver<ArtworkCommand>,
    _controls: mpsc::Receiver<sdeck_plus::device_control::DeviceAction>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Stops the layer worker and the polling source, as closing the app does.
        self.app.emit_by_name::<()>("shutdown", &[]);
        // Dialogs outlive their parent; later tests look for visible ones.
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

fn fs_set_writable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

fn scratch(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "sdeck-model-session-{}-{}-{tag}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn roots_in(base: &Path) -> SessionRoots {
    SessionRoots {
        data_dir: base.join("data/sdeck-plus"),
        preference: base.join("config/sdeck-plus/device.json"),
    }
}

/// Run the main loop until `condition` holds, failing after a bounded wait.
fn wait_until(what: &str, condition: impl Fn() -> bool + 'static) {
    if condition() {
        return;
    }
    let main_loop = glib::MainLoop::new(None, false);
    let deadline = Instant::now() + Duration::from_secs(20);
    let satisfied = Rc::new(Cell::new(false));
    let (quit, flag) = (main_loop.clone(), satisfied.clone());
    glib::timeout_add_local(Duration::from_millis(5), move || {
        if condition() {
            flag.set(true);
        }
        if flag.get() || Instant::now() > deadline {
            quit.quit();
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    main_loop.run();
    assert!(satisfied.get(), "timed out waiting for {what}");
}

/// Start the window over `roots` prepared by `prepare`; `None` without a display.
fn start(tag: &str, prepare: impl FnOnce(&SessionRoots)) -> Option<Harness> {
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
    prepare(&roots);
    let app = gtk::Application::new(None::<&str>, gtk::gio::ApplicationFlags::NON_UNIQUE);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let monitor = Rc::new(RefCell::new(None));
    let slot = monitor.clone();
    let environment = AppEnvironment {
        roots: Ok(roots.clone()),
        start_monitor: Box::new(move |events| {
            let (hid, artwork, controls) = HidController::detached();
            *slot.borrow_mut() = Some((events, artwork, controls));
            hid
        }),
        update_preference: Ok(updates),
        observe_focus: false,
    };
    let session = build_ui_in(&app, true, || environment).expect("the window opens");
    let (events, artwork, controls) = monitor.borrow_mut().take().unwrap();
    let window = configuration_window(&app).unwrap();
    Some(Harness {
        base,
        roots,
        app,
        window,
        session,
        events,
        artwork,
        _controls: controls,
    })
}

impl Harness {
    fn model(&self) -> &'static DeviceModel {
        self.session.runtime().session_model()
    }

    fn connect(&self, model: &'static DeviceModel) {
        self.events
            .send(MonitorEvent::Connection(ConnectionState::Connected(model)))
            .unwrap();
    }

    fn wait_for_model(&self, model: &'static DeviceModel) {
        let session = self.session.clone();
        wait_until(model.name, move || {
            session.runtime().session_model() == model
        });
    }

    fn artwork_sent(&self) -> Vec<ArtworkCommand> {
        self.artwork.try_iter().collect()
    }

    fn root(&self, model: &DeviceModel) -> PathBuf {
        self.roots.root_for(model)
    }

    /// A binding in the shown session, saved through the layer worker.
    fn bind_and_save(&self, key: u8) {
        let runtime = self.session.runtime();
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
    }

    fn picker_models(&self) -> Vec<&'static str> {
        self.session
            .picker()
            .models
            .iter()
            .map(|model| model.name)
            .collect()
    }

    fn preview_as(&self, model: &DeviceModel) {
        let index = self
            .session
            .picker()
            .models
            .iter()
            .position(|candidate| candidate.id == model.id)
            .unwrap();
        self.session.picker().dropdown.set_selected(index as u32);
    }

    fn dialogs(&self) -> Vec<gtk::MessageDialog> {
        gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::MessageDialog>().ok())
            .filter(|dialog| {
                dialog.is_visible()
                    && dialog.transient_for().as_ref() == Some(self.window.upcast_ref())
            })
            .collect()
    }

    fn make_editor_dirty(&self) {
        let ui = self.session.runtime().binding_ui();
        assert!(!ui.is_dirty());
        ui.text_entry.set_text("echo changed");
        ui.action_picker.set_selected(1);
        assert!(ui.is_dirty(), "the draft differs from the saved binding");
    }
}

/// Every file under `root` (relative path to bytes), skipping `devices/`.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.map(Result::unwrap) {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "devices") {
                    continue;
                }
                walk(root, &path, found);
            } else {
                found.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}

fn remember(roots: &SessionRoots, model: &DeviceModel) {
    std::fs::create_dir_all(roots.preference.parent().unwrap()).unwrap();
    std::fs::write(&roots.preference, format!(r#"{{"model":"{}"}}"#, model.id)).unwrap();
}

fn stored_binding(root: &Path, model: &'static DeviceModel, key: u8) -> bool {
    // A save in flight can briefly hide a file; the caller polls until it settles.
    LayerStore::open_at_model(root, model)
        .is_ok_and(|store| store.binding("Base", &Trigger::KeyPress(key)).is_some())
}

fn header_label(window: &ApplicationWindow, class: &str) -> Label {
    fn find(widget: &gtk::Widget, class: &str) -> Option<Label> {
        if let Some(label) = widget.downcast_ref::<Label>() {
            if label.has_css_class(class) {
                return Some(label.clone());
            }
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find(&current, class) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
    find(window.upcast_ref(), class).expect("header label")
}

fn count_widgets(widget: &gtk::Widget) -> usize {
    let mut count = 1;
    let mut child = widget.first_child();
    while let Some(current) = child {
        count += count_widgets(&current);
        child = current.next_sibling();
    }
    count
}

fn live_threads(name: &str) -> usize {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(Result::ok)
        .filter(|task| {
            std::fs::read_to_string(task.path().join("comm")).is_ok_and(|comm| comm.trim() == name)
        })
        .count()
}

#[test]
fn startup_without_a_device_shows_the_stream_deck_plus_by_default() {
    run_gtk_test(|| {
        let Some(harness) = start("default", |_| {}) else {
            return;
        };
        assert_eq!(harness.model().id, "plus");
        assert_eq!(
            harness.session.runtime().layers().borrow().root(),
            harness.roots.data_dir
        );
        let picker = harness.session.picker();
        assert_eq!(
            harness.picker_models(),
            ["Stream Deck +", "Stream Deck + XL"]
        );
        assert_eq!(picker.dropdown.selected(), 0);
        assert!(picker.dropdown.is_sensitive());
    });
}

#[test]
fn startup_opens_the_model_remembered_from_the_last_run() {
    run_gtk_test(|| {
        let Some(harness) = start("remembered", |roots| remember(roots, &PLUS_XL)) else {
            return;
        };
        assert_eq!(harness.model().id, "plus-xl");
        assert_eq!(
            harness.session.runtime().layers().borrow().root(),
            harness.root(&PLUS_XL)
        );
        assert_eq!(harness.session.picker().dropdown.selected(), 1);
        assert!(
            !harness.roots.data_dir.join("layers.conf").exists(),
            "the Stream Deck + root is not touched while previewing the XL"
        );
    });
}

#[test]
fn startup_with_a_damaged_preference_shows_the_plus_and_keeps_the_file() {
    run_gtk_test(|| {
        let Some(harness) = start("damaged", |roots| {
            std::fs::create_dir_all(roots.preference.parent().unwrap()).unwrap();
            std::fs::write(&roots.preference, b"not json").unwrap();
        }) else {
            return;
        };
        assert_eq!(harness.model().id, "plus");
        assert_eq!(
            std::fs::read(&harness.roots.preference).unwrap(),
            b"not json"
        );
        let dialog = harness.dialogs();
        assert!(
            dialog.iter().any(|dialog| dialog
                .text()
                .is_some_and(|text| text.contains("last-used device model"))),
            "the damaged preference is reported"
        );
    });
}

#[test]
fn connecting_an_xl_switches_stores_without_touching_the_plus_files() {
    run_gtk_test(|| {
        let Some(harness) = start("connect-xl", |roots| {
            let mut store = LayerStore::open_at(&roots.data_dir).unwrap();
            store
                .bind(
                    "Base",
                    Trigger::KeyPress(2),
                    Action::Shortcut {
                        modifiers: vec![],
                        key: "a".into(),
                    },
                )
                .unwrap();
            store.persist().unwrap();
        }) else {
            return;
        };
        let before = tree(&harness.roots.data_dir);
        assert!(!before.is_empty());
        harness.artwork_sent();

        harness.connect(&PLUS_XL);
        harness.wait_for_model(&PLUS_XL);
        let runtime = harness.session.runtime();
        assert_eq!(runtime.layers().borrow().root(), harness.root(&PLUS_XL));
        assert_eq!(
            harness.root(&PLUS_XL),
            harness.roots.data_dir.join("devices/plus-xl")
        );
        assert_eq!(
            runtime
                .artwork_ui()
                .store
                .borrow()
                .as_ref()
                .unwrap()
                .model()
                .id,
            "plus-xl"
        );
        assert!(runtime
            .layers()
            .borrow()
            .binding("Base", &Trigger::KeyPress(2))
            .is_none());

        // The device gets the XL's own 36 key pictures and nothing else first.
        let sent = harness.artwork_sent();
        let Some(ArtworkCommand::Replace(commands)) = sent.first() else {
            panic!("a full snapshot is sent first: {sent:?}");
        };
        assert!(commands.iter().any(|command| matches!(
            command,
            ArtworkCommand::Clear {
                target: ArtworkTarget::Keypad(36)
            }
        )));

        // A binding saved here lands in the XL root only.
        harness.bind_and_save(20);
        let xl = harness.root(&PLUS_XL);
        let plus_root = harness.roots.data_dir.clone();
        wait_until("the XL binding on disk", {
            let xl = xl.clone();
            move || stored_binding(&xl, &PLUS_XL, 20)
        });
        assert_eq!(tree(&plus_root), before, "the + files stay byte-identical");

        // Switching back restores the + data.
        harness.connect(&PLUS);
        harness.wait_for_model(&PLUS);
        let layers = runtime.layers().borrow();
        assert!(layers.binding("Base", &Trigger::KeyPress(2)).is_some());
        assert_eq!(layers.root(), plus_root);
        drop(layers);
        assert_eq!(tree(&plus_root), before);
    });
}

#[test]
fn connecting_the_model_already_shown_changes_nothing_and_resends_pictures() {
    run_gtk_test(|| {
        let Some(harness) = start("same", |_| {}) else {
            return;
        };
        harness.artwork_sent();
        let generation = harness.session.runtime().key_hold_generation();
        harness.connect(&PLUS);
        let session = harness.session.clone();
        wait_until("the connection to be shown", move || {
            session.last_connected().is_some()
        });
        assert_eq!(harness.model().id, "plus");
        assert_eq!(harness.session.runtime().key_hold_generation(), generation);
        assert!(matches!(
            harness.artwork_sent().first(),
            Some(ArtworkCommand::Replace(_))
        ));
        assert!(!harness.session.picker().dropdown.is_sensitive());
    });
}

#[test]
fn preview_as_is_enabled_without_a_device_and_switches_the_model() {
    run_gtk_test(|| {
        let Some(harness) = start("preview", |_| {}) else {
            return;
        };
        let dropdown = harness.session.picker().dropdown.clone();
        assert!(dropdown.is_sensitive());
        assert_eq!(
            dropdown.tooltip_text().as_deref(),
            Some("Show the configuration of another Stream Deck model while none is connected")
        );
        harness.preview_as(&PLUS_XL);
        assert_eq!(harness.model().id, "plus-xl");
        assert_eq!(
            std::fs::read_to_string(&harness.roots.preference).unwrap(),
            r#"{"model":"plus-xl"}"#.to_owned() + "\n",
            "the choice is remembered once the session opened"
        );
        harness.preview_as(&PLUS);
        assert_eq!(harness.model().id, "plus");
    });
}

#[test]
fn preview_as_sends_no_pictures_to_a_device_that_is_not_there() {
    run_gtk_test(|| {
        let Some(harness) = start("preview-quiet", |_| {}) else {
            return;
        };
        harness.artwork_sent();
        harness.preview_as(&PLUS_XL);
        assert!(harness.artwork_sent().is_empty());
    });
}

#[test]
fn preview_as_is_disabled_with_an_explanation_while_a_device_is_connected() {
    run_gtk_test(|| {
        let Some(harness) = start("preview-locked", |_| {}) else {
            return;
        };
        harness.connect(&PLUS_XL);
        harness.wait_for_model(&PLUS_XL);
        let dropdown = harness.session.picker().dropdown.clone();
        assert!(!dropdown.is_sensitive());
        assert_eq!(
            dropdown.tooltip_text().as_deref(),
            Some("Using the connected Stream Deck + XL")
        );
        // Even a programmatic choice cannot override the hardware.
        harness.preview_as(&PLUS);
        assert_eq!(harness.model().id, "plus-xl");
        assert_eq!(dropdown.selected(), 1);
        // Unplugging keeps the last model shown and unlocks the choice.
        harness
            .events
            .send(MonitorEvent::Connection(ConnectionState::Disconnected(
                "unplugged".into(),
            )))
            .unwrap();
        let unlocked = dropdown.clone();
        wait_until("the picker to unlock", move || unlocked.is_sensitive());
        assert_eq!(harness.model().id, "plus-xl");
        let status = header_label(&harness.window, "status");
        assert_eq!(status.text(), "Disconnected");
        let detail = header_label(&harness.window, "muted");
        assert!(detail.text().contains("Stream Deck + XL disconnected"));
    });
}

#[test]
fn preview_as_asks_before_discarding_a_dirty_draft() {
    run_gtk_test(|| {
        let Some(harness) = start("preview-dirty", |_| {}) else {
            return;
        };
        harness.make_editor_dirty();
        harness.preview_as(&PLUS_XL);
        assert_eq!(
            harness.model().id,
            "plus",
            "nothing changes before the answer"
        );
        let dialogs = harness.dialogs();
        assert_eq!(dialogs.len(), 1);
        assert_eq!(
            dialogs[0].text().as_deref(),
            Some("Discard unsaved changes?")
        );
        // Keeping the edits puts the picker back on the shown model.
        dialogs[0].response(gtk::ResponseType::Cancel);
        assert_eq!(harness.model().id, "plus");
        assert_eq!(harness.session.picker().dropdown.selected(), 0);
        assert!(harness.session.runtime().binding_ui().is_dirty());

        harness.preview_as(&PLUS_XL);
        let dialogs = harness.dialogs();
        assert_eq!(dialogs.len(), 1);
        dialogs[0].response(gtk::ResponseType::Accept);
        assert_eq!(harness.model().id, "plus-xl");
        assert!(!harness.session.runtime().binding_ui().is_dirty());
    });
}

#[test]
fn a_device_switch_discards_the_draft_without_asking_and_says_so() {
    run_gtk_test(|| {
        let Some(harness) = start("device-dirty", |_| {}) else {
            return;
        };
        harness.make_editor_dirty();
        harness.connect(&PLUS_XL);
        harness.wait_for_model(&PLUS_XL);
        assert!(harness.dialogs().is_empty(), "the hardware does not ask");
        assert!(!harness.session.runtime().binding_ui().is_dirty());
        assert_eq!(
            harness.session.toast().label.text(),
            "Switched to Stream Deck + XL; unsaved changes were discarded."
        );
    });
}

#[test]
fn a_switch_cancels_key_holds_timers_and_the_pending_undo() {
    run_gtk_test(|| {
        let Some(harness) = start("cancel", |_| {}) else {
            return;
        };
        let runtime = harness.session.runtime();
        runtime.execute(
            "timer".into(),
            Action::Feature(sdeck_plus::features::FeatureAction::Timer { seconds: 60 }),
        );
        assert!(runtime.timer_active());
        runtime.binding_ui().undo_remove.set_visible(true);
        *runtime.binding_ui().removed.borrow_mut() = Some(Vec::new());
        let generation = runtime.key_hold_generation();

        harness.connect(&PLUS_XL);
        harness.wait_for_model(&PLUS_XL);
        assert!(!runtime.timer_active());
        assert!(!runtime.page_return_active());
        assert!(runtime.key_hold_generation() != generation);
        assert!(!runtime.binding_ui().undo_remove.is_visible());
        assert!(runtime.binding_ui().removed.borrow().is_none());
    });
}

#[test]
fn a_save_queued_before_a_switch_still_lands_in_the_old_root() {
    run_gtk_test(|| {
        let Some(harness) = start("pending-persist", |_| {}) else {
            return;
        };
        let plus = harness.roots.data_dir.clone();
        harness.bind_and_save(3);
        // The switch is requested before the worker has necessarily run.
        harness.connect(&PLUS_XL);
        harness.wait_for_model(&PLUS_XL);
        wait_until("the queued save", {
            let plus = plus.clone();
            move || stored_binding(&plus, &PLUS, 3)
        });
        assert!(!stored_binding(&harness.root(&PLUS_XL), &PLUS_XL, 3));
    });
}

#[test]
fn a_failed_open_keeps_the_previous_session_and_sends_nothing_to_the_device() {
    run_gtk_test(|| {
        let Some(harness) = start("failed-open", |roots| {
            // A read-only parent: the XL root cannot be created.
            std::fs::create_dir_all(roots.data_dir.join("devices")).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                roots.data_dir.join("devices"),
                std::fs::Permissions::from_mode(0o500),
            )
            .unwrap();
        }) else {
            return;
        };
        harness.artwork_sent();
        harness.connect(&PLUS_XL);
        let session = harness.session.clone();
        wait_until("the failure to be reported", move || {
            session
                .runtime()
                .status
                .text()
                .starts_with("Could not open the configuration for Stream Deck + XL:")
        });
        let runtime = harness.session.runtime();
        assert_eq!(harness.model().id, "plus", "the previous session stays");
        assert!(!runtime.editing_enabled(), "editing is off");
        assert!(
            !harness.session.accepts_input(),
            "+ bindings never run on the XL"
        );
        assert!(
            harness.artwork_sent().is_empty(),
            "no picture reaches the device"
        );
        assert!(
            !harness.roots.preference.exists(),
            "the model is not remembered after a failure"
        );
        assert!(harness.dialogs().iter().any(|dialog| dialog
            .text()
            .is_some_and(|text| text.contains("Stream Deck + XL"))));

        // The next connection event retries.
        fs_set_writable(&harness.roots.data_dir.join("devices")).unwrap();
        harness.connect(&PLUS_XL);
        harness.wait_for_model(&PLUS_XL);
        assert!(runtime.editing_enabled());
        assert!(harness.session.accepts_input());
        assert_eq!(
            std::fs::read_to_string(&harness.roots.preference).unwrap(),
            r#"{"model":"plus-xl"}"#.to_owned() + "\n"
        );
    });
}

#[test]
fn a_failed_preview_keeps_the_session_and_does_not_remember_the_choice() {
    run_gtk_test(|| {
        let Some(harness) = start("failed-preview", |roots| {
            std::fs::create_dir_all(roots.data_dir.join("devices")).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                roots.data_dir.join("devices"),
                std::fs::Permissions::from_mode(0o500),
            )
            .unwrap();
        }) else {
            return;
        };
        harness.preview_as(&PLUS_XL);
        assert_eq!(harness.model().id, "plus");
        assert_eq!(harness.session.picker().dropdown.selected(), 0);
        assert!(harness.session.runtime().editing_enabled());
        assert!(!harness.roots.preference.exists());
        assert!(harness
            .session
            .toast()
            .label
            .text()
            .starts_with("Could not open the configuration for Stream Deck + XL:"));
    });
}

#[test]
fn a_switch_waits_for_a_running_configuration_operation() {
    run_gtk_test(|| {
        let Some(harness) = start("deferred", |_| {}) else {
            return;
        };
        let runtime = harness.session.runtime();
        let bundle = harness.base.join("plus.sdeck-plus.json");
        runtime.export_configuration(bundle.clone());
        assert!(!runtime.can_switch_model());
        // The device announces itself first, as it does in production.
        harness
            .session
            .connection_changed(&ConnectionState::Connected(&PLUS_XL));
        assert_eq!(harness.model().id, "plus");
        assert_eq!(harness.model().id, "plus");
        // Once the export finishes the deferred switch runs.
        harness.wait_for_model(&PLUS_XL);
        assert!(bundle.is_file());
    });
}

#[test]
fn twenty_alternating_switches_leave_no_widgets_handlers_or_workers_behind() {
    run_gtk_test(|| {
        let Some(harness) = start("churn", |_| {}) else {
            return;
        };
        let runtime = harness.session.runtime().clone();
        let widgets = count_widgets(harness.window.upcast_ref());
        // Readers of earlier tests' windows may still be around; count from here.
        let readers = live_threads("info-reader");
        let mut actions = harness.app.list_actions();
        actions.sort();
        let strong = Rc::strong_count(runtime.layers());
        let owners = Rc::strong_count(&runtime.artwork_ui().chooser_owners);
        for round in 0..20 {
            let model = if round % 2 == 0 { &PLUS_XL } else { &PLUS };
            harness.connect(model);
            harness.wait_for_model(model);
        }
        assert_eq!(harness.model().id, "plus");
        assert_eq!(count_widgets(harness.window.upcast_ref()), widgets);
        let mut after = harness.app.list_actions();
        after.sort();
        assert_eq!(
            after, actions,
            "no application action is added or duplicated"
        );
        assert!(after.iter().any(|name| name == "check-updates-on-open"));
        assert_eq!(Rc::strong_count(runtime.layers()), strong);
        assert_eq!(
            Rc::strong_count(&runtime.artwork_ui().chooser_owners),
            owners
        );
        // Every replaced information reader ends once its sender is dropped.
        wait_until("old information readers to end", move || {
            live_threads("info-reader") <= readers
        });
        // The update controller keeps working: its action is still the one installed.
        harness.app.activate_action("check-updates-on-open", None);
    });
}

#[test]
fn an_xl_session_exports_its_own_root_as_an_xl_bundle() {
    run_gtk_test(|| {
        let Some(harness) = start("export-xl", |roots| remember(roots, &PLUS_XL)) else {
            return;
        };
        harness.bind_and_save(30);
        let bundle = harness.base.join("xl.sdeck-plus.json");
        harness
            .session
            .runtime()
            .export_configuration(bundle.clone());
        let session = harness.session.clone();
        wait_until("the export to finish", move || {
            session
                .runtime()
                .status
                .text()
                .starts_with("Configuration exported to")
        });
        let exported: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&bundle).unwrap()).unwrap();
        assert_eq!(exported["version"], 5);
        assert_eq!(exported["model"], "plus-xl");
        assert!(stored_binding(&harness.root(&PLUS_XL), &PLUS_XL, 30));
    });
}

#[test]
fn an_xl_session_imports_an_xl_bundle_and_refuses_a_plus_bundle() {
    run_gtk_test(|| {
        let Some(harness) = start("import-xl", |roots| remember(roots, &PLUS_XL)) else {
            return;
        };
        let runtime = harness.session.runtime();
        harness.bind_and_save(31);
        let bundle = harness.base.join("xl.sdeck-plus.json");
        runtime.export_configuration(bundle.clone());
        let session = harness.session.clone();
        wait_until("the export to finish", move || {
            session
                .runtime()
                .status
                .text()
                .starts_with("Configuration exported to")
        });

        // A Stream Deck + bundle names the + and is refused, changing nothing.
        let plus_bundle = harness.base.join("plus.sdeck-plus.json");
        sdeck_plus::config_bundle::export_for(
            &{
                let root = harness.base.join("scratch-plus");
                LayerStore::open_at(&root).unwrap().persist().unwrap();
                root
            },
            &plus_bundle,
            &PLUS,
        )
        .unwrap();
        runtime.import_configuration(plus_bundle);
        let session = harness.session.clone();
        wait_until("the refusal", move || {
            session
                .runtime()
                .status
                .text()
                .starts_with("Could not import configuration:")
        });
        assert!(runtime
            .status
            .text()
            .contains("Stream Deck +; connect or preview that model"));
        assert!(runtime.editing_enabled());

        // The XL bundle imports into the XL root and reloads there.
        runtime.import_configuration(bundle);
        let session = harness.session.clone();
        wait_until("the import", move || {
            session
                .runtime()
                .status
                .text()
                .starts_with("Configuration imported")
        });
        assert_eq!(runtime.layers().borrow().root(), harness.root(&PLUS_XL));
        assert!(runtime
            .layers()
            .borrow()
            .binding("Base", &Trigger::KeyPress(31))
            .is_some());
    });
}

#[test]
fn the_header_uses_the_connected_models_own_wording() {
    run_gtk_test(|| {
        let Some(harness) = start("header", |_| {}) else {
            return;
        };
        let detail = header_label(&harness.window, "muted");
        assert!(detail.text().contains("Stream Deck + or Stream Deck + XL"));
        harness.connect(&PLUS_XL);
        harness.wait_for_model(&PLUS_XL);
        let shown = detail.clone();
        wait_until("the connected text", move || {
            shown.text().starts_with("Stream Deck + XL connected")
        });
        assert_eq!(
            detail.tooltip_text().as_deref(),
            Some("USB device VID 0FD9 / PID 00C6 · HID input and artwork active")
        );
    });
}

#[test]
fn a_deferred_device_switch_is_dropped_when_the_device_flips_back() {
    run_gtk_test(|| {
        let Some(h) = start("stale-deferred", |_| {}) else {
            return;
        };
        let runtime = h.session.runtime();
        runtime.export_configuration(h.base.join("p.sdeck-plus.json"));
        assert!(!runtime.can_switch_model());
        h.session
            .connection_changed(&ConnectionState::Connected(&PLUS_XL));
        h.session
            .connection_changed(&ConnectionState::Connected(&PLUS));
        let s = h.session.clone();
        wait_until("export", move || {
            s.runtime()
                .status
                .text()
                .starts_with("Configuration exported to")
        });
        // Let the polling tick run retry_deferred; the signal is its pass count.
        let ticks = Rc::new(Cell::new(0));
        let counter = ticks.clone();
        let source = glib::timeout_add_local(Duration::from_millis(40), move || {
            counter.set(counter.get() + 1);
            glib::ControlFlow::Continue
        });
        wait_until("two polling ticks", {
            let ticks = ticks.clone();
            move || ticks.get() >= 3
        });
        source.remove();
        assert_eq!(h.model().id, "plus");
        assert!(h.session.accepts_input());
    });
}

#[test]
fn a_plus_session_refuses_an_xl_bundle_and_keeps_its_files() {
    run_gtk_test(|| {
        let Some(h) = start("plus-refuses-xl", |roots| {
            std::fs::create_dir_all(roots.data_dir.join("devices/plus-xl")).unwrap();
            std::fs::write(roots.data_dir.join("devices/plus-xl/marker"), b"keep").unwrap();
        }) else {
            return;
        };
        let xl_root = h.base.join("scratch-xl");
        LayerStore::open_at_model(&xl_root, &PLUS_XL)
            .unwrap()
            .persist()
            .unwrap();
        let bundle = h.base.join("xl.sdeck-plus.json");
        sdeck_plus::config_bundle::export_for(&xl_root, &bundle, &PLUS_XL).unwrap();
        let before = tree(&h.roots.data_dir);
        let runtime = h.session.runtime();
        runtime.import_configuration(bundle);
        let s = h.session.clone();
        wait_until("the refusal", move || {
            s.runtime()
                .status
                .text()
                .starts_with("Could not import configuration:")
        });
        assert!(runtime.status.text().contains(
            "This configuration is for Stream Deck + XL; connect or preview that model to import it."
        ));
        assert_eq!(tree(&h.roots.data_dir), before);
        assert_eq!(
            std::fs::read(h.roots.data_dir.join("devices/plus-xl/marker")).unwrap(),
            b"keep"
        );
    });
}

#[test]
fn a_plus_import_preserves_the_devices_directory() {
    run_gtk_test(|| {
        let Some(h) = start("plus-import-devices", |roots| {
            std::fs::create_dir_all(roots.data_dir.join("devices/plus-xl")).unwrap();
            std::fs::write(roots.data_dir.join("devices/plus-xl/marker"), b"keep").unwrap();
        }) else {
            return;
        };
        let source = h.base.join("scratch-plus");
        LayerStore::open_at(&source).unwrap().persist().unwrap();
        let bundle = h.base.join("plus.sdeck-plus.json");
        sdeck_plus::config_bundle::export_for(&source, &bundle, &PLUS).unwrap();
        let runtime = h.session.runtime();
        runtime.import_configuration(bundle);
        let s = h.session.clone();
        wait_until("the import", move || {
            s.runtime()
                .status
                .text()
                .starts_with("Configuration imported")
        });
        assert_eq!(
            std::fs::read(h.roots.data_dir.join("devices/plus-xl/marker")).unwrap(),
            b"keep"
        );
    });
}

#[test]
fn the_updates_entry_opens_exactly_one_dialog_even_after_switches() {
    run_gtk_test(|| {
        let Some(h) = start("one-updates", |_| {}) else {
            return;
        };
        fn find_button_named(widget: &gtk::Widget, caption: &str) -> Option<Button> {
            if let Some(button) = widget.downcast_ref::<Button>() {
                if button_caption(button).as_deref() == Some(caption) {
                    return Some(button.clone());
                }
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                if let Some(found) = find_button_named(&current, caption) {
                    return Some(found);
                }
                child = current.next_sibling();
            }
            None
        }
        fn find_updates(widget: &gtk::Widget) -> Option<Button> {
            if let Some(menu) = widget.downcast_ref::<gtk::MenuButton>() {
                if let Some(popover) = menu.popover() {
                    if let Some(found) = find_button_named(&popover.child()?, "Updates") {
                        return Some(found);
                    }
                }
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                if let Some(found) = find_updates(&current) {
                    return Some(found);
                }
                child = current.next_sibling();
            }
            None
        }
        let entry = find_updates(h.window.upcast_ref()).expect("Updates menu entry");
        let open = || {
            gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Dialog>().ok())
                .filter(|d| d.is_visible() && d.title().as_deref() == Some("Updates"))
                .count()
        };
        for round in 0..3 {
            entry.emit_clicked();
            entry.emit_clicked();
            assert_eq!(open(), 1, "round {round}");
            h.connect(if round % 2 == 0 { &PLUS_XL } else { &PLUS });
            h.wait_for_model(if round % 2 == 0 { &PLUS_XL } else { &PLUS });
        }
        entry.emit_clicked();
        assert_eq!(open(), 1);
    });
}
