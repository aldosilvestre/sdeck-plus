//! Save, Test and Remove live as icon-only buttons above the sidebar tabs.
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
fn save_test_and_remove_are_icon_buttons_above_the_sidebar_tabs() {
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
    let mut switchers: Vec<gtk::StackSwitcher> = Vec::new();
    collect(widgets.root.upcast_ref(), &mut switchers);
    let switcher = switchers
        .into_iter()
        .find(|switcher| switcher.has_css_class("sidebar-tabs"))
        .expect("sidebar tabs");
    let tabs = switcher.compute_bounds(&widgets.root).unwrap();
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
        assert!(
            !button.is_ancestor(&sidebar_settings(widgets)),
            "{name} left the inspector"
        );
        let bounds = button.compute_bounds(&widgets.root).unwrap();
        assert!(
            bounds.y() + bounds.height() <= tabs.y() + 0.5,
            "{name} sits above the tabs: {bounds:?} vs {tabs:?}"
        );
    }
}
