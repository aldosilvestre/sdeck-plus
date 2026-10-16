//! GTK-thread organization and timers; blocking adapters use the serial worker.
use crate::{ArtworkUi, BindingUi, LayerCommand};
use gtk::{glib, prelude::*, Box as GtkBox, Button, Entry, Label, Orientation};
use sdeck_plus::{
    device_control::DeviceAction,
    event_api::MonitorEvent,
    features::FeatureAction,
    info::{Glyph, InfoSource, InfoValue},
    layers::{Action, LayerStore, PageReturn},
    obs::ObsConfig,
    organization::{NavigationAction, Organization},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

mod configuration;
mod folders;
mod key_swap;
mod page_controls;
#[cfg(test)]
mod page_return_tests;
mod pages;

pub(crate) fn run_worker(
    receiver: mpsc::Receiver<LayerCommand>,
    output: mpsc::Sender<MonitorEvent>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut executor = sdeck_plus::layers::ActionExecutor::new(
        sdeck_plus::layers::SystemProcessRunner,
        sdeck_plus::keyboard::Keyboard::new(),
    );
    let mut interrupted = None;
    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        for error in executor.poll_errors() {
            if let Err(delivery) = output.send(MonitorEvent::LayerError(format!(
                "Desktop action failed: {error}"
            ))) {
                eprintln!("Desktop result delivery failed: {delivery}");
            }
        }
        let command = match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        // A closing window stops admission even when commands precede Shutdown.
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            interrupted = Some(command);
            break;
        }
        let event = match command {
            LayerCommand::Shutdown => break,
            LayerCommand::SwapKeys {
                store,
                artwork_root,
                layer,
                first,
                second,
            } => {
                let result = key_swap::persist(&store, &artwork_root, &layer, first, second)
                    .map_err(|error| error.to_string());
                Some(MonitorEvent::KeySwapResult {
                    layer,
                    first,
                    second,
                    result,
                })
            }
            LayerCommand::Persist(store, organization) => {
                let result = persist_snapshot(&store, organization.as_ref());
                match result {
                    Ok(()) => None,
                    Err(error) => Some(MonitorEvent::LayerError(format!(
                        "Could not save layer/view configuration: {error}"
                    ))),
                }
            }
            LayerCommand::SaveBinding(store, organization) => {
                Some(MonitorEvent::BindingSaveResult(
                    persist_snapshot(&store, organization.as_ref())
                        .map_err(|error| error.to_string()),
                ))
            }
            LayerCommand::SaveProfile(store, organization) => {
                Some(MonitorEvent::ProfileSaveResult(
                    persist_snapshot(&store, organization.as_ref())
                        .map_err(|error| error.to_string()),
                ))
            }
            LayerCommand::ExportConfiguration {
                store,
                organization,
                destination,
            } => {
                let result = persist_snapshot(&store, organization.as_ref())
                    .and_then(|()| sdeck_plus::config_bundle::export(store.root(), &destination))
                    .map_err(|error| error.to_string());
                Some(MonitorEvent::ConfigurationExportResult {
                    path: destination,
                    result,
                })
            }
            LayerCommand::ImportConfiguration { root, source } => {
                let result = sdeck_plus::config_bundle::import(&source, &root)
                    .map_err(|error| error.to_string());
                Some(MonitorEvent::ConfigurationImportResult { root, result })
            }
            LayerCommand::Execute { binding_id, action } => {
                crate::execute_layer_action_for(&mut executor, &binding_id, &action, &output);
                None
            }
            LayerCommand::ObsSave(config) => Some(match config.save() {
                Ok(()) => MonitorEvent::AppEvent {
                    name: "OBS settings saved.".into(),
                },
                Err(error) => {
                    MonitorEvent::LayerError(format!("OBS settings save failed: {error}"))
                }
            }),
            LayerCommand::ObsProbe(config) => {
                Some(match sdeck_plus::obs::Client::new(config).probe() {
                    Ok(status) => MonitorEvent::AppEvent {
                        name: format!(
                            "OBS connected · Scene: {} · Recording: {} · Streaming: {}",
                            status.current_scene, status.recording, status.streaming
                        ),
                    },
                    Err(error) => {
                        MonitorEvent::LayerError(format!("OBS connection failed: {error}"))
                    }
                })
            }
        };
        if let Some(event) = event {
            if let Err(error) = output.send(event) {
                eprintln!("Worker result delivery failed: {error}");
            }
        }
    }
    // Closing cancels queued effects, but a just-saved configuration must survive.
    let mut final_store = None;
    let mut final_obs = None;
    for command in interrupted.into_iter().chain(receiver.try_iter()) {
        match command {
            LayerCommand::Persist(store, organization)
            | LayerCommand::SaveBinding(store, organization)
            | LayerCommand::SaveProfile(store, organization) => {
                final_store = Some((store, organization))
            }
            LayerCommand::ObsSave(config) => final_obs = Some(config),
            LayerCommand::SwapKeys {
                store,
                artwork_root,
                layer,
                first,
                second,
            } => {
                if let Some((previous, organization)) = final_store.take() {
                    if let Err(error) = persist_snapshot(&previous, organization.as_ref()) {
                        eprintln!("Could not save before closing key swap: {error}");
                    }
                }
                if let Err(error) = key_swap::persist(&store, &artwork_root, &layer, first, second)
                {
                    eprintln!("Could not swap keys while closing: {error}");
                }
            }
            LayerCommand::Execute { .. }
            | LayerCommand::ObsProbe(_)
            | LayerCommand::ExportConfiguration { .. }
            | LayerCommand::ImportConfiguration { .. }
            | LayerCommand::Shutdown => {}
        }
    }
    if let Some((store, organization)) = final_store {
        if let Err(error) = persist_snapshot(&store, organization.as_ref()) {
            eprintln!("Could not save closing layer/view configuration: {error}");
        }
    }
    if let Some(config) = final_obs {
        if let Err(error) = config.save() {
            eprintln!("Could not save closing OBS settings: {error}");
        }
    }
}

fn persist_snapshot(
    store: &LayerStore,
    organization: Option<&Organization>,
) -> std::io::Result<()> {
    store.persist()?;
    match organization {
        Some(organization) => organization.persist(),
        None => Ok(()),
    }
}

#[derive(Clone)]
pub(crate) struct FeatureRuntime {
    layers: Rc<RefCell<LayerStore>>,
    organization: Rc<RefCell<Option<Organization>>>,
    ui: BindingUi,
    artwork: ArtworkUi,
    output: mpsc::Sender<LayerCommand>,
    updating: Rc<Cell<bool>>,
    key_swap_pending: Rc<Cell<bool>>,
    key_swap_deferred_persist: Rc<Cell<bool>>,
    key_swap_root: Rc<RefCell<glib::WeakRef<gtk::Widget>>>,
    configuration_operation: Rc<Cell<Option<configuration::Operation>>>,
    configuration_root: Rc<RefCell<glib::WeakRef<gtk::Widget>>>,
    pub(crate) indicator: Label,
    pub(crate) status: Label,
    pub(crate) timer_status: Label,
    timer: Rc<RefCell<Option<glib::SourceId>>>,
    page_return: Rc<RefCell<Option<glib::SourceId>>>,
    info_watch: Rc<RefCell<Option<mpsc::Sender<std::collections::BTreeSet<InfoSource>>>>>,
    brightness: Rc<Cell<u8>>,
    sleeping: Rc<Cell<bool>>,
    device: Rc<RefCell<Option<mpsc::Sender<DeviceAction>>>>,
    events: Rc<RefCell<Option<mpsc::Sender<MonitorEvent>>>>,
    page_controls: page_controls::PageControls,
    organization_management: GtkBox,
    profile_delete: Button,
    profile_tabs: gtk::Notebook,
    profile_pages: Rc<RefCell<Vec<(String, GtkBox)>>>,
    profile_content: Rc<RefCell<Option<GtkBox>>>,
    profile_memory: Rc<RefCell<std::collections::BTreeMap<String, String>>>,
    legacy_picker: GtkBox,
    profile_settings_model: gtk::StringList,
    profile_settings_picker: gtk::DropDown,
    profile_application: Entry,
    profile_application_apply: Button,
    profile_application_status: Label,
    focused_application: Rc<RefCell<Option<sdeck_plus::app_focus::FocusedApplication>>>,
    focus_received: Rc<Cell<bool>>,
    manual_profile: Rc<Cell<bool>>,
    pub(crate) obs_panel: gtk::Expander,
    endpoint: Entry,
    password: Entry,
    obs_save: Button,
    obs_test: Button,
    obs_status: Label,
    obs_valid: Rc<Cell<bool>>,
}

impl FeatureRuntime {
    pub(crate) fn new(
        layers: Rc<RefCell<LayerStore>>,
        ui: BindingUi,
        artwork: ArtworkUi,
        output: mpsc::Sender<LayerCommand>,
        updating: Rc<Cell<bool>>,
    ) -> Self {
        let organization = Organization::in_memory(&layers.borrow());
        let indicator = Label::new(None);
        indicator.set_xalign(0.0);
        indicator.set_ellipsize(gtk::pango::EllipsizeMode::End);
        indicator.add_css_class("muted");
        let status = Label::new(None);
        status.set_visible(false);
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.set_max_width_chars(70);
        status.add_css_class("muted");
        status.connect_label_notify(|label| {
            crate::ui::clear_feedback(label);
            label.set_visible(!label.text().is_empty());
            label.set_tooltip_text(Some(&label.text()));
        });
        let timer_status = Label::new(None);
        timer_status.set_visible(false);
        timer_status.connect_label_notify(|label| label.set_visible(!label.text().is_empty()));
        timer_status.connect_label_notify(crate::ui::clear_feedback);
        timer_status.set_xalign(0.0);
        timer_status.add_css_class("muted");
        timer_status.set_visible(false);
        let page_controls = page_controls::PageControls::new();
        let organization_management = GtkBox::new(Orientation::Vertical, 12);
        let legacy_picker = crate::labeled("Layer (organization unavailable)", &ui.layer_picker);
        legacy_picker.set_visible(false);
        organization_management.append(&legacy_picker);
        let profile_tabs = gtk::Notebook::builder()
            .scrollable(true)
            .show_border(false)
            .hexpand(true)
            .build();
        profile_tabs.add_css_class("profile-tabs");
        let profile_settings_model = gtk::StringList::new(&[]);
        let profile_settings_picker =
            gtk::DropDown::new(Some(profile_settings_model.clone()), gtk::Expression::NONE);
        let profile_application = Entry::builder()
            .placeholder_text("Window class or executable")
            .width_chars(16)
            .hexpand(true)
            .build();
        let profile_application_apply = crate::ui::button("document-save-symbolic", "Save changes");
        profile_application_apply.add_css_class("suggested-action");
        let profile_application_status = Label::new(None);
        profile_application_status.set_wrap(true);
        profile_application_status.set_max_width_chars(34);
        profile_application_status.set_xalign(0.0);
        let profile_delete = crate::ui::button("edit-delete-symbolic", "Delete profile");
        profile_delete.add_css_class("destructive-action");
        let obs_panel = gtk::Expander::new(Some("OBS settings"));
        let fields = GtkBox::new(Orientation::Vertical, 8);
        fields.append(&crate::ui::helper(
            "Enable the WebSocket server in OBS under Tools, then enter its local address and password.",
        ));
        let config = ObsConfig::default();
        let endpoint = Entry::builder()
            .text(&config.endpoint)
            .hexpand(true)
            .width_chars(16)
            .build();
        let password = Entry::builder()
            .visibility(false)
            .hexpand(true)
            .width_chars(16)
            .build();
        fields.append(&crate::labeled("Local WebSocket endpoint", &endpoint));
        fields.append(&crate::labeled("Password", &password));
        let buttons = GtkBox::new(Orientation::Horizontal, 8);
        let obs_save = crate::ui::button("document-save-symbolic", "Save settings");
        obs_save.add_css_class("suggested-action");
        let obs_test = crate::ui::button("network-transmit-receive-symbolic", "Test connection");
        buttons.append(&obs_save);
        buttons.append(&obs_test);
        fields.append(&buttons);
        let obs_status = Label::new(Some("Connection tests query state only."));
        obs_status.connect_label_notify(crate::ui::clear_feedback);
        obs_status.set_xalign(0.0);
        obs_status.set_wrap(true);
        obs_status.set_max_width_chars(36);
        obs_status.add_css_class("muted");
        fields.append(&obs_status);
        obs_panel.set_child(Some(&fields));
        let runtime = Self {
            layers,
            organization: Rc::new(RefCell::new(Some(organization))),
            ui,
            artwork,
            output,
            updating,
            key_swap_pending: Rc::new(Cell::new(false)),
            key_swap_deferred_persist: Rc::new(Cell::new(false)),
            key_swap_root: Rc::new(RefCell::new(glib::WeakRef::new())),
            configuration_operation: Rc::new(Cell::new(None)),
            configuration_root: Rc::new(RefCell::new(glib::WeakRef::new())),
            indicator,
            status,
            timer_status,
            timer: Rc::new(RefCell::new(None)),
            page_return: Rc::new(RefCell::new(None)),
            info_watch: Rc::new(RefCell::new(None)),
            brightness: Rc::new(Cell::new(100)),
            sleeping: Rc::new(Cell::new(false)),
            device: Rc::new(RefCell::new(None)),
            events: Rc::new(RefCell::new(None)),
            page_controls,
            organization_management,
            profile_delete,
            profile_tabs,
            profile_pages: Rc::new(RefCell::new(Vec::new())),
            profile_content: Rc::new(RefCell::new(None)),
            profile_memory: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
            legacy_picker,
            profile_settings_model,
            profile_settings_picker,
            profile_application,
            profile_application_apply,
            profile_application_status,
            focused_application: Rc::new(RefCell::new(None)),
            focus_received: Rc::new(Cell::new(false)),
            manual_profile: Rc::new(Cell::new(false)),
            obs_panel,
            endpoint,
            password,
            obs_save,
            obs_test,
            obs_status,
            obs_valid: Rc::new(Cell::new(true)),
        };
        runtime.page_controls.bind(&runtime);
        let callback = runtime.clone();
        runtime
            .profile_tabs
            .connect_switch_page(move |_, _, index| {
                if callback.updating.get() {
                    return;
                }
                let profile = callback
                    .profile_pages
                    .borrow()
                    .get(index as usize)
                    .map(|(name, _)| name.clone());
                if let Some(profile) = profile {
                    callback.manual_profile.set(true);
                    callback.select_profile(&profile);
                }
            });
        runtime.organization_management.append(&crate::labeled(
            "Profile to edit",
            &runtime.profile_settings_picker,
        ));
        let application_row = crate::labeled(
            "Auto-switch application (optional)",
            &runtime.profile_application,
        );
        crate::ui::field_help(&application_row, "Automatic switching help",
            "Enter a window class or executable, such as kitty or firefox. This profile is selected when that application has focus; it does not launch the application. Leave empty for manual selection. All profile tabs stay visible. Base is always available and cannot have an application rule.");
        runtime.organization_management.append(&application_row);
        runtime
            .organization_management
            .append(&runtime.profile_application_status);
        let actions = GtkBox::new(Orientation::Horizontal, 12);
        actions.append(&runtime.profile_application_apply);
        actions.append(&runtime.profile_delete);
        runtime.profile_delete.set_tooltip_text(Some(
            "Delete the selected profile and all its pages, folders, bindings and artwork. Base cannot be deleted.",
        ));
        runtime.organization_management.append(&actions);
        let callback = runtime.clone();
        runtime
            .profile_delete
            .connect_clicked(move |_| callback.confirm_profile_removal());
        let callback = runtime.clone();
        runtime
            .profile_settings_picker
            .connect_selected_notify(move |_| {
                if !callback.updating.get() {
                    callback.refresh_application_editor();
                }
            });
        let callback = runtime.clone();
        runtime
            .profile_application_apply
            .connect_clicked(move |_| callback.save_profile_application());
        let callback = runtime.clone();
        runtime
            .profile_application
            .connect_activate(move |_| callback.save_profile_application());
        let callback = runtime.clone();
        runtime
            .profile_application
            .connect_changed(move |_| callback.refresh_save_state());
        let callback = runtime.clone();
        runtime
            .obs_save
            .connect_clicked(move |_| callback.submit_obs(false));
        let callback = runtime.clone();
        runtime
            .obs_test
            .connect_clicked(move |_| callback.submit_obs(true));
        runtime.refresh_indicator();
        runtime
    }

    pub(crate) fn controls(&self) -> &GtkBox {
        &self.page_controls.root
    }

    #[cfg(test)]
    pub(crate) fn folder_menu(&self) -> &gtk::MenuButton {
        &self.page_controls.folder_menu
    }

    pub(crate) fn management(&self) -> &GtkBox {
        &self.organization_management
    }

    pub(crate) fn profile_tabs(&self) -> &gtk::Notebook {
        &self.profile_tabs
    }

    pub(crate) fn attach_profile_content(&self, content: &GtkBox) {
        *self.profile_content.borrow_mut() = Some(content.clone());
        self.refresh_profiles();
    }

    fn select_profile(&self, profile: &str) {
        let key = {
            let organization = self.organization.borrow();
            let Some(organization) = organization.as_ref() else {
                return;
            };
            self.profile_memory
                .borrow()
                .get(profile)
                .filter(|key| {
                    self.layers.borrow().layers().contains_key(*key)
                        && organization.profile_for_key(key) == profile
                })
                .cloned()
                .or_else(|| organization.profile_key(profile))
        };
        let Some(key) = key else {
            return;
        };
        if !self.layers.borrow_mut().select(&key) {
            self.report("Could not select profile: its page is unavailable.", true);
            return;
        }
        self.refresh_selection();
        self.persist(self.layers.borrow().clone());
    }

    fn refresh_profiles(&self) {
        let (profiles, active) = match self.organization.borrow().as_ref() {
            Some(organization) => (
                organization.profiles(),
                organization.profile_for_key(self.layers.borrow().active()),
            ),
            None => (
                vec![sdeck_plus::layers::DEFAULT_LAYER.to_owned()],
                sdeck_plus::layers::DEFAULT_LAYER.to_owned(),
            ),
        };
        self.legacy_picker
            .set_visible(self.organization.borrow().is_none());
        self.profile_tabs
            .set_sensitive(self.organization.borrow().is_some());
        self.profile_memory
            .borrow_mut()
            .insert(active.clone(), self.layers.borrow().active().to_owned());
        let content = self.profile_content.borrow().clone();
        let previous = self.updating.replace(true);
        let selected_setting = self
            .profile_settings_picker
            .selected_item()
            .and_then(|item| item.downcast::<gtk::StringObject>().ok())
            .map(|item| item.string().to_string());
        crate::replace_string_list(&self.profile_settings_model, &profiles);
        let setting_index = selected_setting
            .as_ref()
            .and_then(|selected| profiles.iter().position(|name| name == selected))
            .or_else(|| profiles.iter().position(|name| name == &active))
            .unwrap_or(0);
        let settings_changed =
            selected_setting.as_deref() != profiles.get(setting_index).map(String::as_str);
        self.profile_settings_picker
            .set_selected(setting_index as u32);
        let old_names: Vec<_> = self
            .profile_pages
            .borrow()
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        if old_names != profiles {
            if let Some(content) = &content {
                if let Some(parent) = content
                    .parent()
                    .and_then(|parent| parent.downcast::<GtkBox>().ok())
                {
                    parent.remove(content);
                }
            }
            while self.profile_tabs.n_pages() > 0 {
                self.profile_tabs.remove_page(Some(0));
            }
            self.profile_pages.borrow_mut().clear();
            for profile in profiles {
                let page = GtkBox::new(Orientation::Vertical, 0);
                let label = Label::new(Some(&profile));
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_max_width_chars(18);
                label.set_width_chars(profile.chars().count().min(18) as i32);
                label.set_tooltip_text(Some(&profile));
                self.profile_tabs.append_page(&page, Some(&label));
                self.profile_pages.borrow_mut().push((profile, page));
            }
        }
        let selected = self
            .profile_pages
            .borrow()
            .iter()
            .position(|(name, _)| name == &active)
            .unwrap_or(0);
        let page = self
            .profile_pages
            .borrow()
            .get(selected)
            .map(|(_, page)| page.clone());
        if let (Some(content), Some(page)) = (content, page) {
            if content.parent().as_ref() != Some(page.upcast_ref()) {
                if let Some(parent) = content
                    .parent()
                    .and_then(|parent| parent.downcast::<GtkBox>().ok())
                {
                    parent.remove(&content);
                }
                page.append(&content);
            }
        }
        self.profile_tabs.set_current_page(Some(selected as u32));
        self.updating.set(previous);
        if settings_changed {
            self.refresh_application_editor();
        }
    }

    fn refresh_application_editor(&self) {
        let profile = self
            .profile_settings_picker
            .selected_item()
            .and_then(|item| item.downcast::<gtk::StringObject>().ok())
            .map(|item| item.string().to_string());
        let organization = self.organization.borrow();
        let editable = profile.as_deref() != Some(sdeck_plus::layers::DEFAULT_LAYER)
            && profile.is_some()
            && organization.is_some();
        self.profile_application.set_sensitive(editable);
        self.profile_delete
            .set_sensitive(editable && self.artwork.store.borrow().is_some());
        self.profile_settings_picker
            .set_sensitive(organization.is_some());
        let application = profile
            .as_deref()
            .and_then(|profile| organization.as_ref()?.profile_application(profile));
        self.profile_application.set_text(application.unwrap_or(""));
        self.refresh_save_state();
        crate::ui::clear_feedback(&self.profile_application_status);
        self.profile_application_status
            .set_text(match (profile.as_deref(), application) {
                (Some(sdeck_plus::layers::DEFAULT_LAYER), _) => {
                    "Base: always available, cannot be deleted."
                }
                (_, Some(_)) => "Automatic switching enabled.",
                (_, None) => "Manual selection — no application rule.",
            });
    }

    fn refresh_save_state(&self) {
        let profile = self
            .profile_settings_picker
            .selected_item()
            .and_then(|item| item.downcast::<gtk::StringObject>().ok())
            .map(|item| item.string().to_string());
        let organization = self.organization.borrow();
        let dirty = match (profile.as_deref(), organization.as_ref()) {
            (Some(profile), Some(organization)) if profile != sdeck_plus::layers::DEFAULT_LAYER => {
                self.profile_application.text().trim()
                    != organization.profile_application(profile).unwrap_or("")
            }
            _ => false,
        };
        self.profile_application_apply.set_sensitive(dirty);
    }

    fn save_profile_application(&self) {
        let Some(profile) = self
            .profile_settings_picker
            .selected_item()
            .and_then(|item| item.downcast::<gtk::StringObject>().ok())
            .map(|item| item.string().to_string())
        else {
            return;
        };
        let result = {
            let mut organization = self.organization.borrow_mut();
            let Some(organization) = organization.as_mut() else {
                return;
            };
            organization.set_profile_application(&profile, &self.profile_application.text())
        };
        match result {
            Ok(()) => {
                self.save_profile(self.layers.borrow().clone());
                self.refresh_save_state();
            }
            Err(error) => {
                self.profile_application_status.set_text(&error.to_string());
                crate::ui::feedback(&self.profile_application_status, true);
            }
        }
    }

    pub(crate) fn focus_changed(
        &self,
        result: Result<Option<sdeck_plus::app_focus::FocusedApplication>, String>,
    ) {
        let (focus, error) = match result {
            Ok(focus) => (focus, None),
            Err(error) => (None, Some(error)),
        };
        let duplicate =
            self.focus_received.replace(true) && *self.focused_application.borrow() == focus;
        *self.focused_application.borrow_mut() = focus;
        if !duplicate {
            self.reconcile_focus();
            self.refresh_indicator();
        }
        if let Some(error) = error {
            self.report(&format!("Application detection unavailable: {error}"), true);
        }
    }

    fn reconcile_focus(&self) {
        if self.key_swap_pending.get() || self.configuration_operation.get().is_some() {
            return;
        }
        let target = {
            let organization = self.organization.borrow();
            let Some(organization) = organization.as_ref() else {
                return;
            };
            if !organization.has_application_profiles() {
                return;
            }
            let focus = self.focused_application.borrow();
            let current = organization.profile_for_key(self.layers.borrow().active());
            if organization.profile_matches(&current, focus.as_ref()) {
                self.manual_profile.set(false);
                current
            } else {
                match organization
                    .profiles()
                    .into_iter()
                    .find(|profile| organization.profile_matches(profile, focus.as_ref()))
                {
                    Some(profile) => {
                        self.manual_profile.set(false);
                        profile
                    }
                    None if self.manual_profile.get() => current,
                    None => sdeck_plus::layers::DEFAULT_LAYER.to_owned(),
                }
            }
        };
        let current = self
            .organization
            .borrow()
            .as_ref()
            .map(|organization| organization.profile_for_key(self.layers.borrow().active()));
        if current.as_deref() != Some(target.as_str()) {
            self.select_profile(&target);
        }
    }

    pub(crate) fn configure(
        &self,
        organization: std::io::Result<Organization>,
        config: std::io::Result<ObsConfig>,
        device: mpsc::Sender<DeviceAction>,
        events: mpsc::Sender<MonitorEvent>,
    ) {
        *self.device.borrow_mut() = Some(device);
        let info_events = events.clone();
        *self.events.borrow_mut() = Some(events);
        let root = self.layers.borrow().root().to_owned();
        match sdeck_plus::info::spawn_worker(root, info_events) {
            Ok(sender) => *self.info_watch.borrow_mut() = Some(sender),
            Err(error) => eprintln!("Could not start the information reader: {error}"),
        }
        self.artwork.update_info(
            InfoSource::Brightness,
            InfoValue::gauge(Glyph::Sun, 100, "100%".into()),
        );
        self.sync_info_from(&self.layers.borrow());
        match organization {
            Ok(value) => *self.organization.borrow_mut() = Some(value),
            Err(error) => {
                *self.organization.borrow_mut() = None;
                self.page_controls.root.set_sensitive(false);
                self.page_controls.folders.set_sensitive(false);
                self.organization_management.set_sensitive(false);
                self.status.set_text(&format!("Organization writes disabled: {error}. Repair organization.json; original retained."));
                crate::ui::feedback(&self.status, true);
            }
        }
        match config {
            Ok(config) => {
                self.endpoint.set_text(&config.endpoint);
                self.password.set_text(&config.password);
            }
            Err(error) => {
                self.obs_valid.set(false);
                self.obs_save.set_sensitive(false);
                self.obs_test.set_sensitive(false);
                self.obs_status.set_text(&format!(
                    "OBS settings unavailable: {error}. Repair obs.json; original retained."
                ));
                crate::ui::feedback(&self.obs_status, true);
            }
        }
        self.reconcile_focus();
        self.refresh_indicator();
        self.refresh_application_editor();
    }

    pub(crate) fn organization_available(&self) -> bool {
        self.organization.borrow().is_some()
    }

    fn submit(&self, command: LayerCommand) -> bool {
        match self.output.send(command) {
            Ok(()) => true,
            Err(error) => {
                self.report(&format!("Action worker unavailable: {error}"), true);
                false
            }
        }
    }

    /// Tell the reader which sources any binding currently shows.
    pub(crate) fn sync_info_from(&self, store: &LayerStore) {
        let sources = store
            .info_sources()
            .into_iter()
            .filter(|source| source.is_polled())
            .collect();
        if let Some(sender) = self.info_watch.borrow().as_ref() {
            if sender.send(sources).is_err() {
                eprintln!("The information reader stopped.");
            }
        }
    }

    fn track_brightness(&self, action: &DeviceAction) {
        match action {
            DeviceAction::Brightness(value) => self.brightness.set(*value),
            DeviceAction::AdjustBrightness(step) => self
                .brightness
                .set(self.brightness.get().saturating_add_signed(*step).min(100)),
            DeviceAction::Sleep => self.sleeping.set(true),
            DeviceAction::Wake => self.sleeping.set(false),
        }
        let level = self.brightness.get();
        let value = if self.sleeping.get() {
            InfoValue {
                inactive: true,
                ..InfoValue::gauge(Glyph::Sun, 0, "OFF".into())
            }
        } else {
            InfoValue::gauge(Glyph::Sun, level, format!("{level}%"))
        };
        self.artwork.update_info(InfoSource::Brightness, value);
    }

    pub(crate) fn persist(&self, store: LayerStore) {
        if self.configuration_operation.get() == Some(configuration::Operation::Import) {
            return;
        }
        self.sync_info_from(&store);
        if self.key_swap_pending.get() {
            self.key_swap_deferred_persist.set(true);
            return;
        }
        self.persist_with_feedback(store, LayerCommand::Persist);
    }

    pub(crate) fn save_binding(&self, store: LayerStore) -> bool {
        self.sync_info_from(&store);
        self.manual_profile.set(true);
        self.persist_with_feedback(store, LayerCommand::SaveBinding)
    }

    pub(crate) fn save_profile(&self, store: LayerStore) {
        self.sync_info_from(&store);
        self.manual_profile.set(true);
        if self.persist_with_feedback(store, LayerCommand::SaveProfile) {
            self.report_profile_save("Saving profile…", false);
        } else {
            self.report_profile_save("Could not save profile: action worker unavailable.", true);
        }
    }

    pub(crate) fn edit_profile(&self, profile: &str) {
        crate::select_string(
            &self.profile_settings_model,
            &self.profile_settings_picker,
            profile,
        );
    }

    pub(crate) fn report_profile_save(&self, message: &str, failed: bool) {
        self.profile_application_status.set_text(message);
        crate::ui::feedback(&self.profile_application_status, failed);
        self.report(message, failed);
    }

    fn persist_with_feedback(
        &self,
        store: LayerStore,
        command: fn(LayerStore, Option<Organization>) -> LayerCommand,
    ) -> bool {
        let metadata = {
            let mut organization = self.organization.borrow_mut();
            if let Some(organization) = organization.as_mut() {
                organization.sync(&store);
            }
            organization.clone()
        };
        let submitted = self.submit(command(store, metadata));
        self.refresh_indicator();
        submitted
    }

    pub(crate) fn refresh_indicator(&self) {
        match self.organization.borrow().as_ref() {
            Some(organization) => {
                let info = organization.current(&self.layers.borrow());
                self.page_controls.refresh(
                    organization,
                    &self.layers.borrow(),
                    self.artwork.store.borrow().is_some(),
                );
                self.indicator.set_text(&format!(
                    "{}  ·  Page {} of {}",
                    info.label, info.page, info.page_count
                ));
                self.indicator.set_tooltip_text(Some(&info.label));
            }
            None => {
                self.indicator
                    .set_text("Organization unavailable · layer bindings remain editable");
            }
        }
        self.refresh_profiles();
    }

    fn refresh_selection(&self) {
        let names = self.layers.borrow().names();
        self.updating.set(true);
        crate::replace_string_list(&self.ui.layer_model, &names);
        self.ui.refresh_layer_targets(&names);
        let selected = crate::layer_index(&self.layers, self.layers.borrow().active());
        self.ui.layer_picker.set_selected(selected);
        self.updating.set(false);
        self.ui.load_selected_binding(&self.layers);
        self.ui.refresh_summaries(&self.layers);
        self.artwork.refresh(false);
        self.refresh_indicator();
    }

    fn navigate(&self, action: &NavigationAction) {
        let result = {
            let mut organization = self.organization.borrow_mut();
            let Some(organization) = organization.as_mut() else {
                self.report("Organization navigation is unavailable.", true);
                return;
            };
            organization.navigate(&mut self.layers.borrow_mut(), action)
        };
        match result {
            Ok(true) => {
                self.refresh_selection();
                self.persist(self.layers.borrow().clone());
                self.status
                    .set_text(&format!("Selected {}", self.indicator.text()));
            }
            Ok(false) => self
                .status
                .set_text("No matching page or folder in this view."),
            Err(error) => self.report(&format!("Navigation failed: {error}"), true),
        }
    }

    pub(crate) fn execute(&self, binding_id: String, action: Action) {
        if self.configuration_operation.get() == Some(configuration::Operation::Import) {
            return;
        }
        match action {
            Action::None => {}
            Action::SwitchLayer(target) => {
                if self.layers.borrow_mut().apply_switch(&target) {
                    self.refresh_selection();
                    self.persist(self.layers.borrow().clone());
                } else {
                    self.report("Destination layer is unavailable.", true);
                }
            }
            Action::Emit(name) => {
                if let Some(output) = self.events.borrow().as_ref() {
                    if let Err(error) = output.send(MonitorEvent::AppEvent { name }) {
                        self.report(&format!("Event delivery failed: {error}"), true);
                    }
                } else {
                    self.report("Application event output is unavailable.", true);
                }
            }
            Action::Feature(feature) => match feature {
                FeatureAction::Navigation(action) => self.navigate(&action),
                FeatureAction::Device(action) => match self.device.borrow().as_ref() {
                    Some(output) => match output.send(action.clone()) {
                        Ok(()) => {
                            self.track_brightness(&action);
                            self.status.set_text("Device action queued.");
                        }
                        Err(error) => {
                            self.report(&format!("Device action unavailable: {error}"), true)
                        }
                    },
                    None => self.report("Device output is unavailable.", true),
                },
                FeatureAction::Timer { seconds } => self.start_timer(seconds),
                FeatureAction::CancelTimer => {
                    self.cancel_timer();
                    self.timer_status.set_text("Timer cancelled.");
                }
                FeatureAction::Desktop(action) => {
                    if self.submit(LayerCommand::Execute {
                        binding_id,
                        action: Action::Feature(FeatureAction::Desktop(action)),
                    }) {
                        self.status.set_text("Desktop action queued.");
                    }
                }
                FeatureAction::Obs(action) => {
                    if !self.obs_valid.get() {
                        self.report("OBS settings are unavailable.", true);
                        return;
                    }
                    if self.submit(LayerCommand::Execute {
                        binding_id,
                        action: Action::Feature(FeatureAction::Obs(action)),
                    }) {
                        self.status.set_text("OBS action queued.");
                    }
                }
            },
            Action::Launch { .. }
            | Action::Command(_)
            | Action::KeyPress { .. }
            | Action::Shortcut { .. } => {
                if self.submit(LayerCommand::Execute { binding_id, action }) {
                    self.status.set_text("Action queued.");
                }
            }
        }
    }

    /// Run `action`, then move once to the configured page after its delay.
    /// The destination is the exact page chosen in the editor.
    pub(crate) fn execute_with_return(
        &self,
        binding_id: String,
        action: Action,
        page_return: Option<PageReturn>,
    ) {
        if action == Action::None
            || self.configuration_operation.get() == Some(configuration::Operation::Import)
        {
            return;
        }
        self.execute(binding_id, action);
        if let Some(value) = page_return {
            if self.layers.borrow().layers().contains_key(&value.layer) {
                self.arm_page_return(value);
            } else {
                self.report("The return page no longer exists.", true);
            }
        }
    }

    fn arm_page_return(&self, value: PageReturn) {
        if let Some(source) = self.page_return.borrow_mut().take() {
            source.remove();
        }
        let runtime = self.clone();
        *self.page_return.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_secs(u64::from(value.seconds)),
            move || {
                runtime.page_return.borrow_mut().take();
                if runtime.layers.borrow_mut().select(&value.layer) {
                    runtime.manual_profile.set(true);
                    runtime.refresh_selection();
                    runtime.persist(runtime.layers.borrow().clone());
                    runtime.report(&format!("Returned to {}.", value.layer), false);
                } else {
                    runtime.report("Return page is no longer available.", true);
                }
            },
        ));
    }

    fn start_timer(&self, seconds: u32) {
        self.cancel_countdown();
        let deadline = Instant::now() + Duration::from_secs(u64::from(seconds));
        self.timer_status
            .set_text(&format!("Timer: {seconds}s remaining"));
        self.timer_status.set_visible(true);
        let status = self.timer_status.clone();
        let source = self.timer.clone();
        *self.timer.borrow_mut() = Some(glib::timeout_add_local(
            Duration::from_millis(250),
            move || {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    // The returning Break removes the source; do not remove it twice.
                    source.borrow_mut().take();
                    status.set_text("Timer complete.");
                    crate::ui::feedback(&status, false);
                    // Native GTK bell honors the user's existing bell setting.
                    status.error_bell();
                    glib::ControlFlow::Break
                } else {
                    status.set_text(&format!(
                        "Timer: {}s remaining",
                        remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0)
                    ));
                    glib::ControlFlow::Continue
                }
            },
        ));
    }

    fn cancel_countdown(&self) {
        if let Some(source) = self.timer.borrow_mut().take() {
            source.remove();
        }
    }

    /// Cancels the countdown and any pending page return.
    pub(crate) fn cancel_timer(&self) {
        self.cancel_countdown();
        if let Some(source) = self.page_return.borrow_mut().take() {
            source.remove();
        }
    }

    #[cfg(test)]
    pub(crate) fn page_return_active(&self) -> bool {
        self.page_return.borrow().is_some()
    }

    #[cfg(test)]
    pub(crate) fn timer_active(&self) -> bool {
        self.timer.borrow().is_some()
    }

    fn submit_obs(&self, probe: bool) {
        if !self.obs_valid.get() {
            return;
        }
        let config = ObsConfig {
            endpoint: self.endpoint.text().to_string(),
            password: self.password.text().to_string(),
        };
        if let Err(error) = config.validate() {
            self.obs_status.set_text(&error.to_string());
            crate::ui::feedback(&self.obs_status, true);
            return;
        }
        let command = if probe {
            LayerCommand::ObsProbe(config)
        } else {
            LayerCommand::ObsSave(config)
        };
        if self.submit(command) {
            self.obs_status.set_text(if probe {
                "Testing connection…"
            } else {
                "Saving settings…"
            });
        }
    }

    pub(crate) fn report(&self, message: &str, failed: bool) {
        self.status.set_text(message);
        crate::ui::feedback(&self.status, failed);
        if message.starts_with("OBS") {
            self.obs_status.set_text(message);
            crate::ui::feedback(&self.obs_status, failed);
        }
    }
}
