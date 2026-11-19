//! The numbered step list of a multi action and its Add/Edit step dialog.
//!
//! Each row shows the step's number, icon and summary with Move up, Move down, Edit and Remove;
//! Add step and Add wait follow the list. Adding, editing or moving a step never runs it: only
//! Test action or the key does, through the multi action sequencer.
use crate::*;
use action_form::ActionForm;
use sdeck_plus::features::{FeatureAction, MultiStep, MAX_MULTI_STEPS, MULTI_WAIT_RANGE};
use std::ops::Deref;

type Callback = Rc<dyn Fn()>;

/// What the step dialog needs to describe a step: the pages to go to and the plugins.
struct Context {
    layers: Rc<RefCell<LayerStore>>,
    plugins: plugin_ui::PluginHub,
}

#[derive(Clone)]
pub(crate) struct StepList(Rc<ListState>);

pub(crate) struct ListState {
    root: GtkBox,
    list: GtkBox,
    empty: Label,
    pub(crate) add_step: Button,
    pub(crate) add_wait: Button,
    steps: RefCell<Vec<MultiStep>>,
    rows: RefCell<Vec<StepRow>>,
    changed: RefCell<Vec<Callback>>,
    context: RefCell<Option<Context>>,
    dialog: RefCell<Option<StepDialog>>,
}

impl Deref for StepList {
    type Target = ListState;

    fn deref(&self) -> &ListState {
        &self.0
    }
}

/// One row of the list.
#[derive(Clone)]
pub(crate) struct StepRow {
    pub(crate) root: GtkBox,
    #[allow(dead_code)] // read by tests and captures
    pub(crate) number: Label,
    #[allow(dead_code)] // read by tests and captures
    pub(crate) summary: Label,
    pub(crate) up: Button,
    pub(crate) down: Button,
    pub(crate) edit: Button,
    #[allow(dead_code)] // read by tests and captures
    pub(crate) remove: Button,
}

/// What the dialog's primary button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    AddAction,
    AddWait,
    Edit(usize),
}

impl StepList {
    pub(crate) fn new() -> Self {
        let root = GtkBox::new(Orientation::Vertical, 8);
        root.add_css_class("multi-steps");
        let heading_row = GtkBox::new(Orientation::Horizontal, 8);
        let icon = gtk::Image::from_icon_name("sdeck-multi-symbolic");
        icon.set_pixel_size(16);
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        heading_row.append(&icon);
        heading_row.append(&inspector_heading("Steps"));
        root.append(&heading_row);
        let list = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .accessible_role(gtk::AccessibleRole::List)
            .build();
        list.add_css_class("multi-step-list");
        list.update_property(&[gtk::accessible::Property::Label("Steps")]);
        root.append(&list);
        let empty = ui::helper("No steps yet. Add the actions to run, in order.");
        empty.add_css_class("multi-step-empty");
        root.append(&empty);
        let buttons = GtkBox::new(Orientation::Horizontal, 8);
        let add_step = ui::button("list-add-symbolic", "Add step");
        let add_wait = ui::button("list-add-symbolic", "Add wait");
        buttons.append(&add_step);
        buttons.append(&add_wait);
        root.append(&buttons);
        let steps = Self(Rc::new(ListState {
            root,
            list,
            empty,
            add_step,
            add_wait,
            steps: RefCell::default(),
            rows: RefCell::default(),
            changed: RefCell::default(),
            context: RefCell::default(),
            dialog: RefCell::default(),
        }));
        for (button, target) in [
            (&steps.add_step, Target::AddAction),
            (&steps.add_wait, Target::AddWait),
        ] {
            let weak = Rc::downgrade(&steps.0);
            button.connect_clicked(move |_| {
                if let Some(state) = weak.upgrade() {
                    Self(state).open(target);
                }
            });
        }
        steps.rebuild(None);
        steps
    }

    pub(crate) fn root(&self) -> &GtkBox {
        &self.0.root
    }

    /// Lets the step dialog offer the store's pages and the installed plugins.
    pub(crate) fn set_context(
        &self,
        layers: &Rc<RefCell<LayerStore>>,
        plugins: &plugin_ui::PluginHub,
    ) {
        *self.0.context.borrow_mut() = Some(Context {
            layers: layers.clone(),
            plugins: plugins.clone(),
        });
    }

    /// The steps, in order.
    pub(crate) fn steps(&self) -> Vec<MultiStep> {
        self.0.steps.borrow().clone()
    }

    /// The rows as shown, for tests and captures.
    #[allow(dead_code)]
    pub(crate) fn rows(&self) -> Vec<StepRow> {
        self.0.rows.borrow().clone()
    }

    /// The dialog, once Add step, Add wait or Edit opened it.
    #[allow(dead_code)]
    pub(crate) fn dialog(&self) -> Option<StepDialog> {
        self.0.dialog.borrow().clone()
    }

    pub(crate) fn populate(&self, steps: &[MultiStep]) {
        *self.0.steps.borrow_mut() = steps.to_vec();
        self.rebuild(None);
        self.emit();
    }

    pub(crate) fn clear(&self) {
        if let Some(dialog) = self.0.dialog.borrow().as_ref() {
            dialog.window.close();
        }
        self.populate(&[]);
    }

    /// Raw state, to tell whether the editor differs from a saved binding.
    pub(crate) fn snapshot(&self) -> String {
        format!("{:?}", self.0.steps.borrow())
    }

    /// Run `notify` whenever a step is added, changed, moved or removed.
    pub(crate) fn connect_changed(&self, notify: impl Fn() + 'static) {
        self.0.changed.borrow_mut().push(Rc::new(notify));
    }

    fn emit(&self) {
        let callbacks: Vec<Callback> = self.0.changed.borrow().clone();
        for callback in callbacks {
            callback();
        }
    }

    /// Rebuild the rows; `focus` names the row and button to give the keyboard focus to.
    fn rebuild(&self, focus: Option<(usize, Focus)>) {
        let state = &self.0;
        while let Some(child) = state.list.first_child() {
            state.list.remove(&child);
        }
        let steps = self.steps();
        let rows: Vec<StepRow> = steps
            .iter()
            .enumerate()
            .map(|(index, step)| self.row(index, step, steps.len()))
            .collect();
        for row in &rows {
            state.list.append(&row.root);
        }
        state.list.set_visible(!steps.is_empty());
        state.empty.set_visible(steps.is_empty());
        let full = steps.len() >= MAX_MULTI_STEPS;
        for (button, caption) in [(&state.add_step, "Add step"), (&state.add_wait, "Add wait")] {
            button.set_sensitive(!full);
            button.set_tooltip_text(Some(&if full {
                format!("{caption}\nA multi action holds at most {MAX_MULTI_STEPS} steps.")
            } else {
                caption.to_owned()
            }));
        }
        if let Some((index, focus)) = focus {
            let target = rows.get(index).map(|row| match focus {
                Focus::Up if row.up.is_sensitive() => &row.up,
                Focus::Down if row.down.is_sensitive() => &row.down,
                _ => &row.edit,
            });
            match target {
                Some(button) => button.grab_focus(),
                None => state.add_step.grab_focus(),
            };
        }
        *state.rows.borrow_mut() = rows;
    }

    fn row(&self, index: usize, step: &MultiStep, count: usize) -> StepRow {
        let position = index + 1;
        let root = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .accessible_role(gtk::AccessibleRole::ListItem)
            .build();
        root.add_css_class("multi-step-row");
        let number = Label::new(Some(&position.to_string()));
        number.add_css_class("multi-step-number");
        number.set_width_chars(2);
        number.set_xalign(1.0);
        root.append(&number);
        let icon = match step {
            MultiStep::Action(action) => BindingIcon::for_saved_action(action),
            MultiStep::Wait { .. } => BindingIcon::Timer,
        };
        root.append(&binding_icon::binding_icon(icon));
        let text = sdeck_plus::binding_editor::step_summary(step);
        let summary = Label::new(Some(&text));
        summary.add_css_class("multi-step-summary");
        summary.set_xalign(0.0);
        summary.set_hexpand(true);
        summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        summary.set_width_chars(6);
        summary.set_tooltip_text(Some(&text));
        root.append(&summary);
        root.update_property(&[gtk::accessible::Property::Label(&format!(
            "Step {position}: {text}"
        ))]);
        let actions = GtkBox::new(Orientation::Horizontal, 2);
        actions.add_css_class("multi-step-actions");
        let tool = |icon: &str, name: &str, explanation: String, label: String| {
            let button = ui::icon_button(icon, &label);
            button.add_css_class("flat");
            button.set_tooltip_text(Some(&format!("{name}\n{explanation}")));
            actions.append(&button);
            button
        };
        let up = tool(
            "go-up-symbolic",
            "Move up",
            format!("Runs step {position} one place earlier."),
            format!("Move step {position} up"),
        );
        let down = tool(
            "go-down-symbolic",
            "Move down",
            format!("Runs step {position} one place later."),
            format!("Move step {position} down"),
        );
        let edit = tool(
            "document-edit-symbolic",
            "Edit step",
            format!("Changes step {position} without running it."),
            format!("Edit step {position}"),
        );
        let remove = tool(
            "user-trash-symbolic",
            "Remove step",
            format!("Deletes step {position} from this multi action."),
            format!("Remove step {position}"),
        );
        remove.add_css_class("destructive-action");
        up.set_sensitive(index > 0);
        down.set_sensitive(position < count);
        root.append(&actions);
        for (button, command) in [
            (&up, Command::Up),
            (&down, Command::Down),
            (&edit, Command::Edit),
            (&remove, Command::Remove),
        ] {
            let weak = Rc::downgrade(&self.0);
            button.connect_clicked(move |_| {
                if let Some(state) = weak.upgrade() {
                    Self(state).run(command, index);
                }
            });
        }
        StepRow {
            root,
            number,
            summary,
            up,
            down,
            edit,
            remove,
        }
    }

    fn run(&self, command: Command, index: usize) {
        let count = self.0.steps.borrow().len();
        if index >= count {
            return;
        }
        match command {
            Command::Up if index > 0 => {
                self.0.steps.borrow_mut().swap(index - 1, index);
                self.rebuild(Some((index - 1, Focus::Up)));
                self.emit();
            }
            Command::Down if index + 1 < count => {
                self.0.steps.borrow_mut().swap(index, index + 1);
                self.rebuild(Some((index + 1, Focus::Down)));
                self.emit();
            }
            Command::Edit => self.open(Target::Edit(index)),
            Command::Remove => {
                self.0.steps.borrow_mut().remove(index);
                self.rebuild(Some((index.min(count - 1), Focus::Edit)));
                self.emit();
            }
            Command::Up | Command::Down => {}
        }
    }

    /// Open the dialog for `target`; without a context (no store yet) there is nothing to open.
    fn open(&self, target: Target) {
        let (layers, plugins) = match self.0.context.borrow().as_ref() {
            Some(context) => (context.layers.clone(), context.plugins.clone()),
            None => return,
        };
        let dialog = {
            let existing = self.0.dialog.borrow().clone();
            match existing {
                Some(dialog) => dialog,
                None => {
                    let dialog = StepDialog::new(&plugins, Rc::downgrade(&self.0));
                    *self.0.dialog.borrow_mut() = Some(dialog.clone());
                    dialog
                }
            }
        };
        let step = match target {
            Target::Edit(index) => match self.0.steps.borrow().get(index) {
                Some(step) => Some(step.clone()),
                None => return,
            },
            Target::AddAction | Target::AddWait => None,
        };
        let names = layers.borrow().names();
        let count = self.0.steps.borrow().len();
        dialog.show(target, step.as_ref(), count, &names);
        let parent = self.0.root.root().and_downcast::<gtk::Window>();
        dialog.window.set_transient_for(parent.as_ref());
        dialog.window.present();
        if dialog.wait_box.is_visible() {
            dialog.wait_value.grab_focus();
        } else {
            dialog.form.picker.grab_focus();
        }
    }

    /// Store what the dialog built: a new last step, or a replacement.
    fn apply(&self, target: Target, step: MultiStep) {
        let index = {
            let mut steps = self.0.steps.borrow_mut();
            match target {
                Target::Edit(index) if index < steps.len() => {
                    steps[index] = step;
                    index
                }
                _ => {
                    steps.push(step);
                    steps.len() - 1
                }
            }
        };
        self.rebuild(Some((index, Focus::Edit)));
        self.emit();
    }

    /// The layers the dialog builds its step against.
    fn layers(&self) -> Option<Rc<RefCell<LayerStore>>> {
        self.0
            .context
            .borrow()
            .as_ref()
            .map(|context| context.layers.clone())
    }
}

#[derive(Clone, Copy)]
enum Command {
    Up,
    Down,
    Edit,
    Remove,
}

#[derive(Clone, Copy)]
enum Focus {
    Up,
    Down,
    Edit,
}

/// The compact Add/Edit step dialog: an action form or a wait field, then Cancel and Add/Save.
#[derive(Clone)]
pub(crate) struct StepDialog(Rc<DialogState>);

pub(crate) struct DialogState {
    pub(crate) window: gtk::Window,
    pub(crate) title: Label,
    pub(crate) subtitle: Label,
    pub(crate) form: ActionForm,
    pub(crate) wait_box: GtkBox,
    pub(crate) wait_value: Entry,
    pub(crate) wait_unit: gtk::DropDown,
    pub(crate) error: Label,
    pub(crate) primary: Button,
    #[allow(dead_code)] // read by tests and captures
    pub(crate) cancel: Button,
    target: Cell<Option<Target>>,
    /// Whether the dialog edits a wait (else an action step).
    waiting: Cell<bool>,
    /// The step's 1-based position, named in its errors.
    position: Cell<usize>,
    list: std::rc::Weak<ListState>,
}

impl Deref for StepDialog {
    type Target = DialogState;

    fn deref(&self) -> &DialogState {
        &self.0
    }
}

impl StepDialog {
    fn new(plugins: &plugin_ui::PluginHub, list: std::rc::Weak<ListState>) -> Self {
        let form = ActionForm::new(plugins);
        let wait_value = Entry::builder()
            .width_chars(8)
            .input_purpose(gtk::InputPurpose::Number)
            .placeholder_text("500")
            .build();
        let wait_unit = gtk::DropDown::from_strings(&["ms", "s"]);
        wait_unit.set_tooltip_text(Some("Milliseconds or seconds"));
        wait_unit.update_property(&[gtk::accessible::Property::Label("Unit")]);
        let wait_fields = GtkBox::new(Orientation::Horizontal, 8);
        wait_fields.append(&wait_value);
        wait_fields.append(&wait_unit);
        let wait_box = labeled("How long to wait (10 ms to 60 s)", &wait_fields);
        if let Some(label) = wait_box.first_child().and_downcast::<Label>() {
            label.set_mnemonic_widget(Some(&wait_value));
            wait_value
                .update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
        }
        wait_box.append(&ui::helper(
            "The next step starts once this pause ends. Nothing else waits for it.",
        ));
        let error = ui::helper("");
        error.set_visible(false);
        let body = GtkBox::new(Orientation::Vertical, 12);
        body.append(&form.root);
        body.append(&wait_box);
        body.append(&error);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true)
            .min_content_width(360)
            .max_content_height(460)
            .child(&body)
            .build();
        let primary = Button::with_label("Add");
        primary.add_css_class("suggested-action");
        let frame = ui::form_dialog(None, "Add step", "", &scroller, "Cancel", Some(&primary));
        frame.window.add_css_class("step-dialog");
        let dialog = Self(Rc::new(DialogState {
            window: frame.window,
            title: frame.title,
            subtitle: frame.subtitle,
            form,
            wait_box,
            wait_value,
            wait_unit,
            error,
            primary,
            cancel: frame.dismiss,
            target: Cell::new(None),
            waiting: Cell::new(false),
            position: Cell::new(1),
            list,
        }));
        let weak = Rc::downgrade(&dialog.0);
        dialog.primary.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                Self(state).commit();
            }
        });
        let weak = Rc::downgrade(&dialog.0);
        dialog.wait_value.connect_activate(move |_| {
            if let Some(state) = weak.upgrade() {
                Self(state).commit();
            }
        });
        let weak = Rc::downgrade(&dialog.0);
        dialog.window.connect_hide(move |_| {
            if let Some(state) = weak.upgrade() {
                state.target.set(None);
                Self(state).set_error(None);
            }
        });
        dialog
    }

    /// Fill the dialog for `target`: `step` is the edited step, `count` the list's length.
    fn show(&self, target: Target, step: Option<&MultiStep>, count: usize, pages: &[String]) {
        let position = match target {
            Target::Edit(index) => index + 1,
            Target::AddAction | Target::AddWait => count + 1,
        };
        self.target.set(Some(target));
        self.position.set(position);
        self.set_error(None);
        self.form.set_destinations(pages);
        let waiting = matches!(
            (target, step),
            (Target::AddWait, _) | (Target::Edit(_), Some(MultiStep::Wait { .. }))
        );
        self.waiting.set(waiting);
        let (title, subtitle) = match (target, step) {
            (Target::AddWait, _) => (
                "Add wait".to_owned(),
                format!("Step {position} pauses before the next step, from 10 ms to 60 seconds."),
            ),
            (Target::Edit(_), Some(MultiStep::Wait { .. })) => (
                "Edit wait".to_owned(),
                format!("Changes how long step {position} pauses, from 10 ms to 60 seconds."),
            ),
            (Target::Edit(_), _) => (
                "Edit step".to_owned(),
                format!("Changes step {position}. Saving it here does not run it."),
            ),
            (Target::AddAction, _) => (
                "Add step".to_owned(),
                format!("Step {position} starts after the step before it finishes. Adding it does not run it."),
            ),
        };
        self.title.set_text(&title);
        self.window.set_title(Some(&title));
        self.subtitle.set_text(&subtitle);
        self.primary.set_label(match target {
            Target::Edit(_) => "Save",
            Target::AddAction | Target::AddWait => "Add",
        });
        match step {
            Some(MultiStep::Action(action)) => self.form.populate(action),
            Some(MultiStep::Wait { milliseconds }) => {
                let seconds = milliseconds % 1000 == 0;
                self.wait_value.set_text(&if seconds {
                    (milliseconds / 1000).to_string()
                } else {
                    milliseconds.to_string()
                });
                self.wait_unit.set_selected(u32::from(seconds));
            }
            None if waiting => {
                self.wait_value.set_text("1");
                self.wait_unit.set_selected(1);
            }
            None => self.form.reset(),
        }
        self.form.root.set_visible(!waiting);
        self.wait_box.set_visible(waiting);
    }

    fn set_error(&self, message: Option<&str>) {
        match message {
            Some(message) => {
                self.error.set_text(message);
                self.error.set_visible(true);
                ui::feedback(&self.error, true);
            }
            None => {
                self.error.set_text("");
                self.error.set_visible(false);
                ui::clear_feedback(&self.error);
            }
        }
    }

    /// Build the step; on success it joins the list and the dialog closes.
    fn commit(&self) {
        let (Some(target), Some(list)) = (self.target.get(), self.list.upgrade()) else {
            return;
        };
        let list = StepList(list);
        let built = if self.waiting.get() {
            wait_milliseconds(&self.wait_value.text(), self.wait_unit.selected() == 1)
                .map(|milliseconds| MultiStep::Wait { milliseconds })
        } else {
            match list.layers() {
                Some(layers) => {
                    let store = layers.borrow();
                    self.form
                        .action(&store)
                        .and_then(|action| check_step(&action).map(|()| action))
                        .map(MultiStep::Action)
                }
                None => Err("the configuration is not available".into()),
            }
        };
        match built {
            Ok(step) => {
                self.window.close();
                list.apply(target, step);
            }
            Err(message) => {
                self.set_error(Some(&format!("Step {}: {message}", self.position.get())))
            }
        }
    }
}

/// A wait in milliseconds from the dialog's number and unit (seconds may have decimals).
fn wait_milliseconds(value: &str, seconds: bool) -> Result<u32, String> {
    let number: f64 = value
        .trim()
        .parse()
        .ok()
        .filter(|number: &f64| number.is_finite() && *number >= 0.0)
        .ok_or("enter how long to wait, as a number")?;
    let milliseconds = (if seconds { number * 1000.0 } else { number }).round();
    let (shortest, longest) = (*MULTI_WAIT_RANGE.start(), *MULTI_WAIT_RANGE.end());
    if milliseconds < f64::from(shortest) || milliseconds > f64::from(longest) {
        return Err("a wait must last 10 ms to 60 seconds".into());
    }
    Ok(milliseconds as u32)
}

/// Check one step with the multi action's own rules (as if it were the only, last step), so the
/// dialog refuses exactly what Save would; the caller names the step's real position.
fn check_step(action: &Action) -> Result<(), String> {
    FeatureAction::Multi {
        steps: vec![MultiStep::Action(action.clone())],
    }
    .validate()
    .map_err(|error| {
        let message = error.to_string();
        message
            .strip_prefix("Step 1: ")
            .map_or_else(|| message.clone(), str::to_owned)
    })
}
