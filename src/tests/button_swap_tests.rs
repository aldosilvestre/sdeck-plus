use super::*;

fn failed_swap_preserves_configuration(block_artwork: bool) {
    // Given: distinct saved bindings, one image, and an unavailable write destination.
    let (root, mut store, mut images, source) = artwork_fixture("swap-failure");
    store
        .bind(
            "Base",
            Trigger::KeyPress(1),
            Action::Command("first".into()),
        )
        .unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(2),
            Action::Command("second".into()),
        )
        .unwrap();
    store.persist().unwrap();
    let image = images
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &source)
        .unwrap();
    let blocker = if block_artwork {
        root.join("artwork/artwork.conf.tmp")
    } else {
        root.join("layers/layers.conf.tmp")
    };
    std::fs::create_dir(&blocker).unwrap();
    std::fs::write(blocker.join("sentinel"), b"reserved").unwrap();
    let (commands, receiver) = mpsc::channel();
    let (output, events) = mpsc::channel();
    commands
        .send(LayerCommand::SwapKeys {
            store,
            artwork_root: images.root().into(),
            layer: "Base".into(),
            first: 1,
            second: 2,
        })
        .unwrap();
    commands.send(LayerCommand::Shutdown).unwrap();
    // When: the serial persistence worker attempts the exchange.
    feature_runtime::run_worker(
        receiver,
        output,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    // Then: neither file exposes a partial swap, including after reopening.
    assert!(matches!(
        events.try_recv().unwrap(),
        MonitorEvent::KeySwapResult { result: Err(_), .. }
    ));
    let reopened = LayerStore::open_at(root.join("layers")).unwrap();
    assert_eq!(
        reopened.layers()["Base"].bindings[&Trigger::KeyPress(1)],
        Action::Command("first".into())
    );
    assert_eq!(
        reopened.layers()["Base"].bindings[&Trigger::KeyPress(2)],
        Action::Command("second".into())
    );
    let images = ArtworkStore::open_at_for_layer(root.join("artwork"), Some("Base")).unwrap();
    assert_eq!(
        images.assigned_path_in_layer("Base", ArtworkTarget::Keypad(1)),
        Some(image)
    );
    assert!(images
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(2))
        .is_none());
    assert_eq!(
        std::fs::read(blocker.join("sentinel")).unwrap(),
        b"reserved"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_artwork_swap_rolls_back_bindings() {
    failed_swap_preserves_configuration(true);
}

#[test]
fn failed_binding_swap_does_not_change_artwork() {
    failed_swap_preserves_configuration(false);
}

fn controller<T: IsA<gtk::EventController> + IsA<glib::Object> + glib::object::IsClass>(
    widget: &gtk::Widget,
) -> T {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items())
        .find_map(|index| controllers.item(index)?.downcast::<T>().ok())
        .expect("native controller")
}

#[test]
fn dragging_keys_swaps_saved_actions_images_and_text() {
    let _guard = gtk_test_lock();
    if gtk::init().is_err() {
        assert!(
            std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "GTK initialization failed despite an advertised display"
        );
        return;
    }
    // Given: two different saved configurations and artwork on the current page.
    let (root, mut store, mut images, source) = artwork_fixture("key-swap");
    store
        .bind(
            "Base",
            Trigger::KeyPress(1),
            Action::Command("first".into()),
        )
        .unwrap();
    store
        .bind(
            "Base",
            Trigger::KeyPress(2),
            Action::Command("second".into()),
        )
        .unwrap();
    store.persist().unwrap();
    let first_image = images
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &source)
        .unwrap();
    let pixbuf =
        gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 2, 2).unwrap();
    pixbuf.fill(0xff6633ff);
    pixbuf.savev(&source, "png", &[]).unwrap();
    let second_image = images
        .assign_in_layer("Base", ArtworkTarget::Keypad(2), &source)
        .unwrap();
    for (key, text) in [(1, "Alpha"), (2, "Beta")] {
        images
            .set_button_text_in_layer(
                "Base",
                ArtworkTarget::Keypad(key),
                sdeck_plus::button_text::ButtonText {
                    text: text.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let layers = Rc::new(RefCell::new(store));
    let (output, hardware) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(images))),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output,
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(Default::default())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
    };
    let (commands, requests) = mpsc::channel();
    let (events, receiver) = mpsc::channel();
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(800)
        .build();
    install_style(&window);
    let widgets = build_widgets(
        window.clone(),
        layers.clone(),
        artwork.clone(),
        commands.clone(),
    );
    let view = widgets.root.clone();
    let stage = gtk::Fixed::new();
    view.set_size_request(1000, 800);
    stage.put(&view, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);
    let capture = |state: &str| {
        if let Some(directory) = std::env::var_os("STREAMDECK_SWAP_QA_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            let settings = gtk::Settings::default().unwrap();
            let theme = settings.gtk_theme_name();
            let dark = settings.is_gtk_application_prefer_dark_theme();
            for (variant, prefer_dark) in [("light", false), ("dark", true)] {
                settings.set_gtk_theme_name(Some(if prefer_dark {
                    "Adwaita-dark"
                } else {
                    "Adwaita"
                }));
                settings.set_gtk_application_prefer_dark_theme(prefer_dark);
                for (width, height) in [(640, 480), (1000, 800)] {
                    view.set_size_request(width, height);
                    view.allocate(width, height, -1, None);
                    rendered_texture(&window, &view)
                        .save_to_png(
                            std::path::Path::new(&directory)
                                .join(format!("{variant}-{width}x{height}-{state}.png")),
                        )
                        .unwrap();
                }
            }
            settings.set_gtk_theme_name(theme.as_deref());
            settings.set_gtk_application_prefer_dark_theme(dark);
        }
    };
    capture("before");
    let source_control = controller::<gtk::DragSource>(&widgets.key_widgets[0]);
    let target = controller::<gtk::DropTarget>(&widgets.key_widgets[1]);
    let self_target = controller::<gtk::DropTarget>(&widgets.key_widgets[0]);
    let provider = source_control
        .emit_by_name::<Option<gtk::gdk::ContentProvider>>("prepare", &[&0.0f64, &0.0f64])
        .unwrap();
    let value = provider.value(glib::BoxedAnyObject::static_type()).unwrap();
    let boxed = glib::BoxedValue(value);
    assert!(!self_target.emit_by_name::<bool>("drop", &[&boxed, &0.0f64, &0.0f64]));
    assert!(
        requests.try_recv().is_err(),
        "self-drop must not enqueue a write"
    );
    layers.borrow_mut().select("Studio");
    assert!(!target.emit_by_name::<bool>("drop", &[&boxed, &0.0f64, &0.0f64]));
    assert!(
        requests.try_recv().is_err(),
        "changing page cancels the old drag"
    );
    layers.borrow_mut().select("Base");
    target.emit_by_name::<gtk::gdk::DragAction>("enter", &[&0.0f64, &0.0f64]);
    assert!(widgets.key_widgets[1].has_css_class("swap-target"));
    widgets.key_widgets[0].add_css_class("swap-source");
    capture("destination");
    widgets.key_widgets[0].remove_css_class("swap-source");
    target.emit_by_name::<()>("leave", &[]);
    assert!(!widgets.key_widgets[1].has_css_class("swap-target"));
    let shell = view
        .child()
        .unwrap()
        .downcast::<gtk::Viewport>()
        .unwrap()
        .child()
        .unwrap();
    let completed = Rc::new(Cell::new(false));
    let done = completed.clone();
    let main_loop = glib::MainLoop::new(None, false);
    let wake = main_loop.clone();
    let signal = shell.connect_sensitive_notify(move |shell| {
        if shell.is_sensitive() {
            done.set(true);
            wake.quit();
        }
    });
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let wake = main_loop.clone();
    let deadline = glib::timeout_add_local_once(Duration::from_secs(5), move || {
        flag.set(true);
        wake.quit();
    });
    let worker_output = events.clone();
    let runtime = widgets.feature_runtime.clone();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker = thread::spawn(move || feature_runtime::run_worker(requests, worker_output, stop));
    let polling = start_event_updates(
        widgets,
        receiver,
        layers.clone(),
        artwork.clone(),
        commands.clone(),
        events,
    );
    // When: deliver the native prepared payload to the real destination controller.
    assert!(target.emit_by_name::<bool>("drop", &[&boxed, &0.0f64, &0.0f64]));
    assert!(!shell.is_sensitive());
    // Navigation may request a save while the disk swap is in flight.
    runtime.persist(layers.borrow().clone());
    if !completed.get() {
        main_loop.run();
    }
    shell.disconnect(signal);
    if !expired.get() {
        deadline.remove();
    }
    assert!(
        completed.get(),
        "swap must finish without a timing-dependent sleep"
    );
    capture("after");
    // Then: both disk state and the refreshed UI contain the opposite configurations.
    wait_for_frame(&window);
    commands.send(LayerCommand::Shutdown).unwrap();
    worker.join().unwrap();
    assert_eq!(
        layers.borrow().layers()["Base"].bindings[&Trigger::KeyPress(1)],
        Action::Command("second".into())
    );
    let disk = LayerStore::open_at(root.join("layers")).unwrap();
    assert_eq!(
        disk.layers()["Base"].bindings[&Trigger::KeyPress(2)],
        Action::Command("first".into())
    );
    let disk_images = ArtworkStore::open_at_for_layer(root.join("artwork"), Some("Base")).unwrap();
    assert_eq!(
        disk_images.assigned_path_in_layer("Base", ArtworkTarget::Keypad(1)),
        Some(second_image)
    );
    assert_eq!(
        disk_images.assigned_path_in_layer("Base", ArtworkTarget::Keypad(2)),
        Some(first_image)
    );
    assert_eq!(
        disk_images
            .button_text_in_layer("Base", ArtworkTarget::Keypad(1))
            .unwrap()
            .text,
        "Beta"
    );
    assert_eq!(
        artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .button_text_in_layer("Base", ArtworkTarget::Keypad(2))
            .unwrap()
            .text,
        "Alpha"
    );
    assert!(hardware
        .try_iter()
        .any(|command| matches!(command, ArtworkCommand::Replace(_))));
    if let Some(directory) = std::env::var_os("STREAMDECK_SWAP_QA_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        rendered_texture(&window, &view)
            .save_to_png(std::path::Path::new(&directory).join("keys-swapped.png"))
            .unwrap();
    }
    polling.remove();
    window.destroy();
    std::fs::remove_dir_all(root).unwrap();
}
