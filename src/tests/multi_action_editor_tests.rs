//! Multi action editor (feature task T4): the numbered step list in More actions, the
//! Add/Edit step dialog, waits, Save and the dedicated icon. Editing a step never runs it.
use super::dial_action_tests::{catalog_row, choose_event, drop_on_dial};
use super::*;
use sdeck_plus::{
    binding_icons::BindingIcon,
    desktop_actions::DesktopAction,
    features::{FeatureAction, MultiStep},
};

fn multi(steps: Vec<MultiStep>) -> Action {
    Action::Feature(FeatureAction::Multi { steps })
}

fn step(action: Action) -> MultiStep {
    MultiStep::Action(action)
}

fn wait(milliseconds: u32) -> MultiStep {
    MultiStep::Wait { milliseconds }
}

/// Production widgets whose layer commands and device pictures the test keeps.
struct Editor {
    root: std::path::PathBuf,
    window: ApplicationWindow,
    widgets: MonitorWidgets,
    layers: Rc<RefCell<LayerStore>>,
    artwork: ArtworkUi,
    requests: mpsc::Receiver<LayerCommand>,
    hardware: mpsc::Receiver<ArtworkCommand>,
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.window.destroy();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn editor(tag: &str, setup: impl FnOnce(&mut LayerStore)) -> Option<Editor> {
    if !init_gtk_for_test() {
        return None;
    }
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let (root, mut store, images, _) = artwork_fixture(tag);
    setup(&mut store);
    store.persist().unwrap();
    let layers = Rc::new(RefCell::new(store));
    let (output, hardware) = mpsc::channel();
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
    let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), commands);
    window.set_child(Some(&widgets.root));
    window.present();
    wait_for_frame(&window);
    while requests.try_recv().is_ok() {}
    Some(Editor {
        root,
        window,
        widgets,
        layers,
        artwork,
        requests,
        hardware,
    })
}

impl Editor {
    fn ui(&self) -> &BindingUi {
        &self.widgets.binding_ui
    }

    fn list(&self) -> multi_editor::StepList {
        self.ui()
            .feature_editor
            .steps()
            .expect("the binding editor owns a step list")
            .clone()
    }

    fn dialog(&self) -> multi_editor::StepDialog {
        self.list().dialog().expect("a step dialog was opened")
    }

    fn select_key(&self, number: usize) {
        key_button(&self.widgets.key_labels[number - 1]).emit_clicked();
        assert_eq!(self.ui().control_kind(), ControlKind::Button);
        assert_eq!(self.ui().control_number(), Some(number as u8));
    }

    fn click(&self, caption: &str) {
        find_button(self.widgets.root.upcast_ref(), caption).emit_clicked();
    }

    fn save(&self) {
        self.click("Save binding");
    }

    fn toast(&self) -> String {
        self.widgets.toast.label.text().to_string()
    }

    fn summaries(&self) -> Vec<String> {
        self.list()
            .rows()
            .iter()
            .map(|row| row.summary.text().to_string())
            .collect()
    }

    /// Every command the editor sent since the last call; none of them may run an action.
    fn commands(&self) -> Vec<LayerCommand> {
        let commands: Vec<_> = self.requests.try_iter().collect();
        for command in &commands {
            assert!(
                !matches!(
                    command,
                    LayerCommand::Execute { .. } | LayerCommand::ExecuteStep { .. }
                ),
                "editing or saving must never run an action"
            );
        }
        commands
    }

    /// The stored binding after the saved snapshot is written and read back from disk.
    fn reloaded(&self, trigger: &Trigger) -> Option<Action> {
        let snapshot = self
            .commands()
            .into_iter()
            .rev()
            .find_map(|command| match command {
                LayerCommand::SaveBinding(snapshot, _) => Some(snapshot),
                _ => None,
            })
            .expect("saving enqueues the store");
        snapshot.persist().unwrap();
        let reloaded = LayerStore::open_at(self.root.join("layers")).unwrap();
        reloaded.binding("Base", trigger)
    }
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

fn find_button(widget: &gtk::Widget, caption: &str) -> Button {
    let mut buttons = Vec::new();
    all_buttons(widget, &mut buttons);
    buttons
        .into_iter()
        .find(|button| button_caption(button).as_deref() == Some(caption))
        .unwrap_or_else(|| panic!("missing button: {caption}"))
}

/// The texts of every label under `widget`, in order.
fn texts(widget: &gtk::Widget) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(label) = widget.downcast_ref::<Label>() {
        found.push(label.text().to_string());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        found.extend(texts(&current));
        child = current.next_sibling();
    }
    found
}

fn icon_ids(widget: &gtk::Widget) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![widget.clone()];
    while let Some(current) = pending.pop() {
        if let Some(area) = current.downcast_ref::<gtk::DrawingArea>() {
            if area.has_css_class("binding-icon") && area.is_visible() {
                found.push(area.widget_name().to_string());
            }
        }
        let mut child = current.last_child();
        while let Some(next) = child {
            child = next.prev_sibling();
            pending.push(next);
        }
    }
    found
}

fn entry_with_placeholder(widget: &gtk::Widget, placeholder: &str) -> Entry {
    let mut entries = Vec::new();
    collect_entries(widget, &mut entries);
    entries
        .into_iter()
        .find(|entry| entry.placeholder_text().as_deref() == Some(placeholder))
        .unwrap_or_else(|| panic!("entry {placeholder}"))
}

/// Opens Add step and fills the action form with the given type and text.
fn add_text_step(editor: &Editor, picker: u32, text: &str) {
    editor.list().add_step.emit_clicked();
    let dialog = editor.dialog();
    assert!(dialog.window.is_visible(), "Add step opens the dialog");
    dialog.form.picker.set_selected(picker);
    dialog.form.core.text_entry.set_text(text);
    dialog.primary.emit_clicked();
}

fn add_wait(editor: &Editor, value: &str, seconds: bool) {
    editor.list().add_wait.emit_clicked();
    let dialog = editor.dialog();
    dialog.wait_value.set_text(value);
    dialog.wait_unit.set_selected(u32::from(seconds));
    dialog.primary.emit_clicked();
}

#[test]
fn multi_action_is_offered_only_for_key_press_and_key_hold() {
    run_gtk_test(|| {
        let Some(editor) = editor("multi-offered", |_| {}) else {
            return;
        };
        let ui = editor.ui();
        assert!(
            ui.feature_editor.offered().contains(&"Multi action"),
            "a key press offers Multi action"
        );
        ui.hold_toggle.set_active(true);
        assert!(
            ui.feature_editor.offered().contains(&"Multi action"),
            "a key hold offers Multi action"
        );
        ui.hold_toggle.set_active(false);
        ui.select_control(ControlKind::Encoder, Some(1), true)
            .unwrap();
        for event in 0..3 {
            ui.event_picker.set_selected(event);
            assert!(
                !ui.feature_editor.offered().contains(&"Multi action"),
                "dial event {event} does not offer it"
            );
        }
        ui.select_control(ControlKind::Touch, Some(0), true)
            .unwrap();
        let touch = ui.feature_editor.offered();
        assert!(!touch.contains(&"Multi action"), "the touch strip does not");
        assert!(
            touch.contains(&"Next touchscreen page"),
            "touch keeps its own choices"
        );
        // Dropped on a dial it is refused and nothing is stored or sent.
        assert!(drop_on_dial(&editor.widgets, "Multi action", 0));
        choose_event(&editor.widgets, 0, "Press");
        assert_eq!(ui.control_kind(), ControlKind::Encoder);
        assert!(ui.feature_editor.parse().is_err());
        assert!(
            texts(ui.feature_editor.root.upcast_ref())
                .iter()
                .any(|text| text
                    .starts_with("Multi action runs only from a key press or a key hold.")),
            "the guidance says why nothing is selected"
        );
        assert!(!editor.list().root().is_mapped(), "no step list on a dial");
        editor.save();
        assert_eq!(
            editor
                .layers
                .borrow()
                .binding("Base", &Trigger::EncoderPress(1)),
            None
        );
        assert!(!editor
            .commands()
            .iter()
            .any(|command| matches!(command, LayerCommand::SaveBinding(..))));
        // On a key the catalog shows the empty step list as a draft (the dial draft is dropped).
        ui.select_control(ControlKind::Button, Some(1), true)
            .unwrap();
        assert_eq!(ui.control_kind(), ControlKind::Button);
        catalog_row(&editor.widgets, "Multi action").emit_clicked();
        assert_eq!(ui.action_kind(), ActionKind::Extended);
        assert!(
            editor.list().root().is_mapped(),
            "the step list replaces the fields"
        );
        assert!(editor.list().rows().is_empty());
        assert!(editor.list().add_step.is_mapped() && editor.list().add_wait.is_mapped());
        assert_eq!(
            editor
                .layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(1)),
            None
        );
        assert!(
            editor.commands().is_empty(),
            "choosing Multi writes nothing"
        );
    });
}

#[test]
fn adding_an_action_step_and_a_wait_then_saving_stores_the_exact_multi_action() {
    run_gtk_test(|| {
        let Some(editor) = editor("multi-add-save", |_| {}) else {
            return;
        };
        let ui = editor.ui();
        catalog_row(&editor.widgets, "Multi action").emit_clicked();
        assert!(ui.is_dirty(), "a new multi action is a draft");
        add_text_step(&editor, 1, "echo one");
        assert!(
            !editor.dialog().window.is_visible(),
            "Add closes the dialog"
        );
        add_wait(&editor, "2", true);
        add_text_step(&editor, 4, "lights");
        assert_eq!(
            editor.summaries(),
            ["Run echo one", "Wait 2 s", "Emit lights"]
        );
        let numbers: Vec<String> = editor
            .list()
            .rows()
            .iter()
            .map(|row| row.number.text().to_string())
            .collect();
        assert_eq!(numbers, ["1", "2", "3"]);
        assert!(ui.is_dirty());
        assert_eq!(
            editor
                .layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(1)),
            None
        );
        let expected = multi(vec![
            step(Action::Command("echo one".into())),
            wait(2000),
            step(Action::Emit("lights".into())),
        ]);
        editor.save();
        assert_eq!(
            editor
                .layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(1)),
            Some(expected.clone())
        );
        assert!(!ui.is_dirty(), "saving makes the list the new baseline");
        assert_eq!(editor.reloaded(&Trigger::KeyPress(1)), Some(expected));
    });
}

#[test]
fn up_down_and_remove_reorder_and_delete_steps() {
    run_gtk_test(|| {
        let saved = multi(vec![
            step(Action::Command("a".into())),
            wait(100),
            step(Action::Command("b".into())),
        ]);
        let Some(editor) = editor("multi-reorder", |store| {
            store.bind("Base", Trigger::KeyPress(2), saved).unwrap();
        }) else {
            return;
        };
        editor.select_key(2);
        let ui = editor.ui();
        assert!(!ui.is_dirty());
        let rows = editor.list().rows();
        assert!(!rows[0].up.is_sensitive(), "the first step cannot move up");
        assert!(
            !rows[2].down.is_sensitive(),
            "the last step cannot move down"
        );
        for (button, name) in [
            (&rows[0].down, "Move down"),
            (&rows[1].up, "Move up"),
            (&rows[1].edit, "Edit step"),
            (&rows[1].remove, "Remove step"),
        ] {
            let tip = button.tooltip_text().unwrap();
            let (title, explanation) = tip.split_once('\n').expect("two-line tooltip");
            assert_eq!(title, name);
            assert!(explanation.contains("step 2") || explanation.contains("step 1"));
            assert!(
                button.has_css_class("icon-only"),
                "{name} is an icon button"
            );
        }
        rows[0].down.emit_clicked();
        assert_eq!(editor.summaries(), ["Wait 100 ms", "Run a", "Run b"]);
        assert!(ui.is_dirty(), "reordering is an unsaved edit");
        editor.list().rows()[2].up.emit_clicked();
        assert_eq!(editor.summaries(), ["Wait 100 ms", "Run b", "Run a"]);
        editor.list().rows()[0].remove.emit_clicked();
        assert_eq!(editor.summaries(), ["Run b", "Run a"]);
        let numbers: Vec<String> = editor
            .list()
            .rows()
            .iter()
            .map(|row| row.number.text().to_string())
            .collect();
        assert_eq!(numbers, ["1", "2"]);
        editor.save();
        assert_eq!(
            editor.reloaded(&Trigger::KeyPress(2)),
            Some(multi(vec![
                step(Action::Command("b".into())),
                step(Action::Command("a".into())),
            ]))
        );
    });
}

#[test]
fn edit_loads_a_step_into_the_dialog_and_saves_its_changes() {
    run_gtk_test(|| {
        let Some(editor) = editor("multi-edit", |store| {
            store
                .bind(
                    "Base",
                    Trigger::KeyPress(2),
                    multi(vec![
                        step(Action::Command("echo one".into())),
                        wait(2000),
                        step(Action::Emit("lights".into())),
                    ]),
                )
                .unwrap();
        }) else {
            return;
        };
        editor.select_key(2);
        editor.list().rows()[0].edit.emit_clicked();
        let dialog = editor.dialog();
        assert!(dialog.window.is_visible());
        assert_eq!(dialog.title.text(), "Edit step");
        assert_eq!(button_caption(&dialog.primary).as_deref(), Some("Save"));
        assert_eq!(texts(dialog.cancel.upcast_ref()), ["Cancel", "Esc"]);
        assert!(dialog.form.root.is_visible() && !dialog.wait_box.is_visible());
        assert_eq!(dialog.form.picker.selected(), 1);
        assert_eq!(dialog.form.core.text_entry.text(), "echo one");
        dialog.form.core.text_entry.set_text("echo two");
        dialog.primary.emit_clicked();
        assert!(!dialog.window.is_visible());
        assert_eq!(editor.summaries()[0], "Run echo two");
        assert!(editor.ui().is_dirty(), "a step edit is an unsaved change");
        // A wait opens with its own number and unit.
        editor.list().rows()[1].edit.emit_clicked();
        let dialog = editor.dialog();
        assert_eq!(dialog.title.text(), "Edit wait");
        assert!(dialog.wait_box.is_visible() && !dialog.form.root.is_visible());
        assert_eq!(dialog.wait_value.text(), "2");
        assert_eq!(dialog.wait_unit.selected(), 1, "2000 ms shows as 2 s");
        dialog.wait_value.set_text("750");
        dialog.wait_unit.set_selected(0);
        dialog.primary.emit_clicked();
        assert_eq!(editor.summaries()[1], "Wait 750 ms");
        // Cancel keeps the step as it was.
        editor.list().rows()[2].edit.emit_clicked();
        let dialog = editor.dialog();
        assert_eq!(dialog.form.picker.selected(), 4);
        dialog.form.core.text_entry.set_text("other");
        dialog.cancel.emit_clicked();
        assert!(!dialog.window.is_visible());
        assert_eq!(editor.summaries()[2], "Emit lights");
        assert!(editor.commands().is_empty(), "editing steps sends nothing");
        editor.save();
        assert_eq!(
            editor.reloaded(&Trigger::KeyPress(2)),
            Some(multi(vec![
                step(Action::Command("echo two".into())),
                wait(750),
                step(Action::Emit("lights".into())),
            ]))
        );
    });
}

#[test]
fn a_saved_multi_action_fills_the_step_list_when_its_key_is_selected() {
    run_gtk_test(|| {
        let saved = multi(vec![
            step(Action::Command("echo one".into())),
            wait(2000),
            step(Action::Emit("lights".into())),
        ]);
        let held = multi(vec![step(Action::Emit("held".into()))]);
        let (stored, stored_hold) = (saved.clone(), held.clone());
        let Some(editor) = editor("multi-populate", move |store| {
            store.bind("Base", Trigger::KeyPress(2), stored).unwrap();
            store
                .bind("Base", Trigger::KeyHold(3), stored_hold)
                .unwrap();
        }) else {
            return;
        };
        let ui = editor.ui();
        editor.select_key(2);
        assert_eq!(ui.action_kind(), ActionKind::Extended);
        assert!(editor.list().root().is_mapped());
        assert_eq!(
            editor.summaries(),
            ["Run echo one", "Wait 2 s", "Emit lights"]
        );
        let icons: Vec<Vec<String>> = editor
            .list()
            .rows()
            .iter()
            .map(|row| icon_ids(row.root.upcast_ref()))
            .collect();
        assert_eq!(icons, [["command"], ["timer"], ["named-event"]]);
        assert!(!ui.is_dirty(), "loading leaves the editor clean");
        assert_eq!(ui.binding_from_widgets(&editor.layers).unwrap().2, saved);
        // Another key empties the list; the hold of key 3 shows its own steps.
        editor.select_key(1);
        assert!(!editor.list().root().is_mapped());
        assert!(editor.list().rows().is_empty());
        editor.select_key(3);
        ui.hold_toggle.set_active(true);
        assert_eq!(editor.summaries(), ["Emit held"]);
        assert_eq!(ui.binding_from_widgets(&editor.layers).unwrap().2, held);
        assert!(!ui.is_dirty());
    });
}

#[test]
fn save_refuses_an_invalid_step_list_and_stores_nothing() {
    run_gtk_test(|| {
        let Some(editor) = editor("multi-invalid", |store| {
            store
                .bind(
                    "Base",
                    Trigger::KeyPress(1),
                    Action::Command("original".into()),
                )
                .unwrap();
        }) else {
            return;
        };
        let ui = editor.ui();
        let original = Some(Action::Command("original".into()));
        catalog_row(&editor.widgets, "Multi action").emit_clicked();
        editor.save();
        assert_eq!(editor.toast(), "A multi action needs 1 to 16 steps");
        add_wait(&editor, "500", false);
        editor.save();
        assert_eq!(
            editor.toast(),
            "A multi action needs at least one action step"
        );
        editor.list().rows()[0].remove.emit_clicked();
        // Go to profile or page is offered, but only the last step may change the page.
        editor.list().add_step.emit_clicked();
        let dialog = editor.dialog();
        dialog.form.picker.set_selected(0);
        select_string(
            &dialog.form.core.layer_target_model,
            &dialog.form.core.layer_target_picker,
            "Studio",
        );
        dialog.primary.emit_clicked();
        assert!(!dialog.window.is_visible(), "a page step is accepted here");
        add_text_step(&editor, 1, "after");
        assert_eq!(editor.summaries(), ["Go to Studio", "Run after"]);
        editor.save();
        assert_eq!(
            editor.toast(),
            "Step 1: Go to profile or page can only be the last step"
        );
        assert_eq!(
            editor
                .layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(1)),
            original
        );
        assert!(ui.is_dirty(), "the refused draft stays for correction");
        assert!(!editor
            .commands()
            .iter()
            .any(|command| matches!(command, LayerCommand::SaveBinding(..))));
        // Moving the page change last makes it valid.
        editor.list().rows()[0].down.emit_clicked();
        editor.save();
        assert_eq!(
            editor.reloaded(&Trigger::KeyPress(1)),
            Some(multi(vec![
                step(Action::Command("after".into())),
                step(Action::SwitchLayer("Studio".into())),
            ]))
        );
    });
}

#[test]
fn the_step_form_offers_no_none_multi_timer_or_touchscreen_actions() {
    run_gtk_test(|| {
        let Some(editor) = editor("multi-step-form", |_| {}) else {
            return;
        };
        catalog_row(&editor.widgets, "Multi action").emit_clicked();
        editor.list().add_step.emit_clicked();
        let dialog = editor.dialog();
        assert_eq!(dialog.title.text(), "Add step");
        assert_eq!(button_caption(&dialog.primary).as_deref(), Some("Add"));
        // The shared dialog frame: modal over the window, one action row of Cancel then Add.
        assert!(dialog.window.is_modal());
        assert_eq!(
            dialog.window.transient_for().as_ref(),
            Some(editor.window.upcast_ref::<gtk::Window>())
        );
        assert!(dialog.window.has_css_class("popup-dialog"));
        assert!(dialog.primary.has_css_class("suggested-action"));
        assert_eq!(dialog.cancel.parent(), dialog.primary.parent());
        assert_eq!(
            dialog.cancel.next_sibling().as_ref(),
            Some(dialog.primary.upcast_ref())
        );
        let model = dialog.form.picker.model().unwrap();
        let types: Vec<String> = (0..model.n_items())
            .map(|index| {
                model
                    .item(index)
                    .and_downcast::<gtk::StringObject>()
                    .unwrap()
                    .string()
                    .to_string()
            })
            .collect();
        assert_eq!(
            types,
            [
                "Go to profile or page",
                "Run command",
                "Open application",
                "Shortcut",
                "Send named event",
                "More actions",
                "Play sound",
            ]
        );
        let offered = dialog.form.feature.offered();
        for absent in [
            "Multi action",
            "Start timer",
            "Cancel timer",
            "Next touchscreen page",
            "Previous touchscreen page",
        ] {
            assert!(!offered.contains(&absent), "{absent} is not a step");
        }
        for present in [
            "Next page",
            "Go to page",
            "Type text",
            "Play / pause",
            "Switch scene",
        ] {
            assert!(offered.contains(&present), "{present} is a step");
        }
        // A More actions step is built by the same feature form.
        dialog.form.picker.set_selected(5);
        assert!(dialog.form.feature.root.is_visible());
        dialog
            .form
            .feature
            .populate(&FeatureAction::Desktop(DesktopAction::Website {
                url: "https://example.com".into(),
            }));
        dialog.primary.emit_clicked();
        assert!(!dialog.window.is_visible());
        assert_eq!(editor.summaries(), ["Website https://example.com"]);
    });
}

#[test]
fn waits_convert_their_unit_and_refuse_lengths_outside_10_ms_to_60_s() {
    run_gtk_test(|| {
        let Some(editor) = editor("multi-wait", |_| {}) else {
            return;
        };
        catalog_row(&editor.widgets, "Multi action").emit_clicked();
        add_text_step(&editor, 1, "go");
        add_wait(&editor, "2", true);
        add_wait(&editor, "1.5", true);
        add_wait(&editor, "250", false);
        assert_eq!(
            editor.ui().feature_editor.steps().unwrap().steps(),
            [
                step(Action::Command("go".into())),
                wait(2000),
                wait(1500),
                wait(250),
            ]
        );
        assert_eq!(
            editor.summaries()[1..],
            ["Wait 2 s", "Wait 1500 ms", "Wait 250 ms"]
        );
        for (value, seconds, message) in [
            ("5", false, "Step 5: a wait must last 10 ms to 60 seconds"),
            ("61", true, "Step 5: a wait must last 10 ms to 60 seconds"),
            (
                "60001",
                false,
                "Step 5: a wait must last 10 ms to 60 seconds",
            ),
            ("abc", false, "Step 5: enter how long to wait, as a number"),
            ("", true, "Step 5: enter how long to wait, as a number"),
        ] {
            editor.list().add_wait.emit_clicked();
            let dialog = editor.dialog();
            dialog.wait_value.set_text(value);
            dialog.wait_unit.set_selected(u32::from(seconds));
            dialog.primary.emit_clicked();
            assert!(
                dialog.window.is_visible(),
                "{value:?} keeps the dialog open"
            );
            assert!(dialog.error.is_visible());
            assert_eq!(dialog.error.text(), message, "{value:?}");
            assert_eq!(editor.list().rows().len(), 4, "{value:?} adds nothing");
            dialog.cancel.emit_clicked();
        }
        // The bounds themselves are accepted.
        add_wait(&editor, "10", false);
        add_wait(&editor, "60", true);
        let steps = editor.ui().feature_editor.steps().unwrap().steps();
        assert_eq!(steps[4..], [wait(10), wait(60_000)]);
    });
}

#[test]
fn multi_actions_draw_their_own_icon_on_keys_and_in_menus() {
    run_gtk_test(|| {
        if init_gtk_for_test() {
            app_icon::register().unwrap();
        }
        let saved = multi(vec![step(Action::Command("make".into())), wait(100)]);
        let stored = saved.clone();
        let Some(editor) = editor("multi-icon", move |store| {
            store.bind("Base", Trigger::KeyPress(2), stored).unwrap();
        }) else {
            return;
        };
        assert_eq!(BindingIcon::for_action(&saved), BindingIcon::Multi);
        assert_eq!(BindingIcon::Multi.id(), "multi");
        let theme = gtk::IconTheme::for_display(&gtk::prelude::WidgetExt::display(&editor.window));
        assert!(
            theme.has_icon("sdeck-multi-symbolic"),
            "bundled icon is registered"
        );
        let row = catalog_row(&editor.widgets, "Multi action");
        assert_eq!(icon_ids(row.upcast_ref()), ["multi"]);
        catalog_row(&editor.widgets, "Multi action").emit_clicked();
        let ui = editor.ui();
        // A key has no Action selector: the guidance beside the description names the action,
        // and the step list heading carries the bundled symbolic icon.
        assert!(icon_ids(ui.feature_editor.root.upcast_ref()).contains(&"multi".to_owned()));
        let heading_icon = editor
            .list()
            .root()
            .first_child()
            .and_then(|row| row.first_child())
            .and_downcast::<gtk::Image>()
            .expect("step list heading icon");
        assert_eq!(
            heading_icon.icon_name().as_deref(),
            Some("sdeck-multi-symbolic")
        );
        assert_eq!(
            ui.feature_editor.selected_icon(),
            Some(BindingIcon::Multi),
            "the closed Action button follows the selected icon"
        );
        // The key and the device picture draw it.
        editor.artwork.refresh(true);
        let snapshot = editor
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
            ArtworkCommand::ComposeIcon {
                target: ArtworkTarget::Keypad(2),
                icon: BindingIcon::Multi,
                ..
            }
        )));
    });
}

#[test]
fn a_step_key_name_error_names_the_step() {
    run_gtk_test(|| {
        let Some(editor) = editor("multi-key-error", |_| {}) else {
            return;
        };
        let ui = editor.ui();
        // A legacy key name the parser no longer knows, loaded as it was stored.
        ui.populate_action(&multi(vec![
            step(Action::Command("x".into())),
            step(Action::KeyPress {
                key: "XF86Unsupported".into(),
            }),
        ]));
        editor.save();
        assert!(
            editor
                .toast()
                .starts_with("Could not save binding: Step 2: Unsupported physical key"),
            "{}",
            editor.toast()
        );
        assert_eq!(
            editor
                .layers
                .borrow()
                .binding("Base", &Trigger::KeyPress(1)),
            None
        );
        // The dialog names the step it is adding.
        add_text_step(&editor, 3, "XF86Unsupported");
        let dialog = editor.dialog();
        assert!(dialog.window.is_visible());
        assert!(
            dialog
                .error
                .text()
                .starts_with("Step 3: Unsupported physical key"),
            "{}",
            dialog.error.text()
        );
        assert_eq!(editor.list().rows().len(), 2);
        dialog.cancel.emit_clicked();
    });
}

#[test]
fn single_action_editor_flows_still_save_as_before() {
    run_gtk_test(|| {
        let Some(editor) = editor("multi-preserve", |_| {}) else {
            return;
        };
        let ui = editor.ui();
        // A shortcut from the catalog, typed with a modifier.
        catalog_row(&editor.widgets, "Shortcut").emit_clicked();
        assert_eq!(ui.action_kind(), ActionKind::Shortcut);
        assert!(ui.text_row.is_mapped() && ui.modifier_row.is_mapped());
        ui.text_entry.set_text("d");
        ui.ctrl_mod.set_active(true);
        assert_eq!(ui.shortcut_preview.text(), "Presses Ctrl + D");
        editor.save();
        assert_eq!(
            editor.reloaded(&Trigger::KeyPress(1)),
            Some(Action::Shortcut {
                modifiers: vec!["CTRL".into()],
                key: "D".into()
            })
        );
        // A More actions desktop action on another key.
        editor.select_key(2);
        catalog_row(&editor.widgets, "Open website").emit_clicked();
        assert!(
            !editor.list().root().is_mapped(),
            "no step list for a single action"
        );
        entry_with_placeholder(ui.feature_editor.root.upcast_ref(), "https://example.com")
            .set_text("https://example.org/docs");
        editor.save();
        assert_eq!(
            editor.reloaded(&Trigger::KeyPress(2)),
            Some(Action::Feature(FeatureAction::Desktop(
                DesktopAction::Website {
                    url: "https://example.org/docs".into()
                }
            )))
        );
        // An application with arguments on a key.
        editor.select_key(3);
        catalog_row(&editor.widgets, "Open application").emit_clicked();
        assert!(ui.application_row.is_mapped() && ui.args_row.is_mapped());
        ui.application_entry.set_text("/usr/bin/env");
        ui.args_view.buffer().set_text("FOO=1\n\\empty");
        editor.save();
        assert_eq!(
            editor.reloaded(&Trigger::KeyPress(3)),
            Some(Action::Launch {
                executable: "/usr/bin/env".into(),
                args: vec!["FOO=1".into(), String::new()],
            })
        );
        // A dial keeps its Action selector and the command field's own label.
        ui.select_control(ControlKind::Encoder, Some(1), true)
            .unwrap();
        ui.event_picker.set_selected(1);
        ui.action_picker.set_selected(1);
        let label = ui.text_row.first_child().and_downcast::<Label>().unwrap();
        assert_eq!(label.text(), "Shell command");
        assert_eq!(
            ui.text_entry.placeholder_text().as_deref(),
            Some("Shell command run with /bin/sh -c")
        );
        ui.text_entry.set_text("ls");
        editor.save();
        assert_eq!(
            editor.reloaded(&Trigger::EncoderClockwise(1)),
            Some(Action::Command("ls".into()))
        );
        // A destination page from the Go to profile or page selector.
        ui.action_picker.set_selected(0);
        assert!(ui.layer_target_row.is_mapped() && !ui.text_row.is_mapped());
        select_string(&ui.layer_target_model, &ui.layer_target_picker, "Studio");
        editor.save();
        assert_eq!(
            editor.reloaded(&Trigger::EncoderClockwise(1)),
            Some(Action::SwitchLayer("Studio".into()))
        );
    });
}

/// Opt-in captures for review: `STREAMDECK_MULTI_QA_DIR=<dir>` writes the editor with a
/// four-step multi action and the Edit step dialog. Unset, nothing is written.
#[test]
fn multi_action_editor_captures_when_explicitly_requested() {
    let Some(directory) = std::env::var_os("STREAMDECK_MULTI_QA_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    gtk::test_synced(move || {
        if !init_gtk_for_test() {
            return;
        }
        app_icon::register().unwrap();
        let Some(editor) = editor("multi-capture", |store| {
            store
                .bind(
                    "Base",
                    Trigger::KeyPress(1),
                    multi(vec![
                        step(Action::Shortcut {
                            modifiers: vec!["CTRL".into()],
                            key: "S".into(),
                        }),
                        wait(500),
                        step(Action::Command("notify-send Saved".into())),
                        step(Action::SwitchLayer("Studio".into())),
                    ]),
                )
                .unwrap();
        }) else {
            return;
        };
        editor.select_key(1);
        let card = editor.list().root().clone();
        let mut editor_card = card.parent();
        while let Some(widget) = editor_card.clone() {
            if widget.has_css_class("editor-card") {
                break;
            }
            editor_card = widget.parent();
        }
        let editor_card = editor_card.expect("editor card");
        let bounds = card.compute_bounds(&editor.widgets.root).unwrap();
        let adjustment = editor.widgets.scroller.vadjustment();
        adjustment.set_value(f64::from(bounds.y()) - 200.0);
        wait_for_frame(&editor.window);
        rendered_texture(&editor.window, &editor_card)
            .save_to_png(directory.join("multi-action-editor.png"))
            .unwrap();
        editor.list().rows()[2].edit.emit_clicked();
        let dialog = editor.dialog();
        wait_for_frame(&dialog.window);
        rendered_texture(&dialog.window, &dialog.window.child().unwrap())
            .save_to_png(directory.join("multi-action-step-dialog.png"))
            .unwrap();
        dialog.cancel.emit_clicked();
        editor.list().add_wait.emit_clicked();
        let dialog = editor.dialog();
        wait_for_frame(&dialog.window);
        rendered_texture(&dialog.window, &dialog.window.child().unwrap())
            .save_to_png(directory.join("multi-action-wait-dialog.png"))
            .unwrap();
        dialog.cancel.emit_clicked();
    });
}
