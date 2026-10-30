//! Information buttons show their help on hover and keyboard focus.
use super::*;

fn help_button(window_child: &gtk::Widget) -> gtk::MenuButton {
    fn find(widget: &gtk::Widget) -> Option<gtk::MenuButton> {
        if let Some(button) = widget.downcast_ref::<gtk::MenuButton>() {
            if button.has_css_class("field-help") {
                return Some(button.clone());
            }
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
    find(window_child).expect("help button")
}

fn controller<T: IsA<gtk::EventController> + IsA<glib::Object> + glib::object::IsClass>(
    widget: &gtk::Widget,
) -> T {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items())
        .find_map(|index| controllers.item(index)?.downcast::<T>().ok())
        .expect("controller")
}

/// Wait (bounded) for the popover to reach the expected mapped state.
fn settle(panel: &gtk::Popover, mapped: bool) -> bool {
    let started = std::time::Instant::now();
    while panel.is_mapped() != mapped && started.elapsed() < Duration::from_secs(3) {
        glib::MainContext::default().iteration(false);
    }
    panel.is_mapped() == mapped
}

#[test]
fn help_buttons_open_on_hover_and_close_when_the_pointer_leaves() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let entry = Entry::new();
        let row = labeled("Profile name", &entry);
        ui::field_help(&row, "Profile help", "Create a separate set of bindings.");
        let window = ApplicationWindow::builder().default_width(400).build();
        install_style(&window);
        window.set_child(Some(&row));
        window.present();
        wait_for_frame(&window);
        let help = help_button(row.upcast_ref());
        let panel = help.popover().unwrap();
        assert!(!panel.is_mapped());
        // Hover shows the help without a click.
        let motion = controller::<gtk::EventControllerMotion>(help.upcast_ref());
        motion.emit_by_name::<()>("enter", &[&2.0f64, &2.0f64]);
        assert!(settle(&panel, true), "hover opens the help");
        assert_eq!(help.tooltip_text(), None, "no second, native tooltip");
        motion.emit_by_name::<()>("leave", &[]);
        assert!(settle(&panel, false), "leaving closes it");
        // Keyboard focus shows it too (only when the focus is a visible, keyboard one).
        window.set_focus_visible(true);
        let focus = controller::<gtk::EventControllerFocus>(help.upcast_ref());
        focus.emit_by_name::<()>("enter", &[]);
        assert!(settle(&panel, true), "focus opens the help");
        focus.emit_by_name::<()>("leave", &[]);
        assert!(settle(&panel, false));
        window.destroy();
    });
}

#[test]
fn help_icons_are_bare_icons_without_a_button_frame() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let entry = Entry::new();
        let row = labeled("Profile name", &entry);
        ui::field_help(&row, "Profile help", "Create a separate set of bindings.");
        let window = ApplicationWindow::builder().default_width(400).build();
        install_style(&window);
        window.set_child(Some(&row));
        window.present();
        wait_for_frame(&window);
        let help = help_button(row.upcast_ref());
        assert!(!help.has_frame(), "the icon is not drawn as a button");
        // No border and no fill: only the glyph shows.
        let inner = help.first_child().unwrap();
        let bounds = inner.compute_bounds(&help).unwrap();
        assert!(bounds.width() <= 28.0, "compact glyph: {bounds:?}");
        assert!(inner.has_css_class("flat"), "flat inner button");
        window.destroy();
    });
}
