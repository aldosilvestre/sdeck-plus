//! Main-window hierarchy and legibility contract (feature: UI refresh).
use super::*;
use sdeck_plus::features::FeatureAction;

struct Fixture {
    root: std::path::PathBuf,
    window: ApplicationWindow,
    widgets: MonitorWidgets,
    layers: Rc<RefCell<LayerStore>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.window.destroy();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Production widgets at the default size, with an optional binding setup.
fn fixture(tag: &str, setup: impl FnOnce(&mut LayerStore)) -> Option<Fixture> {
    if !init_gtk_for_test() {
        assert!(
            std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "GTK initialization failed despite an advertised display"
        );
        return None;
    }
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let (root, mut store, images, _) = artwork_fixture(tag);
    setup(&mut store);
    let layers = Rc::new(RefCell::new(store));
    let (output, _hardware) = mpsc::channel();
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
    let widgets = build_widgets(window.clone(), layers.clone(), artwork, commands);
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(DEFAULT_WIDTH, DEFAULT_HEIGHT);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);
    Some(Fixture {
        root,
        window,
        widgets,
        layers,
    })
}

fn find_button(widgets: &MonitorWidgets, caption: &str) -> Button {
    let mut buttons = Vec::new();
    collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
    buttons
        .into_iter()
        .find(|button| button_caption(button).as_deref() == Some(caption))
        .unwrap_or_else(|| panic!("missing production button: {caption}"))
}

#[test]
fn page_toolbar_separates_navigation_from_page_management() {
    run_gtk_test(assert_page_toolbar_hierarchy);
}

fn assert_page_toolbar_hierarchy() {
    let Some(fixture) = fixture("toolbar", |_| {}) else {
        return;
    };
    let widgets = &fixture.widgets;
    let content = widgets.root.child().unwrap();
    let bounds = |widget: &gtk::Widget| widget.compute_bounds(&content).unwrap();

    // Navigation: previous/next/parent form one linked group before the breadcrumb.
    let navigation: Vec<Button> = ["Previous", "Next", "Parent"]
        .iter()
        .map(|caption| find_button(widgets, caption))
        .collect();
    let group = navigation[0].parent().unwrap();
    assert!(
        group.has_css_class("linked"),
        "navigation is one linked group"
    );
    for button in &navigation {
        assert_eq!(button.parent().as_ref(), Some(&group));
    }
    let breadcrumb = bounds(widgets.feature_runtime.indicator.upcast_ref());
    let group_bounds = bounds(&group);
    assert!(
        group_bounds.x() + group_bounds.width() <= breadcrumb.x(),
        "breadcrumb follows its navigation: {group_bounds:?}, {breadcrumb:?}"
    );

    // Management: New page keeps its caption; rename/delete are lighter icon actions.
    let create = find_button(widgets, "New page");
    assert!(breadcrumb.x() + breadcrumb.width() <= bounds(create.upcast_ref()).x());
    for (caption, destructive) in [("Rename page", false), ("Delete page", true)] {
        let button = find_button(widgets, caption);
        assert!(
            button.child().unwrap().downcast::<gtk::Image>().is_ok(),
            "{caption} is icon-only"
        );
        assert!(
            button.has_css_class("flat"),
            "{caption} is visually lighter"
        );
        assert!(button.has_css_class("icon-only"));
        assert_eq!(button.tooltip_text().as_deref(), Some(caption));
        assert_eq!(button.has_css_class("destructive-action"), destructive);
        assert_eq!(button.width(), button.height(), "{caption} square hit area");
    }

    // Profile tools beside the tabs are secondary to the tabs themselves.
    for menu in [&widgets.management_menu, &widgets.obs_menu] {
        assert!(!menu.has_frame(), "profile tools are frameless");
        assert!(menu.tooltip_text().is_some());
    }
    eprintln!("GTK page toolbar hierarchy assertions executed");
}

#[test]
fn header_states_connection_in_plain_language() {
    run_gtk_test(assert_header_connection_states);
}

fn assert_header_connection_states() {
    let Some(fixture) = fixture("header", |_| {}) else {
        return;
    };
    let widgets = &fixture.widgets;
    let (status, detail) = (&widgets.status, &widgets.connection_detail);
    let assert_state = |badge: &str, tone: &str, text: &str, tooltip: &str| {
        assert_eq!(status.text(), badge);
        for class in ["status-idle", "status-online", "status-error"] {
            assert_eq!(
                status.has_css_class(class),
                class == tone,
                "{badge}: {class}"
            );
        }
        assert_eq!(detail.text(), text);
        assert!(
            !detail.text().contains("VID"),
            "technical IDs stay out of the text"
        );
        for label in [status, detail] {
            let hint = label.tooltip_text().unwrap_or_default();
            assert!(
                hint.contains(tooltip),
                "{badge} tooltip {hint:?} lacks {tooltip:?}"
            );
        }
    };
    let not_connected = "Stream Deck + not connected — plug it in";
    // Before the HID worker reports anything, the header already speaks plainly.
    assert_state(
        "Not connected",
        "status-idle",
        not_connected,
        "VID 0FD9 / PID 0084",
    );
    for (state, badge, tone, text, tooltip) in [
        (
            ConnectionState::Searching,
            "Not connected",
            "status-idle",
            not_connected,
            "VID 0FD9 / PID 0084",
        ),
        (
            ConnectionState::Unavailable(
                "Device not found · connect Stream Deck + (VID 0FD9 / PID 0084)".into(),
            ),
            "Not connected",
            "status-idle",
            not_connected,
            "Device not found",
        ),
        (
            ConnectionState::Unavailable("Device found but could not open: denied".into()),
            "Unavailable",
            "status-error",
            "Stream Deck + found but unavailable — hover for details",
            "could not open: denied",
        ),
        (
            ConnectionState::Disconnected("HID read ended: gone".into()),
            "Disconnected",
            "status-idle",
            "Stream Deck + disconnected — plug it back in",
            "HID read ended: gone",
        ),
        (
            ConnectionState::Connected,
            "Connected",
            "status-online",
            "Stream Deck + connected — keys, dials and touch strip are live",
            "VID 0FD9 / PID 0084",
        ),
    ] {
        assert_eq!(
            show_connection_state(status, detail, &state),
            state == ConnectionState::Connected
        );
        assert_state(badge, tone, text, tooltip);
    }
    eprintln!("GTK plain-language connection header assertions executed");
}

#[test]
fn preview_dials_show_their_assigned_actions() {
    run_gtk_test(assert_dial_legends_and_touch_placeholder);
}

fn assert_dial_legends_and_touch_placeholder() {
    let volume = Action::Feature(FeatureAction::Desktop(
        sdeck_plus::desktop_actions::DesktopAction::Media(
            sdeck_plus::desktop_actions::MediaCommand::VolumeUp,
        ),
    ));
    let mute = Action::Shortcut {
        modifiers: vec!["CTRL".into()],
        key: "m".into(),
    };
    let Some(fixture) = fixture("dials", |store| {
        store
            .bind("Base", Trigger::EncoderClockwise(2), volume.clone())
            .unwrap();
        store
            .bind("Base", Trigger::EncoderPress(3), mute.clone())
            .unwrap();
    }) else {
        return;
    };
    let widgets = &fixture.widgets;
    let legends = &widgets.binding_ui.dial_legends;
    assert_eq!(legends.len(), 4);
    assert!(!legends[0].is_visible(), "unassigned dials stay unlabeled");
    assert!(!legends[3].is_visible());
    let volume_text = match &volume {
        Action::Feature(action) => action.summary(),
        _ => unreachable!(),
    };
    assert_eq!(legends[1].text(), volume_text);
    assert_eq!(legends[2].text(), "Shortcut CTRL+M");
    let content = widgets.root.child().unwrap();
    for (index, knob) in widgets.encoder_widgets.iter().enumerate() {
        let legend = &legends[index];
        assert!(legend.has_css_class("dial-legend"));
        // Same parent rail as the knob, centered underneath it.
        assert_eq!(legend.parent(), knob.parent());
        if !legend.is_visible() {
            continue;
        }
        assert_eq!(legend.tooltip_text(), Some(legend.text()));
        let knob = knob.compute_bounds(&content).unwrap();
        let below = legend.compute_bounds(&content).unwrap();
        assert!(
            below.y() >= knob.y() + knob.height(),
            "legend sits below the dial"
        );
        assert!(
            (below.x() + below.width() / 2.0 - knob.x() - knob.width() / 2.0).abs() <= 1.0,
            "legend is centered under its dial: {below:?} {knob:?}"
        );
        let rail = legend.parent().unwrap().compute_bounds(&content).unwrap();
        assert!(below.y() + below.height() <= rail.y() + rail.height());
    }

    // Removing the binding clears the legend on the next summary refresh.
    fixture
        .layers
        .borrow_mut()
        .unbind("Base", &Trigger::EncoderClockwise(2))
        .unwrap();
    widgets.binding_ui.refresh_summaries(&fixture.layers);
    assert!(!legends[1].is_visible());
    assert_eq!(legends[2].text(), "Shortcut CTRL+M");
    assert_eq!(
        widgets.touch_status.text(),
        "Touch strip · waiting for input"
    );
    assert!(widgets.touch_status.has_css_class("touch-display"));
    eprintln!("GTK dial legend and touch placeholder assertions executed");
}
