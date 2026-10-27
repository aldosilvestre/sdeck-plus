//! Binding icons in the window: Action menu rows, preview keys, dials and
//! touch sections (feature: binding icons).
use super::ui_refresh_tests::fixture;
use super::*;
use sdeck_plus::binding_icons::BindingIcon;

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

/// Icon widgets carry the vocabulary id in their name.
fn icon_ids(widget: &gtk::Widget) -> Vec<String> {
    descendants::<gtk::DrawingArea>(widget)
        .into_iter()
        .filter(|area| area.has_css_class("binding-icon") && area.is_visible())
        .map(|area| area.widget_name().to_string())
        .collect()
}

/// Open the dropdown's native popup and wait until it is mapped and painted.
fn open_popup(picker: &gtk::DropDown, panel: &gtk::Popover) {
    let main_loop = glib::MainLoop::new(None, false);
    let done = main_loop.clone();
    let signal = panel.connect_map(move |_| done.quit());
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let done = main_loop.clone();
    let timeout = glib::timeout_add_local_once(Duration::from_secs(5), move || {
        flag.set(true);
        done.quit();
    });
    assert!(picker.activate(), "native dropdown activation");
    if !panel.is_mapped() {
        main_loop.run();
    }
    panel.disconnect(signal);
    if !expired.get() {
        timeout.remove();
    }
    assert!(panel.is_mapped(), "dropdown popup did not map");
    wait_for_frame(panel);
}

#[test]
fn action_menu_rows_show_an_icon_name_and_check_mark() {
    run_gtk_test(assert_action_menu_icons);
}

fn assert_action_menu_icons() {
    let Some(fixture) = fixture("action-menu-icons", |_| {}) else {
        return;
    };
    let picker = &fixture.widgets.binding_ui.action_picker;
    picker.set_selected(3);
    let panel = descendants::<gtk::Popover>(picker.upcast_ref())
        .into_iter()
        .next()
        .expect("action menu popup");
    open_popup(picker, &panel);
    let expected: Vec<String> = (0..8)
        .map(|index| BindingIcon::for_action_type(index).id().to_owned())
        .collect();
    assert_eq!(icon_ids(panel.upcast_ref()), expected);
    let labels: Vec<String> = descendants::<GtkBox>(panel.upcast_ref())
        .into_iter()
        .filter(|row| row.has_css_class("icon-row"))
        .flat_map(|row| descendants::<Label>(row.upcast_ref()))
        .map(|label| label.text().to_string())
        .collect();
    assert_eq!(
        labels,
        [
            "Go to profile or page",
            "Run command",
            "Open application",
            "Shortcut",
            "Send named event",
            "More actions",
            "Play sound",
            "None",
        ]
    );
    // Exactly the selected row shows the check mark.
    let checks: Vec<gtk::Image> = descendants::<gtk::Image>(panel.upcast_ref())
        .into_iter()
        .filter(|image| image.icon_name().as_deref() == Some("object-select-symbolic"))
        .collect();
    assert_eq!(checks.len(), 8);
    let shown: Vec<usize> = checks
        .iter()
        .enumerate()
        .filter(|(_, image)| image.opacity() > 0.5)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(shown, [3]);
    panel.popdown();
    picker.set_selected(6);
    // The closed button shows the selected type's icon beside its name.
    assert_eq!(
        icon_ids(picker.upcast_ref()).first().map(String::as_str),
        Some("play-sound")
    );
    eprintln!("GTK action menu icon assertions executed");
}

#[test]
fn preview_dials_show_their_action_icon_with_the_caption() {
    run_gtk_test(assert_dial_icons);
}

fn assert_dial_icons() {
    let volume = Action::Feature(sdeck_plus::features::FeatureAction::Desktop(
        sdeck_plus::desktop_actions::DesktopAction::Media(
            sdeck_plus::desktop_actions::MediaCommand::VolumeUp,
        ),
    ));
    let Some(fixture) = fixture("dial-icons", |store| {
        store
            .bind("Base", Trigger::EncoderClockwise(2), volume.clone())
            .unwrap();
        store
            .bind(
                "Base",
                Trigger::EncoderPress(3),
                Action::Shortcut {
                    modifiers: vec!["CTRL".into()],
                    key: "m".into(),
                },
            )
            .unwrap();
        store
            .bind("Base", Trigger::EncoderPress(4), Action::None)
            .unwrap();
    }) else {
        return;
    };
    let widgets = &fixture.widgets;
    let icons = &widgets.binding_ui.dial_icons;
    assert_eq!(icons.len(), 4);
    assert!(!icons[0].is_visible(), "unassigned dials stay plain");
    assert!(!icons[3].is_visible(), "None shows no icon");
    assert_eq!(icons[1].widget_name(), "volume-up");
    assert_eq!(icons[2].widget_name(), "shortcut");
    let content = widgets.root.child().unwrap();
    for index in [1, 2] {
        let icon = &icons[index];
        assert!(icon.is_visible() && icon.has_css_class("binding-icon"));
        assert!(!icon.can_target(), "the knob keeps every click");
        let knob = widgets.encoder_widgets[index]
            .compute_bounds(&content)
            .unwrap();
        let bounds = icon.compute_bounds(&content).unwrap();
        let center = |rect: &gtk::graphene::Rect| {
            (
                rect.x() + rect.width() / 2.0,
                rect.y() + rect.height() / 2.0,
            )
        };
        let (icon_x, icon_y) = center(&bounds);
        let (knob_x, knob_y) = center(&knob);
        assert!((icon_x - knob_x).abs() <= 1.0 && (icon_y - knob_y).abs() <= 1.0);
        // The caption sits directly below the same knob.
        let caption = widgets.binding_ui.dial_legends[index]
            .compute_bounds(&content)
            .unwrap();
        assert!(caption.y() >= bounds.y() + bounds.height());
    }
    fixture
        .layers
        .borrow_mut()
        .unbind("Base", &Trigger::EncoderClockwise(2))
        .unwrap();
    widgets.binding_ui.refresh_summaries(&fixture.layers);
    assert!(!icons[1].is_visible());
    eprintln!("GTK dial icon assertions executed");
}

struct IconFixture {
    root: std::path::PathBuf,
    window: ApplicationWindow,
    artwork: ArtworkUi,
    hardware: mpsc::Receiver<ArtworkCommand>,
    _widgets: MonitorWidgets,
}

impl Drop for IconFixture {
    fn drop(&mut self) {
        self.window.destroy();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Production widgets whose artwork output channel the test keeps.
fn icon_fixture(
    tag: &str,
    setup: impl FnOnce(&mut LayerStore, &mut ArtworkStore, &std::path::Path),
) -> Option<IconFixture> {
    if !init_gtk_for_test() {
        return None;
    }
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let (root, mut store, mut images, source) = artwork_fixture(tag);
    setup(&mut store, &mut images, &source);
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
    let (commands, _requests) = mpsc::channel();
    let window = ApplicationWindow::builder()
        .default_width(DEFAULT_WIDTH)
        .default_height(DEFAULT_HEIGHT)
        .build();
    install_style(&window);
    let widgets = build_widgets(window.clone(), layers, artwork.clone(), commands);
    window.set_child(Some(&widgets.root));
    window.present();
    wait_for_frame(&window);
    artwork.refresh(true);
    Some(IconFixture {
        root,
        window,
        artwork,
        hardware,
        _widgets: widgets,
    })
}

fn picture(fixture: &IconFixture, target: ArtworkTarget) -> Picture {
    fixture
        .artwork
        .pictures
        .borrow()
        .iter()
        .find(|(shown, _)| *shown == target)
        .map(|(_, picture)| picture.clone())
        .unwrap()
}

/// Near-white pixels of a picture's paintable in `rows`.
fn bright_pixels(picture: &Picture, rows: std::ops::Range<usize>) -> usize {
    let texture = picture
        .paintable()
        .and_downcast::<gtk::gdk::Texture>()
        .expect("a texture");
    let (width, height) = (texture.width() as usize, texture.height() as usize);
    let mut pixels = vec![0; width * height * 4];
    texture.download(&mut pixels, width * 4);
    rows.flat_map(|y| (0..width).map(move |x| (y * width + x) * 4))
        .filter(|offset| {
            // Memory format is native-endian B8G8R8A8 premultiplied.
            pixels[offset + 3] > 200 && pixels[*offset] > 200 && pixels[offset + 1] > 200
        })
        .count()
}

fn tree_snapshot(root: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push((path.clone(), std::fs::read(&path).unwrap()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn keys_and_touch_sections_without_artwork_show_their_action_icon() {
    run_gtk_test(assert_preview_icons);
}

fn assert_preview_icons() {
    let Some(fixture) = icon_fixture("preview-icons", |store, images, source| {
        let command = Action::Command("make".into());
        store
            .bind("Base", Trigger::KeyPress(1), command.clone())
            .unwrap();
        store.bind("Base", Trigger::KeyPress(2), command).unwrap();
        store
            .bind("Base", Trigger::KeyPress(3), Action::None)
            .unwrap();
        store
            .bind("Base", Trigger::ZoneTap(2), Action::Emit("lights".into()))
            .unwrap();
        // Key 1 keeps its user artwork.
        images
            .assign_in_layer("Base", ArtworkTarget::Keypad(1), source)
            .unwrap();
    }) else {
        return;
    };
    let artwork_root = fixture.root.join("artwork");
    let before = tree_snapshot(&artwork_root);

    let key1 = picture(&fixture, ArtworkTarget::Keypad(1));
    assert!(key1.file().is_some(), "user artwork is shown as before");
    let key2 = picture(&fixture, ArtworkTarget::Keypad(2));
    assert!(key2.is_visible(), "a key without artwork shows its icon");
    assert!(bright_pixels(&key2, 14..64) > 60, "icon in the upper band");
    assert!(bright_pixels(&key2, 70..112) > 10, "caption below the icon");
    assert!(
        !picture(&fixture, ArtworkTarget::Keypad(3)).is_visible(),
        "None keeps the key empty"
    );
    assert!(
        !picture(&fixture, ArtworkTarget::Keypad(4)).is_visible(),
        "unassigned keys stay empty"
    );
    let zone = picture(&fixture, ArtworkTarget::EncoderZone(2));
    assert!(zone.is_visible(), "a bound touch section shows its icon");
    assert!(bright_pixels(&zone, 10..60) > 40);
    assert!(!picture(&fixture, ArtworkTarget::EncoderZone(3)).is_visible());

    // Rendering never writes into the artwork or button-text stores.
    fixture.artwork.refresh(true);
    assert_eq!(tree_snapshot(&artwork_root), before);
    let store = fixture.artwork.store.borrow();
    let store = store.as_ref().unwrap();
    assert!(store
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(2))
        .is_none());
    assert!(store
        .button_text_in_layer("Base", ArtworkTarget::Keypad(2))
        .is_none());
    // On the device, the key with artwork keeps its plain artwork command.
    let snapshot = fixture
        .hardware
        .try_iter()
        .filter_map(|command| match command {
            ArtworkCommand::Replace(commands) => Some(commands),
            _ => None,
        })
        .last()
        .expect("a device snapshot");
    assert!(snapshot.iter().any(|command| matches!(
        command,
        ArtworkCommand::Assign {
            target: ArtworkTarget::Keypad(1),
            ..
        }
    )));
    eprintln!("GTK preview binding icon assertions executed");
}

#[test]
fn icon_actions_share_one_size_and_one_delete_icon() {
    run_gtk_test(assert_icon_action_consistency);
}

fn image_names(widget: &gtk::Widget) -> Vec<String> {
    descendants::<gtk::Image>(widget)
        .into_iter()
        .filter_map(|image| image.icon_name().map(|name| name.to_string()))
        .collect()
}

fn assert_icon_action_consistency() {
    let Some(fixture) = fixture("icon-actions", |_| {}) else {
        return;
    };
    let root = fixture.widgets.root.upcast_ref::<gtk::Widget>();
    let mut deletes = Vec::new();
    for button in descendants::<Button>(root) {
        if !button.has_css_class("icon-action") {
            continue;
        }
        let caption = button.tooltip_text().unwrap_or_default().to_string();
        assert!(!caption.is_empty(), "icon actions keep a tooltip");
        for image in descendants::<gtk::Image>(button.upcast_ref()) {
            assert_eq!(image.pixel_size(), 16, "{caption}: 16px icon");
            let name = image.icon_name().unwrap_or_default();
            assert!(name.ends_with("-symbolic"), "{caption}: {name}");
        }
        if caption.starts_with("Delete") || caption == "Remove binding" {
            deletes.push((caption, image_names(button.upcast_ref())));
        }
    }
    for caption in [
        "Remove binding",
        "Delete page",
        "Delete folder",
        "Delete touchscreen page",
        "Delete the selected profile",
    ] {
        assert!(
            deletes.iter().any(|(shown, _)| shown.starts_with(caption)),
            "{caption} is an icon action: {deletes:?}"
        );
    }
    for (caption, names) in &deletes {
        assert_eq!(names, &["user-trash-symbolic"], "{caption}");
    }
    let organize = descendants::<gtk::MenuButton>(root)
        .into_iter()
        .find(|button| button.tooltip_text().as_deref() == Some("Organize"))
        .expect("Organize menu");
    assert_eq!(
        image_names(organize.upcast_ref())
            .first()
            .map(String::as_str),
        Some("preferences-system-symbolic"),
        "profile organization is not a new-folder action"
    );
    let text = descendants::<gtk::MenuButton>(root)
        .into_iter()
        .find(|button| {
            matches!(
                button.tooltip_text().as_deref(),
                Some("Add text" | "Edit text")
            )
        })
        .expect("button text editor");
    assert_eq!(
        image_names(text.upcast_ref()).first().map(String::as_str),
        Some("format-text-plaintext-symbolic"),
        "button text reads as text"
    );
    eprintln!("GTK icon action consistency assertions executed");
}

#[test]
fn device_snapshot_draws_binding_icons_only_where_the_preview_does() {
    run_gtk_test(assert_device_icons);
}

fn assert_device_icons() {
    let Some(fixture) = icon_fixture("device-icons", |store, images, source| {
        let command = Action::Command("make".into());
        store
            .bind("Base", Trigger::KeyPress(1), command.clone())
            .unwrap();
        store
            .bind("Base", Trigger::KeyPress(2), command.clone())
            .unwrap();
        store
            .bind("Base", Trigger::KeyPress(3), Action::None)
            .unwrap();
        store.bind("Base", Trigger::KeyPress(5), command).unwrap();
        images
            .assign_in_layer("Base", ArtworkTarget::Keypad(1), source)
            .unwrap();
        // Centered user text leaves no room: key 5 keeps its text picture.
        images
            .set_button_text_in_layer(
                "Base",
                ArtworkTarget::Keypad(5),
                sdeck_plus::button_text::ButtonText {
                    text: "Build".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        // A second touch page puts a "previous" indicator on section 1.
        let organization = sdeck_plus::organization::Organization::in_memory(store);
        store
            .initialize_touchscreens(&organization, images)
            .unwrap();
        store.create_touchscreen_page("Base").unwrap();
        store
            .bind("Base", Trigger::ZoneTap(1), Action::Emit("lights".into()))
            .unwrap();
        store
            .bind("Base", Trigger::ZonePress(2), Action::Command("ls".into()))
            .unwrap();
    }) else {
        return;
    };
    let snapshot = fixture
        .hardware
        .try_iter()
        .filter_map(|command| match command {
            ArtworkCommand::Replace(commands) => Some(commands),
            _ => None,
        })
        .last()
        .expect("a device snapshot");
    let icons: Vec<(ArtworkTarget, BindingIcon, bool)> = snapshot
        .iter()
        .filter_map(|command| match command {
            ArtworkCommand::ComposeIcon {
                target,
                icon,
                previous,
                ..
            } => Some((*target, *icon, *previous)),
            _ => None,
        })
        .collect();
    assert_eq!(
        icons,
        [
            (ArtworkTarget::Keypad(2), BindingIcon::Command, false),
            (ArtworkTarget::EncoderZone(1), BindingIcon::NamedEvent, true),
            (ArtworkTarget::EncoderZone(2), BindingIcon::Command, false),
        ]
    );
    assert!(snapshot
        .iter()
        .any(|command| matches!(command, ArtworkCommand::ComposeButton { key: 5, .. })));
    // Indicated section 1 is drawn once, by its icon command.
    assert!(!snapshot.iter().any(|command| matches!(
        command,
        ArtworkCommand::ComposeTouchscreenZone { zone: 1, .. }
    )));
    // The worker turns each command into a device JPEG without touching disk.
    for command in snapshot {
        if let ArtworkCommand::ComposeIcon {
            target,
            icon,
            caption,
            text,
            strip_path,
            previous,
            next,
        } = command
        {
            let jpeg = sdeck_plus::image_output::prepare_icon_jpeg(
                target,
                icon,
                &caption,
                text.as_ref(),
                strip_path.as_deref(),
                previous,
                next,
            )
            .unwrap();
            assert_eq!(&jpeg[..2], &[0xff, 0xd8]);
        }
    }
    eprintln!("GTK device binding icon assertions executed");
}

#[test]
fn strip_header_shows_gesture_badges_without_covering_the_sections() {
    run_gtk_test(assert_gesture_badges);
}

fn assert_gesture_badges() {
    use sdeck_plus::layers::SwipeDirection;
    let Some(fixture) = fixture("gesture-badges", |store| {
        store
            .bind(
                "Base",
                Trigger::TouchSwipe(SwipeDirection::Right),
                Action::Emit("lights".into()),
            )
            .unwrap();
        store
            .bind("Base", Trigger::TouchTap, Action::Command("ls".into()))
            .unwrap();
    }) else {
        return;
    };
    let widgets = &fixture.widgets;
    let row = &widgets.binding_ui.gesture_badges;
    let badges = || {
        descendants::<GtkBox>(row.upcast_ref())
            .into_iter()
            .filter(|badge| badge.has_css_class("gesture-badge"))
            .collect::<Vec<_>>()
    };
    let shown: Vec<(Vec<String>, bool, String)> = badges()
        .iter()
        .map(|badge| {
            (
                icon_ids(badge.upcast_ref()),
                badge.has_css_class("default"),
                badge.tooltip_text().unwrap_or_default().to_string(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (
                vec!["swipe-left".to_owned(), "next-page".to_owned()],
                true,
                "Swipe left (default): Next touchscreen page".to_owned()
            ),
            (
                vec!["swipe-right".to_owned(), "named-event".to_owned()],
                false,
                "Swipe right: Emit lights".to_owned()
            ),
            (
                vec!["tap".to_owned(), "command".to_owned()],
                false,
                "Tap: Run ls".to_owned()
            ),
        ]
    );
    // The badges share the strip header line and stay above the sections.
    assert_eq!(row.parent(), widgets.touch_status.parent());
    let content = widgets.root.child().unwrap();
    let header = row.compute_bounds(&content).unwrap();
    for zone in &widgets.zone_widgets {
        let zone = zone.compute_bounds(&content).unwrap();
        assert!(header.y() + header.height() <= zone.y() + 0.5);
    }
    fixture
        .layers
        .borrow_mut()
        .bind(
            "Base",
            Trigger::TouchSwipe(SwipeDirection::Left),
            Action::None,
        )
        .unwrap();
    widgets.binding_ui.refresh_summaries(&fixture.layers);
    assert_eq!(
        badges().len(),
        2,
        "an explicit None removes the default badge"
    );
    eprintln!("GTK strip gesture badge assertions executed");
}
