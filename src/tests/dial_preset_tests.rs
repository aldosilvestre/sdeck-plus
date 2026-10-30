use super::action_catalog_tests::{catalog_fixture, collect_catalog_buttons};
use super::*;
use sdeck_plus::{
    desktop_actions::{DesktopAction, MediaCommand},
    device_control::DeviceAction,
    features::FeatureAction,
};

fn media(command: MediaCommand) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Media(command)))
}

/// Drag a Dials-tab preset onto one of the four dial previews.
fn preset_drop(widgets: &MonitorWidgets, caption: &str, dial: usize) -> bool {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    let tooltip = format!("Drag {caption} onto a dial, or click to assign to the selected dial.");
    let row = buttons
        .iter()
        .find(|button| button.tooltip_text().as_deref() == Some(tooltip.as_str()))
        .expect("dial preset row");
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
        .expect("dial preset target");
    let value = provider.value(kind).unwrap();
    target.emit_by_name::<bool>("drop", &[&glib::BoxedValue(value), &0.0f64, &0.0f64])
}

#[test]
fn dropping_volume_preset_on_a_dial_assigns_turn_and_press_without_executing() {
    run_gtk_test(|| {
        catalog_fixture("dial-volume", |widgets, layers, requests| {
            while requests.try_recv().is_ok() {}
            assert!(preset_drop(widgets, "Volume", 1));
            let store = layers.borrow();
            assert_eq!(
                store.binding("Base", &Trigger::EncoderClockwise(2)),
                Some(media(MediaCommand::VolumeUp))
            );
            assert_eq!(
                store.binding("Base", &Trigger::EncoderCounterClockwise(2)),
                Some(media(MediaCommand::VolumeDown))
            );
            assert_eq!(
                store.binding("Base", &Trigger::EncoderPress(2)),
                Some(media(MediaCommand::Mute))
            );
            assert_eq!(widgets.binding_ui.control_kind(), ControlKind::Encoder);
            assert_eq!(widgets.binding_ui.control_number(), Some(2));
            let mut saved = false;
            while let Ok(command) = requests.try_recv() {
                match command {
                    LayerCommand::SaveBinding(..) => saved = true,
                    LayerCommand::Execute { .. } => panic!("assigning must not execute"),
                    _ => {}
                }
            }
            assert!(saved, "drop must enqueue persistence");
        })
    });
}

#[test]
fn dropping_a_preset_keeps_dial_events_it_does_not_define() {
    run_gtk_test(|| {
        catalog_fixture("dial-keep", |widgets, layers, _requests| {
            layers
                .borrow_mut()
                .bind(
                    "Base",
                    Trigger::EncoderPress(1),
                    Action::Command("keep".into()),
                )
                .unwrap();
            assert!(preset_drop(widgets, "Brightness", 0));
            let store = layers.borrow();
            assert_eq!(
                store.binding("Base", &Trigger::EncoderClockwise(1)),
                Some(Action::Feature(FeatureAction::Device(
                    DeviceAction::AdjustBrightness(10)
                )))
            );
            assert_eq!(
                store.binding("Base", &Trigger::EncoderPress(1)),
                Some(Action::Command("keep".into()))
            );
        })
    });
}

#[test]
fn dial_presets_ask_before_discarding_a_draft_elsewhere() {
    run_gtk_test(|| {
        catalog_fixture("dial-dirty", |widgets, layers, _requests| {
            widgets.binding_ui.action_picker.set_selected(1);
            assert!(widgets.binding_ui.is_dirty());
            assert!(preset_drop(widgets, "Volume", 2), "drop is accepted");
            // Nothing is assigned until the prompt is answered.
            discard_dialog().response(ResponseType::Cancel);
            assert_eq!(
                layers
                    .borrow()
                    .binding("Base", &Trigger::EncoderClockwise(3)),
                None
            );
            assert!(preset_drop(widgets, "Volume", 2));
            discard_dialog().response(ResponseType::Accept);
            assert_eq!(
                layers
                    .borrow()
                    .binding("Base", &Trigger::EncoderClockwise(3)),
                Some(media(MediaCommand::VolumeUp))
            );
        })
    });
}

fn all_widgets(widget: &gtk::Widget, found: &mut Vec<gtk::Widget>) {
    found.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(current) = child {
        all_widgets(&current, found);
        child = current.next_sibling();
    }
}

#[test]
fn sidebar_offers_actions_dials_and_binding_tabs_with_live_information_in_binding() {
    run_gtk_test(|| {
        catalog_fixture("sidebar-tabs", |widgets, _layers, _requests| {
            let mut found = Vec::new();
            all_widgets(widgets.root.upcast_ref(), &mut found);
            let stack = found
                .iter()
                .filter_map(|widget| widget.downcast_ref::<gtk::Stack>())
                .find(|stack| {
                    stack.page(&stack.first_child().unwrap()).title().as_deref() == Some("Actions")
                })
                .expect("sidebar stack");
            let pages = stack.pages();
            let titles: Vec<String> = (0..pages.n_items())
                .map(|index| {
                    pages
                        .item(index)
                        .and_downcast::<gtk::StackPage>()
                        .unwrap()
                        .title()
                        .unwrap_or_default()
                        .to_string()
                })
                .collect();
            assert_eq!(titles, ["Actions", "Dials", "Binding"]);
            let binding = stack.child_by_name("binding").expect("binding page");
            let toggle = &widgets.binding_ui.info_toggle;
            assert!(toggle.is_ancestor(&binding));
            assert!(!toggle.is_ancestor(&sidebar_settings(widgets)));
            if let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") {
                let window = widgets
                    .root
                    .root()
                    .and_downcast::<ApplicationWindow>()
                    .unwrap();
                for name in ["dials", "binding"] {
                    stack.set_visible_child_name(name);
                    wait_for_frame(&window);
                    rendered_texture(&window, &widgets.root)
                        .save_to_png(
                            std::path::Path::new(&directory).join(format!("tab-{name}.png")),
                        )
                        .unwrap();
                }
            }
        })
    });
}
