//! The window of every model the app drives shows only the sections of the
//! controls that model has. A Stream Deck Classic or XL has keys only and the
//! Neo keys, two touch buttons and an information window: their action catalog
//! serves the keys, and there is no Dials tab, no touchscreen page control, no
//! strip appearance (the Neo's window has its own, see neo_window_ui_tests) and
//! no text that offers a dial or a touch strip. The Stream Deck + and + XL keep
//! every section exactly as before. No device is involved: the window runs over
//! scratch stores and a channel that stands in for the layer worker.
use super::action_catalog_tests::collect_catalog_buttons;
use super::preview_model_tests::{
    bounds_in, chassis_of, class_of, controls_of, descendants_of, kind_of, model_fixture,
    tab_order, ModelFixture,
};
use super::*;
use sdeck_plus::{
    binding_editor::ControlKind,
    desktop_actions::{DesktopAction, MediaCommand},
    device_model::{DeviceModel, CLASSIC, PLUS, PLUS_XL, XL},
    features::FeatureAction,
    preview_layout::{ControlRef, PreviewLayout},
};

fn neo() -> &'static DeviceModel {
    DeviceModel::by_id("neo").unwrap()
}

/// Every model the window is built for: the supported ones, and the Neo,
/// whose touch buttons and window the preview already draws.
fn window_models() -> Vec<&'static DeviceModel> {
    DeviceModel::all()
        .iter()
        .copied()
        .filter(|model| model.is_supported() || model.id == "neo")
        .collect()
}

/// The profile's touch pages, as every session opens them (models without a
/// touch strip get them too: they hold nothing).
fn with_touch_pages(store: &mut LayerStore, images: &mut sdeck_plus::local_artwork::ArtworkStore) {
    let organization = sdeck_plus::organization::Organization::in_memory(store);
    store
        .initialize_touchscreens(&organization, images)
        .unwrap();
    store.create_touchscreen_page("Base").unwrap();
}

fn media(command: MediaCommand) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Media(command)))
}

/// The catalog row tooltip of `caption` on `model`: only a model with dials
/// offers them as a drop target.
fn row_tooltip(model: &DeviceModel, caption: &str) -> String {
    let targets = if model.dials > 0 {
        "a key or dial"
    } else {
        "a key"
    };
    format!("Drag {caption} onto {targets}, or click to assign to the selected control.")
}

/// The captions of the side panel's tabs the user can see, in order.
fn shown_tabs(f: &ModelFixture) -> Vec<String> {
    let switcher = class_of(f.widgets.root.upcast_ref(), "sidebar-tabs");
    let mut tabs = Vec::new();
    let mut child = switcher.first_child();
    while let Some(button) = child {
        if button.is_visible() {
            if let Some(label) = descendant_label(&button) {
                tabs.push(label.text().to_string());
            }
        }
        child = button.next_sibling();
    }
    tabs
}

fn sidebar_stack(f: &ModelFixture) -> gtk::Stack {
    descendants_of::<gtk::Stack>(f.widgets.root.upcast_ref())
        .into_iter()
        .find(|stack| {
            stack.child_by_name("actions").is_some() && stack.child_by_name("binding").is_some()
        })
        .expect("the side panel's tabs")
}

/// Whether `widget` takes catalog (COPY) drops.
fn takes_catalog_drops(widget: &gtk::Widget) -> bool {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| controllers.item(index)?.downcast::<gtk::DropTarget>().ok())
        .any(|target| target.actions() == gtk::gdk::DragAction::COPY)
}

fn catalog_row(f: &ModelFixture, tooltip: &str) -> Button {
    let mut buttons = Vec::new();
    collect_catalog_buttons(f.widgets.root.upcast_ref(), &mut buttons);
    buttons
        .into_iter()
        .find(|button| button.tooltip_text().as_deref() == Some(tooltip))
        .unwrap_or_else(|| panic!("no catalog row {tooltip:?}"))
}

/// Drop the catalog row with `tooltip` onto `target`, as GTK ends a drag.
fn drop_row(f: &ModelFixture, tooltip: &str, target: &gtk::Widget) -> bool {
    let row = catalog_row(f, tooltip);
    let sources = row.observe_controllers();
    let source = (0..sources.n_items())
        .find_map(|index| sources.item(index)?.downcast::<gtk::DragSource>().ok())
        .expect("the row is draggable");
    let provider = source
        .emit_by_name::<Option<gtk::gdk::ContentProvider>>("prepare", &[&0.0f64, &0.0f64])
        .unwrap();
    let targets = target.observe_controllers();
    let drop = (0..targets.n_items())
        .filter_map(|index| targets.item(index)?.downcast::<gtk::DropTarget>().ok())
        .find(|target| target.actions() == gtk::gdk::DragAction::COPY)
        .expect("a catalog drop target");
    let value = provider.value(provider.formats().types()[0]).unwrap();
    drop.emit_by_name::<bool>("drop", &[&glib::BoxedValue(value), &0.0f64, &0.0f64])
}

fn select(f: &ModelFixture, control: ControlRef) {
    f.widgets
        .preview
        .control(control)
        .unwrap_or_else(|| panic!("{control:?} is on the shown preview"))
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
}

/// What reached the layer worker since the last call: (saved, executed).
fn worker_requests(f: &ModelFixture) -> (bool, bool) {
    let mut seen = (false, false);
    for command in f.requests.as_ref().unwrap().try_iter() {
        match command {
            LayerCommand::SaveBinding(..) => seen.0 = true,
            LayerCommand::Execute { .. } | LayerCommand::ExecuteStep { .. } => seen.1 = true,
            _ => {}
        }
    }
    seen
}

/// The texts the setup help shows once opened, in order (the dialog itself is
/// hidden until the Menu opens it).
pub(super) fn help_texts(dialog: &gtk::Dialog) -> Vec<String> {
    fn walk(widget: &gtk::Widget, found: &mut Vec<String>) {
        // Its own flag: `is_visible` also asks the hidden dialog.
        if !widget.get_visible() {
            return;
        }
        if let Some(label) = widget.downcast_ref::<Label>() {
            found.push(label.text().to_string());
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            walk(&next, found);
            child = next.next_sibling();
        }
    }
    let mut found = Vec::new();
    let mut child = dialog.first_child();
    while let Some(next) = child {
        walk(&next, &mut found);
        child = next.next_sibling();
    }
    found
}

const PLUS_SELECT_HELP: &str =
    "Click a key, dial or the touch strip on the preview. Dials have separate press and rotation events.";
const PLUS_CONFIGURE_HELP: &str = "Drag an action from the right onto a key or a dial (a dial asks for clockwise, counterclockwise or press). Actions without parameters save on drop; otherwise complete the settings below and Save binding. You can also click a catalog action for the selected control, or use the Action selector for any control.";
const PLUS_INFO_TOOLTIP: &str = "Replaces the image with a live picture, such as the volume or the time. Dials, and a tap or hold on a touch section, draw it on that section.";

#[test]
fn each_model_shows_the_sections_of_the_controls_it_has() {
    run_gtk_test(|| {
        let models = window_models();
        for wanted in [&PLUS, &PLUS_XL, &CLASSIC, &XL, neo()] {
            assert!(models.contains(&wanted), "{} is covered", wanted.id);
        }
        for model in models {
            let Some(f) = model_fixture("sections", model, with_touch_pages) else {
                return;
            };
            let id = model.id;
            let root: gtk::Widget = f.widgets.root.clone().upcast();
            // The preview holds exactly the model's controls, and Tab visits them in order.
            let chassis = chassis_of(&root);
            let buttons = controls_of(&chassis);
            let layout = PreviewLayout::for_model(model);
            assert_eq!(buttons.len(), layout.controls().len(), "{id}");
            assert_eq!((chassis.width(), chassis.height()), layout.size, "{id}");
            for (button, control) in buttons.iter().zip(layout.controls()) {
                let rect = layout.rect_of(control).unwrap();
                assert_eq!(
                    bounds_in(button, &chassis),
                    (rect.x, rect.y, rect.w, rect.h),
                    "{id}: {control:?} sits at its layout rectangle"
                );
            }
            let count = |kind: &str| buttons.iter().filter(|b| kind_of(b) == kind).count();
            assert_eq!(count("key"), usize::from(model.keys()), "{id}");
            assert_eq!(count("strip"), usize::from(model.has_touch()), "{id}");
            assert_eq!(count("zone"), usize::from(model.zones()), "{id}");
            assert_eq!(count("dial"), usize::from(model.dials), "{id}");
            assert_eq!(
                count("touch button"),
                usize::from(model.touch_buttons),
                "{id}"
            );
            assert_eq!(
                count("window"),
                usize::from(model.has_info_window()),
                "{id}"
            );
            assert_eq!(count("other"), 0, "{id}");
            assert_eq!(
                tab_order(&f.window, &chassis, &buttons),
                buttons,
                "{id}: Tab visits every control in order"
            );
            // The side panel offers the Dials tab only to a model with dials.
            let expected: &[&str] = if model.dials > 0 {
                &["Actions", "Dials", "Binding"]
            } else {
                &["Actions", "Binding"]
            };
            assert_eq!(shown_tabs(&f), expected, "{id}");
            let stack = sidebar_stack(&f);
            assert_eq!(
                stack.visible_child_name().as_deref(),
                Some("actions"),
                "{id}"
            );
            // The Actions tab holds the catalog, and every key and dial takes its drops.
            let actions = stack.child_by_name("actions").unwrap();
            assert!(
                descendants_of::<gtk::SearchEntry>(&actions)
                    .iter()
                    .any(|search| search.placeholder_text().as_deref() == Some("Search actions")),
                "{id}: the Actions tab holds the catalog"
            );
            catalog_row(&f, &row_tooltip(model, "Play / pause"));
            for widget in f
                .widgets
                .preview
                .key_widgets()
                .iter()
                .chain(&f.widgets.preview.dial_widgets())
            {
                assert!(takes_catalog_drops(widget), "{id}");
            }
            // Only the model's displays hold pictures, and the Appearance picker
            // lists only its strip and sections (none on a Classic or an XL).
            assert_eq!(
                f.artwork.pictures.borrow().len(),
                usize::from(model.keys())
                    + usize::from(model.has_strip())
                    + usize::from(model.zones()),
                "{id}"
            );
            assert_eq!(
                f.widgets.target_picker.model().unwrap().n_items() as usize,
                ArtworkTarget::touch_for(model).len(),
                "{id}"
            );
            // Gesture badges only describe a touch strip: none over the Neo's window.
            let mut badges = 0;
            let mut badge = f.widgets.binding_ui.gesture_badges.first_child();
            while let Some(next) = badge {
                badges += 1;
                badge = next.next_sibling();
            }
            assert_eq!(badges > 0, model.has_touch(), "{id}: {badges} badges");
            // Touch pages and the strip's appearance never show for a key.
            assert!(
                !f.widgets
                    .feature_runtime
                    .touchscreen_controls()
                    .is_visible(),
                "{id}"
            );
            assert!(!class_of(&root, "appearance-section").is_visible(), "{id}");
            // The texts name only what the model has.
            let new_profile = class_of(&root, "rail-new-profile").tooltip_text().unwrap();
            let info = f.widgets.binding_ui.info_toggle.tooltip_text().unwrap();
            let help = help_texts(&f.widgets._configuration_menu.help_for_test());
            if model.dials > 0 && model.has_touch() {
                assert_eq!(
                    new_profile,
                    "New profile: add a profile with its own keys, dials, pages and touch strip.",
                    "{id}"
                );
                assert_eq!(info, PLUS_INFO_TOOLTIP, "{id}");
                assert!(
                    help.iter().any(|text| text == PLUS_SELECT_HELP),
                    "{id}: {help:?}"
                );
                assert!(
                    help.iter().any(|text| text == PLUS_CONFIGURE_HELP),
                    "{id}: {help:?}"
                );
                assert!(
                    help.iter().any(|text| text == "Touch strip pages"),
                    "{id}: {help:?}"
                );
            } else {
                for text in [new_profile.as_str(), info.as_str()]
                    .into_iter()
                    .chain(help.iter().map(String::as_str))
                {
                    let lower = text.to_lowercase();
                    assert!(
                        !lower.contains("dial") && !lower.contains("touch strip"),
                        "{id}: {text:?} offers a control the model lacks"
                    );
                }
                assert!(
                    help.iter().any(|text| text.starts_with("Click a key")),
                    "{id}: {help:?}"
                );
            }
        }
    });
}

#[test]
fn the_texts_of_each_model_name_only_its_own_controls() {
    run_gtk_test(|| {
        for (model, new_profile, select, link) in [
            (
                &CLASSIC,
                "New profile: add a profile with its own keys and pages.",
                "Click a key on the preview.",
                "key",
            ),
            (
                &XL,
                "New profile: add a profile with its own keys and pages.",
                "Click a key on the preview.",
                "key",
            ),
            (
                neo(),
                "New profile: add a profile with its own keys, pages and touch buttons.",
                "Click a key, the info window or a touch button on the preview.",
                "key or touch button",
            ),
            (
                &PLUS,
                "New profile: add a profile with its own keys, dials, pages and touch strip.",
                PLUS_SELECT_HELP,
                "key or dial",
            ),
        ] {
            let Some(f) = model_fixture("texts", model, |_, _| {}) else {
                return;
            };
            let root: gtk::Widget = f.widgets.root.clone().upcast();
            assert_eq!(
                class_of(&root, "rail-new-profile")
                    .tooltip_text()
                    .unwrap()
                    .as_str(),
                new_profile
            );
            if model.dials == 0 {
                assert_eq!(
                    f.widgets
                        .binding_ui
                        .info_toggle
                        .tooltip_text()
                        .unwrap()
                        .as_str(),
                    "Replaces the image with a live picture, such as the volume or the time."
                );
            }
            let help = help_texts(&f.widgets._configuration_menu.help_for_test());
            assert!(help.iter().any(|text| text == select), "{help:?}");
            assert_eq!(
                help.iter().any(|text| text == "Touch strip pages"),
                model.has_touch(),
                "{help:?}"
            );
            // The page folders say where a link is saved.
            let linking =
                format!("Link saves navigation on the selected {link} without opening it.");
            assert!(
                descendants_of::<Label>(&root)
                    .iter()
                    .any(|label| label.text() == linking.as_str()),
                "{}: {linking}",
                model.id
            );
        }
    });
}

#[test]
fn the_catalog_assigns_actions_to_every_key_of_a_model_without_dials() {
    run_gtk_test(|| {
        for model in [&CLASSIC, &XL, neo()] {
            let Some(f) = model_fixture("keys-catalog", model, |_, _| {}) else {
                return;
            };
            let id = model.id;
            let last = model.keys();
            let key = f.widgets.preview.control(ControlRef::Key(last)).unwrap();
            worker_requests(&f);
            // A parameter-free action dropped on the last key is saved, never run.
            assert!(
                drop_row(&f, &row_tooltip(model, "Play / pause"), &key),
                "{id}"
            );
            assert_eq!(
                f.layers.borrow().binding("Base", &Trigger::KeyPress(last)),
                Some(media(MediaCommand::PlayPause)),
                "{id}"
            );
            assert_eq!(f.widgets.preview.selected(), ControlRef::Key(last), "{id}");
            assert_eq!(worker_requests(&f), (true, false), "{id}");
            // Clicking a row assigns it to the selected key.
            select(&f, ControlRef::Key(2));
            catalog_row(&f, &row_tooltip(model, "Next track")).emit_clicked();
            assert_eq!(
                f.layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(media(MediaCommand::Next)),
                "{id}"
            );
            assert_eq!(worker_requests(&f), (true, false), "{id}");
        }
        // The Neo's touch buttons take their action from the Action selector.
        let Some(f) = model_fixture("neo-catalog", neo(), |_, _| {}) else {
            return;
        };
        select(&f, ControlRef::TouchButton(1));
        catalog_row(&f, &row_tooltip(neo(), "Play / pause")).emit_clicked();
        assert_eq!(
            f.widgets.binding_ui.status.text().as_str(),
            "Select a key before assigning an action."
        );
        assert_eq!(
            f.layers.borrow().binding("Base", &Trigger::TouchButton(1)),
            None
        );
        assert_eq!(worker_requests(&f), (false, false));
    });
}

#[test]
fn the_editor_refuses_dials_and_a_touch_strip_on_a_model_without_them() {
    run_gtk_test(|| {
        for model in [&CLASSIC, &XL, neo()] {
            let Some(f) = model_fixture("refuse", model, with_touch_pages) else {
                return;
            };
            let ui = &f.widgets.binding_ui;
            let touchscreen = f.widgets.feature_runtime.touchscreen_controls().clone();
            select(&f, ControlRef::Key(3));
            let heading = class_of(f.widgets.root.upcast_ref(), "control-heading")
                .downcast::<Label>()
                .unwrap();
            assert_eq!(
                ui.select_control(ControlKind::Encoder, Some(1), true),
                Err(format!("the {} has no dials", model.name))
            );
            assert_eq!(
                ui.select_control(ControlKind::Touch, None, true),
                Err(format!("the {} has no touch strip", model.name))
            );
            assert_eq!(
                ui.select_control(ControlKind::Touch, Some(1), true),
                Err(format!("the {} has no touch strip", model.name))
            );
            // The editor stays on the key it was on.
            assert_eq!(ui.selected_control(), Some(ControlRef::Key(3)));
            assert_eq!(heading.text().as_str(), "Configuring: Key 3");
            assert!(!touchscreen.is_visible(), "{}", model.id);
            // Even an editor forced onto a dial or the strip offers no touch pages.
            for forced in [1, 2] {
                ui.control_picker.set_selected(forced);
                assert!(!touchscreen.is_visible(), "{}: {forced}", model.id);
            }
            ui.control_picker.set_selected(0);
        }
        // The Stream Deck + still selects them all.
        let Some(f) = model_fixture("refuse-plus", &PLUS, with_touch_pages) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        let touchscreen = f.widgets.feature_runtime.touchscreen_controls().clone();
        ui.select_control(ControlKind::Encoder, Some(4), true)
            .unwrap();
        assert!(touchscreen.is_visible());
        ui.select_control(ControlKind::Touch, None, true).unwrap();
        assert!(touchscreen.is_visible());
        ui.select_control(ControlKind::Button, Some(1), true)
            .unwrap();
        assert!(!touchscreen.is_visible());
    });
}
