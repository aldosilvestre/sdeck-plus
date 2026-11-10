//! The session registry: one complete session per device model, created the
//! first time the model is needed and kept alive afterwards.
//!
//! A session owns its widget tree (built once), runtime, stores, layer worker,
//! information reader and its own event channel, so devices of different
//! models run live at the same time and nothing crosses between them. Two
//! devices of one model share that model's session (they mirror each other).
//! The window shows one session at a time, as the visible child of a stack;
//! the tray, update controller, focus observer and HID hub are shared.
use super::*;
use crate::device_preference;
use sdeck_plus::{
    device_model::DeviceModel,
    device_roots,
    event_api::{ConnectionState, DeviceId},
    layers::DEFAULT_LAYER,
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

impl FeatureRuntime {
    #[cfg(test)]
    pub(crate) fn preview(&self) -> Option<Rc<crate::preview::PreviewView>> {
        self.preview.borrow().upgrade()
    }

    pub(crate) fn attach_preview(&self, preview: &Rc<crate::preview::PreviewView>) {
        *self.preview.borrow_mut() = Rc::downgrade(preview);
    }

    /// The model whose stores this session holds.
    #[cfg(test)]
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

    /// Resend the session's complete picture to its devices.
    pub(crate) fn resend_artwork(&self) {
        self.artwork.refresh(true);
    }

    /// Stop the information reader; it ends once its sender is gone.
    pub(crate) fn stop_info_worker(&self) {
        self.info_watch.borrow_mut().take();
    }
}

/// One model's complete session: its editor (the stack child the window
/// shows), runtime and the channel its own workers report through.
pub(crate) struct Session {
    pub(crate) model: &'static DeviceModel,
    pub(crate) runtime: FeatureRuntime,
    /// What the window's stack shows.
    pub(crate) root: GtkBox,
    /// Keeps the editor's weakly referenced callbacks alive.
    _body: crate::Body,
    surface: crate::EventSurface,
    events: mpsc::Receiver<MonitorEvent>,
    /// Devices of this model that are connected now.
    live: Cell<usize>,
}

impl Session {
    pub(crate) fn new(
        model: &'static DeviceModel,
        body: crate::Body,
        surface: crate::EventSurface,
        events: mpsc::Receiver<MonitorEvent>,
    ) -> Self {
        Self {
            model,
            runtime: body.feature_runtime.clone(),
            root: body.root.clone(),
            _body: body,
            surface,
            events,
            live: Cell::new(0),
        }
    }
}

/// A session just built, with what could not be opened along the way.
pub(crate) struct Created {
    pub(crate) session: Session,
    pub(crate) artwork_error: Option<String>,
}

/// Builds the session of a model from its opened stores. The last argument is
/// whether its first picture is sent right away; a model that is not shown
/// yet has no device to send it to.
pub(crate) type SessionFactory =
    Box<dyn Fn(&'static DeviceModel, OpenedSession, bool) -> Result<Created, String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SwitchReason {
    /// The connected hardware decides which model is shown.
    Device,
    /// The user picked a model with 'Preview as'.
    Preview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Unchanged,
    Switched,
    /// Awaiting confirmation, or failed to open.
    Blocked,
}

/// The window's connection labels, 'Preview as' and menu: shared by every
/// session, bound to the shown one.
#[derive(Clone)]
pub(crate) struct SharedHeader {
    pub(crate) status: Label,
    pub(crate) detail: Label,
    pub(crate) picker: crate::PreviewPicker,
    pub(crate) menu: crate::configuration_menu::ConfigurationMenu,
}

/// The registry: which sessions exist, which one the window shows and which
/// devices are connected to them.
pub(crate) struct ModelSession {
    roots: SessionRoots,
    header: SharedHeader,
    stack: gtk::Stack,
    window: glib::WeakRef<gtk::Window>,
    toast: crate::ui::Toast,
    factory: SessionFactory,
    sessions: RefCell<Vec<Rc<Session>>>,
    shown: Cell<&'static DeviceModel>,
    /// Connected devices in the order they connected.
    devices: RefCell<Vec<(DeviceId, &'static DeviceModel)>>,
    last_connected: Cell<Option<&'static DeviceModel>>,
    /// What the header says while no device is connected.
    last_state: RefCell<ConnectionState>,
    /// The model that could not be opened; the window stays read-only while it
    /// is the one the hardware wants and nothing else is running.
    failed: Cell<Option<&'static DeviceModel>>,
    last_focus: RefCell<Option<Result<Option<sdeck_plus::app_focus::FocusedApplication>, String>>>,
    syncing: Cell<bool>,
}

impl ModelSession {
    /// The registry over the first session, already built for `model`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        roots: SessionRoots,
        header: SharedHeader,
        stack: gtk::Stack,
        window: &gtk::Window,
        toast: crate::ui::Toast,
        factory: SessionFactory,
        first: Session,
    ) -> Rc<Self> {
        let shown = first.model;
        let first = Rc::new(first);
        stack.add_named(&first.root, Some(shown.id));
        stack.set_visible_child(&first.root);
        header.menu.bind(first.runtime.clone());
        let registry = Rc::new(Self {
            roots,
            header,
            stack,
            window: window.downgrade(),
            toast,
            factory,
            sessions: RefCell::new(vec![first]),
            shown: Cell::new(shown),
            devices: RefCell::new(Vec::new()),
            last_connected: Cell::new(None),
            last_state: RefCell::new(ConnectionState::Searching),
            failed: Cell::new(None),
            last_focus: RefCell::new(None),
            syncing: Cell::new(false),
        });
        let weak = Rc::downgrade(&registry);
        registry
            .header
            .picker
            .dropdown
            .connect_selected_notify(move |dropdown| {
                let Some(registry) = weak.upgrade() else {
                    return;
                };
                if registry.syncing.get() {
                    return;
                }
                let model = registry
                    .header
                    .picker
                    .models
                    .get(dropdown.selected() as usize)
                    .copied();
                match model {
                    Some(model) if registry.devices.borrow().is_empty() => {
                        registry.show(model, SwitchReason::Preview);
                    }
                    _ => registry.sync_picker(),
                }
            });
        registry.sync_picker();
        registry
    }

    /// The shown session's runtime.
    #[cfg(test)]
    pub(crate) fn runtime(&self) -> FeatureRuntime {
        self.shown_session().runtime.clone()
    }

    /// The model the window shows.
    #[cfg(test)]
    pub(crate) fn model(&self) -> &'static DeviceModel {
        self.shown.get()
    }

    #[cfg(test)]
    pub(crate) fn picker(&self) -> &crate::PreviewPicker {
        &self.header.picker
    }

    #[cfg(test)]
    pub(crate) fn menu(&self) -> &crate::configuration_menu::ConfigurationMenu {
        &self.header.menu
    }

    #[cfg(test)]
    pub(crate) fn toast(&self) -> &crate::ui::Toast {
        &self.toast
    }

    /// The runtime of `model`'s session, if it exists.
    #[cfg(test)]
    pub(crate) fn runtime_of(&self, model: &DeviceModel) -> Option<FeatureRuntime> {
        self.session(model).map(|session| session.runtime.clone())
    }

    #[cfg(test)]
    pub(crate) fn session_count(&self) -> usize {
        self.sessions.borrow().len()
    }

    #[cfg(test)]
    pub(crate) fn live_devices(&self, model: &DeviceModel) -> usize {
        self.session(model).map_or(0, |session| session.live.get())
    }

    #[cfg(test)]
    pub(crate) fn device_count(&self) -> usize {
        self.devices.borrow().len()
    }

    #[cfg(test)]
    pub(crate) fn last_connected(&self) -> Option<&'static DeviceModel> {
        self.last_connected.get()
    }

    /// Whether the shown session can be used: it cannot while the hardware
    /// wants a model that failed to open and nothing of the shown one runs.
    #[cfg(test)]
    pub(crate) fn accepts_input(&self) -> bool {
        !self.read_only()
    }

    fn read_only(&self) -> bool {
        self.failed.get().is_some() && self.shown_session().live.get() == 0
    }

    fn session(&self, model: &DeviceModel) -> Option<Rc<Session>> {
        self.sessions
            .borrow()
            .iter()
            .find(|session| session.model == model)
            .cloned()
    }

    fn shown_session(&self) -> Rc<Session> {
        self.session(self.shown.get())
            .expect("the shown model always has a session")
    }

    /// Drain what arrived and apply it: the hub's tagged events to the session
    /// of their model, then every session's own results.
    pub(crate) fn pump(self: &Rc<Self>, hub: &mpsc::Receiver<MonitorEvent>) {
        let arrived: Vec<_> = hub.try_iter().collect();
        for event in &arrived {
            if let MonitorEvent::ApplicationFocus(result) = event {
                self.focus_changed(result.clone());
            }
        }
        for event in arrived {
            self.route(event);
        }
        let sessions: Vec<_> = self.sessions.borrow().clone();
        for session in &sessions {
            session
                .surface
                .apply_all(session.events.try_iter().collect(), true);
            session.runtime.poll_key_holds();
        }
    }

    /// Every session follows the focused application, now or when it is built.
    pub(crate) fn focus_changed(
        &self,
        result: Result<Option<sdeck_plus::app_focus::FocusedApplication>, String>,
    ) {
        *self.last_focus.borrow_mut() = Some(result.clone());
        for session in self.sessions.borrow().iter() {
            session.runtime.focus_changed(result.clone());
        }
    }

    fn route(self: &Rc<Self>, event: MonitorEvent) {
        let by_device = |device| {
            self.devices
                .borrow()
                .iter()
                .find(|(known, _)| *known == device)
                .and_then(|(_, model)| self.session(model))
                .unwrap_or_else(|| self.shown_session())
        };
        match event {
            MonitorEvent::Connection(state) => self.connection_changed(&state),
            MonitorEvent::Heartbeat | MonitorEvent::ApplicationFocus(_) => {}
            MonitorEvent::Input { model, .. } => {
                if let Some(session) = self.session(model) {
                    session.surface.apply(event, true);
                }
            }
            MonitorEvent::ArtworkResult { device, .. } => {
                by_device(device).surface.apply(event, true);
            }
            MonitorEvent::DeviceResult { device, .. } => {
                let session = match device {
                    Some(device) => by_device(device),
                    // Addressed to a model with no device: the window shows it.
                    None => self.shown_session(),
                };
                session.surface.apply(event, true);
            }
            // Nothing else is tagged: workers report through their session's
            // own channel, so these come from outside and belong to the window.
            other => self.shown_session().surface.apply(other, true),
        }
    }

    /// Follow one connection report.
    pub(crate) fn connection_changed(self: &Rc<Self>, state: &ConnectionState) {
        match state {
            ConnectionState::Connected { device, model } => self.device_connected(*device, model),
            ConnectionState::Disconnected { device, .. } => self.device_gone(*device, state),
            ConnectionState::Searching | ConnectionState::Unavailable(_) => {
                // The hub only says this while it holds no device at all.
                let gone: Vec<_> = self.devices.borrow().iter().map(|(id, _)| *id).collect();
                for device in gone {
                    self.release(device);
                }
                *self.last_state.borrow_mut() = state.clone();
                self.refresh_header();
            }
        }
    }

    fn device_connected(self: &Rc<Self>, device: DeviceId, model: &'static DeviceModel) {
        // A device that reports again as another model went away unseen.
        let known = self
            .devices
            .borrow()
            .iter()
            .find(|(known, _)| *known == device)
            .map(|(_, model)| *model);
        if let Some(previous) = known {
            if previous != model {
                self.release(device);
            }
        }
        self.last_connected.set(Some(model));
        // The model may have been unreachable before: try its stores again.
        let session = match self.ensure_session(model) {
            Ok(session) => session,
            Err(error) => {
                self.devices
                    .borrow_mut()
                    .retain(|(known, _)| *known != device);
                self.open_failed(model, SwitchReason::Device, &error);
                self.refresh_header();
                return;
            }
        };
        if self.failed.get() == Some(model) {
            self.failed.set(None);
        }
        if known != Some(model) {
            self.devices.borrow_mut().push((device, model));
            session.live.set(session.live.get() + 1);
        }
        self.refresh_header();
        // The only device decides what the window shows; with several, the
        // window keeps what it shows until the user (or an unplug) changes it.
        if self.devices.borrow().len() == 1 && self.shown.get() != model {
            self.show(model, SwitchReason::Device);
        }
        session.runtime.resend_artwork();
        self.apply_read_only();
        self.sync_picker();
    }

    fn device_gone(self: &Rc<Self>, device: DeviceId, state: &ConnectionState) {
        self.release(device);
        if self.devices.borrow().is_empty() {
            *self.last_state.borrow_mut() = state.clone();
        }
        let shown = self.shown_session();
        let next = self.devices.borrow().first().map(|(_, model)| *model);
        if shown.live.get() == 0 {
            if let Some(next) = next {
                self.show(next, SwitchReason::Device);
            }
        }
        self.refresh_header();
        self.apply_read_only();
        self.sync_picker();
    }

    /// Forget a device; its model's session stops holding keys down when no
    /// device of the model remains.
    fn release(&self, device: DeviceId) {
        let model = {
            let mut devices = self.devices.borrow_mut();
            let Some(at) = devices.iter().position(|(known, _)| *known == device) else {
                return;
            };
            devices.remove(at).1
        };
        if let Some(session) = self.session(model) {
            session.live.set(session.live.get().saturating_sub(1));
            if session.live.get() == 0 {
                session.runtime.cancel_key_holds();
            }
        }
    }

    /// The session of `model`, built on first use. Nothing is sent to a device
    /// while it is built: the model's device gets its picture once it connects.
    fn ensure_session(&self, model: &'static DeviceModel) -> Result<Rc<Session>, String> {
        if let Some(session) = self.session(model) {
            return Ok(session);
        }
        let opened = open_session(model, &self.roots).map_err(|error| error.to_string())?;
        let Created {
            session,
            artwork_error,
        } = (self.factory)(model, opened, false)?;
        let session = Rc::new(session);
        self.stack.add_named(&session.root, Some(model.id));
        if let Some(focus) = self.last_focus.borrow().clone() {
            session.runtime.focus_changed(focus);
        }
        self.sessions.borrow_mut().push(session.clone());
        if let Some(error) = artwork_error {
            session.runtime.report(&error, true);
            if let Some(window) = self.window.upgrade() {
                crate::show_artwork_error(&window, &error);
            }
        }
        Ok(session)
    }

    /// Show `model`'s session.
    pub(crate) fn show(
        self: &Rc<Self>,
        model: &'static DeviceModel,
        reason: SwitchReason,
    ) -> Outcome {
        if model == self.shown.get() {
            return Outcome::Unchanged;
        }
        if reason == SwitchReason::Preview && self.shown_session().runtime.editor_dirty() {
            self.confirm_preview(model);
            return Outcome::Blocked;
        }
        self.commit(model, reason)
    }

    fn confirm_preview(self: &Rc<Self>, model: &'static DeviceModel) {
        let Some(window) = self.shown_session().runtime.window() else {
            self.commit(model, SwitchReason::Preview);
            return;
        };
        let accepted = Rc::downgrade(self);
        let declined = Rc::downgrade(self);
        crate::confirm_discard_in(
            &window,
            "You edited this binding without saving it. Showing another Stream Deck model discards those edits.",
            Rc::new(move || {
                if let Some(registry) = accepted.upgrade() {
                    // A device that connected while the question was open decides.
                    if registry.devices.borrow().is_empty() {
                        // The draft belongs to the model being left, which keeps its session.
                        registry.shown_session().runtime.discard_draft();
                        registry.commit(model, SwitchReason::Preview);
                    }
                    registry.sync_picker();
                }
            }),
            Rc::new(move || {
                if let Some(registry) = declined.upgrade() {
                    registry.sync_picker();
                }
            }),
        );
    }

    fn commit(self: &Rc<Self>, model: &'static DeviceModel, reason: SwitchReason) -> Outcome {
        let session = match self.ensure_session(model) {
            Ok(session) => session,
            Err(error) => {
                self.open_failed(model, reason, &error);
                return Outcome::Blocked;
            }
        };
        self.stack.set_visible_child(&session.root);
        self.shown.set(model);
        self.header.menu.bind(session.runtime.clone());
        self.apply_read_only();
        // Remembered only once the session really opened.
        if let Err(error) = device_preference::save_to(&self.roots.preference, model) {
            session.runtime.report(
                &format!("Could not remember the selected model: {error}"),
                true,
            );
        }
        self.sync_picker();
        self.toast
            .show(&format!("Switched to {}.", model.name), false);
        Outcome::Switched
    }

    fn open_failed(&self, model: &'static DeviceModel, reason: SwitchReason, error: &str) {
        let message = format!(
            "Could not open the configuration for {}: {error}",
            model.name
        );
        if reason == SwitchReason::Device {
            // The hardware wants a model that cannot open: the window keeps
            // what it shows, read-only, and nothing reaches the device.
            self.failed.set(Some(model));
            self.apply_read_only();
            self.shown_session().runtime.report(&message, true);
        } else {
            self.toast.show(&message, true);
        }
        if let Some(window) = self.window.upgrade() {
            crate::show_artwork_error(&window, &message);
        }
        self.sync_picker();
    }

    /// Editing is off while the window can only show a model that failed.
    fn apply_read_only(&self) {
        let read_only = self.read_only();
        self.shown_session().runtime.set_editing_enabled(!read_only);
        for session in self.sessions.borrow().iter() {
            if session.model != self.shown.get() {
                session.runtime.set_editing_enabled(true);
            }
        }
    }

    /// The header says what is connected.
    fn refresh_header(&self) {
        let devices = self.devices.borrow();
        let header = &self.header;
        match devices.as_slice() {
            [] => {
                crate::show_connection_state(
                    &header.status,
                    &header.detail,
                    &self.last_state.borrow(),
                    self.last_connected.get(),
                );
            }
            [(device, model)] => {
                crate::show_connection_state(
                    &header.status,
                    &header.detail,
                    &ConnectionState::Connected {
                        device: *device,
                        model,
                    },
                    self.last_connected.get(),
                );
            }
            several => {
                let models: Vec<_> = several.iter().map(|(_, model)| *model).collect();
                crate::show_connected_devices(&header.status, &header.detail, &models);
            }
        }
    }

    /// Show the shown model in the picker; it is only usable without a device.
    pub(crate) fn sync_picker(&self) {
        let shown = self.shown.get();
        let picker = &self.header.picker;
        self.syncing.set(true);
        if let Some(index) = picker.models.iter().position(|model| *model == shown) {
            picker.dropdown.set_selected(index as u32);
        }
        self.syncing.set(false);
        let connected = self.devices.borrow().first().map(|(_, model)| *model);
        let tooltip = match connected {
            Some(model) => format!("Using the connected {}", model.name),
            None => "Show the configuration of another Stream Deck model while none is connected"
                .to_owned(),
        };
        picker.dropdown.set_sensitive(connected.is_none());
        picker.dropdown.set_tooltip_text(Some(&tooltip));
    }
}
