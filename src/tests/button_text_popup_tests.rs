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
        rendered_text_popup(&editor.panel, || editor.open.popup());
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
    assert_eq!(editor.open.tooltip_text().as_deref(), Some("Add text"));
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
    assert_eq!(editor.open.tooltip_text().as_deref(), Some("Edit text"));
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
    assert_eq!(editor.open.tooltip_text().as_deref(), Some("Add text"));
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
