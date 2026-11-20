//! The Stream Deck Neo's two touch buttons in the window: the preview draws
//! them as named, selectable controls beside the information window, the
//! editor configures and saves them like a key press, and a touch runs the
//! saved action once per touch. Models without touch buttons show none. No
//! device is involved: the window runs over scratch stores and a channel
//! that stands in for the layer worker.
use super::action_catalog_tests::collect_catalog_buttons;
use super::preview_model_tests::{class_of, descendants_of, model_fixture, ModelFixture};
use super::*;
use sdeck_plus::{
    binding_editor::ControlKind,
    device_model::{DeviceModel, CLASSIC, PLUS, PLUS_XL, XL},
    preview_layout::{ControlRef, PreviewLayout},
};

fn neo() -> &'static DeviceModel {
    DeviceModel::by_id("neo").unwrap()
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

fn control(f: &ModelFixture, control: ControlRef) -> Button {
    f.widgets
        .preview
        .control(control)
        .unwrap_or_else(|| panic!("{control:?} is on the shown preview"))
        .downcast::<Button>()
        .unwrap()
}

fn executed(f: &ModelFixture) -> Vec<(String, Action)> {
    f.requests
        .as_ref()
        .unwrap()
        .try_iter()
        .filter_map(|command| match command {
            LayerCommand::Execute { binding_id, action } => Some((binding_id, action)),
            _ => None,
        })
        .collect()
}

fn heading(f: &ModelFixture) -> String {
    class_of(f.widgets.root.upcast_ref(), "control-heading")
        .downcast::<Label>()
        .unwrap()
        .text()
        .to_string()
}

fn touch(f: &ModelFixture, index: usize, pressed: bool) {
    f.widgets
        .feature_runtime
        .execute_input(&protocol::Event::Key { index, pressed });
}

#[test]
fn the_neo_preview_draws_two_named_touch_buttons_beside_its_window() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("tb-preview", neo(), |store, _| {
            store
                .bind(
                    "Base",
                    Trigger::TouchButton(2),
                    Action::Command("echo right".into()),
                )
                .unwrap();
        }) else {
            return;
        };
        let layout = PreviewLayout::for_model(neo());
        let chassis = class_of(f.widgets.root.upcast_ref(), "hardware-chassis");
        for number in 1..=2u8 {
            let button = control(&f, ControlRef::TouchButton(number));
            assert!(button.has_css_class("touch-button"), "{number}");
            assert!(button.has_css_class("control-selectable"), "{number}");
            assert!(
                button.is_focusable(),
                "{number}: reachable with the keyboard"
            );
            // Placed at its layout rectangle on the chassis, at its size.
            let rect = layout.rect_of(ControlRef::TouchButton(number)).unwrap();
            let bounds = button.compute_bounds(&chassis).expect("on the chassis");
            assert_eq!(
                (
                    bounds.x() as i32,
                    bounds.y() as i32,
                    bounds.width() as i32,
                    bounds.height() as i32
                ),
                (rect.x, rect.y, rect.w, rect.h),
                "touch button {number}"
            );
        }
        assert_eq!(
            control(&f, ControlRef::TouchButton(2))
                .tooltip_text()
                .unwrap(),
            "Touch button 2 · Press: Run echo right"
        );
        assert_eq!(
            control(&f, ControlRef::TouchButton(1))
                .tooltip_text()
                .unwrap(),
            "Touch button 1 · Press: Unassigned"
        );
        assert!(f
            .widgets
            .preview
            .control(ControlRef::TouchButton(3))
            .is_none());
        // The window is no touch strip: no strip control is offered.
        assert!(f.widgets.preview.control(ControlRef::Strip).is_none());
        drop(f);

        for model in [&PLUS, &PLUS_XL, &CLASSIC, &XL] {
            let Some(f) = model_fixture("tb-none", model, |_, _| {}) else {
                return;
            };
            assert!(
                f.widgets
                    .preview
                    .control(ControlRef::TouchButton(1))
                    .is_none(),
                "{}",
                model.id
            );
            assert!(
                descendants_of::<gtk::Widget>(f.widgets.root.upcast_ref())
                    .iter()
                    .all(|widget| !widget.has_css_class("touch-button")),
                "{}",
                model.id
            );
        }
    });
}

#[test]
fn a_touch_button_is_selected_edited_saved_and_run_once_per_touch() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("tb-edit", neo(), |_, _| {}) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        control(&f, ControlRef::TouchButton(1)).emit_clicked();
        assert_eq!(ui.control_kind(), ControlKind::TouchButton);
        assert_eq!(ui.selected_control(), Some(ControlRef::TouchButton(1)));
        assert_eq!(heading(&f), "Configuring: Touch button 1");
        assert!(control(&f, ControlRef::TouchButton(1)).has_css_class("control-selected"));
        assert_eq!(
            ui.selected_layer_and_trigger(&f.layers).unwrap(),
            ("Base".to_owned(), Trigger::TouchButton(1))
        );
        // A touch button has one event, no hold and no live information.
        assert!(!ui.hold_toggle.is_visible());
        assert!(!ui.info_toggle.is_visible());
        assert_eq!(
            (0..ui.event_model.n_items())
                .filter_map(|index| ui.event_model.string(index))
                .map(|text| text.to_string())
                .collect::<Vec<_>>(),
            ["Press"]
        );
        // The Event and Action selectors are shown: the catalog does not serve it.
        assert!(ui.action_picker.is_mapped());

        ui.action_picker.set_selected(1);
        ui.text_entry.set_text("echo touched");
        while f.requests.as_ref().unwrap().try_recv().is_ok() {}
        click(&f, "Save binding");
        assert!(executed(&f).is_empty(), "saving never runs the action");
        let action = Action::Command("echo touched".into());
        assert_eq!(
            f.layers.borrow().binding("Base", &Trigger::TouchButton(1)),
            Some(action.clone())
        );
        assert_eq!(
            control(&f, ControlRef::TouchButton(1))
                .tooltip_text()
                .unwrap(),
            "Touch button 1 · Press: Run echo touched"
        );
        // The store the layer worker would write, read back from disk.
        let snapshot = f.layers.borrow().clone();
        snapshot.persist().unwrap();
        let text = std::fs::read_to_string(snapshot.root().join("layers.conf")).unwrap();
        assert!(text.contains("\ttb:1\t"), "{text}");

        // One touch runs it once; the repeated held state and the release do not.
        let identity = binding_identity("Base", &Trigger::TouchButton(1));
        for pressed in [true, true, false, false] {
            touch(&f, 8, pressed);
        }
        assert_eq!(executed(&f), [(identity.clone(), action.clone())]);
        touch(&f, 8, true);
        assert_eq!(executed(&f), [(identity, action.clone())]);
        touch(&f, 8, false);
        // The other touch button and the keys run nothing of it.
        touch(&f, 9, true);
        touch(&f, 0, true);
        touch(&f, 0, false);
        assert!(executed(&f).is_empty());

        // Remove clears it and Undo puts it back.
        click(&f, "Remove binding");
        assert_eq!(
            f.layers.borrow().binding("Base", &Trigger::TouchButton(1)),
            None
        );
        f.widgets.toast.action.emit_clicked();
        assert_eq!(
            f.layers.borrow().binding("Base", &Trigger::TouchButton(1)),
            Some(action)
        );
    });
}

#[test]
fn the_editor_lists_the_touch_buttons_of_the_model_and_refuses_others() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("tb-picker", neo(), |_, _| {}) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        ui.select_control(ControlKind::TouchButton, Some(2), true)
            .unwrap();
        assert_eq!(
            (0..ui.control_number_model.n_items())
                .filter_map(|index| ui.control_number_model.string(index))
                .map(|text| text.to_string())
                .collect::<Vec<_>>(),
            ["1", "2"]
        );
        assert_eq!(ui.control_number(), Some(2));
        assert_eq!(
            ui.select_control(ControlKind::TouchButton, Some(3), true),
            Err("touch button number must be 1 through 2".to_owned())
        );
        assert_eq!(
            ui.select_control(ControlKind::TouchButton, Some(0), true),
            Err("touch button number must be 1 through 2".to_owned())
        );
        drop(f);
        let Some(f) = model_fixture("tb-picker-plus", &PLUS, |store, _| {
            store
                .bind(
                    "Base",
                    Trigger::KeyPress(8),
                    Action::Command("echo 8".into()),
                )
                .unwrap();
        }) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        assert_eq!(
            ui.select_control(ControlKind::TouchButton, Some(1), true),
            Err("the Stream Deck + has no touch buttons".to_owned())
        );
        assert_eq!(ui.control_kind(), ControlKind::Button);
        // The Stream Deck + reads index 7 as key 8 and ignores index 8, as before.
        while f.requests.as_ref().unwrap().try_recv().is_ok() {}
        touch(&f, 8, true);
        touch(&f, 8, false);
        assert!(executed(&f).is_empty());
        touch(&f, 7, true);
        touch(&f, 7, false);
        assert_eq!(
            executed(&f),
            [(
                binding_identity("Base", &Trigger::KeyPress(8)),
                Action::Command("echo 8".into())
            )]
        );
    });
}
