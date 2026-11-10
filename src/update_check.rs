//! Release checks own their worker; GTK receives only values, never worker widgets.
use gtk::{gio, glib, prelude::*};
use serde::Deserialize;
use std::{
    cell::{Cell, RefCell},
    io::{self, Read},
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const LIMIT: usize = 262144;
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(String),
    Unavailable,
}

pub(crate) fn parse_tag(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.strip_prefix('v')?.split('.');
    let mut number = || {
        let s = parts.next()?;
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let version = (number()?, number()?, number()?);
    if parts.next().is_some() {
        None
    } else {
        Some(version)
    }
}

pub(crate) fn check(
    runner: impl FnOnce(&str, &[&str]) -> io::Result<Vec<u8>>,
    current: &str,
) -> UpdateState {
    let agent = format!("User-Agent: sdeck-plus/{current}");
    let args = [
        "-fsS",
        "--proto",
        "=https",
        "--max-time",
        "8",
        "--max-filesize",
        "262144",
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        &agent,
        "https://api.github.com/repos/aldosilvestre/sdeck-plus/releases/latest",
    ];
    let Ok(bytes) = runner("curl", &args) else {
        return UpdateState::Unavailable;
    };
    if bytes.len() > LIMIT {
        return UpdateState::Unavailable;
    }
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        draft: bool,
        prerelease: bool,
    }
    let Ok(release) = serde_json::from_slice::<Release>(&bytes) else {
        return UpdateState::Unavailable;
    };
    if release.draft || release.prerelease {
        return UpdateState::Unavailable;
    }
    let (Some(latest), Some(current)) = (
        parse_tag(&release.tag_name),
        parse_tag(&format!("v{current}")),
    ) else {
        return UpdateState::Unavailable;
    };
    if latest > current {
        UpdateState::Available(release.tag_name)
    } else {
        UpdateState::UpToDate
    }
}

fn curl(program: &str, args: &[&str]) -> io::Result<Vec<u8>> {
    // Do not load curlrc: it can enable redirects or override the bounded request.
    let mut child = Command::new(program)
        .arg("-q")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing stdout"))?
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > LIMIT {
            return Err(io::Error::other("release response too large"));
        }
        if !child.wait()?.success() {
            return Err(io::Error::other("curl failed"));
        }
        Ok(bytes)
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[derive(Default)]
pub(crate) struct Gate {
    in_flight: bool,
    completed: Option<Duration>,
}
impl Gate {
    pub(crate) fn begin(&mut self, enabled: bool, manual: bool, now: Duration) -> bool {
        if self.in_flight
            || (!manual
                && (!enabled
                    || self
                        .completed
                        .is_some_and(|last| now.saturating_sub(last) < INTERVAL)))
        {
            return false;
        }
        self.in_flight = true;
        true
    }
    pub(crate) fn complete(&mut self, now: Duration) {
        self.in_flight = false;
        self.completed = Some(now);
    }
}

#[derive(Clone)]
pub(crate) struct Panel {
    pub(crate) body: gtk::Box,
    pub(crate) status: gtk::Label,
    pub(crate) manual: gtk::Button,
    pub(crate) automatic: gtk::CheckButton,
    pub(crate) indicator: gtk::Button,
    error: gtk::Label,
    pub(crate) update: gtk::Button,
    pub(crate) restart: gtk::Button,
    pub(crate) guidance: gtk::Label,
}
impl Panel {
    pub(crate) fn new() -> Self {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let heading = gtk::Label::new(Some("Updates"));
        heading.set_xalign(0.0);
        heading.add_css_class("section-label");
        let status = crate::ui::helper("Not checked yet");
        let manual = gtk::Button::with_label("Check for updates");
        let automatic = gtk::CheckButton::with_label("Check for updates automatically");
        let error = crate::ui::helper("");
        error.add_css_class("error");
        error.set_visible(false);
        body.append(&heading);
        body.append(&status);
        body.append(&manual);
        body.append(&automatic);
        body.append(&error);
        let update = gtk::Button::with_label("Update");
        update.set_tooltip_text(Some("Download and run the release installer from GitHub"));
        update.set_visible(false);
        let restart = gtk::Button::with_label("Restart now");
        restart.set_visible(false);
        let guidance = crate::ui::helper("");
        guidance.set_visible(false);
        body.append(&guidance);
        body.append(&update);
        body.append(&restart);
        let indicator = gtk::Button::new();
        indicator.add_css_class("update-indicator");
        indicator.set_visible(false);
        Self {
            body,
            status,
            manual,
            automatic,
            indicator,
            error,
            update,
            restart,
            guidance,
        }
    }
    pub(crate) fn render(&self, state: &UpdateState) {
        let text = match state {
            UpdateState::Idle => "Not checked yet".into(),
            UpdateState::Checking => "Checking...".into(),
            UpdateState::UpToDate => format!("Up to date (v{})", env!("CARGO_PKG_VERSION")),
            UpdateState::Available(tag) => format!("Update available: {tag}"),
            UpdateState::Unavailable => "Could not check for updates".into(),
        };
        self.status.set_text(&text);
        self.manual.set_sensitive(*state != UpdateState::Checking);
        // A known release remains visible even during a retry or a failed check.
        if let UpdateState::Available(_) = state {
            self.indicator.set_label(&text);
            self.indicator.set_tooltip_text(Some("Open Help > Updates"));
            self.indicator.set_visible(true);
        } else if *state == UpdateState::UpToDate {
            self.indicator.set_visible(false);
        }
    }
    pub(crate) fn render_install(
        &self,
        check: &UpdateState,
        state: &crate::update_install::InstallState,
        origin: crate::update_install::Origin,
    ) {
        use crate::update_install::{InstallState, Origin};
        self.render(check);
        let running = *state == InstallState::Running;
        self.update.set_visible(
            origin == Origin::Installer
                && (matches!(check, UpdateState::Available(_)) || running)
                && !matches!(state, InstallState::Succeeded(_)),
        );
        self.update
            .set_sensitive(crate::update_install::can_begin(check, state, origin));
        self.restart
            .set_visible(matches!(state, InstallState::Succeeded(_)));
        self.manual
            .set_sensitive(!running && *check != UpdateState::Checking);
        self.automatic.set_sensitive(!running);
        let guidance = match origin {
            Origin::Installer => "Updates download and run the release installer from GitHub. Restart is your choice after installation.",
            Origin::Package => "Installed by your package manager. Update it with your package manager.",
            Origin::Other => "This copy was not installed by the sdeck-plus installer. Re-run the install command from the documentation to update.",
        };
        self.guidance.set_text(guidance);
        self.guidance.set_visible(true);
        match state {
            InstallState::Running => self.status.set_text("Updating..."),
            InstallState::Succeeded(tag) => self
                .status
                .set_text(&format!("Updated to {tag}. Restart to use it.")),
            InstallState::Failed(error) => self.status.set_text(error),
            InstallState::Idle => {}
        }
    }
    fn preference_error(&self, error: &str) {
        self.error.set_text(&format!("Could not save or read update preference: {error}. Original preference preserved; setting unchanged. Repair the preference and restart to enable writes."));
        self.error.set_visible(true);
    }
}

pub(crate) struct Worker {
    pub(crate) sender: mpsc::Sender<()>,
    handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub(crate) fn start(
        runner: impl Fn(&str, &[&str]) -> io::Result<Vec<u8>> + Send + 'static,
    ) -> io::Result<(Self, async_channel::Receiver<UpdateState>)> {
        let (sender, receiver) = mpsc::channel();
        let (results, received) = async_channel::bounded(1);
        let handle = thread::Builder::new()
            .name("release-check".into())
            .spawn(move || {
                while receiver.recv().is_ok() {
                    if results
                        .send_blocking(check(&runner, env!("CARGO_PKG_VERSION")))
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok((
            Self {
                sender,
                handle: Some(handle),
            },
            received,
        ))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Disconnect before joining; an active curl has an eight-second deadline.
        let (replacement, _) = mpsc::channel();
        self.sender = replacement;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub(crate) struct Controller {
    panel: Panel,
    state: RefCell<UpdateState>,
    gate: RefCell<Gate>,
    enabled: Cell<bool>,
    path: Option<PathBuf>,
    writable: Cell<bool>,
    epoch: Instant,
    worker: RefCell<Option<Worker>>,
    installer: RefCell<Option<crate::update_install::Worker>>,
    installation: RefCell<crate::update_install::InstallState>,
    origin: crate::update_install::Origin,
}
impl Controller {
    pub(crate) fn new(
        panel: Panel,
        preference: (Option<PathBuf>, bool, bool),
        worker: Option<Worker>,
        origin: crate::update_install::Origin,
    ) -> Rc<Self> {
        let (path, enabled, writable) = preference;
        Rc::new(Self {
            panel,
            state: RefCell::new(UpdateState::Idle),
            gate: RefCell::new(Gate::default()),
            enabled: Cell::new(enabled),
            path,
            writable: Cell::new(writable),
            epoch: Instant::now(),
            worker: RefCell::new(worker),
            installer: RefCell::new(None),
            installation: RefCell::new(crate::update_install::InstallState::Idle),
            origin,
        })
    }
    /// Current release-check state; installation has its own single-flight state.
    pub(crate) fn state(&self) -> UpdateState {
        self.state.borrow().clone()
    }
    pub(crate) fn request(&self, manual: bool) {
        if *self.installation.borrow() == crate::update_install::InstallState::Running
            || self.worker.borrow().is_none()
            || !self
                .gate
                .borrow_mut()
                .begin(self.enabled.get(), manual, self.epoch.elapsed())
        {
            return;
        }
        self.state.replace(UpdateState::Checking);
        self.render();
        let sent = self
            .worker
            .borrow()
            .as_ref()
            .is_some_and(|w| w.sender.send(()).is_ok());
        if !sent {
            self.complete(UpdateState::Unavailable);
        }
    }
    pub(crate) fn complete(&self, result: UpdateState) {
        self.gate.borrow_mut().complete(self.epoch.elapsed());
        self.state.replace(result);
        self.render();
    }
    pub(crate) fn shutdown(&self) {
        self.installer.borrow_mut().take();
        self.worker.borrow_mut().take();
    }
    fn render(&self) {
        self.panel
            .render_install(&self.state(), &self.installation.borrow(), self.origin);
        if !self.writable.get() {
            self.panel.automatic.set_sensitive(false);
        }
    }
    fn begin_install(self: &Rc<Self>) {
        self.begin_install_with(crate::update_install::Worker::start);
    }
    pub(crate) fn begin_install_with(
        self: &Rc<Self>,
        start: impl FnOnce(
            String,
        ) -> io::Result<(
            crate::update_install::Worker,
            async_channel::Receiver<Result<(), String>>,
        )>,
    ) {
        if !crate::update_install::can_begin(
            &self.state(),
            &self.installation.borrow(),
            self.origin,
        ) {
            return;
        }
        let UpdateState::Available(tag) = self.state() else {
            return;
        };
        self.installation
            .replace(crate::update_install::InstallState::Running);
        self.render();
        match start(tag.clone()) {
            Ok((worker, received)) => {
                self.installer.replace(Some(worker));
                let weak = Rc::downgrade(self);
                glib::MainContext::default().spawn_local(async move {
                    if let Ok(result) = received.recv().await {
                        if let Some(c) = weak.upgrade() {
                            // Shutdown takes the worker first; never touch destroyed widgets.
                            if c.installer.borrow().is_none() {
                                return;
                            }
                            c.installer.borrow_mut().take();
                            c.installation.replace(match result {
                                Ok(()) => crate::update_install::InstallState::Succeeded(tag),
                                Err(error) => crate::update_install::InstallState::Failed(error),
                            });
                            c.render();
                        }
                    }
                });
            }
            Err(error) => {
                self.installation
                    .replace(crate::update_install::InstallState::Failed(format!(
                        "Update failed: {error}"
                    )));
                self.render();
            }
        }
    }
}

pub(crate) fn install(
    app: &gtk::Application,
    menu: &crate::configuration_menu::ConfigurationMenu,
    panel: Panel,
) -> Rc<Controller> {
    let preference = crate::update_preference::path()
        .and_then(|path| crate::update_preference::load(&path).map(|enabled| (path, enabled)));
    let (path, enabled, writable) = match preference {
        Ok((path, enabled)) => (Some(path), enabled, true),
        Err(error) => {
            panel.preference_error(&error.to_string());
            (None, false, false)
        }
    };
    panel.automatic.set_active(enabled);
    panel.automatic.set_sensitive(writable);
    let (worker, received) = match Worker::start(curl) {
        Ok((worker, received)) => (Some(worker), received),
        Err(_) => {
            panel.render(&UpdateState::Unavailable);
            let (_, received) = async_channel::bounded(1);
            (None, received)
        }
    };
    let controller = Controller::new(
        panel,
        (path, enabled, writable),
        worker,
        crate::update_install::current_origin(),
    );
    controller.render();
    controller.panel.update.connect_clicked(glib::clone!(
        #[weak]
        controller,
        move |_| controller.begin_install()
    ));
    controller.panel.restart.connect_clicked(glib::clone!(
        #[weak]
        controller,
        #[weak]
        app,
        move |_| {
            let flag = Cell::new(false);
            if crate::update_install::request_restart(&controller.installation.borrow(), &flag) {
                if let Some(action) = app
                    .lookup_action("restart-after-update")
                    .and_then(|a| a.downcast::<gio::SimpleAction>().ok())
                {
                    action.set_enabled(true);
                    action.activate(None);
                }
            }
        }
    ));
    controller.panel.manual.connect_clicked(glib::clone!(
        #[weak]
        controller,
        move |_| controller.request(true)
    ));
    menu.bind_help_button(&controller.panel.indicator);
    let weak = Rc::downgrade(&controller);
    controller.panel.automatic.connect_toggled(move |button| {
        let Some(c) = weak.upgrade() else {
            return;
        };
        if button.is_active() == c.enabled.get() {
            return;
        }
        let result = c
            .path
            .as_ref()
            .filter(|_| c.writable.get())
            .ok_or_else(|| io::Error::other("preference writes disabled"))
            .and_then(|p| crate::update_preference::apply(p, &c.enabled, button.is_active()));
        if let Err(error) = result {
            c.panel.preference_error(&error.to_string());
            c.writable.set(false);
            button.set_active(c.enabled.get());
            button.set_sensitive(false);
        }
    });
    let action = gio::SimpleAction::new("check-updates-on-open", None);
    let weak = Rc::downgrade(&controller);
    action.connect_activate(move |_, _| {
        if let Some(c) = weak.upgrade() {
            c.request(false);
        }
    });
    app.add_action(&action);
    let weak = Rc::downgrade(&controller);
    let task = glib::MainContext::default().spawn_local(async move {
        while let Ok(result) = received.recv().await {
            let Some(c) = weak.upgrade() else {
                break;
            };
            c.complete(result);
        }
    });
    controller.request(false);
    let shutdown = controller.clone();
    app.connect_shutdown(move |_| {
        task.abort();
        shutdown.shutdown();
    });
    controller
}
