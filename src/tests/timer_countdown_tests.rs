use super::*;
use sdeck_plus::{
    features::FeatureAction,
    info::{Glyph, InfoSource, InfoValue},
};

fn countdown_commands(
    hardware: &mpsc::Receiver<ArtworkCommand>,
) -> Vec<(ArtworkTarget, InfoValue)> {
    hardware
        .try_iter()
        .flat_map(|command| match command {
            ArtworkCommand::Replace(commands) => commands,
            command => vec![command],
        })
        .filter_map(|command| match command {
            ArtworkCommand::ComposeInfo { target, value, .. } => Some((target, value)),
            _ => None,
        })
        .collect()
}

struct CountdownFixture {
    root: std::path::PathBuf,
    layers: Rc<RefCell<LayerStore>>,
    artwork: ArtworkUi,
    hardware: mpsc::Receiver<ArtworkCommand>,
    window: ApplicationWindow,
    widgets: MonitorWidgets,
}

impl CountdownFixture {
    fn new(tag: &str) -> Self {
        let (root, mut store, images, _) = artwork_fixture(tag);
        store.bind("Base", Trigger::KeyPress(2), timer(45)).unwrap();
        let layers = Rc::new(RefCell::new(store));
        let (output, hardware) = mpsc::channel();
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
        let window = ApplicationWindow::builder().build();
        let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
        window.set_child(Some(&widgets.root));
        widgets.feature_runtime.set_timer_sound_for_test(
            gtk::gio::SubprocessLauncher::new(gtk::gio::SubprocessFlags::NONE),
            vec!["/bin/true".into()],
        );
        while hardware.try_recv().is_ok() {}
        Self {
            root,
            layers,
            artwork,
            hardware,
            window,
            widgets,
        }
    }

    fn press(&self, key: u8, seconds: u32) {
        self.widgets
            .feature_runtime
            .execute_input(&protocol::Event::Key {
                index: usize::from(key - 1),
                pressed: false,
            });
        let layer = self.layers.borrow().active().to_owned();
        self.layers
            .borrow_mut()
            .bind(&layer, Trigger::KeyPress(key), timer(seconds))
            .unwrap();
        self.widgets
            .feature_runtime
            .execute_input(&protocol::Event::Key {
                index: usize::from(key - 1),
                pressed: true,
            });
    }

    fn preview(&self, key: u8) -> Vec<u8> {
        let pictures = self.artwork.pictures.borrow();
        let (_, picture) = pictures
            .iter()
            .find(|(target, _)| *target == ArtworkTarget::Keypad(key))
            .unwrap();
        let texture = picture
            .paintable()
            .unwrap()
            .downcast::<gtk::gdk::Texture>()
            .unwrap();
        let mut bytes = vec![0; (texture.width() * texture.height() * 4) as usize];
        texture.download(&mut bytes, texture.width() as usize * 4);
        bytes
    }
}

impl Drop for CountdownFixture {
    fn drop(&mut self) {
        self.widgets.feature_runtime.cancel_timer();
        self.window.destroy();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn expected_countdown(time: &str, units: &str) -> InfoValue {
    InfoValue {
        detail: units.into(),
        ..InfoValue::text(Glyph::Timer, time)
    }
}

fn timer(seconds: u32) -> Action {
    Action::Feature(FeatureAction::Timer { seconds })
}

fn await_timer_status(label: &Label, expected: &str) {
    if label.text() == expected {
        return;
    }
    let loop_ = glib::MainLoop::new(None, false);
    let done = loop_.clone();
    let expected = expected.to_owned();
    let signal = label.connect_label_notify(move |label| {
        if label.text() == expected {
            done.quit();
        }
    });
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let guard_loop = loop_.clone();
    let guard = glib::timeout_add_local_once(Duration::from_secs(4), move || {
        flag.set(true);
        guard_loop.quit();
    });
    loop_.run();
    label.disconnect(signal);
    if !expired.get() {
        guard.remove();
    }
    assert!(!expired.get(), "timer did not reach expected status");
}

#[test]
fn active_timer_ignores_short_taps_until_cancelled_or_complete() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-no-restart");
        let runtime = &fixture.widgets.feature_runtime;
        fixture
            .layers
            .borrow_mut()
            .bind(
                "Base",
                Trigger::KeyHold(2),
                Action::Feature(FeatureAction::CancelTimer),
            )
            .unwrap();
        let release = || {
            runtime.execute_input(&protocol::Event::Key {
                index: 1,
                pressed: false,
            })
        };
        fixture.press(2, 2);
        release();
        await_timer_status(&runtime.timer_status, "Timer: 1s remaining");
        countdown_commands(&fixture.hardware);
        let preview = fixture.preview(2);
        fixture.press(2, 2);
        release();
        assert_eq!(
            runtime.timer_status.text(),
            "Timer: 1s remaining",
            "short tap cannot restart the active timer"
        );
        assert_eq!(fixture.preview(2), preview);
        assert!(
            countdown_commands(&fixture.hardware).is_empty(),
            "ignored tap cannot reset the key picture"
        );

        fixture.press(3, 60);
        runtime.execute("test-start".into(), timer(60));
        fixture
            .layers
            .borrow_mut()
            .bind("Base", Trigger::EncoderPress(1), timer(60))
            .unwrap();
        runtime.execute_input(&protocol::Event::EncoderButton {
            index: 0,
            pressed: true,
        });
        runtime.execute_with_return(
            "ignored-start-return".into(),
            timer(60),
            Some(sdeck_plus::layers::PageReturn {
                layer: "Base".into(),
                seconds: 1,
            }),
        );
        assert_eq!(runtime.timer_status.text(), "Timer: 1s remaining");
        assert!(
            !runtime.page_return_active(),
            "ignored timer start cannot arm a page return"
        );
        assert!(
            countdown_commands(&fixture.hardware).is_empty(),
            "another start cannot move the active countdown to a different key"
        );
        assert_eq!(fixture.preview(2), preview);
        // A reset to 60 seconds would miss this real completion signal.
        await_timer_status(&runtime.timer_status, "Timer complete.");
        assert!(!runtime.timer_active());

        fixture.press(3, 3);
        assert_eq!(
            runtime.timer_status.text(),
            "Timer: 3s remaining",
            "completion permits a new timer"
        );
        assert!(runtime.timer_active());
        runtime.execute("cancel".into(), Action::Feature(FeatureAction::CancelTimer));
        assert!(!runtime.timer_active());
        fixture.press(2, 4);
        release();
        assert_eq!(
            runtime.timer_status.text(),
            "Timer: 4s remaining",
            "cancellation permits a new timer"
        );
        assert!(runtime.timer_active());
    });
}

#[test]
fn exclusive_tap_starts_countdown_only_on_release() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("hold-tap-countdown");
        fixture
            .layers
            .borrow_mut()
            .bind(
                "Base",
                Trigger::KeyHold(2),
                Action::Feature(FeatureAction::CancelTimer),
            )
            .unwrap();
        fixture.press(2, 45);
        assert!(
            !fixture.widgets.feature_runtime.timer_active(),
            "hold-configured key must defer tap"
        );
        assert!(countdown_commands(&fixture.hardware).is_empty());
        fixture
            .widgets
            .feature_runtime
            .execute_input(&protocol::Event::Key {
                index: 1,
                pressed: false,
            });
        assert!(fixture.widgets.feature_runtime.timer_active());
        assert_eq!(
            countdown_commands(&fixture.hardware),
            [(
                ArtworkTarget::Keypad(2),
                expected_countdown("00:45", "mm:ss")
            )]
        );
    });
}

#[test]
fn physical_timer_draws_countdown_on_its_key_and_preview() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-key");
        fixture.press(2, 45);
        assert_eq!(
            countdown_commands(&fixture.hardware),
            [(
                ArtworkTarget::Keypad(2),
                expected_countdown("00:45", "mm:ss")
            )],
        );
        let expected = sdeck_plus::info_render::compose_info_image(
            &expected_countdown("00:45", "mm:ss"),
            None,
        )
        .unwrap();
        let texture = gtk::gdk::Texture::for_pixbuf(&expected);
        let mut bytes = vec![0; (texture.width() * texture.height() * 4) as usize];
        texture.download(&mut bytes, texture.width() as usize * 4);
        assert_eq!(
            fixture.preview(2),
            bytes,
            "GTK and hardware use the same composition"
        );
    });
}

#[test]
fn physical_timer_formats_seconds_minutes_and_hours_boundaries() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-formats");
        for (seconds, expected, units) in [
            (7200, "02:00", "hh:mm"),
            (1, "00:01", "mm:ss"),
            (45, "00:45", "mm:ss"),
            (59, "00:59", "mm:ss"),
            (60, "01:00", "mm:ss"),
            (135, "02:15", "mm:ss"),
            (3599, "59:59", "mm:ss"),
            (3600, "01:00", "hh:mm"),
            (3601, "01:00", "hh:mm"),
            (3735, "01:02", "hh:mm"),
            (7199, "01:59", "hh:mm"),
            (86400, "24:00", "hh:mm"),
        ] {
            fixture.widgets.feature_runtime.cancel_timer();
            fixture.press(2, seconds);
            let pictures = countdown_commands(&fixture.hardware);
            assert_eq!(
                pictures.last(),
                Some(&(
                    ArtworkTarget::Keypad(2),
                    expected_countdown(expected, units)
                ))
            );
        }
    });
}

#[test]
fn countdown_caption_switches_units_when_crossing_one_hour() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-units");
        fixture.press(2, 3600);
        assert_eq!(
            countdown_commands(&fixture.hardware).last(),
            Some(&(
                ArtworkTarget::Keypad(2),
                expected_countdown("01:00", "hh:mm")
            ))
        );
        let hours_preview = fixture.preview(2);
        fixture.artwork.update_countdown(3599);
        assert_eq!(
            countdown_commands(&fixture.hardware).last(),
            Some(&(
                ArtworkTarget::Keypad(2),
                expected_countdown("59:59", "mm:ss")
            ))
        );
        assert_ne!(fixture.preview(2), hours_preview);
        let minutes_preview = fixture.preview(2);
        fixture.artwork.refresh(true);
        assert_eq!(
            countdown_commands(&fixture.hardware).last(),
            Some(&(
                ArtworkTarget::Keypad(2),
                expected_countdown("59:59", "mm:ss")
            ))
        );
        assert_eq!(fixture.preview(2), minutes_preview);
    });
}

#[test]
fn hours_countdown_redraws_only_when_its_minutes_change() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-hours-redraw");
        fixture.press(2, 7200);
        assert_eq!(
            countdown_commands(&fixture.hardware),
            vec![(
                ArtworkTarget::Keypad(2),
                expected_countdown("02:00", "hh:mm")
            )]
        );
        fixture.artwork.update_countdown(7199);
        assert_eq!(
            countdown_commands(&fixture.hardware),
            vec![(
                ArtworkTarget::Keypad(2),
                expected_countdown("01:59", "hh:mm")
            )]
        );
        let shown = fixture.preview(2);
        // Every remaining second of that minute shows the same hh:mm picture.
        for seconds in (7140..7199).rev() {
            fixture.artwork.update_countdown(seconds);
        }
        assert!(
            countdown_commands(&fixture.hardware).is_empty(),
            "an unchanged hh:mm picture must not be re-sent to the device"
        );
        assert_eq!(fixture.preview(2), shown);
        fixture.artwork.refresh(true);
        assert_eq!(
            countdown_commands(&fixture.hardware).last(),
            Some(&(
                ArtworkTarget::Keypad(2),
                expected_countdown("01:59", "hh:mm")
            ))
        );
        fixture.artwork.update_countdown(7139);
        assert_eq!(
            countdown_commands(&fixture.hardware),
            vec![(
                ArtworkTarget::Keypad(2),
                expected_countdown("01:58", "hh:mm")
            )]
        );
        assert_ne!(fixture.preview(2), shown);
        // Below one hour every second is visible, so every second is drawn.
        fixture.artwork.update_countdown(59);
        fixture.artwork.update_countdown(58);
        assert_eq!(
            countdown_commands(&fixture.hardware),
            vec![
                (
                    ArtworkTarget::Keypad(2),
                    expected_countdown("00:59", "mm:ss")
                ),
                (
                    ArtworkTarget::Keypad(2),
                    expected_countdown("00:58", "mm:ss")
                ),
            ]
        );
    });
}

#[test]
fn test_action_and_non_key_input_keep_timer_without_claiming_a_key() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-non-key");
        let runtime = &fixture.widgets.feature_runtime;
        runtime.execute(binding_identity("Base", &Trigger::KeyPress(2)), timer(45));
        assert!(runtime.timer_active());
        assert_eq!(runtime.timer_status.text(), "Timer: 45s remaining");
        assert!(countdown_commands(&fixture.hardware).is_empty());
        runtime.execute_input(&protocol::Event::Key {
            index: 1,
            pressed: false,
        });
        assert!(countdown_commands(&fixture.hardware).is_empty());
        fixture
            .layers
            .borrow_mut()
            .bind("Base", Trigger::EncoderPress(1), timer(60))
            .unwrap();
        runtime.execute_input(&protocol::Event::EncoderButton {
            index: 0,
            pressed: true,
        });
        assert_eq!(runtime.timer_status.text(), "Timer: 45s remaining");
        assert!(countdown_commands(&fixture.hardware).is_empty());
        runtime.cancel_timer();
        runtime.execute_input(&protocol::Event::EncoderButton {
            index: 0,
            pressed: true,
        });
        assert_eq!(runtime.timer_status.text(), "Timer: 60s remaining");
        assert!(countdown_commands(&fixture.hardware).is_empty());
    });
}

#[test]
fn countdown_survives_refresh_reconnect_and_returns_only_to_its_page() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-pages");
        fixture.press(2, 135);
        let preview = fixture.preview(2);
        countdown_commands(&fixture.hardware);
        fixture.artwork.refresh(true); // Same forced snapshot used on reconnect.
        assert_eq!(
            countdown_commands(&fixture.hardware),
            [(
                ArtworkTarget::Keypad(2),
                expected_countdown("02:15", "mm:ss")
            )]
        );
        assert_eq!(fixture.preview(2), preview);
        fixture.layers.borrow_mut().create("Other").unwrap();
        fixture.layers.borrow_mut().select("Other");
        fixture.artwork.refresh(false);
        assert!(countdown_commands(&fixture.hardware).is_empty());
        fixture.layers.borrow_mut().select("Base");
        fixture.artwork.refresh(false);
        assert_eq!(
            countdown_commands(&fixture.hardware),
            [(
                ArtworkTarget::Keypad(2),
                expected_countdown("02:15", "mm:ss")
            )]
        );
        assert_eq!(fixture.preview(2), preview);
    });
}

#[test]
fn cancelling_before_new_starts_restores_latest_information() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-restore");
        fixture
            .layers
            .borrow_mut()
            .set_info("Base", &Trigger::KeyPress(2), Some(InfoSource::Volume))
            .unwrap();
        let initial = InfoValue::gauge(Glyph::Speaker, 30, "30%".into());
        fixture.artwork.update_info(InfoSource::Volume, initial);
        fixture.press(2, 45);
        countdown_commands(&fixture.hardware);
        let latest = InfoValue::gauge(Glyph::Speaker, 80, "80%".into());
        fixture
            .artwork
            .update_info(InfoSource::Volume, latest.clone());
        assert!(
            countdown_commands(&fixture.hardware).is_empty(),
            "live readings cannot overwrite the timer"
        );
        fixture.widgets.feature_runtime.cancel_timer();
        fixture.press(3, 60);
        let restored = countdown_commands(&fixture.hardware);
        assert!(restored.contains(&(ArtworkTarget::Keypad(2), latest.clone())));
        assert_eq!(
            restored.last(),
            Some(&(
                ArtworkTarget::Keypad(3),
                expected_countdown("01:00", "mm:ss")
            ))
        );
        fixture.widgets.feature_runtime.cancel_timer();
        fixture
            .widgets
            .feature_runtime
            .execute("test".into(), timer(10));
        let restored = countdown_commands(&fixture.hardware);
        assert_eq!(
            restored,
            [(ArtworkTarget::Keypad(2), latest.clone())],
            "Cancelling before Test action clears the physical origin"
        );
        fixture.widgets.feature_runtime.cancel_timer();
        fixture.press(2, 45);
        countdown_commands(&fixture.hardware);
        fixture
            .widgets
            .feature_runtime
            .execute("cancel".into(), Action::Feature(FeatureAction::CancelTimer));
        assert!(!fixture.widgets.feature_runtime.timer_active());
        assert_eq!(
            fixture.widgets.feature_runtime.timer_status.text(),
            "Timer cancelled."
        );
        assert_eq!(
            countdown_commands(&fixture.hardware),
            [(ArtworkTarget::Keypad(2), latest)]
        );
    });
}

#[test]
fn cancellation_restores_custom_artwork_and_button_text_without_changing_assignments() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-artwork");
        let target = ArtworkTarget::Keypad(2);
        let text = sdeck_plus::button_text::ButtonText {
            text: "Tea".into(),
            ..Default::default()
        };
        let background = {
            let mut images = fixture.artwork.store.borrow_mut();
            let images = images.as_mut().unwrap();
            images
                .assign_in_layer("Base", target, fixture.root.join("source.png"))
                .unwrap();
            images
                .set_button_text_in_layer("Base", target, text.clone())
                .unwrap();
            images.assigned_path_in_layer("Base", target).unwrap()
        };
        fixture.artwork.refresh(true);
        let normal = fixture.preview(2);
        while fixture.hardware.try_recv().is_ok() {}
        fixture.press(2, 45);
        assert!(countdown_commands(&fixture.hardware)
            .iter()
            .any(
                |(target, value)| *target == ArtworkTarget::Keypad(2) && value.headline == "00:45"
            ));
        assert!(fixture.preview(2) != normal);
        fixture.widgets.feature_runtime.cancel_timer();
        assert_eq!(fixture.preview(2), normal);
        let restored = fixture
            .hardware
            .try_iter()
            .find_map(|command| match command {
                ArtworkCommand::Replace(commands) => {
                    commands.into_iter().find_map(|command| match command {
                        ArtworkCommand::ComposeButton { key: 2, path, text } => Some((path, text)),
                        _ => None,
                    })
                }
                _ => None,
            });
        assert_eq!(restored, Some((Some(background.clone()), text.clone())));
        let images = fixture.artwork.store.borrow();
        let images = images.as_ref().unwrap();
        assert_eq!(
            images.assigned_path_in_layer("Base", target),
            Some(background)
        );
        assert_eq!(images.button_text_in_layer("Base", target), Some(&text));
        assert_eq!(
            fixture
                .layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(2)),
            Some(timer(45))
        );
    });
}

#[test]
fn completion_restores_normal_icon_and_ticks_are_deduplicated() {
    run_gtk_test(|| {
        let fixture = CountdownFixture::new("countdown-completion");
        fixture
            .layers
            .borrow_mut()
            .bind("Base", Trigger::KeyPress(2), timer(2))
            .unwrap();
        fixture.artwork.refresh(true);
        let normal = fixture.preview(2);
        while fixture.hardware.try_recv().is_ok() {}
        let runtime = &fixture.widgets.feature_runtime;
        let loop_ = glib::MainLoop::new(None, false);
        let completed = loop_.clone();
        let connection = runtime.timer_status.connect_label_notify(move |label| {
            if label.text() == "Timer complete." {
                completed.quit();
            }
        });
        let expired = Rc::new(Cell::new(false));
        let timeout_flag = expired.clone();
        let guard_loop = loop_.clone();
        let guard = glib::timeout_add_local_once(Duration::from_secs(4), move || {
            timeout_flag.set(true);
            guard_loop.quit();
        });
        fixture.press(2, 2);
        loop_.run();
        if !expired.get() {
            guard.remove();
        }
        runtime.timer_status.disconnect(connection);
        assert!(!expired.get(), "awaited actual completion signal");
        assert!(!runtime.timer_active());
        assert_eq!(runtime.timer_status.text(), "Timer complete.");
        assert_eq!(
            countdown_commands(&fixture.hardware),
            [
                (
                    ArtworkTarget::Keypad(2),
                    expected_countdown("00:02", "mm:ss")
                ),
                (
                    ArtworkTarget::Keypad(2),
                    expected_countdown("00:01", "mm:ss")
                ),
            ],
            "250 ms polling renders once per changed second, never zero"
        );
        assert_eq!(fixture.preview(2), normal);
        let snapshot = fixture.artwork.with_info("Base", Vec::new());
        assert!(snapshot.iter().any(|command| matches!(
            command,
            ArtworkCommand::ComposeIcon {
                target: ArtworkTarget::Keypad(2),
                ..
            }
        )));
    });
}
