//! The "Add plugin" dialog: a folder path or an `https://` repository address is checked on a
//! worker thread (`sdeck_plus::plugins::{parse_source_in, stage}`), previewed, and only then
//! installed (disabled) by an explicit press of Install or Replace. Nothing here runs a plugin.
use crate::plugin_ui::{control_name, PluginHub};
use gtk::{
    glib, prelude::*, Box as GtkBox, Button, Entry, Image, Label, Orientation, ToggleButton,
};
use sdeck_plus::plugins::{
    install, parse_source_in, stage, CancelToken, InstallError, InstallMode, Installed,
    PluginPreview, PluginRegistry, PluginSource, StageError, StageOptions, StagedPlugin,
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
const RUN_WARNING: &str = "Once enabled, this plugin runs with your user's privileges. It gets no administrator (root) rights, but it can read your files and use your network. Read its code before you enable it.";

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
    /// The "Folder | Repository" selector: it only changes the hints and Browse, never what is
    /// checked (that is decided by `parse_source_in` from the text).
    folder_kind: ToggleButton,
    repo_kind: ToggleButton,
    hint: Label,
    intro: Label,
    why: Label,
    empty: GtkBox,
    scroll: gtk::ScrolledWindow,
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

/// An alert-role label: assistive technology announces it when it appears.
fn alert_label(text: &str) -> Label {
    Label::builder()
        .label(clean(text))
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .hexpand(true)
        .accessible_role(gtk::AccessibleRole::Alert)
        .build()
}

/// One line of the error callout: an icon (never colour alone) and the alert text.
fn error_row(text: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("plugin-error-row");
    let icon = Image::from_icon_name("window-close-symbolic");
    icon.set_valign(gtk::Align::Start);
    icon.add_css_class("plugin-error-icon");
    row.append(&icon);
    row.append(&alert_label(text));
    row
}

/// Untrusted text that wraps instead of widening the dialog; never markup.
fn flowing(text: &str) -> Label {
    Label::builder()
        .label(clean(text))
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .hexpand(true)
        .build()
}

fn chip(text: &str, classes: &[&str]) -> Label {
    let label = Label::new(Some(&clean(text)));
    label.add_css_class("plugin-chip");
    for class in classes {
        label.add_css_class(class);
    }
    label
}

/// A chip with a small icon in front of its text; the label is returned for later lookups.
fn icon_chip(icon: &str, text: &str, classes: &[&str]) -> (GtkBox, Label) {
    let holder = GtkBox::new(Orientation::Horizontal, 5);
    holder.set_valign(gtk::Align::Center);
    holder.add_css_class("plugin-chip");
    for class in classes {
        holder.add_css_class(class);
    }
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(14);
    holder.append(&image);
    let label = Label::new(Some(&clean(text)));
    holder.append(&label);
    (holder, label)
}

/// A pinned note: an icon, a short title and the sentence, with a toned left edge.
fn callout(icon: &str, title: &str, body: &str, tone: &str) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .accessible_role(gtk::AccessibleRole::Note)
        .build();
    row.add_css_class("plugin-callout");
    row.add_css_class(tone);
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(22);
    image.set_valign(gtk::Align::Start);
    image.add_css_class("plugin-callout-icon");
    row.append(&image);
    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    let heading = flowing(title);
    heading.add_css_class("plugin-callout-title");
    text.append(&heading);
    text.append(&flowing(body));
    row.append(&text);
    row
}

/// One half of the "Folder | Repository" selector.
fn kind_button(icon: &str, caption: &str, tooltip: &str) -> ToggleButton {
    let button = ToggleButton::new();
    button.set_child(Some(&crate::ui::icon_caption(icon, caption)));
    button.set_tooltip_text(Some(tooltip));
    button.update_property(&[gtk::accessible::Property::Label(caption)]);
    button
}

impl AddPluginDialog {
    fn build(parent: Option<&gtk::Window>, hub: &PluginHub, notify: Notify) -> Self {
        let window = gtk::Window::builder()
            .title("Add plugin")
            .modal(true)
            .default_width(660)
            .default_height(760)
            .build();
        window.set_transient_for(parent);
        window.add_css_class("plugin-add-dialog");
        let entry = Entry::builder().hexpand(true).build();
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
        let errors = GtkBox::new(Orientation::Vertical, 8);
        errors.add_css_class("plugin-callout");
        errors.add_css_class("error");
        let card = GtkBox::new(Orientation::Vertical, 0);
        let pinned = GtkBox::new(Orientation::Vertical, 8);
        pinned.add_css_class("plugin-pinned");
        pinned.set_visible(false);
        errors.set_visible(false);

        // The header row is the window's title bar, so the dialog keeps the native frame and
        // can still be dragged by it, but reads as one surface with its content.
        let title = Label::builder()
            .label("Add plugin")
            .xalign(0.0)
            .hexpand(true)
            .accessible_role(gtk::AccessibleRole::Heading)
            .build();
        title.add_css_class("plugin-add-title");
        let header_close = crate::ui::icon_button("window-close-symbolic", "Close");
        header_close.add_css_class("plugin-close");
        let header = GtkBox::new(Orientation::Horizontal, 8);
        header.append(&title);
        header.append(&header_close);
        let handle = gtk::WindowHandle::new();
        handle.add_css_class("plugin-add-header");
        handle.set_child(Some(&header));
        window.set_titlebar(Some(&handle));

        let intro = crate::ui::helper(INTRO);
        intro.set_max_width_chars(-1);
        intro.add_css_class("plugin-add-intro");
        let source_title = Label::builder()
            .label("Plugin source")
            .xalign(0.0)
            .hexpand(true)
            .accessible_role(gtk::AccessibleRole::Heading)
            .build();
        source_title.add_css_class("section-label");
        let folder_kind = kind_button("folder-symbolic", "Folder", "A folder on this computer");
        let repo_kind = kind_button(
            "folder-remote-symbolic",
            "Repository",
            "An HTTPS repository",
        );
        repo_kind.set_group(Some(&folder_kind));
        folder_kind.set_active(true);
        let kinds = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .accessible_role(gtk::AccessibleRole::Group)
            .build();
        kinds.add_css_class("linked");
        kinds.add_css_class("plugin-source-kind");
        kinds.update_property(&[gtk::accessible::Property::Label("Source type")]);
        kinds.append(&folder_kind);
        kinds.append(&repo_kind);
        let source_head = GtkBox::new(Orientation::Horizontal, 8);
        source_head.append(&source_title);
        source_head.append(&kinds);
        let field = GtkBox::new(Orientation::Horizontal, 8);
        field.append(&entry);
        field.append(&browse);
        field.append(&check);
        let hint = Label::builder().xalign(0.0).wrap(true).build();
        hint.add_css_class("helper");
        let source = GtkBox::new(Orientation::Vertical, 8);
        source.append(&source_head);
        source.append(&field);
        source.append(&hint);
        let working = GtkBox::new(Orientation::Horizontal, 8);
        working.append(&spinner);
        working.append(&progress);
        working.append(&cancel);

        let top = GtkBox::new(Orientation::Vertical, 12);
        top.set_margin_top(16);
        top.set_margin_bottom(12);
        top.set_margin_start(16);
        top.set_margin_end(16);
        top.append(&intro);
        top.append(&source);
        top.append(&working);

        let empty = GtkBox::new(Orientation::Vertical, 6);
        empty.add_css_class("card");
        empty.add_css_class("plugin-empty");
        empty.set_accessible_role(gtk::AccessibleRole::Group);
        empty.update_property(&[gtk::accessible::Property::Label("No plugin checked yet")]);
        let empty_icon = Image::from_icon_name("text-x-generic-symbolic");
        empty_icon.set_pixel_size(28);
        empty.append(&empty_icon);
        let empty_title = Label::new(Some("No plugin checked yet"));
        empty_title.add_css_class("section-label");
        empty.append(&empty_title);
        let empty_hint = flowing(
            "Paste a repository address or choose a folder, then press Check plugin. Checking installs nothing.",
        );
        empty_hint.set_justify(gtk::Justification::Center);
        empty_hint.set_xalign(0.5);
        empty_hint.add_css_class("helper");
        empty.append(&empty_hint);

        let content = GtkBox::new(Orientation::Vertical, 12);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_bottom(12);
        content.append(&errors);
        content.append(&empty);
        content.append(&card);
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(160)
            .child(&content)
            .build();

        let why = Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        why.add_css_class("helper");
        let gap = GtkBox::new(Orientation::Horizontal, 0);
        gap.set_hexpand(true);
        let footer = GtkBox::new(Orientation::Horizontal, 8);
        footer.add_css_class("plugin-add-footer");
        footer.append(&why);
        footer.append(&gap);
        footer.append(&close);
        footer.append(&install);
        pinned.set_margin_start(16);
        pinned.set_margin_end(16);
        pinned.set_margin_bottom(12);

        let body = GtkBox::new(Orientation::Vertical, 0);
        body.append(&top);
        body.append(&scroll);
        body.append(&pinned);
        body.append(&footer);
        window.set_child(Some(&body));
        gtk::prelude::GtkWindowExt::set_focus(&window, Some(&entry));

        let state = Rc::new(State {
            window: window.clone(),
            entry,
            folder_kind,
            repo_kind,
            hint,
            intro,
            why,
            empty,
            scroll,
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
        dialog.apply_kind();
        dialog.set_phase(Phase::Idle);
        dialog.wire(&[close, header_close]);
        dialog
    }

    fn wire(&self, closers: &[Button]) {
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
        for close in closers {
            let window = state.window.downgrade();
            close.connect_clicked(move |_| {
                if let Some(window) = window.upgrade() {
                    window.close();
                }
            });
        }
        let weak_kind = weak.clone();
        state.repo_kind.connect_toggled(move |_| {
            if let Some(state) = weak_kind.upgrade() {
                AddPluginDialog(state).apply_kind();
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
        state.folder_kind.set_sensitive(editable);
        state.repo_kind.set_sensitive(editable);
        state.check.set_sensitive(editable);
        state.intro.set_visible(matches!(
            phase,
            Phase::Idle | Phase::Checking | Phase::Invalid
        ));
        state.empty.set_visible(phase == Phase::Idle);
        // Say what is needed while Install cannot be pressed; nothing to say once it can.
        state.why.set_visible(matches!(
            phase,
            Phase::Idle | Phase::Checking | Phase::Invalid
        ));
        state.why.set_text("Check a plugin to enable Install.");
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
        self.refresh_check_style();
        if editable && was_busy {
            state.entry.grab_focus();
        }
    }

    /// Check plugin is the primary action while there is text to check and no preview yet; once
    /// a preview exists, Install or Replace takes that role.
    fn refresh_check_style(&self) {
        let state = &self.0;
        let primary = matches!(state.phase.get(), Phase::Idle | Phase::Invalid)
            && !state.entry.text().trim().is_empty();
        if primary {
            state.check.add_css_class("suggested-action");
        } else {
            state.check.remove_css_class("suggested-action");
        }
    }

    /// The selector only changes the hints and Browse; `parse_source_in` decides what is checked.
    fn apply_kind(&self) {
        let state = &self.0;
        let repository = state.repo_kind.is_active();
        state.entry.set_placeholder_text(Some(if repository {
            "https://github.com/owner/plugin"
        } else {
            "Absolute folder path, for example /home/you/sdeck-plugin-hello"
        }));
        state.hint.set_text(if repository {
            "Public HTTPS repositories only; the plugin must be at the repository root."
        } else {
            "Choose the plugin's own project folder, not the app's plugins folder."
        });
        state.browse.set_visible(!repository);
    }

    /// Keeps the selector from contradicting the text: an `https://` address means Repository,
    /// an absolute (or `~`) path means Folder.
    fn follow_text(&self) {
        let state = &self.0;
        let text = state.entry.text();
        let text = text.trim_start().to_lowercase();
        if text.starts_with("https://") && !state.repo_kind.is_active() {
            state.repo_kind.set_active(true);
        } else if text.starts_with(['/', '~']) && state.repo_kind.is_active() {
            state.folder_kind.set_active(true);
        }
    }

    fn clear_results(&self) {
        let state = &self.0;
        self.clear_errors();
        for area in [&state.card, &state.pinned] {
            while let Some(child) = area.first_child() {
                area.remove(&child);
            }
        }
        state.pinned.set_visible(false);
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
        self.0.errors.set_visible(false);
    }

    fn add_error(&self, message: &str) {
        self.0.errors.append(&error_row(message));
        self.0.errors.set_visible(true);
    }

    fn show_failure(&self, messages: &[String]) {
        for message in messages {
            self.add_error(message);
        }
        self.set_phase(Phase::Invalid);
    }

    fn entry_edited(&self) {
        self.follow_text();
        if self.busy() {
            return;
        }
        self.refresh_check_style();
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
            self.add_error("Another plugin install is already running. Wait for it to finish.");
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
                    self.add_error(
                        "That plugin was installed in the meantime. Review the notice below; Replace swaps it for this version and disables it.",
                    );
                }
                Err(error) => {
                    self.show_candidate(candidate);
                    for message in install_messages(&error) {
                        self.add_error(&message);
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
        self.0.pinned.set_visible(true);
        self.0.scroll.vadjustment().set_value(0.0);
        self.reveal_missing();
        let phase = if candidate.installed.is_some() {
            Phase::Replace
        } else {
            Phase::Preview
        };
        *self.0.ready.borrow_mut() = Some(candidate);
        self.set_phase(phase);
    }

    /// Once layout has happened, scrolls the card so the first missing required command is fully
    /// visible. Runs on the frame clock for a few frames (the expander's content is laid out a
    /// frame late); it never blocks and stops by itself.
    fn reveal_missing(&self) {
        const MARGIN: f64 = 8.0;
        let scroll = self.0.scroll.clone();
        let card = self.0.card.clone();
        let frames = Cell::new(0u8);
        scroll.clone().add_tick_callback(move |_, _| {
            frames.set(frames.get() + 1);
            let Some(row) = with_missing_row(&card) else {
                return glib::ControlFlow::Break;
            };
            let Some(viewport) = scroll.child() else {
                return glib::ControlFlow::Break;
            };
            if let Some(bounds) = row.compute_bounds(&viewport) {
                let adjustment = scroll.vadjustment();
                let (top, bottom) = (
                    f64::from(bounds.y()),
                    f64::from(bounds.y() + bounds.height()),
                );
                if bottom > adjustment.value() + adjustment.page_size() {
                    adjustment.set_value(bottom - adjustment.page_size() + MARGIN);
                }
                if top < adjustment.value() {
                    adjustment.set_value((top - MARGIN).max(0.0));
                }
            }
            if frames.get() >= 6 {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
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

/// What must stay visible under the scrolling card: the replace notice and the run warning.
fn pinned_notices(installed: Option<&Option<String>>) -> Vec<GtkBox> {
    let mut notices = Vec::new();
    if let Some(version) = installed {
        let current = version
            .as_deref()
            .map_or_else(String::new, |version| format!(" (version {version})"));
        notices.push(callout(
            "dialog-information-symbolic",
            "Replacing disables the plugin",
            &format!(
                "This plugin is already installed{current}. Replacing it swaps in the files above and disables it until you enable it again in the Plugins section."
            ),
            "info",
        ));
    }
    notices.push(callout(
        "dialog-warning-symbolic",
        "Runs as your user, not as administrator",
        RUN_WARNING,
        "warning",
    ));
    notices
}

/// A collapsible block of the preview card: a title with a count pill over its content.
fn section(title: &str, count: &str, expanded: bool, content: &GtkBox) -> gtk::Expander {
    let heading = GtkBox::new(Orientation::Horizontal, 8);
    let name = Label::new(Some(title));
    name.add_css_class("plugin-section-title");
    let pill = Label::new(Some(count));
    pill.add_css_class("plugin-count");
    heading.append(&name);
    heading.append(&pill);
    content.set_margin_top(4);
    let expander = gtk::Expander::builder()
        .label_widget(&heading)
        .child(content)
        .expanded(expanded)
        .build();
    expander.add_css_class("plugin-section");
    expander.update_property(&[gtk::accessible::Property::Label(&format!(
        "{title}, {count}"
    ))]);
    expander
}

fn stat(name: &str, value: &Label) -> GtkBox {
    let cell = GtkBox::new(Orientation::Vertical, 2);
    cell.add_css_class("plugin-stat");
    let caption = Label::builder().label(name).xalign(0.0).build();
    caption.add_css_class("plugin-stat-name");
    value.set_xalign(0.0);
    cell.append(&caption);
    cell.append(value);
    cell
}

fn action_rows(preview: &PluginPreview) -> GtkBox {
    let rows = GtkBox::new(Orientation::Vertical, 0);
    rows.add_css_class("plugin-rows");
    for action in &preview.actions {
        let row = GtkBox::new(Orientation::Vertical, 2);
        row.add_css_class("plugin-row");
        let line = GtkBox::new(Orientation::Horizontal, 12);
        let name = flowing(&action.name);
        name.add_css_class("plugin-row-name");
        line.append(&name);
        let controls = GtkBox::new(Orientation::Horizontal, 4);
        controls.set_valign(gtk::Align::Center);
        for control in &action.controls {
            controls.append(&chip(control_name(*control), &["plugin-mono"]));
        }
        line.append(&controls);
        row.append(&line);
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
            let line = flowing(&format!("Settings: {}", settings.join(", ")));
            line.add_css_class("helper");
            row.append(&line);
        }
        rows.append(&row);
    }
    rows
}

/// The requirements list and whether a required command is missing (which opens the section).
fn requirement_rows(preview: &PluginPreview) -> (GtkBox, bool) {
    let rows = GtkBox::new(Orientation::Vertical, 0);
    rows.add_css_class("plugin-rows");
    let mut required_missing = false;
    for entry in &preview.requirements {
        let requirement = &entry.requirement;
        let mut text = format!("{}: {}", requirement.kind.as_str(), requirement.name);
        if let Some(version) = &requirement.version {
            text.push_str(&format!(" {version}"));
        }
        if requirement.optional {
            text.push_str(" (optional)");
        }
        let row = GtkBox::new(Orientation::Horizontal, 12);
        row.add_css_class("plugin-row");
        row.append(&flowing(&text));
        // Only commands are looked up on PATH, so only they can be reported found or missing.
        let status = if requirement.kind.as_str() != "command" {
            let (holder, _) = icon_chip("dialog-information-symbolic", "Not checked", &["dim"]);
            holder.set_tooltip_text(Some("Only commands are looked up on PATH."));
            holder
        } else if !entry.missing {
            icon_chip("object-select-symbolic", "Found", &["ok"]).0
        } else if requirement.optional {
            icon_chip(
                "dialog-information-symbolic",
                "Optional, not found",
                &["dim"],
            )
            .0
        } else {
            if !required_missing {
                // The row the dialog scrolls into view.
                row.add_css_class("plugin-missing-row");
            }
            required_missing = true;
            let (holder, label) = icon_chip(
                "window-close-symbolic",
                &format!("Missing: {}", requirement.name),
                &["bad"],
            );
            label.set_accessible_role(gtk::AccessibleRole::Alert);
            holder
        };
        status.set_valign(gtk::Align::Center);
        row.append(&status);
        rows.append(&row);
    }
    (rows, required_missing)
}

/// The first row of the card that reports a missing required command, if any.
fn with_missing_row(root: &impl IsA<gtk::Widget>) -> Option<gtk::Widget> {
    let mut stack = vec![root.as_ref().clone()];
    while let Some(widget) = stack.pop() {
        if widget.has_css_class("plugin-missing-row") {
            return Some(widget);
        }
        let mut child = widget.last_child();
        while let Some(next) = child {
            child = next.prev_sibling();
            stack.push(next);
        }
    }
    None
}

/// What a valid candidate offers, shown before anything is installed.
fn preview_card(preview: &PluginPreview) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 12);
    card.add_css_class("card");
    card.add_css_class("plugin-preview");
    card.set_accessible_role(gtk::AccessibleRole::Group);
    card.update_property(&[gtk::accessible::Property::Label("Plugin preview")]);

    let name = clean(&preview.name);
    let letter = name
        .chars()
        .find(|c| !c.is_whitespace())
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect());
    let tile = Label::new(Some(&letter));
    tile.add_css_class("plugin-monogram");
    tile.set_valign(gtk::Align::Center);
    tile.set_halign(gtk::Align::Center);
    tile.set_accessible_role(gtk::AccessibleRole::Presentation);
    let title = Label::builder()
        .label(&name)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .tooltip_text(&name)
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    title.add_css_class("control-heading");
    let (source_text, source_icon) = match &preview.source {
        PluginSource::Folder(_) => ("Folder", "folder-symbolic"),
        PluginSource::Repository(_) => ("Repository", "folder-remote-symbolic"),
    };
    let meta = GtkBox::new(Orientation::Horizontal, 6);
    meta.add_css_class("plugin-meta");
    // Long values shorten instead of widening the dialog; the full text is the tooltip.
    for text in [
        format!("v{}", preview.version),
        format!("id {}", preview.id),
    ] {
        let label = chip(&text, &["plugin-mono"]);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_tooltip_text(Some(&clean(&text)));
        meta.append(&label);
    }
    meta.append(&icon_chip(source_icon, source_text, &["plugin-pill"]).0);
    let identity = GtkBox::new(Orientation::Vertical, 6);
    identity.set_hexpand(true);
    identity.set_valign(gtk::Align::Center);
    identity.append(&title);
    identity.append(&meta);
    let head = GtkBox::new(Orientation::Horizontal, 14);
    head.append(&tile);
    head.append(&identity);
    card.append(&head);

    let source = clean(&preview.source.to_string());
    let address = flowing(&source);
    address.add_css_class("helper");
    address.set_tooltip_text(Some(&source));
    card.append(&address);

    let stats = GtkBox::new(Orientation::Horizontal, 0);
    stats.set_homogeneous(true);
    stats.add_css_class("plugin-stats");
    stats.set_accessible_role(gtk::AccessibleRole::Group);
    stats.update_property(&[gtk::accessible::Property::Label("At a glance")]);
    let value = |text: &str| {
        let label = Label::new(Some(text));
        label.add_css_class("plugin-stat-value");
        label
    };
    let commit = match &preview.commit {
        Some(commit) => {
            let label = value(&commit.chars().take(10).collect::<String>());
            label.add_css_class("plugin-mono");
            label
        }
        None => {
            let label = value("n/a");
            label.add_css_class("dim");
            label
        }
    };
    stats.append(&stat("Files", &value(&preview.file_count.to_string())));
    stats.append(&stat("Size", &value(&format_size(preview.total_bytes))));
    stats.append(&stat("Commit", &commit));
    stats.append(&stat("Actions", &value(&preview.actions.len().to_string())));
    card.append(&stats);

    // A missing required command is what needs attention, so the actions make room for it.
    let required_missing = preview
        .requirements
        .iter()
        .any(|entry| entry.missing && !entry.requirement.optional);
    card.append(&section(
        "Actions",
        &preview.actions.len().to_string(),
        !required_missing,
        &action_rows(preview),
    ));
    if !preview.requirements.is_empty() {
        let (rows, _) = requirement_rows(preview);
        let commands = preview
            .requirements
            .iter()
            .filter(|entry| entry.requirement.kind.as_str() == "command");
        let total = commands.clone().count();
        let found = commands.filter(|entry| !entry.missing).count();
        let count = if total == 0 {
            "not checked".to_owned()
        } else {
            format!("{found} of {total} found")
        };
        if preview.requirements.iter().any(|entry| entry.missing) {
            let note = flowing(
                "A missing command does not stop the install; the plugin will fail until it is installed.",
            );
            note.add_css_class("helper");
            note.set_margin_top(6);
            let body = GtkBox::new(Orientation::Vertical, 0);
            body.append(&rows);
            body.append(&note);
            card.append(&section("Requirements", &count, required_missing, &body));
        } else {
            card.append(&section("Requirements", &count, false, &rows));
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
        let mut rows = self.0.errors.first_child();
        while let Some(row) = rows {
            let mut child = row.first_child();
            while let Some(widget) = child {
                labels.extend(widget.downcast_ref::<Label>().cloned());
                child = widget.next_sibling();
            }
            rows = row.next_sibling();
        }
        labels
    }

    pub(crate) fn error_callout(&self) -> GtkBox {
        self.0.errors.clone()
    }

    pub(crate) fn folder_kind(&self) -> ToggleButton {
        self.0.folder_kind.clone()
    }

    pub(crate) fn repo_kind(&self) -> ToggleButton {
        self.0.repo_kind.clone()
    }

    pub(crate) fn hint(&self) -> Label {
        self.0.hint.clone()
    }

    pub(crate) fn why(&self) -> Label {
        self.0.why.clone()
    }

    pub(crate) fn scroller(&self) -> gtk::ScrolledWindow {
        self.0.scroll.clone()
    }

    pub(crate) fn empty_state(&self) -> GtkBox {
        self.0.empty.clone()
    }

    pub(crate) fn card(&self) -> GtkBox {
        self.0.card.clone()
    }

    pub(crate) fn pinned(&self) -> GtkBox {
        self.0.pinned.clone()
    }

    pub(crate) fn spinner_shown(&self) -> bool {
        self.0.spinner.is_visible() && self.0.spinner.is_mapped() && self.0.spinner.is_spinning()
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
