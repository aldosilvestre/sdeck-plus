//! The "Add plugin" dialog: a folder path or an `https://` repository address is checked on a
//! worker thread (`sdeck_plus::plugins::{parse_source_in, stage}`), previewed, and only then
//! installed (disabled) by an explicit press of Install or Replace. Nothing here runs a plugin.
use crate::plugin_ui::{control_name, PluginHub};
use gtk::{glib, prelude::*, Box as GtkBox, Button, Entry, Label, Orientation};
use sdeck_plus::plugins::{
    install, parse_source_in, stage, CancelToken, InstallError, InstallMode, Installed,
    PluginPreview, PluginRegistry, StageError, StageOptions, StagedPlugin,
};
use std::{
    cell::{Cell, RefCell},
    fs,
    rc::Rc,
    sync::mpsc,
    thread::JoinHandle,
    time::Duration,
};

/// Receives a message for the status line and whether it reports a failure.
pub(crate) type Notify = Rc<dyn Fn(&str, bool)>;

const POLL: Duration = Duration::from_millis(40);
const MAX_LISTED_PROBLEMS: usize = 8;
const INTRO: &str = "Add a plugin from a folder on this computer or from an HTTPS repository address. Nothing runs until you enable it.";
const RUN_WARNING: &str = "Once enabled, this plugin runs with your user's privileges. Read its code before you enable it.";

/// Where the dialog stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Idle,
    Checking,
    /// A valid preview for exactly the text in the entry; Install is available.
    Preview,
    /// Like `Preview`, but the plugin is installed: the button reads Replace.
    Replace,
    Invalid,
    Installing,
}

/// A staged, validated plugin and the text it was checked for. Dropping it removes the staging
/// directory.
struct Candidate {
    text: String,
    staged: StagedPlugin,
    /// `Some(version)` when `<plugins>/<id>` exists (`None` inside when its version is unknown).
    installed: Option<Option<String>>,
}

enum CheckFailure {
    Cancelled,
    Messages(Vec<String>),
}

enum Report {
    Checked(Result<Candidate, CheckFailure>),
    Installed {
        candidate: Candidate,
        outcome: Result<Installed, InstallError>,
    },
}

struct Job {
    cancel: CancelToken,
    handle: JoinHandle<()>,
    receiver: mpsc::Receiver<Report>,
}

pub(crate) struct State {
    window: gtk::Window,
    entry: Entry,
    browse: Button,
    check: Button,
    cancel: Button,
    install: Button,
    spinner: gtk::Spinner,
    progress: Label,
    errors: GtkBox,
    card: GtkBox,
    pinned: GtkBox,
    hub: PluginHub,
    notify: Notify,
    phase: Cell<Phase>,
    job: RefCell<Option<Job>>,
    /// Set when the window closes: the poll then only finishes the worker off.
    closed: Cell<bool>,
    ready: RefCell<Option<Candidate>>,
    chooser: RefCell<Option<gtk::FileChooserNative>>,
    /// Handlers on the transient parent, removed when the dialog shuts down.
    parent_links: RefCell<Vec<(gtk::Window, glib::SignalHandlerId)>>,
}

/// One open dialog; clones share it.
#[derive(Clone)]
pub(crate) struct AddPluginDialog(Rc<State>);

/// Opens the dialog, or presents the one that is already open. `notify` receives the outcome
/// of an install (message, failed) even after the dialog closed.
pub(crate) fn open(
    parent: Option<&gtk::Window>,
    hub: &PluginHub,
    notify: Notify,
) -> AddPluginDialog {
    if let Some(state) = hub.add_dialog().filter(|state| state.window.is_visible()) {
        state.window.present();
        return AddPluginDialog(state);
    }
    let dialog = AddPluginDialog::build(parent, hub, notify);
    hub.set_add_dialog(&dialog.0);
    if let Some(parent) = parent {
        dialog.watch_parent(parent);
    }
    dialog.0.window.present();
    dialog.0.entry.grab_focus();
    dialog
}

/// Untrusted text for display: no control characters and no invisible or bidirectional
/// formatting characters that could disguise what is shown.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c,
                    '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{2064}'
                    | '\u{2066}'..='\u{2069}'
                    | '\u{FEFF}')
        })
        .collect()
}

/// Removes a staged copy without making the GTK thread wait for the deletion.
fn discard(candidate: Candidate) {
    // If the thread cannot start, the closure (and the candidate) is dropped right here.
    let _ = std::thread::Builder::new()
        .name("plugin-add-cleanup".into())
        .spawn(move || drop(candidate));
}

fn alert(text: &str) -> Label {
    let label = Label::builder()
        .label(clean(text))
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .max_width_chars(60)
        .accessible_role(gtk::AccessibleRole::Alert)
        .build();
    label.add_css_class("feedback-error");
    label
}

fn text_row(text: &str) -> Label {
    let label = Label::new(Some(&clean(text)));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_max_width_chars(60);
    label.set_selectable(false);
    label
}

impl AddPluginDialog {
    fn build(parent: Option<&gtk::Window>, hub: &PluginHub, notify: Notify) -> Self {
        let window = gtk::Window::builder()
            .title("Add plugin")
            .modal(true)
            .default_width(560)
            .default_height(720)
            .build();
        window.set_transient_for(parent);
        window.add_css_class("plugin-add-dialog");
        let entry = Entry::builder()
            .hexpand(true)
            .placeholder_text("Folder path or https:// repository address")
            .build();
        entry.update_property(&[gtk::accessible::Property::Label(
            "Plugin folder path or repository address",
        )]);
        let browse = crate::ui::button("folder-open-symbolic", "Browse…");
        let check = crate::ui::button("system-search-symbolic", "Check plugin");
        let cancel = crate::ui::button("process-stop-symbolic", "Cancel");
        let install = crate::ui::button("list-add-symbolic", "Install");
        install.add_css_class("suggested-action");
        let close = crate::ui::button("window-close-symbolic", "Close");
        let spinner = gtk::Spinner::new();
        let progress = Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .accessible_role(gtk::AccessibleRole::Status)
            .build();
        progress.add_css_class("helper");
        let errors = GtkBox::new(Orientation::Vertical, 4);
        let card = GtkBox::new(Orientation::Vertical, 0);
        let pinned = GtkBox::new(Orientation::Vertical, 4);

        let body = GtkBox::new(Orientation::Vertical, 12);
        body.set_margin_top(18);
        body.set_margin_bottom(18);
        body.set_margin_start(18);
        body.set_margin_end(18);
        body.append(&crate::ui::helper(INTRO));
        let source = GtkBox::new(Orientation::Horizontal, 8);
        source.append(&entry);
        source.append(&browse);
        body.append(&crate::labeled("Plugin source", &source));
        let actions = GtkBox::new(Orientation::Horizontal, 8);
        actions.append(&check);
        actions.append(&cancel);
        actions.append(&spinner);
        actions.append(&progress);
        body.append(&actions);
        body.append(&errors);
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(160)
            .child(&card)
            .build();
        body.append(&scroll);
        body.append(&pinned);
        let footer = GtkBox::new(Orientation::Horizontal, 8);
        let gap = GtkBox::new(Orientation::Horizontal, 0);
        gap.set_hexpand(true);
        footer.append(&gap);
        footer.append(&close);
        footer.append(&install);
        body.append(&footer);
        window.set_child(Some(&body));
        gtk::prelude::GtkWindowExt::set_focus(&window, Some(&entry));

        let state = Rc::new(State {
            window: window.clone(),
            entry,
            browse,
            check,
            cancel,
            install,
            spinner,
            progress,
            errors,
            card,
            pinned,
            hub: hub.clone(),
            notify,
            phase: Cell::new(Phase::Idle),
            job: RefCell::default(),
            closed: Cell::new(false),
            ready: RefCell::default(),
            chooser: RefCell::default(),
            parent_links: RefCell::default(),
        });
        let dialog = Self(state);
        dialog.set_phase(Phase::Idle);
        dialog.wire(&close);
        dialog
    }

    fn wire(&self, close: &Button) {
        let state = &self.0;
        let weak = Rc::downgrade(state);
        let on = |action: fn(&AddPluginDialog)| {
            let weak = weak.clone();
            move || {
                if let Some(state) = weak.upgrade() {
                    action(&AddPluginDialog(state));
                }
            }
        };
        let run = on(AddPluginDialog::start_check);
        state.check.connect_clicked({
            let run = run.clone();
            move |_| run()
        });
        state.entry.connect_activate(move |_| run());
        let cancel = on(AddPluginDialog::cancel_check);
        state.cancel.connect_clicked(move |_| cancel());
        let install = on(AddPluginDialog::start_install);
        state.install.connect_clicked(move |_| install());
        let browse = on(AddPluginDialog::browse);
        state.browse.connect_clicked(move |_| browse());
        let weak_changed = weak.clone();
        state.entry.connect_changed(move |_| {
            if let Some(state) = weak_changed.upgrade() {
                AddPluginDialog(state).entry_edited();
            }
        });
        let window = state.window.downgrade();
        close.connect_clicked(move |_| {
            if let Some(window) = window.upgrade() {
                window.close();
            }
        });
        let escape = gtk::EventControllerKey::new();
        escape.set_propagation_phase(gtk::PropagationPhase::Capture);
        let window = state.window.downgrade();
        escape.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(window) = window.upgrade() {
                    window.close();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        state.window.add_controller(escape);
        // The window's handlers keep the state alive while the window is open (every widget
        // handler above is weak). Closing or destroying the window hands the owner back, so
        // nothing is left in a reference cycle.
        let owner = Rc::new(RefCell::new(Some(self.clone())));
        let on_close = owner.clone();
        state.window.connect_close_request(move |_| {
            let taken = on_close.borrow_mut().take();
            if let Some(dialog) = taken {
                dialog.shutdown();
            }
            glib::Propagation::Proceed
        });
        state.window.connect_destroy(move |_| {
            let taken = owner.borrow_mut().take();
            if let Some(dialog) = taken {
                dialog.shutdown();
            }
        });
    }

    /// Closes this dialog when its transient parent closes or is destroyed.
    fn watch_parent(&self, parent: &gtk::Window) {
        let weak = Rc::downgrade(&self.0);
        let on_close = parent.connect_close_request({
            let weak = weak.clone();
            move |_| {
                let state = weak.upgrade();
                if let Some(state) = state {
                    // Closing the dialog runs its own (non-blocking) shutdown.
                    state.window.close();
                }
                glib::Propagation::Proceed
            }
        });
        // Hiding covers a parent that goes away without a close request.
        let on_destroy = parent.connect_hide(move |_| {
            if let Some(state) = weak.upgrade() {
                let dialog = AddPluginDialog(state);
                dialog.shutdown();
                if dialog.0.window.is_visible() {
                    dialog.0.window.destroy();
                }
            }
        });
        self.0
            .parent_links
            .borrow_mut()
            .push((parent.clone(), on_close));
        self.0
            .parent_links
            .borrow_mut()
            .push((parent.clone(), on_destroy));
    }

    fn phase(&self) -> Phase {
        self.0.phase.get()
    }

    fn busy(&self) -> bool {
        matches!(self.phase(), Phase::Checking | Phase::Installing)
    }

    fn set_phase(&self, phase: Phase) {
        let state = &self.0;
        let was_busy = self.busy();
        state.phase.set(phase);
        let editable = !self.busy();
        state.entry.set_sensitive(editable);
        state.browse.set_sensitive(editable);
        state.check.set_sensitive(editable);
        state.cancel.set_visible(phase == Phase::Checking);
        let working = !editable;
        state.spinner.set_visible(working);
        state.spinner.set_spinning(working);
        state.progress.set_visible(working);
        state.progress.set_text(match phase {
            Phase::Checking => "Checking the plugin…",
            Phase::Installing => "Installing the plugin…",
            _ => "",
        });
        state
            .install
            .set_sensitive(matches!(phase, Phase::Preview | Phase::Replace));
        let caption = if phase == Phase::Replace {
            "Replace"
        } else {
            "Install"
        };
        if state.install.tooltip_text().as_deref() != Some(caption) {
            let icon = if phase == Phase::Replace {
                "view-refresh-symbolic"
            } else {
                "list-add-symbolic"
            };
            crate::ui::decorate_button(&state.install, icon, caption);
        }
        if editable && was_busy {
            state.entry.grab_focus();
        }
    }

    fn clear_results(&self) {
        let state = &self.0;
        while let Some(child) = state.errors.first_child() {
            state.errors.remove(&child);
        }
        for area in [&state.card, &state.pinned] {
            while let Some(child) = area.first_child() {
                area.remove(&child);
            }
        }
        // Dropping the candidate removes its staging directory, off the GTK thread.
        let released = state.ready.borrow_mut().take();
        if let Some(candidate) = released {
            discard(candidate);
        }
    }

    fn clear_errors(&self) {
        while let Some(child) = self.0.errors.first_child() {
            self.0.errors.remove(&child);
        }
    }

    fn show_failure(&self, messages: &[String]) {
        for message in messages {
            self.0.errors.append(&alert(message));
        }
        self.set_phase(Phase::Invalid);
    }

    fn entry_edited(&self) {
        if self.busy() {
            return;
        }
        let stale = self
            .0
            .ready
            .borrow()
            .as_ref()
            .is_none_or(|ready| ready.text != self.0.entry.text().as_str());
        if stale {
            self.clear_results();
            self.set_phase(Phase::Idle);
        }
    }

    fn spawn(&self, work: impl FnOnce(&CancelToken, &mpsc::Sender<Report>) + Send + 'static) {
        #[cfg(test)]
        if self.0.hub.spawn_fails() {
            return self.spawn_failed("injected failure");
        }
        let cancel = CancelToken::new();
        let (sender, receiver) = mpsc::channel();
        let worker_cancel = cancel.clone();
        let handle = std::thread::Builder::new()
            .name("plugin-add".into())
            .spawn(move || work(&worker_cancel, &sender));
        match handle {
            Ok(handle) => {
                *self.0.job.borrow_mut() = Some(Job {
                    cancel,
                    handle,
                    receiver,
                });
                // Strong on purpose: the poll outlives a closed dialog until the worker reports,
                // then joins it and drops the state.
                let dialog = self.clone();
                glib::timeout_add_local(POLL, move || {
                    if dialog.pump() {
                        glib::ControlFlow::Continue
                    } else {
                        glib::ControlFlow::Break
                    }
                });
            }
            Err(error) => self.spawn_failed(&error.to_string()),
        }
    }

    fn spawn_failed(&self, error: &str) {
        let error = clean(error);
        self.show_failure(&[if self.phase() == Phase::Installing {
            format!("Could not start the installation: {error}. Nothing was installed.")
        } else {
            format!("Could not start the check: {error}.")
        }]);
    }

    fn start_check(&self) {
        if self.busy() {
            return;
        }
        self.clear_results();
        let text = self.0.entry.text().to_string();
        let (root, options) = (self.0.hub.root(), self.0.hub.stage_options());
        self.set_phase(Phase::Checking);
        #[cfg(test)]
        let panic_worker = self.0.hub.worker_panics();
        self.spawn(move |cancel, sender| {
            #[cfg(test)]
            if panic_worker {
                panic!("injected worker panic");
            }
            let _ = sender.send(Report::Checked(check(&text, root, &options, cancel)));
        });
    }

    fn cancel_check(&self) {
        if self.phase() != Phase::Checking {
            return;
        }
        if let Some(job) = self.0.job.borrow().as_ref() {
            job.cancel.cancel();
        }
    }

    fn start_install(&self) {
        if self.busy() {
            return;
        }
        let Some(guard) = self.0.hub.try_begin_install() else {
            // Replace, not append: repeated clicks must not stack identical lines.
            self.clear_errors();
            self.0.errors.append(&alert(
                "Another plugin install is already running. Wait for it to finish.",
            ));
            return;
        };
        let Some(root) = self.0.hub.root() else {
            return;
        };
        let Some(candidate) = self.0.ready.borrow_mut().take() else {
            return;
        };
        let replacing = self.phase() == Phase::Replace;
        #[cfg(test)]
        let delay = self.0.hub.install_delay();
        self.clear_errors();
        #[cfg(test)]
        let panic_worker = self.0.hub.worker_panics();
        self.set_phase(Phase::Installing);
        self.spawn(move |_, sender| {
            #[cfg(test)]
            if panic_worker {
                panic!("injected worker panic");
            }
            let mode = if replacing {
                InstallMode::Replace
            } else {
                InstallMode::New
            };
            #[cfg(test)]
            std::thread::sleep(delay);
            let outcome = install(&candidate.staged, &root, mode);
            // Released before the report goes out, so the next request is not refused.
            drop(guard);
            let _ = sender.send(Report::Installed { candidate, outcome });
        });
    }

    /// Polls the worker once; false when no job is left to poll.
    fn pump(&self) -> bool {
        let received = self
            .0
            .job
            .borrow()
            .as_ref()
            .map(|job| job.receiver.try_recv());
        match received {
            None => false,
            Some(Err(mpsc::TryRecvError::Empty)) => true,
            Some(Ok(report)) => {
                self.finish_job();
                if self.0.closed.get() {
                    self.report_detached(report);
                } else {
                    self.handle(report);
                }
                false
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                let installing = self.phase() == Phase::Installing;
                self.finish_job();
                if self.0.closed.get() {
                    return false;
                }
                self.show_failure(&[if installing {
                    "The installation stopped unexpectedly. Check the plugins folder before trying again."
                        .to_owned()
                } else {
                    "The plugin check stopped unexpectedly. Nothing was installed.".to_owned()
                }]);
                false
            }
        }
    }

    fn finish_job(&self) {
        if let Some(job) = self.0.job.borrow_mut().take() {
            // The worker has sent its report, so this returns at once.
            let _ = job.handle.join();
        }
    }

    fn handle(&self, report: Report) {
        match report {
            Report::Checked(Err(CheckFailure::Cancelled)) => {
                self.set_phase(Phase::Idle);
                self.0.progress.set_visible(true);
                self.0.progress.set_text("Check cancelled.");
            }
            Report::Checked(Err(CheckFailure::Messages(messages))) => self.show_failure(&messages),
            Report::Checked(Ok(candidate)) => {
                if candidate.text == self.0.entry.text().as_str() {
                    self.show_candidate(candidate);
                } else {
                    self.set_phase(Phase::Idle);
                }
            }
            Report::Installed { candidate, outcome } => match outcome {
                Ok(done) => {
                    self.0.hub.rescan();
                    (self.0.notify)(&success_message(&candidate.staged, &done), false);
                    discard(candidate);
                    self.0.window.close();
                }
                Err(InstallError::AlreadyInstalled {
                    installed_version, ..
                }) => {
                    let candidate = Candidate {
                        installed: Some(installed_version),
                        ..candidate
                    };
                    self.show_candidate(candidate);
                    self.0.errors.append(&alert(
                        "That plugin was installed in the meantime. Review the notice below; Replace swaps it for this version and disables it.",
                    ));
                }
                Err(error) => {
                    self.show_candidate(candidate);
                    for message in install_messages(&error) {
                        self.0.errors.append(&alert(&message));
                    }
                }
            },
        }
    }

    fn show_candidate(&self, candidate: Candidate) {
        self.clear_results();
        let card = preview_card(candidate.staged.preview());
        self.0.card.append(&card);
        for notice in pinned_notices(candidate.installed.as_ref()) {
            self.0.pinned.append(&notice);
        }
        let phase = if candidate.installed.is_some() {
            Phase::Replace
        } else {
            Phase::Preview
        };
        *self.0.ready.borrow_mut() = Some(candidate);
        self.set_phase(phase);
    }

    fn browse(&self) {
        let chooser = self.create_chooser();
        chooser.show();
        *self.0.chooser.borrow_mut() = Some(chooser);
    }

    fn create_chooser(&self) -> gtk::FileChooserNative {
        let chooser = gtk::FileChooserNative::new(
            Some("Choose plugin folder"),
            Some(&self.0.window),
            gtk::FileChooserAction::SelectFolder,
            Some("Choose"),
            Some("Cancel"),
        );
        chooser.set_modal(true);
        let weak = Rc::downgrade(&self.0);
        chooser.connect_response(move |chooser, response| {
            if let Some(state) = weak.upgrade() {
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|file| file.path()) {
                        state.entry.set_text(&path.to_string_lossy());
                    }
                }
                state.chooser.borrow_mut().take();
            }
            chooser.destroy();
        });
        chooser
    }

    /// Never blocks: cancels the worker, releases the preview's staging directory (deleted on a
    /// short-lived thread) and returns. The poll started by `spawn` keeps running until the
    /// worker reports, joins it, and reports an install that finished meanwhile.
    fn shutdown(&self) {
        self.0.closed.set(true);
        for (parent, id) in self.0.parent_links.take() {
            parent.disconnect(id);
        }
        let chooser = self.0.chooser.borrow_mut().take();
        if let Some(chooser) = chooser {
            chooser.destroy();
        }
        if let Some(job) = self.0.job.borrow().as_ref() {
            job.cancel.cancel();
        }
        let released = self.0.ready.borrow_mut().take();
        if let Some(candidate) = released {
            discard(candidate);
        }
    }

    /// A report that arrives after the dialog closed: an install outcome still reaches the
    /// status line; any staged copy is released.
    fn report_detached(&self, report: Report) {
        match report {
            Report::Installed { candidate, outcome } => {
                let (message, failed) = match &outcome {
                    Ok(done) => {
                        self.0.hub.rescan();
                        (success_message(&candidate.staged, done), false)
                    }
                    Err(error) => (
                        format!(
                            "Installing {} failed: {}",
                            clean(&candidate.staged.preview().name),
                            install_messages(error).join(" ")
                        ),
                        true,
                    ),
                };
                (self.0.notify)(&message, failed);
                discard(candidate);
            }
            Report::Checked(Ok(candidate)) => discard(candidate),
            Report::Checked(Err(_)) => {}
        }
    }
}

/// The worker half of a check: parse, stage, and look for an installed plugin of the same id.
fn check(
    text: &str,
    root: Option<std::path::PathBuf>,
    options: &StageOptions,
    cancel: &CancelToken,
) -> Result<Candidate, CheckFailure> {
    let messages = |list: Vec<String>| CheckFailure::Messages(list);
    let root = root.ok_or_else(|| {
        messages(vec![
            "The plugins folder is not available, so plugins cannot be added.".into(),
        ])
    })?;
    let source = parse_source_in(text, &root)
        .map_err(|error| messages(vec![sentence(&error.to_string())]))?;
    let staged = stage(&source, options, cancel).map_err(|error| match error {
        StageError::Cancelled => CheckFailure::Cancelled,
        other => messages(stage_messages(&other)),
    })?;
    let id = staged.manifest().id.clone();
    let installed = fs::symlink_metadata(root.join(&id)).ok().map(|_| {
        PluginRegistry::open(&root)
            .plugin(&id)
            .map(|manifest| manifest.version.clone())
    });
    Ok(Candidate {
        text: text.to_owned(),
        staged,
        installed,
    })
}

/// Capitalised, ending in a full stop: library messages are lower-case fragments.
fn sentence(text: &str) -> String {
    let cleaned = clean(text);
    let text = cleaned.trim();
    let mut chars = text.chars();
    let mut out: String = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    if !out.ends_with(['.', '!', '?', ':']) {
        out.push('.');
    }
    out
}

fn format_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KIB {
        format!("{bytes} bytes")
    } else if value < KIB * KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{:.1} MiB", value / (KIB * KIB))
    }
}

/// A few safe lines of git's own complaint: no control characters, bounded.
fn git_excerpt(diagnostics: &str) -> String {
    diagnostics
        .lines()
        .map(clean)
        .map(|line| line.trim().chars().take(200).collect::<String>())
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every [`StageError`] as readable sentences; cancelling says nothing.
pub(crate) fn stage_messages(error: &StageError) -> Vec<String> {
    match error {
        StageError::Source(message) => vec![sentence(message)],
        StageError::Invalid(problems) => {
            let mut lines =
                vec!["This does not follow the plugin format. Nothing was installed.".to_owned()];
            lines.extend(problems.iter().take(MAX_LISTED_PROBLEMS).map(|p| sentence(p)));
            if problems.len() > MAX_LISTED_PROBLEMS {
                lines.push(format!(
                    "And {} more problems.",
                    problems.len() - MAX_LISTED_PROBLEMS
                ));
            }
            lines
        }
        StageError::GitMissing(_) => vec![
            "Repository addresses need git, and git could not be started. Install git, or choose a folder on this computer instead."
                .to_owned(),
        ],
        StageError::GitFailed {
            status,
            diagnostics,
        } => {
            let head = match status {
                Some(code) => format!(
                    "The repository could not be downloaded: git failed with exit status {code}. Check the address and that the repository is public."
                ),
                None => "The download was stopped before git finished.".to_owned(),
            };
            let excerpt = git_excerpt(diagnostics);
            if excerpt.is_empty() {
                vec![head]
            } else {
                vec![head, sentence(&format!("git said: {excerpt}"))]
            }
        }
        StageError::DeadlineExceeded(limit) => vec![format!(
            "Downloading took longer than {} seconds and was stopped. Check the address and your connection, or choose a folder instead.",
            limit.as_secs().max(1)
        )],
        StageError::TooLarge { limit } => vec![format!(
            "The repository is larger than {}, more than a plugin may be, so the download was stopped.",
            format_size(*limit)
        )],
        StageError::Cancelled => Vec::new(),
        StageError::BadCommit(_) => vec![
            "Git reported an unexpected version for the downloaded repository, so it was not used."
                .to_owned(),
        ],
        StageError::Io(message) => vec![format!(
            "Could not prepare the plugin: {}",
            sentence(message)
        )],
    }
}

/// Every [`InstallError`] except `AlreadyInstalled` (which flips the dialog to Replace).
pub(crate) fn install_messages(error: &InstallError) -> Vec<String> {
    match error {
        InstallError::AlreadyInstalled { id, .. } => vec![format!(
            "A plugin with the id \"{id}\" is already installed."
        )],
        InstallError::InvalidState(message) => vec![format!(
            "The enabled-plugins file cannot be read ({message}), so nothing was changed. Repair or remove plugins.json and try again."
        )],
        InstallError::Io(message) => vec![format!(
            "Could not install the plugin: {}",
            sentence(message)
        )],
    }
}

fn success_message(staged: &StagedPlugin, done: &Installed) -> String {
    let preview = staged.preview();
    let (name, version) = (clean(&preview.name), clean(&done.version));
    match (done.replaced, done.disabled) {
        (true, true) => format!(
            "Installed {name} {version}, replacing the previous version. The plugin was disabled: enable it again in the Plugins section."
        ),
        (true, false) => format!(
            "Installed {name} {version}, replacing the previous version. It is disabled: enable it in the Plugins section."
        ),
        (false, _) => format!(
            "Installed {name} {version}. It is disabled: enable it in the Plugins section."
        ),
    }
}

fn heading_label(text: &str) -> Label {
    let text = clean(text);
    let label = Label::builder()
        .label(&text)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(50)
        .tooltip_text(&text)
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    label.add_css_class("section-label");
    label
}

/// What must stay visible under the scrolling card: the replace notice and the run warning.
fn pinned_notices(installed: Option<&Option<String>>) -> Vec<Label> {
    let mut notices = Vec::new();
    if let Some(version) = installed {
        let current = version
            .as_deref()
            .map_or_else(String::new, |version| format!(" (version {version})"));
        let notice = text_row(&format!(
            "This plugin is already installed{current}. Replacing it swaps in the files above and disables it until you enable it again in the Plugins section."
        ));
        notice.add_css_class("feedback-warning");
        notices.push(notice);
    }
    let warning = text_row(RUN_WARNING);
    warning.add_css_class("feedback-warning");
    notices.push(warning);
    notices
}

/// What a valid candidate offers, shown before anything is installed.
fn preview_card(preview: &PluginPreview) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 6);
    card.add_css_class("plugin-preview");
    card.set_accessible_role(gtk::AccessibleRole::Group);
    card.update_property(&[gtk::accessible::Property::Label("Plugin preview")]);
    card.append(&heading_label(&format!(
        "{} {}",
        preview.name, preview.version
    )));
    card.append(&text_row(&format!("Name: {}", preview.name)));
    card.append(&text_row(&format!("Id: {}", preview.id)));
    card.append(&text_row(&format!("Version: {}", preview.version)));
    let source = match &preview.commit {
        Some(commit) => format!(
            "Source: {} (commit {})",
            preview.source,
            commit.chars().take(10).collect::<String>()
        ),
        None => format!("Source: {}", preview.source),
    };
    card.append(&text_row(&source));
    card.append(&text_row(&format!(
        "Files: {} ({})",
        preview.file_count,
        format_size(preview.total_bytes)
    )));
    card.append(&heading_label("Actions"));
    for action in &preview.actions {
        let controls: Vec<&str> = action.controls.iter().map(|c| control_name(*c)).collect();
        card.append(&text_row(&format!(
            "{}: {}",
            action.name,
            controls.join(" · ")
        )));
        if !action.settings.is_empty() {
            let settings: Vec<String> = action
                .settings
                .iter()
                .map(|s| {
                    format!(
                        "{} ({}{})",
                        s.label,
                        s.kind,
                        if s.required { ", required" } else { "" }
                    )
                })
                .collect();
            let row = crate::ui::helper(&clean(&format!("Settings: {}", settings.join(", "))));
            card.append(&row);
        }
    }
    if !preview.requirements.is_empty() {
        card.append(&heading_label("Requires"));
        for entry in &preview.requirements {
            let requirement = &entry.requirement;
            let mut text = format!("{}: {}", requirement.kind.as_str(), requirement.name);
            if let Some(version) = &requirement.version {
                text.push_str(&format!(" {version}"));
            }
            if requirement.optional {
                text.push_str(" (optional)");
            }
            card.append(&text_row(&text));
            if entry.missing {
                if requirement.optional {
                    card.append(&crate::ui::helper(&clean(&format!(
                        "Not found: {}",
                        requirement.name
                    ))));
                } else {
                    card.append(&alert(&format!("Missing: {}", requirement.name)));
                }
            }
        }
        if preview.requirements.iter().any(|entry| entry.missing) {
            card.append(&crate::ui::helper(
                "A missing command does not stop the install; the plugin will fail until it is installed.",
            ));
        }
    }
    card
}

/// The visible dialog of `hub`, if any.
#[cfg(test)]
pub(crate) fn current(hub: &PluginHub) -> Option<AddPluginDialog> {
    hub.add_dialog()
        .filter(|state| state.window.is_visible())
        .map(AddPluginDialog)
}

#[cfg(test)]
impl AddPluginDialog {
    pub(crate) fn state_phase(&self) -> Phase {
        self.phase()
    }

    pub(crate) fn window(&self) -> gtk::Window {
        self.0.window.clone()
    }

    pub(crate) fn entry(&self) -> Entry {
        self.0.entry.clone()
    }

    pub(crate) fn check_button(&self) -> Button {
        self.0.check.clone()
    }

    pub(crate) fn cancel_button(&self) -> Button {
        self.0.cancel.clone()
    }

    pub(crate) fn install_button(&self) -> Button {
        self.0.install.clone()
    }

    pub(crate) fn browse_button(&self) -> Button {
        self.0.browse.clone()
    }

    pub(crate) fn error_labels(&self) -> Vec<Label> {
        let mut labels = Vec::new();
        let mut child = self.0.errors.first_child();
        while let Some(widget) = child {
            labels.extend(widget.downcast_ref::<Label>().cloned());
            child = widget.next_sibling();
        }
        labels
    }

    pub(crate) fn card(&self) -> GtkBox {
        self.0.card.clone()
    }

    pub(crate) fn pinned(&self) -> GtkBox {
        self.0.pinned.clone()
    }

    pub(crate) fn spinner_spinning(&self) -> bool {
        self.0.spinner.is_spinning()
    }

    pub(crate) fn chooser(&self) -> gtk::FileChooserNative {
        self.create_chooser()
    }

    pub(crate) fn candidate_text(&self) -> Option<String> {
        self.0.ready.borrow().as_ref().map(|c| c.text.clone())
    }
}
