#[test]
fn button_text_alignment_icons_update_preview_and_save() {
    run_gtk_test(assert_button_text_alignment_icons_update_preview_and_save);
}

fn assert_button_text_alignment_icons_update_preview_and_save() {
    if !init_gtk_for_test() {
        assert!(
            std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "GTK initialization failed despite an advertised display"
        );
        return;
    }
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    assert_button_text_editor();
}

fn assert_button_text_editor() {
    use sdeck_plus::button_text::{
        ButtonText, HorizontalAlignment, TextOrientation, VerticalAlignment,
    };
    let (root, mut layers, mut store, source) = artwork_fixture("button-text-editor");
    layers.select("Base");
    let path = store
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &source)
        .unwrap();
    let layers = Rc::new(RefCell::new(layers));
    let (output, commands) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(store))),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output,
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(Default::default())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
        info: Rc::default(),
    };
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(700)
        .build();
    install_style(&window);
    let (requests, _requests) = mpsc::channel();
    let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), requests);
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(1000, 700);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    let editor = &widgets._text_editor;
    // Keep shared-desktop focus changes from dismissing this test's popup.
    let autohide = editor.panel.is_autohide();
    editor.panel.set_autohide(false);
    commands.try_iter().for_each(drop);
    assert!(!editor.panel.is_visible());
    wait_for_frame(&widgets.root);
    rendered_text_popup(&editor.panel, || editor.open.popup());
    assert!(editor.panel.is_mapped(), "new text popup must be mapped");
    assert!(editor.panel.is_visible());
    for button in editor.orientation.iter().chain(editor.position.iter()) {
        assert!(
            button.label().is_none(),
            "orientation and position controls must be icon-only"
        );
        assert!(button.child().is_some());
        assert!(button
            .tooltip_text()
            .is_some_and(|tooltip| !tooltip.is_empty()));
    }

    // Given: a background image and a draft with every text control configured.
    editor.input.buffer().set_text("Play\nNow");
    let typed = editor.value().unwrap();
    assert_live_text_preview(editor, &artwork, Some(&path), &typed);
    for (horizontal, expected_horizontal) in [
        HorizontalAlignment::Left,
        HorizontalAlignment::Center,
        HorizontalAlignment::Right,
    ]
    .into_iter()
    .enumerate()
    {
        for (vertical, expected_vertical) in [
            VerticalAlignment::Top,
            VerticalAlignment::Center,
            VerticalAlignment::Bottom,
        ]
        .into_iter()
        .enumerate()
        {
            editor.position[vertical * 3 + horizontal].emit_clicked();
            let value = editor.value().unwrap();
            assert_eq!(value.horizontal_alignment, expected_horizontal);
            assert_eq!(value.vertical_alignment, expected_vertical);
            assert_eq!(
                editor
                    .position
                    .iter()
                    .filter(|button| button.is_active())
                    .count(),
                1
            );
            assert_live_text_preview(editor, &artwork, Some(&path), &value);
        }
    }
    editor.size.set_value(30.0);
    editor.orientation[1].set_active(true);
    editor.position[8].set_active(true);
    assert_font_dialog_keeps_draft(editor);
    let mut font = gtk::pango::FontDescription::new();
    font.set_family("Serif");
    editor.font.set_font_desc(&font);
    assert!(
        commands.try_recv().is_err(),
        "a draft must not reach the device"
    );
    assert!(artwork
        .store
        .borrow()
        .as_ref()
        .unwrap()
        .button_text_in_layer("Base", ArtworkTarget::Keypad(1))
        .is_none());
    let value = editor.value().unwrap();
    assert_eq!(value.font_family, "Serif");
    assert_eq!(value.font_size, 30);
    assert_eq!(value.orientation, TextOrientation::Vertical);
    assert_eq!(value.horizontal_alignment, HorizontalAlignment::Right);
    assert_eq!(value.vertical_alignment, VerticalAlignment::Bottom);
    assert_live_text_preview(editor, &artwork, Some(&path), &value);

    // When: the user explicitly saves the overlay.
    editor.save.emit_clicked();
    assert!(!editor.panel.is_visible());

    // Then: persistence, device commands and physical preview use the same value.
    let reloaded = ArtworkStore::open_at_for_layer(root.join("artwork"), Some("Base")).unwrap();
    assert_eq!(
        reloaded.button_text_in_layer("Base", ArtworkTarget::Keypad(1)),
        Some(&value)
    );
    assert!(matches!(commands.try_recv().unwrap(),
        ArtworkCommand::Replace(snapshot) if matches!(&snapshot[0],
            ArtworkCommand::ComposeButton { key: 1, path: Some(saved), text }
                if saved == &path && text == &value)));
    let picture = artwork
        .pictures
        .borrow()
        .iter()
        .find(|(target, _)| *target == ArtworkTarget::Keypad(1))
        .unwrap()
        .1
        .clone();
    let actual = picture
        .paintable()
        .unwrap()
        .downcast::<gtk::gdk::Texture>()
        .unwrap();
    let expected = gtk::gdk::Texture::for_pixbuf(
        &sdeck_plus::image_output::compose_button_image(Some(&path), &value).unwrap(),
    );
    let mut actual_pixels = vec![0; 120 * 120 * 4];
    let mut expected_pixels = actual_pixels.clone();
    actual.download(&mut actual_pixels, 120 * 4);
    expected.download(&mut expected_pixels, 120 * 4);
    assert_eq!(actual_pixels, expected_pixels);

    // Reopening edits the saved values in the same popup; closing drops drafts.
    rendered_text_popup(&editor.panel, || editor.open.popup());
    assert_eq!(editor.value().unwrap(), value);
    editor.input.buffer().set_text("Discard this draft");
    editor.panel.popdown();
    assert_eq!(editor.value().unwrap(), value);
    assert!(commands.try_recv().is_err());
    rendered_text_popup(&editor.panel, || editor.open.popup());
    editor.input.buffer().set_text("Resume\nNow");
    let value = editor.value().unwrap();
    editor.save.emit_clicked();
    assert!(!editor.panel.is_visible());
    assert_eq!(
        ArtworkStore::open_at_for_layer(root.join("artwork"), Some("Base"))
            .unwrap()
            .button_text_in_layer("Base", ArtworkTarget::Keypad(1)),
        Some(&value)
    );

    widgets.target_picker.set_selected(1);
    assert_eq!(editor.value().unwrap(), ButtonText::default());
    editor.input.buffer().set_text("Second");
    editor.save.emit_clicked();
    widgets.target_picker.set_selected(0);
    assert_eq!(editor.value().unwrap(), value);
    layers.borrow_mut().select("Studio");
    artwork.refresh(false);
    assert_eq!(editor.value().unwrap(), ButtonText::default());
    editor.input.buffer().set_text("Studio");
    editor.save.emit_clicked();
    layers.borrow_mut().select("Base");
    artwork.refresh(false);
    assert_eq!(editor.value().unwrap(), value);
    widgets.target_picker.set_selected(8);
    assert!(!editor.root.is_visible(), "text controls are keypad-only");
    widgets.target_picker.set_selected(0);
    editor.remove.emit_clicked();
    assert!(artwork
        .store
        .borrow()
        .as_ref()
        .unwrap()
        .button_text_in_layer("Base", ArtworkTarget::Keypad(1))
        .is_none());
    assert_eq!(
        artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Base", ArtworkTarget::Keypad(1)),
        Some(path)
    );
    editor.input.buffer().set_text("Play\nNow");
    editor.save.emit_clicked();
    widgets.artwork_section.set_expanded(false);

    if let Some(directory) = std::env::var_os("STREAMDECK_TEXT_QA_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let settings = gtk::Settings::default().unwrap();
        let original_theme = settings.gtk_theme_name();
        let original_dark = settings.is_gtk_application_prefer_dark_theme();
        for (theme, dark) in [("light", false), ("dark", true)] {
            settings.set_gtk_theme_name(Some(if dark { "Adwaita-dark" } else { "Adwaita" }));
            settings.set_gtk_application_prefer_dark_theme(dark);
            for (width, height) in [(640, 480), (1000, 700), (1280, 800)] {
                widgets.root.set_size_request(width, height);
                widgets.root.allocate(width, height, -1, None);
                wait_for_frame(&widgets.root);
                let scroll = widgets.root.vadjustment();
                scroll.set_value(0.0);
                let texture = rendered_texture(&window, &widgets.root);
                texture
                    .save_to_png(
                        directory.join(format!("text-compact-{theme}-{width}x{height}.png")),
                    )
                    .unwrap();
                if width >= 1000 {
                    assert!(
                        scroll.upper() <= scroll.page_size(),
                        "compact editor scrolls"
                    );
                }
                let bounds = editor.open.compute_bounds(&widgets.root).unwrap();
                scroll.set_value(
                    (scroll.value() + f64::from(bounds.y()) - f64::from(height) / 2.0)
                        .clamp(0.0, (scroll.upper() - scroll.page_size()).max(0.0)),
                );
                wait_for_frame(&widgets.root);
                for (name, orientation) in [("horizontal", 0), ("vertical", 1)] {
                    let texture = rendered_text_popup(&editor.panel, || {
                        editor.open.popup();
                        editor.orientation[orientation].set_active(true);
                    });
                    assert!(editor.panel.is_mapped(), "popup must be mapped at {width}x{height}; anchor={:?}, window={}x{}, panel={}x{}",
                        editor.open.compute_bounds(&window), window.width(), window.height(),
                        editor.panel.width(), editor.panel.height());
                    {
                        assert!(texture.width() <= width && texture.height() <= height);
                        assert_text_popup_fits(editor);
                        texture
                            .save_to_png(
                                directory.join(format!("text-{name}-{theme}-{width}x{height}.png")),
                            )
                            .unwrap();
                    }
                    editor.panel.popdown();
                }
                if width == 1000 {
                    for (index, name) in ["start", "center", "end"].into_iter().enumerate() {
                        let texture = rendered_text_popup(&editor.panel, || {
                            editor.open.popup();
                            editor.position[index * 4].set_active(true);
                            assert!(editor.position[index * 4].grab_focus());
                            window.set_focus_visible(true);
                        });
                        assert_text_popup_fits(editor);
                        texture
                            .save_to_png(
                                directory.join(format!("text-alignment-{name}-{theme}.png")),
                            )
                            .unwrap();
                        editor.panel.popdown();
                    }
                    window.set_focus_visible(false);
                    let texture = rendered_text_popup(&editor.panel, || {
                        editor.open.popup();
                        for button in &editor.position {
                            button.set_sensitive(false);
                        }
                    });
                    texture
                        .save_to_png(directory.join(format!("text-alignment-disabled-{theme}.png")))
                        .unwrap();
                    for button in &editor.position {
                        button.set_sensitive(true);
                    }
                    editor.panel.popdown();
                }
            }
        }
        settings.set_gtk_theme_name(original_theme.as_deref());
        settings.set_gtk_application_prefer_dark_theme(original_dark);
    }
    // Given: a different key's saved artwork and text, and the real startup owner transfer.
    let second = ButtonText {
        text: "Second".into(),
        font_size: 18,
        ..ButtonText::default()
    };
    let second_path = artwork
        .store
        .borrow_mut()
        .as_mut()
        .unwrap()
        .assign_in_layer("Base", ArtworkTarget::Keypad(2), &source)
        .unwrap();
    artwork
        .store
        .borrow_mut()
        .as_mut()
        .unwrap()
        .set_button_text_in_layer("Base", ArtworkTarget::Keypad(2), second.clone())
        .unwrap();
    let editor_owner = Rc::downgrade(editor);
    let input = editor.input.clone();
    let open = editor.open.clone();
    let panel = editor.panel.clone();
    let save = editor.save.clone();
    let key = key_button(&widgets.key_labels[1]);
    let (events, receiver) = mpsc::channel();
    let (requests, _requests) = mpsc::channel();
    let polling = start_event_updates(
        widgets,
        receiver,
        layers.clone(),
        artwork.clone(),
        requests,
        events,
    );
    let retained = editor_owner.upgrade().is_some();
    if !retained {
        polling.remove();
        panic!("production startup dropped the text editor: Add text, preview and Save callbacks have no owner");
    }

    // When: selection and Add text are activated after production startup.
    key.emit_clicked();
    rendered_text_popup(&panel, || assert!(open.activate()));
    // Then: the selected key's saved state loads, typing composes, and Save persists.
    assert_eq!(
        input.buffer().text(
            &input.buffer().start_iter(),
            &input.buffer().end_iter(),
            true
        ),
        "Second"
    );
    assert_eq!(editor_owner.upgrade().unwrap().value().unwrap(), second);
    assert_live_text_preview(
        &editor_owner.upgrade().unwrap(),
        &artwork,
        Some(&second_path),
        &second,
    );
    input.buffer().set_text("Live Second");
    let draft = ButtonText {
        text: "Live Second".into(),
        ..second
    };
    assert_live_text_preview(
        &editor_owner.upgrade().unwrap(),
        &artwork,
        Some(&second_path),
        &draft,
    );
    save.emit_clicked();
    assert_eq!(
        artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .button_text_in_layer("Base", ArtworkTarget::Keypad(2)),
        Some(&draft)
    );
    panel.set_autohide(autohide);
    panel.set_autohide(autohide);
    polling.remove();
    assert!(
        editor_owner.upgrade().is_none(),
        "stopping updates must release the editor owner"
    );
    window.close();
    window.set_child(None::<&gtk::Widget>);
    std::fs::remove_dir_all(root).unwrap();
    eprintln!("GTK button text: shared add/edit popup, explicit save, draft cancellation, composed preview, key/profile isolation and removal verified");
    if std::env::var_os("STREAMDECK_TEXT_HID_QA").is_some() {
        assert_saved_text_on_streamdeck();
    }
}

fn assert_font_dialog_keeps_draft(editor: &button_text_editor::ButtonTextEditor) {
    let surface = editor
        .panel
        .surface()
        .unwrap()
        .downcast::<gtk::gdk::Popup>()
        .unwrap();
    assert!(
        !surface.is_autohide(),
        "the text editor must not grab input and dismiss when its native font chooser takes focus"
    );
    let initial = editor.value().unwrap();
    let button = editor
        .font
        .first_child()
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    for response in [gtk::ResponseType::Cancel, gtk::ResponseType::Ok] {
        button.emit_clicked();
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::FontChooserDialog>().ok())
            .find(|dialog| dialog.is_visible())
            .expect("font button must open its native chooser");
        wait_for_frame(&dialog);
        assert_eq!(
            editor.value().unwrap(),
            initial,
            "opening the font chooser discarded the text draft"
        );
        let mut font = gtk::pango::FontDescription::new();
        font.set_family("Serif");
        font.set_size(12 * gtk::pango::SCALE);
        select_dialog_font(&dialog, &font);
        let mut expected = initial.clone();
        if response == gtk::ResponseType::Ok {
            expected.font_family = dialog.font_desc().unwrap().family().unwrap().to_string();
        }
        dialog.response(response);
        assert!(
            editor.panel.is_visible(),
            "font choice must leave the text popup open"
        );
        rendered_text_popup(&editor.panel, || {});
        assert_eq!(
            editor.value().unwrap(),
            expected,
            "font response discarded the text draft"
        );
    }
}

fn select_dialog_font(dialog: &gtk::FontChooserDialog, font: &gtk::pango::FontDescription) {
    let family = font.family().unwrap();
    let selected = Rc::new(Cell::new(
        dialog.font_desc().and_then(|font| font.family()).as_ref() == Some(&family),
    ));
    let main_loop = glib::MainLoop::new(None, false);
    let ready = selected.clone();
    let done = main_loop.clone();
    let signal = dialog.connect_font_desc_notify(move |dialog| {
        ready.set(dialog.font_desc().and_then(|font| font.family()).as_ref() == Some(&family));
        if ready.get() {
            done.quit();
        }
    });
    let timed_out = Rc::new(Cell::new(false));
    let expired = timed_out.clone();
    let done = main_loop.clone();
    let deadline = glib::timeout_add_local_once(Duration::from_secs(5), move || {
        expired.set(true);
        done.quit();
    });
    dialog.set_font_desc(font);
    if !selected.get() {
        main_loop.run();
    }
    dialog.disconnect(signal);
    if !timed_out.get() {
        deadline.remove();
    }
    assert!(
        selected.get() && !timed_out.get(),
        "native font selection did not complete"
    );
}

fn assert_saved_text_on_streamdeck() {
    use sdeck_plus::local_artwork::ArtworkTarget;
    let (events, receiver) = mpsc::channel();
    let mut hid = event_api::start_hid_monitor(events);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap()
        {
            MonitorEvent::Connection(ConnectionState::Connected) => break,
            MonitorEvent::Connection(ConnectionState::Unavailable(error)) => {
                panic!("HID unavailable: {error}")
            }
            _ => {}
        }
    }
    let (root, mut layers, mut store, source) = artwork_fixture("text-hid-save");
    layers.select("Base");
    let background = store
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), source)
        .unwrap();
    let layers = Rc::new(RefCell::new(layers));
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(store))),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output: hid.artwork_sender(),
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(Default::default())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
        info: Rc::default(),
    };
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(700)
        .build();
    install_style(&window);
    let (requests, _requests) = mpsc::channel();
    let widgets = build_widgets(window.clone(), layers, artwork.clone(), requests);
    window.set_child(Some(&widgets.root));
    window.present();
    wait_for_frame(&widgets.root);
    wait_for_saved_hid_images(&receiver);
    let editor = &widgets._text_editor;

    for (key, orientation) in [(1, 0), (8, 1)] {
        // Given: the production key selection and editor, wired to the real HID owner.
        key_button(&widgets.key_labels[key - 1]).emit_clicked();
        rendered_text_popup(&editor.panel, || editor.open.popup());
        editor.input.buffer().set_text(&format!("Key {key}"));
        editor.size.set_value(18.0);
        editor.orientation[orientation].set_active(true);
        editor.position[8].set_active(true);
        let value = editor.value().unwrap();
        assert_live_text_preview(
            editor,
            &artwork,
            if key == 1 {
                Some(background.as_path())
            } else {
                None
            },
            &value,
        );
        // When: Save text uses the production persistence and output path.
        editor.save.emit_clicked();
        // Then: all nine snapshot results succeed on the physical device.
        wait_for_saved_hid_images(&receiver);
        let target = ArtworkTarget::Keypad(u8::try_from(key).unwrap());
        let reloaded = ArtworkStore::open_at_for_layer(root.join("artwork"), Some("Base")).unwrap();
        assert_eq!(reloaded.button_text_in_layer("Base", target), Some(&value));
        assert_live_text_preview(
            editor,
            &artwork,
            if key == 1 {
                Some(background.as_path())
            } else {
                None
            },
            &value,
        );
        eprintln!(
            "GTK_SAVE_HID_OK: key {key}, {:?}, persisted text and all device image writes accepted",
            value.orientation
        );
    }
    window.close();
    hid.shutdown();
    window.set_child(None::<&gtk::Widget>);
    drop(widgets);
    std::fs::remove_dir_all(root).unwrap();
}

fn wait_for_saved_hid_images(receiver: &mpsc::Receiver<MonitorEvent>) {
    let mut pending = (1..=8)
        .map(ArtworkTarget::Keypad)
        .chain(std::iter::once(ArtworkTarget::TouchStripBackground))
        .collect::<std::collections::HashSet<_>>();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !pending.is_empty() {
        match receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap()
        {
            MonitorEvent::ArtworkResult { target, result } => {
                result.unwrap_or_else(|error| panic!("HID image {target:?} failed: {error}"));
                pending.remove(&target);
            }
            MonitorEvent::Connection(ConnectionState::Disconnected(error)) => {
                panic!("HID disconnected: {error}")
            }
            _ => {}
        }
    }
}

fn assert_live_text_preview(
    editor: &button_text_editor::ButtonTextEditor,
    artwork: &ArtworkUi,
    path: Option<&std::path::Path>,
    value: &sdeck_plus::button_text::ButtonText,
) {
    let expected = gtk::gdk::Texture::for_pixbuf(
        &sdeck_plus::image_output::compose_button_image(path, value).unwrap(),
    );
    let mut expected_pixels = vec![0; 120 * 120 * 4];
    expected.download(&mut expected_pixels, 120 * 4);
    let preview = editor
        .preview
        .paintable()
        .unwrap()
        .downcast::<gtk::gdk::Texture>()
        .unwrap();
    let picture = artwork
        .pictures
        .borrow()
        .iter()
        .find(|(target, _)| *target == artwork.selected_target.get())
        .unwrap()
        .1
        .clone();
    let key_preview = picture
        .paintable()
        .unwrap()
        .downcast::<gtk::gdk::Texture>()
        .unwrap();
    for texture in [preview, key_preview] {
        assert_eq!((texture.width(), texture.height()), (120, 120));
        let mut pixels = vec![0; 120 * 120 * 4];
        texture.download(&mut pixels, 120 * 4);
        assert_eq!(
            pixels, expected_pixels,
            "live preview differs from the device composition"
        );
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[..3].iter().all(|v| *v > 240)),
            "typed text must produce visible white pixels"
        );
    }
}

fn assert_text_popup_fits(editor: &button_text_editor::ButtonTextEditor) {
    let content = editor.panel.child().unwrap();
    // Save follows the form's state and Remove exists only for saved text, so
    // they must be unclipped but not necessarily sensitive or shown.
    for button in [&editor.save, &editor.remove] {
        if !button.is_visible() {
            continue;
        }
        assert!(button.is_mapped());
        let bounds = button.compute_bounds(&content).unwrap();
        assert!(
            bounds.x() >= 0.0
                && bounds.y() >= 0.0
                && bounds.x() + bounds.width() <= content.width() as f32 + 1.0
                && bounds.y() + bounds.height() <= content.height() as f32 + 1.0,
            "{:?} is clipped in the text popup: {bounds:?}",
            button.tooltip_text()
        );
    }
    assert!(editor.save.is_visible(), "Save text is always offered");
    for widget in [
        editor.input.clone().upcast::<gtk::Widget>(),
        editor.font.clone().upcast(),
        editor.size.clone().upcast(),
        editor.preview.clone().upcast(),
        editor.context_line.clone().upcast(),
    ]
    .into_iter()
    .chain(
        editor
            .orientation
            .iter()
            .chain(editor.position.iter())
            .map(|button| button.clone().upcast()),
    ) {
        assert!(widget.is_mapped());
        assert!(
            widget.is_sensitive(),
            "{} is disabled in the text popup: {:?}",
            widget.type_().name(),
            widget.state_flags()
        );
        let bounds = widget.compute_bounds(&content).unwrap();
        assert!(
            bounds.x() >= 0.0
                && bounds.y() >= 0.0
                && bounds.x() + bounds.width() <= content.width() as f32 + 1.0
                && bounds.y() + bounds.height() <= content.height() as f32 + 1.0,
            "{} is clipped in the text popup: {bounds:?}",
            widget.type_().name()
        );
    }
    let mut scrollers = Vec::new();
    collect_text_scrollers(&content, &mut scrollers);
    assert_eq!(scrollers.len(), 1, "only the multiline input may scroll");
    assert_eq!(scrollers[0].child(), Some(editor.input.clone().upcast()));
}

fn collect_text_scrollers(widget: &gtk::Widget, output: &mut Vec<gtk::ScrolledWindow>) {
    if let Some(scroller) = widget.downcast_ref::<gtk::ScrolledWindow>() {
        output.push(scroller.clone());
        return;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        collect_text_scrollers(&current, output);
    }
}

fn rendered_text_popup(panel: &gtk::Popover, update: impl FnOnce()) -> gtk::gdk::Texture {
    // Include GTK's popup material and padding, not only its transparent body.
    let content = panel.child().unwrap().parent().unwrap();
    let paintable = gtk::WidgetPaintable::new(Some(&content));
    let main_loop = glib::MainLoop::new(None, false);
    let captured = Rc::new(RefCell::new(None));
    let result = captured.clone();
    let done = main_loop.clone();
    let popup = panel.clone();
    let widget = content.clone();
    let updated = Rc::new(Cell::new(false));
    let ready = updated.clone();
    let signal = paintable.connect_invalidate_contents(move |paintable| {
        if !ready.get() || !popup.is_mapped() {
            return;
        }
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(
            &snapshot,
            f64::from(widget.width()),
            f64::from(widget.height()),
        );
        if let Some(node) = snapshot.to_node() {
            *result.borrow_mut() = Some(popup.renderer().unwrap().render_texture(
                &node,
                Some(&gtk::graphene::Rect::new(
                    0.0,
                    0.0,
                    widget.width() as f32,
                    widget.height() as f32,
                )),
            ));
            done.quit();
        }
    });
    let timed_out = Rc::new(Cell::new(false));
    let expired = timed_out.clone();
    let done = main_loop.clone();
    let deadline = glib::timeout_add_local_once(Duration::from_secs(5), move || {
        expired.set(true);
        done.quit();
    });
    update();
    updated.set(true);
    content.queue_draw();
    if captured.borrow().is_none() {
        main_loop.run();
    }
    paintable.disconnect(signal);
    if !timed_out.get() {
        deadline.remove();
    }
    assert!(!timed_out.get(), "popup did not publish rendered content: visible={}, mapped={}, content={}x{}, panel={}x{}, parent={:?}",
        panel.is_visible(), panel.is_mapped(), content.width(), content.height(),
        panel.width(), panel.height(), panel.parent().and_then(|parent| parent.root())
            .map(|root| (root.clone().upcast::<gtk::Widget>().width(), root.upcast::<gtk::Widget>().height())));
    let texture = captured
        .borrow_mut()
        .take()
        .expect("rendered popup content");
    texture
}
