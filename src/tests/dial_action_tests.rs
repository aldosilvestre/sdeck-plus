use super::action_catalog_tests::{catalog_fixture, collect_catalog_buttons};
use super::*;
use sdeck_plus::{
    desktop_actions::{DesktopAction, MediaCommand},
    features::FeatureAction,
};

fn media(command: MediaCommand) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Media(command)))
}

pub(super) fn catalog_row(widgets: &MonitorWidgets, caption: &str) -> Button {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    let tooltip =
        format!("Drag {caption} onto a key or dial, or click to assign to the selected control.");
    buttons
        .into_iter()
        .find(|button| button.tooltip_text().as_deref() == Some(tooltip.as_str()))
        .expect("catalog action row")
}

/// Drag a catalog action onto one of the four dial previews.
pub(super) fn drop_on_dial(widgets: &MonitorWidgets, caption: &str, dial: usize) -> bool {
    let row = catalog_row(widgets, caption);
    let controllers = row.observe_controllers();
    let source = (0..controllers.n_items())
        .find_map(|index| controllers.item(index)?.downcast::<gtk::DragSource>().ok())
        .unwrap();
    let provider = source
        .emit_by_name::<Option<gtk::gdk::ContentProvider>>("prepare", &[&0.0f64, &0.0f64])
        .unwrap();
    let kind = provider.formats().types()[0];
    let controllers = widgets.encoder_widgets[dial].observe_controllers();
    let target = (0..controllers.n_items())
        .filter_map(|index| controllers.item(index)?.downcast::<gtk::DropTarget>().ok())
        .find(|target| {
            target
                .formats()
                .is_some_and(|formats| formats.types().contains(&kind))
        })
        .expect("dial action target");
    let value = provider.value(kind).unwrap();
    target.emit_by_name::<bool>("drop", &[&glib::BoxedValue(value), &0.0f64, &0.0f64])
}

/// Pick one of the three events offered after a drop on a dial.
pub(super) fn choose_event(widgets: &MonitorWidgets, dial: usize, caption: &str) {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.encoder_widgets[dial].upcast_ref(), &mut buttons);
    buttons
        .into_iter()
        .find(|button| button.label().as_deref() == Some(caption))
        .unwrap_or_else(|| panic!("event choice {caption}"))
        .emit_clicked();
}

#[test]
fn dropping_an_action_on_a_dial_assigns_the_chosen_event_only() {
    run_gtk_test(|| {
        catalog_fixture("dial-action-ready", |widgets, layers, requests| {
            while requests.try_recv().is_ok() {}
            assert!(drop_on_dial(widgets, "Mute", 2));
            // Nothing is assigned until the event is chosen.
            assert_eq!(
                layers
                    .borrow()
                    .binding("Base", &Trigger::EncoderCounterClockwise(3)),
                None
            );
            choose_event(widgets, 2, "Turn counterclockwise");
            let store = layers.borrow();
            assert_eq!(
                store.binding("Base", &Trigger::EncoderCounterClockwise(3)),
                Some(media(MediaCommand::Mute))
            );
            assert_eq!(store.binding("Base", &Trigger::EncoderClockwise(3)), None);
            assert_eq!(store.binding("Base", &Trigger::EncoderPress(3)), None);
            assert_eq!(widgets.binding_ui.control_kind(), ControlKind::Encoder);
            assert_eq!(widgets.binding_ui.control_number(), Some(3));
            assert_eq!(
                widgets.binding_ui.event_kind(),
                EventKind::EncoderCounterClockwise
            );
            let mut saved = false;
            while let Ok(command) = requests.try_recv() {
                match command {
                    LayerCommand::SaveBinding(..) => saved = true,
                    LayerCommand::Execute { .. } => panic!("assigning must not execute"),
                    _ => {}
                }
            }
            assert!(saved, "choosing the event must enqueue persistence");
        })
    });
}

#[test]
fn dropping_an_incomplete_action_on_a_dial_keeps_the_binding_until_save() {
    run_gtk_test(|| {
        catalog_fixture("dial-action-draft", |widgets, layers, requests| {
            layers
                .borrow_mut()
                .bind(
                    "Base",
                    Trigger::EncoderClockwise(1),
                    Action::Command("original".into()),
                )
                .unwrap();
            while requests.try_recv().is_ok() {}
            assert!(drop_on_dial(widgets, "Run command", 0));
            choose_event(widgets, 0, "Turn clockwise");
            let ui = &widgets.binding_ui;
            assert_eq!(ui.action_kind(), ActionKind::RunCommand);
            assert_eq!(ui.event_kind(), EventKind::EncoderClockwise);
            assert!(ui.is_dirty());
            assert_eq!(
                layers
                    .borrow()
                    .binding("Base", &Trigger::EncoderClockwise(1)),
                Some(Action::Command("original".into()))
            );
            assert!(
                requests.try_recv().is_err(),
                "incomplete drop must not write"
            );
        })
    });
}

#[test]
fn clicking_an_action_assigns_it_to_the_selected_dial_event() {
    run_gtk_test(|| {
        catalog_fixture("dial-action-click", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            ui.select_control(ControlKind::Encoder, Some(2), true)
                .unwrap();
            ui.event_picker.set_selected(1);
            ui.load_selected_binding(layers);
            assert_eq!(ui.event_kind(), EventKind::EncoderClockwise);
            catalog_row(widgets, "Play / pause").emit_clicked();
            assert_eq!(
                layers
                    .borrow()
                    .binding("Base", &Trigger::EncoderClockwise(2)),
                Some(media(MediaCommand::PlayPause))
            );
        })
    });
}

#[test]
fn key_editor_hides_event_and_action_selectors_but_dials_keep_them() {
    run_gtk_test(|| {
        catalog_fixture("key-selectors", |widgets, _layers, _requests| {
            let ui = &widgets.binding_ui;
            // Key 1 is selected and unassigned: no selectors, no "do nothing" hint.
            assert!(!ui.event_picker.is_mapped(), "a key has one event");
            assert!(
                !ui.action_picker.is_mapped(),
                "actions come from the catalog"
            );
            assert!(
                !ui.action_guidance.is_mapped(),
                "nothing to explain for None"
            );
            // Key 2 runs a command: its guidance stays, the selectors do not return.
            key_button(&widgets.key_labels[1]).emit_clicked();
            assert!(ui.action_guidance.is_mapped(), "real guidance is kept");
            assert!(!ui.event_picker.is_mapped());
            assert!(!ui.action_picker.is_mapped());
            // A dial needs its three events and keeps the action selector.
            ui.select_control(ControlKind::Encoder, Some(1), true)
                .unwrap();
            assert!(ui.event_picker.is_mapped(), "dials choose their event");
            assert!(ui.action_picker.is_mapped());
        })
    });
}

fn dropdowns(widget: &gtk::Widget, found: &mut Vec<gtk::DropDown>) {
    if let Ok(dropdown) = widget.clone().downcast::<gtk::DropDown>() {
        found.push(dropdown);
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        dropdowns(&current, found);
        child = current.next_sibling();
    }
}

#[test]
fn key_editor_hides_category_and_action_dropdowns_but_keeps_the_description() {
    run_gtk_test(|| {
        catalog_fixture("key-feature-selectors", |widgets, _layers, _requests| {
            let ui = &widgets.binding_ui;
            catalog_row(widgets, "Alternate shortcuts").emit_clicked();
            let mut found = Vec::new();
            dropdowns(ui.feature_editor.root.upcast_ref(), &mut found);
            assert!(!found.is_empty(), "the editor still owns its selectors");
            assert!(
                found.iter().all(|dropdown| !dropdown.is_mapped()),
                "a key picks its action from the catalog"
            );
            assert!(
                ui.feature_editor.root.is_mapped(),
                "the action's own fields and description stay"
            );
            // A dial keeps choosing its action here.
            ui.select_control(ControlKind::Encoder, Some(1), true)
                .unwrap();
            ui.action_picker.set_selected(5);
            ui.update_action_visibility();
            let mut found = Vec::new();
            dropdowns(ui.feature_editor.root.upcast_ref(), &mut found);
            assert!(
                found.iter().filter(|dropdown| dropdown.is_mapped()).count() >= 2,
                "dials show Category and Action"
            );
        })
    });
}
