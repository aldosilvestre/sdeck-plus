use super::*;
use crate::update_install::*;
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[test]
fn origin_guard_accepts_only_default_canonical_install() {
    for (exe, home, expected) in [
        (
            Some("/home/test/.local/bin/sdeck-plus"),
            Some("/home/test"),
            Origin::Installer,
        ),
        (
            Some("/usr/bin/sdeck-plus"),
            Some("/home/test"),
            Origin::Package,
        ),
        (Some("/opt/sdeck-plus"), Some("/home/test"), Origin::Other),
        (
            Some("/checkout/target/release/sdeck-plus"),
            Some("/home/test"),
            Origin::Other,
        ),
        (
            Some("/elsewhere/sdeck-plus"),
            Some("/home/test"),
            Origin::Other,
        ),
        (
            Some("/home/test/.local/bin/sdeck-plus"),
            Some("relative"),
            Origin::Other,
        ),
        (
            Some("/home/test/.local/bin/sdeck-plus"),
            None,
            Origin::Other,
        ),
        (None, Some("/home/test"), Origin::Other),
    ] {
        assert_eq!(origin(exe.map(Path::new), home.map(Path::new)), expected);
    }
}

#[test]
fn symlink_origin_uses_resolved_file_not_launch_path() {
    use std::os::unix::fs::symlink;
    let root = std::env::temp_dir().join(format!("sdeck-origin-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".local/bin")).unwrap();
    std::fs::write(root.join("other"), b"not the installer location").unwrap();
    symlink(root.join("other"), root.join(".local/bin/sdeck-plus")).unwrap();
    let resolved = std::fs::canonicalize(root.join(".local/bin/sdeck-plus")).unwrap();
    assert_eq!(origin(Some(&resolved), Some(&root)), Origin::Other);
    std::fs::write(root.join(".local/bin/real"), b"installed file").unwrap();
    let resolved = std::fs::canonicalize(root.join(".local/bin/real")).unwrap();
    assert_eq!(origin(Some(&resolved), Some(&root)), Origin::Other);
    std::fs::remove_dir_all(root).unwrap();
}

#[derive(Default)]
struct Record {
    specs: Vec<Spec>,
    signals: Vec<i32>,
    directory: Option<PathBuf>,
}
struct FakeRunner {
    record: Arc<Mutex<Record>>,
    now: Duration,
    mode: u8,
}
struct FakeProcess {
    record: Arc<Mutex<Record>>,
    code: Option<i32>,
}
impl Process for FakeProcess {
    fn poll(&mut self, tail: &mut Tail) -> io::Result<Option<Outcome>> {
        tail.push(&vec![b'x'; 5000]);
        tail.push(b"\nlast output");
        Ok(self.code.map(|code| {
            if code < 0 {
                Outcome::Signal(-code)
            } else {
                Outcome::Exited(code)
            }
        }))
    }
    fn signal(&mut self, signal: i32) {
        self.record.lock().unwrap().signals.push(signal);
        if signal == libc::SIGKILL {
            self.code = Some(137);
        }
    }
    fn group_alive(&self) -> bool {
        self.code.is_none()
    }
}
impl Runner for FakeRunner {
    fn spawn(&mut self, spec: &Spec) -> io::Result<Box<dyn Process>> {
        let mut record = self.record.lock().unwrap();
        record.specs.push(spec.clone());
        let download = spec.program == Path::new("/usr/bin/curl");
        if download {
            let path = PathBuf::from(&spec.args[spec.args.len() - 2]);
            record.directory = path.parent().map(Path::to_path_buf);
            if self.mode == 5 {
                return Err(io::Error::other("download spawn failed"));
            }
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            std::fs::write(
                path,
                if self.mode == 3 {
                    vec![0; 524289]
                } else {
                    b"fake installer".to_vec()
                },
            )?;
        } else if self.mode == 2 {
            return Err(io::Error::other("spawn failed"));
        }
        Ok(Box::new(FakeProcess {
            record: self.record.clone(),
            code: if (!download && self.mode == 4) || (download && self.mode == 6) {
                None
            } else {
                Some(if !download && self.mode == 8 {
                    -libc::SIGTERM
                } else if !download && self.mode == 1 {
                    7
                } else if download && self.mode == 7 {
                    22
                } else {
                    0
                })
            },
        }))
    }
    fn now(&self) -> Duration {
        self.now
    }
    fn wait(&mut self, cancel: &mpsc::Receiver<()>, duration: Duration) -> bool {
        self.now += duration;
        cancel.try_recv().is_ok()
    }
}
fn fake(mode: u8) -> (FakeRunner, Arc<Mutex<Record>>) {
    let record = Arc::new(Mutex::new(Record::default()));
    (
        FakeRunner {
            record: record.clone(),
            now: Duration::ZERO,
            mode,
        },
        record,
    )
}

#[test]
fn bad_tags_rejected_before_download() {
    for tag in ["v1.2.3;id", "v1.2", "v1.2.3/else", "v1.2.3\n", "1.2.3"] {
        let (mut runner, record) = fake(0);
        let (_, cancel) = mpsc::channel();
        assert!(run_update(&mut runner, tag, &[], &cancel).is_err());
        assert!(record.lock().unwrap().specs.is_empty());
    }
}

#[test]
fn installer_environment_is_allowlisted_not_inherited() {
    let keys = [
        "PATH",
        "HOME",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "LANG",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "PREFIX",
        "SDECK_PLUS_INSTALL_TEST",
        "SDECK_PLUS_RELEASE_API",
        "SDECK_PLUS_ARCHIVE_BASE",
        "CARGO_TARGET_DIR",
        "CARGO_HOME",
        "RUSTFLAGS",
        "BASH_ENV",
        "ENV",
        "LD_PRELOAD",
    ];
    let env = clean_environment(
        keys.into_iter()
            .map(|key| (OsString::from(key), OsString::from("value"))),
    );
    assert_eq!(
        env.iter()
            .map(|(key, _)| key.to_str().unwrap())
            .collect::<Vec<_>>(),
        ["PATH", "HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "LANG"]
    );
}

#[test]
fn outcomes_argv_cleanup_and_bounded_failure_tail() {
    for mode in 0..=8 {
        let (mut runner, record) = fake(mode);
        let (_, cancel) = mpsc::channel();
        let result = run_update(&mut runner, "v1.2.3", &[], &cancel);
        assert_eq!(result.is_ok(), mode == 0, "{result:?}");
        let record = record.lock().unwrap();
        assert!(!record.directory.as_ref().unwrap().exists());
        let download = &record.specs[0];
        assert_eq!(download.program, Path::new("/usr/bin/curl"));
        assert_eq!(
            &download.args[..9],
            [
                "-q",
                "-fsS",
                "--proto",
                "=https",
                "--max-time",
                "30",
                "--max-filesize",
                "524288",
                "--output"
            ]
            .map(OsString::from)
        );
        assert_eq!(
            download.args.last().unwrap(),
            "https://raw.githubusercontent.com/aldosilvestre/sdeck-plus/v1.2.3/install.sh"
        );
        if matches!(mode, 0 | 1 | 2 | 4 | 8) {
            let execute = &record.specs[1];
            assert_eq!(execute.program, Path::new("/bin/bash"));
            assert_eq!(
                execute.args,
                vec![
                    OsString::from("--"),
                    record
                        .directory
                        .as_ref()
                        .unwrap()
                        .join("install.sh")
                        .into_os_string(),
                    OsString::from("--tag"),
                    OsString::from("v1.2.3")
                ]
            );
        }
        if mode == 1 {
            let message = result.as_ref().unwrap_err();
            assert!(message.contains("exit code 7"));
            assert!(message.ends_with("last output"));
            assert!(message.len() < 4200);
        }
        if mode == 8 {
            assert!(result
                .as_ref()
                .unwrap_err()
                .starts_with("Update failed (signal 15)"));
        }
        if matches!(mode, 4 | 6) {
            assert_eq!(record.signals, [libc::SIGTERM, libc::SIGKILL]);
        }
    }
}

#[test]
fn cancellation_before_spawn_leaves_nothing_and_runs_nothing() {
    let (mut runner, record) = fake(4);
    let (stop, cancel) = mpsc::channel();
    stop.send(()).unwrap();
    assert!(run_update(&mut runner, "v1.2.3", &[], &cancel).is_err());
    let record = record.lock().unwrap();
    assert!(record.directory.is_none());
    assert!(record.specs.is_empty());
    assert!(record.signals.is_empty());
}

// Replace only the execution stage with an owned, TERM-resistant short-lived
// child. Readiness is an actual pipe message, never a guessed startup sleep.
struct RealChildRunner {
    fake: FakeRunner,
    system: SystemRunner,
    ready: mpsc::Sender<()>,
    signals: Arc<Mutex<Vec<(i32, std::time::Instant)>>>,
    reaped: Arc<std::sync::atomic::AtomicBool>,
}
struct ObservedProcess {
    inner: Option<Box<dyn Process>>,
    ready: Option<mpsc::Sender<()>>,
    signals: Arc<Mutex<Vec<(i32, std::time::Instant)>>>,
    reaped: Arc<std::sync::atomic::AtomicBool>,
}
impl Process for ObservedProcess {
    fn poll(&mut self, tail: &mut Tail) -> io::Result<Option<Outcome>> {
        let result = self.inner.as_mut().unwrap().poll(tail)?;
        if tail.0.windows(5).any(|s| s == b"READY") {
            if let Some(ready) = self.ready.take() {
                ready.send(()).unwrap();
            }
        }
        Ok(result)
    }
    fn signal(&mut self, signal: i32) {
        self.signals
            .lock()
            .unwrap()
            .push((signal, std::time::Instant::now()));
        self.inner.as_mut().unwrap().signal(signal);
    }
    fn group_alive(&self) -> bool {
        self.inner.as_ref().unwrap().group_alive()
    }
}
impl Drop for ObservedProcess {
    fn drop(&mut self) {
        drop(self.inner.take()); // SystemProcess kills leftovers and wait() reaps.
        self.reaped
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
impl Runner for RealChildRunner {
    fn spawn(&mut self, spec: &Spec) -> io::Result<Box<dyn Process>> {
        if spec.program == Path::new("/usr/bin/curl") {
            return self.fake.spawn(spec);
        }
        let child = self.system.spawn(&Spec {
            program: "/bin/bash".into(),
            args: ["-c", "trap '' TERM; printf READY; exec /bin/sleep 20"]
                .map(OsString::from)
                .to_vec(),
            env: vec![],
        })?;
        Ok(Box::new(ObservedProcess {
            inner: Some(child),
            ready: Some(self.ready.clone()),
            signals: self.signals.clone(),
            reaped: self.reaped.clone(),
        }))
    }
    fn now(&self) -> Duration {
        self.system.now()
    }
    fn wait(&mut self, cancel: &mpsc::Receiver<()>, duration: Duration) -> bool {
        self.system.wait(cancel, duration)
    }
}

#[test]
fn shutdown_active_owned_child_terms_waits_ten_seconds_kills_and_joins() {
    let (fake, record) = fake(0);
    let (ready, arrival) = mpsc::channel();
    let signals = Arc::new(Mutex::new(Vec::new()));
    let reaped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let runner = RealChildRunner {
        fake,
        system: SystemRunner(std::time::Instant::now()),
        ready,
        signals: signals.clone(),
        reaped: reaped.clone(),
    };
    let (worker, result) = Worker::with_runner("v1.2.3".into(), runner, vec![]).unwrap();
    arrival
        .recv_timeout(Duration::from_secs(3))
        .expect("child readiness");
    let (done, finished) = mpsc::channel();
    let join = std::thread::spawn(move || {
        drop(worker);
        done.send(()).unwrap();
    });
    finished
        .recv_timeout(Duration::from_secs(13))
        .expect("TERM grace, KILL and worker join must be bounded");
    join.join().unwrap();
    assert!(result
        .try_recv()
        .unwrap()
        .unwrap_err()
        .contains("cancelled"));
    let signals = signals.lock().unwrap();
    assert_eq!(
        signals.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        [libc::SIGTERM, libc::SIGKILL]
    );
    assert!(signals[1].1.duration_since(signals[0].1) >= Duration::from_secs(10));
    assert!(reaped.load(std::sync::atomic::Ordering::Acquire));
    assert!(!record.lock().unwrap().directory.as_ref().unwrap().exists());
}

#[test]
fn controller_allows_one_install_and_suppresses_both_check_paths() {
    run_gtk_test(|| {
        use crate::update_check::{Controller, Panel, UpdateState};
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        let (worker, _results) = crate::update_check::Worker::start(move |_, _| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(br#"{"tag_name":"v1.2.3","draft":false,"prerelease":false}"#.to_vec())
        })
        .unwrap();
        let panel = Panel::new();
        let c = Controller::new(
            panel.clone(),
            (None, true, true),
            Some(worker),
            Origin::Installer,
        );
        c.complete(UpdateState::Available("v1.2.3".into()));
        let starts = Cell::new(0);
        for _ in 0..2 {
            c.begin_install_with(|tag| {
                starts.set(starts.get() + 1);
                Worker::with_runner(tag, fake(0).0, vec![])
            });
        }
        c.request(false);
        c.request(true);
        assert_eq!(starts.get(), 1);
        assert_eq!(c.state(), UpdateState::Available("v1.2.3".into()));
        assert!(!panel.update.is_sensitive());
        assert!(!panel.manual.is_sensitive());
        assert!(!panel.automatic.is_sensitive());
        c.shutdown(); // Joins the check worker, establishing the zero-call assertion.
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        eprintln!("GTK controller single-flight body executed");
    });
}

#[test]
fn real_process_captures_ordered_bounded_output_and_exit_status() {
    let mut runner = SystemRunner(std::time::Instant::now());
    let mut process = runner
        .spawn(&Spec {
            program: "/bin/bash".into(),
            args: [
                "-c",
                "printf '%05000d' 0; printf 'stdout\\n'; printf 'stderr\\n' >&2; exit 7",
            ]
            .map(OsString::from)
            .to_vec(),
            env: vec![],
        })
        .unwrap();
    let (stop, cancel) = mpsc::channel();
    let mut tail = Tail::default();
    loop {
        if let Some(code) = process.poll(&mut tail).unwrap() {
            assert_eq!(code, Outcome::Exited(7));
            assert_eq!(tail.0.len(), 4096);
            assert!(tail.0.ends_with(b"stdout\nstderr\n"));
            break;
        }
        assert!(runner.now() < Duration::from_secs(3));
        runner.wait(&cancel, Duration::from_millis(1));
    }
    drop(process);
    let mut process = runner
        .spawn(&Spec {
            program: "/bin/bash".into(),
            args: ["-c", "kill -TERM $$"].map(OsString::from).to_vec(),
            env: vec![],
        })
        .unwrap();
    loop {
        if let Some(status) = process.poll(&mut tail).unwrap() {
            assert_eq!(status, Outcome::Signal(libc::SIGTERM));
            break;
        }
        assert!(runner.now() < Duration::from_secs(3));
        runner.wait(&cancel, Duration::from_millis(1));
    }
    drop(process);
    drop(stop);
}

#[test]
fn controller_preserves_checks_when_no_install_is_running() {
    run_gtk_test(|| {
        use crate::update_check::{Controller, Panel, UpdateState};
        let (called, calls) = mpsc::channel();
        let (worker, results) = crate::update_check::Worker::start(move |_, _| {
            called.send(()).unwrap();
            Err(io::Error::other("offline"))
        })
        .unwrap();
        let panel = Panel::new();
        let c = Controller::new(
            panel.clone(),
            (None, true, true),
            Some(worker),
            Origin::Installer,
        );
        c.request(false);
        calls.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(c.state(), UpdateState::Checking);
        c.complete(results.recv_blocking().unwrap());
        assert_eq!(c.state(), UpdateState::Unavailable);
        assert!(panel.manual.is_sensitive());
        c.request(true);
        calls.recv_timeout(Duration::from_secs(2)).unwrap();
        c.complete(results.recv_blocking().unwrap());
        c.shutdown();
        eprintln!("GTK check preservation body executed");
    });
}

#[test]
fn gtk_restart_fake_spawner_observes_run_shutdown_and_bus_name_release() {
    run_gtk_test(|| {
        use gtk::gio;
        let id = format!("dev.streamdeck.RestartTest.p{}", std::process::id());
        let app = Application::builder().application_id(&id).build();
        let released = Rc::new(Cell::new(false));
        let cleanup = released.clone();
        app.connect_shutdown(move |_| cleanup.set(true));
        app.connect_activate(|app| {
            assert!(app.is_registered());
            assert!(!app.is_remote());
            assert!(app.dbus_connection().is_some());
            app.quit();
        });
        let flag = Cell::new(false);
        assert!(request_restart(
            &InstallState::Succeeded("v1.2.3".into()),
            &flag
        ));
        let returned = Cell::new(false);
        after_run(
            &flag,
            || {
                assert_eq!(app.run_with_args::<&str>(&[]), glib::ExitCode::SUCCESS);
                drop(app);
                returned.set(true);
            },
            || {
                assert!(returned.get());
                assert!(released.get());
                let connection =
                    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).unwrap();
                let reply = connection
                    .call_sync(
                        Some("org.freedesktop.DBus"),
                        "/org/freedesktop/DBus",
                        "org.freedesktop.DBus",
                        "NameHasOwner",
                        Some(&(id.as_str(),).to_variant()),
                        None,
                        gio::DBusCallFlags::NONE,
                        2000,
                        gio::Cancellable::NONE,
                    )
                    .unwrap();
                assert_eq!(reply.get::<(bool,)>(), Some((false,)));
            },
        );
        eprintln!("GTK restart bus-name ordering body executed");
    });
}

#[test]
fn restart_only_after_success_and_run_returns() {
    let flag = Cell::new(false);
    assert!(!request_restart(&InstallState::Idle, &flag));
    assert!(!request_restart(&InstallState::Running, &flag));
    assert!(!request_restart(
        &InstallState::Failed("failed".into()),
        &flag
    ));
    assert!(!flag.get());
    after_run(
        &flag,
        || (),
        || panic!("must not spawn without an explicit successful restart request"),
    );
    assert!(request_restart(
        &InstallState::Succeeded("v1.2.3".into()),
        &flag
    ));
    let order = RefCell::new(Vec::new());
    after_run(
        &flag,
        || {
            order.borrow_mut().push("run returned");
            42
        },
        || {
            order.borrow_mut().push("spawn");
        },
    );
    assert_eq!(*order.borrow(), ["run returned", "spawn"]);
}

#[test]
fn gtk_install_visibility_guidance_and_busy_checks() {
    run_gtk_test(|| {
        use crate::update_check::{Panel, UpdateState};
        let panel = Panel::new();
        for state in [
            UpdateState::Idle,
            UpdateState::Checking,
            UpdateState::Unavailable,
            UpdateState::UpToDate,
        ] {
            panel.render_install(&state, &InstallState::Idle, Origin::Installer);
            assert!(!panel.update.is_visible());
            assert!(!panel.restart.is_visible());
        }
        let available = UpdateState::Available("v1.2.3".into());
        panel.render_install(&available, &InstallState::Idle, Origin::Installer);
        assert!(panel.update.is_visible());
        assert!(panel.update.is_sensitive());
        panel.render_install(&available, &InstallState::Running, Origin::Installer);
        assert!(panel.update.is_visible());
        assert!(!panel.update.is_sensitive());
        assert!(!panel.manual.is_sensitive());
        assert!(!panel.automatic.is_sensitive());
        panel.render_install(
            &available,
            &InstallState::Succeeded("v1.2.3".into()),
            Origin::Installer,
        );
        assert!(panel.restart.is_visible());
        assert_eq!(panel.status.text(), "Updated to v1.2.3. Restart to use it.");
        panel.render_install(&available, &InstallState::Idle, Origin::Package);
        assert!(!panel.update.is_visible());
        assert_eq!(
            panel.guidance.text(),
            "Installed by your package manager. Update it with your package manager."
        );
        panel.render_install(&available, &InstallState::Idle, Origin::Other);
        assert!(!panel.update.is_visible());
        assert_eq!(panel.guidance.text(), "This copy was not installed by the sdeck-plus installer. Re-run the install command from the documentation to update.");
        eprintln!("GTK installer body executed");
    });
}
