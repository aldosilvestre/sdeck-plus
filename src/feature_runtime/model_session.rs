//! In-process device-model sessions: open one model's stores away from the UI,
//! then swap them into the running window without restarting anything.
//!
//! The widgets, tray, update controller, layer worker, focus observer and HID
//! monitor outlive every switch. Rebuilding the widget tree would leave each
//! old one alive through its own signal closures, so a switch replaces the
//! data the widgets show instead, exactly as an imported configuration does.
use super::*;
use crate::device_preference;
use sdeck_plus::{
    device_model::DeviceModel, device_roots, event_api::ConnectionState, layers::DEFAULT_LAYER,
    local_artwork::ArtworkStore,
};

/// Where sessions keep their data and the last-model preference.
#[derive(Clone, Debug)]
pub(crate) struct SessionRoots {
    /// The Stream Deck + root; every other model lives beneath it.
    pub(crate) data_dir: PathBuf,
    /// The strict last-used-model file shared by every model.
    pub(crate) preference: PathBuf,
}

impl SessionRoots {
    /// The user's real data and configuration directories.
    pub(crate) fn system() -> std::io::Result<Self> {
        Ok(Self {
            data_dir: device_roots::data_dir()?,
            preference: device_preference::path()?,
        })
    }

    pub(crate) fn root_for(&self, model: &DeviceModel) -> PathBuf {
        device_roots::data_root_in(&self.data_dir, model)
    }
}

/// Why a model's stores could not be opened.
#[derive(Debug)]
pub(crate) enum OpenError {
    Layers(std::io::Error),
    Touchscreens(std::io::Error),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Layers(error) | Self::Touchscreens(error) => error.fmt(formatter),
        }
    }
}

/// One model's stores, validated and not yet shown.
pub(crate) struct OpenedSession {
    pub(crate) layers: LayerStore,
    pub(crate) artwork: std::io::Result<ArtworkStore>,
    pub(crate) organization: std::io::Result<Organization>,
    pub(crate) obs: std::io::Result<ObsConfig>,
}

/// Open every store of `model` under `roots` without touching the UI. A
/// damaged layer or touchscreen store is fatal; artwork, organization and OBS
/// settings degrade to disabled writes, as they always have.
pub(crate) fn open_session(
    model: &'static DeviceModel,
    roots: &SessionRoots,
) -> Result<OpenedSession, OpenError> {
    let root = roots.root_for(model);
    let mut layers = LayerStore::open_at_model(&root, model)
        .and_then(crate::checked_layer_store)
        .map_err(OpenError::Layers)?;
    // Every session starts on Base (its first page), whatever was saved last.
    layers.select(DEFAULT_LAYER);
    let mut artwork =
        ArtworkStore::open_at_for_layer_model(layers.root(), Some(layers.active()), layers.model());
    let organization = Organization::open_at(layers.root(), &layers);
    if let (Ok(organization), Ok(store)) = (&organization, artwork.as_mut()) {
        layers
            .initialize_touchscreens(organization, store)
            .map_err(OpenError::Touchscreens)?;
    }
    let obs = ObsConfig::load_at(&root);
    Ok(OpenedSession {
        layers,
        artwork,
        organization,
        obs,
    })
}

/// Windows over the main one (dialogs, pickers) belong to the old session.
fn close_secondary_windows(main: &gtk::Window) {
    for window in gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::Window>().ok())
    {
        if &window != main && window.transient_for().as_ref() == Some(main) {
            window.close();
        }
    }
}

impl FeatureRuntime {
    /// The model whose stores are shown now.
    pub(crate) fn session_model(&self) -> &'static DeviceModel {
        self.layers.borrow().model()
    }

    #[cfg(test)]
    pub(crate) fn layers(&self) -> &Rc<RefCell<LayerStore>> {
        &self.layers
    }

    #[cfg(test)]
    pub(crate) fn binding_ui(&self) -> &BindingUi {
        &self.ui
    }

    #[cfg(test)]
    pub(crate) fn artwork_ui(&self) -> &ArtworkUi {
        &self.artwork
    }

    #[cfg(test)]
    pub(crate) fn editing_enabled(&self) -> bool {
        self.configuration_root
            .borrow()
            .upgrade()
            .is_some_and(|root| root.is_sensitive())
    }

    #[cfg(test)]
    pub(crate) fn key_hold_generation(&self) -> u64 {
        self.key_generation.get()
    }

    pub(crate) fn editor_dirty(&self) -> bool {
        self.ui.is_dirty()
    }

    /// A switch must not race an import, export or key swap.
    pub(crate) fn can_switch_model(&self) -> bool {
        self.configuration_operation.get().is_none() && !self.key_swap_pending.get()
    }

    pub(crate) fn set_editing_enabled(&self, enabled: bool) {
        if let Some(root) = self.configuration_root.borrow().upgrade() {
            root.set_sensitive(enabled);
        }
    }

    pub(crate) fn window(&self) -> Option<gtk::Window> {
        self.configuration_root
            .borrow()
            .upgrade()
            .and_then(|root| root.root())
            .and_then(|root| root.downcast::<gtk::Window>().ok())
    }

    /// Stop the countdown and pending page return without redrawing: the next
    /// snapshot belongs to the new session.
    fn drop_timers(&self) {
        if let Some(source) = self.timer.borrow_mut().take() {
            source.remove();
        }
        if let Some(source) = self.page_return.borrow_mut().take() {
            source.remove();
        }
        self.artwork.info.borrow_mut().countdown = None;
    }

    /// Show `session` in place of the current one. Nothing is replaced when
    /// the layer worker cannot be told about the new root.
    pub(crate) fn install_session(
        &self,
        session: OpenedSession,
        send_artwork: bool,
    ) -> Result<(), String> {
        // Ordered after every earlier command: queued work still sees the old root.
        self.output
            .send(LayerCommand::SetRoot(session.layers.root().to_owned()))
            .map_err(|error| format!("the action worker is unavailable: {error}"))?;
        self.cancel_key_holds();
        self.drop_timers();
        // The reader ends once its only sender is gone.
        self.info_watch.borrow_mut().take();
        self.artwork.chooser_owners.borrow_mut().clear();
        if let Some(window) = self.window() {
            close_secondary_windows(&window);
        }
        self.ui.removed.borrow_mut().take();
        self.ui.undo_remove.set_visible(false);
        *self.layers.borrow_mut() = session.layers;
        match session.artwork {
            Ok(store) => *self.artwork.store.borrow_mut() = Some(store),
            Err(error) => {
                *self.artwork.store.borrow_mut() = None;
                self.report(&format!("Could not open or migrate artwork: {error}. Artwork writes and layer removal are disabled. Original files have not been replaced."), true);
            }
        }
        *self.artwork.last_layer.borrow_mut() = None;
        {
            let mut info = self.artwork.info.borrow_mut();
            info.clear();
            info.insert(InfoSource::Brightness, self.brightness_value());
        }
        self.profile_memory.borrow_mut().clear();
        self.manual_profile.set(false);
        self.apply_organization(session.organization, true);
        self.apply_obs(session.obs);
        self.start_info_worker();
        self.sync_info_from(&self.layers.borrow());
        self.refresh_selection_for(send_artwork);
        if let Some(editor) = self.artwork.text_editor.borrow().upgrade() {
            editor.load_context(true);
        }
        self.reconcile_focus();
        self.refresh_application_editor();
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SwitchReason {
    /// The connected hardware is another model: it decides.
    Device,
    /// The user picked a model with 'Preview as'.
    Preview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Unchanged,
    Switched,
    /// Deferred, awaiting confirmation, or failed to open.
    Blocked,
}

/// Which model the window shows, follows the hardware and serves 'Preview as'.
pub(crate) struct ModelSession {
    runtime: FeatureRuntime,
    roots: SessionRoots,
    picker: crate::PreviewPicker,
    organization_writes: Vec<Button>,
    toast: crate::ui::Toast,
    connected: Cell<Option<&'static DeviceModel>>,
    last_connected: Cell<Option<&'static DeviceModel>>,
    deferred: Cell<Option<(&'static DeviceModel, SwitchReason)>>,
    /// The model that could not be opened; editing stays off until one does.
    failed: Cell<Option<&'static DeviceModel>>,
    syncing: Cell<bool>,
}

impl ModelSession {
    pub(crate) fn new(
        runtime: FeatureRuntime,
        roots: SessionRoots,
        picker: crate::PreviewPicker,
        organization_writes: Vec<Button>,
        toast: crate::ui::Toast,
    ) -> Rc<Self> {
        let session = Rc::new(Self {
            runtime,
            roots,
            picker,
            organization_writes,
            toast,
            connected: Cell::new(None),
            last_connected: Cell::new(None),
            deferred: Cell::new(None),
            failed: Cell::new(None),
            syncing: Cell::new(false),
        });
        let weak = Rc::downgrade(&session);
        session
            .picker
            .dropdown
            .connect_selected_notify(move |dropdown| {
                let Some(session) = weak.upgrade() else {
                    return;
                };
                if session.syncing.get() {
                    return;
                }
                let model = session
                    .picker
                    .models
                    .get(dropdown.selected() as usize)
                    .copied();
                match model {
                    Some(model) if session.connected.get().is_none() => {
                        session.switch_model(model, SwitchReason::Preview);
                    }
                    _ => session.sync_picker(),
                }
            });
        session.sync_picker();
        session
    }

    #[cfg(test)]
    pub(crate) fn runtime(&self) -> &FeatureRuntime {
        &self.runtime
    }

    #[cfg(test)]
    pub(crate) fn picker(&self) -> &crate::PreviewPicker {
        &self.picker
    }

    #[cfg(test)]
    pub(crate) fn toast(&self) -> &crate::ui::Toast {
        &self.toast
    }

    pub(crate) fn last_connected(&self) -> Option<&'static DeviceModel> {
        self.last_connected.get()
    }

    /// Input only drives the session that was built for the model sending it.
    pub(crate) fn accepts_input(&self) -> bool {
        self.failed.get().is_none()
            && self
                .connected
                .get()
                .is_none_or(|model| model == self.runtime.session_model())
    }

    /// Follow the connection. Returns whether the current pictures still have
    /// to be sent to the device: a switch has already sent its own.
    pub(crate) fn connection_changed(self: &Rc<Self>, state: &ConnectionState) -> bool {
        let ConnectionState::Connected(model) = state else {
            self.connected.set(None);
            self.sync_picker();
            return false;
        };
        let model: &'static DeviceModel = model;
        self.connected.set(Some(model));
        self.last_connected.set(Some(model));
        let outcome = self.switch_model(model, SwitchReason::Device);
        self.sync_picker();
        outcome == Outcome::Unchanged
    }

    /// Run a switch that waited for an import, export or key swap.
    pub(crate) fn retry_deferred(self: &Rc<Self>) {
        if self.runtime.can_switch_model() {
            if let Some((model, reason)) = self.deferred.take() {
                // A device that appeared meanwhile has already decided.
                if reason == SwitchReason::Device || self.connected.get().is_none() {
                    self.switch_model(model, reason);
                    self.sync_picker();
                }
            }
        }
    }

    pub(crate) fn switch_model(
        self: &Rc<Self>,
        model: &'static DeviceModel,
        reason: SwitchReason,
    ) -> Outcome {
        let current = self.runtime.session_model();
        if model == current && self.failed.get().is_none() {
            return Outcome::Unchanged;
        }
        if !self.runtime.can_switch_model() {
            self.deferred.set(Some((model, reason)));
            self.runtime.report(
                "Wait for the current configuration operation to finish; the model changes afterwards.",
                true,
            );
            self.sync_picker();
            return Outcome::Blocked;
        }
        self.deferred.set(None);
        if model == current {
            // Nothing to open: the shown session was never replaced.
            self.failed.set(None);
            self.runtime.set_editing_enabled(true);
            self.sync_picker();
            return Outcome::Unchanged;
        }
        if reason == SwitchReason::Preview && self.runtime.editor_dirty() {
            self.confirm_preview(model);
            return Outcome::Blocked;
        }
        self.commit(model, reason)
    }

    fn confirm_preview(self: &Rc<Self>, model: &'static DeviceModel) {
        let Some(window) = self.runtime.window() else {
            self.commit(model, SwitchReason::Preview);
            return;
        };
        let accepted = Rc::downgrade(self);
        let declined = Rc::downgrade(self);
        crate::confirm_discard_in(
            &window,
            "You edited this binding without saving it. Showing another Stream Deck model discards those edits.",
            Rc::new(move || {
                if let Some(session) = accepted.upgrade() {
                    // A device that connected while the question was open decides.
                    if session.connected.get().is_none() {
                        session.commit(model, SwitchReason::Preview);
                    }
                    session.sync_picker();
                }
            }),
            Rc::new(move || {
                if let Some(session) = declined.upgrade() {
                    session.sync_picker();
                }
            }),
        );
    }

    fn commit(self: &Rc<Self>, model: &'static DeviceModel, reason: SwitchReason) -> Outcome {
        let discarded = self.runtime.editor_dirty();
        let opened = match open_session(model, &self.roots) {
            Ok(opened) => opened,
            Err(error) => {
                self.open_failed(model, reason, &error.to_string());
                return Outcome::Blocked;
            }
        };
        let send_artwork = self.connected.get() == Some(model);
        if let Err(error) = self.runtime.install_session(opened, send_artwork) {
            self.open_failed(model, reason, &error);
            return Outcome::Blocked;
        }
        self.failed.set(None);
        self.runtime.set_editing_enabled(true);
        for button in &self.organization_writes {
            button.set_sensitive(self.runtime.organization_available());
        }
        // Remembered only once the session really opened.
        if let Err(error) = device_preference::save_to(&self.roots.preference, model) {
            self.runtime.report(
                &format!("Could not remember the selected model: {error}"),
                true,
            );
        }
        self.sync_picker();
        let message = if reason == SwitchReason::Device && discarded {
            format!(
                "Switched to {}; unsaved changes were discarded.",
                model.name
            )
        } else {
            format!("Switched to {}.", model.name)
        };
        self.toast.show(&message, false);
        Outcome::Switched
    }

    fn open_failed(&self, model: &'static DeviceModel, reason: SwitchReason, error: &str) {
        let message = format!(
            "Could not open the configuration for {}: {error}",
            model.name
        );
        if reason == SwitchReason::Device {
            // The previous pictures belong to the other model: keep them on
            // screen, read-only, and send nothing to the device.
            self.failed.set(Some(model));
            self.runtime.set_editing_enabled(false);
            self.runtime.report(&message, true);
        } else {
            self.toast.show(&message, true);
        }
        if let Some(window) = self.runtime.window() {
            crate::show_artwork_error(&window, &message);
        }
        self.sync_picker();
    }

    /// Show the shown model in the picker; it is only usable without a device.
    pub(crate) fn sync_picker(&self) {
        let shown = self.runtime.session_model();
        self.syncing.set(true);
        if let Some(index) = self.picker.models.iter().position(|model| *model == shown) {
            self.picker.dropdown.set_selected(index as u32);
        }
        self.syncing.set(false);
        let tooltip = match self.connected.get() {
            Some(model) => format!("Using the connected {}", model.name),
            None => "Show the configuration of another Stream Deck model while none is connected"
                .to_owned(),
        };
        self.picker
            .dropdown
            .set_sensitive(self.connected.get().is_none());
        self.picker.dropdown.set_tooltip_text(Some(&tooltip));
    }
}
