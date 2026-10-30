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

fn timer(seconds: u32) -> Action {
    Action::Feature(FeatureAction::Timer { seconds })
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
                InfoValue::text(Glyph::None, "45s")
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
                InfoValue::text(Glyph::None, "45s")
            )],
        );
        let expected =
            sdeck_plus::info_render::compose_info_image(&InfoValue::text(Glyph::None, "45s"), None)
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
        for (seconds, expected) in [
            (1, "1s"),
            (45, "45s"),
            (59, "59s"),
            (60, "01:00"),
            (135, "02:15"),
            (3599, "59:59"),
            (3600, "01:00:00"),
            (3735, "01:02:15"),
            (86400, "24:00:00"),
        ] {
            fixture.press(2, seconds);
            let pictures = countdown_commands(&fixture.hardware);
            assert_eq!(
                pictures.last(),
                Some(&(
                    ArtworkTarget::Keypad(2),
                    InfoValue::text(Glyph::None, expected)
                ))
            );
        }
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
                InfoValue::text(Glyph::None, "02:15")
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
                InfoValue::text(Glyph::None, "02:15")
            )]
        );
        assert_eq!(fixture.preview(2), preview);
    });
}

#[test]
fn cancellation_replacement_and_test_restart_restore_latest_information() {
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
        fixture.press(3, 60);
        let restored = countdown_commands(&fixture.hardware);
        assert!(restored.contains(&(ArtworkTarget::Keypad(2), latest.clone())));
        assert_eq!(
            restored.last(),
            Some(&(
                ArtworkTarget::Keypad(3),
                InfoValue::text(Glyph::None, "01:00")
            ))
        );
        fixture
            .widgets
            .feature_runtime
            .execute("test".into(), timer(10));
        let restored = countdown_commands(&fixture.hardware);
        assert_eq!(
            restored,
            [(ArtworkTarget::Keypad(2), latest.clone())],
            "Test action replacement clears the physical origin"
        );
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
                .assign_in_layer("Base", target, &fixture.root.join("source.png"))
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
            .any(|(target, value)| *target == ArtworkTarget::Keypad(2) && value.headline == "45s"));
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
                (ArtworkTarget::Keypad(2), InfoValue::text(Glyph::None, "2s")),
                (ArtworkTarget::Keypad(2), InfoValue::text(Glyph::None, "1s")),
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
