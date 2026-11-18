//! The action fields shared by the binding editor and the multi action step form.
//!
//! [`CoreFields`] are the fields of the basic actions (Go to profile or page, Run command, Open
//! application, Shortcut and Send named event): the binding editor shows its own set, and
//! [`ActionForm`] combines another set with the More actions, Play sound and plugin forms so a
//! step of a multi action is described exactly as a binding is.
use crate::*;
use sdeck_plus::features::FeatureAction;
use std::ops::Deref;

/// The binding editor's Action menu without None and Plugin action, in its order.
pub(crate) const ACTION_TYPES: [&str; 7] = [
    "Go to profile or page",
    "Run command",
    "Open application",
    "Shortcut",
    "Send named event",
    "More actions",
    "Play sound",
];

/// The step form's "Plugin action" row: it follows Play sound (there is no None).
pub(crate) const STEP_PLUGIN: u32 = 7;

const SHORTCUT_GUIDANCE: &str =
    "Type a key or chord. <a href=\"shortcut-reference\">Key names and aliases</a>";

/// The explanation shown above a basic action's fields; empty for the actions with their own form.
pub(crate) fn guidance(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::None => {
            "Do nothing when this control is used. Live information can still be shown."
        }
        ActionKind::MoveToLayer => "Switch to another profile or page when this control is used.",
        ActionKind::RunCommand => "Run a shell command with /bin/sh. Saving does not run it.",
        ActionKind::OpenApplication => "Enter the executable path and one argument per line.",
        ActionKind::Shortcut => "Type a key or full chord. The checkboxes add modifiers.",
        ActionKind::EmitLegacy => "Send a named internal event. Mainly used by older bindings.",
        ActionKind::Extended | ActionKind::Plugin => "",
    }
}

/// Write [`guidance`] into `label`; Shortcut links to the key name reference.
pub(crate) fn show_guidance(label: &Label, kind: ActionKind) {
    label.set_text(guidance(kind));
    if matches!(kind, ActionKind::Shortcut) {
        label.set_markup(SHORTCUT_GUIDANCE);
    }
}

/// "Presses Ctrl + P" once the chord parses, else empty.
pub(crate) fn chord_preview(modifiers: &[String], key: &str) -> String {
    match sdeck_plus::keyboard::parse_chord(modifiers, key) {
        Ok(chord) => format!("Presses {}", chord.label()),
        Err(_) => String::new(),
    }
}

/// The Action menu row of a basic action; `None` for None and the feature actions.
pub(crate) fn core_index(action: &Action) -> Option<u32> {
    Some(match action {
        Action::SwitchLayer(_) => 0,
        Action::Command(_) => 1,
        Action::Launch { .. } => 2,
        Action::KeyPress { .. } | Action::Shortcut { .. } => 3,
        Action::Emit(_) => 4,
        Action::None | Action::Feature(_) => return None,
    })
}

/// The fields of the basic actions, with their labelled rows.
#[derive(Clone)]
pub(crate) struct CoreFields {
    pub(crate) layer_target_model: gtk::StringList,
    pub(crate) layer_target_picker: gtk::DropDown,
    pub(crate) text_entry: Entry,
    pub(crate) application_entry: Entry,
    pub(crate) args_view: TextView,
    pub(crate) super_mod: gtk::ToggleButton,
    pub(crate) shift_mod: gtk::ToggleButton,
    pub(crate) ctrl_mod: gtk::ToggleButton,
    pub(crate) alt_mod: gtk::ToggleButton,
    pub(crate) layer_target_row: GtkBox,
    pub(crate) text_row: GtkBox,
    pub(crate) application_row: GtkBox,
    pub(crate) args_row: GtkBox,
    pub(crate) modifier_row: GtkBox,
}

impl CoreFields {
    /// Build the fields, offering `destinations` as the profiles and pages to go to.
    pub(crate) fn new(destinations: &[String]) -> Self {
        let layer_target_model = gtk::StringList::new(&[]);
        for name in destinations {
            layer_target_model.append(name);
        }
        let layer_target_picker =
            gtk::DropDown::new(Some(layer_target_model.clone()), gtk::Expression::NONE);
        bound_layer_picker(&layer_target_picker);
        let text_entry = Entry::builder().hexpand(true).build();
        let application_entry = Entry::builder()
            .placeholder_text("Executable, for example /usr/bin/firefox")
            .hexpand(true)
            .build();
        let args_view = TextView::new();
        args_view.set_monospace(true);
        args_view.set_size_request(-1, 48);
        let [ctrl_mod, alt_mod, shift_mod, super_mod] =
            ["Ctrl", "Alt", "Shift", "Super"].map(modifier_toggle);
        let layer_target_row = labeled("Destination profile or page", &layer_target_picker);
        let text_row = labeled("Command / key / shortcut key / event name", &text_entry);
        let application_row = labeled("Application executable", &application_entry);
        let args_scroller = gtk::ScrolledWindow::builder()
            .min_content_height(48)
            .child(&args_view)
            .build();
        let args_row = labeled("Arguments (one per line)", &args_scroller);
        if let Some(args_label) = args_row.first_child().and_downcast::<Label>() {
            args_label.set_mnemonic_widget(Some(&args_view));
            args_view.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
                args_label.upcast_ref()
            ])]);
        }
        // One aligned, linked row of toggles instead of a ragged check grid.
        let modifier_buttons = GtkBox::new(Orientation::Horizontal, 0);
        modifier_buttons.add_css_class("linked");
        modifier_buttons.add_css_class("modifier-toggles");
        modifier_buttons.set_halign(gtk::Align::Start);
        for toggle in [&ctrl_mod, &alt_mod, &shift_mod, &super_mod] {
            modifier_buttons.append(toggle);
        }
        let modifier_row = labeled("Shortcut modifiers", &modifier_buttons);
        Self {
            layer_target_model,
            layer_target_picker,
            text_entry,
            application_entry,
            args_view,
            super_mod,
            shift_mod,
            ctrl_mod,
            alt_mod,
            layer_target_row,
            text_row,
            application_row,
            args_row,
            modifier_row,
        }
    }

    /// The text, shortcut, application and argument rows as one adaptive group following
    /// `picker` (an Action menu whose rows 0 to 4 are the basic actions): two columns for an
    /// application or a shortcut when there is room, hidden for the other actions.
    pub(crate) fn adaptive_fields(&self, picker: &gtk::DropDown) -> gtk::FlowBox {
        let columns = |selected: u32| if matches!(selected, 2 | 3) { 2 } else { 1 };
        let action_fields = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .focusable(false)
            .min_children_per_line(1)
            .max_children_per_line(columns(picker.selected()))
            .homogeneous(true)
            .column_spacing(16)
            .row_spacing(12)
            .hexpand(true)
            .build();
        action_fields.add_css_class("action-fields");
        for row in [
            &self.text_row,
            &self.modifier_row,
            &self.application_row,
            &self.args_row,
        ] {
            row.set_size_request(180, -1);
            let cell = gtk::FlowBoxChild::new();
            cell.set_focusable(false);
            cell.set_child(Some(row));
            action_fields.insert(&cell, -1);
            row.bind_property("visible", &cell, "visible")
                .sync_create()
                .build();
        }
        action_fields.set_visible(matches!(picker.selected(), 1..=4));
        let fields = action_fields.downgrade();
        picker.connect_selected_notify(move |picker| {
            if let Some(fields) = fields.upgrade() {
                fields.set_max_children_per_line(columns(picker.selected()));
                fields.set_visible(matches!(picker.selected(), 1..=4));
            }
        });
        action_fields
    }

    /// The checked modifiers, in their stored order.
    pub(crate) fn modifiers(&self) -> Vec<String> {
        [
            (&self.super_mod, "SUPER"),
            (&self.shift_mod, "SHIFT"),
            (&self.ctrl_mod, "CTRL"),
            (&self.alt_mod, "ALT"),
        ]
        .into_iter()
        .filter(|(toggle, _)| toggle.is_active())
        .map(|(_, name)| name.to_owned())
        .collect()
    }

    pub(crate) fn clear(&self) {
        self.text_entry.set_text("");
        self.application_entry.set_text("");
        self.args_view.buffer().set_text("");
        for toggle in [
            &self.super_mod,
            &self.shift_mod,
            &self.ctrl_mod,
            &self.alt_mod,
        ] {
            toggle.set_active(false);
        }
    }

    /// Show a basic action in its fields; other actions leave them as they are.
    pub(crate) fn populate(&self, action: &Action) {
        match action {
            Action::SwitchLayer(target) => {
                select_string(&self.layer_target_model, &self.layer_target_picker, target);
            }
            Action::Command(command) => self.text_entry.set_text(command),
            Action::KeyPress { key } => {
                // Inspect legacy names verbatim; only an explicit save canonicalizes them.
                self.text_entry.set_text(key);
                for toggle in [
                    &self.super_mod,
                    &self.shift_mod,
                    &self.ctrl_mod,
                    &self.alt_mod,
                ] {
                    toggle.set_active(false);
                }
            }
            Action::Launch { executable, args } => {
                self.application_entry
                    .set_text(&executable.to_string_lossy());
                self.args_view
                    .buffer()
                    .set_text(&format_application_args(args));
            }
            Action::Shortcut { modifiers, key } => {
                self.text_entry.set_text(key);
                self.super_mod
                    .set_active(modifiers.iter().any(|m| m == "SUPER"));
                self.shift_mod
                    .set_active(modifiers.iter().any(|m| m == "SHIFT"));
                self.ctrl_mod
                    .set_active(modifiers.iter().any(|m| m == "CTRL" || m == "CONTROL"));
                self.alt_mod
                    .set_active(modifiers.iter().any(|m| m == "ALT"));
            }
            Action::Emit(name) => self.text_entry.set_text(name),
            Action::None | Action::Feature(_) => {}
        }
    }

    /// The basic action of `kind` the fields describe, keyboard names normalized.
    pub(crate) fn action(&self, store: &LayerStore, kind: ActionKind) -> Result<Action, String> {
        let destination = dropdown_value(
            &self.layer_target_model,
            self.layer_target_picker.selected(),
        )
        .unwrap_or_default();
        let buffer = self.args_view.buffer();
        let args = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        action_from_fields(
            store,
            kind,
            &destination,
            &self.application_entry.text(),
            &args,
            &self.text_entry.text(),
            &self.modifiers(),
        )
        .map_err(|error| error.0)
    }

    /// Show only the rows `kind` uses, with its label, tooltip and placeholder.
    pub(crate) fn show(&self, kind: ActionKind) {
        if let Some(label) = self.text_row.first_child().and_downcast::<Label>() {
            label.set_text(match kind {
                ActionKind::RunCommand => "Shell command",
                ActionKind::Shortcut => "Shortcut",
                ActionKind::EmitLegacy => "Event name",
                ActionKind::None
                | ActionKind::MoveToLayer
                | ActionKind::OpenApplication
                | ActionKind::Extended
                | ActionKind::Plugin => "",
            });
        }
        self.layer_target_row
            .set_visible(matches!(kind, ActionKind::MoveToLayer));
        self.application_row
            .set_visible(matches!(kind, ActionKind::OpenApplication));
        self.args_row
            .set_visible(matches!(kind, ActionKind::OpenApplication));
        self.modifier_row
            .set_visible(matches!(kind, ActionKind::Shortcut));
        self.text_row.set_visible(matches!(
            kind,
            ActionKind::RunCommand | ActionKind::Shortcut | ActionKind::EmitLegacy
        ));
        self.text_entry.set_tooltip_text(Some(match kind {
            ActionKind::Shortcut => "Type a full chord or one key; checkboxes add modifiers without duplicates. Win/Windows/Meta = SUPER; Enter/Return are aliases.",
            _ => "",
        }));
        self.text_entry.set_placeholder_text(Some(match kind {
            ActionKind::RunCommand => "Shell command run with /bin/sh -c",
            ActionKind::Shortcut => "A, F5, Enter (no modifiers), or CTRL + D",
            ActionKind::EmitLegacy => "Internal event name",
            ActionKind::None
            | ActionKind::MoveToLayer
            | ActionKind::OpenApplication
            | ActionKind::Extended
            | ActionKind::Plugin => "",
        }));
    }
}

/// The step form of a multi action: an Action menu with the binding editor's action types
/// except None, then the same fields, More actions form and plugin form as the editor.
#[derive(Clone)]
pub(crate) struct ActionForm(Rc<FormState>);

pub(crate) struct FormState {
    pub(crate) root: GtkBox,
    pub(crate) picker: gtk::DropDown,
    picker_model: gtk::StringList,
    pub(crate) guidance: Label,
    pub(crate) core: CoreFields,
    preview: Label,
    pub(crate) feature: feature_editor::FeatureEditor,
    pub(crate) plugin: plugin_ui::PluginEditor,
    plugins: plugin_ui::PluginHub,
}

impl Deref for ActionForm {
    type Target = FormState;

    fn deref(&self) -> &FormState {
        &self.0
    }
}

impl ActionForm {
    pub(crate) fn new(plugins: &plugin_ui::PluginHub) -> Self {
        let root = GtkBox::new(Orientation::Vertical, 12);
        root.add_css_class("step-action-form");
        let picker_model = gtk::StringList::new(&ACTION_TYPES);
        let picker = gtk::DropDown::new(Some(picker_model.clone()), gtk::Expression::NONE);
        root.append(&labeled("Action", &picker));
        let guidance = ui::helper("");
        shortcut_reference::connect(&guidance);
        root.append(&guidance);
        let core = CoreFields::new(&[]);
        let preview = Label::new(None);
        preview.add_css_class("muted");
        preview.set_xalign(0.0);
        preview.set_visible(false);
        core.modifier_row.append(&preview);
        root.append(&core.layer_target_row);
        root.append(&core.adaptive_fields(&picker));
        let feature = feature_editor::FeatureEditor::for_steps();
        root.append(&feature.root);
        let plugin = plugin_ui::PluginEditor::new(plugins);
        root.append(&*plugin);
        // The closed button names the chosen More action, as in the binding editor.
        let more_icon = Rc::new(Cell::new(BindingIcon::More));
        let closed_more = more_icon.clone();
        let menu_icon = |position| match position {
            STEP_PLUGIN => BindingIcon::More,
            other => BindingIcon::for_action_type(other),
        };
        binding_icon::icon_dropdown_with(
            &picker,
            Rc::new(move |position| match position {
                5 => closed_more.get(),
                other => menu_icon(other),
            }),
            Rc::new(menu_icon),
        );
        let form = Self(Rc::new(FormState {
            root,
            picker,
            picker_model,
            guidance,
            core,
            preview,
            feature,
            plugin,
            plugins: plugins.clone(),
        }));
        let weak = form.picker.downgrade();
        form.feature.connect_icon_changed(move |icon| {
            let icon = icon.unwrap_or(BindingIcon::More);
            more_icon.set(icon);
            if let Some(picker) = weak.upgrade().filter(|picker| picker.selected() == 5) {
                binding_icon::set_closed_icon(&picker, icon);
            }
        });
        let weak = Rc::downgrade(&form.0);
        form.picker.connect_selected_notify(move |_| {
            if let Some(state) = weak.upgrade() {
                Self(state).update();
            }
        });
        let weak = Rc::downgrade(&form.0);
        let preview = move || {
            if let Some(state) = weak.upgrade() {
                Self(state).update_preview();
            }
        };
        let text_preview = preview.clone();
        form.core
            .text_entry
            .connect_changed(move |_| text_preview());
        for toggle in [
            &form.core.super_mod,
            &form.core.shift_mod,
            &form.core.ctrl_mod,
            &form.core.alt_mod,
        ] {
            let preview = preview.clone();
            toggle.connect_toggled(move |_| preview());
        }
        form.reset();
        form
    }

    /// What the selected Action menu row stands for.
    fn kind(&self) -> Option<ActionKind> {
        Some(match self.picker.selected() {
            0 => ActionKind::MoveToLayer,
            1 => ActionKind::RunCommand,
            2 => ActionKind::OpenApplication,
            3 => ActionKind::Shortcut,
            4 => ActionKind::EmitLegacy,
            5 | 6 => ActionKind::Extended,
            STEP_PLUGIN => ActionKind::Plugin,
            _ => return None,
        })
    }

    fn update(&self) {
        let kind = self.kind();
        self.core.show(kind.unwrap_or(ActionKind::None));
        self.feature.show_direct_sound(self.picker.selected() == 6);
        self.feature
            .root
            .set_visible(kind == Some(ActionKind::Extended));
        self.plugin.set_visible(kind == Some(ActionKind::Plugin));
        match kind {
            Some(kind) => show_guidance(&self.guidance, kind),
            None => self.guidance.set_text(""),
        }
        self.guidance.set_visible(!self.guidance.text().is_empty());
        self.update_preview();
    }

    fn update_preview(&self) {
        let text = if self.kind() == Some(ActionKind::Shortcut) {
            chord_preview(&self.core.modifiers(), &self.core.text_entry.text())
        } else {
            String::new()
        };
        self.preview.set_text(&text);
        self.preview.set_visible(!text.is_empty());
    }

    /// Offer "Plugin action" once a plugin is installed, as the binding editor does.
    fn sync_plugin_row(&self) {
        if !self.plugins.plugins().is_empty() {
            self.ensure_plugin_row();
        }
    }

    fn ensure_plugin_row(&self) {
        if self.picker_model.n_items() == STEP_PLUGIN {
            self.picker_model.append("Plugin action");
        }
    }

    /// The profiles and pages a Go to profile or page step may go to.
    pub(crate) fn set_destinations(&self, names: &[String]) {
        replace_string_list(&self.core.layer_target_model, names);
        self.core.layer_target_picker.set_selected(0);
    }

    /// A fresh Shortcut step.
    pub(crate) fn reset(&self) {
        self.core.clear();
        self.feature.reset();
        self.plugin.reset();
        self.sync_plugin_row();
        self.picker.set_selected(3);
        self.update();
    }

    /// Show `action` for editing.
    pub(crate) fn populate(&self, action: &Action) {
        self.reset();
        match action {
            Action::Feature(FeatureAction::Plugin(binding)) => {
                self.ensure_plugin_row();
                self.picker.set_selected(STEP_PLUGIN);
                self.plugin.populate(binding);
            }
            Action::Feature(feature) => {
                let sound = matches!(
                    feature,
                    FeatureAction::Desktop(
                        sdeck_plus::desktop_actions::DesktopAction::PlayAudio { .. }
                    )
                );
                self.picker.set_selected(if sound { 6 } else { 5 });
                self.feature.populate(feature);
            }
            other => {
                if let Some(index) = core_index(other) {
                    self.picker.set_selected(index);
                }
                self.core.populate(other);
            }
        }
        self.update();
    }

    /// The action the form describes, built and checked as the binding editor does.
    pub(crate) fn action(&self, store: &LayerStore) -> Result<Action, String> {
        match self.kind().ok_or("Select an action.")? {
            ActionKind::Plugin => Ok(Action::Feature(FeatureAction::Plugin(
                self.plugin.binding()?,
            ))),
            ActionKind::Extended => Ok(Action::Feature(self.feature.parse()?)),
            kind => self.core.action(store, kind),
        }
    }
}
