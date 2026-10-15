use super::*;

pub(super) fn assert_reference_dialog() {
    let (root, layers, store, _) = artwork_fixture("shortcut-reference");
    let layers = Rc::new(RefCell::new(layers));
    let (output, _output) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(store))),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output,
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(Default::default())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
    };
    let window = ApplicationWindow::builder()
        .default_width(1000)
        .default_height(700)
        .build();
    install_style(&window);
    let (sender, commands) = mpsc::channel();
    let widgets = build_widgets(window.clone(), layers.clone(), artwork, sender);
    let stage = gtk::Fixed::new();
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    let ui = &widgets.binding_ui;
    ui.action_picker.set_selected(3);
    ui.update_action_visibility();
    ui.text_entry.set_text("CTRL + mute_mic");
    ui.shift_mod.set_active(true);
    commands.try_iter().for_each(drop);
    let original = ui.binding_from_widgets(&layers).unwrap();
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
        for (width, height) in [(640, 480), (1000, 700), (1280, 800)] {
            widgets.root.set_size_request(width, height);
            widgets.root.allocate(width, height, -1, None);
            let save = |name: &str, widget: &gtk::Widget| {
                let texture = rendered_texture(&window, widget);
                if let Some(directory) = std::env::var_os("STREAMDECK_REFERENCE_QA_DIR") {
                    let directory = std::path::PathBuf::from(directory);
                    std::fs::create_dir_all(&directory).unwrap();
                    texture
                        .save_to_png(
                            directory.join(format!("{variant}-{width}x{height}-{name}.png")),
                        )
                        .unwrap();
                }
            };
            let adjustment = widgets.root.vadjustment();
            adjustment.set_value(0.0);
            save("shortcut-top", widgets.root.upcast_ref());
            adjustment.set_value((adjustment.upper() - adjustment.page_size()).max(0.0));
            save("shortcut-end", widgets.root.upcast_ref());
            // Given: an unsaved media chord and modifiers on the production editor.
            assert!(ui.action_guidance.uses_markup());
            // When: the actual GTK link signal opens the reference.
            assert!(ui
                .action_guidance
                .emit_by_name::<bool>("activate-link", &[&"shortcut-reference"]));
            let dialog = current_dialog();
            let scroller = dialog
                .content_area()
                .last_child()
                .unwrap()
                .downcast::<gtk::ScrolledWindow>()
                .unwrap();
            let list = scroller.child().unwrap();
            let list = if let Ok(viewport) = list.clone().downcast::<gtk::Viewport>() {
                viewport.child().unwrap()
            } else {
                list
            };
            let mut label_values = Vec::new();
            collect_label_values(&list, &mut label_values);
            // Then: every displayed token comes from the live parser catalogue.
            for (alias, canonical) in sdeck_plus::keyboard::supported_key_aliases() {
                assert!(label_values.iter().any(|value| value == canonical));
                assert!(
                    label_values
                        .iter()
                        .flat_map(|value| value.split(", "))
                        .any(|value| value == alias),
                    "{alias}"
                );
            }
            save("reference-top", dialog.upcast_ref());
            assert!(dialog.width() <= width && dialog.height() <= height);
            let scroll = scroller.vadjustment();
            let step = scroll.page_size();
            assert!(step > 0.0);
            let end = (scroll.upper() - step).max(0.0);
            let mut offset = step;
            let mut page = 1;
            while offset < end {
                scroll.set_value(offset);
                save(&format!("reference-page-{page}"), dialog.upcast_ref());
                offset += step;
                page += 1;
            }
            scroll.set_value((scroll.upper() - scroll.page_size()).max(0.0));
            save("reference-end", dialog.upcast_ref());
            dialog.response(ResponseType::Close);
            assert!(!dialog.is_visible());
            assert_eq!(ui.binding_from_widgets(&layers).unwrap(), original);
            assert!(
                commands.try_recv().is_err(),
                "reference must not save or execute"
            );
            ui.action_picker.set_selected(1);
            ui.update_action_visibility();
            assert!(!ui.action_guidance.uses_markup());
            ui.action_picker.set_selected(3);
            ui.update_action_visibility();
        }
    }
    settings.set_gtk_theme_name(theme.as_deref());
    settings.set_gtk_application_prefer_dark_theme(dark);
    window.close();
    window.set_child(None::<&gtk::Widget>);
    drop(widgets);
    std::fs::remove_dir_all(root).unwrap();
    eprintln!("GTK shortcut alias reference verified: catalogue, link, close, unsaved draft and no dispatch");
}

fn collect_label_values(widget: &gtk::Widget, values: &mut Vec<String>) {
    if let Some(label) = widget.downcast_ref::<Label>() {
        values.push(label.text().to_string());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        collect_label_values(&widget, values);
        child = widget.next_sibling();
    }
}
