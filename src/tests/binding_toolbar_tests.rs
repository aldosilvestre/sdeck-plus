//! Save, Test and Remove live as icon-only buttons in the editor card's heading row.
use super::ui_refresh_tests::fixture;
use super::*;

fn collect<T: IsA<gtk::Widget> + glib::object::IsClass>(widget: &gtk::Widget, out: &mut Vec<T>) {
    if let Ok(found) = widget.clone().downcast::<T>() {
        out.push(found);
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect(&current, out);
        child = current.next_sibling();
    }
}

#[test]
fn save_test_and_remove_are_icon_buttons_in_the_editor_heading_row() {
    run_gtk_test(assert_binding_toolbar);
}

fn assert_binding_toolbar() {
    // Production registers the bundled icons at startup, before any window.
    if init_gtk_for_test() {
        app_icon::register().unwrap();
    }
    let Some(fixture) = fixture("binding-toolbar", |_| {}) else {
        return;
    };
    let widgets = &fixture.widgets;
    let theme = gtk::IconTheme::for_display(&gtk::prelude::WidgetExt::display(&fixture.window));
    for icon in ["sdeck-save-symbolic", "sdeck-remove-symbolic"] {
        assert!(theme.has_icon(icon), "bundled icon {icon} is registered");
    }
    wait_for_frame(&fixture.window);
    if let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        rendered_texture(&fixture.window, &widgets.root)
            .save_to_png(std::path::Path::new(&directory).join("binding-toolbar.png"))
            .unwrap();
    }
    let mut buttons: Vec<Button> = Vec::new();
    collect(widgets.root.upcast_ref(), &mut buttons);
    let mut labels: Vec<Label> = Vec::new();
    collect(widgets.root.upcast_ref(), &mut labels);
    let heading = labels
        .into_iter()
        .find(|label| label.has_css_class("control-heading"))
        .expect("editor heading");
    let heading_row = heading.parent().expect("heading row");
    assert!(
        heading_row.has_css_class("editor-heading-row"),
        "the heading sits in its own row"
    );
    // The row is the editor card's fixed top; the scrolling form below it is in the same card.
    let card = heading_row.parent().expect("editor card");
    assert!(card.has_css_class("editor-card"));
    assert!(
        sidebar_settings(widgets).is_ancestor(&card),
        "the heading row and the form share the editor card"
    );
    let heading_bounds = heading.compute_bounds(&widgets.root).unwrap();
    let mut previous_x = heading_bounds.x() + heading_bounds.width();
    for (name, icon, hint) in [
        ("Save binding", "sdeck-save-symbolic", "without running"),
        ("Test action", "media-playback-start-symbolic", "Runs"),
        ("Remove binding", "sdeck-remove-symbolic", "Deletes"),
    ] {
        let button = buttons
            .iter()
            .find(|button| {
                button
                    .tooltip_text()
                    .is_some_and(|tip| tip.lines().next() == Some(name))
            })
            .unwrap_or_else(|| panic!("{name} button"));
        assert!(button.label().is_none(), "{name} shows only its icon");
        let image = button
            .child()
            .and_downcast::<gtk::Image>()
            .unwrap_or_else(|| panic!("{name} child is an icon"));
        assert_eq!(image.icon_name().as_deref(), Some(icon), "{name}");
        let tip = button.tooltip_text().unwrap();
        let (title, explanation) = tip.split_once('\n').expect("explanatory tooltip");
        assert_eq!(title, name);
        assert!(explanation.contains(hint), "{name}: {explanation}");
        assert_eq!(
            button.parent().as_ref(),
            Some(&heading_row),
            "{name} shares the heading row"
        );
        let bounds = button.compute_bounds(&widgets.root).unwrap();
        let centre = bounds.y() + bounds.height() / 2.0;
        assert!(
            centre >= heading_bounds.y() && centre <= heading_bounds.y() + heading_bounds.height(),
            "{name} is level with the heading: {bounds:?} vs {heading_bounds:?}"
        );
        assert!(
            bounds.x() >= previous_x - 0.5,
            "{name} follows the heading and the previous button"
        );
        previous_x = bounds.x() + bounds.width();
    }
}
