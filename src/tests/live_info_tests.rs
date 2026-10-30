use super::*;
use sdeck_plus::info::{Glyph, InfoSource, InfoValue};

/// Drains queued artwork, flattening snapshots, and keeps only information pictures.
fn info_pictures(commands: &mpsc::Receiver<ArtworkCommand>) -> Vec<(ArtworkTarget, InfoValue)> {
    let mut found = Vec::new();
    for command in commands.try_iter() {
        let list = match command {
            ArtworkCommand::Replace(list) => list,
            other => vec![other],
        };
        for command in list {
            if let ArtworkCommand::ComposeInfo { target, value, .. } = command {
                found.push((target, value));
            }
        }
    }
    found
}

#[test]
fn live_information_is_saved_drawn_on_keys_and_zones_and_updated_in_place() {
    run_gtk_test(assert_live_information_is_saved_drawn_on_keys_and_zones_and_updated_in_place);
}

fn assert_live_information_is_saved_drawn_on_keys_and_zones_and_updated_in_place() {
    if !init_gtk_for_test() {
        assert!(
            std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "GTK initialization failed despite an advertised display"
        );
        return;
    }
    let (root, store, mut images, source) = artwork_fixture("live-info");
    let strip =
        gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 800, 100).unwrap();
    strip.fill(0x4488ffff);
    strip.savev(&source, "png", &[]).unwrap();
    images
        .assign_in_layer("Base", ArtworkTarget::TouchStripBackground, &source)
        .unwrap();
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
        info: Rc::default(),
    };
    let (commands, requests) = mpsc::channel();
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(700)
        .build();
    install_style(&window);
    let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(1000, 700);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);

    widgets.feature_runtime.assert_brightness_info();
    // Size of the picture the window shows for a target, when it is visible.
    let preview = |target: ArtworkTarget| {
        let pictures = artwork.pictures.borrow();
        let (_, picture) = pictures.iter().find(|(shown, _)| *shown == target).unwrap();
        if !picture.get_visible() {
            return None;
        }
        picture
            .paintable()
            .map(|paintable| (paintable.intrinsic_width(), paintable.intrinsic_height()))
    };

    key_button(&widgets.key_labels[0]).emit_clicked();
    let ui = &widgets.binding_ui;
    ui.action_picker.set_selected(1);
    ui.text_entry.set_text("true");
    assert!(ui.info_toggle.is_visible(), "keys can show information");
    assert!(!ui.info_row.is_visible(), "off until the user opts in");
    ui.info_toggle.set_active(true);
    assert!(ui.info_row.is_visible() && !ui.info_location_row.is_visible());
    let position = |wanted: InfoSource| {
        u32::try_from(InfoSource::ALL.iter().position(|s| *s == wanted).unwrap()).unwrap()
    };
    ui.info_source.set_selected(position(InfoSource::Volume));
    let mut buttons = Vec::new();
    collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
    let find = |label: &str| {
        buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some(label))
            .unwrap()
    };
    while requests.try_recv().is_ok() {}
    while hardware.try_recv().is_ok() {}

    assert_eq!(preview(ArtworkTarget::Keypad(1)), None);
    // Saving stores the option and replaces the key image with a placeholder picture.
    find("Save binding").emit_clicked();
    let LayerCommand::SaveBinding(snapshot, _) = requests.try_recv().unwrap() else {
        panic!("save must persist the binding");
    };
    let key = Trigger::KeyPress(1);
    assert_eq!(snapshot.info_source("Base", &key), Some(InfoSource::Volume));
    assert_eq!(
        preview(ArtworkTarget::Keypad(1)),
        Some((120, 120)),
        "the window preview shows the picture"
    );
    let drawn = info_pictures(&hardware);
    assert!(drawn.iter().any(
        |(target, value)| *target == ArtworkTarget::Keypad(1) && value.glyph == Glyph::Speaker
    ));
    let assert_transparent_preview = |target| {
        let pictures = artwork.pictures.borrow();
        let (_, picture) = pictures.iter().find(|(shown, _)| *shown == target).unwrap();
        let texture = picture
            .paintable()
            .unwrap()
            .downcast::<gtk::gdk::Texture>()
            .unwrap();
        let mut pixels = vec![0; texture.width() as usize * texture.height() as usize * 4];
        texture.download(&mut pixels, texture.width() as usize * 4);
        assert_eq!(pixels[3], 0, "{target:?} has an opaque background");
    };
    assert_transparent_preview(ArtworkTarget::Keypad(1));

    // A reading redraws only what shows it, and only when it changes.
    let loud = InfoValue::gauge(Glyph::Speaker, 45, "45%".into());
    artwork.update_info(InfoSource::Volume, loud.clone());
    assert_eq!(
        info_pictures(&hardware),
        [(ArtworkTarget::Keypad(1), loud.clone())]
    );
    artwork.update_info(InfoSource::Volume, loud.clone());
    artwork.update_info(InfoSource::Clock, InfoValue::text(Glyph::None, "12:00"));
    assert!(info_pictures(&hardware).is_empty());

    // A dial shows its information on its touch zone.
    ui.control_picker.set_selected(1);
    assert!(ui.info_toggle.is_visible(), "dials can show information");
    ui.action_picker.set_selected(1);
    ui.text_entry.set_text("true");
    ui.info_toggle.set_active(true);
    ui.info_source.set_selected(position(InfoSource::Cpu));
    while hardware.try_recv().is_ok() {}
    find("Save binding").emit_clicked();
    while requests.try_recv().is_ok() {}
    let zone = info_pictures(&hardware);
    assert!(
        zone.iter()
            .any(|(target, value)| *target == ArtworkTarget::EncoderZone(1)
                && value.glyph == Glyph::Chip),
        "{zone:?}"
    );
    assert_eq!(preview(ArtworkTarget::EncoderZone(1)), Some((200, 100)));
    assert_transparent_preview(ArtworkTarget::EncoderZone(1));
    let ArtworkCommand::ComposeInfo {
        path,
        strip_path,
        value,
        target,
    } = artwork.info_command("Base", ArtworkTarget::EncoderZone(1), InfoSource::Cpu)
    else {
        panic!("expected live information");
    };
    assert!(path.is_none());
    assert!(
        strip_path.is_some(),
        "device composition must retain the strip"
    );
    let jpeg = sdeck_plus::image_output::prepare_info_jpeg(
        target,
        &value,
        path.as_deref(),
        strip_path.as_deref(),
    )
    .unwrap();
    let encoded = gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
    let bytes = encoded.read_pixel_bytes();
    assert!(
        bytes[2] > 240 && bytes[0] > 50,
        "strip artwork was replaced by black"
    );
    let cpu = InfoValue::gauge(Glyph::Chip, 30, "30%".into());
    artwork.update_info(InfoSource::Cpu, cpu.clone());
    assert_eq!(
        info_pictures(&hardware),
        [(ArtworkTarget::EncoderZone(1), cpu)]
    );

    // Weather asks for a location, saved with the binding.
    ui.info_source.set_selected(position(InfoSource::Weather));
    assert!(ui.info_location_row.is_visible());
    ui.info_location.set_text("Madrid");
    find("Save binding").emit_clicked();
    assert_eq!(
        sdeck_plus::info::load_location(snapshot.root()).as_deref(),
        Some("Madrid")
    );
    ui.load_selected_binding(&layers);
    assert!(ui.info_toggle.is_active());
    assert_eq!(ui.info_location.text(), "Madrid");

    // Removing the dial binding restores the zone's own image.
    while requests.try_recv().is_ok() {}
    while hardware.try_recv().is_ok() {}
    find("Remove binding").emit_clicked();
    assert_eq!(layers.borrow().zone_info_source("Base", 1), None);
    assert_eq!(preview(ArtworkTarget::EncoderZone(1)), None);
    assert!(info_pictures(&hardware)
        .iter()
        .all(|(target, _)| *target != ArtworkTarget::EncoderZone(1)));

    // A tap on one of the four touch sections shows information on that section.
    ui.select_control(ControlKind::Touch, Some(3), true)
        .unwrap();
    ui.load_selected_binding(&layers);
    assert!(
        ui.info_toggle.is_visible(),
        "a touch section can show information"
    );
    ui.action_picker.set_selected(1);
    ui.text_entry.set_text("true");
    ui.info_toggle.set_active(true);
    ui.info_source.set_selected(position(InfoSource::Clock));
    while hardware.try_recv().is_ok() {}
    find("Save binding").emit_clicked();
    while requests.try_recv().is_ok() {}
    let section = info_pictures(&hardware);
    assert!(
        section
            .iter()
            .any(|(target, value)| *target == ArtworkTarget::EncoderZone(3)
                && value.glyph == Glyph::None),
        "{section:?}"
    );
    assert_eq!(
        layers.borrow().zone_info_source("Base", 3),
        Some(InfoSource::Clock)
    );
    assert!(layers.borrow().layers()["Base"]
        .bindings
        .contains_key(&Trigger::ZoneTap(3)));
    assert_eq!(preview(ArtworkTarget::EncoderZone(3)), Some((200, 100)));
    // Four sections hold four bindings at once.
    for zone in [1, 2, 4] {
        ui.select_control(ControlKind::Touch, Some(zone), true)
            .unwrap();
        ui.load_selected_binding(&layers);
        ui.action_picker.set_selected(1);
        ui.text_entry.set_text(&format!("zone {zone}"));
        ui.info_toggle.set_active(true);
        ui.info_source.set_selected(position(InfoSource::Volume));
        find("Save binding").emit_clicked();
        while requests.try_recv().is_ok() {}
    }
    for zone in 1..=4_u8 {
        assert!(layers.borrow().layers()["Base"]
            .bindings
            .contains_key(&Trigger::ZoneTap(zone)));
        assert_transparent_preview(ArtworkTarget::EncoderZone(zone));
    }
    // The whole strip and strip gestures cannot show information.
    ui.select_control(ControlKind::Touch, None, true).unwrap();
    ui.load_selected_binding(&layers);
    assert!(!ui.info_toggle.is_visible());
    ui.select_control(ControlKind::Touch, Some(1), true)
        .unwrap();
    ui.event_picker.set_selected(2);
    ui.load_selected_binding(&layers);
    assert!(!ui.info_toggle.is_visible());
    ui.select_control(ControlKind::Button, Some(1), true)
        .unwrap();
    ui.load_selected_binding(&layers);
    assert!(ui.info_toggle.is_visible());

    window.destroy();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn now_playing_is_selectable_and_draws_its_three_states_on_keys_and_sections() {
    run_gtk_test(assert_now_playing_is_selectable_and_draws_its_three_states);
}

fn assert_now_playing_is_selectable_and_draws_its_three_states() {
    if !init_gtk_for_test() {
        assert!(
            std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "GTK initialization failed despite an advertised display"
        );
        return;
    }
    let (root, store, images, _) = artwork_fixture("now-playing");
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
        info: Rc::default(),
    };
    let (commands, requests) = mpsc::channel();
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(700)
        .build();
    install_style(&window);
    let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(1000, 700);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);
    let preview = |target: ArtworkTarget| {
        let pictures = artwork.pictures.borrow();
        let (_, picture) = pictures.iter().find(|(shown, _)| *shown == target).unwrap();
        picture
            .paintable()
            .filter(|_| picture.get_visible())
            .map(|paintable| (paintable.intrinsic_width(), paintable.intrinsic_height()))
    };
    let mut buttons = Vec::new();
    collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
    let save = buttons
        .iter()
        .find(|button| button_caption(button).as_deref() == Some("Save binding"))
        .unwrap()
        .clone();

    // The settings list offers "Now playing" by its label.
    let ui = &widgets.binding_ui;
    let model = ui
        .info_source
        .model()
        .unwrap()
        .downcast::<gtk::StringList>()
        .unwrap();
    let offered = (0..model.n_items())
        .position(|index| model.string(index).as_deref() == Some("Now playing"))
        .expect("Now playing is offered");
    assert_eq!(
        InfoSource::ALL[offered],
        InfoSource::NowPlaying,
        "labels follow the source order"
    );

    let choose = |select: &dyn Fn()| {
        select();
        ui.action_picker.set_selected(1);
        ui.text_entry.set_text("true");
        ui.info_toggle.set_active(true);
        ui.info_source.set_selected(u32::try_from(offered).unwrap());
        while hardware.try_recv().is_ok() {}
        save.emit_clicked();
    };
    choose(&|| key_button(&widgets.key_labels[1]).emit_clicked());
    let LayerCommand::SaveBinding(snapshot, _) = requests.try_recv().unwrap() else {
        panic!("save must persist the binding");
    };
    assert_eq!(
        snapshot.info_source("Base", &Trigger::KeyPress(2)),
        Some(InfoSource::NowPlaying)
    );
    while requests.try_recv().is_ok() {}
    choose(&|| {
        ui.select_control(ControlKind::Touch, Some(2), true)
            .unwrap();
        ui.load_selected_binding(&layers);
    });
    while requests.try_recv().is_ok() {}
    assert_eq!(
        layers.borrow().zone_info_source("Base", 2),
        Some(InfoSource::NowPlaying)
    );
    assert_eq!(preview(ArtworkTarget::Keypad(2)), Some((120, 120)));
    assert_eq!(preview(ArtworkTarget::EncoderZone(2)), Some((200, 100)));

    // Playing, paused and nothing playing each redraw the key and the section.
    let playing = InfoValue {
        detail: "Miles Davis".into(),
        ..InfoValue::text(Glyph::Play, "So What")
    };
    let paused = InfoValue {
        glyph: Glyph::Pause,
        inactive: true,
        ..playing.clone()
    };
    let idle = sdeck_plus::info::format_now_playing(None);
    for value in [playing, paused, idle] {
        while hardware.try_recv().is_ok() {}
        artwork.update_info(InfoSource::NowPlaying, value.clone());
        let mut drawn = info_pictures(&hardware);
        drawn.sort_by_key(|(target, _)| format!("{target:?}"));
        assert_eq!(
            drawn,
            [
                (ArtworkTarget::EncoderZone(2), value.clone()),
                (ArtworkTarget::Keypad(2), value.clone()),
            ]
        );
        assert_eq!(preview(ArtworkTarget::Keypad(2)), Some((120, 120)));
        assert_eq!(preview(ArtworkTarget::EncoderZone(2)), Some((200, 100)));
    }
    assert!(
        artwork.info.borrow().marquee.timer.is_none(),
        "short titles never start the preview timer"
    );

    // A long playing title scrolls in the preview, at a phase from the clock.
    let pixels = |target: ArtworkTarget| {
        let pictures = artwork.pictures.borrow();
        let (_, picture) = pictures.iter().find(|(shown, _)| *shown == target).unwrap();
        let texture = picture
            .paintable()
            .unwrap()
            .downcast::<gtk::gdk::Texture>()
            .unwrap();
        let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
        texture.download(&mut bytes, texture.width() as usize * 4);
        bytes
    };
    let long = InfoValue {
        detail: "Queen".into(),
        ..InfoValue::text(Glyph::Play, "Bohemian Rhapsody (Remastered 2011)")
    };
    while hardware.try_recv().is_ok() {}
    artwork.update_info(InfoSource::NowPlaying, long.clone());
    assert_eq!(
        info_pictures(&hardware).len(),
        2,
        "the device gets the reading once; its own worker scrolls it"
    );
    assert_eq!(artwork.info.borrow().marquee.running.len(), 2);
    assert!(artwork.info.borrow().marquee.timer.is_some());
    let (key, section) = (
        pixels(ArtworkTarget::Keypad(2)),
        pixels(ArtworkTarget::EncoderZone(2)),
    );
    let later = std::time::Instant::now() + Duration::from_secs(3);
    assert!(matches!(
        artwork.tick_preview_marquee(later),
        glib::ControlFlow::Continue
    ));
    assert_ne!(key, pixels(ArtworkTarget::Keypad(2)), "the key title moved");
    assert_ne!(
        section,
        pixels(ArtworkTarget::EncoderZone(2)),
        "the section title moved"
    );
    let moved = pixels(ArtworkTarget::Keypad(2));
    artwork.tick_preview_marquee(later);
    assert_eq!(
        moved,
        pixels(ArtworkTarget::Keypad(2)),
        "no redraw in place"
    );

    // Paused: static again, and the real timer removes itself.
    artwork.update_info(
        InfoSource::NowPlaying,
        InfoValue {
            glyph: Glyph::Pause,
            inactive: true,
            ..long.clone()
        },
    );
    assert!(artwork.info.borrow().marquee.running.is_empty());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while artwork.info.borrow().marquee.timer.is_some() && std::time::Instant::now() < deadline {
        glib::MainContext::default().iteration(true);
    }
    assert!(artwork.info.borrow().marquee.timer.is_none());

    // Scrolling again, then the preview leaves its window: the timer stops.
    artwork.update_info(InfoSource::NowPlaying, long.clone());
    assert!(artwork.info.borrow().marquee.timer.is_some());
    window.set_child(None::<&gtk::Widget>);
    assert!(matches!(
        artwork.tick_preview_marquee(later),
        glib::ControlFlow::Break
    ));
    assert!(artwork.info.borrow().marquee.running.is_empty());
    window.destroy();
    std::fs::remove_dir_all(root).unwrap();
}
