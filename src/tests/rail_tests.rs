//! The profile rail keeps its tools visible without scrolling in the default window.
use super::ui_refresh_tests::fixture;
use super::*;

#[test]
fn the_rail_tools_are_visible_at_the_default_window_without_scrolling() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("rail-reachability", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        wait_for_frame(&fixture.window);
        assert_eq!(
            (widgets.root.width(), widgets.root.height()),
            (DEFAULT_WIDTH, DEFAULT_HEIGHT)
        );
        assert_eq!(widgets.root.vadjustment().value(), 0.0);
        let page = widgets.root.vadjustment().page_size() as f32;
        for (name, tool) in [
            ("Organize", &widgets.management_menu),
            ("OBS", &widgets.obs_menu),
            ("Menu", &widgets.help_menu),
        ] {
            let bounds = tool.compute_bounds(&widgets.root).unwrap();
            assert!(
                bounds.y() >= 0.0 && bounds.y() + bounds.height() <= page,
                "{name} is visible without scrolling: {bounds:?} in {page}"
            );
        }
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
        }
        settings.set_gtk_theme_name(theme.as_deref());
        settings.set_gtk_application_prefer_dark_theme(dark);
    });
}
