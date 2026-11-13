use super::*;

pub(super) fn catalog_fixture(
    tag: &str,
    check: impl FnOnce(&MonitorWidgets, &Rc<RefCell<LayerStore>>, &mpsc::Receiver<LayerCommand>),
) {
    if !init_gtk_for_test() {
        return;
    }
    let (root, mut store, images, _) = artwork_fixture(tag);
    store
        .bind(
            "Base",
            Trigger::KeyPress(2),
            Action::Command("original".into()),
        )
        .unwrap();
    store.persist().unwrap();
    let layers = Rc::new(RefCell::new(store));
    let (output, _hardware) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(images))),
        layers: layers.clone(),
        pictures: Rc::default(),
        output,
        last_layer: Rc::default(),
        chooser_owners: Rc::default(),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::default(),
        info: Rc::default(),
    };
    let (commands, requests) = mpsc::channel();
    let window = ApplicationWindow::builder()
        .default_width(1100)
        .default_height(900)
        .build();
    install_style(&window);
    let widgets = build_widgets(window.clone(), layers.clone(), artwork, commands);
    window.set_child(Some(&widgets.root));
    window.present();
    wait_for_frame(&window);
    check(&widgets, &layers, &requests);
    if let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        rendered_texture(&window, &widgets.root)
            .save_to_png(std::path::Path::new(&directory).join(format!("{tag}.png")))
            .unwrap();
    }
    window.close();
    std::fs::remove_dir_all(root).unwrap();
}

pub(super) fn collect_catalog_buttons(widget: &gtk::Widget, buttons: &mut Vec<Button>) {
    if let Some(button) = widget.downcast_ref::<Button>() {
        buttons.push(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        collect_catalog_buttons(&widget, buttons);
        child = widget.next_sibling();
    }
}

pub(super) fn catalog_drop(widgets: &MonitorWidgets, caption: &str, key: usize) -> bool {
    let mut buttons = Vec::new();
    collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
    let row = buttons
        .iter()
        .find(|b| {
            b.tooltip_text().as_deref()
                == Some(&format!(
                    "Drag {caption} onto a key or dial, or click to assign to the selected control."
                ))
        })
        .expect("catalog action row");
    let source = row.observe_controllers();
    let source = (0..source.n_items())
        .find_map(|i| source.item(i)?.downcast::<gtk::DragSource>().ok())
        .unwrap();
    let provider = source
        .emit_by_name::<Option<gtk::gdk::ContentProvider>>("prepare", &[&0.0f64, &0.0f64])
        .unwrap();
    let targets = widgets.key_widgets[key].observe_controllers();
    let target = (0..targets.n_items())
        .filter_map(|i| targets.item(i)?.downcast::<gtk::DropTarget>().ok())
        .find(|t| t.actions() == gtk::gdk::DragAction::COPY)
        .expect("action copy target");
    let value = provider.value(provider.formats().types()[0]).unwrap();
    target.emit_by_name::<bool>("drop", &[&glib::BoxedValue(value), &0.0f64, &0.0f64])
}

#[test]
fn dropping_parameter_free_action_assigns_without_executing() {
    run_gtk_test(|| {
        catalog_fixture("catalog-ready", |widgets, layers, requests| {
            assert!(catalog_drop(widgets, "Play / pause", 1));
            let expected = Action::Feature(sdeck_plus::features::FeatureAction::Desktop(
                sdeck_plus::desktop_actions::DesktopAction::Media(
                    sdeck_plus::desktop_actions::MediaCommand::PlayPause,
                ),
            ));
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(expected.clone())
            );
            assert_eq!(widgets.binding_ui.control_number(), Some(2));
            assert_eq!(
                widgets.binding_ui.binding_from_widgets(layers).unwrap().2,
                expected
            );
            assert!(!widgets.binding_ui.is_dirty());
            let mut saved = false;
            while let Ok(command) = requests.try_recv() {
                match command {
                    LayerCommand::SaveBinding(..) => saved = true,
                    LayerCommand::Execute { .. } => panic!("assigning must not execute"),
                    _ => {}
                }
            }
            assert!(saved, "drop must enqueue persistence");
        })
    });
}

#[test]
fn dropping_incomplete_action_preserves_binding_until_save() {
    run_gtk_test(|| {
        catalog_fixture("catalog-draft", |widgets, layers, requests| {
            while requests.try_recv().is_ok() {}
            assert!(catalog_drop(widgets, "Run command", 1));
            let ui = &widgets.binding_ui;
            assert_eq!(ui.action_kind(), ActionKind::RunCommand);
            assert_eq!(ui.control_number(), Some(2));
            assert!(ui.is_dirty());
            assert!(ui.text_row.is_visible());
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(Action::Command("original".into()))
            );
            assert!(
                requests.try_recv().is_err(),
                "incomplete drop must not write or execute"
            );
            // A draft elsewhere is only discarded after the user agrees.
            assert!(catalog_drop(widgets, "Play / pause", 2));
            discard_dialog().response(ResponseType::Cancel);
            assert_eq!(ui.control_number(), Some(2));
            assert_eq!(layers.borrow().binding("Base", &Trigger::KeyPress(3)), None);
            ui.text_entry.set_text("configured");
            let mut buttons = Vec::new();
            collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
            buttons
                .iter()
                .find(|b| button_caption(b).as_deref() == Some("Save binding"))
                .unwrap()
                .emit_clicked();
            assert_eq!(
                layers.borrow().binding("Base", &Trigger::KeyPress(2)),
                Some(Action::Command("configured".into()))
            );
            assert!(!ui.is_dirty());
        })
    });
}

/// SearchEntry debounces its change signal; wait for the real signal, never a fixed sleep.
fn set_search_text(search: &gtk::SearchEntry, text: &str) {
    let main_loop = glib::MainLoop::new(None, false);
    let done = main_loop.clone();
    let emitted = Rc::new(Cell::new(false));
    let seen = emitted.clone();
    let signal = search.connect_search_changed(move |_| {
        seen.set(true);
        done.quit();
    });
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let done = main_loop.clone();
    let guard = glib::timeout_add_local_once(Duration::from_secs(5), move || {
        flag.set(true);
        done.quit();
    });
    search.set_text(text);
    // Clearing the text emits synchronously; typing is debounced.
    if !emitted.get() {
        main_loop.run();
    }
    search.disconnect(signal);
    if !expired.get() {
        guard.remove();
    }
    assert!(!expired.get(), "search-changed was never emitted");
}

fn descendant_search(widget: &gtk::Widget) -> Option<gtk::SearchEntry> {
    // Profile dropdowns embed their own search entries; pick the catalog's.
    if let Ok(search) = widget.clone().downcast::<gtk::SearchEntry>() {
        if search.placeholder_text().as_deref() == Some("Search actions") {
            return Some(search);
        }
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = descendant_search(&current) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

#[test]
fn searching_the_catalog_shows_only_matching_actions_and_restores_on_clear() {
    run_gtk_test(|| {
        catalog_fixture("catalog-search", |widgets, _layers, _requests| {
            let mut buttons = Vec::new();
            collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
            let find = |caption: &str| {
                buttons
                    .iter()
                    .find(|b| {
                        b.tooltip_text().as_deref()
                            == Some(&format!(
                                "Drag {caption} onto a key or dial, or click to assign to the selected control."
                            ))
                    })
                    .unwrap()
                    .clone()
            };
            let volume = find("Volume up");
            let command = find("Run command");
            // The row's section is its grandparent's parent: body -> revealer -> section.
            let section = |row: &Button| row.parent().unwrap().parent().unwrap().parent().unwrap();
            let search = descendant_search(widgets.root.upcast_ref()).unwrap();
            set_search_text(&search, "volume");
            assert!(volume.is_visible() && section(&volume).is_visible());
            assert!(!command.is_visible() && !section(&command).is_visible());
            set_search_text(&search, "");
            assert!(command.is_visible() && section(&command).is_visible());
        });
    });
}

fn collect_with_class(widget: &gtk::Widget, class: &str, found: &mut Vec<gtk::Widget>) {
    if widget.has_css_class(class) {
        found.push(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect_with_class(&current, class, found);
        child = current.next_sibling();
    }
}

fn label_text(widget: &gtk::Widget) -> String {
    widget
        .downcast_ref::<gtk::Label>()
        .map(|label| label.text().to_string())
        .or_else(|| {
            widget
                .downcast_ref::<gtk::ToggleButton>()
                .and_then(|button| button.label())
                .map(|text| text.to_string())
        })
        .unwrap_or_default()
}

#[test]
fn category_chips_filter_the_catalog_to_one_category_and_all_restores_it() {
    run_gtk_test(|| {
        catalog_fixture("catalog-chips", |widgets, _layers, _requests| {
            let mut chips = Vec::new();
            collect_with_class(widgets.root.upcast_ref(), "catalog-chip", &mut chips);
            let names: Vec<_> = chips
                .iter()
                .filter(|chip| chip.is_visible())
                .map(label_text)
                .collect();
            assert_eq!(
                names,
                [
                    "All",
                    "Basic",
                    "System",
                    "Multimedia",
                    "Organization",
                    "Device",
                    "OBS"
                ],
                "plugins add their own chip only when a plugin is installed"
            );
            let chip = |name: &str| {
                chips
                    .iter()
                    .find(|chip| label_text(chip) == name)
                    .unwrap()
                    .downcast_ref::<gtk::ToggleButton>()
                    .unwrap()
                    .clone()
            };
            assert!(chip("All").is_active());
            let mut buttons = Vec::new();
            collect_catalog_buttons(widgets.root.upcast_ref(), &mut buttons);
            let find = |caption: &str| {
                buttons
                    .iter()
                    .find(|b| {
                        b.tooltip_text().as_deref()
                            == Some(&format!(
                                "Drag {caption} onto a key or dial, or click to assign to the selected control."
                            ))
                    })
                    .unwrap()
                    .clone()
            };
            let section = |row: &Button| row.parent().unwrap().parent().unwrap().parent().unwrap();
            let volume = find("Volume up");
            let command = find("Run command");
            chip("Multimedia").set_active(true);
            assert!(!chip("All").is_active(), "chips behave as one choice");
            assert!(section(&volume).is_visible() && !section(&command).is_visible());
            // The chosen category is open so its actions are reachable at once.
            let revealer = volume.parent().unwrap().parent().unwrap();
            assert!(revealer
                .downcast_ref::<gtk::Revealer>()
                .unwrap()
                .reveals_child());
            chip("All").set_active(true);
            assert!(section(&command).is_visible() && section(&volume).is_visible());
        });
    });
}

#[test]
fn multimedia_is_grouped_into_media_output_input_and_files_and_search_hides_the_headings() {
    run_gtk_test(|| {
        catalog_fixture("catalog-groups", |widgets, _layers, _requests| {
            let mut headings = Vec::new();
            collect_with_class(widgets.root.upcast_ref(), "catalog-group", &mut headings);
            let names: Vec<_> = headings.iter().map(label_text).collect();
            assert_eq!(
                names,
                [
                    "Media",
                    "Output (speakers, headphones)",
                    "Input (microphone)",
                    "Files"
                ]
            );
            let search = descendant_search(widgets.root.upcast_ref()).unwrap();
            set_search_text(&search, "microphone");
            assert!(
                headings.iter().all(|heading| !heading.is_visible()),
                "headings disappear while filtering"
            );
            set_search_text(&search, "");
            assert!(headings.iter().all(|heading| heading.is_visible()));
        });
    });
}

#[test]
fn the_dial_event_menu_lists_compact_left_aligned_rows() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let window = ApplicationWindow::builder()
            .default_width(400)
            .default_height(300)
            .build();
        install_style(&window);
        let anchor = Button::with_label("Dial 1");
        window.set_child(Some(&anchor));
        window.present();
        wait_for_frame(&window);
        crate::action_catalog::choose_event(anchor.upcast_ref(), |_| {});
        let mut popovers = Vec::new();
        let mut child = anchor.first_child();
        while let Some(widget) = child {
            if widget.has_css_class("dial-event-choice") {
                popovers.push(widget.clone());
            }
            child = widget.next_sibling();
        }
        assert_eq!(popovers.len(), 1);
        let mut rows = Vec::new();
        collect_catalog_buttons(&popovers[0], &mut rows);
        let captions: Vec<_> = rows.iter().filter_map(button_caption).collect();
        assert_eq!(
            captions,
            ["Turn clockwise", "Turn counterclockwise", "Press"]
        );
        for row in &rows {
            let label = row
                .child()
                .and_downcast::<gtk::Label>()
                .expect("row caption");
            assert_eq!(label.xalign(), 0.0, "{} starts at the left", label.text());
            assert!(
                row.measure(gtk::Orientation::Vertical, -1).0 >= 36,
                "{} is a comfortable target",
                label.text()
            );
        }
        window.destroy();
    });
}
