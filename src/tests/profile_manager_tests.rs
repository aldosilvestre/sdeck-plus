use super::*;

pub(super) fn assert_compact_profile_manager() {
    let (root, mut store, artwork_store, _) = artwork_fixture("profile-manager");
    if !store.layers().contains_key("Studio") {
        store.create("Studio").unwrap();
    }
    let layers = Rc::new(RefCell::new(store));
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
    let (sender, commands) = mpsc::channel();
    let widgets = build_widgets(window.clone(), layers.clone(), artwork, sender);
    let stage = gtk::Fixed::new();
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    let panel = widgets.management_menu.popover().unwrap();
    panel.set_autohide(false);
    let content = panel.child().unwrap();
    let scroller = content
        .last_child()
        .unwrap()
        .downcast::<gtk::ScrolledWindow>()
        .unwrap();
    let body = scroller
        .child()
        .unwrap()
        .downcast::<gtk::Viewport>()
        .unwrap()
        .child()
        .unwrap();
    let tasks = body.last_child().unwrap().downcast::<gtk::Stack>().unwrap();
    let manager = widgets.feature_runtime.management();
    let picker = manager
        .first_child()
        .unwrap()
        .next_sibling()
        .unwrap()
        .last_child()
        .unwrap()
        .downcast::<gtk::DropDown>()
        .unwrap();
    let mut buttons = Vec::new();
    collect_layout_buttons(panel.upcast_ref(), &mut buttons);
    let create = buttons
        .iter()
        .find(|b| button_caption(b).as_deref() == Some("Create profile"))
        .unwrap();
    let settings = gtk::Settings::default().unwrap();
    let original_theme = settings.gtk_theme_name();
    let original_dark = settings.is_gtk_application_prefer_dark_theme();
    for (theme, dark) in [("light", false), ("dark", true)] {
        settings.set_gtk_theme_name(Some(if dark { "Adwaita-dark" } else { "Adwaita" }));
        settings.set_gtk_application_prefer_dark_theme(dark);
        for (width, height) in [(640, 480), (1000, 700)] {
            widgets.root.set_size_request(width, height);
            widgets.root.allocate(width, height, -1, None);
            wait_for_frame(&window);
            widgets.management_menu.popup();
            let save = |name: &str, widget: &gtk::Widget| {
                let texture = rendered_texture(&window, widget);
                if let Some(dir) = std::env::var_os("STREAMDECK_PROFILES_QA_DIR") {
                    let dir = std::path::PathBuf::from(dir);
                    std::fs::create_dir_all(&dir).unwrap();
                    texture
                        .save_to_png(dir.join(format!("{theme}-{width}x{height}-{name}.png")))
                        .unwrap();
                }
            };
            for (state, profile) in [("base", 0), ("manual", 1)] {
                tasks.set_visible_child_name("edit");
                picker.set_selected(profile);
                save(state, panel.upcast_ref());
                assert!(
                    scroller.vadjustment().upper() <= scroller.vadjustment().page_size() + 1.0,
                    "edit task must fit without scrolling at {width}x{height}"
                );
            }
            // Given: an unsaved association, and two independent manager tasks.
            let mut entries = Vec::new();
            collect_entries(manager.upcast_ref(), &mut entries);
            let app = &entries[0];
            app.set_text("unsaved-app");
            // When: switch to creation and back.
            tasks.set_visible_child_name("new");
            let mut new_entries = Vec::new();
            collect_entries(&tasks.visible_child().unwrap(), &mut new_entries);
            new_entries[0].set_text("");
            save("new", panel.upcast_ref());
            let new_help = tasks
                .visible_child()
                .unwrap()
                .first_child()
                .unwrap()
                .first_child()
                .unwrap()
                .last_child()
                .unwrap()
                .downcast::<gtk::MenuButton>()
                .unwrap();
            assert!(new_help.popover().is_some(), "help opens on hover");
            new_help.popup();
            save("new-help", new_help.popover().unwrap().upcast_ref());
            new_help.popdown();
            assert!(scroller.vadjustment().upper() <= scroller.vadjustment().page_size() + 1.0);
            let creation = tasks.visible_child().unwrap();
            let mut creation_entries = Vec::new();
            collect_entries(&creation, &mut creation_entries);
            creation_entries[0].set_text("Base");
            create.emit_clicked();
            save("invalid-new", panel.upcast_ref());
            assert_eq!(tasks.visible_child_name().as_deref(), Some("new"));
            tasks.set_visible_child_name("edit");
            // Then: the association draft is unchanged.
            assert_eq!(app.text(), "unsaved-app");
            let row = manager
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .next_sibling()
                .unwrap();
            let help = row
                .first_child()
                .unwrap()
                .last_child()
                .unwrap()
                .downcast::<gtk::MenuButton>()
                .unwrap();
            assert!(help.popover().is_some(), "help opens on hover");
            help.popup();
            save("help", help.popover().unwrap().upcast_ref());
            help.popdown();
            app.set_text("");
            widgets
                .feature_runtime
                .report_profile_save("Profile saved.", false);
            save("saved", panel.upcast_ref());
            widgets
                .feature_runtime
                .report_profile_save("Could not save profile: storage is read-only.", true);
            save("save-error", panel.upcast_ref());
            widgets.management_menu.popdown();
        }
    }
    // Given: a valid name in the real creation form.
    tasks.set_visible_child_name("new");
    let mut names = Vec::new();
    collect_entries(&tasks.visible_child().unwrap(), &mut names);
    names[0].set_text("Created from manager");
    // When: create using its native button.
    create.emit_clicked();
    // Then: creation returns to editing that profile and enqueues a real save.
    assert_eq!(tasks.visible_child_name().as_deref(), Some("edit"));
    assert_eq!(layers.borrow().active(), "Created from manager");
    assert!(commands
        .try_iter()
        .any(|command| matches!(command, LayerCommand::SaveProfile(_, _))));
    let delete = buttons
        .iter()
        .find(|button| button_caption(button).as_deref() == Some("Delete profile"))
        .unwrap();
    delete.emit_clicked();
    current_dialog().response(ResponseType::Cancel);
    assert!(layers
        .borrow()
        .layers()
        .contains_key("Created from manager"));
    delete.emit_clicked();
    current_dialog().response(ResponseType::Accept);
    assert!(!layers
        .borrow()
        .layers()
        .contains_key("Created from manager"));
    settings.set_gtk_theme_name(original_theme.as_deref());
    settings.set_gtk_application_prefer_dark_theme(original_dark);
    window.destroy();
    std::fs::remove_dir_all(root).unwrap();
    eprintln!("GTK compact profile manager verified: no scroll, task switching, contextual help, validation and creation");
}
