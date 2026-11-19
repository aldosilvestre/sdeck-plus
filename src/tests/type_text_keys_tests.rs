//! Type text keys: the Insert key buttons under the Text field put a key token such as {Tab} at
//! the cursor, a muted line previews what is typed, and the value is saved with its tokens, in
//! the binding editor and in a multi action step. Editing never runs the action.
use super::dial_action_tests::catalog_row;
use super::*;
use sdeck_plus::{
    desktop_actions::DesktopAction,
    features::{FeatureAction, MultiStep},
};

/// Production widgets whose layer commands the test keeps.
struct Fixture {
    root: std::path::PathBuf,
    window: ApplicationWindow,
    widgets: MonitorWidgets,
    layers: Rc<RefCell<LayerStore>>,
    requests: mpsc::Receiver<LayerCommand>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.window.destroy();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fixture(tag: &str) -> Option<Fixture> {
    if !init_gtk_for_test() {
        return None;
    }
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let (root, store, images, _) = artwork_fixture(tag);
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
        .default_width(DEFAULT_WIDTH)
        .default_height(DEFAULT_HEIGHT)
        .build();
    install_style(&window);
    let widgets = build_widgets(window.clone(), layers.clone(), artwork, commands);
    window.set_child(Some(&widgets.root));
    window.present();
    wait_for_frame(&window);
    while requests.try_recv().is_ok() {}
    key_button(&widgets.key_labels[0]).emit_clicked();
    Some(Fixture {
        root,
        window,
        widgets,
        layers,
        requests,
    })
}

impl Fixture {
    fn ui(&self) -> &BindingUi {
        &self.widgets.binding_ui
    }

    fn save(&self) {
        let mut buttons = Vec::new();
        collect_layout_buttons(self.widgets.root.upcast_ref(), &mut buttons);
        buttons
            .into_iter()
            .find(|button| button_caption(button).as_deref() == Some("Save binding"))
            .expect("Save binding")
            .emit_clicked();
    }

    /// Key 1's binding once the saved snapshot is written and read back from disk; nothing the
    /// editor sent may run an action.
    fn saved(&self) -> Option<Action> {
        let commands: Vec<_> = self.requests.try_iter().collect();
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                LayerCommand::Execute { .. } | LayerCommand::ExecuteStep { .. }
            )),
            "editing or saving must never run an action"
        );
        let snapshot = commands
            .into_iter()
            .rev()
            .find_map(|command| match command {
                LayerCommand::SaveBinding(snapshot, _) => Some(snapshot),
                _ => None,
            })
            .expect("saving enqueues the store");
        snapshot.persist().unwrap();
        LayerStore::open_at(snapshot.root())
            .unwrap()
            .binding("Base", &Trigger::KeyPress(1))
    }
}

fn click_key(editor: &feature_editor::FeatureEditor, caption: &str) {
    editor
        .text_key_buttons()
        .into_iter()
        .find(|button| button.label().as_deref() == Some(caption))
        .unwrap_or_else(|| panic!("Insert key button {caption}"))
        .emit_clicked();
}

fn contents(buffer: &gtk::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

fn check_button(widget: &gtk::Widget, label: &str) -> Option<gtk::CheckButton> {
    if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
        if check.label().as_deref() == Some(label) {
            return Some(check.clone());
        }
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = check_button(&current, label) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

fn typed(value: &str, enter: bool) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Text {
        value: value.into(),
        enter,
    }))
}

#[test]
fn insert_key_buttons_put_tokens_at_the_cursor_preview_them_and_save_them() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("type-text-keys") else {
            return;
        };
        let ui = fixture.ui();
        let editor = &ui.feature_editor;
        catalog_row(&fixture.widgets, "Open website").emit_clicked();
        wait_for_frame(&fixture.window);
        let width = editor.root.width();
        catalog_row(&fixture.widgets, "Type text").emit_clicked();
        // Given the Insert key row under the Text field, one compact button per key.
        let buttons = editor.text_key_buttons();
        let captions: Vec<_> = buttons
            .iter()
            .map(|button| button.label().unwrap().to_string())
            .collect();
        assert_eq!(captions, ["Enter", "Tab", "Esc", "↑", "↓", "←", "→"]);
        for (button, (token, name)) in buttons.iter().zip([
            ("{Enter}", "Enter"),
            ("{Tab}", "Tab"),
            ("{Esc}", "Escape"),
            ("{Up}", "Up arrow"),
            ("{Down}", "Down arrow"),
            ("{Left}", "Left arrow"),
            ("{Right}", "Right arrow"),
        ]) {
            assert!(button.is_mapped(), "{token} is shown with the Text field");
            assert_eq!(
                button.tooltip_text().unwrap(),
                format!("Insert {token}\nPresses {name} at this point of the text.")
            );
        }
        let preview = editor.text_preview();
        // The row fits the editor at the default window size without widening it.
        wait_for_frame(&fixture.window);
        assert_eq!(editor.root.width(), width);
        assert!(!preview.is_visible(), "nothing typed, nothing previewed");
        let buffer = editor.text_buffer();
        buffer.set_text("aldo");
        assert!(preview.is_visible());
        assert_eq!(preview.text(), "Types: aldo");
        fixture.save();
        assert_eq!(fixture.saved(), Some(typed("aldo", false)));
        assert!(!ui.is_dirty());

        // When a key button is clicked with the cursor at the end of the text.
        buffer.place_cursor(&buffer.end_iter());
        click_key(editor, "Tab");
        // Then its token is inserted there and the edit is unsaved.
        assert_eq!(contents(&buffer), "aldo{Tab}");
        assert!(ui.is_dirty());
        assert_eq!(ui.status.text(), "Unsaved changes.");
        buffer.insert_at_cursor("mi-clave");
        click_key(editor, "Enter");
        assert_eq!(contents(&buffer), "aldo{Tab}mi-clave{Enter}");
        assert_eq!(preview.text(), "Types: aldo ⇥ mi-clave ⏎");
        // Press Enter after text adds a final Enter to the preview.
        let enter = check_button(editor.root.upcast_ref(), "Press Enter after text")
            .expect("Press Enter after text");
        enter.set_active(true);
        assert_eq!(preview.text(), "Types: aldo ⇥ mi-clave ⏎ ⏎");

        // A token goes at the cursor, even in the middle of the text.
        buffer.place_cursor(&buffer.iter_at_offset(2));
        click_key(editor, "Esc");
        assert_eq!(contents(&buffer), "al{Esc}do{Tab}mi-clave{Enter}");
        assert_eq!(buffer.cursor_position(), 7, "the cursor follows the token");
        // And replaces the selection, as typing would.
        buffer.select_range(&buffer.iter_at_offset(2), &buffer.iter_at_offset(7));
        for caption in ["↑", "↓", "←", "→"] {
            click_key(editor, caption);
        }
        assert_eq!(
            contents(&buffer),
            "al{Up}{Down}{Left}{Right}do{Tab}mi-clave{Enter}"
        );
        assert_eq!(preview.text(), "Types: al ↑ ↓ ← → do ⇥ mi-clave ⏎ ⏎");

        // Saving stores the value with its tokens, as written.
        buffer.set_text("aldo{Tab}mi-clave{Enter}");
        fixture.save();
        assert_eq!(
            fixture.saved(),
            Some(typed("aldo{Tab}mi-clave{Enter}", true))
        );
        assert!(!ui.is_dirty());
        // Reloading the key shows the same text and preview.
        ui.load_selected_binding(&fixture.layers);
        assert_eq!(contents(&buffer), "aldo{Tab}mi-clave{Enter}");
        assert_eq!(preview.text(), "Types: aldo ⇥ mi-clave ⏎ ⏎");
        assert!(!ui.is_dirty());
    });
}

#[test]
fn a_multi_action_step_inserts_keys_and_keeps_the_tokens() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("type-text-keys-step") else {
            return;
        };
        let ui = fixture.ui();
        catalog_row(&fixture.widgets, "Multi action").emit_clicked();
        let list = ui
            .feature_editor
            .steps()
            .expect("the binding editor owns a step list")
            .clone();
        // Given a Type text step in the Add step dialog.
        list.add_step.emit_clicked();
        let dialog = list.dialog().expect("Add step opens the dialog");
        dialog.form.picker.set_selected(5);
        let step = &dialog.form.feature;
        step.populate(&FeatureAction::Desktop(DesktopAction::Text {
            value: "aldo".into(),
            enter: false,
        }));
        wait_for_frame(&dialog.window);
        assert!(step
            .text_key_buttons()
            .iter()
            .all(|button| button.is_mapped()));
        // The row fits the 400px step dialog without widening it.
        let (minimum, _, _, _) = dialog.window.measure(gtk::Orientation::Horizontal, -1);
        assert!(minimum <= 400, "the step dialog needs {minimum}px");
        // When keys are inserted around more text.
        let buffer = step.text_buffer();
        buffer.place_cursor(&buffer.end_iter());
        click_key(step, "Tab");
        buffer.insert_at_cursor("mi-clave");
        click_key(step, "Enter");
        // Then the step previews and keeps them.
        assert_eq!(contents(&buffer), "aldo{Tab}mi-clave{Enter}");
        assert_eq!(step.text_preview().text(), "Types: aldo ⇥ mi-clave ⏎");
        dialog.primary.emit_clicked();
        assert!(!dialog.window.is_visible());
        let expected = MultiStep::Action(typed("aldo{Tab}mi-clave{Enter}", false));
        assert_eq!(list.steps(), std::slice::from_ref(&expected));
        assert!(ui.is_dirty(), "a new step is an unsaved edit");
        fixture.save();
        assert_eq!(
            fixture.saved(),
            Some(Action::Feature(FeatureAction::Multi {
                steps: vec![expected]
            }))
        );
    });
}
