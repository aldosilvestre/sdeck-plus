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
    let _guard = gtk_test_lock();
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
    find("Remove").emit_clicked();
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
