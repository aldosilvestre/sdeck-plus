use super::key_hold_ui_tests::HoldFixture;
use super::*;
use sdeck_plus::{
    features::FeatureAction,
    info::{Glyph, InfoValue},
};

/// Await a production feedback signal; timeout is only a failure guard.
fn await_label(label: &Label, expected: &str, trigger: impl FnOnce()) {
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
    let guard = glib::timeout_add_local_once(Duration::from_secs(5), move || {
        flag.set(true);
        guard_loop.quit();
    });
    trigger();
    loop_.run();
    if !expired.get() {
        guard.remove();
    }
    label.disconnect(signal);
    assert!(
        !expired.get(),
        "timed out awaiting actual production feedback"
    );
}

#[test]
fn physical_channel_tap_then_hold_cancels_countdown_without_second_tap() {
    run_gtk_test(|| {
        let mut fixture = HoldFixture::new("hold-channel");
        let widgets = fixture.widgets.as_ref().unwrap();
        let ui = &widgets.binding_ui;
        let mut buttons = Vec::new();
        collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
        let save = buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some("Save binding"))
            .unwrap();
        assert!(super::action_catalog_tests::catalog_drop(
            widgets,
            "Start timer",
            0
        ));
        ui.populate_action(&Action::Feature(FeatureAction::Timer { seconds: 45 }));
        save.emit_clicked();
        let LayerCommand::SaveBinding(primary, _) = fixture.requests.try_recv().unwrap() else {
            panic!("expected primary save")
        };
        primary.persist().unwrap();
        ui.hold_toggle.set_active(true);
        // Parameterless catalog actions use the real auto-save path.
        assert!(super::action_catalog_tests::catalog_drop(
            widgets,
            "Cancel timer",
            0
        ));
        let LayerCommand::SaveBinding(secondary, _) = fixture.requests.try_recv().unwrap() else {
            panic!("expected hold save")
        };
        secondary.persist().unwrap();
        // Reload exactly the store the editor's save command writes, not a
        // prebound substitute or the fixture's parent directory.
        let reloaded = LayerStore::open_at(secondary.root()).unwrap();
        assert_eq!(
            reloaded.binding("Base", &Trigger::KeyPress(1)),
            Some(Action::Feature(FeatureAction::Timer { seconds: 45 }))
        );
        assert_eq!(
            reloaded.binding("Base", &Trigger::KeyHold(1)),
            Some(Action::Feature(FeatureAction::CancelTimer))
        );
        *fixture.layers.borrow_mut() = reloaded;
        assert!(
            !fixture.runtime.timer_active(),
            "configuration must not execute either action"
        );
        fixture.hardware.try_iter().for_each(drop);
        let widgets = fixture.widgets.take().unwrap();
        let key = widgets.key_labels[0].clone();
        let (events, input) = mpsc::channel();
        let (commands, _) = mpsc::channel();
        let polling = start_event_updates(
            widgets,
            input,
            fixture.layers.clone(),
            fixture.artwork.clone(),
            commands,
            events.clone(),
        );
        let send = |pressed| {
            events
                .send(MonitorEvent::Input(protocol::Event::Key {
                    index: 0,
                    pressed,
                }))
                .unwrap()
        };
        await_label(&key, "KEY 1 · PRESSED", || send(true));
        assert!(!fixture.runtime.timer_active());
        await_label(&key, "KEY 1 · IDLE", || send(false));
        assert!(fixture.runtime.timer_active());
        assert!(fixture
            .hardware
            .try_iter()
            .flat_map(|command| match command {
                ArtworkCommand::Replace(commands) => commands,
                command => vec![command],
            })
            .any(|command| matches!(command,
            ArtworkCommand::ComposeInfo { target: ArtworkTarget::Keypad(1), value, .. }
                if value == InfoValue {
                    detail: "mm:ss".into(),
                    ..InfoValue::text(Glyph::Timer, "00:45")
                })));
        await_label(&fixture.runtime.timer_status, "Timer cancelled.", || {
            send(true)
        });
        assert!(!fixture.runtime.timer_active());
        let restored: Vec<_> = fixture
            .hardware
            .try_iter()
            .flat_map(|command| match command {
                ArtworkCommand::Replace(commands) => commands,
                command => vec![command],
            })
            .collect();
        assert!(
            restored.iter().any(|command| matches!(
                command,
                ArtworkCommand::ComposeIcon {
                    target: ArtworkTarget::Keypad(1),
                    icon: sdeck_plus::binding_icons::BindingIcon::Timer,
                    ..
                }
            )),
            "cancellation must restore normal key artwork"
        );
        await_label(&key, "KEY 1 · IDLE", || send(false));
        assert!(
            !fixture.runtime.timer_active(),
            "releasing a hold cannot run tap"
        );
        assert_eq!(fixture.runtime.timer_status.text(), "Timer cancelled.");
        polling.remove();
    });
}

#[test]
fn owner_switch_away_and_back_cancels_pending_tap_and_keeps_down_suppression() {
    run_gtk_test(|| {
        let fixture = HoldFixture::new("hold-owner");
        fixture.layers.borrow_mut().create("Other").unwrap();
        fixture
            .layers
            .borrow_mut()
            .bind(
                "Base",
                Trigger::KeyPress(1),
                Action::Feature(FeatureAction::Timer { seconds: 45 }),
            )
            .unwrap();
        fixture
            .layers
            .borrow_mut()
            .bind(
                "Base",
                Trigger::KeyHold(1),
                Action::Feature(FeatureAction::Timer { seconds: 60 }),
            )
            .unwrap();
        let input = |pressed| {
            fixture
                .runtime
                .execute_input(&protocol::Event::Key { index: 0, pressed })
        };
        input(true);
        fixture
            .runtime
            .execute("navigate".into(), Action::SwitchLayer("Other".into()));
        fixture
            .runtime
            .execute("navigate".into(), Action::SwitchLayer("Base".into()));
        input(true);
        input(false);
        assert!(!fixture.runtime.timer_active());
        input(true);
        input(false);
        assert_eq!(fixture.runtime.timer_status.text(), "Timer: 45s remaining");
    });
}

#[test]
fn physical_keys_without_hold_still_dispatch_immediately_once_and_encoders_are_unchanged() {
    run_gtk_test(|| {
        let fixture = HoldFixture::new("hold-preserve");
        fixture
            .layers
            .borrow_mut()
            .bind("Base", Trigger::KeyPress(1), Action::Command("true".into()))
            .unwrap();
        fixture
            .layers
            .borrow_mut()
            .bind(
                "Base",
                Trigger::EncoderPress(1),
                Action::Command("true".into()),
            )
            .unwrap();
        for pressed in [true, true, false, false] {
            fixture
                .runtime
                .execute_input(&protocol::Event::Key { index: 0, pressed });
        }
        assert!(
            matches!(fixture.requests.try_recv().unwrap(), LayerCommand::Execute { binding_id, .. } if binding_id == binding_identity("Base", &Trigger::KeyPress(1)))
        );
        assert!(fixture.requests.try_recv().is_err());
        fixture
            .runtime
            .execute_input(&protocol::Event::EncoderButton {
                index: 0,
                pressed: true,
            });
        assert!(matches!(
            fixture.requests.try_recv().unwrap(),
            LayerCommand::Execute { .. }
        ));
    });
}
