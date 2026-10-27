//! Explicitly opted-in native documentation captures; never starts device/action workers.
use super::*;
use sdeck_plus::{button_text::ButtonText, features::FeatureAction, obs::ObsAction};

fn descendants<T: IsA<gtk::Widget> + glib::object::IsClass>(widget: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    if let Ok(value) = widget.clone().downcast::<T>() {
        found.push(value);
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        found.extend(descendants::<T>(&widget));
        child = widget.next_sibling();
    }
    found
}

fn click(widgets: &MonitorWidgets, caption: &str) {
    let mut buttons = Vec::new();
    collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
    buttons
        .iter()
        .find(|button| button_caption(button).as_deref() == Some(caption))
        .unwrap_or_else(|| panic!("missing production button: {caption}"))
        .emit_clicked();
}

fn save(directory: &std::path::Path, widget: &gtk::Widget, name: &str) {
    if !widget.is_mapped() {
        let main_loop = glib::MainLoop::new(None, false);
        let done = main_loop.clone();
        let signal = widget.connect_map(move |_| done.quit());
        let expired = Rc::new(Cell::new(false));
        let flag = expired.clone();
        let done = main_loop.clone();
        let timeout = glib::timeout_add_local_once(Duration::from_secs(5), move || {
            flag.set(true);
            done.quit();
        });
        main_loop.run();
        widget.disconnect(signal);
        if !expired.get() {
            timeout.remove();
        }
        assert!(widget.is_mapped(), "{name} did not map");
    }
    let window = widget.root().unwrap().downcast::<gtk::Window>().unwrap();
    wait_for_frame(widget);
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(widget)).snapshot(
        &snapshot,
        f64::from(widget.width()),
        f64::from(widget.height()),
    );
    let node = snapshot.to_node().expect("native GTK render node");
    let texture = window.renderer().unwrap().render_texture(&node, None);
    assert!(texture.width() > 0 && texture.height() > 0);
    texture
        .save_to_png(directory.join(format!("{name}.png")))
        .unwrap();
    eprintln!(
        "documentation screenshot: {name} ({}x{})",
        texture.width(),
        texture.height()
    );
}

fn menu(directory: &std::path::Path, button: &gtk::MenuButton, name: &str) {
    let panel = button.popover().unwrap();
    if name == "page-folders" {
        for scroller in descendants::<gtk::ScrolledWindow>(panel.upcast_ref()) {
            scroller.set_max_content_height(600);
        }
    }
    panel.set_autohide(false);
    button.popup();
    save(directory, panel.upcast_ref(), name);
    button.popdown();
}

fn dropdown(directory: &std::path::Path, picker: &gtk::DropDown, name: &str) {
    let panel = descendants::<gtk::Popover>(picker.upcast_ref())
        .into_iter()
        .next()
        .expect("production dropdown popup");
    if name == "artwork-touch-zones" {
        for scroller in descendants::<gtk::ScrolledWindow>(panel.upcast_ref()) {
            scroller.set_max_content_height(600);
            scroller.set_min_content_height(520);
        }
    }
    assert!(picker.activate(), "native dropdown activation");
    save(directory, panel.upcast_ref(), name);
    panel.popdown();
}

#[test]
fn regenerate_documentation_screenshots_when_explicitly_requested() {
    if std::env::var_os("STREAMDECK_DOC_SCREENSHOTS_DIR").is_some() {
        gtk::test_synced(capture_documentation_screenshots);
    }
}

fn capture_documentation_screenshots() {
    let Some(directory) = std::env::var_os("STREAMDECK_DOC_SCREENSHOTS_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    gtk::init().expect("documentation capture requires a native GTK display");
    // Bundled action icons resolve through the app resource path, as at startup.
    crate::app_icon::register().unwrap();
    let settings = gtk::Settings::default().unwrap();
    let original_theme = settings.gtk_theme_name();
    let original_dark = settings.is_gtk_application_prefer_dark_theme();
    let original_animations = settings.is_gtk_enable_animations();
    settings.set_gtk_enable_animations(false);
    settings.set_gtk_theme_name(Some("Adwaita-dark"));
    settings.set_gtk_application_prefer_dark_theme(true);

    // Given: isolated, realistic profiles, bindings and app-owned artwork.
    let (root, mut store, mut images, source) = artwork_fixture("documentation");
    store.create("OBS").unwrap();
    store.select("Base");
    let mut organization = sdeck_plus::organization::Organization::in_memory(&store);
    organization.set_profile_application("OBS", "obs").unwrap();
    organization.rename_page(&store, "Everyday").unwrap();
    // A few page folders so the Page folders popover shows a real list.
    for folder in ["Studio tools", "Audio mix", "Lighting"] {
        organization.create_folder(&mut store, folder).unwrap();
        store.select("Base");
    }
    organization
        .create_named_page(&mut store, "Production")
        .unwrap();
    store.select("Base");
    let labels = [
        "Play", "Mute", "Scene", "Files", "Record", "Browser", "Next", "Studio",
    ];
    let colors = [
        0x254b75ff, 0x5c3566ff, 0x245c58ff, 0x66462cff, 0x702f49ff, 0x3a457aff, 0x3a5b38ff,
        0x55447aff,
    ];
    for profile in ["Base", "Studio", "OBS"] {
        for (index, (label, color)) in labels.iter().zip(colors).enumerate() {
            let key = u8::try_from(index + 1).unwrap();
            // Keys 5 to 7 have no artwork: they show their binding icons.
            if (5..=7).contains(&key) {
                continue;
            }
            let pixbuf =
                gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 120, 120)
                    .unwrap();
            pixbuf.fill(color);
            pixbuf.savev(&source, "png", &[]).unwrap();
            images
                .assign_in_layer(profile, ArtworkTarget::Keypad(key), &source)
                .unwrap();
            images
                .set_button_text_in_layer(
                    profile,
                    ArtworkTarget::Keypad(key),
                    ButtonText {
                        text: (*label).into(),
                        font_size: 22,
                        ..ButtonText::default()
                    },
                )
                .unwrap();
        }
        // Sections 2 and 3 have no artwork either.
        for zone in [1, 4] {
            images
                .assign_in_layer(profile, ArtworkTarget::EncoderZone(zone), &source)
                .unwrap();
        }
        store
            .bind(
                profile,
                Trigger::KeyPress(1),
                Action::Shortcut {
                    modifiers: vec!["CTRL".into(), "ALT".into()],
                    key: "p".into(),
                },
            )
            .unwrap();
        for (key, action) in [
            (
                5,
                Action::Feature(FeatureAction::Obs(ObsAction::ToggleRecording)),
            ),
            (
                6,
                Action::Feature(FeatureAction::Desktop(
                    sdeck_plus::desktop_actions::DesktopAction::Website {
                        url: "https://example.com".into(),
                    },
                )),
            ),
            (
                7,
                Action::Feature(FeatureAction::Desktop(
                    sdeck_plus::desktop_actions::DesktopAction::Media(
                        sdeck_plus::desktop_actions::MediaCommand::Next,
                    ),
                )),
            ),
        ] {
            store.bind(profile, Trigger::KeyPress(key), action).unwrap();
        }
        store
            .bind(
                profile,
                Trigger::ZoneTap(2),
                Action::Feature(FeatureAction::Obs(ObsAction::Scene("Camera".into()))),
            )
            .unwrap();
        store
            .bind(
                profile,
                Trigger::ZoneTap(3),
                Action::Feature(FeatureAction::Desktop(
                    sdeck_plus::desktop_actions::DesktopAction::Media(
                        sdeck_plus::desktop_actions::MediaCommand::Mute,
                    ),
                )),
            )
            .unwrap();
        store
            .bind(
                profile,
                Trigger::EncoderClockwise(1),
                Action::Feature(FeatureAction::Desktop(
                    sdeck_plus::desktop_actions::DesktopAction::Media(
                        sdeck_plus::desktop_actions::MediaCommand::VolumeUp,
                    ),
                )),
            )
            .unwrap();
        store
            .bind(
                profile,
                Trigger::TouchSwipe(SwipeDirection::Left),
                Action::Feature(FeatureAction::Navigation(
                    sdeck_plus::organization::NavigationAction::NextPage,
                )),
            )
            .unwrap();
    }
    store.persist().unwrap();
    let configuration_root = store.root().to_path_buf();
    let layers = Rc::new(RefCell::new(store));
    let (output, hardware_commands) = mpsc::channel();
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
    let window = ApplicationWindow::builder()
        .title(APP_NAME)
        .default_width(1000)
        .default_height(950)
        .build();
    install_style(&window);
    let (sender, commands) = mpsc::channel();
    let widgets = build_widgets(window.clone(), layers, artwork.clone(), sender);
    let (device, _device_commands) = mpsc::channel();
    let (events, _events) = mpsc::channel();
    widgets.feature_runtime.configure(
        Ok(organization),
        Ok(sdeck_plus::obs::ObsConfig::default()),
        device,
        events,
    );
    widgets
        .binding_ui
        .refresh_layer_targets(&artwork.layers.borrow().names());
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(1000, 950);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);
    artwork.refresh(false);

    // When: drive the same native controls as configuration, never Test action.
    // The setup above edits fields directly, so treat the editor as saved before navigating.
    widgets.binding_ui.mark_clean();
    key_button(&widgets.key_labels[0]).emit_clicked();
    let capture = |name: &str| save(&directory, widgets.root.upcast_ref(), name);
    capture("main-window");
    settings.set_gtk_theme_name(Some("Adwaita"));
    settings.set_gtk_application_prefer_dark_theme(false);
    wait_for_frame(&widgets.root);
    capture("main-window-light");
    settings.set_gtk_theme_name(Some("Adwaita-dark"));
    settings.set_gtk_application_prefer_dark_theme(true);
    key_button(&widgets.key_labels[7]).emit_clicked();
    capture("default-none");
    key_button(&widgets.key_labels[0]).emit_clicked();
    widgets.binding_ui.return_toggle.set_active(true);
    widgets.binding_ui.return_seconds.set_text("10");
    assert!(widgets.binding_ui.return_model.n_items() > 0);
    widgets.binding_ui.return_target.set_selected(0);
    capture("page-return");
    widgets.binding_ui.return_toggle.set_active(false);
    assert_reachable(&widgets.binding_ui.action_picker, &widgets.root);
    dropdown(
        &directory,
        &widgets.binding_ui.action_picker,
        "action-types",
    );
    widgets.binding_ui.action_picker.set_selected(5);
    wait_for_frame(&widgets.root);
    let category =
        descendants::<gtk::DropDown>(widgets.binding_ui.feature_editor.root.upcast_ref()).remove(0);
    assert_reachable(&category, &widgets.root);
    dropdown(&directory, &category, "more-actions-categories");
    widgets
        .binding_ui
        .populate_action(&Action::Feature(FeatureAction::Obs(ObsAction::Scene(
            "Studio camera".into(),
        ))));
    widgets.root.vadjustment().set_value(0.0);
    capture("more-actions-obs");
    widgets
        .binding_ui
        .populate_action(&Action::Feature(FeatureAction::Desktop(
            sdeck_plus::desktop_actions::DesktopAction::CycleAudioOutput,
        )));
    widgets.binding_ui.feature_editor.wait_for_audio_outputs();
    capture("more-actions-audio-output");
    widgets.binding_ui.mark_clean();
    widgets.encoder_labels[0]
        .ancestor(Button::static_type())
        .unwrap()
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
    assert_reachable(&widgets.binding_ui.event_picker, &widgets.root);
    dropdown(
        &directory,
        &widgets.binding_ui.event_picker,
        "encoder-events",
    );
    widgets.binding_ui.mark_clean();
    widgets
        .touch_status
        .parent()
        .unwrap()
        .next_sibling()
        .unwrap()
        .downcast::<gtk::Overlay>()
        .unwrap()
        .child()
        .unwrap()
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
    widgets.binding_ui.event_picker.set_selected(3);
    widgets.root.vadjustment().set_value(0.0);
    capture("touch-strip");

    widgets.binding_ui.mark_clean();
    key_button(&widgets.key_labels[0]).emit_clicked();
    let editor = &widgets._text_editor;
    wait_for_frame(&widgets.root);
    assert_reachable(&editor.open, &widgets.root);
    wait_for_frame(&widgets.root);
    // Show an edited draft: Save enabled, the unsaved hint and a grid choice.
    editor.panel.set_autohide(false);
    editor.open.popup();
    editor.input.buffer().set_text("Play\nNow");
    editor.position[7].set_active(true);
    save(&directory, editor.panel.upcast_ref(), "button-text-editor");
    editor.open.popdown();
    widgets.artwork_section.set_expanded(true);
    wait_for_frame(&widgets.root);
    assert_reachable(&widgets.target_picker, &widgets.root);
    dropdown(&directory, &widgets.target_picker, "artwork-touch-zones");
    widgets.artwork_section.set_expanded(false);
    widgets.root.vadjustment().set_value(0.0);
    wait_for_frame(&widgets.root);

    widgets.feature_runtime.edit_profile("Studio");
    menu(
        &directory,
        &widgets.management_menu,
        "organize-profiles-pages",
    );
    let panel = widgets.management_menu.popover().unwrap();
    let tasks = descendants::<gtk::Stack>(panel.upcast_ref()).remove(0);
    tasks.set_visible_child_name("new");
    menu(&directory, &widgets.management_menu, "organize-new-profile");
    tasks.set_visible_child_name("edit");
    settings.set_gtk_theme_name(Some("Adwaita"));
    settings.set_gtk_application_prefer_dark_theme(false);
    wait_for_frame(&widgets.root);
    menu(&directory, &widgets.management_menu, "organize-light-theme");
    settings.set_gtk_theme_name(Some("Adwaita-dark"));
    settings.set_gtk_application_prefer_dark_theme(true);
    widgets
        .feature_runtime
        .profile_tabs()
        .set_current_page(Some(1));
    capture("application-profile-tab");
    menu(&directory, &widgets.obs_menu, "obs-settings");
    menu(&directory, &widgets.help_menu, "configuration-menu");
    let help = widgets._configuration_menu.help_for_test();
    help.present();
    save(&directory, help.upcast_ref(), "setup-help");
    help.response(ResponseType::Close);
    let bundle = root.join("stream-deck-configuration.sdeck-plus.json");
    sdeck_plus::config_bundle::export(&configuration_root, &bundle).unwrap();
    widgets
        ._configuration_menu
        .select_path_for_test(true, bundle);
    let confirmation = widgets._configuration_menu.confirmation_for_test().unwrap();
    save(
        &directory,
        confirmation.upcast_ref(),
        "import-configuration",
    );
    confirmation.response(ResponseType::Cancel);
    widgets
        .feature_runtime
        .profile_tabs()
        .set_current_page(Some(0));

    click(&widgets, "New page");
    let dialog = current_dialog();
    page_dialog_entry(&dialog).set_text("Editing tools");
    save(&directory, dialog.upcast_ref(), "new-page-name");
    dialog.response(ResponseType::Cancel);
    click(&widgets, "Rename page");
    let dialog = current_dialog();
    page_dialog_entry(&dialog).set_text("Everyday controls");
    save(&directory, dialog.upcast_ref(), "rename-page");
    dialog.response(ResponseType::Cancel);
    menu(
        &directory,
        widgets.feature_runtime.folder_menu(),
        "page-folders",
    );

    key_button(&widgets.key_labels[0]).emit_clicked();
    widgets.root.vadjustment().set_value(0.0);
    set_pressed(&widgets.key_widgets[0], true);
    set_pressed(&widgets.encoder_widgets[1], true);
    // Refresh momentary feedback on the frame update signal, not a timer race.
    let zones = widgets.zone_widgets.clone();
    let dial = widgets.encoder_widgets[2].clone();
    let clock = widgets.root.frame_clock().unwrap();
    let feedback = clock.connect_update(move |_| {
        flash_zones(&zones, 0, 199);
        flash(&dial, "control-rotated");
    });
    clock.request_phase(gtk::gdk::FrameClockPhase::UPDATE);
    capture("live-input-feedback");
    clock.disconnect(feedback);
    set_pressed(&widgets.key_widgets[0], false);
    set_pressed(&widgets.encoder_widgets[1], false);
    widgets.encoder_widgets[2].remove_css_class("control-rotated");
    for zone in widgets.zone_widgets.iter() {
        zone.remove_css_class("control-touched");
    }
    {
        let mut layers = artwork.layers.borrow_mut();
        for (key, source) in [
            (1, sdeck_plus::info::InfoSource::Clock),
            (2, sdeck_plus::info::InfoSource::Cpu),
            (3, sdeck_plus::info::InfoSource::Memory),
            (4, sdeck_plus::info::InfoSource::Temperature),
            (5, sdeck_plus::info::InfoSource::Volume),
        ] {
            layers
                .bind("Base", Trigger::KeyPress(key), Action::None)
                .unwrap();
            layers
                .set_info("Base", &Trigger::KeyPress(key), Some(source))
                .unwrap();
        }
        layers
            .bind("Base", Trigger::EncoderPress(1), Action::None)
            .unwrap();
        layers
            .set_info(
                "Base",
                &Trigger::EncoderPress(1),
                Some(sdeck_plus::info::InfoSource::AudioDevice),
            )
            .unwrap();
        layers
            .bind("Base", Trigger::ZoneTap(2), Action::None)
            .unwrap();
        layers
            .set_info(
                "Base",
                &Trigger::ZoneTap(2),
                Some(sdeck_plus::info::InfoSource::Weather),
            )
            .unwrap();
        layers
            .bind("Base", Trigger::ZoneTap(4), Action::None)
            .unwrap();
        layers
            .set_info(
                "Base",
                &Trigger::ZoneTap(4),
                Some(sdeck_plus::info::InfoSource::NowPlaying),
            )
            .unwrap();
    }
    // Deterministic readings use the production renderer, not the host's data.
    for (source, headline, detail, level) in [
        (
            sdeck_plus::info::InfoSource::Clock,
            "10:24",
            "Fri 16 Oct",
            None,
        ),
        (sdeck_plus::info::InfoSource::Cpu, "38%", "CPU", Some(38)),
        (
            sdeck_plus::info::InfoSource::Memory,
            "6.4 GiB",
            "Memory",
            Some(41),
        ),
        (
            sdeck_plus::info::InfoSource::Temperature,
            "47°C",
            "Temperature",
            Some(47),
        ),
        (
            sdeck_plus::info::InfoSource::Volume,
            "42%",
            "Volume",
            Some(42),
        ),
        (
            sdeck_plus::info::InfoSource::AudioDevice,
            "Speakers",
            "Default output",
            None,
        ),
        (sdeck_plus::info::InfoSource::Weather, "22°C", "Clear", None),
        (
            sdeck_plus::info::InfoSource::NowPlaying,
            "So What",
            "Miles Davis",
            None,
        ),
    ] {
        artwork.update_info(
            source,
            sdeck_plus::info::InfoValue {
                glyph: match source {
                    sdeck_plus::info::InfoSource::Weather => sdeck_plus::info::Glyph::Sun,
                    sdeck_plus::info::InfoSource::NowPlaying => sdeck_plus::info::Glyph::Play,
                    _ => source.glyph(),
                },
                detail: detail.into(),
                level,
                ..sdeck_plus::info::InfoValue::text(source.glyph(), headline)
            },
        );
    }
    widgets.binding_ui.load_selected_binding(&artwork.layers);
    capture("live-information");
    widgets.binding_ui.info_source.set_selected(4);
    widgets.binding_ui.info_location.set_text("Mexico City");
    capture("live-information-settings");

    // Then: every documented artifact was rendered, with no dispatched action.
    assert_eq!(widgets.status.text(), "Not connected");
    assert!(commands.try_iter().all(|command| !matches!(
        command,
        LayerCommand::Execute { .. } | LayerCommand::ObsProbe(_)
    )));
    drop(hardware_commands);
    window.destroy();
    settings.set_gtk_theme_name(original_theme.as_deref());
    settings.set_gtk_application_prefer_dark_theme(original_dark);
    settings.set_gtk_enable_animations(original_animations);
    std::fs::remove_dir_all(root).unwrap();
    eprintln!(
        "Generated all 26 native documentation screenshots in {}",
        directory.display()
    );
}
