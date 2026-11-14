use super::action_catalog_tests::{catalog_fixture, collect_catalog_buttons};
use super::*;
use sdeck_plus::{
    desktop_actions::{DesktopAction, MediaCommand},
    features::FeatureAction,
    info::InfoSource,
    layers::PageReturn,
};

fn media(command: MediaCommand) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Media(command)))
}

fn click(widgets: &MonitorWidgets, caption: &str) {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    buttons
        .iter()
        .find(|button| button_caption(button).as_deref() == Some(caption))
        .unwrap_or_else(|| panic!("{caption} button"))
        .emit_clicked();
}

/// Click a Dials-tab preset to assign it to the selected dial.
fn apply_preset(widgets: &MonitorWidgets, caption: &str) {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    let tooltip = format!("Drag {caption} onto a dial, or click to assign to the selected dial.");
    buttons
        .iter()
        .find(|button| button.tooltip_text().as_deref() == Some(tooltip.as_str()))
        .expect("dial preset row")
        .emit_clicked();
}

fn select_dial(widgets: &MonitorWidgets, layers: &RefCell<LayerStore>, dial: u8) {
    let ui = &widgets.binding_ui;
    ui.select_control(ControlKind::Encoder, Some(dial), true)
        .unwrap();
    ui.load_selected_binding(layers);
}

#[test]
fn removing_a_dial_after_a_preset_clears_every_turn_and_the_legend() {
    run_gtk_test(|| {
        catalog_fixture("remove-dial-preset", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            select_dial(widgets, layers, 1);
            apply_preset(widgets, "Volume");
            assert!(ui.dial_legends[0].is_visible());
            // The event picker is back on Press, yet the whole dial goes.
            click(widgets, "Remove binding");
            let store = layers.borrow();
            for trigger in [
                Trigger::EncoderPress(1),
                Trigger::EncoderClockwise(1),
                Trigger::EncoderCounterClockwise(1),
            ] {
                assert!(store.binding("Base", &trigger).is_none(), "{trigger:?}");
            }
            assert!(!ui.dial_legends[0].is_visible());
            assert!(!ui
                .summary(ControlRef::Dial(1))
                .expect("dial 1 has a summary")
                .text()
                .contains(": Volume"));
        })
    });
}

#[test]
fn one_undo_restores_every_event_return_and_display_of_the_dial() {
    run_gtk_test(|| {
        catalog_fixture("remove-dial-undo", |widgets, layers, requests| {
            let ui = &widgets.binding_ui;
            {
                let mut store = layers.borrow_mut();
                store
                    .bind(
                        "Base",
                        Trigger::EncoderClockwise(2),
                        media(MediaCommand::VolumeUp),
                    )
                    .unwrap();
                store
                    .bind("Base", Trigger::EncoderPress(2), media(MediaCommand::Mute))
                    .unwrap();
                store
                    .bind("Base", Trigger::Rotation(2, 3), media(MediaCommand::Next))
                    .unwrap();
                store
                    .set_return(
                        "Base",
                        &Trigger::EncoderClockwise(2),
                        Some(PageReturn {
                            layer: "Base".into(),
                            seconds: 20,
                        }),
                    )
                    .unwrap();
                store
                    .set_info("Base", &Trigger::EncoderPress(2), Some(InfoSource::Clock))
                    .unwrap();
            }
            select_dial(widgets, layers, 2);
            while requests.try_recv().is_ok() {}
            // Remove with Press selected, then move to another control.
            click(widgets, "Remove binding");
            assert!(layers
                .borrow()
                .binding("Base", &Trigger::Rotation(2, 3))
                .is_none());
            assert!(ui.undo_remove.is_visible());
            ui.select_control(ControlKind::Button, Some(5), true)
                .unwrap();
            while requests.try_recv().is_ok() {}
            ui.undo_remove.emit_clicked();
            let store = layers.borrow();
            assert_eq!(
                store.binding("Base", &Trigger::EncoderClockwise(2)),
                Some(media(MediaCommand::VolumeUp))
            );
            assert_eq!(
                store.binding("Base", &Trigger::EncoderPress(2)),
                Some(media(MediaCommand::Mute))
            );
            assert_eq!(
                store.binding("Base", &Trigger::Rotation(2, 3)),
                Some(media(MediaCommand::Next))
            );
            assert_eq!(
                store
                    .page_return("Base", &Trigger::EncoderClockwise(2))
                    .map(|r| r.seconds),
                Some(20)
            );
            assert_eq!(
                store.info_source("Base", &Trigger::EncoderPress(2)),
                Some(InfoSource::Clock)
            );
            assert!(store.binding("Base", &Trigger::KeyPress(5)).is_none());
            assert!(matches!(
                requests.try_recv().unwrap(),
                LayerCommand::SaveBinding(..)
            ));
        })
    });
}

#[test]
fn removing_a_key_with_a_hold_action_clears_both_and_undo_restores_both() {
    run_gtk_test(|| {
        catalog_fixture("remove-key-hold", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            {
                let mut store = layers.borrow_mut();
                store
                    .bind("Base", Trigger::KeyHold(3), media(MediaCommand::Mute))
                    .unwrap();
                store
                    .bind("Base", Trigger::KeyPress(3), media(MediaCommand::Next))
                    .unwrap();
            }
            ui.select_control(ControlKind::Button, Some(3), true)
                .unwrap();
            ui.load_selected_binding(layers);
            click(widgets, "Remove binding");
            assert!(layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(3))
                .is_none());
            assert!(layers
                .borrow()
                .binding("Base", &Trigger::KeyHold(3))
                .is_none());
            ui.undo_remove.emit_clicked();
            assert!(layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(3))
                .is_some());
            assert!(layers
                .borrow()
                .binding("Base", &Trigger::KeyHold(3))
                .is_some());
        })
    });
}

#[test]
fn removing_a_key_with_a_single_event_works_as_before() {
    run_gtk_test(|| {
        catalog_fixture("remove-key-single", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            ui.select_control(ControlKind::Button, Some(2), true)
                .unwrap();
            ui.load_selected_binding(layers);
            click(widgets, "Remove binding");
            assert!(layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(2))
                .is_none());
            ui.undo_remove.emit_clicked();
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(Action::Command("original".into()))
            );
        })
    });
}

#[test]
fn applying_a_preset_drops_the_stale_return_and_display_of_replaced_events() {
    run_gtk_test(|| {
        catalog_fixture("preset-stale", |widgets, layers, _requests| {
            {
                let mut store = layers.borrow_mut();
                store
                    .bind(
                        "Base",
                        Trigger::EncoderClockwise(1),
                        media(MediaCommand::Next),
                    )
                    .unwrap();
                store
                    .set_return(
                        "Base",
                        &Trigger::EncoderClockwise(1),
                        Some(PageReturn {
                            layer: "Base".into(),
                            seconds: 9,
                        }),
                    )
                    .unwrap();
                store
                    .set_info(
                        "Base",
                        &Trigger::EncoderClockwise(1),
                        Some(InfoSource::Clock),
                    )
                    .unwrap();
            }
            select_dial(widgets, layers, 1);
            apply_preset(widgets, "Volume");
            let store = layers.borrow();
            assert_eq!(
                store.binding("Base", &Trigger::EncoderClockwise(1)),
                Some(media(MediaCommand::VolumeUp))
            );
            assert_eq!(
                store.page_return("Base", &Trigger::EncoderClockwise(1)),
                None
            );
            assert_eq!(
                store.info_source("Base", &Trigger::EncoderClockwise(1)),
                None
            );
        })
    });
}

#[test]
fn undo_after_a_later_edit_refuses_and_keeps_the_new_binding() {
    run_gtk_test(|| {
        catalog_fixture("undo-conflict", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            ui.select_control(ControlKind::Button, Some(2), true)
                .unwrap();
            ui.load_selected_binding(layers);
            click(widgets, "Remove binding");
            assert!(ui.undo_remove.is_visible());
            layers
                .borrow_mut()
                .bind(
                    "Base",
                    Trigger::KeyPress(2),
                    Action::Command("newer".into()),
                )
                .unwrap();
            ui.undo_remove.emit_clicked();
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(Action::Command("newer".into()))
            );
            assert!(!ui.undo_remove.is_visible());
            assert!(ui.removed.borrow().is_none());
            assert!(ui.status.text().contains("Cannot undo"));
        })
    });
}

fn touch_remove_and_undo(number: u8, gone: &[Trigger], kept: &[Trigger]) {
    catalog_fixture("touch-remove", |widgets, layers, _requests| {
        let ui = &widgets.binding_ui;
        let everything = [
            Trigger::ZoneTap(2),
            Trigger::ZonePress(2),
            Trigger::ZoneTap(3),
            Trigger::TouchTap,
            Trigger::TouchFlick,
            Trigger::TouchSwipe(sdeck_plus::layers::SwipeDirection::Left),
        ];
        for trigger in &everything {
            layers
                .borrow_mut()
                .bind("Base", trigger.clone(), Action::Command("t".into()))
                .unwrap();
        }
        ui.select_control(ControlKind::Touch, Some(number), true)
            .unwrap();
        ui.load_selected_binding(layers);
        click(widgets, "Remove binding");
        for trigger in gone {
            assert!(
                layers.borrow().binding("Base", trigger).is_none(),
                "{trigger:?}"
            );
        }
        for trigger in kept {
            assert!(
                layers.borrow().binding("Base", trigger).is_some(),
                "{trigger:?}"
            );
        }
        ui.undo_remove.emit_clicked();
        for trigger in &everything {
            assert!(
                layers.borrow().binding("Base", trigger).is_some(),
                "{trigger:?}"
            );
        }
    })
}

#[test]
fn removing_a_touch_section_keeps_the_strip_and_other_sections() {
    run_gtk_test(|| {
        touch_remove_and_undo(
            2,
            &[Trigger::ZoneTap(2), Trigger::ZonePress(2)],
            &[Trigger::ZoneTap(3), Trigger::TouchTap, Trigger::TouchFlick],
        )
    });
}

#[test]
fn removing_the_touch_strip_keeps_the_sections() {
    run_gtk_test(|| {
        touch_remove_and_undo(
            0,
            &[
                Trigger::TouchTap,
                Trigger::TouchFlick,
                Trigger::TouchSwipe(sdeck_plus::layers::SwipeDirection::Left),
            ],
            &[Trigger::ZoneTap(3), Trigger::ZoneTap(2)],
        )
    });
}

#[test]
fn removing_a_binding_offers_undo_in_the_toast_and_using_it_restores_the_binding() {
    run_gtk_test(|| {
        catalog_fixture("remove-toast-undo", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            {
                let mut store = layers.borrow_mut();
                store
                    .bind("Base", Trigger::EncoderPress(2), media(MediaCommand::Mute))
                    .unwrap();
            }
            select_dial(widgets, layers, 2);
            click(widgets, "Remove binding");
            assert!(layers
                .borrow()
                .binding("Base", &Trigger::EncoderPress(2))
                .is_none());
            // The toast says what happened and carries the same undo as the toolbar button.
            assert!(widgets.toast.action.is_visible());
            assert_eq!(widgets.toast.action.label().as_deref(), Some("Undo"));
            assert!(widgets.toast.label.text().contains("removed"));
            assert!(ui.undo_remove.is_visible());
            widgets.toast.action.emit_clicked();
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::EncoderPress(2)),
                Some(media(MediaCommand::Mute))
            );
            assert!(!ui.undo_remove.is_visible());
            assert!(!widgets.toast.root.reveals_child());
        })
    });
}
