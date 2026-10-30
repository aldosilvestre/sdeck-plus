use super::*;
use gtk::gio;

#[test]
fn timer_completion_notifies_once_while_hidden_and_preserves_cancel_restart() {
    // A private session bus keeps the mock and notifications off the user's desktop.
    const CHILD: &str = "SDECK_TIMER_NOTIFICATION_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let result = std::process::Command::new("dbus-run-session")
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args([
                "tests::timer_notification_tests::timer_completion_notifies_once_while_hidden_and_preserves_cancel_restart",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .env("GTK_A11Y", "none")
            .env("GIO_USE_VFS", "local")
            .env("GNOTIFICATION_BACKEND", "freedesktop")
            .status()
            .expect("isolated D-Bus test runner");
        assert!(
            result.success(),
            "isolated notification test failed: {result}"
        );
        return;
    }
    run_gtk_test(assert_timer_notification);
}

#[test]
fn mock_timer_chime() {
    let Some(path) = std::env::var_os("SDECK_TIMER_SOUND_LOG") else {
        return;
    };
    use std::io::Write;
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(log, "chime").unwrap();
}

fn assert_timer_notification() {
    let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).unwrap();
    connection
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            Some(&("org.freedesktop.Notifications", 0u32).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        )
        .unwrap();
    let node = gio::DBusNodeInfo::for_xml(
        r#"<node><interface name="org.freedesktop.Notifications">
        <method name="GetCapabilities"><arg type="as" direction="out"/></method>
        <method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/></method>
        <method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method>
        </interface></node>"#,
    )
    .unwrap();
    let notices = Rc::new(RefCell::new(Vec::new()));
    let received = notices.clone();
    let loop_ = glib::MainLoop::new(None, false);
    let notified = loop_.clone();
    let registration = connection
        .register_object("/org/freedesktop/Notifications", &node.interfaces()[0])
        .method_call(
            move |_, _, _, _, method, parameters, invocation| match method {
                "GetCapabilities" => invocation.return_value(Some(&(vec!["body"],).to_variant())),
                "GetServerInformation" => {
                    invocation.return_value(Some(&("test", "sdeck-plus", "1", "1.2").to_variant()))
                }
                "Notify" => {
                    received.borrow_mut().push((
                        parameters.child_get::<String>(3),
                        parameters.child_get::<String>(4),
                    ));
                    invocation.return_value(Some(&(1u32,).to_variant()));
                    notified.quit();
                }
                _ => panic!("unexpected notification method {method}"),
            },
        )
        .build()
        .unwrap();
    let app = Application::builder()
        .application_id("dev.streamdeck.TimerNotificationTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let (root, store, images, _) = artwork_fixture("timer-notification");
    let layers = Rc::new(RefCell::new(store));
    let (output, _hardware) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(images))),
        layers: layers.clone(),
        pictures: Rc::default(),
        output,
        last_layer: Rc::default(),
        chooser_owners: Rc::default(),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::default(),
        info: Rc::default(),
    };
    let (commands, _requests) = mpsc::channel();
    let window = ApplicationWindow::builder().application(&app).build();
    window.set_hide_on_close(true);
    let widgets = build_widgets(window.clone(), layers, artwork, commands);
    window.set_child(Some(&widgets.root));
    window.present();
    wait_for_frame(&window);
    let runtime = &widgets.feature_runtime;
    let sound_log = root.join("chimes");
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::NONE);
    launcher.setenv("SDECK_TIMER_SOUND_LOG", &sound_log, true);
    let mock_args = vec![
        std::env::current_exe().unwrap().into_os_string(),
        "--exact".into(),
        "tests::timer_notification_tests::mock_timer_chime".into(),
        "--test-threads=1".into(),
    ];
    runtime.set_timer_sound_for_test(launcher, mock_args);
    let start = |seconds| {
        runtime.execute(
            "timer-test".into(),
            Action::Feature(sdeck_plus::features::FeatureAction::Timer { seconds }),
        );
    };
    start(1);
    runtime.execute(
        "cancel-test".into(),
        Action::Feature(sdeck_plus::features::FeatureAction::CancelTimer),
    );
    assert!(!runtime.timer_active());
    assert_eq!(runtime.timer_status.text(), "Timer cancelled.");
    start(60);
    assert_eq!(runtime.timer_status.text(), "Timer: 60s remaining");
    start(1);
    start(2);
    window.close();
    assert!(!window.is_visible());
    assert!(runtime.timer_active());

    let timed_out = Rc::new(Cell::new(false));
    let timeout_flag = timed_out.clone();
    let timeout_loop = loop_.clone();
    let timeout = glib::timeout_add_local_once(Duration::from_secs(4), move || {
        timeout_flag.set(true);
        timeout_loop.quit();
    });
    loop_.run();
    if !timed_out.get() {
        timeout.remove();
    }
    // Await the mock's real write, not an assumed subprocess startup delay.
    let played = glib::MainLoop::new(None, false);
    let done = played.clone();
    let log = sound_log.clone();
    let sound_ready = Rc::new(Cell::new(false));
    let ready = sound_ready.clone();
    let watcher = glib::idle_add_local(move || {
        if std::fs::read_to_string(&log).is_ok_and(|text| !text.is_empty()) {
            ready.set(true);
            done.quit();
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    let guard_loop = played.clone();
    let guard = glib::timeout_add_local_once(Duration::from_secs(4), move || guard_loop.quit());
    played.run();
    if sound_ready.get() {
        guard.remove();
    } else {
        watcher.remove();
    }
    let actual = notices.borrow().clone();
    let sounds = std::fs::read_to_string(&sound_log).unwrap_or_default();
    runtime.cancel_timer();
    window.destroy();
    connection.unregister_object(registration).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(
        sounds, "chime\n",
        "only the completed timer must play one chime"
    );
    assert_eq!(runtime.timer_status.text(), "Timer complete.");
    assert!(!runtime.timer_active());
    assert!(runtime.timer_status.has_css_class("feedback-success"));
    assert_eq!(
        actual,
        vec![(
            "Timer complete".into(),
            "Your 2-second timer has finished.".into()
        )],
        "completion must deliver exactly one native notification, including while hidden"
    );
}
