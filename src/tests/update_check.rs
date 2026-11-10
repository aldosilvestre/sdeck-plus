use super::*;
use crate::update_check::{check, parse_tag, Gate, UpdateState};
use std::time::Duration;

#[test]
fn strict_versions_and_numeric_release_comparison() {
    assert_eq!(parse_tag("v0.10.0"), Some((0, 10, 0)));
    for tag in [
        "0.1.0",
        "v1.2",
        "v1.2.3-beta",
        "v1.2.3\n",
        "v+1.2.3",
        "v1.2.18446744073709551616",
    ] {
        assert_eq!(parse_tag(tag), None, "{tag}");
    }
    for (tag, current, expected) in [
        ("v0.10.0", "0.9.9", UpdateState::Available("v0.10.0".into())),
        ("v0.9.9", "0.10.0", UpdateState::UpToDate),
        ("v0.1.0", "0.1.0", UpdateState::UpToDate),
    ] {
        let json = format!(r#"{{"tag_name":"{tag}","draft":false,"prerelease":false}}"#);
        assert_eq!(check(|_, _| Ok(json.into_bytes()), current), expected);
    }
    for json in [
        r#"{"tag_name":"v9.0.0","draft":true,"prerelease":false}"#,
        r#"{"tag_name":"v9.0.0","draft":false,"prerelease":true}"#,
        r#"{"tag_name":"v9.0.0-beta","draft":false,"prerelease":false}"#,
    ] {
        assert_eq!(
            check(|_, _| Ok(json.as_bytes().to_vec()), "0.1.0"),
            UpdateState::Unavailable
        );
    }
}

#[test]
fn runner_failures_are_unavailable_and_request_is_fixed() {
    for kind in [
        std::io::ErrorKind::NotFound,
        std::io::ErrorKind::Other,
        std::io::ErrorKind::TimedOut,
    ] {
        assert_eq!(
            check(|_, _| Err(std::io::Error::from(kind)), "0.1.0"),
            UpdateState::Unavailable
        );
    }
    for bytes in [vec![b' '; 262145], b"invalid json".to_vec()] {
        assert_eq!(check(|_, _| Ok(bytes), "0.1.0"), UpdateState::Unavailable);
    }
    check(
        |program, args| {
            assert_eq!(program, "curl");
            assert_eq!(
                args,
                [
                    "-fsS",
                    "--proto",
                    "=https",
                    "--max-time",
                    "8",
                    "--max-filesize",
                    "262144",
                    "-H",
                    "Accept: application/vnd.github+json",
                    "-H",
                    "User-Agent: sdeck-plus/0.1.0",
                    "https://api.github.com/repos/aldosilvestre/sdeck-plus/releases/latest"
                ]
            );
            Err(std::io::ErrorKind::Other.into())
        },
        "0.1.0",
    );
}

#[test]
fn single_in_flight_and_six_hour_monotonic_suppression() {
    let mut gate = Gate::default();
    assert!(!gate.begin(false, false, Duration::ZERO));
    assert!(gate.begin(true, false, Duration::ZERO));
    assert!(!gate.begin(true, true, Duration::ZERO));
    gate.complete(Duration::from_secs(10));
    assert!(!gate.begin(true, false, Duration::from_secs(21609)));
    assert!(gate.begin(true, false, Duration::from_secs(21610)));
    gate.complete(Duration::from_secs(21610));
    assert!(gate.begin(false, true, Duration::from_secs(21611)));
}

#[test]
fn preference_default_roundtrip_and_invalid_original_preserved() {
    let root = std::env::temp_dir().join(format!("sdeck-updates-{}", std::process::id()));
    let path = root.join("updates.json");
    assert!(crate::update_preference::load(&path).unwrap());
    let current = Cell::new(true);
    crate::update_preference::apply(&path, &current, false).unwrap();
    assert!(!current.get());
    assert!(!crate::update_preference::load(&path).unwrap());
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, b"other writer").unwrap();
    let original = std::fs::read(&path).unwrap();
    assert!(crate::update_preference::apply(&path, &current, true).is_err());
    assert!(!current.get());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read(&temporary).unwrap(), b"other writer");
    std::fs::remove_file(temporary).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    for malformed in ["oops", "{}", r#"{"automatic_check":true,"extra":0}"#] {
        std::fs::write(&path, malformed).unwrap();
        assert!(crate::update_preference::load(&path).is_err());
        assert!(crate::update_preference::save(&path, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), malformed);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn worker_runs_off_caller_and_joins_without_network() {
    let caller = std::thread::current().id();
    let (entered, arrival) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (worker, results) = crate::update_check::Worker::start(move |_, _| {
        entered.send(std::thread::current().id()).unwrap();
        resume.recv_timeout(Duration::from_secs(2)).unwrap();
        Ok(br#"{"tag_name":"v0.1.0","draft":false,"prerelease":false}"#.to_vec())
    })
    .unwrap();
    worker.sender.send(()).unwrap();
    assert_ne!(
        arrival.recv_timeout(Duration::from_secs(2)).unwrap(),
        caller
    );
    release.send(()).unwrap();
    drop(worker);
    assert_eq!(results.try_recv().unwrap(), UpdateState::UpToDate);
}

#[test]
fn preference_paths_are_absolute_with_home_fallback() {
    use std::path::Path;
    assert_eq!(
        crate::update_preference::config_path(Some(Path::new("/config")), None).unwrap(),
        Path::new("/config/sdeck-plus/updates.json")
    );
    assert_eq!(
        crate::update_preference::config_path(
            Some(Path::new("relative")),
            Some(Path::new("/home/test"))
        )
        .unwrap(),
        Path::new("/home/test/.config/sdeck-plus/updates.json")
    );
    assert!(crate::update_preference::config_path(None, Some(Path::new("relative"))).is_err());
}

#[test]
fn existing_help_and_configuration_menu_regression() {
    // Preserve the existing public GTK journey under this task's focused filter.
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "tests::configuration_transfer_tests::menu_transfers_configuration_and_reloads_without_running_actions",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("Existing Help/configuration menu journey passed");
}

#[test]
fn gtk_updates_and_existing_help_content() {
    run_gtk_test(|| {
        let window = ApplicationWindow::builder().build();
        let body = GtkBox::new(Orientation::Vertical, 16);
        body.append(&Label::new(Some("Existing setup help")));
        let menu = configuration_menu::ConfigurationMenu::new(&window, &body);
        let updates = crate::update_check::Panel::new();
        menu.add_updates(&updates.body);
        menu.bind_updates_button(&updates.indicator);
        updates.render(&UpdateState::UpToDate);
        assert_eq!(updates.status.text(), "Up to date (v0.1.0)");
        assert!(!updates.indicator.is_visible());
        updates.render(&UpdateState::Available("v0.10.0".into()));
        assert!(updates.indicator.is_visible());
        assert_eq!(
            updates.indicator.label().as_deref(),
            Some("Update available: v0.10.0")
        );
        updates.render(&UpdateState::Checking);
        assert!(!updates.manual.is_sensitive());
        assert_eq!(updates.status.text(), "Checking...");
        assert_eq!(
            updates
                .body
                .first_child()
                .unwrap()
                .downcast::<Label>()
                .unwrap()
                .text(),
            "Updates"
        );
        assert!(body.first_child().is_some());
        assert!(updates.body.parent().is_some());
        updates.indicator.emit_clicked();
        assert!(menu.updates_for_test().is_visible());
        assert!(!menu.help_for_test().is_visible());
        updates.render(&UpdateState::Unavailable);
        assert_eq!(updates.status.text(), "Could not check for updates");
        assert!(updates.manual.is_sensitive());
        assert!(updates.indicator.is_visible());
        eprintln!("GTK Updates body executed");
        window.destroy();
    });
}

#[test]
fn drop_with_active_check_and_closed_receiver_joins() {
    let (entered, arrival) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (worker, results) = crate::update_check::Worker::start(move |_, _| {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(2)).unwrap();
        Ok(br#"{"tag_name":"v9.0.0","draft":false,"prerelease":false}"#.to_vec())
    })
    .unwrap();

    worker.sender.send(()).unwrap();
    arrival.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(results);

    let (done, finished) = mpsc::channel();
    let join = std::thread::spawn(move || {
        drop(worker);
        done.send(()).unwrap();
    });
    release.send(()).unwrap();
    finished
        .recv_timeout(Duration::from_secs(2))
        .expect("shutdown must join an active worker after the GTK receiver closes");
    join.join().unwrap();
}

#[test]
fn known_release_survives_later_retry_and_failure() {
    run_gtk_test(|| {
        use crate::update_check::{Controller, Panel};
        let (called, calls) = mpsc::channel();
        let (worker, results) = crate::update_check::Worker::start(move |_, _| {
            called.send(()).unwrap();
            Err(std::io::Error::other("offline"))
        })
        .unwrap();
        let panel = Panel::new();
        let c = Controller::new(
            panel.clone(),
            (None, true, true),
            Some(worker),
            crate::update_install::Origin::Installer,
        );
        let tag = UpdateState::Available("v9.0.0".into());
        c.complete(tag.clone());
        c.request(true);
        calls.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(c.state(), tag);
        assert!(panel.update.is_visible());
        assert!(panel.indicator.is_visible());
        assert!(panel.status.text().contains("v9.0.0"));
        assert!(panel.status.text().contains("Checking"));
        assert!(!panel.manual.is_sensitive());
        c.complete(results.recv_blocking().unwrap());
        assert_eq!(c.state(), tag);
        assert!(panel.update.is_visible());
        assert!(panel.indicator.is_visible());
        assert!(panel.status.text().contains("v9.0.0"));
        assert!(panel.status.text().contains("could not"));
        assert!(panel.manual.is_sensitive());
        c.complete(UpdateState::UpToDate);
        assert_eq!(c.state(), UpdateState::UpToDate);
        assert!(!panel.indicator.is_visible());
        c.shutdown();
    });
}

#[test]
fn retain_known_release_transition_table() {
    use crate::update_check::retain_known;
    let old = UpdateState::Available("v1.0.0".into());
    let newer = UpdateState::Available("v2.0.0".into());
    for (previous, next, expected) in [
        (&old, UpdateState::Checking, old.clone()),
        (&old, UpdateState::Unavailable, old.clone()),
        (&old, UpdateState::UpToDate, UpdateState::UpToDate),
        (&old, newer.clone(), newer.clone()),
        (
            &UpdateState::UpToDate,
            UpdateState::Checking,
            UpdateState::Checking,
        ),
        (
            &UpdateState::Idle,
            UpdateState::Unavailable,
            UpdateState::Unavailable,
        ),
    ] {
        assert_eq!(retain_known(previous, next.clone()), expected, "{next:?}");
    }
}

fn contains_scroller(widget: &gtk::Widget) -> bool {
    widget.is::<gtk::ScrolledWindow>()
        || std::iter::successors(widget.first_child(), |child| child.next_sibling())
            .any(|child| contains_scroller(&child))
}

fn labels_in(widget: &gtk::Widget, found: &mut Vec<String>) {
    if let Some(label) = widget.downcast_ref::<Label>() {
        found.push(label.text().to_string());
    }
    for child in std::iter::successors(widget.first_child(), |child| child.next_sibling()) {
        labels_in(&child, found);
    }
}

#[test]
fn gtk_updates_dialog_is_unscrolled_live_and_help_has_no_updates() {
    run_gtk_test(|| {
        use crate::update_install::{InstallState, Origin};
        let window = ApplicationWindow::builder().build();
        let body = GtkBox::new(Orientation::Vertical, 16);
        body.append(&Label::new(Some("Existing setup help")));
        let menu = configuration_menu::ConfigurationMenu::new(&window, &body);
        let updates = crate::update_check::Panel::new();
        menu.add_updates(&updates.body);
        menu.bind_updates_button(&updates.indicator);
        let mut help_labels = Vec::new();
        labels_in(menu.help_for_test().upcast_ref(), &mut help_labels);
        assert!(help_labels.iter().all(|text| !text.contains("pdate")));
        assert!(!menu.updates_for_test().is_visible());

        updates.render_install(
            &UpdateState::Available("v9.9.9".into()),
            &InstallState::Idle,
            Origin::Installer,
        );
        updates.indicator.emit_clicked();
        let dialog = menu.updates_for_test();
        assert!(dialog.is_visible() && dialog.is_modal());
        assert_eq!(dialog.title().as_deref(), Some("Updates"));
        assert!(!contains_scroller(dialog.upcast_ref()));
        let mut texts = Vec::new();
        labels_in(dialog.upcast_ref(), &mut texts);
        assert!(texts.iter().any(|text| text == "Update available: v9.9.9"));
        // Live changes while open, and the same widgets after closing and re-opening.
        updates.render(&UpdateState::UpToDate);
        assert_eq!(updates.status.text(), "Up to date (v0.1.0)");
        dialog.response(gtk::ResponseType::Close);
        assert!(!dialog.is_visible());
        dialog.present();
        assert!(updates.body.parent().is_some() && contains_updates_body(&dialog, &updates));
        dialog.response(gtk::ResponseType::Close);

        let long = "installer output line\n".repeat(190);
        let available = UpdateState::Available("v9.9.9".into());
        for (check, install, origin) in [
            (UpdateState::Idle, InstallState::Idle, Origin::Installer),
            (UpdateState::Checking, InstallState::Idle, Origin::Installer),
            (UpdateState::UpToDate, InstallState::Idle, Origin::Installer),
            (available.clone(), InstallState::Idle, Origin::Installer),
            (available.clone(), InstallState::Running, Origin::Installer),
            (
                available.clone(),
                InstallState::Failed(long),
                Origin::Installer,
            ),
            (
                available.clone(),
                InstallState::Succeeded("v9.9.9".into()),
                Origin::Installer,
            ),
            (available.clone(), InstallState::Idle, Origin::Package),
            (available, InstallState::Idle, Origin::Other),
        ] {
            updates.render_install(&check, &install, origin);
            assert!(!contains_scroller(dialog.upcast_ref()));
            let (minimum, natural, _, _) = updates.body.measure(gtk::Orientation::Vertical, 440);
            assert!(
                natural < 420 && minimum <= natural,
                "{natural} {:?} {:?}",
                check,
                origin
            );
        }
        window.destroy();
    });
}

fn contains_updates_body(dialog: &gtk::Dialog, panel: &crate::update_check::Panel) -> bool {
    panel
        .body
        .ancestor(gtk::Dialog::static_type())
        .is_some_and(|found| found == *dialog.upcast_ref::<gtk::Widget>())
}
