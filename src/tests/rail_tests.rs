//! The fixed profile rail: headings, profiles, New profile and the tools at its foot, while
//! only the content column scrolls.
use super::organize_popover_tests::dialog_titled;
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

fn rail_label(widgets: &MonitorWidgets, text: &str) -> Label {
    let mut labels: Vec<Label> = Vec::new();
    collect(
        widgets.feature_runtime.profile_tabs().upcast_ref(),
        &mut labels,
    );
    labels
        .into_iter()
        .find(|label| label.text() == text)
        .unwrap_or_else(|| panic!("rail label {text}"))
}

fn new_profile_button(widgets: &MonitorWidgets) -> Button {
    let mut buttons: Vec<Button> = Vec::new();
    collect(
        widgets.feature_runtime.profile_tabs().upcast_ref(),
        &mut buttons,
    );
    buttons
        .into_iter()
        .find(|button| {
            button_caption(button).as_deref() == Some("New profile")
                && button.is_ancestor(widgets.feature_runtime.profile_tabs())
                && !button.is_ancestor(&widgets.management_menu.popover().unwrap())
        })
        .expect("flat New profile button in the rail")
}

fn top(widget: &impl IsA<gtk::Widget>, widgets: &MonitorWidgets) -> f32 {
    widget.compute_bounds(&widgets.root).unwrap().y()
}

#[test]
fn the_rail_lists_badge_profiles_new_profile_and_the_tools_in_artboard_order() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("rail-order", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        wait_for_frame(&fixture.window);
        let tabs = widgets.feature_runtime.profile_tabs();
        let badge = {
            let mut labels: Vec<Label> = Vec::new();
            collect(tabs.upcast_ref(), &mut labels);
            labels
                .into_iter()
                .find(|label| label.has_css_class("rail-badge"))
                .expect("connection badge")
        };
        let new_profile = new_profile_button(widgets);
        let ordered = [
            top(&badge, widgets),
            top(&rail_label(widgets, "PROFILES"), widgets),
            top(&rail_label(widgets, "Base"), widgets),
            top(&new_profile, widgets),
            top(&rail_label(widgets, "TOOLS"), widgets),
            top(&widgets.management_menu, widgets),
            top(&widgets.obs_menu, widgets),
            top(&widgets.help_menu, widgets),
        ];
        assert!(
            ordered.windows(2).all(|pair| pair[0] < pair[1]),
            "badge, PROFILES, profiles, New profile, TOOLS, Organize, OBS, Menu: {ordered:?}"
        );
        assert!(widgets.help_menu.is_ancestor(tabs), "Menu is a rail tool");
        assert_eq!(
            widgets.help_menu.tooltip_text().as_deref(),
            Some("Menu"),
            "Menu keeps its name"
        );
        let content = widgets.scroller.compute_bounds(&widgets.root).unwrap();
        for tool in [
            &widgets.management_menu,
            &widgets.obs_menu,
            &widgets.help_menu,
        ] {
            let bounds = tool.compute_bounds(&widgets.root).unwrap();
            assert!(
                bounds.x() + bounds.width() <= content.x() + 0.5,
                "tools sit left of the content column: {bounds:?} vs {content:?}"
            );
        }
    });
}

#[test]
fn new_profile_in_the_rail_opens_the_new_profile_dialog() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("rail-new-profile", |_| {}) else {
            return;
        };
        let button = new_profile_button(&fixture.widgets);
        button.emit_clicked();
        let dialog = dialog_titled(&fixture.window, "New profile");
        dialog.close();
    });
}

#[test]
fn the_rail_tools_and_their_popovers_fit_the_window_without_scrolling() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("rail-reachability", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        for (width, height) in [(DEFAULT_WIDTH, DEFAULT_HEIGHT), (1080, 700)] {
            widgets.root.set_size_request(width, height);
            wait_for_frame(&fixture.window);
            assert_eq!(widgets.scroller.vadjustment().value(), 0.0);
            for (name, tool) in [
                ("Organize", &widgets.management_menu),
                ("OBS", &widgets.obs_menu),
                ("Menu", &widgets.help_menu),
            ] {
                let bounds = tool.compute_bounds(&widgets.root).unwrap();
                assert!(
                    bounds.y() >= 0.0 && bounds.y() + bounds.height() <= height as f32,
                    "{name} is inside {width}x{height} without scrolling: {bounds:?}"
                );
                let panel = tool.popover().unwrap();
                let autohide = panel.is_autohide();
                panel.set_autohide(false);
                tool.popup();
                assert!(panel.is_visible(), "{name} opens at {width}x{height}");
                assert!(
                    panel.width() <= width && panel.height() <= height,
                    "{name} popover {}x{} fits {width}x{height}",
                    panel.width(),
                    panel.height()
                );
                tool.popdown();
                panel.set_autohide(autohide);
            }
        }
    });
}

#[test]
fn the_rail_does_not_move_when_the_content_scrolls() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("rail-fixed", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        wait_for_frame(&fixture.window);
        let adjustment = widgets.scroller.vadjustment();
        assert!(
            adjustment.upper() > adjustment.page_size(),
            "the content column overflows and scrolls: {} vs {}",
            adjustment.upper(),
            adjustment.page_size()
        );
        let tracked = |widgets: &MonitorWidgets| {
            [
                top(&widgets.management_menu, widgets),
                top(&widgets.help_menu, widgets),
                top(&rail_label(widgets, "PROFILES"), widgets),
                top(&rail_label(widgets, "Base"), widgets),
            ]
        };
        let before = tracked(widgets);
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
        wait_for_frame(&fixture.window);
        assert!(adjustment.value() > 0.0);
        assert_eq!(tracked(widgets), before, "the rail stays put");
    });
}

#[test]
fn the_editor_heading_row_stays_visible_while_the_form_scrolls() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("rail-sticky-heading", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        let save = widgets
            .binding_ui
            .status
            .ancestor(gtk::Window::static_type())
            .and_then(|_| {
                let mut buttons: Vec<Button> = Vec::new();
                collect(widgets.root.upcast_ref(), &mut buttons);
                buttons.into_iter().find(|button| {
                    button
                        .tooltip_text()
                        .is_some_and(|tip| tip.starts_with("Save binding"))
                })
            })
            .expect("Save binding");
        let row = save.parent().unwrap();
        assert!(row.has_css_class("editor-heading-row"));
        // A tiny form region forces the form to scroll.
        let region = widgets
            .binding_ui
            .action_picker
            .ancestor(gtk::ScrolledWindow::static_type());
        let region = region
            .and_downcast::<gtk::ScrolledWindow>()
            .expect("form scroller");
        assert!(
            !row.is_ancestor(&region),
            "the heading row sits above the scrolled form"
        );
        region.set_max_content_height(60);
        let filler = GtkBox::new(Orientation::Vertical, 0);
        filler.set_size_request(-1, 400);
        sidebar_settings(widgets)
            .downcast::<GtkBox>()
            .expect("form box")
            .append(&filler);
        wait_for_frame(&fixture.window);
        let before = row.compute_bounds(&widgets.root).unwrap();
        let adjustment = region.vadjustment();
        assert!(
            adjustment.upper() > adjustment.page_size(),
            "the form scrolls"
        );
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
        wait_for_frame(&fixture.window);
        assert!(adjustment.value() > 0.0);
        let after = row.compute_bounds(&widgets.root).unwrap();
        assert_eq!(before.y(), after.y(), "the heading row does not scroll");
        assert!(after.y() >= 0.0 && after.y() + after.height() <= widgets.root.height() as f32);
    });
}

/// Opt-in capture of the whole main window at 1080x700 in both themes.
#[test]
fn main_window_capture() {
    run_gtk_test(|| {
        let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") else {
            return;
        };
        // Production registers the bundled icons at startup, before any window.
        if init_gtk_for_test() {
            app_icon::register().unwrap();
        }
        let Some(fixture) = fixture("rail-capture", |_| {}) else {
            return;
        };
        std::fs::create_dir_all(&directory).unwrap();
        let widgets = &fixture.widgets;
        widgets.root.set_size_request(1080, 700);
        let settings = widgets.root.settings();
        let (theme, dark) = (
            settings.gtk_theme_name(),
            settings.is_gtk_application_prefer_dark_theme(),
        );
        for (name, prefer_dark) in [("dark", true), ("light", false)] {
            settings.set_gtk_theme_name(Some(if prefer_dark {
                "Adwaita-dark"
            } else {
                "Adwaita"
            }));
            settings.set_gtk_application_prefer_dark_theme(prefer_dark);
            wait_for_frame(&fixture.window);
            rendered_texture(&fixture.window, &widgets.root)
                .save_to_png(std::path::Path::new(&directory).join(format!("main-{name}.png")))
                .unwrap();
            if prefer_dark {
                let adjustment = widgets.scroller.vadjustment();
                adjustment.set_value(adjustment.upper() - adjustment.page_size());
                wait_for_frame(&fixture.window);
                rendered_texture(&fixture.window, &widgets.root)
                    .save_to_png(std::path::Path::new(&directory).join("main-dark-scrolled.png"))
                    .unwrap();
                adjustment.set_value(0.0);
                wait_for_frame(&fixture.window);
            }
        }
        settings.set_gtk_theme_name(theme.as_deref());
        settings.set_gtk_application_prefer_dark_theme(dark);
    });
}
