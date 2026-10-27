//! Main-window hierarchy and legibility contract (feature: UI refresh).
use super::*;

struct Fixture {
    root: std::path::PathBuf,
    window: ApplicationWindow,
    widgets: MonitorWidgets,
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
    let widgets = build_widgets(window.clone(), layers, artwork, commands);
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
