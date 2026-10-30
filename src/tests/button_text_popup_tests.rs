//! Button text popover: context line, save state, shortcut, position grid and
//! preview background (feature: button text popup).
use super::*;
use sdeck_plus::button_text::ButtonText;

struct PopupFixture {
    root: std::path::PathBuf,
    window: ApplicationWindow,
    artwork: ArtworkUi,
    widgets: MonitorWidgets,
}

impl Drop for PopupFixture {
    fn drop(&mut self) {
        self.widgets._text_editor.panel.popdown();
        self.window.destroy();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl PopupFixture {
    fn editor(&self) -> &button_text_editor::ButtonTextEditor {
        &self.widgets._text_editor
    }

    /// Select a key through its production button and open the popup.
    fn open(&self, key: usize) {
        key_button(&self.widgets.key_labels[key - 1]).emit_clicked();
        let editor = self.editor();
        rendered_text_popup(&editor.panel, || editor.popup());
        assert!(editor.panel.is_mapped(), "text popup must be mapped");
    }

    fn saved(&self, key: u8) -> Option<ButtonText> {
        self.artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .button_text_in_layer("Base", ArtworkTarget::Keypad(key))
            .cloned()
    }
}

fn popup_fixture(
    tag: &str,
    setup: impl FnOnce(&mut LayerStore, &mut ArtworkStore, &std::path::Path),
) -> Option<PopupFixture> {
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
    let (root, mut store, mut images, source) = artwork_fixture(tag);
    store.select("Base");
    setup(&mut store, &mut images, &source);
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
    let widgets = build_widgets(window.clone(), layers, artwork.clone(), commands);
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(DEFAULT_WIDTH, DEFAULT_HEIGHT);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&widgets.root);
    artwork.refresh(true);
    // Keep shared-desktop focus changes from dismissing the popup under test.
    widgets._text_editor.panel.set_autohide(false);
    Some(PopupFixture {
        root,
        window,
        artwork,
        widgets,
    })
}

#[test]
fn button_text_popup_names_the_edited_key_and_page() {
    run_gtk_test(assert_popup_context);
}

fn assert_popup_context() {
    let Some(fixture) = popup_fixture("text-context", |_, _, _| {}) else {
        return;
    };
    let editor = fixture.editor();
    // Given: key 3 selected on the default profile's first page.
    fixture.open(3);
    // Then: the context line names the key and the page indicator's view.
    assert_eq!(editor.context_line.text(), "Key 3 on “Base / Page 1”");
    assert!(editor.context_line.is_mapped());
    // When: the page indicator names the current view (profile / folder / page).
    fixture
        .widgets
        .feature_runtime
        .indicator
        .set_tooltip_text(Some("Everyday / Page 2"));
    // Then: the open popup follows that name.
    assert_eq!(editor.context_line.text(), "Key 3 on “Everyday / Page 2”");
    editor.panel.popdown();
    fixture.open(5);
    assert_eq!(editor.context_line.text(), "Key 5 on “Everyday / Page 2”");
}

#[test]
fn button_text_save_and_remove_follow_the_saved_state() {
    run_gtk_test(assert_save_state);
}

fn assert_save_state() {
    let Some(fixture) = popup_fixture("text-save-state", |_, _, _| {}) else {
        return;
    };
    let editor = fixture.editor();
    // Given: a key without saved text.
    fixture.open(1);
    assert_eq!(editor.caption(), "Add text");
    assert!(!editor.save.is_sensitive(), "nothing to save yet");
    assert!(!editor.remove.is_visible(), "no saved text to remove");
    assert!(!editor.unsaved.is_visible());
    // When: text is typed, then cleared or blanked.
    editor.input.buffer().set_text("Play");
    assert!(editor.save.is_sensitive());
    assert!(editor.unsaved.is_visible());
    assert_eq!(editor.unsaved.text(), "Unsaved changes");
    for blank in ["", "  \n "] {
        editor.input.buffer().set_text(blank);
        assert!(
            !editor.save.is_sensitive(),
            "blank text {blank:?} is invalid"
        );
    }
    editor.input.buffer().set_text("");
    assert!(
        !editor.unsaved.is_visible(),
        "the default form is unchanged"
    );
    // An insensitive Save never writes, even when its signal is emitted.
    editor.save.emit_clicked();
    assert_eq!(fixture.saved(1), None);
    // When: valid text is saved.
    editor.input.buffer().set_text("Play");
    editor.save.emit_clicked();
    assert_eq!(fixture.saved(1).unwrap().text, "Play");
    assert!(!editor.panel.is_visible());
    // Then: reopening shows the saved value as unchanged and offers Remove.
    fixture.open(1);
    assert_eq!(editor.caption(), "Edit text");
    assert!(!editor.save.is_sensitive(), "the saved value is unchanged");
    assert!(editor.remove.is_visible() && editor.remove.is_sensitive());
    assert!(!editor.unsaved.is_visible());
    let size = editor.size.value();
    editor.size.set_value(size + 2.0);
    assert!(editor.save.is_sensitive());
    assert!(editor.unsaved.is_visible());
    editor.size.set_value(size);
    assert!(
        !editor.save.is_sensitive(),
        "reverting restores the saved state"
    );
    assert!(!editor.unsaved.is_visible());
    // When: the saved text is removed.
    editor.remove.emit_clicked();
    assert_eq!(
        fixture.saved(1).map(|text| text.text).unwrap_or_default(),
        ""
    );
    fixture.open(1);
    assert!(!editor.remove.is_visible());
    assert_eq!(editor.caption(), "Add text");
    // Another key keeps its own state.
    editor.input.buffer().set_text("Draft");
    fixture.open(2);
    assert!(!editor.save.is_sensitive());
    assert!(!editor.unsaved.is_visible());
}

#[test]
fn ctrl_enter_saves_button_text() {
    run_gtk_test(assert_ctrl_enter_saves);
}

fn assert_ctrl_enter_saves() {
    let Some(fixture) = popup_fixture("text-shortcut", |_, _, _| {}) else {
        return;
    };
    let editor = fixture.editor();
    fixture.open(4);
    let trigger = editor.save_shortcut.trigger().unwrap();
    // Plain Enter keeps inserting a new line; only Ctrl+Enter saves.
    assert_eq!(trigger.to_str(), "<Control>Return|<Control>KP_Enter");
    let activate = || {
        editor.save_shortcut.action().unwrap().activate(
            gtk::ShortcutActionFlags::empty(),
            &editor.panel,
            None,
        )
    };
    // Unchanged forms do not save.
    assert!(activate());
    assert_eq!(fixture.saved(4), None);
    editor.input.buffer().set_text("Go\nNow");
    assert!(activate());
    assert_eq!(fixture.saved(4).unwrap().text, "Go\nNow");
    assert!(!editor.panel.is_visible());
    let controller = editor
        .panel
        .observe_controllers()
        .into_iter()
        .filter_map(|item| item.ok()?.downcast::<gtk::ShortcutController>().ok())
        .find(|controller| {
            controller.item(0).and_downcast::<gtk::Shortcut>().as_ref()
                == Some(&editor.save_shortcut)
        })
        .expect("the popup owns its save shortcut controller");
    assert_eq!(
        controller.propagation_phase(),
        gtk::PropagationPhase::Capture
    );
}

fn preview_pixels(editor: &button_text_editor::ButtonTextEditor) -> Vec<u8> {
    let texture = editor
        .preview
        .paintable()
        .unwrap()
        .downcast::<gtk::gdk::Texture>()
        .unwrap();
    assert_eq!((texture.width(), texture.height()), (120, 120));
    let mut pixels = vec![0; 120 * 120 * 4];
    texture.download(&mut pixels, 120 * 4);
    pixels
}

fn assert_same_pixels(actual: &[u8], expected: &[u8], what: &str) {
    let differing = actual
        .iter()
        .zip(expected)
        .filter(|(actual, expected)| actual != expected)
        .count();
    assert!(
        actual.len() == expected.len() && differing == 0,
        "{what}: {differing} of {} bytes differ",
        expected.len()
    );
}

fn pixbuf_pixels(image: &gtk::gdk_pixbuf::Pixbuf) -> Vec<u8> {
    let texture = gtk::gdk::Texture::for_pixbuf(image);
    let mut pixels = vec![0; 120 * 120 * 4];
    texture.download(&mut pixels, 120 * 4);
    pixels
}

#[test]
fn button_text_position_grid_maps_every_alignment() {
    run_gtk_test(assert_position_grid);
}

fn assert_position_grid() {
    use sdeck_plus::button_text::{HorizontalAlignment, TextOrientation, VerticalAlignment};
    let Some(fixture) = popup_fixture("text-position", |_, images, _| {
        images
            .set_button_text_in_layer(
                "Base",
                ArtworkTarget::Keypad(6),
                ButtonText {
                    text: "Saved".into(),
                    horizontal_alignment: HorizontalAlignment::Right,
                    vertical_alignment: VerticalAlignment::Top,
                    orientation: TextOrientation::Vertical,
                    ..ButtonText::default()
                },
            )
            .unwrap();
    }) else {
        return;
    };
    let editor = fixture.editor();
    fixture.open(1);
    let names = [
        "Top left",
        "Top center",
        "Top right",
        "Middle left",
        "Center",
        "Middle right",
        "Bottom left",
        "Bottom center",
        "Bottom right",
    ];
    let horizontal = [
        HorizontalAlignment::Left,
        HorizontalAlignment::Center,
        HorizontalAlignment::Right,
    ];
    let vertical = [
        VerticalAlignment::Top,
        VerticalAlignment::Center,
        VerticalAlignment::Bottom,
    ];
    // Then: the default value selects the center cell.
    assert!(editor.position[4].is_active());
    editor.input.buffer().set_text("Go");
    for (index, button) in editor.position.iter().enumerate() {
        assert!(button.label().is_none(), "position cells are icon-only");
        assert!(button.child().is_some());
        assert_eq!(button.tooltip_text().as_deref(), Some(names[index]));
        assert!(
            button.is_focusable(),
            "position cells are keyboard reachable"
        );
        // When: a cell is chosen.
        button.emit_clicked();
        // Then: exactly that cell is active and the value maps row x column.
        let active: Vec<_> = editor
            .position
            .iter()
            .enumerate()
            .filter(|(_, button)| button.is_active())
            .map(|(index, _)| index)
            .collect();
        assert_eq!(active, vec![index]);
        let value = editor.value().unwrap();
        assert_eq!(value.horizontal_alignment, horizontal[index % 3]);
        assert_eq!(value.vertical_alignment, vertical[index / 3]);
        wait_for_frame(&editor.panel);
        assert!(
            editor.panel.is_visible(),
            "choosing {} keeps the popup open",
            names[index]
        );
    }
    // The grid shares one row with the orientation pair.
    let grid = editor.position[0].parent().unwrap();
    let orientation = editor.orientation[0].parent().unwrap();
    assert_eq!(
        grid.parent().and_then(|field| field.parent()),
        orientation.parent().and_then(|field| field.parent()),
        "orientation and position share one compact row"
    );
    for (button, name) in editor.orientation.iter().zip(["Horizontal", "Vertical"]) {
        assert!(
            button.label().is_none(),
            "orientation toggles are icon-only"
        );
        assert!(button
            .tooltip_text()
            .is_some_and(|tooltip| tooltip.starts_with(name)));
    }
    assert!(orientation.has_css_class("linked"));
    // A saved value activates its own cell and orientation.
    editor.panel.popdown();
    fixture.open(6);
    assert!(editor.position[2].is_active());
    assert!(editor.orientation[1].is_active());
}

#[test]
fn button_text_preview_uses_the_keys_real_background() {
    run_gtk_test(assert_preview_background);
}

fn assert_preview_background() {
    use sdeck_plus::binding_icons::BindingIcon;
    use sdeck_plus::button_text::VerticalAlignment;
    let action = Action::Command("make".into());
    let bound = action.clone();
    let Some(fixture) = popup_fixture("text-background", move |store, images, source| {
        store.bind("Base", Trigger::KeyPress(2), bound).unwrap();
        images
            .assign_in_layer("Base", ArtworkTarget::Keypad(1), source)
            .unwrap();
    }) else {
        return;
    };
    let editor = fixture.editor();
    let artwork_root = fixture.root.join("artwork");
    let files = || {
        let mut files: Vec<_> = walk_files(&artwork_root);
        files.sort();
        files
    };
    let before = files();
    // Given: key 1 with artwork.
    fixture.open(1);
    editor.input.buffer().set_text("Art");
    let value = editor.value().unwrap();
    let path = fixture
        .artwork
        .store
        .borrow()
        .as_ref()
        .unwrap()
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(1));
    assert!(path.is_some());
    assert_same_pixels(
        &preview_pixels(editor),
        &pixbuf_pixels(
            &sdeck_plus::image_output::compose_button_image(path.as_deref(), &value).unwrap(),
        ),
        "text over the key's artwork",
    );
    editor.panel.popdown();
    // Given: key 2 without artwork but with a binding icon.
    fixture.open(2);
    let icon = BindingIcon::for_action(&action);
    let caption = sdeck_plus::binding_editor::action_caption(&action);
    let empty = editor.value().unwrap();
    assert_same_pixels(
        &preview_pixels(editor),
        &pixbuf_pixels(
            &sdeck_plus::binding_icons::compose_key_image(icon, &caption, Some(&empty))
                .unwrap()
                .unwrap(),
        ),
        "an empty draft shows the key's icon tile",
    );
    editor.input.buffer().set_text("Build");
    editor.position[1].emit_clicked();
    let top = editor.value().unwrap();
    assert_eq!(top.vertical_alignment, VerticalAlignment::Top);
    assert_same_pixels(
        &preview_pixels(editor),
        &pixbuf_pixels(
            &sdeck_plus::binding_icons::compose_key_image(icon, &caption, Some(&top))
                .unwrap()
                .unwrap(),
        ),
        "top text sits above the key's icon, as on the device",
    );
    // Centered text leaves no room for the icon: the neutral key background.
    editor.position[4].emit_clicked();
    let centered = editor.value().unwrap();
    assert_same_pixels(
        &preview_pixels(editor),
        &pixbuf_pixels(&sdeck_plus::image_output::compose_button_image(None, &centered).unwrap()),
        "centered text leaves the neutral key background",
    );
    // Given: key 3 with neither artwork nor binding.
    editor.panel.popdown();
    fixture.open(3);
    editor.input.buffer().set_text("Plain");
    editor.position[1].emit_clicked();
    let plain = editor.value().unwrap();
    assert_same_pixels(
        &preview_pixels(editor),
        &pixbuf_pixels(&sdeck_plus::image_output::compose_button_image(None, &plain).unwrap()),
        "an unbound key without artwork is neutral",
    );
    // Derived previews are never written into the artwork store.
    assert_eq!(files(), before);
}

fn walk_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

#[test]
fn button_text_overflow_warns_without_blocking_save() {
    run_gtk_test(assert_overflow_warning);
}

fn assert_overflow_warning() {
    let Some(fixture) = popup_fixture("text-overflow", |_, _, _| {}) else {
        return;
    };
    let editor = fixture.editor();
    fixture.open(1);
    assert!(!editor.overflow.is_visible(), "an empty draft fits");
    // Given: short text at a small size.
    editor.input.buffer().set_text("Mute");
    editor.size.set_value(18.0);
    assert!(!editor.overflow.is_visible());
    // When: the size makes it wider than the key.
    editor.size.set_value(60.0);
    // Then: an inline warning appears, and Save is still allowed.
    assert!(editor.overflow.is_visible());
    assert!(editor.overflow.is_mapped());
    assert_eq!(
        editor.overflow.text(),
        "Text does not fit; try a smaller size"
    );
    assert!(editor.overflow.has_css_class("feedback-warning"));
    assert!(editor.save.is_sensitive(), "overflow is informational");
    // The key is square, so rotating overflowing text keeps the warning.
    editor.orientation[1].emit_clicked();
    assert!(editor.overflow.is_visible());
    assert!(!button_text_editor::text_fits(&editor.value().unwrap()));
    editor.orientation[0].emit_clicked();
    editor.size.set_value(24.0);
    editor
        .input
        .buffer()
        .set_text("One\nTwo\nThree\nFour\nFive");
    editor.orientation[0].emit_clicked();
    assert!(
        editor.overflow.is_visible(),
        "five lines are too tall at 24px"
    );
    editor.size.set_value(12.0);
    assert!(!editor.overflow.is_visible());
    // Saving keeps the overflowing text as typed.
    editor.size.set_value(60.0);
    editor.save.emit_clicked();
    assert_eq!(fixture.saved(1).unwrap().font_size, 60);
}

fn all_buttons(widget: &gtk::Widget, found: &mut Vec<Button>) {
    if let Some(button) = widget.downcast_ref::<Button>() {
        found.push(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        all_buttons(&current, found);
        child = current.next_sibling();
    }
}

fn button_named(widget: &gtk::Widget, caption: &str) -> Option<Button> {
    let mut buttons = Vec::new();
    all_buttons(widget, &mut buttons);
    buttons
        .into_iter()
        .find(|button| button_caption(button).as_deref() == Some(caption))
}

fn right_click(widget: &gtk::Widget) {
    let controllers = widget.observe_controllers();
    let gesture = (0..controllers.n_items())
        .filter_map(|index| {
            controllers
                .item(index)?
                .downcast::<gtk::GestureClick>()
                .ok()
        })
        .find(|gesture| gesture.button() == 3)
        .expect("right-click gesture");
    gesture.emit_by_name::<()>("pressed", &[&1i32, &4.0f64, &4.0f64]);
}

/// Wait for the real map signal, never a fixed sleep.
fn wait_until_mapped(panel: &gtk::Popover) {
    if panel.is_mapped() {
        return;
    }
    let main_loop = glib::MainLoop::new(None, false);
    let done = main_loop.clone();
    let signal = panel.connect_map(move |_| done.quit());
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let done = main_loop.clone();
    let guard = glib::timeout_add_local_once(Duration::from_secs(5), move || {
        flag.set(true);
        done.quit();
    });
    main_loop.run();
    panel.disconnect(signal);
    if !expired.get() {
        guard.remove();
    }
    assert!(panel.is_mapped(), "text and image modal never opened");
}

#[test]
fn right_clicking_a_key_opens_the_text_and_image_modal_on_that_key() {
    run_gtk_test(assert_right_click_opens_modal);
}

fn assert_right_click_opens_modal() {
    let Some(fixture) = popup_fixture("key-context-menu", |_, _, _| {}) else {
        return;
    };
    let editor = fixture.editor();
    let key = fixture.widgets.key_widgets[2].clone();
    assert!(!editor.panel.is_visible());
    right_click(&key);
    // The menu itself must really appear, not only exist in the widget tree.
    let menu = {
        let mut popovers = Vec::new();
        collect_popovers(key.upcast_ref(), &mut popovers);
        popovers.into_iter().next().expect("context menu popover")
    };
    wait_until_mapped(&menu);
    let entry = button_named(&key, "Add text and image…").expect("context menu entry");
    entry.emit_clicked();
    wait_until_mapped(&editor.panel);
    assert_eq!(editor.panel.parent().as_ref(), Some(&key));
    assert_eq!(editor.context_line.text(), "Key 3 on “Base / Page 1”");
    if let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        rendered_text_popup(&editor.panel, || {})
            .save_to_png(std::path::Path::new(&directory).join("key-modal.png"))
            .unwrap();
    }
}

#[test]
fn appearance_section_is_hidden_for_keys_and_kept_for_touch_sections() {
    run_gtk_test(assert_appearance_hidden_for_keys);
}

fn assert_appearance_hidden_for_keys() {
    let Some(fixture) = popup_fixture("key-appearance-hidden", |_, _, _| {}) else {
        return;
    };
    let widgets = &fixture.widgets;
    let appearance = widgets.artwork_section.parent().unwrap();
    assert!(!appearance.is_visible(), "keys are styled from their menu");
    widgets.zone_widgets[0]
        .clone()
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
    assert!(appearance.is_visible(), "touch sections still need it");
    key_button(&widgets.key_labels[0]).emit_clicked();
    assert!(!appearance.is_visible());
}

#[test]
fn the_modal_chooses_and_clears_the_keys_background_image() {
    run_gtk_test(assert_modal_image_controls);
}

fn assert_modal_image_controls() {
    let Some(fixture) = popup_fixture("key-modal-image", |_, images, source| {
        images
            .assign_in_layer("Base", ArtworkTarget::Keypad(1), source)
            .unwrap();
    }) else {
        return;
    };
    fixture.open(1);
    let panel = fixture.editor().panel.clone();
    let choose = button_named(panel.upcast_ref(), "Image…").expect("image chooser in the modal");
    let clear = button_named(panel.upcast_ref(), "Clear image").expect("image clearing");
    assert!(choose.is_sensitive());
    assert!(clear.is_sensitive(), "a key with an image can clear it");
    clear.emit_clicked();
    let assigned = fixture
        .artwork
        .store
        .borrow()
        .as_ref()
        .unwrap()
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(1));
    assert!(assigned.is_none(), "image must be removed from storage");
    assert!(!clear.is_sensitive(), "nothing left to clear");
}

#[test]
fn appearance_section_is_hidden_for_dials() {
    run_gtk_test(assert_appearance_hidden_for_dials);
}

fn assert_appearance_hidden_for_dials() {
    let Some(fixture) = popup_fixture("dial-appearance-hidden", |_, _, _| {}) else {
        return;
    };
    let widgets = &fixture.widgets;
    let appearance = widgets.artwork_section.parent().unwrap();
    widgets.encoder_widgets[1]
        .clone()
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
    assert_eq!(widgets.binding_ui.control_kind(), ControlKind::Encoder);
    assert!(!appearance.is_visible(), "dials have no appearance to edit");
    widgets.zone_widgets[0]
        .clone()
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
    assert!(appearance.is_visible(), "touch sections keep it");
}

fn collect_popovers(widget: &gtk::Widget, found: &mut Vec<gtk::Popover>) {
    if let Some(popover) = widget.downcast_ref::<gtk::Popover>() {
        found.push(popover.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect_popovers(&current, found);
        child = current.next_sibling();
    }
}
