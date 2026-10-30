//! Saving a sound or file binding stores an app-owned link, not the real path.
use super::action_catalog_tests::{catalog_fixture, collect_catalog_buttons};
use super::*;
use sdeck_plus::{desktop_actions::DesktopAction, features::FeatureAction};

fn press_save(widgets: &MonitorWidgets) {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    buttons
        .iter()
        .find(|button| button_caption(button).as_deref() == Some("Save binding"))
        .unwrap()
        .emit_clicked();
}

fn desktop(action: DesktopAction) -> FeatureAction {
    FeatureAction::Desktop(action)
}

#[test]
fn saving_a_sound_stores_a_link_in_the_data_directory_and_the_editor_shows_the_real_file() {
    run_gtk_test(|| {
        catalog_fixture("link-sound", |widgets, layers, _requests| {
            let original = std::env::temp_dir().join(format!("beep-{}.ogg", std::process::id()));
            std::fs::write(&original, b"audio").unwrap();
            let ui = &widgets.binding_ui;
            ui.action_picker.set_selected(6);
            ui.update_action_visibility();
            ui.feature_editor
                .populate(&desktop(DesktopAction::PlayAudio {
                    path: original.clone(),
                }));
            press_save(widgets);
            let stored = layers.borrow().binding("Base", &Trigger::KeyPress(1));
            let Some(Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path }))) =
                stored
            else {
                panic!("sound binding expected");
            };
            let links = layers.borrow().root().join("links");
            assert!(path.starts_with(&links), "{path:?} is not in {links:?}");
            assert_eq!(std::fs::read_link(&path).unwrap(), original);
            // Reloading the control shows what the user chose, not our link.
            ui.load_selected_binding(layers);
            let shown = ui.feature_editor.parse().unwrap();
            assert_eq!(
                shown,
                desktop(DesktopAction::PlayAudio {
                    path: original.clone()
                })
            );
            assert!(!ui.is_dirty(), "showing the real file is not an edit");
            std::fs::remove_file(original).unwrap();
        })
    });
}

#[test]
fn saving_a_file_to_open_stores_a_link_too() {
    run_gtk_test(|| {
        catalog_fixture("link-open", |widgets, layers, _requests| {
            let original = std::env::temp_dir().join(format!("notes-{}.txt", std::process::id()));
            std::fs::write(&original, b"x").unwrap();
            let ui = &widgets.binding_ui;
            ui.action_picker.set_selected(5);
            ui.update_action_visibility();
            ui.feature_editor.populate(&desktop(DesktopAction::Open {
                target: original.to_string_lossy().into_owned(),
            }));
            press_save(widgets);
            let stored = layers.borrow().binding("Base", &Trigger::KeyPress(1));
            let Some(Action::Feature(FeatureAction::Desktop(DesktopAction::Open { target }))) =
                stored
            else {
                panic!("open binding expected");
            };
            assert!(target.contains("/links/"), "{target}");
            assert_eq!(std::fs::read_link(&target).unwrap(), original);
            std::fs::remove_file(original).unwrap();
        })
    });
}

#[test]
fn a_link_that_cannot_be_created_keeps_the_previous_binding_and_says_why() {
    run_gtk_test(|| {
        catalog_fixture("link-failure", |widgets, layers, _requests| {
            // A regular file where the links directory must go blocks linking.
            std::fs::write(layers.borrow().root().join("links"), b"in the way").unwrap();
            let ui = &widgets.binding_ui;
            ui.action_picker.set_selected(6);
            ui.update_action_visibility();
            ui.feature_editor
                .populate(&desktop(DesktopAction::PlayAudio {
                    path: "/tmp/anything.wav".into(),
                }));
            press_save(widgets);
            assert_eq!(layers.borrow().binding("Base", &Trigger::KeyPress(1)), None);
            assert!(
                ui.status.text().contains("Could not save binding"),
                "{}",
                ui.status.text()
            );
        })
    });
}
