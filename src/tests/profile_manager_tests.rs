use super::*;

fn capture_profile_panel(window: &ApplicationWindow, widget: &gtk::Widget) -> gtk::gdk::Texture {
    let Some(panel) = widget
        .downcast_ref::<gtk::Popover>()
        .filter(|panel| panel.has_css_class("help-popover"))
    else {
        return rendered_texture(window, widget);
    };
    // Hover/focus help is intentionally transient. Capture its real content in
    // an owned viewport instead of requiring the shared desktop to keep the
    // native popup open. Hover/focus behavior is tested separately.
    let content = panel.child().unwrap();
    let request = (content.width_request(), content.height_request());
    let width = content.measure(gtk::Orientation::Horizontal, -1).1.max(1);
    let height = content.measure(gtk::Orientation::Vertical, width).1.max(1);
    panel.set_child(gtk::Widget::NONE);
    content.set_size_request(width, height);
    let stage = gtk::Fixed::new();
    stage.put(&content, 0.0, 0.0);
    let capture_window = ApplicationWindow::builder()
        .default_width(width)
        .default_height(height)
        .child(&stage)
        .build();
    install_style(&capture_window);
    capture_window.present();
    let texture = rendered_texture(&capture_window, &content);
    stage.remove(&content);
    content.set_size_request(request.0, request.1);
    panel.set_child(Some(&content));
    capture_window.destroy();
    texture
}

#[test]
fn profile_help_capture_survives_pointer_leave_and_restores_its_content() {
    run_gtk_test(|| {
        let row = labeled("Profile name", &Entry::new());
        ui::field_help(&row, "Profile help", "Create a separate set of bindings.");
        let window = ApplicationWindow::builder().child(&row).build();
        install_style(&window);
        let settings = window.settings();
        let animations = settings.is_gtk_enable_animations();
        settings.set_gtk_enable_animations(false);
        window.present();
        wait_for_frame(&window);
        let help = row
            .first_child()
            .unwrap()
            .last_child()
            .unwrap()
            .downcast::<gtk::MenuButton>()
            .unwrap();
        let panel = help.popover().unwrap();
        let content = panel.child().unwrap();
        let request = (content.width_request(), content.height_request());
        let controllers = help.observe_controllers();
        let motion = (0..controllers.n_items())
            .find_map(|index| {
                controllers
                    .item(index)?
                    .downcast::<gtk::EventControllerMotion>()
                    .ok()
            })
            .unwrap();
        motion.emit_by_name::<()>("enter", &[&2.0f64, &2.0f64]);
        assert!(panel.is_mapped(), "production hover must open the help");
        // Leave before the first published frame: the capture must not depend
        // on the shared desktop keeping the pointer over this help button.
        motion.emit_by_name::<()>("leave", &[]);
        let texture = capture_profile_panel(&window, panel.upcast_ref());
        assert!(
            !panel.is_visible(),
            "normal hover dismissal must remain unchanged"
        );
        assert_eq!(panel.child().as_ref(), Some(&content));
        assert_eq!((content.width_request(), content.height_request()), request);
        let mut pixels = vec![0; (texture.width() * texture.height() * 4) as usize];
        texture.download(&mut pixels, texture.width() as usize * 4);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[..] != pixels[..4]),
            "capture must contain rendered help, not an empty surface"
        );
        settings.set_gtk_enable_animations(animations);
        window.destroy();
    });
}
