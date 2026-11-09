use gtk::{gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct TestTray;
impl ksni::Tray for TestTray {
    fn id(&self) -> String {
        "isolated-startup-test".into()
    }
}

// Every test re-execs on a private bus, including when run by the ordinary suite.
fn isolated(test: fn()) {
    const CHILD: &str = "SDECK_TRAY_STARTUP_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new("dbus-run-session")
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args([
                std::thread::current().name().unwrap(),
                "--exact",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(result.success(), "isolated tray test failed: {result}");
        return;
    }
    let context = glib::MainContext::new();
    context.with_thread_default(test).unwrap();
}

fn receive<T>(receiver: &async_channel::Receiver<T>) -> T {
    glib::MainContext::ref_thread_default().block_on(async {
        glib::future_with_timeout(Duration::from_secs(4), receiver.recv())
            .await
            .expect("bounded tray result")
            .unwrap()
    })
}

struct Watcher {
    connection: gio::DBusConnection,
    registration: Option<gio::RegistrationId>,
    calls: Rc<Cell<u32>>,
    entered: async_channel::Receiver<()>,
    pending: Rc<RefCell<Vec<gio::DBusMethodInvocation>>>,
}
impl Watcher {
    fn new(delayed_host: bool, reject: bool, hang: bool) -> Self {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).unwrap();
        connection
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some(&("org.kde.StatusNotifierWatcher", 0u32).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                1000,
                gio::Cancellable::NONE,
            )
            .unwrap();
        let node = gio::DBusNodeInfo::for_xml(
            r#"<node><interface name="org.kde.StatusNotifierWatcher">
            <method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
            <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
            </interface></node>"#,
        )
        .unwrap();
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let (entered_tx, entered) = async_channel::unbounded();
        let pending = Rc::new(RefCell::new(Vec::new()));
        let waiting = pending.clone();
        let queries = Cell::new(0);
        let registration = connection
            .register_object("/StatusNotifierWatcher", &node.interfaces()[0])
            .method_call(move |_, _, _, _, method, parameters, invocation| {
                assert_eq!(method, "RegisterStatusNotifierItem");
                assert!(parameters
                    .child_get::<String>(0)
                    .starts_with("org.kde.StatusNotifierItem-"));
                count.set(count.get() + 1);
                entered_tx.try_send(()).unwrap();
                if hang {
                    waiting.borrow_mut().push(invocation);
                    return;
                }
                if reject && count.get() == 1 {
                    invocation.return_dbus_error("org.freedesktop.DBus.Error.Failed", "not ready");
                } else {
                    invocation.return_value(None);
                }
            })
            .property(move |_, _, _, _, property| {
                assert_eq!(property, "IsStatusNotifierHostRegistered");
                queries.set(queries.get() + 1);
                (!delayed_host || queries.get() > 1).to_variant()
            })
            .build()
            .unwrap();
        Self {
            connection,
            registration: Some(registration),
            calls,
            entered,
            pending,
        }
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        self.connection
            .unregister_object(self.registration.take().unwrap())
            .unwrap();
        self.connection
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "ReleaseName",
                Some(&("org.kde.StatusNotifierWatcher",).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                1000,
                gio::Cancellable::NONE,
            )
            .unwrap();
    }
}

#[test]
fn no_tray_waits_until_deadline_then_reports_unavailable() {
    isolated(|| {
        let grace = Duration::from_millis(200);
        let began = Instant::now();
        let (service, receiver) = crate::tray_startup::Service::start(TestTray, grace).unwrap();
        assert!(receive(&receiver).is_err());
        assert!(
            began.elapsed() >= grace,
            "missing tray must get the readiness grace"
        );
        drop(service);
    });
}

#[test]
fn watcher_without_host_retries_until_host_is_ready() {
    isolated(|| {
        let watcher = Watcher::new(true, false, false);
        let (service, receiver) =
            crate::tray_startup::Service::start(TestTray, Duration::from_secs(2)).unwrap();
        let handle = receive(&receiver).expect("late host must register");
        assert!(watcher.calls.get() >= 2);
        assert!(!handle.is_closed());
        drop(service);
        assert!(
            handle.is_closed(),
            "shutdown must stop the registered service"
        );
    });
}

#[test]
fn ready_tray_preserves_immediate_registration() {
    isolated(|| {
        let watcher = Watcher::new(false, false, false);
        let (service, receiver) =
            crate::tray_startup::Service::start(TestTray, Duration::from_secs(2)).unwrap();
        let handle = receive(&receiver).unwrap();
        assert_eq!(watcher.calls.get(), 1);
        assert!(!handle.is_closed());
        drop(service);
        assert!(handle.is_closed());
    });
}

#[test]
fn rejected_registration_is_not_a_readiness_receipt() {
    isolated(|| {
        let watcher = Watcher::new(false, true, false);
        let (service, receiver) =
            crate::tray_startup::Service::start(TestTray, Duration::from_secs(2)).unwrap();
        let handle = receive(&receiver).unwrap();
        assert_eq!(watcher.calls.get(), 2);
        drop(service);
        assert!(handle.is_closed());
    });
}

#[test]
fn hung_registration_obeys_overall_deadline_and_ignores_late_reply() {
    isolated(|| {
        let watcher = Watcher::new(false, false, true);
        let began = Instant::now();
        let (service, receiver) =
            crate::tray_startup::Service::start(TestTray, Duration::from_millis(200)).unwrap();
        receive(&watcher.entered);
        assert!(
            receiver.is_empty(),
            "GTK context is responsive while registration waits"
        );
        assert!(receive(&receiver).is_err());
        assert!(began.elapsed() >= Duration::from_millis(200));
        drop(service);
        for invocation in watcher.pending.borrow_mut().drain(..) {
            invocation.return_value(None);
        }
        assert!(receiver.is_closed());
        assert!(receiver.is_empty(), "late reply cannot resurrect readiness");
    });
}

#[test]
fn shutdown_cancels_pending_registration_without_waiting_for_deadline() {
    isolated(|| {
        let watcher = Watcher::new(false, false, true);
        let (service, receiver) =
            crate::tray_startup::Service::start(TestTray, Duration::from_secs(10)).unwrap();
        receive(&watcher.entered);
        let began = Instant::now();
        drop(service);
        assert!(began.elapsed() < Duration::from_secs(1));
        assert!(receiver.is_closed());
        assert!(receiver.is_empty());
        for invocation in watcher.pending.borrow_mut().drain(..) {
            invocation.return_value(None);
        }
    });
}

#[test]
fn delayed_watcher_is_registered_after_the_first_failed_attempt() {
    isolated(|| {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).unwrap();
        let watcher = Rc::new(RefCell::new(None));
        let install = watcher.clone();
        // A failed attempt relinquishes its item name. Start the tray only then,
        // rather than guessing how long the first registration might take.
        let subscription = connection.signal_subscribe(
            Some("org.freedesktop.DBus"),
            Some("org.freedesktop.DBus"),
            Some("NameOwnerChanged"),
            Some("/org/freedesktop/DBus"),
            None,
            gio::DBusSignalFlags::NONE,
            move |_, _, _, _, _, parameters| {
                let (name, _, owner) = parameters.get::<(String, String, String)>().unwrap();
                if name.starts_with("org.kde.StatusNotifierItem-")
                    && owner.is_empty()
                    && install.borrow().is_none()
                {
                    *install.borrow_mut() = Some(Watcher::new(false, false, false));
                }
            },
        );
        let (service, receiver) =
            crate::tray_startup::Service::start(TestTray, Duration::from_secs(2)).unwrap();
        let handle = receive(&receiver).expect("delayed watcher must be registered");
        assert_eq!(watcher.borrow().as_ref().unwrap().calls.get(), 1);
        connection.signal_unsubscribe(subscription);
        drop(service);
        assert!(handle.is_closed());
    });
}

#[test]
fn production_install_handles_absent_pending_and_late_trays_without_changing_preferences() {
    isolated(|| {
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            eprintln!("production tray lifecycle skipped: no display");
            return;
        }
        struct State(std::path::PathBuf);
        impl Drop for State {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let state = State(std::env::temp_dir().join(format!(
            "sdeck-tray-lifecycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        std::fs::create_dir(&state.0).unwrap();
        // Only this exact-test child sees these environment changes.
        std::env::set_var("HOME", &state.0);
        std::env::set_var("XDG_CONFIG_HOME", state.0.join("config"));
        std::env::set_var("XDG_DATA_HOME", state.0.join("data"));
        let preference = crate::tray_preference::path().unwrap();
        crate::tray_preference::save(&preference, true).unwrap();
        let original = std::fs::read(&preference).unwrap();
        super::run_gtk_test(|| {
            for mode in ["absent", "pending", "late"] {
                let app = gtk::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
                let window = Rc::new(RefCell::new(None::<gtk::ApplicationWindow>));
                let created = window.clone();
                app.connect_startup(move |app| {
                    crate::app_icon::register().unwrap();
                    crate::tray::install(app).unwrap();
                    let window = gtk::ApplicationWindow::builder().application(app).build();
                    window.set_widget_name("configuration");
                    crate::configure_window_close(&window, app);
                    *created.borrow_mut() = Some(window);
                });
                let watcher = Rc::new(RefCell::new(None));
                let install = watcher.clone();
                app.connect_activate(move |app| {
                    if mode != "absent" {
                        crate::build_ui(app);
                    }
                    if mode == "late" {
                        *install.borrow_mut() = Some(Watcher::new(false, false, false));
                    }
                });
                let satisfied = Rc::new(Cell::new(false));
                let observed = satisfied.clone();
                let polled = window.clone();
                let polling_app = app.clone();
                let poll = glib::timeout_add_local(Duration::from_millis(10), move || {
                    if let Some(window) = polled.borrow().as_ref() {
                        let ready = crate::keep_running_on_close(&polling_app);
                        if mode == "late" && ready {
                            observed.set(window.is_visible());
                            polling_app.quit();
                        } else if mode != "late" && window.is_visible() {
                            observed.set(!ready);
                            // Closing an unpresented synthetic window does not quit.
                            window.present();
                            window.close();
                        }
                    }
                    glib::ControlFlow::Continue
                });
                let expired = Rc::new(Cell::new(false));
                let timed_out = expired.clone();
                let guarded_app = app.clone();
                let guard = glib::timeout_add_local(Duration::from_secs(12), move || {
                    timed_out.set(true);
                    guarded_app.quit();
                    glib::ControlFlow::Continue
                });
                // Independent of GTK dispatch, so a stalled main loop is bounded too.
                let (cancel, cancelled) = std::sync::mpsc::channel();
                let watchdog = std::thread::spawn(move || {
                    if cancelled.recv_timeout(Duration::from_secs(13)).is_err() {
                        eprintln!("production tray lifecycle watchdog expired: {mode}");
                        std::process::exit(1);
                    }
                });
                let began = Instant::now();
                let result = app.run_with_args::<&str>(&[]);
                let elapsed = began.elapsed();
                poll.remove();
                guard.remove();
                cancel.send(()).unwrap();
                watchdog.join().unwrap();
                for window in app.windows() {
                    window.destroy();
                }
                if let Some(window) = window.borrow_mut().take() {
                    window.destroy();
                }
                watcher.borrow_mut().take();
                assert_eq!(result, glib::ExitCode::SUCCESS, "{mode}");
                assert!(!expired.get(), "lifecycle guard expired: {mode}");
                assert!(
                    satisfied.get(),
                    "unexpected visibility/close behavior: {mode}"
                );
                match mode {
                    "absent" => assert!(elapsed >= crate::tray_startup::GRACE),
                    "pending" => assert!(elapsed < Duration::from_secs(1)),
                    _ => assert!(elapsed < crate::tray_startup::GRACE),
                }
            }
        });
        assert_eq!(std::fs::read(preference).unwrap(), original);
    });
}

#[test]
fn pending_close_is_safe_and_late_readiness_preserves_open_configuration() {
    super::run_gtk_test(|| {
        let app = gtk::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
        app.register(gio::Cancellable::NONE).unwrap();
        let action = crate::tray::install_close_preference(&app, false);
        let window = gtk::ApplicationWindow::builder().application(&app).build();
        window.set_widget_name("configuration");
        crate::configure_window_close(&window, &app);
        assert!(!window.hides_on_close());
        assert!(!crate::keep_running_on_close(&app));
        // Existing windows bypass all configuration I/O and device initialization.
        crate::build_ui_with_visibility(&app, false);
        assert!(!window.is_visible());
        crate::build_ui(&app);
        crate::tray::apply_close_preference(&app, &action, true);
        assert!(
            window.is_visible(),
            "late readiness must not re-hide configuration"
        );
        assert!(window.hides_on_close());
        window.close();
        assert!(
            !window.is_visible(),
            "ready tray retains normal hide-on-close"
        );
        window.destroy();
    });
}
