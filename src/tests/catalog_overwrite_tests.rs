//! A catalog action always replaces the draft or binding it is assigned over.
use super::action_catalog_tests::{catalog_drop, catalog_fixture};
use super::dial_action_tests::catalog_row;
use super::*;
use sdeck_plus::{
    desktop_actions::{DesktopAction, MediaCommand},
    features::FeatureAction,
};

fn media(command: MediaCommand) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Media(command)))
}

#[test]
fn choosing_another_action_replaces_the_draft_on_the_same_key() {
    run_gtk_test(|| {
        catalog_fixture("overwrite-key", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            catalog_row(widgets, "Run command").emit_clicked();
            assert_eq!(ui.action_kind(), ActionKind::RunCommand);
            catalog_row(widgets, "Shortcut").emit_clicked();
            assert_eq!(ui.action_kind(), ActionKind::Shortcut, "draft replaced");
            assert!(!ui.status.text().contains("Save or discard"));
            catalog_row(widgets, "Play / pause").emit_clicked();
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(1)),
                Some(media(MediaCommand::PlayPause))
            );
            assert!(!ui.is_dirty());
        })
    });
}

#[test]
fn choosing_an_action_for_a_saved_one_overwrites_it() {
    run_gtk_test(|| {
        catalog_fixture("overwrite-saved", |widgets, layers, _requests| {
            assert!(catalog_drop(widgets, "Play / pause", 1));
            assert!(catalog_drop(widgets, "Mute", 1));
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(media(MediaCommand::Mute))
            );
        })
    });
}

#[test]
fn dropping_on_another_key_asks_before_discarding_a_draft() {
    run_gtk_test(|| {
        catalog_fixture("overwrite-confirm", |widgets, layers, _requests| {
            catalog_row(widgets, "Run command").emit_clicked();
            assert!(catalog_drop(widgets, "Mute", 1), "drop is accepted");
            // Nothing changes until the prompt is answered; declining keeps the draft.
            discard_dialog().response(ResponseType::Cancel);
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(Action::Command("original".into()))
            );
            assert_eq!(widgets.binding_ui.action_kind(), ActionKind::RunCommand);
            assert!(catalog_drop(widgets, "Mute", 1));
            discard_dialog().response(ResponseType::Accept);
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(media(MediaCommand::Mute))
            );
        })
    });
}

#[test]
fn choosing_another_action_replaces_the_draft_on_the_same_dial_event() {
    run_gtk_test(|| {
        catalog_fixture("overwrite-dial", |widgets, layers, _requests| {
            let ui = &widgets.binding_ui;
            ui.select_control(ControlKind::Encoder, Some(1), true)
                .unwrap();
            ui.event_picker.set_selected(1);
            ui.load_selected_binding(layers);
            catalog_row(widgets, "Run command").emit_clicked();
            assert_eq!(ui.action_kind(), ActionKind::RunCommand);
            catalog_row(widgets, "Play / pause").emit_clicked();
            assert_eq!(
                layers
                    .borrow()
                    .binding("Base", &Trigger::EncoderClockwise(1)),
                Some(media(MediaCommand::PlayPause))
            );
        })
    });
}
