//! The "Multi action" catalog row (feature task T6, L15 advisory): like an unfit plugin
//! row it is dimmed while a dial or the touch strip is selected, and a drop on a dial is
//! refused with an error toast. Nothing is stored, saved or run.
use super::action_catalog_tests::{catalog_fixture, collect_catalog_buttons};
use super::dial_action_tests::{choose_event, drop_on_dial};
use super::*;

const UNFIT: &str = "This action does not support the selected control.";

fn drag_tooltip(caption: &str) -> String {
    format!("Drag {caption} onto a key or dial, or click to assign to the selected control.")
}

fn label_texts(widget: &gtk::Widget, found: &mut Vec<String>) {
    if let Some(label) = widget.downcast_ref::<Label>() {
        found.push(label.text().to_string());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        label_texts(&current, found);
        child = current.next_sibling();
    }
}

/// The catalog row captioned `caption`, found by its text: a dimmed row's tooltip changes.
fn row(widgets: &MonitorWidgets, caption: &str) -> Button {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    buttons
        .into_iter()
        .filter(|button| button.has_css_class("catalog-action"))
        .find(|button| {
            let mut texts = Vec::new();
            label_texts(button.upcast_ref(), &mut texts);
            texts.iter().any(|text| text == caption)
        })
        .unwrap_or_else(|| panic!("no catalog row {caption}"))
}

/// Whether `row` can be assigned, and the tooltip it shows.
fn state(row: &Button) -> (bool, String) {
    (
        row.is_sensitive(),
        row.tooltip_text().map(String::from).unwrap_or_default(),
    )
}

#[test]
fn the_multi_action_row_is_dimmed_and_refused_for_dials_and_the_touch_strip() {
    run_gtk_test(|| {
        catalog_fixture("multi-catalog-unfit", |widgets, layers, requests| {
            let ui = &widgets.binding_ui;
            let multi = row(widgets, "Multi action");
            let media = row(widgets, "Play / pause");
            let fits = (true, drag_tooltip("Multi action"));
            let unfit = (false, UNFIT.to_owned());
            ui.select_control(ControlKind::Button, Some(1), true)
                .unwrap();
            assert_eq!(state(&multi), fits, "a key press takes it");
            ui.hold_toggle.set_active(true);
            assert_eq!(state(&multi), fits, "a key hold takes it");
            ui.hold_toggle.set_active(false);
            ui.select_control(ControlKind::Encoder, Some(1), true)
                .unwrap();
            for event in 0..3 {
                ui.event_picker.set_selected(event);
                assert_eq!(state(&multi), unfit, "dial event {event}");
                // Other built-in actions still fit a dial.
                assert_eq!(state(&media), (true, drag_tooltip("Play / pause")));
            }
            ui.select_control(ControlKind::Touch, Some(0), true)
                .unwrap();
            assert_eq!(state(&multi), unfit, "the touch strip");
            ui.select_control(ControlKind::Button, Some(1), true)
                .unwrap();
            assert_eq!(state(&multi), fits, "back on a key");

            // Dragged from the catalog onto a dial, it is refused like an unfit plugin action.
            while requests.try_recv().is_ok() {}
            for (dial, event, control) in [
                (0, "Press", "dial press"),
                (1, "Turn clockwise", "dial turn"),
            ] {
                assert!(drop_on_dial(widgets, "Multi action", dial));
                choose_event(widgets, dial, event);
                let refusal = format!("Multi action cannot be assigned to a {control}.");
                assert_eq!(widgets.toast.label.text(), refusal);
                assert!(widgets.toast.root.reveals_child());
                assert!(widgets.toast.tone.has_css_class("feedback-error"));
                assert_eq!(ui.status.text(), refusal);
                // The editor stays on the key; nothing is stored, saved or run.
                assert_eq!(ui.control_kind(), ControlKind::Button);
                assert_eq!(ui.control_number(), Some(1));
            }
            let layers = layers.borrow();
            assert_eq!(layers.binding("Base", &Trigger::EncoderPress(1)), None);
            assert_eq!(layers.binding("Base", &Trigger::EncoderClockwise(2)), None);
            assert_eq!(layers.binding("Base", &Trigger::KeyPress(1)), None);
            assert!(requests.try_iter().all(|command| !matches!(
                command,
                LayerCommand::SaveBinding(..)
                    | LayerCommand::Execute { .. }
                    | LayerCommand::ExecuteStep { .. }
            )));
        })
    });
}
