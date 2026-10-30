//! The device preview keeps one size; tall configuration scrolls on its own.
use super::ui_refresh_tests::fixture;
use super::*;

fn preview_size(widgets: &MonitorWidgets) -> (f32, f32, f64) {
    let content = widgets.root.child().unwrap();
    let canvas = device_canvas(widgets);
    let bounds = canvas.compute_bounds(&content).unwrap();
    let scale = canvas
        .parent()
        .unwrap()
        .downcast::<scaled_bin::ScaledBin>()
        .unwrap()
        .scale();
    (bounds.width(), bounds.height(), scale)
}

#[test]
fn preview_keeps_one_size_across_actions_controls_and_window_widths() {
    run_gtk_test(assert_preview_is_fixed);
}

fn assert_preview_is_fixed() {
    let Some(fixture) = fixture("preview-fixed", |_| {}) else {
        return;
    };
    let widgets = &fixture.widgets;
    let ui = &widgets.binding_ui;
    widgets.root.allocate(1000, 700, -1, None);
    let baseline = preview_size(widgets);
    let same = |label: &str| {
        let now = preview_size(widgets);
        assert!(
            (now.0 - baseline.0).abs() <= 0.5
                && (now.1 - baseline.1).abs() <= 0.5
                && (now.2 - baseline.2).abs() < 0.001,
            "{label}: preview changed from {baseline:?} to {now:?}"
        );
    };
    for action in 0..8 {
        ui.action_picker.set_selected(action);
        ui.update_action_visibility();
        widgets.root.allocate(1000, 700, -1, None);
        same(&format!("action {action}"));
    }
    for (width, height) in [(1280, 800), (1840, 980), (900, 700)] {
        widgets.root.allocate(width, height, -1, None);
        same(&format!("window {width}x{height}"));
    }
    ui.select_control(ControlKind::Encoder, Some(1), true)
        .unwrap();
    widgets.root.allocate(1000, 700, -1, None);
    same("dial selected");
}

#[test]
fn tall_configuration_scrolls_inside_its_own_region() {
    run_gtk_test(assert_configuration_scrolls);
}

fn assert_configuration_scrolls() {
    let Some(fixture) = fixture("config-scroll", |_| {}) else {
        return;
    };
    let widgets = &fixture.widgets;
    let ui = &widgets.binding_ui;
    let settings = sidebar_settings(widgets);
    let scroller = settings
        .ancestor(gtk::ScrolledWindow::static_type())
        .and_downcast::<gtk::ScrolledWindow>()
        .expect("configuration region scroller");
    assert_ne!(scroller, widgets.root, "its own scroller, not the page's");
    widgets.root.allocate(1000, 560, -1, None);
    ui.action_picker.set_selected(5);
    ui.update_action_visibility();
    ui.return_toggle.set_active(true);
    widgets.root.allocate(1000, 560, -1, None);
    wait_for_frame(&widgets.root);
    let adjustment = scroller.vadjustment();
    assert!(
        adjustment.upper() > adjustment.page_size(),
        "a tall form scrolls within its region: {}/{}",
        adjustment.upper(),
        adjustment.page_size()
    );
}
