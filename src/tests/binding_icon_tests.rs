//! Binding icons in the window: Action menu rows, preview keys, dials and
//! touch sections (feature: binding icons).
use super::ui_refresh_tests::fixture;
use super::*;
use sdeck_plus::binding_icons::BindingIcon;

fn descendants<T: IsA<gtk::Widget> + glib::object::IsClass>(widget: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    if let Ok(value) = widget.clone().downcast::<T>() {
        found.push(value);
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        found.extend(descendants::<T>(&widget));
        child = widget.next_sibling();
    }
    found
}

/// Icon widgets carry the vocabulary id in their name.
fn icon_ids(widget: &gtk::Widget) -> Vec<String> {
    descendants::<gtk::DrawingArea>(widget)
        .into_iter()
        .filter(|area| area.has_css_class("binding-icon") && area.is_visible())
        .map(|area| area.widget_name().to_string())
        .collect()
}

/// Open the dropdown's native popup and wait until it is mapped and painted.
fn open_popup(picker: &gtk::DropDown, panel: &gtk::Popover) {
    let main_loop = glib::MainLoop::new(None, false);
    let done = main_loop.clone();
    let signal = panel.connect_map(move |_| done.quit());
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let done = main_loop.clone();
    let timeout = glib::timeout_add_local_once(Duration::from_secs(5), move || {
        flag.set(true);
        done.quit();
    });
    assert!(picker.activate(), "native dropdown activation");
    if !panel.is_mapped() {
        main_loop.run();
    }
    panel.disconnect(signal);
    if !expired.get() {
        timeout.remove();
    }
    assert!(panel.is_mapped(), "dropdown popup did not map");
    wait_for_frame(panel);
}

#[test]
fn action_menu_rows_show_an_icon_name_and_check_mark() {
    run_gtk_test(assert_action_menu_icons);
}

fn assert_action_menu_icons() {
    let Some(fixture) = fixture("action-menu-icons", |_| {}) else {
        return;
    };
    let picker = &fixture.widgets.binding_ui.action_picker;
    picker.set_selected(3);
    let panel = descendants::<gtk::Popover>(picker.upcast_ref())
        .into_iter()
        .next()
        .expect("action menu popup");
    open_popup(picker, &panel);
    let expected: Vec<String> = (0..8)
        .map(|index| BindingIcon::for_action_type(index).id().to_owned())
        .collect();
    assert_eq!(icon_ids(panel.upcast_ref()), expected);
    let labels: Vec<String> = descendants::<GtkBox>(panel.upcast_ref())
        .into_iter()
        .filter(|row| row.has_css_class("icon-row"))
        .flat_map(|row| descendants::<Label>(row.upcast_ref()))
        .map(|label| label.text().to_string())
        .collect();
    assert_eq!(
        labels,
        [
            "Go to profile or page",
            "Run command",
            "Open application",
            "Shortcut",
            "Send named event",
            "More actions",
            "Play sound",
            "None",
        ]
    );
    // Exactly the selected row shows the check mark.
    let checks: Vec<gtk::Image> = descendants::<gtk::Image>(panel.upcast_ref())
        .into_iter()
        .filter(|image| image.icon_name().as_deref() == Some("object-select-symbolic"))
        .collect();
    assert_eq!(checks.len(), 8);
    let shown: Vec<usize> = checks
        .iter()
        .enumerate()
        .filter(|(_, image)| image.opacity() > 0.5)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(shown, [3]);
    panel.popdown();
    picker.set_selected(6);
    // The closed button shows the selected type's icon beside its name.
    assert_eq!(
        icon_ids(picker.upcast_ref()).first().map(String::as_str),
        Some("play-sound")
    );
    eprintln!("GTK action menu icon assertions executed");
}

#[test]
fn preview_dials_show_their_action_icon_with_the_caption() {
    run_gtk_test(assert_dial_icons);
}

fn assert_dial_icons() {
    let volume = Action::Feature(sdeck_plus::features::FeatureAction::Desktop(
        sdeck_plus::desktop_actions::DesktopAction::Media(
            sdeck_plus::desktop_actions::MediaCommand::VolumeUp,
        ),
    ));
    let Some(fixture) = fixture("dial-icons", |store| {
        store
            .bind("Base", Trigger::EncoderClockwise(2), volume.clone())
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::EncoderPress(3),
                Action::Shortcut {
                    modifiers: vec!["CTRL".into()],
                    key: "m".into(),
                },
            )
            .unwrap();
        store
            .bind("Base", Trigger::EncoderPress(4), Action::None)
            .unwrap();
    }) else {
        return;
    };
    let widgets = &fixture.widgets;
    let icons = &widgets.binding_ui.dial_icons;
    assert_eq!(icons.len(), 4);
    assert!(!icons[0].is_visible(), "unassigned dials stay plain");
    assert!(!icons[3].is_visible(), "None shows no icon");
    assert_eq!(icons[1].widget_name(), "volume-up");
    assert_eq!(icons[2].widget_name(), "shortcut");
    let content = widgets.root.child().unwrap();
    for index in [1, 2] {
        let icon = &icons[index];
        assert!(icon.is_visible() && icon.has_css_class("binding-icon"));
        assert!(!icon.can_target(), "the knob keeps every click");
        let knob = widgets.encoder_widgets[index]
            .compute_bounds(&content)
            .unwrap();
        let bounds = icon.compute_bounds(&content).unwrap();
        let center = |rect: &gtk::graphene::Rect| {
            (
                rect.x() + rect.width() / 2.0,
                rect.y() + rect.height() / 2.0,
            )
        };
        let (icon_x, icon_y) = center(&bounds);
        let (knob_x, knob_y) = center(&knob);
        assert!((icon_x - knob_x).abs() <= 1.0 && (icon_y - knob_y).abs() <= 1.0);
        // The caption sits directly below the same knob.
        let caption = widgets.binding_ui.dial_legends[index]
            .compute_bounds(&content)
            .unwrap();
        assert!(caption.y() >= bounds.y() + bounds.height());
    }
    fixture
        .layers
        .borrow_mut()
        .unbind("Base", &Trigger::EncoderClockwise(2))
        .unwrap();
    widgets.binding_ui.refresh_summaries(&fixture.layers);
    assert!(!icons[1].is_visible());
    eprintln!("GTK dial icon assertions executed");
}
