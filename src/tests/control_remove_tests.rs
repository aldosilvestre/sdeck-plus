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

/// Use the Undo that the removal's toast offers.
fn undo(widgets: &MonitorWidgets) {
    assert!(
        widgets.toast.root.reveals_child(),
        "the removal toast is open"
    );
    assert!(widgets.toast.action.is_visible(), "the toast offers Undo");
    assert_eq!(widgets.toast.action.label().as_deref(), Some("Undo"));
    widgets.toast.action.emit_clicked();
}

fn select_key(widgets: &MonitorWidgets, layers: &RefCell<LayerStore>, key: u8) {
    let ui = &widgets.binding_ui;
    ui.select_control(ControlKind::Button, Some(key), true)
        .unwrap();
    ui.load_selected_binding(layers);
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
            ui.select_control(ControlKind::Button, Some(5), true)
                .unwrap();
            // Selecting another control neither replaces the toast nor redirects the undo.
            assert_eq!(widgets.toast.label.text(), "Control configuration removed.");
            while requests.try_recv().is_ok() {}
            undo(widgets);
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
            undo(widgets);
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
            undo(widgets);
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(Action::Command("original".into()))
            );
        })
    });
}

#[test]
fn removing_a_control_says_so_only_in_the_toast_and_leaves_no_undo_button() {
    run_gtk_test(|| {
        catalog_fixture("remove-toast-only", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            select_key(widgets, layers, 2);
            assert_eq!(ui.status.text(), "Binding loaded for editing.");
            click(widgets, "Remove binding");
            assert!(layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(2))
                .is_none());
            assert!(widgets.toast.root.reveals_child());
            assert_eq!(widgets.toast.label.text(), "Control configuration removed.");
            assert!(widgets.toast.action.is_visible());
            assert_eq!(widgets.toast.action.label().as_deref(), Some("Undo"));
            // The status line keeps no copy of the confirmation, nor its tone.
            assert_eq!(ui.status.text(), "");
            for tone in ["feedback-success", "feedback-warning", "feedback-error"] {
                assert!(!ui.status.has_css_class(tone), "{tone}");
            }
            let mut buttons = Vec::new();
            collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
            let undo_buttons: Vec<_> = buttons
                .iter()
                .filter_map(button_caption)
                .filter(|caption| caption.contains("Undo"))
                .collect();
            assert!(undo_buttons.is_empty(), "{undo_buttons:?}");
        })
    });
}

/// The side panel's toolbar, found from the window body: it holds only the plugin buttons row.
fn binding_toolbar(widgets: &MonitorWidgets) -> gtk::Widget {
    fn find(widget: &gtk::Widget) -> Option<gtk::Widget> {
        if widget.has_css_class("binding-toolbar") {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find(&current) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
    find(widgets.root.upcast_ref()).expect("the side panel has a binding toolbar")
}

#[test]
fn no_status_message_shows_below_the_plugin_buttons() {
    run_gtk_test(|| {
        catalog_fixture("remove-no-status-row", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            let toolbar = binding_toolbar(widgets);
            let buttons = toolbar.first_child().expect("plugin buttons row");
            assert!(buttons.next_sibling().is_none(), "only the buttons row");
            toolbar
                .next_sibling()
                .and_downcast::<gtk::StackSwitcher>()
                .expect("the Action/Dials/Binding tabs follow the toolbar");
            let check = |when: &str| {
                wait_for_frame(&toolbar);
                assert!(!ui.status.is_mapped(), "{when}: no inline message");
                assert!(!ui.status.is_ancestor(&toolbar), "{when}");
                assert_eq!(toolbar.height(), buttons.height(), "{when}: no blank row");
            };

            select_key(widgets, layers, 2);
            check("binding loaded");
            click(widgets, "Remove binding");
            check("removed");
            click(widgets, "Remove binding");
            check("nothing to remove");
            assert!(widgets.toast.root.reveals_child());
            assert_eq!(
                widgets.toast.label.text(),
                "No binding exists for that control."
            );
            ui.error("Could not save binding: disk full.");
            check("error");
        })
    });
}

#[test]
fn the_toast_shows_at_the_top_center_of_the_window() {
    run_gtk_test(|| {
        catalog_fixture("toast-top-center", |widgets, layers, _requests| {
            // The window shows its body under the toast's overlay, as the app does.
            let window = widgets
                .root
                .root()
                .and_downcast::<gtk::Window>()
                .expect("the body sits in a window");
            window.set_child(None::<&gtk::Widget>);
            window.set_child(Some(&widgets.toast.wrap(&widgets.root)));
            wait_for_frame(&window);
            select_key(widgets, layers, 2);
            click(widgets, "Remove binding");
            let toast = &widgets.toast.root;
            let overlay = toast.parent().expect("the toast overlays the window body");
            wait_for_frame(toast);
            let bounds = toast
                .compute_bounds(&overlay)
                .expect("the toast is laid out");
            assert!(bounds.height() > 0.0);
            assert!(
                (bounds.y() - 24.0).abs() < 0.5,
                "the toast sits at the top: {}",
                bounds.y()
            );
            let center = bounds.x() + bounds.width() / 2.0;
            let middle = overlay.width() as f32 / 2.0;
            assert!(
                (center - middle).abs() < 1.0,
                "the toast is centered: {center} vs {middle}"
            );
        })
    });
}

#[test]
fn the_undo_toast_stays_ten_seconds_and_then_can_no_longer_restore() {
    run_gtk_test(|| {
        catalog_fixture("remove-undo-expiry", |widgets, layers, _requests| {
            select_key(widgets, layers, 2);
            let shown = std::time::Instant::now();
            click(widgets, "Remove binding");
            let lifetime = ui::time_until_closed(&widgets.toast, shown, Duration::from_secs(15))
                .expect("the removal toast closes by itself");
            assert!(lifetime >= Duration::from_secs(10), "{lifetime:?}");
            // An expired Undo is inert, even when triggered programmatically.
            widgets.toast.action.emit_clicked();
            assert!(layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(2))
                .is_none());
            assert!(!widgets.toast.root.reveals_child());
            assert_eq!(widgets.binding_ui.status.text(), "");
        })
    });
}

#[test]
fn a_later_removal_replaces_the_earlier_undo_which_runs_once() {
    run_gtk_test(|| {
        catalog_fixture("remove-latest-wins", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            layers
                .borrow_mut()
                .bind(
                    "Base",
                    Trigger::KeyPress(3),
                    Action::Command("third".into()),
                )
                .unwrap();
            select_key(widgets, layers, 2);
            click(widgets, "Remove binding");
            select_key(widgets, layers, 3);
            click(widgets, "Remove binding");
            undo(widgets);
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(3)),
                Some(Action::Command("third".into()))
            );
            assert!(
                layers
                    .borrow()
                    .binding("Base", &Trigger::KeyPress(2))
                    .is_none(),
                "the earlier removal's undo was replaced"
            );
            // A second use finds nothing to run: no refusal, no second restore.
            widgets.toast.action.emit_clicked();
            assert!(!ui.status.text().contains("Cannot undo"));
            assert!(!widgets.toast.root.reveals_child());
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
            layers
                .borrow_mut()
                .bind(
                    "Base",
                    Trigger::KeyPress(2),
                    Action::Command("newer".into()),
                )
                .unwrap();
            undo(widgets);
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(Action::Command("newer".into()))
            );
            assert!(ui.status.text().contains("Cannot undo"));
            // The refusal replaces the Undo toast; it offers nothing to retry.
            assert_eq!(
                widgets.toast.label.text(),
                "Cannot undo: this control was changed after it was removed."
            );
            assert!(widgets.toast.tone.has_css_class("feedback-error"));
            assert!(!widgets.toast.action.is_visible());
            widgets.toast.action.emit_clicked();
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(Action::Command("newer".into()))
            );
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
        undo(widgets);
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
            // The toast says what happened and carries the only undo.
            assert!(widgets.toast.label.text().contains("removed"));
            undo(widgets);
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::EncoderPress(2)),
                Some(media(MediaCommand::Mute))
            );
            assert_eq!(ui.status.text(), "Binding restored; saving to disk…");
            assert!(!widgets.toast.root.reveals_child());
        })
    });
}
