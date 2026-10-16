use super::*;

fn pump_until(mut done: impl FnMut() -> bool) {
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let timeout = glib::timeout_add_local_once(Duration::from_secs(5), move || flag.set(true));
    let context = glib::MainContext::default();
    while !done() && !expired.get() {
        context.iteration(true);
    }
    assert!(done(), "GTK event was not applied before timeout");
    if !expired.get() {
        timeout.remove();
    }
}

pub(super) fn assert_control_feedback() {
    let (root, layers, mut artwork_store, source) = artwork_fixture("control-feedback");
    for target in [
        ArtworkTarget::Keypad(1),
        ArtworkTarget::EncoderZone(1),
        ArtworkTarget::TouchStripBackground,
    ] {
        artwork_store
            .assign_in_layer("Base", target, &source)
            .unwrap();
    }
    let layers = Rc::new(RefCell::new(layers));
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(700)
        .build();
    install_style(&window);
    let (output, _hardware) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(artwork_store))),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output,
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(Default::default())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
        info: Rc::default(),
    };
    let (layer_sender, _layer_commands) = mpsc::channel();
    let widgets = build_widgets(
        window.clone(),
        layers.clone(),
        artwork.clone(),
        layer_sender,
    );
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(1000, 700);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);
    let view = widgets.root.clone();
    let keys = widgets.key_widgets.clone();
    let encoders = widgets.encoder_widgets.clone();
    let zones = widgets.zone_widgets.clone();
    let (events, receiver) = mpsc::channel();
    let (layer_sender, _layer_commands) = mpsc::channel();
    let polling = start_event_updates(
        widgets,
        receiver,
        layers,
        artwork,
        layer_sender,
        events.clone(),
    );
    let capture = |name: &str| {
        if let Some(dir) = std::env::var_os("STREAMDECK_FEEDBACK_QA_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            rendered_texture(&window, &view)
                .save_to_png(dir.join(format!("{name}.png")))
                .unwrap();
        }
    };
    let send = |event: protocol::Event| events.send(MonitorEvent::Input(event)).unwrap();
    let idle = rendered_pixels(&window, &view);
    capture("idle");

    send(protocol::Event::Key {
        index: 0,
        pressed: true,
    });
    pump_until(|| keys[0].has_css_class("control-pressed"));
    assert_ne!(idle, rendered_pixels(&window, &view), "key press visible");
    capture("key-pressed");
    send(protocol::Event::Key {
        index: 0,
        pressed: false,
    });
    pump_until(|| !keys[0].has_css_class("control-pressed"));

    send(protocol::Event::EncoderButton {
        index: 1,
        pressed: true,
    });
    pump_until(|| encoders[1].has_css_class("control-pressed"));
    capture("dial-pressed");
    send(protocol::Event::EncoderButton {
        index: 1,
        pressed: false,
    });
    pump_until(|| !encoders[1].has_css_class("control-pressed"));
    send(protocol::Event::Rotation { index: 2, ticks: 1 });
    pump_until(|| encoders[2].has_css_class("control-rotated"));
    assert!(!encoders[1].has_css_class("control-rotated"));
    assert_ne!(idle, rendered_pixels(&window, &view), "rotation visible");
    capture("dial-rotated");
    pump_until(|| !encoders[2].has_css_class("control-rotated"));

    // The 800 px strip maps to four 200 px zones; zone 1 has artwork (production mapping).
    send(protocol::Event::TouchTap { x: 50, y: 10 });
    pump_until(|| zones[0].has_css_class("control-touched"));
    assert!((1..4).all(|zone| !zones[zone].has_css_class("control-touched")));
    assert_ne!(idle, rendered_pixels(&window, &view), "touch visible");
    capture("touch-zone-1");
    pump_until(|| !zones[0].has_css_class("control-touched"));
    send(protocol::Event::TouchTap { x: 799, y: 10 });
    pump_until(|| zones[3].has_css_class("control-touched"));
    assert!(!zones[0].has_css_class("control-touched"));
    pump_until(|| !zones[3].has_css_class("control-touched"));
    send(protocol::Event::TouchFlick {
        start_x: 700,
        start_y: 10,
        end_x: 100,
        end_y: 10,
    });
    pump_until(|| {
        zones
            .iter()
            .all(|zone| zone.has_css_class("control-touched"))
    });
    capture("touch-flick");
    pump_until(|| {
        zones
            .iter()
            .all(|zone| !zone.has_css_class("control-touched"))
    });

    polling.remove();
    window.destroy();
    std::fs::remove_dir_all(root).unwrap();
    eprintln!("GTK control feedback verified: key, dial, rotation and touch with artwork");
}
