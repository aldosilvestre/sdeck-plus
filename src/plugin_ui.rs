//! Native GTK pieces for external plugins: the shared registry view, the generic settings form
//! and the per-plugin sidebar controls. Browsing and saving only read manifests and the
//! enabled state; a plugin runs solely through the plugin worker (`plugin_runtime`).
use gtk::{prelude::*, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation};
use sdeck_plus::binding_editor::{ControlKind, EventKind};
use sdeck_plus::plugins::{missing_commands, Requirement, StageOptions};
use sdeck_plus::plugins::{
    PluginAction, PluginBinding, PluginControl, PluginManifest, PluginRegistry, SettingField,
    Settings,
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    path::PathBuf,
    rc::{Rc, Weak},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// The action-type row the plugin editor answers to (after "None").
pub(crate) const PLUGIN_PICKER: u32 = 8;

pub(crate) const UNTRUSTED_WARNING: &str =
    "Plugins are separate programs that run with your user's privileges: no administrator \
(root) rights are requested or given, but a plugin can do what you can do. Process \
separation is not sandboxing, so enable only plugins you trust.";

/// The plugin control an event of `kind` on `control` maps to.
pub(crate) fn plugin_control(control: ControlKind, event: EventKind) -> PluginControl {
    match (control, event) {
        (ControlKind::Touch, _) => PluginControl::Touch,
        (ControlKind::Encoder, EventKind::EncoderPress) => PluginControl::DialPress,
        (ControlKind::Encoder, _) => PluginControl::DialTurn,
        (ControlKind::Button, _) => PluginControl::Key,
    }
}

pub(crate) fn control_name(control: PluginControl) -> &'static str {
    match control {
        PluginControl::Key => "Key",
        PluginControl::DialTurn => "Dial turn",
        PluginControl::DialPress => "Dial press",
        PluginControl::Touch => "Touch strip",
    }
}

/// Where a saved binding stands against the installed plugins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BindingState {
    Available,
    Disabled,
    Missing,
    UnknownAction,
    Invalid(String),
}

impl BindingState {
    fn describe(&self, binding: &PluginBinding) -> String {
        let (plugin, action) = (&binding.plugin_id, &binding.action_id);
        match self {
            Self::Available | Self::Disabled => String::new(),
            Self::Missing => format!(
                "Plugin \"{plugin}\" is not installed. The saved action \"{action}\" and its settings are kept; install the plugin and rescan, or choose another action."
            ),
            Self::UnknownAction => format!(
                "Plugin \"{plugin}\" no longer offers \"{action}\". The saved settings are kept until you choose another action."
            ),
            Self::Invalid(error) => format!(
                "The saved settings for \"{plugin}/{action}\" do not fit the installed plugin ({error}). They are kept until you choose another action."
            ),
        }
    }
}

type Listener = Rc<dyn Fn() -> bool>;

#[derive(Default)]
struct HubState {
    registry: RefCell<Option<PluginRegistry>>,
    root_error: RefCell<Option<String>>,
    control: Cell<Option<PluginControl>>,
    changed: RefCell<Vec<Listener>>,
    control_changed: RefCell<Vec<Listener>>,
    /// Set while an install runs on a worker; installs are one at a time because the library
    /// does not serialise replacements of the same plugin.
    installing: Arc<AtomicBool>,
    add_dialog: RefCell<Weak<crate::plugin_add_ui::State>>,
    /// Staging options tests inject (a fake git, a scratch temp directory); never configuration.
    stage_override: RefCell<Option<StageOptions>>,
    #[cfg(test)]
    install_delay: Cell<std::time::Duration>,
    #[cfg(test)]
    worker_panics: Cell<bool>,
    #[cfg(test)]
    spawn_fails: Cell<bool>,
}

/// Held by the worker that installs; releases the hub's install slot when dropped.
pub(crate) struct InstallGuard(Arc<AtomicBool>);

impl Drop for InstallGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// One session's view of the plugin registry. Nothing is read until a root is loaded, so
/// editors built without plugins (and tests) never touch the user's data directory.
#[derive(Clone, Default)]
pub(crate) struct PluginHub(Rc<HubState>);

fn notify(listeners: &RefCell<Vec<Listener>>) {
    let current: Vec<Listener> = listeners.borrow().clone();
    let dead: Vec<Listener> = current.into_iter().filter(|listener| !listener()).collect();
    listeners
        .borrow_mut()
        .retain(|listener| !dead.iter().any(|gone| Rc::ptr_eq(gone, listener)));
}

impl PluginHub {
    /// Reads the manifests under `root`; never runs plugin code.
    pub(crate) fn load(&self, root: PathBuf) {
        *self.0.root_error.borrow_mut() = None;
        *self.0.registry.borrow_mut() = Some(PluginRegistry::open(root));
        notify(&self.0.changed);
    }

    /// Loads `<data dir>/sdeck-plus/plugins`, the app-owned global root. Tests load temporary roots.
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn load_default(&self) {
        match sdeck_plus::device_roots::data_dir() {
            Ok(dir) => self.load(dir.join("plugins")),
            Err(error) => {
                *self.0.root_error.borrow_mut() = Some(format!("No plugin folder: {error}"));
                notify(&self.0.changed);
            }
        }
    }

    pub(crate) fn root(&self) -> Option<PathBuf> {
        self.0
            .registry
            .borrow()
            .as_ref()
            .map(|registry| registry.root().to_owned())
    }

    /// Takes the single install slot; `None` while another install is running.
    pub(crate) fn try_begin_install(&self) -> Option<InstallGuard> {
        self.0
            .installing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| InstallGuard(self.0.installing.clone()))
    }

    /// How the add dialog stages candidates: defaults, aimed at this hub's plugins folder.
    pub(crate) fn stage_options(&self) -> StageOptions {
        let options = self.0.stage_override.borrow().clone().unwrap_or_default();
        match self.root() {
            Some(root) => options.plugins_root(root),
            None => options,
        }
    }

    #[cfg(test)]
    pub(crate) fn use_stage_options(&self, options: StageOptions) {
        *self.0.stage_override.borrow_mut() = Some(options);
    }

    /// Test hook: the install worker waits this long before installing.
    #[cfg(test)]
    pub(crate) fn set_install_delay(&self, delay: std::time::Duration) {
        self.0.install_delay.set(delay);
    }

    /// Test hooks: the next workers panic / thread creation fails.
    #[cfg(test)]
    pub(crate) fn set_worker_panics(&self, on: bool) {
        self.0.worker_panics.set(on);
    }

    #[cfg(test)]
    pub(crate) fn worker_panics(&self) -> bool {
        self.0.worker_panics.get()
    }

    #[cfg(test)]
    pub(crate) fn set_spawn_fails(&self, on: bool) {
        self.0.spawn_fails.set(on);
    }

    #[cfg(test)]
    pub(crate) fn spawn_fails(&self) -> bool {
        self.0.spawn_fails.get()
    }

    #[cfg(test)]
    pub(crate) fn install_delay(&self) -> std::time::Duration {
        self.0.install_delay.get()
    }

    pub(crate) fn add_dialog(&self) -> Option<Rc<crate::plugin_add_ui::State>> {
        self.0.add_dialog.borrow().upgrade()
    }

    pub(crate) fn set_add_dialog(&self, dialog: &Rc<crate::plugin_add_ui::State>) {
        *self.0.add_dialog.borrow_mut() = Rc::downgrade(dialog);
    }

    pub(crate) fn rescan(&self) {
        if let Some(registry) = self.0.registry.borrow_mut().as_mut() {
            registry.rescan();
        }
        notify(&self.0.changed);
    }

    /// Installed plugins with their enabled state, ordered by id.
    pub(crate) fn plugins(&self) -> Vec<(PluginManifest, bool)> {
        self.0
            .registry
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |registry| {
                registry
                    .plugins()
                    .map(|manifest| (manifest.clone(), registry.is_enabled(&manifest.id)))
                    .collect()
            })
    }

    pub(crate) fn manifest(&self, id: &str) -> Option<(PluginManifest, bool)> {
        self.plugins()
            .into_iter()
            .find(|(manifest, _)| manifest.id == id)
    }

    /// Malformed plugins and an unusable enabled-state file, as readable messages.
    pub(crate) fn problems(&self) -> Vec<String> {
        let mut problems: Vec<String> = self.0.root_error.borrow().iter().cloned().collect();
        if let Some(registry) = self.0.registry.borrow().as_ref() {
            problems.extend(registry.problems().iter().map(|problem| {
                format!(
                    "Plugin folder \"{}\" was skipped: {}",
                    problem.directory, problem.error
                )
            }));
            if let Some(error) = registry.state_error() {
                problems.push(format!(
                    "{error}. Nothing is enabled and plugins cannot be enabled or disabled until plugins.json is repaired."
                ));
            }
        }
        problems
    }

    /// Explicitly enables or disables a plugin and saves the choice.
    pub(crate) fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let result = match self.0.registry.borrow_mut().as_mut() {
            Some(registry) => registry
                .set_enabled(id, enabled)
                .map_err(|error| error.to_string()),
            None => Err("Plugins are not loaded.".into()),
        };
        notify(&self.0.changed);
        result
    }

    pub(crate) fn binding_state(&self, binding: &PluginBinding) -> BindingState {
        let Some((manifest, enabled)) = self.manifest(&binding.plugin_id) else {
            return BindingState::Missing;
        };
        let Some(action) = manifest.action(&binding.action_id) else {
            return BindingState::UnknownAction;
        };
        match action.validate_settings(&binding.settings) {
            Err(error) => BindingState::Invalid(error.to_string()),
            Ok(_) if enabled => BindingState::Available,
            Ok(_) => BindingState::Disabled,
        }
    }

    /// Whether the installed plugin offers `action` for `control`.
    pub(crate) fn supports(&self, plugin: &str, action: &str, control: PluginControl) -> bool {
        self.manifest(plugin)
            .and_then(|(manifest, _)| {
                manifest
                    .action(action)
                    .map(|a| a.controls.contains(&control))
            })
            .unwrap_or(false)
    }

    /// The control the editor currently addresses; the sidebar dims actions that do not fit.
    pub(crate) fn control(&self) -> Option<PluginControl> {
        self.0.control.get()
    }

    pub(crate) fn set_control(&self, control: Option<PluginControl>) {
        if self.0.control.replace(control) != control {
            notify(&self.0.control_changed);
        }
    }

    /// The enabled plugin actions that declare a display, as `(plugin id, action id)`. Reads
    /// the cached manifests only.
    pub(crate) fn display_actions(&self) -> BTreeSet<(String, String)> {
        self.plugins()
            .into_iter()
            .filter(|(_, enabled)| *enabled)
            .flat_map(|(manifest, _)| {
                manifest
                    .actions
                    .iter()
                    .filter(|action| action.display.is_some())
                    .map(|action| (manifest.id.clone(), action.id.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Calls `listener` with the display actions after the plugin list changes, until the hub
    /// is gone. It holds the hub weakly, so it never keeps the session alive.
    pub(crate) fn on_displays(
        &self,
        listener: impl Fn(BTreeSet<(String, String)>) -> bool + 'static,
    ) {
        let state = Rc::downgrade(&self.0);
        self.on_change(move || {
            state
                .upgrade()
                .is_some_and(|state| listener(PluginHub(state).display_actions()))
        });
    }

    /// Calls `listener` after the plugin list changes, until it returns false.
    pub(crate) fn on_change(&self, listener: impl Fn() -> bool + 'static) {
        self.0.changed.borrow_mut().push(Rc::new(listener));
    }

    /// Calls `listener` after the addressed control changes, until it returns false.
    pub(crate) fn on_control(&self, listener: impl Fn() -> bool + 'static) {
        self.0.control_changed.borrow_mut().push(Rc::new(listener));
    }
}

/// The icon-only rescan control at the right end of the binding toolbar. It only reads the
/// plugin folder again; nothing is run.
pub(crate) fn rescan_button(hub: &PluginHub) -> Button {
    let rescan = crate::ui::icon_button("view-refresh-symbolic", "Update plugins");
    rescan.set_halign(gtk::Align::End);
    let hub = hub.clone();
    rescan.connect_clicked(move |_| hub.rescan());
    rescan
}

/// The icon-only "Add plugin…" control, left of the rescan control in the binding toolbar. It
/// opens the single add dialog over the window it sits in; `installed` receives the message of
/// a finished install.
pub(crate) fn add_button(hub: &PluginHub, installed: crate::plugin_add_ui::Notify) -> Button {
    let add = crate::ui::icon_button("list-add-symbolic", "Add plugin…");
    add.update_property(&[gtk::accessible::Property::Label("Add plugin")]);
    add.set_halign(gtk::Align::End);
    let hub = hub.clone();
    add.connect_clicked(move |button| {
        let parent = button
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok());
        crate::plugin_add_ui::open(parent.as_ref(), &hub, installed.clone());
    });
    add
}

/// Where the plugin entries of the sidebar begin: a divider, a "Plugins" heading row, then
/// plugin problems or the empty-state hint. Always shown, so the way to add a plugin (the
/// toolbar's Add plugin button) is explained before any plugin exists.
pub(crate) fn heading(hub: &PluginHub) -> GtkBox {
    let problems = hub.problems();
    let root = hub.root();
    let bar = GtkBox::new(Orientation::Vertical, 6);
    bar.add_css_class("plugin-toolbar");
    bar.append(&gtk::Separator::new(Orientation::Horizontal));
    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.add_css_class("plugin-heading-row");
    let title = Label::builder()
        .label("Plugins")
        .xalign(0.0)
        .hexpand(true)
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    title.add_css_class("plugin-heading");
    row.append(&title);
    bar.append(&row);
    for problem in problems {
        let label = Label::builder()
            .label(&problem)
            .wrap(true)
            .xalign(0.0)
            .max_width_chars(38)
            .accessible_role(gtk::AccessibleRole::Alert)
            .build();
        label.add_css_class("feedback-error");
        bar.append(&label);
    }
    if hub.plugins().is_empty() {
        let hint = match root {
            Some(root) => format!(
                "No plugins yet. Use the Add plugin button in the toolbar, or copy a plugin folder into {} and use Update plugins.",
                root.display()
            ),
            None => "No plugins yet. Use the Add plugin button in the toolbar to add one.".to_owned(),
        };
        bar.append(&crate::ui::helper(&hint));
    }
    bar
}

/// The warning and the explicit enable switch at the top of one plugin's sidebar item.
pub(crate) fn preface(
    hub: &PluginHub,
    manifest: &PluginManifest,
    enabled: bool,
    fail: Rc<dyn Fn(&str)>,
) -> GtkBox {
    let preface = GtkBox::new(Orientation::Vertical, 6);
    preface.add_css_class("plugin-preface");
    let warning = Label::new(Some(UNTRUSTED_WARNING));
    warning.set_wrap(true);
    warning.set_xalign(0.0);
    warning.set_max_width_chars(38);
    warning.add_css_class("feedback-warning");
    preface.append(&warning);
    let toggle =
        CheckButton::with_label(&format!("Enable {} (v{})", manifest.name, manifest.version));
    toggle.set_active(enabled);
    let (hub, id) = (hub.clone(), manifest.id.clone());
    toggle.connect_toggled(move |toggle| {
        if let Err(error) = hub.set_enabled(&id, toggle.is_active()) {
            fail(&error);
        }
    });
    preface.append(&toggle);
    if let Some(block) = requirements_block(manifest) {
        preface.append(&block);
    }
    if !enabled {
        preface.append(&crate::ui::helper(
            "Disabled: its actions are listed but cannot be assigned or run.",
        ));
    }
    preface
}

/// The compact "Requires" list of one plugin: each declared requirement with its kind, name,
/// version and an "(optional)" marker (description as tooltip), plus an alert for every required
/// command that no `PATH` directory provides. Built from a fresh lookup each time the sidebar is
/// rebuilt (every rescan or refresh); it only looks at files and never blocks enabling or
/// assigning. `None` when the plugin declares nothing.
pub(crate) fn requirements_block(manifest: &PluginManifest) -> Option<GtkBox> {
    if manifest.requirements.is_empty() {
        return None;
    }
    let missing: Vec<&Requirement> = missing_commands(&manifest.requirements);
    let block = GtkBox::new(Orientation::Vertical, 2);
    block.add_css_class("plugin-requirements");
    block.append(&crate::ui::helper("Requires"));
    for requirement in &manifest.requirements {
        let mut text = format!("{}: {}", requirement.kind.as_str(), requirement.name);
        if let Some(version) = &requirement.version {
            text.push_str(&format!(" {version}"));
        }
        if requirement.optional {
            text.push_str(" (optional)");
        }
        let row = crate::ui::helper(&text);
        row.set_tooltip_text(requirement.description.as_deref());
        block.append(&row);
        if missing.contains(&requirement) {
            if requirement.optional {
                block.append(&crate::ui::helper(&format!(
                    "Not found: {}",
                    requirement.name
                )));
            } else {
                let alert = Label::builder()
                    .label(format!("Missing: {}", requirement.name))
                    .wrap(true)
                    .xalign(0.0)
                    .max_width_chars(38)
                    .accessible_role(gtk::AccessibleRole::Alert)
                    .build();
                alert.add_css_class("feedback-error");
                block.append(&alert);
            }
        }
    }
    Some(block)
}

enum Input {
    Text(Entry),
    Integer(gtk::SpinButton),
    Boolean(CheckButton),
}

struct FieldWidget {
    id: String,
    input: Input,
}

type Callback = Rc<dyn Fn()>;

struct EditorState {
    root: GtkBox,
    hub: PluginHub,
    plugin: gtk::DropDown,
    plugin_model: gtk::StringList,
    action: gtk::DropDown,
    action_model: gtk::StringList,
    description: Label,
    notice: Label,
    form: GtkBox,
    fields: RefCell<Vec<FieldWidget>>,
    plugin_ids: RefCell<Vec<String>>,
    actions: RefCell<Vec<PluginAction>>,
    /// A saved binding the installed plugins cannot show; kept verbatim until another action is chosen.
    preserved: RefCell<Option<PluginBinding>>,
    /// The saved binding `populate` showed and the user has not replaced; a rescan that makes it
    /// unavailable must keep it instead of falling back to the first plugin and action.
    saved: RefCell<Option<PluginBinding>>,
    quiet: Cell<bool>,
    changed: RefCell<Vec<Callback>>,
}

/// Generic form for plugin actions: a plugin and action choice plus one native widget per
/// manifest field (text, bounded integer, boolean). Plugins cannot inject their own UI.
#[derive(Clone)]
pub(crate) struct PluginEditor(Rc<EditorState>);

impl std::ops::Deref for PluginEditor {
    type Target = GtkBox;

    fn deref(&self) -> &GtkBox {
        &self.0.root
    }
}

impl PluginEditor {
    pub(crate) fn new(hub: &PluginHub) -> Self {
        let root = GtkBox::new(Orientation::Vertical, 12);
        let plugin_model = gtk::StringList::new(&[]);
        let plugin = gtk::DropDown::new(Some(plugin_model.clone()), gtk::Expression::NONE);
        let action_model = gtk::StringList::new(&[]);
        let action = gtk::DropDown::new(Some(action_model.clone()), gtk::Expression::NONE);
        let notice = Label::new(None);
        notice.set_wrap(true);
        notice.set_xalign(0.0);
        notice.set_max_width_chars(38);
        let description = crate::ui::helper("");
        let form = GtkBox::new(Orientation::Vertical, 10);
        root.append(&crate::labeled("Plugin", &plugin));
        root.append(&crate::labeled("Plugin action", &action));
        root.append(&description);
        root.append(&notice);
        root.append(&form);
        let editor = Self(Rc::new(EditorState {
            root,
            hub: hub.clone(),
            plugin,
            plugin_model,
            action,
            action_model,
            description,
            notice,
            form,
            fields: RefCell::default(),
            plugin_ids: RefCell::default(),
            actions: RefCell::default(),
            preserved: RefCell::default(),
            saved: RefCell::default(),
            quiet: Cell::new(false),
            changed: RefCell::default(),
        }));
        let weak = Rc::downgrade(&editor.0);
        editor.0.plugin.connect_selected_notify(move |_| {
            if let Some(state) = weak.upgrade().filter(|state| !state.quiet.get()) {
                let editor = Self(state);
                editor.user_chose(true);
            }
        });
        let weak = Rc::downgrade(&editor.0);
        editor.0.action.connect_selected_notify(move |_| {
            if let Some(state) = weak.upgrade().filter(|state| !state.quiet.get()) {
                let editor = Self(state);
                editor.user_chose(false);
            }
        });
        let weak = Rc::downgrade(&editor.0);
        hub.on_change(move || {
            weak.upgrade().is_some_and(|state| {
                Self(state).refresh();
                true
            })
        });
        let weak = Rc::downgrade(&editor.0);
        hub.on_control(move || {
            weak.upgrade().is_some_and(|state| {
                Self(state).refresh();
                true
            })
        });
        editor.reset();
        editor
    }

    pub(crate) fn connect_changed(&self, notify: impl Fn() + Clone + 'static) {
        self.0.changed.borrow_mut().push(Rc::new(notify));
    }

    fn emit(&self) {
        if self.0.quiet.get() {
            return;
        }
        let callbacks: Vec<Callback> = self.0.changed.borrow().clone();
        for callback in callbacks {
            callback();
        }
    }

    /// The user picked another plugin or action: any kept binding is replaced.
    fn user_chose(&self, plugin_changed: bool) {
        self.0.preserved.borrow_mut().take();
        self.0.saved.borrow_mut().take();
        let plugin = self.selected_plugin();
        let action = if plugin_changed {
            None
        } else {
            self.selected_action_id()
        };
        self.show(plugin.as_deref(), action.as_deref(), None);
        self.emit();
    }

    fn selected_plugin(&self) -> Option<String> {
        let index = usize::try_from(self.0.plugin.selected()).ok()?;
        self.0.plugin_ids.borrow().get(index).cloned()
    }

    fn selected_action(&self) -> Option<PluginAction> {
        let index = usize::try_from(self.0.action.selected()).ok()?;
        self.0.actions.borrow().get(index).cloned()
    }

    fn selected_action_id(&self) -> Option<String> {
        self.selected_action().map(|action| action.id)
    }

    /// Re-reads the hub (after a rescan, an enable change or another control) and keeps what
    /// the user has chosen where it still exists.
    fn refresh(&self) {
        let kept = self.0.preserved.borrow().clone();
        match kept {
            Some(binding) => self.populate(&binding),
            None => {
                let saved = self.0.saved.borrow().clone();
                if let Some(saved) = saved.filter(|saved| {
                    !matches!(
                        self.0.hub.binding_state(saved),
                        BindingState::Available | BindingState::Disabled
                    )
                }) {
                    self.populate(&saved);
                    return;
                }
                let (plugin, action) = (self.selected_plugin(), self.selected_action_id());
                let settings = self.current_settings();
                self.show(plugin.as_deref(), action.as_deref(), Some(&settings));
            }
        }
    }

    /// Fills the plugin and action lists for the current control and selects the requested
    /// entries (or the first ones); returns whether the requested action was shown.
    fn show(
        &self,
        plugin: Option<&str>,
        action: Option<&str>,
        settings: Option<&Settings>,
    ) -> bool {
        let state = &self.0;
        let was_quiet = state.quiet.replace(true);
        let plugins = state.hub.plugins();
        let names: Vec<String> = plugins
            .iter()
            .map(|(manifest, enabled)| {
                if *enabled {
                    manifest.name.clone()
                } else {
                    format!("{} (disabled)", manifest.name)
                }
            })
            .collect();
        *state.plugin_ids.borrow_mut() = plugins.iter().map(|(m, _)| m.id.clone()).collect();
        state.plugin_model.splice(
            0,
            state.plugin_model.n_items(),
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        let plugin_index = plugin
            .and_then(|id| plugins.iter().position(|(manifest, _)| manifest.id == id))
            .unwrap_or(0);
        state.plugin.set_selected(if plugins.is_empty() {
            gtk::INVALID_LIST_POSITION
        } else {
            plugin_index as u32
        });
        let control = state.hub.control();
        let offered: Vec<PluginAction> = plugins
            .get(plugin_index)
            .map(|(manifest, _)| {
                manifest
                    .actions
                    .iter()
                    .filter(|candidate| {
                        control.is_none_or(|control| candidate.controls.contains(&control))
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let labels: Vec<&str> = offered
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect();
        state
            .action_model
            .splice(0, state.action_model.n_items(), &labels);
        let action_index =
            action.and_then(|id| offered.iter().position(|candidate| candidate.id == id));
        let found = action_index.is_some() || action.is_none();
        let index = action_index.unwrap_or(0);
        state.action.set_selected(if offered.is_empty() {
            gtk::INVALID_LIST_POSITION
        } else {
            index as u32
        });
        let chosen = offered.get(index).cloned();
        *state.actions.borrow_mut() = offered;
        self.build_form(chosen.as_ref(), settings);
        let disabled = plugins
            .get(plugin_index)
            .is_some_and(|(_, enabled)| !enabled);
        self.set_notice(if plugins.is_empty() {
            "No plugins are installed. Add one to the plugin folder and rescan."
        } else if chosen.is_none() {
            "This plugin has no actions for the selected control."
        } else if disabled {
            "This plugin is disabled. The binding can be saved; enable the plugin in the sidebar to run it."
        } else {
            ""
        });
        state.quiet.set(was_quiet);
        found
    }

    fn set_notice(&self, text: &str) {
        self.0.notice.set_text(text);
        self.0.notice.set_visible(!text.is_empty());
        crate::ui::clear_feedback(&self.0.notice);
        self.0.notice.add_css_class("feedback-warning");
    }

    fn build_form(&self, action: Option<&PluginAction>, settings: Option<&Settings>) {
        let state = &self.0;
        while let Some(child) = state.form.first_child() {
            state.form.remove(&child);
        }
        state.fields.borrow_mut().clear();
        state.description.set_text(
            action
                .and_then(|action| action.description.as_deref())
                .unwrap_or(""),
        );
        state
            .description
            .set_visible(!state.description.text().is_empty());
        let Some(action) = action else { return };
        for field in &action.settings {
            let saved = settings.and_then(|settings| settings.get(field.id()));
            let input = match field {
                SettingField::Text {
                    max_length,
                    default,
                    ..
                } => {
                    let entry = Entry::builder()
                        .hexpand(true)
                        .max_length(*max_length as i32)
                        .build();
                    let text = saved
                        .and_then(Value::as_str)
                        .or(default.as_deref())
                        .unwrap_or("");
                    entry.set_text(text);
                    let weak = Rc::downgrade(&self.0);
                    entry.connect_changed(move |_| {
                        if let Some(state) = weak.upgrade() {
                            Self(state).emit();
                        }
                    });
                    Input::Text(entry)
                }
                SettingField::Integer {
                    min, max, default, ..
                } => {
                    let spin = gtk::SpinButton::with_range(*min as f64, *max as f64, 1.0);
                    spin.set_hexpand(true);
                    let value = saved.and_then(Value::as_i64).or(*default).unwrap_or(0);
                    spin.set_value(value.clamp(*min, *max) as f64);
                    let weak = Rc::downgrade(&self.0);
                    spin.connect_value_changed(move |_| {
                        if let Some(state) = weak.upgrade() {
                            Self(state).emit();
                        }
                    });
                    Input::Integer(spin)
                }
                SettingField::Boolean { default, .. } => {
                    let check = CheckButton::new();
                    check.set_active(saved.and_then(Value::as_bool).or(*default).unwrap_or(false));
                    let weak = Rc::downgrade(&self.0);
                    check.connect_toggled(move |_| {
                        if let Some(state) = weak.upgrade() {
                            Self(state).emit();
                        }
                    });
                    Input::Boolean(check)
                }
            };
            let widget: gtk::Widget = match &input {
                Input::Text(widget) => widget.clone().upcast(),
                Input::Integer(widget) => widget.clone().upcast(),
                Input::Boolean(widget) => widget.clone().upcast(),
            };
            state.form.append(&crate::labeled(field.label(), &widget));
            state.fields.borrow_mut().push(FieldWidget {
                id: field.id().to_owned(),
                input,
            });
        }
    }

    /// The form's values as settings; empty optional text is left out.
    fn current_settings(&self) -> Settings {
        let mut settings = Settings::new();
        for field in self.0.fields.borrow().iter() {
            let value = match &field.input {
                Input::Text(entry) if entry.text().is_empty() => continue,
                Input::Text(entry) => Value::from(entry.text().to_string()),
                Input::Integer(spin) => Value::from(spin.value_as_int() as i64),
                Input::Boolean(check) => Value::from(check.is_active()),
            };
            settings.insert(field.id.clone(), value);
        }
        settings
    }

    /// Shows a saved binding. One the installed plugins cannot show is kept verbatim.
    pub(crate) fn populate(&self, binding: &PluginBinding) {
        let state = self.0.hub.binding_state(binding);
        let shown = matches!(state, BindingState::Available | BindingState::Disabled)
            && self.show(
                Some(&binding.plugin_id),
                Some(&binding.action_id),
                Some(&binding.settings),
            );
        if shown {
            self.0.preserved.borrow_mut().take();
            *self.0.saved.borrow_mut() = Some(binding.clone());
            return;
        }
        let state = if matches!(state, BindingState::Available | BindingState::Disabled) {
            BindingState::UnknownAction
        } else {
            state
        };
        *self.0.preserved.borrow_mut() = Some(binding.clone());
        self.show(None, None, None);
        // Nothing is selected, so choosing any entry (even the first) replaces the kept binding.
        let was_quiet = self.0.quiet.replace(true);
        self.0.plugin.set_selected(gtk::INVALID_LIST_POSITION);
        self.0.action.set_selected(gtk::INVALID_LIST_POSITION);
        self.build_form(None, None);
        self.0.quiet.set(was_quiet);
        let mut text = state.describe(binding);
        if state == BindingState::UnknownAction {
            text = format!(
                "\"{}/{}\" is not available for the selected control or plugin. The saved settings are kept until you choose another action.",
                binding.plugin_id, binding.action_id
            );
        }
        self.set_notice(&text);
    }

    /// Starts a fresh draft for `plugin/action` (catalog assignment).
    pub(crate) fn select(&self, plugin: &str, action: &str) {
        self.0.preserved.borrow_mut().take();
        self.0.saved.borrow_mut().take();
        self.show(Some(plugin), Some(action), None);
        self.emit();
    }

    pub(crate) fn reset(&self) {
        self.0.preserved.borrow_mut().take();
        self.0.saved.borrow_mut().take();
        self.show(None, None, None);
    }

    /// The binding the form describes, validated against the installed manifest (which does
    /// not need to be enabled) or the kept saved binding.
    pub(crate) fn binding(&self) -> Result<PluginBinding, String> {
        if let Some(kept) = self.0.preserved.borrow().clone() {
            return Ok(kept);
        }
        let (Some(plugin), Some(action)) = (self.selected_plugin(), self.selected_action()) else {
            return Err("Select a plugin action.".into());
        };
        if let Some(control) = self
            .0
            .hub
            .control()
            .filter(|control| !action.controls.contains(control))
        {
            return Err(format!(
                "\"{}\" cannot be bound to a {}.",
                action.name,
                control_name(control).to_lowercase()
            ));
        }
        let settings = action
            .validate_settings(&self.current_settings())
            .map_err(|error| error.to_string())?;
        let binding = PluginBinding::new(plugin, action.id, settings);
        binding.validate().map_err(|error| error.to_string())?;
        Ok(binding)
    }

    /// Raw values, to tell whether the editor differs from a saved state.
    pub(crate) fn snapshot(&self) -> String {
        if let Some(kept) = self.0.preserved.borrow().as_ref() {
            return format!(
                "kept:{}/{}:{:?}",
                kept.plugin_id, kept.action_id, kept.settings
            );
        }
        format!(
            "{:?}/{:?}/{:?}",
            self.selected_plugin(),
            self.selected_action_id(),
            self.current_settings()
        )
    }

    #[cfg(test)]
    pub(crate) fn notice(&self) -> String {
        self.0.notice.text().to_string()
    }

    #[cfg(test)]
    pub(crate) fn field_count(&self) -> usize {
        self.0.fields.borrow().len()
    }

    #[cfg(test)]
    pub(crate) fn action_labels(&self) -> Vec<String> {
        self.0
            .actions
            .borrow()
            .iter()
            .map(|action| action.name.clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn set_text(&self, id: &str, text: &str) {
        for field in self.0.fields.borrow().iter() {
            if let (true, Input::Text(entry)) = (field.id == id, &field.input) {
                entry.set_text(text);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn integer_range(&self, id: &str) -> Option<(f64, f64)> {
        self.0
            .fields
            .borrow()
            .iter()
            .find_map(|field| match &field.input {
                Input::Integer(spin) if field.id == id => Some(spin.range()),
                _ => None,
            })
    }
}
