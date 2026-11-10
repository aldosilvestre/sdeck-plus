//! The XL+ session end to end: every control of the model can be selected,
//! edited, saved, tested, removed and undone, and the editor, catalog and
//! pictures follow the model's shape and key size. No real device, HOME or
//! network is involved: the window runs over scratch stores and a channel
//! that stands in for the layer worker.
use super::action_catalog_tests::collect_catalog_buttons;
use super::preview_model_tests::{class_of, model_fixture, preview_of, ModelFixture};
use super::*;
use sdeck_plus::{
    binding_editor::{control_number_error, ControlKind},
    desktop_actions::{DesktopAction, MediaCommand},
    device_model::{DeviceModel, PLUS, PLUS_XL},
    features::FeatureAction,
    preview_layout::ControlRef,
};

fn media(command: MediaCommand) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Media(command)))
}

fn fixture(tag: &str, model: &'static DeviceModel) -> Option<ModelFixture> {
    model_fixture(tag, model, |_, _| {})
}

/// Opt-in capture of the window for a human to look at.
fn capture(f: &ModelFixture, name: &str) {
    if let Some(directory) = std::env::var_os("STREAMDECK_XL_QA_DIR") {
        wait_for_frame(&f.window);
        std::fs::create_dir_all(&directory).unwrap();
        rendered_texture(&f.window, &f.widgets.root)
            .save_to_png(std::path::Path::new(&directory).join(format!("{name}.png")))
            .unwrap();
    }
}

fn click(f: &ModelFixture, caption: &str) {
    let mut buttons = Vec::new();
    collect_catalog_buttons(f.widgets.root.upcast_ref(), &mut buttons);
    buttons
        .iter()
        .find(|button| button_caption(button).as_deref() == Some(caption))
        .unwrap_or_else(|| panic!("{caption} button"))
        .emit_clicked();
}

/// Select `control` through its production preview button.
fn select(f: &ModelFixture, control: ControlRef) {
    f.widgets
        .preview
        .control(control)
        .unwrap_or_else(|| panic!("{control:?} is on the shown preview"))
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
}

fn requests(f: &ModelFixture) -> &mpsc::Receiver<LayerCommand> {
    f.requests.as_ref().unwrap()
}

fn executed(f: &ModelFixture) -> Vec<(String, Action)> {
    requests(f)
        .try_iter()
        .filter_map(|command| match command {
            LayerCommand::Execute { binding_id, action } => Some((binding_id, action)),
            _ => None,
        })
        .collect()
}

/// Every control an XL+ has that a Stream Deck + lacks, and the strip, with the
/// event the editor must offer first and the trigger it stands for.
fn xl_cases() -> Vec<(ControlRef, usize, Trigger)> {
    vec![
        (ControlRef::Key(36), 0, Trigger::KeyPress(36)),
        (ControlRef::Key(20), 0, Trigger::KeyPress(20)),
        (ControlRef::Key(9), 0, Trigger::KeyPress(9)),
        (ControlRef::Dial(6), 1, Trigger::EncoderClockwise(6)),
        (ControlRef::Dial(5), 0, Trigger::EncoderPress(5)),
        (ControlRef::Zone(6), 0, Trigger::ZoneTap(6)),
        (ControlRef::Zone(5), 0, Trigger::ZoneTap(5)),
        (ControlRef::Strip, 0, Trigger::TouchTap),
    ]
}

#[test]
fn every_xl_control_can_be_edited_saved_and_read_back_from_disk() {
    run_gtk_test(|| {
        let Some(f) = fixture("xl-save", &PLUS_XL) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        for (control, event, trigger) in xl_cases() {
            select(&f, control);
            assert_eq!(ui.selected_control(), Some(control), "{control:?}");
            ui.event_picker.set_selected(event as u32);
            assert_ne!(
                ui.status.text(),
                "Select a valid control event.",
                "{control:?} is a valid control"
            );
            assert_eq!(
                ui.selected_layer_and_trigger(&f.layers).unwrap().1,
                trigger,
                "{control:?}"
            );
            let command = format!("echo {control:?}");
            ui.action_picker.set_selected(1);
            ui.text_entry.set_text(&command);
            while requests(&f).try_recv().is_ok() {}
            click(&f, "Save binding");
            assert!(
                executed(&f).is_empty(),
                "{control:?}: saving never runs the action"
            );
            assert_eq!(
                f.layers.borrow().binding("Base", &trigger),
                Some(Action::Command(command.clone())),
                "{control:?}"
            );
            // The store the layer worker would write, read back from disk.
            let snapshot = f.layers.borrow().clone();
            snapshot.persist().unwrap();
            let reloaded = LayerStore::open_at_model(snapshot.root(), &PLUS_XL).unwrap();
            assert_eq!(
                reloaded.binding("Base", &trigger),
                Some(Action::Command(command)),
                "{control:?} survives a reload"
            );
        }
        // The keypad summary shows what was saved on the last keys.
        select(&f, ControlRef::Key(36));
        capture(&f, "xl-key-36-saved");
        assert!(ui
            .summary(ControlRef::Key(36))
            .unwrap()
            .text()
            .contains("echo Key(36)"));
    });
}

#[test]
fn test_action_runs_the_xl_binding_and_saving_never_does() {
    run_gtk_test(|| {
        let Some(f) = fixture("xl-test-action", &PLUS_XL) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        for (control, trigger) in [
            (ControlRef::Key(36), Trigger::KeyPress(36)),
            (ControlRef::Dial(6), Trigger::EncoderPress(6)),
            (ControlRef::Zone(6), Trigger::ZoneTap(6)),
        ] {
            select(&f, control);
            ui.action_picker.set_selected(1);
            ui.text_entry.set_text("echo guarded");
            while requests(&f).try_recv().is_ok() {}
            click(&f, "Save binding");
            assert!(executed(&f).is_empty(), "{control:?}");
            click(&f, "Test action");
            assert_eq!(
                executed(&f),
                [(
                    binding_identity("Base", &trigger),
                    Action::Command("echo guarded".into())
                )],
                "{control:?}: Test action runs it once"
            );
        }
    });
}

/// Every trigger of `control` is bound, so a removal has something to clear.
fn bind_whole_control(f: &ModelFixture, control: ControlRef) -> Vec<Trigger> {
    let triggers = match control {
        ControlRef::Key(number) => vec![Trigger::KeyPress(number), Trigger::KeyHold(number)],
        ControlRef::Dial(number) => vec![
            Trigger::EncoderPress(number),
            Trigger::EncoderClockwise(number),
            Trigger::EncoderCounterClockwise(number),
        ],
        ControlRef::Zone(number) => vec![Trigger::ZoneTap(number), Trigger::ZonePress(number)],
        ControlRef::Strip => vec![Trigger::TouchTap, Trigger::TouchPress],
    };
    let mut layers = f.layers.borrow_mut();
    for (index, trigger) in triggers.iter().enumerate() {
        layers
            .bind(
                "Base",
                trigger.clone(),
                Action::Command(format!("echo {index}")),
            )
            .unwrap();
    }
    triggers
}

#[test]
fn removing_an_xl_control_clears_all_of_it_and_one_undo_restores_it() {
    run_gtk_test(|| {
        let Some(f) = fixture("xl-remove", &PLUS_XL) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        for control in [
            ControlRef::Key(36),
            ControlRef::Key(20),
            ControlRef::Dial(6),
            ControlRef::Dial(5),
            ControlRef::Zone(6),
            ControlRef::Zone(5),
            ControlRef::Strip,
        ] {
            let triggers = bind_whole_control(&f, control);
            select(&f, control);
            click(&f, "Remove binding");
            for trigger in &triggers {
                assert!(
                    f.layers.borrow().binding("Base", trigger).is_none(),
                    "{control:?} {trigger:?} is removed"
                );
            }
            assert!(ui.undo_remove.is_visible(), "{control:?}");
            ui.undo_remove.emit_clicked();
            for (index, trigger) in triggers.iter().enumerate() {
                assert_eq!(
                    f.layers.borrow().binding("Base", trigger),
                    Some(Action::Command(format!("echo {index}"))),
                    "{control:?} {trigger:?} is restored"
                );
            }
            assert!(!ui.undo_remove.is_visible(), "{control:?}");
        }
    });
}

#[test]
fn undo_on_an_xl_control_refuses_after_a_later_edit() {
    run_gtk_test(|| {
        let Some(f) = fixture("xl-undo-refused", &PLUS_XL) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        for (control, trigger) in [
            (ControlRef::Key(36), Trigger::KeyPress(36)),
            (ControlRef::Dial(6), Trigger::EncoderPress(6)),
        ] {
            bind_whole_control(&f, control);
            select(&f, control);
            click(&f, "Remove binding");
            f.layers
                .borrow_mut()
                .bind("Base", trigger.clone(), Action::Command("newer".into()))
                .unwrap();
            ui.undo_remove.emit_clicked();
            assert_eq!(
                f.layers.borrow().binding("Base", &trigger),
                Some(Action::Command("newer".into())),
                "{control:?}"
            );
            assert!(ui.status.text().contains("Cannot undo"), "{control:?}");
            assert!(!ui.undo_remove.is_visible());
        }
    });
}

fn numbers(model: &gtk::StringList) -> Vec<String> {
    (0..model.n_items())
        .filter_map(|index| model.string(index))
        .map(|text| text.to_string())
        .collect()
}

fn range(last: u8) -> Vec<String> {
    (1..=last).map(|number| number.to_string()).collect()
}

#[test]
fn the_control_picker_lists_the_numbers_of_the_model_and_names_its_range() {
    run_gtk_test(|| {
        for (model, keys, dials) in [(&PLUS, 8u8, 4u8), (&PLUS_XL, 36, 6)] {
            let Some(f) = fixture("pickers", model) else {
                return;
            };
            let ui = &f.widgets.binding_ui;
            ui.select_control(ControlKind::Button, Some(1), true)
                .unwrap();
            assert_eq!(
                numbers(&ui.control_number_model),
                range(keys),
                "{}",
                model.id
            );
            ui.select_control(ControlKind::Encoder, Some(1), true)
                .unwrap();
            assert_eq!(numbers(&ui.control_number_model), range(dials));
            ui.select_control(ControlKind::Touch, None, true).unwrap();
            let mut touch = vec!["Any".to_owned()];
            touch.extend(range(dials));
            assert_eq!(numbers(&ui.control_number_model), touch);
            assert_eq!(
                ui.select_control(ControlKind::Button, Some(keys + 1), true),
                Err(format!("button number must be 1 through {keys}"))
            );
            assert_eq!(
                ui.select_control(ControlKind::Encoder, Some(dials + 1), true),
                Err(format!("encoder number must be 1 through {dials}"))
            );
            assert_eq!(
                ui.select_control(ControlKind::Touch, Some(dials + 1), true),
                Err(format!(
                    "touch section must be 0 (whole strip) through {dials}"
                ))
            );
        }
    });
}

#[test]
fn a_control_the_model_lacks_is_named_clearly_instead_of_out_of_range() {
    assert_eq!(
        control_number_error(&PLUS, ControlKind::Button, 9).unwrap(),
        "Button 9 does not exist on the Stream Deck +; its buttons are 1..=8."
    );
    assert_eq!(
        control_number_error(&PLUS_XL, ControlKind::Encoder, 7).unwrap(),
        "Encoder 7 does not exist on the Stream Deck + XL; its encoders are 1..=6."
    );
    assert!(control_number_error(&PLUS_XL, ControlKind::Button, 36).is_none());
    assert!(control_number_error(&PLUS_XL, ControlKind::Touch, 0).is_none());
    run_gtk_test(|| {
        let Some(f) = fixture("lacks", &PLUS_XL) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        select(&f, ControlRef::Key(36));
        // The editor is left on a key the model it follows does not have.
        ui.set_model(&PLUS);
        ui.load_selected_binding(&f.layers);
        assert_eq!(
            ui.status.text(),
            "Button 36 does not exist on the Stream Deck +; its buttons are 1..=8."
        );
        assert!(ui.selected_layer_and_trigger(&f.layers).is_err());
    });
}

/// The page keeps its header and profile cards above the preview; the whole
/// card must still sit inside the viewport at the default window size, so the
/// page needs no vertical scroll to reach the bottom of the chassis.
#[test]
fn the_preview_card_fits_the_default_window_without_scrolling() {
    run_gtk_test(|| {
        for model in [&PLUS, &PLUS_XL] {
            let Some(f) = model_fixture("fit", model, |_, _| {}) else {
                return;
            };
            let root: gtk::Widget = f.widgets.root.clone().upcast();
            if let Some(directory) = std::env::var_os("STREAMDECK_XL_QA_DIR") {
                std::fs::create_dir_all(&directory).unwrap();
                rendered_texture(&f.window, &f.widgets.root)
                    .save_to_png(std::path::Path::new(&directory).join(format!("{}.png", model.id)))
                    .unwrap();
            }
            let card = class_of(&root, "device-canvas");
            let bounds = card.compute_bounds(&root).unwrap();
            let (bottom, visible) = ((bounds.y() + bounds.height()).ceil() as i32, root.height());
            assert_eq!(visible, DEFAULT_HEIGHT, "{}", model.id);
            assert!(
                bottom <= visible,
                "{}: the card ends at {bottom}, below the {visible}px viewport",
                model.id
            );
            assert!(
                preview_of(&root)
                    .compute_bounds(&root)
                    .is_some_and(|bounds| bounds.y() + bounds.height() <= visible as f32),
                "{}",
                model.id
            );
        }
    });
}

/// Drag the catalog row with `tooltip` onto `target`; whether the drop was taken.
fn drop_on(f: &ModelFixture, tooltip: &str, target: ControlRef) -> bool {
    let mut buttons = Vec::new();
    collect_catalog_buttons(f.widgets.root.upcast_ref(), &mut buttons);
    let row = buttons
        .iter()
        .find(|button| button.tooltip_text().as_deref() == Some(tooltip))
        .unwrap_or_else(|| panic!("row {tooltip}"));
    let controllers = row.observe_controllers();
    let source = (0..controllers.n_items())
        .find_map(|index| controllers.item(index)?.downcast::<gtk::DragSource>().ok())
        .unwrap();
    let provider = source
        .emit_by_name::<Option<gtk::gdk::ContentProvider>>("prepare", &[&0.0f64, &0.0f64])
        .unwrap();
    let kind = provider.formats().types()[0];
    let control = f.widgets.preview.control(target).unwrap();
    let controllers = control.observe_controllers();
    let accepting = (0..controllers.n_items())
        .filter_map(|index| controllers.item(index)?.downcast::<gtk::DropTarget>().ok())
        .find(|candidate| {
            candidate
                .formats()
                .is_some_and(|formats| formats.types().contains(&kind))
        })
        .unwrap_or_else(|| panic!("{target:?} accepts {tooltip}"));
    let value = provider.value(kind).unwrap();
    accepting.emit_by_name::<bool>("drop", &[&glib::BoxedValue(value), &0.0f64, &0.0f64])
}

const CATALOG_PLAY: &str =
    "Drag Play / pause onto a key or dial, or click to assign to the selected control.";

#[test]
fn a_catalog_action_dropped_on_xl_key_20_is_saved_and_never_executed() {
    run_gtk_test(|| {
        let Some(f) = fixture("xl-catalog-key", &PLUS_XL) else {
            return;
        };
        while requests(&f).try_recv().is_ok() {}
        assert!(drop_on(&f, CATALOG_PLAY, ControlRef::Key(20)));
        assert_eq!(
            f.layers.borrow().binding("Base", &Trigger::KeyPress(20)),
            Some(media(MediaCommand::PlayPause))
        );
        assert_eq!(
            f.widgets.binding_ui.selected_control(),
            Some(ControlRef::Key(20))
        );
        let commands: Vec<_> = requests(&f).try_iter().collect();
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, LayerCommand::SaveBinding(..))),
            "the drop is saved"
        );
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, LayerCommand::Execute { .. })),
            "the drop never runs the action"
        );
        let saved = commands
            .into_iter()
            .find_map(|command| match command {
                LayerCommand::SaveBinding(store, _) => Some(store),
                _ => None,
            })
            .unwrap();
        saved.persist().unwrap();
        let reloaded = LayerStore::open_at_model(saved.root(), &PLUS_XL).unwrap();
        assert_eq!(
            reloaded.binding("Base", &Trigger::KeyPress(20)),
            Some(media(MediaCommand::PlayPause))
        );
    });
}

#[test]
fn every_xl_key_and_dial_takes_a_drop_and_the_plus_keeps_eight_and_four() {
    run_gtk_test(|| {
        for (model, keys, dials) in [(&PLUS, 8u8, 4u8), (&PLUS_XL, 36, 6)] {
            let Some(f) = fixture("drop-targets", model) else {
                return;
            };
            let has_target = |control: ControlRef, tooltip: &str| {
                let widget = f.widgets.preview.control(control).unwrap();
                let controllers = widget.observe_controllers();
                (0..controllers.n_items()).any(|index| {
                    controllers
                        .item(index)
                        .and_then(|controller| controller.downcast::<gtk::DropTarget>().ok())
                        .is_some_and(|target| {
                            target.formats().is_some_and(|formats| {
                                formats.types().iter().any(|kind| {
                                    kind.name()
                                        == if tooltip == "key" {
                                            "SdeckCatalogAction"
                                        } else {
                                            "SdeckDialPreset"
                                        }
                                })
                            })
                        })
                })
            };
            for key in 1..=keys {
                assert!(
                    has_target(ControlRef::Key(key), "key"),
                    "{} key {key}",
                    model.id
                );
            }
            for dial in 1..=dials {
                assert!(
                    has_target(ControlRef::Dial(dial), "dial"),
                    "{} dial {dial}",
                    model.id
                );
                assert!(
                    has_target(ControlRef::Dial(dial), "key"),
                    "{} dial {dial} takes catalog actions",
                    model.id
                );
            }
        }
    });
}

#[test]
fn a_preset_dropped_on_xl_dial_6_is_saved_and_never_executed() {
    run_gtk_test(|| {
        let Some(f) = fixture("xl-preset-dial", &PLUS_XL) else {
            return;
        };
        while requests(&f).try_recv().is_ok() {}
        assert!(drop_on(
            &f,
            "Drag Volume onto a dial, or click to assign to the selected dial.",
            ControlRef::Dial(6)
        ));
        {
            let store = f.layers.borrow();
            assert_eq!(
                store.binding("Base", &Trigger::EncoderClockwise(6)),
                Some(media(MediaCommand::VolumeUp))
            );
            assert_eq!(
                store.binding("Base", &Trigger::EncoderPress(6)),
                Some(media(MediaCommand::Mute))
            );
            for dial in 1..=5 {
                assert!(store
                    .binding("Base", &Trigger::EncoderClockwise(dial))
                    .is_none());
            }
        }
        assert_eq!(
            f.widgets.binding_ui.selected_control(),
            Some(ControlRef::Dial(6))
        );
        assert!(f.widgets.binding_ui.dial_legends[5].is_visible());
        capture(&f, "xl-dial-6-preset");
        let commands: Vec<_> = requests(&f).try_iter().collect();
        assert!(commands
            .iter()
            .any(|command| matches!(command, LayerCommand::SaveBinding(..))));
        assert!(!commands
            .iter()
            .any(|command| matches!(command, LayerCommand::Execute { .. })));
        // Clicking a preset with dial 5 selected assigns it to dial 5.
        select(&f, ControlRef::Dial(5));
        let mut buttons = Vec::new();
        collect_catalog_buttons(f.widgets.root.upcast_ref(), &mut buttons);
        buttons
            .iter()
            .find(|button| {
                button.tooltip_text().as_deref()
                    == Some("Drag Tracks onto a dial, or click to assign to the selected dial.")
            })
            .unwrap()
            .emit_clicked();
        assert_eq!(
            f.layers
                .borrow()
                .binding("Base", &Trigger::EncoderClockwise(5)),
            Some(media(MediaCommand::Next))
        );
    });
}

/// The size of the picture the preview shows for `target`.
fn picture_size(f: &ModelFixture, target: ArtworkTarget) -> Option<(i32, i32)> {
    f.artwork
        .pictures
        .borrow()
        .iter()
        .find(|(shown, _)| *shown == target)
        .and_then(|(_, picture)| picture.paintable())
        .map(|paintable| (paintable.intrinsic_width(), paintable.intrinsic_height()))
}

#[test]
fn key_pictures_are_composed_at_the_key_size_of_the_model() {
    run_gtk_test(|| {
        for (model, key, side) in [(&PLUS, 3u8, 120), (&PLUS_XL, 30, 112)] {
            let Some(f) = model_fixture("key-px", model, move |store, images| {
                // A binding icon on key `key`, button text on the next key.
                store
                    .bind(
                        "Base",
                        Trigger::KeyPress(key),
                        Action::Command("echo icon".into()),
                    )
                    .unwrap();
                images
                    .set_button_text_in_layer(
                        "Base",
                        ArtworkTarget::Keypad(key - 1),
                        sdeck_plus::button_text::ButtonText {
                            text: "Hi".into(),
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }) else {
                return;
            };
            assert_eq!(
                picture_size(&f, ArtworkTarget::Keypad(key)),
                Some((side, side)),
                "{}: the binding icon picture",
                model.id
            );
            assert_eq!(
                picture_size(&f, ArtworkTarget::Keypad(key - 1)),
                Some((side, side)),
                "{}: the button text picture",
                model.id
            );
            // A live-information key too.
            f.layers
                .borrow_mut()
                .set_info(
                    "Base",
                    &Trigger::KeyPress(key),
                    Some(sdeck_plus::info::InfoSource::Clock),
                )
                .unwrap();
            f.artwork.refresh(true);
            let value = sdeck_plus::info::InfoSource::Clock.placeholder();
            f.artwork
                .paint_info_preview(ArtworkTarget::Keypad(key), &value);
            assert_eq!(
                picture_size(&f, ArtworkTarget::Keypad(key)),
                Some((side, side)),
                "{}: the live information picture",
                model.id
            );
        }
    });
}

fn right_click(widget: &gtk::Widget) {
    let controllers = widget.observe_controllers();
    let gesture = (0..controllers.n_items())
        .filter_map(|index| {
            controllers
                .item(index)?
                .downcast::<gtk::GestureClick>()
                .ok()
        })
        .find(|gesture| gesture.button() == 3)
        .expect("a right-click gesture on the key");
    gesture.emit_by_name::<()>("pressed", &[&1i32, &4.0f64, &4.0f64]);
}

fn popovers(widget: &gtk::Widget, found: &mut Vec<gtk::Popover>) {
    if let Some(popover) = widget.downcast_ref::<gtk::Popover>() {
        found.push(popover.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        popovers(&current, found);
        child = current.next_sibling();
    }
}

/// Wait for the real map signal, never a fixed sleep.
fn wait_until_mapped(popover: &gtk::Popover) {
    let panel = popover.clone();
    super::model_session_tests::wait_until("a popover to map", move || panel.is_mapped());
}

fn context_menu_entry(key: &gtk::Widget) -> Button {
    let mut found = Vec::new();
    popovers(key, &mut found);
    let menu = found.into_iter().next().expect("the key's context menu");
    wait_until_mapped(&menu);
    menu.child().unwrap().downcast::<Button>().unwrap()
}

#[test]
fn right_clicking_xl_keys_9_and_36_opens_the_text_editor_at_the_key_size() {
    run_gtk_test(|| {
        let Some(f) = fixture("xl-right-click", &PLUS_XL) else {
            return;
        };
        let editor = f.widgets._text_editor.clone();
        // Keep shared-desktop focus changes from dismissing the popup under test.
        editor.panel.set_autohide(false);
        for number in [9u8, 36] {
            let key = f.widgets.preview.control(ControlRef::Key(number)).unwrap();
            right_click(&key);
            assert_eq!(
                f.widgets.binding_ui.selected_control(),
                Some(ControlRef::Key(number)),
                "the right click selects the key"
            );
            context_menu_entry(&key).emit_clicked();
            wait_until_mapped(&editor.panel);
            assert_eq!(editor.panel.parent().as_ref(), Some(&key));
            assert_eq!(
                editor.context_line.text(),
                format!("Key {number} on “Base / Page 1”")
            );
            editor.input.buffer().set_text(&format!("K{number}"));
            assert_eq!(
                (
                    editor.preview.width_request(),
                    editor.preview.height_request()
                ),
                (112, 112),
                "the text preview is the XL+ key"
            );
            let paintable = editor.preview.paintable().unwrap();
            assert_eq!(
                (paintable.intrinsic_width(), paintable.intrinsic_height()),
                (112, 112)
            );
            editor.save.emit_clicked();
            let saved = f
                .artwork
                .store
                .borrow()
                .as_ref()
                .unwrap()
                .button_text_in_layer("Base", ArtworkTarget::Keypad(number))
                .cloned()
                .expect("the text is saved on the key");
            assert_eq!(saved.text, format!("K{number}"));
            assert_eq!(
                picture_size(&f, ArtworkTarget::Keypad(number)),
                Some((112, 112))
            );
            editor.panel.popdown();
        }
    });
}

#[test]
fn the_plus_text_editor_keeps_its_120_px_preview() {
    run_gtk_test(|| {
        let Some(f) = fixture("plus-right-click", &PLUS) else {
            return;
        };
        let editor = f.widgets._text_editor.clone();
        editor.panel.set_autohide(false);
        let key = f.widgets.preview.control(ControlRef::Key(8)).unwrap();
        right_click(&key);
        context_menu_entry(&key).emit_clicked();
        wait_until_mapped(&editor.panel);
        editor.input.buffer().set_text("K8");
        assert_eq!(editor.preview.width_request(), 120);
        assert_eq!(editor.preview.paintable().unwrap().intrinsic_width(), 120);
        editor.panel.popdown();
    });
}

#[test]
fn switching_model_pops_down_a_key_menu_and_leaves_no_popover_on_a_removed_key() {
    run_gtk_test(|| {
        let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = warnings.clone();
        let handler = glib::log_set_handler(
            Some("Gtk"),
            glib::LogLevels::LEVEL_WARNING | glib::LogLevels::LEVEL_CRITICAL,
            false,
            false,
            move |_, _, message| sink.lock().unwrap().push(message.to_owned()),
        );
        let Some(f) = fixture("popover-teardown", &PLUS_XL) else {
            glib::log_remove_handler(Some("Gtk"), handler);
            return;
        };
        let key = f.widgets.preview.control(ControlRef::Key(3)).unwrap();
        right_click(&key);
        let mut found = Vec::new();
        popovers(&key, &mut found);
        let menu = found.pop().expect("the key's context menu");
        wait_until_mapped(&menu);
        drop(key);
        f.widgets.preview.show_model(&PLUS);
        assert_eq!(menu.parent(), None, "no popover stays on a removed key");
        assert!(!menu.is_visible());
        // Let GTK finalize what the rebuild dropped.
        let context = glib::MainContext::default();
        while context.iteration(false) {}
        glib::log_remove_handler(Some("Gtk"), handler);
        let seen = warnings.lock().unwrap().clone();
        assert!(
            !seen
                .iter()
                .any(|message| message.contains("still has children")),
            "{seen:?}"
        );
    });
}

#[test]
fn the_artwork_target_mini_map_draws_the_shown_models_sections() {
    run_gtk_test(|| {
        // Strip selected: every section is filled. Between sections 1 and 2 the
        // 4-section map has none of its own gaps, the 6-section map does.
        let mut seen = Vec::new();
        for model in [&PLUS, &PLUS_XL] {
            let Some(f) = fixture("mini-map", model) else {
                return;
            };
            select(&f, ControlRef::Strip);
            assert_eq!(
                f.widgets.target_picker.selected(),
                0,
                "the strip is the picker's first target"
            );
            f.widgets.artwork_section.set_expanded(true);
            wait_for_frame(&f.window);
            let marker = super::preview_model_tests::descendants_of::<gtk::DrawingArea>(
                f.widgets.target_picker.upcast_ref(),
            )
            .into_iter()
            .find(|area| area.has_css_class("target-marker"))
            .expect("the picker's marker");
            assert_eq!(marker.widget_name(), "target-strip");
            let pixels = rendered_pixels(&f.window, &marker);
            let alpha = |x: usize, y: usize| pixels[(y * marker.width() as usize + x) * 4 + 3];
            seen.push((alpha(4, 19), alpha(3, 19)));
        }
        assert!(seen[0].0 > 0xc0, "the + section 1 spans x = 4: {seen:?}");
        assert!(seen[1].0 < 0xc0, "the XL+ has a gap at x = 4: {seen:?}");
        assert!(seen[1].1 > 0x80, "the XL+ section 1 is drawn: {seen:?}");
    });
}

fn catalog_search(f: &ModelFixture) -> gtk::SearchEntry {
    super::preview_model_tests::descendants_of::<gtk::SearchEntry>(f.widgets.root.upcast_ref())
        .into_iter()
        .next()
        .expect("the catalog search")
}

#[test]
fn the_catalog_search_and_open_categories_survive_a_model_switch() {
    run_gtk_test(|| {
        let Some(f) = fixture("catalog-memory", &PLUS_XL) else {
            return;
        };
        let mut buttons = Vec::new();
        collect_catalog_buttons(f.widgets.root.upcast_ref(), &mut buttons);
        let system = buttons
            .iter()
            .find(|button| button.tooltip_text().as_deref() == Some("Show or hide System actions"))
            .expect("the System category");
        system.emit_clicked();
        catalog_search(&f).set_text("mute");
        f.widgets.preview.show_model(&PLUS);
        assert_eq!(catalog_search(&f).text(), "mute");
        let visible_rows = |f: &ModelFixture| {
            let mut buttons = Vec::new();
            collect_catalog_buttons(f.widgets.root.upcast_ref(), &mut buttons);
            buttons
                .iter()
                .filter(|button| {
                    button.is_visible()
                        && button.tooltip_text().is_some_and(|tip| {
                            tip.starts_with("Drag ") && tip.contains("onto a key")
                        })
                })
                .count()
        };
        assert!(
            visible_rows(&f) >= 1,
            "the search still filters the new catalog"
        );
        catalog_search(&f).set_text("");
        f.widgets.preview.show_model(&PLUS_XL);
        let mut buttons = Vec::new();
        collect_catalog_buttons(f.widgets.root.upcast_ref(), &mut buttons);
        let header = buttons
            .iter()
            .find(|button| button.tooltip_text().as_deref() == Some("Show or hide System actions"))
            .unwrap();
        let chevron = super::preview_model_tests::descendants_of::<gtk::Image>(header.upcast_ref())
            .pop()
            .unwrap();
        assert_eq!(
            chevron.icon_name().as_deref(),
            Some("pan-down-symbolic"),
            "System stays open"
        );
    });
}
