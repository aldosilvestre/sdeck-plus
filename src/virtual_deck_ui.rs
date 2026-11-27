//! The header menu's virtual deck, as drawn in
//! design/virtual-classic/04-all-in-menu.html: a "Virtual deck (Classic)"
//! switch below Export configuration..., and everything the switch needs.
//!
//! The switch is off on every launch. Turned on without a stored password it
//! shows an inline form first (the server never starts without one); a
//! password saved by an older version (an Argon2id hash) cannot log in over
//! plain HTTP, so it counts as none and the form says why. While on, the menu
//! shows the page address (`http://…`: QR code, Copy, Open), the password
//! (Change…), the port ("Restarts the server") and the viewers, each with
//! Disconnect; the header says "Virtual deck on · N viewers". The password
//! changes only while the deck is on, and a server that starts or restarts
//! takes the stored password before it serves.
//!
//! Turning it on starts the plain HTTP server, attaches a virtual Stream Deck
//! Classic to the HID hub (no browser window opens by itself; the Open button
//! does that); turning it off
//! (or quitting) stops the server, detaches the deck and joins the glue thread
//! that carries presses and pictures between the two. GTK never waits for any
//! of it: deriving the password verifier, starting and stopping run on owned
//! worker threads whose results, like the server's events, arrive through
//! async channels.
mod glue;
#[cfg(test)]
mod hold;
mod qr;

use crate::ui;
use gtk::{glib, prelude::*, Box as GtkBox, Button, Label, Orientation};
use sdeck_plus::{
    event_api::{virtual_deck_pair, VirtualDeck, VirtualDeckLink},
    virtual_deck::{
        build_verifier,
        settings::{parse_port, SettingsStore},
        DeckInput, ServerConfig, ServerEvent, Snapshot, Verifier, Viewer, VirtualDeckServer,
    },
};
use std::{
    cell::{Cell, RefCell},
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
    rc::Rc,
    sync::{mpsc, Arc},
    thread,
    time::SystemTime,
};

/// Every interface, so phones on the LAN reach the page.
#[cfg(not(test))]
pub(crate) const LAN: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
/// The test binary never listens beyond this computer.
#[cfg(test)]
pub(crate) const LAN: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

pub(crate) const TITLE: &str = "Virtual deck (Classic)";
pub(crate) const NEEDS_PASSWORD: &str = "Set a password before turning the virtual deck on.";
/// The first-time form's warning when the settings hold an older version's password.
pub(crate) const OLD_PASSWORD: &str =
    "Passwords saved by an older version no longer work. Set a new one before turning the virtual deck on.";
pub(crate) const MISMATCH: &str = "The passwords do not match.";
pub(crate) const NO_VIEWERS: &str = "No one is viewing the deck yet.";
pub(crate) const CHANGE_TITLE: &str = "Change the password?";
pub(crate) const CHANGE_WARNING: &str = "Changing the password disconnects every viewer.";
/// The confirmation's text while the server restarts: it can be confirmed once on.
pub(crate) const CONFIRM_WAIT: &str =
    "The server is restarting. Confirm once it is on; changing the password disconnects every viewer.";
/// Why a new password is refused while the server restarts.
pub(crate) const CHANGE_WAIT: &str = "The server is restarting. Change the password once it is on.";
const SWITCH_TIP: &str = "Serve a virtual Stream Deck Classic in a web page on this network";
/// The QR code's side, as in the artboard.
const QR_SIZE: i32 = 112;
/// Viewer rows shown before the list scrolls inside the menu.
const VIEWER_ROWS: i32 = 4;
/// One viewer row: 40px plus its divider.
const VIEWER_ROW_HEIGHT: i32 = 41;
/// The width of the forms and details, so the menu matches the artboard's 380px.
const CONTENT_WIDTH: i32 = 344;
/// The details' tallest natural height; past it (or on a short screen) they scroll.
const DETAILS_HEIGHT: i32 = 620;

/// The HID hub, as far as the virtual deck needs it.
pub(crate) trait DeckHub {
    /// Serve `deck` beside the hardware; a deck attached before is unplugged.
    fn attach(&self, deck: VirtualDeck) -> Result<(), String>;
    /// Unplug the attached deck, if any.
    fn detach(&self);
}

/// Opens the page address in the browser.
pub(crate) type Opener = Rc<dyn Fn(&str) -> Result<(), String>>;

/// What the virtual deck needs from the window.
pub(crate) struct Environment {
    /// The app's config directory: the settings live here.
    pub(crate) config_dir: Result<PathBuf, String>,
    /// Where the server listens: [`LAN`] in the app, loopback in tests.
    pub(crate) bind: IpAddr,
    pub(crate) hub: Rc<dyn DeckHub>,
    pub(crate) open: Opener,
}

/// Opens an address with `xdg-open` through the app's process runner; the
/// helper is reaped off the GTK thread, since it may stay with the browser.
#[cfg(not(test))]
pub(crate) fn browser_opener() -> Opener {
    use sdeck_plus::layers::{ProcessRunner, SystemProcessRunner};
    Rc::new(|url| {
        let argv = ["xdg-open".to_string(), url.to_string()];
        match SystemProcessRunner.launch(&argv) {
            Ok(Some(mut child)) => {
                let reaper = thread::Builder::new()
                    .name("virtual-deck-browser".into())
                    .spawn(move || {
                        let _ = child.wait();
                    });
                if let Err(error) = reaper {
                    eprintln!("Could not wait for xdg-open: {error}");
                }
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(error) => Err(format!("Could not open the browser: {error}")),
        }
    })
}

#[cfg(test)]
thread_local! {
    static OPENED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The test binary never starts a browser: it records the address instead.
#[cfg(test)]
pub(crate) fn browser_opener() -> Opener {
    Rc::new(|url| {
        OPENED.with(|opened| opened.borrow_mut().push(url.to_string()));
        Ok(())
    })
}

/// The addresses [`browser_opener`] was asked to open since the last call.
#[cfg(test)]
pub(crate) fn take_opened() -> Vec<String> {
    OPENED.with(|opened| std::mem::take(&mut *opened.borrow_mut()))
}

thread_local! {
    /// The address the virtual deck is served at, for the header's connection line.
    static SERVED_AT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The address the virtual deck is served at while it is on.
pub(crate) fn served_at() -> Option<String> {
    SERVED_AT.with(|served| served.borrow().clone())
}

fn set_served_at(url: Option<String>) {
    SERVED_AT.with(|served| *served.borrow_mut() = url);
}

/// "Virtual deck on · 2 viewers" (one viewer is "1 viewer").
pub(crate) fn badge_text(viewers: usize) -> String {
    match viewers {
        1 => "Virtual deck on · 1 viewer".to_string(),
        count => format!("Virtual deck on · {count} viewers"),
    }
}

/// The server moved to `port` but the settings could not remember it.
pub(crate) fn port_not_remembered(port: u16, error: &str) -> String {
    format!("The server now runs on port {port}, but the port could not be remembered: {error}")
}

/// "192.168.1.73 · since 21:04", in local time.
pub(crate) fn viewer_detail(viewer: &Viewer) -> String {
    let seconds = viewer
        .since
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let since = i64::try_from(seconds)
        .ok()
        .and_then(|seconds| glib::DateTime::from_unix_local(seconds).ok())
        .and_then(|time| time.format("%H:%M").ok())
        .map_or_else(|| "--:--".to_string(), |time| time.to_string());
    format!("{} · since {since}", viewer.address)
}

/// A phone for phones and tablets, a computer otherwise.
fn viewer_icon(browser: &str) -> &'static str {
    let mobile = ["Android", "iOS", "iPadOS"]
        .iter()
        .any(|system| browser.starts_with(system));
    if mobile {
        "phone-symbolic"
    } else {
        "computer-symbolic"
    }
}

/// The new password's verifier when both fields agree; the length rules come first.
fn checked_verifier(new: &str, repeat: &str) -> Result<Verifier, String> {
    let verifier = build_verifier(new)?;
    if new == repeat {
        Ok(verifier)
    } else {
        Err(MISMATCH.to_string())
    }
}

/// A "New password" and "Repeat it" pair with its error line and buttons.
pub(crate) struct PasswordForm {
    pub(crate) root: GtkBox,
    pub(crate) new: gtk::PasswordEntry,
    pub(crate) repeat: gtk::PasswordEntry,
    pub(crate) error: Label,
    pub(crate) cancel: Button,
    pub(crate) submit: Button,
}

impl PasswordForm {
    fn new(submit: &str) -> Self {
        let root = GtkBox::new(Orientation::Vertical, 8);
        let new = password_entry("New password");
        let repeat = password_entry("Repeat it");
        for (caption, entry) in [("New password", &new), ("Repeat it", &repeat)] {
            let label = Label::new(Some(caption));
            label.add_css_class("section-label");
            label.set_xalign(0.0);
            label.set_mnemonic_widget(Some(entry));
            root.append(&label);
            root.append(entry);
        }
        let error = Label::new(None);
        error.set_xalign(0.0);
        error.set_wrap(true);
        error.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        error.set_max_width_chars(38);
        error.set_visible(false);
        root.append(&error);
        let actions = GtkBox::new(Orientation::Horizontal, 6);
        actions.set_halign(gtk::Align::End);
        let cancel = Button::with_label("Cancel");
        let submit = Button::with_label(submit);
        submit.add_css_class("suggested-action");
        actions.append(&cancel);
        actions.append(&submit);
        root.append(&actions);
        // Enter in either field submits, as the primary button would.
        for entry in [&new, &repeat] {
            let submit = submit.downgrade();
            entry.connect_activate(move |_| {
                if let Some(submit) = submit.upgrade() {
                    if submit.is_sensitive() {
                        submit.emit_clicked();
                    }
                }
            });
        }
        Self {
            root,
            new,
            repeat,
            error,
            cancel,
            submit,
        }
    }

    fn texts(&self) -> (String, String) {
        (self.new.text().to_string(), self.repeat.text().to_string())
    }

    fn clear(&self) {
        self.new.set_text("");
        self.repeat.set_text("");
        ui::clear_feedback(&self.error);
        self.error.set_visible(false);
        self.set_busy(false);
    }

    fn show_error(&self, message: &str) {
        self.error.set_text(message);
        self.error.set_visible(true);
        ui::feedback(&self.error, true);
    }

    /// While the verifier is derived nothing in the form can be used.
    fn set_busy(&self, busy: bool) {
        for widget in [
            self.new.upcast_ref::<gtk::Widget>(),
            self.repeat.upcast_ref(),
            self.cancel.upcast_ref(),
            self.submit.upcast_ref(),
        ] {
            widget.set_sensitive(!busy);
        }
    }

    fn open(&self) {
        self.clear();
        self.root.set_visible(true);
        self.new.grab_focus();
    }
}

/// What a hidden password shows per character: a bullet, lighter than GTK's
/// default large disc.
pub(crate) const HIDDEN_CHAR: char = '\u{2022}';

fn password_entry(name: &str) -> gtk::PasswordEntry {
    let entry = gtk::PasswordEntry::builder().show_peek_icon(true).build();
    entry.add_css_class("virtual-deck-password");
    let mut child = entry.first_child();
    while let Some(widget) = child {
        if let Some(text) = widget.downcast_ref::<gtk::Text>() {
            text.set_invisible_char(HIDDEN_CHAR);
        }
        child = widget.next_sibling();
    }
    entry.update_property(&[gtk::accessible::Property::Label(name)]);
    entry
}

/// Every widget of the menu section.
pub(crate) struct Widgets {
    /// What the menu shows below its separator.
    pub(crate) section: GtkBox,
    pub(crate) switch: gtk::Switch,
    /// Why turning on (or opening the browser) failed.
    pub(crate) feedback: Label,
    pub(crate) first_time: GtkBox,
    /// "Set a password…", or why an older version's password no longer works.
    pub(crate) first_warning: Label,
    pub(crate) first_form: PasswordForm,
    pub(crate) running: GtkBox,
    pub(crate) qr: qr::QrView,
    pub(crate) address: Label,
    pub(crate) copy: Button,
    pub(crate) open: Button,
    pub(crate) change: Button,
    pub(crate) change_form: PasswordForm,
    pub(crate) port: gtk::Entry,
    pub(crate) port_feedback: Label,
    pub(crate) viewers_heading: Label,
    pub(crate) viewers: gtk::ListBox,
    pub(crate) viewers_scroll: gtk::ScrolledWindow,
    pub(crate) no_viewers: Label,
}

/// A wrapping feedback line, hidden until it has something to say.
fn feedback_line() -> Label {
    let label = Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_max_width_chars(38);
    label.set_visible(false);
    label
}

/// A monospace chip at most `characters` wide that wraps anywhere and can be
/// selected for copying.
fn chip(characters: i32) -> Label {
    let label = Label::new(None);
    label.add_css_class("reference-chip");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::Char);
    label.set_max_width_chars(characters);
    label.set_selectable(true);
    label
}

/// "Password" and "Port" headings, one width so their values line up.
fn row_heading(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("section-label");
    label.set_xalign(0.0);
    label.set_size_request(70, -1);
    label
}

fn build_widgets() -> Widgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.add_css_class("virtual-deck");

    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.add_css_class("virtual-deck-row");
    let icon = gtk::Image::from_icon_name("input-dialpad-symbolic");
    icon.set_pixel_size(16);
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    row.append(&icon);
    let title = Label::new(Some(TITLE));
    title.add_css_class("virtual-deck-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    row.append(&title);
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_tooltip_text(Some(SWITCH_TIP));
    switch.update_property(&[
        gtk::accessible::Property::Label(TITLE),
        gtk::accessible::Property::Description(SWITCH_TIP),
    ]);
    row.append(&switch);
    section.append(&row);

    let content = GtkBox::new(Orientation::Vertical, 12);
    content.add_css_class("virtual-deck-content");
    content.set_margin_top(4);
    content.set_margin_start(10);
    content.set_margin_end(10);
    content.set_margin_bottom(10);
    // On a short screen the menu still opens: the details scroll instead.
    let details = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_width(true)
        .propagate_natural_height(true)
        .max_content_height(DETAILS_HEIGHT)
        .child(&content)
        .build();
    section.append(&details);
    let feedback = feedback_line();
    content.append(&feedback);

    // First time: no password yet.
    let first_time = GtkBox::new(Orientation::Vertical, 8);
    first_time.set_size_request(CONTENT_WIDTH, -1);
    let first_warning = Label::new(Some(NEEDS_PASSWORD));
    first_warning.add_css_class("feedback-warning");
    first_warning.set_xalign(0.0);
    first_warning.set_wrap(true);
    first_warning.set_max_width_chars(38);
    first_time.append(&first_warning);
    let first_form = PasswordForm::new("Save and turn on");
    first_time.append(&first_form.root);
    first_time.set_visible(false);
    content.append(&first_time);

    // On: address, password, port and viewers.
    let running = GtkBox::new(Orientation::Vertical, 12);
    running.set_size_request(CONTENT_WIDTH, -1);
    let address_row = GtkBox::new(Orientation::Horizontal, 12);
    let qr = qr::QrView::new(QR_SIZE);
    address_row.append(&qr.area);
    let address_column = GtkBox::new(Orientation::Vertical, 6);
    address_column.set_valign(gtk::Align::Center);
    address_column.set_hexpand(true);
    // Beside the QR code: "http://192.168.100.200:65535/" fits on one line.
    let address = chip(30);
    address_column.append(&address);
    let address_actions = GtkBox::new(Orientation::Horizontal, 6);
    let copy = ui::button("edit-copy-symbolic", "Copy");
    copy.set_tooltip_text(Some("Copy the address"));
    copy.update_property(&[gtk::accessible::Property::Label("Copy the address")]);
    let open = ui::button("web-browser-symbolic", "Open");
    open.set_tooltip_text(Some("Open the address in the browser"));
    open.update_property(&[gtk::accessible::Property::Label(
        "Open the address in the browser",
    )]);
    address_actions.append(&copy);
    address_actions.append(&open);
    address_column.append(&address_actions);
    address_row.append(&address_column);
    running.append(&address_row);

    let password_row = GtkBox::new(Orientation::Horizontal, 8);
    password_row.append(&row_heading("Password"));
    let set = Label::new(Some("Set"));
    set.add_css_class("helper");
    set.set_xalign(0.0);
    set.set_hexpand(true);
    password_row.append(&set);
    let change = Button::with_label("Change…");
    change.set_tooltip_text(Some("Change the password; every viewer is disconnected"));
    password_row.append(&change);
    running.append(&password_row);
    let change_form = PasswordForm::new("Change password");
    change_form.root.set_visible(false);
    running.append(&change_form.root);

    let port_row = GtkBox::new(Orientation::Horizontal, 8);
    let port_heading = row_heading("Port");
    port_row.append(&port_heading);
    let port = gtk::Entry::builder()
        .input_purpose(gtk::InputPurpose::Digits)
        .max_length(5)
        .width_chars(6)
        .max_width_chars(6)
        .tooltip_text("1024 to 65535; press Enter to restart the server on it")
        .build();
    port.update_property(&[gtk::accessible::Property::Label("Port")]);
    port_heading.set_mnemonic_widget(Some(&port));
    port_row.append(&port);
    let restarts = Label::new(Some("Restarts the server"));
    restarts.add_css_class("helper");
    restarts.set_xalign(0.0);
    port_row.append(&restarts);
    running.append(&port_row);
    let port_feedback = feedback_line();
    running.append(&port_feedback);

    let viewers_heading = Label::new(Some("Viewers (0)"));
    viewers_heading.add_css_class("section-label");
    viewers_heading.set_xalign(0.0);
    running.append(&viewers_heading);
    let viewers = gtk::ListBox::new();
    viewers.set_selection_mode(gtk::SelectionMode::None);
    viewers.add_css_class("folder-list");
    viewers.add_css_class("virtual-deck-viewers");
    viewers.update_property(&[gtk::accessible::Property::Label("Viewers")]);
    let viewers_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(VIEWER_ROWS * VIEWER_ROW_HEIGHT + 2)
        .child(&viewers)
        .build();
    viewers_scroll.set_visible(false);
    running.append(&viewers_scroll);
    let no_viewers = Label::new(Some(NO_VIEWERS));
    no_viewers.add_css_class("helper");
    no_viewers.set_xalign(0.0);
    running.append(&no_viewers);
    running.set_visible(false);
    content.append(&running);
    // The details' padding shows only around something: off, the menu ends at the switch.
    let sync = {
        let parts = [
            feedback.clone().upcast::<gtk::Widget>().downgrade(),
            first_time.clone().upcast::<gtk::Widget>().downgrade(),
            running.clone().upcast::<gtk::Widget>().downgrade(),
        ];
        let details = details.downgrade();
        Rc::new(move || {
            if let Some(details) = details.upgrade() {
                let shown = parts
                    .iter()
                    .filter_map(|part| part.upgrade())
                    .any(|part| part.get_visible());
                details.set_visible(shown);
            }
        })
    };
    sync();
    for part in [
        feedback.upcast_ref::<gtk::Widget>(),
        first_time.upcast_ref(),
        running.upcast_ref(),
    ] {
        let sync = sync.clone();
        part.connect_visible_notify(move |_| sync());
    }

    Widgets {
        section,
        switch,
        feedback,
        first_time,
        first_warning,
        first_form,
        running,
        qr,
        address,
        copy,
        open,
        change,
        change_form,
        port,
        port_feedback,
        viewers_heading,
        viewers,
        viewers_scroll,
        no_viewers,
    }
}

/// Where the virtual deck is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Off,
    Starting,
    On,
    /// On, moving to another port.
    Restarting,
    Stopping,
}

/// A server that started on a worker, with the deck it serves.
struct Started {
    server: VirtualDeckServer,
    deck: VirtualDeck,
    link: VirtualDeckLink,
    inputs: mpsc::Receiver<DeckInput>,
    verifier: Verifier,
}

/// Start the server on `config` for a new virtual deck. Runs on a worker.
fn start_server(config: ServerConfig, verifier: Verifier) -> Result<Started, String> {
    let (deck, link) = virtual_deck_pair();
    let (sender, inputs) = mpsc::channel();
    let viewer_link = link.clone();
    let snapshot: Snapshot = Box::new(move || {
        viewer_link
            .snapshot()
            .into_iter()
            .map(glue::deck_frame)
            .collect()
    });
    let server = VirtualDeckServer::start(config, verifier.clone(), sender, snapshot)?;
    Ok(Started {
        server,
        deck,
        link,
        inputs,
        verifier,
    })
}

/// The server, its glue and what the menu shows about them.
struct Running {
    server: Arc<VirtualDeckServer>,
    glue: glue::Glue,
    url: String,
    /// The password verifier the server checks, kept for a port change.
    verifier: Verifier,
    /// Tells this server's events from an earlier one's.
    generation: u64,
}

/// Stop the glue (joined), then the server (every thread joined). Blocks.
fn stop_running(running: Running) {
    let Running { server, glue, .. } = running;
    glue.stop();
    match Arc::try_unwrap(server) {
        Ok(server) => server.stop(),
        Err(shared) => drop(shared),
    }
}

/// What a worker hands back to GTK.
enum Outcome {
    Started(Result<Started, String>),
    /// The server for a new port, which replaces the running one.
    Restarted(u16, Result<Started, String>),
    /// A server stopped; `last` when the deck is now off.
    Stopped {
        last: bool,
    },
    FirstPassword(Result<Verifier, String>),
    NewPassword(Result<Verifier, String>),
}

struct Worker {
    handle: thread::JoinHandle<()>,
    outcome: async_channel::Receiver<Outcome>,
}

struct State {
    env: Environment,
    widgets: Widgets,
    /// The header's "Virtual deck on · N viewers".
    badge: Label,
    toast: Option<ui::Toast>,
    phase: Cell<Phase>,
    running: RefCell<Option<Running>>,
    generation: Cell<u64>,
    /// Set while the app moves the switch itself.
    syncing: Cell<bool>,
    workers: RefCell<Vec<Worker>>,
    confirmation: RefCell<Option<gtk::MessageDialog>>,
    closed: Cell<bool>,
    /// Holds server starts on their workers while a test acts.
    #[cfg(test)]
    hold: RefCell<Option<hold::Gate>>,
}

impl State {
    /// Stop everything and wait for it: the server, the glue and every worker.
    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        if let Some(dialog) = self.confirmation.borrow_mut().take() {
            dialog.destroy();
        }
        if let Some(running) = self.running.borrow_mut().take() {
            self.env.hub.detach();
            set_served_at(None);
            stop_running(running);
        }
        for worker in self.workers.borrow_mut().drain(..) {
            if worker.handle.join().is_err() {
                eprintln!("A virtual deck worker panicked");
            }
            // A result nobody will read: a server that just started stops as it drops.
            while let Ok(outcome) = worker.outcome.try_recv() {
                drop(outcome);
            }
        }
        self.phase.set(Phase::Off);
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.close();
    }
}

/// The virtual deck section of the header menu and what it runs.
#[derive(Clone)]
pub(crate) struct VirtualDeckPanel(Rc<State>);

impl VirtualDeckPanel {
    /// The section, off; `badge` is the header label it keeps up to date.
    pub(crate) fn new(env: Environment, badge: Label, toast: Option<ui::Toast>) -> Self {
        badge.set_visible(false);
        let panel = Self(Rc::new(State {
            env,
            widgets: build_widgets(),
            badge,
            toast,
            phase: Cell::new(Phase::Off),
            running: RefCell::default(),
            generation: Cell::new(0),
            syncing: Cell::new(false),
            workers: RefCell::default(),
            confirmation: RefCell::default(),
            closed: Cell::new(false),
            #[cfg(test)]
            hold: RefCell::default(),
        }));
        panel.connect();
        panel
    }

    /// What the menu shows below its separator.
    pub(crate) fn section(&self) -> &GtkBox {
        &self.0.widgets.section
    }

    /// Stop the server, detach the deck and join every thread (app quit).
    pub(crate) fn shutdown(&self) {
        self.0.close();
        self.0.badge.set_visible(false);
    }

    fn widgets(&self) -> &Widgets {
        &self.0.widgets
    }

    fn phase(&self) -> Phase {
        self.0.phase.get()
    }

    /// Run `action` on the panel from a widget callback, if it still exists.
    fn on(&self, action: impl Fn(&Self) + 'static) -> impl Fn() + 'static {
        let weak = Rc::downgrade(&self.0);
        move || {
            if let Some(state) = weak.upgrade() {
                action(&Self(state));
            }
        }
    }

    fn connect(&self) {
        let w = self.widgets();
        let weak = Rc::downgrade(&self.0);
        w.switch.connect_active_notify(move |switch| {
            let Some(state) = weak.upgrade() else { return };
            if state.syncing.get() {
                return;
            }
            let panel = Self(state);
            if switch.is_active() {
                panel.turn_on();
            } else {
                panel.turn_off();
            }
        });
        let submit = self.on(Self::save_first_password);
        w.first_form.submit.connect_clicked(move |_| submit());
        let cancel = self.on(|panel| {
            panel.widgets().first_form.clear();
            panel.widgets().first_time.set_visible(false);
        });
        w.first_form.cancel.connect_clicked(move |_| cancel());
        let copy = self.on(Self::copy_address);
        w.copy.connect_clicked(move |_| copy());
        let open = self.on(|panel| {
            if let Some(url) = panel.url() {
                if let Err(error) = (panel.0.env.open)(&url) {
                    panel.report(&error);
                }
            }
        });
        w.open.connect_clicked(move |_| open());
        let change = self.on(|panel| {
            if panel.phase() == Phase::On {
                panel.widgets().change_form.open();
            }
        });
        w.change.connect_clicked(move |_| change());
        let submit = self.on(Self::submit_change);
        w.change_form.submit.connect_clicked(move |_| submit());
        let cancel = self.on(|panel| {
            panel.widgets().change_form.clear();
            panel.widgets().change_form.root.set_visible(false);
        });
        w.change_form.cancel.connect_clicked(move |_| cancel());
        let apply = self.on(Self::apply_port);
        w.port.connect_activate(move |_| apply());
    }

    /// Move the switch without treating it as the user's request.
    fn set_switch(&self, active: bool) {
        self.0.syncing.set(true);
        self.widgets().switch.set_active(active);
        self.0.syncing.set(false);
    }

    fn set_phase(&self, phase: Phase) {
        self.0.phase.set(phase);
        let w = self.widgets();
        let on = matches!(phase, Phase::On | Phase::Restarting);
        self.set_switch(matches!(
            phase,
            Phase::Starting | Phase::On | Phase::Restarting
        ));
        w.switch
            .set_sensitive(matches!(phase, Phase::Off | Phase::On));
        w.port.set_sensitive(phase == Phase::On);
        // The password changes only while on, never for a server still starting.
        w.change.set_sensitive(phase == Phase::On);
        w.running.set_visible(on);
        self.0.badge.set_visible(on);
        if on {
            self.sync_confirmation();
        } else {
            w.change_form.clear();
            w.change_form.root.set_visible(false);
            w.port_feedback.set_visible(false);
            self.close_confirmation();
        }
    }

    /// The confirmation can be accepted only while on; meanwhile it says why.
    fn sync_confirmation(&self) {
        let confirmation = self.0.confirmation.borrow().clone();
        if let Some(dialog) = confirmation {
            let ready = self.phase() == Phase::On;
            dialog.set_response_sensitive(gtk::ResponseType::Accept, ready);
            dialog.set_secondary_text(Some(if ready { CHANGE_WARNING } else { CONFIRM_WAIT }));
        }
    }

    fn close_confirmation(&self) {
        let confirmation = self.0.confirmation.borrow_mut().take();
        if let Some(dialog) = confirmation {
            dialog.destroy();
        }
    }

    fn report(&self, message: &str) {
        let label = &self.widgets().feedback;
        label.set_text(message);
        label.set_visible(true);
        ui::feedback(label, true);
    }

    fn clear_feedback(&self) {
        let label = &self.widgets().feedback;
        ui::clear_feedback(label);
        label.set_visible(false);
    }

    fn port_error(&self, message: &str) {
        let label = &self.widgets().port_feedback;
        label.set_text(message);
        label.set_visible(true);
        ui::feedback(label, true);
    }

    fn url(&self) -> Option<String> {
        self.0
            .running
            .borrow()
            .as_ref()
            .map(|running| running.url.clone())
    }

    /// The settings, unless the file on disk is unusable (it is then kept).
    fn open_store(&self) -> Result<SettingsStore, String> {
        let dir = self.0.env.config_dir.clone()?;
        let store = SettingsStore::open(&dir);
        match store.error() {
            Some(error) => Err(error.to_string()),
            None => Ok(store),
        }
    }

    fn server_config(&self, port: u16) -> ServerConfig {
        ServerConfig {
            port,
            bind: self.0.env.bind,
        }
    }

    /// Run `work` on an owned worker thread; its outcome comes back to GTK.
    /// When no thread can start, `failed` gives the outcome instead.
    fn run(
        &self,
        name: &str,
        work: impl FnOnce() -> Outcome + Send + 'static,
        failed: impl FnOnce(String) -> Outcome,
    ) {
        let (sender, outcome) = async_channel::bounded(1);
        let spawned = thread::Builder::new().name(name.into()).spawn(move || {
            let _ = sender.send_blocking(work());
        });
        let handle = match spawned {
            Ok(handle) => handle,
            Err(error) => {
                self.finish(failed(format!(
                    "Could not start the virtual deck worker: {error}"
                )));
                return;
            }
        };
        self.0.workers.borrow_mut().push(Worker {
            handle,
            outcome: outcome.clone(),
        });
        let weak = Rc::downgrade(&self.0);
        glib::MainContext::default().spawn_local(async move {
            let Ok(result) = outcome.recv().await else {
                return;
            };
            match weak.upgrade() {
                Some(state) => Self(state).finish(result),
                // The panel is gone: a server that just started stops as it drops.
                None => drop(result),
            }
        });
    }

    /// What a server worker does before it starts its server: nothing in the
    /// app; a test may hold it there.
    fn hold(&self) -> impl FnOnce() + Send + 'static {
        #[cfg(test)]
        let gate = self.0.hold.borrow().clone();
        move || {
            #[cfg(test)]
            {
                if let Some(gate) = gate {
                    gate.pass();
                }
            }
        }
    }

    /// Join the workers that are done.
    fn reap(&self) {
        let finished: Vec<Worker> = {
            let mut workers = self.0.workers.borrow_mut();
            let (finished, busy) = workers
                .drain(..)
                .partition(|worker| worker.handle.is_finished());
            *workers = busy;
            finished
        };
        for worker in finished {
            if worker.handle.join().is_err() {
                eprintln!("A virtual deck worker panicked");
            }
        }
    }

    fn finish(&self, outcome: Outcome) {
        self.reap();
        if self.0.closed.get() {
            return;
        }
        match outcome {
            Outcome::Started(result) => self.started(result),
            Outcome::Restarted(port, result) => self.restarted(port, result),
            Outcome::Stopped { last } => {
                if last {
                    self.set_phase(Phase::Off);
                }
            }
            Outcome::FirstPassword(result) => self.first_password_derived(result),
            Outcome::NewPassword(result) => self.new_password_derived(result),
        }
    }

    /// The switch was turned on: start, or ask for a password first.
    fn turn_on(&self) {
        if self.phase() != Phase::Off {
            return;
        }
        self.clear_feedback();
        let store = match self.open_store() {
            Ok(store) => store,
            Err(error) => {
                self.set_switch(false);
                self.report(&error);
                return;
            }
        };
        match store.settings().verifier.clone() {
            Some(verifier) => self.start(store.settings().port, verifier),
            None => {
                self.set_switch(false);
                let w = self.widgets();
                w.first_warning.set_text(if store.has_old_password() {
                    OLD_PASSWORD
                } else {
                    NEEDS_PASSWORD
                });
                w.first_time.set_visible(true);
                w.first_form.open();
            }
        }
    }

    fn start(&self, port: u16, verifier: Verifier) {
        let config = self.server_config(port);
        self.clear_feedback();
        self.set_phase(Phase::Starting);
        let hold = self.hold();
        self.run(
            "virtual-deck-start",
            move || {
                hold();
                Outcome::Started(start_server(config, verifier))
            },
            |error| Outcome::Started(Err(error)),
        );
    }

    fn started(&self, result: Result<Started, String>) {
        if self.phase() != Phase::Starting {
            return;
        }
        let installed = result.and_then(|mut started| {
            self.take_stored_password(&mut started);
            self.launch(started)
        });
        match installed {
            Ok(running) => {
                // No browser window opens by itself: the page opens only from
                // the panel's Open button (or the QR code on a phone).
                self.0.running.replace(Some(running));
                self.set_phase(Phase::On);
                self.show_running();
            }
            Err(error) => {
                self.set_phase(Phase::Off);
                self.report(&error);
            }
        }
    }

    /// The password may have changed while `started` was starting: before it
    /// is installed, the server takes the stored verifier (which disconnects
    /// every viewer it may already have), so it never accepts an older
    /// password. An unusable settings file keeps the verifier it started
    /// with: nothing can be saved over that file either.
    fn take_stored_password(&self, started: &mut Started) {
        let stored = self
            .open_store()
            .ok()
            .and_then(|store| store.settings().verifier.clone());
        if let Some(stored) = stored {
            if stored != started.verifier {
                started.server.set_password(stored.clone());
                started.verifier = stored;
            }
        }
    }

    /// Wire a started server to the hub: glue thread, then the deck. On
    /// failure the server stops as it drops.
    fn launch(&self, started: Started) -> Result<Running, String> {
        let Started {
            server,
            deck,
            link,
            inputs,
            verifier,
        } = started;
        let url = server
            .url_candidates()
            .into_iter()
            .next()
            .ok_or("The virtual deck has no address to offer.")?;
        let server = Arc::new(server);
        let generation = self.0.generation.get() + 1;
        self.0.generation.set(generation);
        let (events, received) = async_channel::unbounded();
        let glue = glue::Glue::start(link, inputs, server.clone(), server.events(), events)?;
        // The header names the address as soon as the hub reports the deck.
        let previous = served_at();
        set_served_at(Some(url.clone()));
        if let Err(error) = self.0.env.hub.attach(deck) {
            set_served_at(previous);
            glue.stop();
            return Err(error);
        }
        self.listen(received, generation);
        Ok(Running {
            server,
            glue,
            url,
            verifier,
            generation,
        })
    }

    /// Hear the server's events on GTK while this server runs.
    fn listen(&self, received: async_channel::Receiver<ServerEvent>, generation: u64) {
        let weak = Rc::downgrade(&self.0);
        glib::MainContext::default().spawn_local(async move {
            while let Ok(event) = received.recv().await {
                let Some(state) = weak.upgrade() else { break };
                Self(state).server_event(generation, event);
            }
        });
    }

    fn server_event(&self, generation: u64, event: ServerEvent) {
        let current = self
            .0
            .running
            .borrow()
            .as_ref()
            .is_some_and(|running| running.generation == generation);
        if !current {
            return;
        }
        match event {
            ServerEvent::ViewersChanged => self.refresh_viewers(),
            ServerEvent::Error(error) => {
                let message = format!("Virtual deck: {error}");
                match &self.0.toast {
                    Some(toast) => toast.show(&message, true),
                    None => self.report(&message),
                }
            }
        }
    }

    /// Fill the running details from the server.
    fn show_running(&self) {
        let w = self.widgets();
        {
            let running = self.0.running.borrow();
            let Some(running) = running.as_ref() else {
                return;
            };
            w.address.set_text(&running.url);
            let candidates = running.server.url_candidates();
            let tip = match candidates.as_slice() {
                [_] | [] => format!("The page address: {}", running.url),
                more => format!("The page answers at {}", more.join(", ")),
            };
            w.address.set_tooltip_text(Some(&tip));
            if let Err(error) = w.qr.show(&running.url) {
                self.report(&error);
            }
            w.port.set_text(&running.server.local_port().to_string());
            self.0
                .badge
                .set_tooltip_text(Some(&format!("Served at {}", running.url)));
        }
        w.first_time.set_visible(false);
        self.refresh_viewers();
    }

    /// List the viewers, count them in the heading and the header.
    fn refresh_viewers(&self) {
        let viewers = match self.0.running.borrow().as_ref() {
            Some(running) => running.server.viewers(),
            None => return,
        };
        let w = self.widgets();
        while let Some(row) = w.viewers.first_child() {
            w.viewers.remove(&row);
        }
        for viewer in &viewers {
            w.viewers.append(&self.viewer_row(viewer));
        }
        w.viewers_heading
            .set_text(&format!("Viewers ({})", viewers.len()));
        w.viewers_scroll.set_visible(!viewers.is_empty());
        w.no_viewers.set_visible(viewers.is_empty());
        self.0.badge.set_text(&badge_text(viewers.len()));
    }

    fn viewer_row(&self, viewer: &Viewer) -> gtk::ListBoxRow {
        let content = GtkBox::new(Orientation::Horizontal, 10);
        let icon = gtk::Image::from_icon_name(viewer_icon(&viewer.browser));
        icon.set_pixel_size(16);
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);
        content.append(&icon);
        let who = GtkBox::new(Orientation::Vertical, 0);
        who.set_hexpand(true);
        let browser = Label::new(Some(&viewer.browser));
        browser.add_css_class("folder-name");
        browser.set_xalign(0.0);
        browser.set_ellipsize(gtk::pango::EllipsizeMode::End);
        who.append(&browser);
        let detail = Label::new(Some(&viewer_detail(viewer)));
        detail.add_css_class("helper");
        detail.set_xalign(0.0);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        who.append(&detail);
        content.append(&who);
        let disconnect = Button::with_label("Disconnect");
        disconnect.set_valign(gtk::Align::Center);
        let name = format!("Disconnect {} at {}", viewer.browser, viewer.address);
        disconnect.set_tooltip_text(Some(&name));
        disconnect.update_property(&[gtk::accessible::Property::Label(&name)]);
        let id = viewer.id;
        let disconnect_viewer = self.on(move |panel| {
            if let Some(running) = panel.0.running.borrow().as_ref() {
                running.server.disconnect(id);
            }
        });
        disconnect.connect_clicked(move |_| disconnect_viewer());
        content.append(&disconnect);
        let row = gtk::ListBoxRow::builder()
            .activatable(false)
            .selectable(false)
            .child(&content)
            .build();
        row.add_css_class("folder-row");
        row
    }

    fn copy_address(&self) {
        let Some(url) = self.url() else { return };
        self.widgets().copy.clipboard().set_text(&url);
        if let Some(toast) = &self.0.toast {
            toast.show("Address copied.", false);
        }
    }

    /// The switch was turned off: drop the deck now, stop the server on a worker.
    fn turn_off(&self) {
        if self.phase() != Phase::On {
            // Starting, restarting and stopping finish first (the switch is insensitive).
            self.set_phase(self.phase());
            return;
        }
        let Some(running) = self.0.running.borrow_mut().take() else {
            self.set_phase(Phase::Off);
            return;
        };
        self.0.env.hub.detach();
        set_served_at(None);
        self.set_phase(Phase::Stopping);
        self.run(
            "virtual-deck-stop",
            move || {
                stop_running(running);
                Outcome::Stopped { last: true }
            },
            |_| Outcome::Stopped { last: true },
        );
    }

    fn save_first_password(&self) {
        let form = &self.widgets().first_form;
        let (new, repeat) = form.texts();
        form.set_busy(true);
        self.run(
            "virtual-deck-password",
            move || Outcome::FirstPassword(checked_verifier(&new, &repeat)),
            |error| Outcome::FirstPassword(Err(error)),
        );
    }

    fn first_password_derived(&self, result: Result<Verifier, String>) {
        let form = &self.widgets().first_form;
        form.set_busy(false);
        let verifier = match result {
            Ok(verifier) => verifier,
            Err(error) => return form.show_error(&error),
        };
        if self.phase() != Phase::Off {
            return;
        }
        let mut store = match self.open_store() {
            Ok(store) => store,
            Err(error) => return form.show_error(&error),
        };
        if let Err(error) = store.set_verifier(verifier.clone()) {
            return form.show_error(&error);
        }
        form.clear();
        self.widgets().first_time.set_visible(false);
        self.start(store.settings().port, verifier);
    }

    fn submit_change(&self) {
        let form = &self.widgets().change_form;
        if self.phase() != Phase::On {
            return form.show_error(CHANGE_WAIT);
        }
        let (new, repeat) = form.texts();
        form.set_busy(true);
        self.run(
            "virtual-deck-password",
            move || Outcome::NewPassword(checked_verifier(&new, &repeat)),
            |error| Outcome::NewPassword(Err(error)),
        );
    }

    fn new_password_derived(&self, result: Result<Verifier, String>) {
        let form = &self.widgets().change_form;
        form.set_busy(false);
        // Turned off meanwhile: the form was closed and there is nothing to confirm.
        if !matches!(self.phase(), Phase::On | Phase::Restarting) {
            return;
        }
        match result {
            Ok(verifier) => self.confirm_change(verifier),
            Err(error) => form.show_error(&error),
        }
    }

    /// "Changing the password disconnects every viewer." Cancel is the default.
    /// While the server restarts it waits: "Change password" acts only while on.
    fn confirm_change(&self, verifier: Verifier) {
        let Some(window) = self.widgets().section.root().and_downcast::<gtk::Window>() else {
            return;
        };
        self.close_confirmation();
        let dialog = gtk::MessageDialog::builder()
            .transient_for(&window)
            .modal(true)
            .destroy_with_parent(true)
            .message_type(gtk::MessageType::Question)
            .text(CHANGE_TITLE)
            .secondary_text(CHANGE_WARNING)
            .build();
        ui::style_confirm_dialog(&dialog);
        ui::dismiss_button(dialog.upcast_ref(), "Cancel", gtk::ResponseType::Cancel);
        dialog
            .add_button("Change password", gtk::ResponseType::Accept)
            .add_css_class("destructive-action");
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let weak = Rc::downgrade(&self.0);
        dialog.connect_response(move |dialog, response| {
            if let Some(state) = weak.upgrade() {
                let panel = Self(state);
                let current = panel.0.confirmation.borrow().as_ref() == Some(dialog);
                if current {
                    if response == gtk::ResponseType::Accept && panel.phase() != Phase::On {
                        // Not on yet: the confirmation keeps asking and says why.
                        panel.sync_confirmation();
                        return;
                    }
                    panel.0.confirmation.borrow_mut().take();
                    if response == gtk::ResponseType::Accept {
                        panel.apply_password(verifier.clone());
                    }
                }
            }
            dialog.destroy();
        });
        self.0.confirmation.replace(Some(dialog.clone()));
        self.sync_confirmation();
        dialog.present();
    }

    /// Remember the new verifier, then give it to the server, which
    /// disconnects every viewer. Only while on: a server still starting
    /// would miss it.
    fn apply_password(&self, verifier: Verifier) {
        let form = &self.widgets().change_form;
        if self.phase() != Phase::On {
            return form.show_error(CHANGE_WAIT);
        }
        let mut store = match self.open_store() {
            Ok(store) => store,
            Err(error) => return form.show_error(&error),
        };
        if let Err(error) = store.set_verifier(verifier.clone()) {
            return form.show_error(&error);
        }
        if let Some(running) = self.0.running.borrow_mut().as_mut() {
            running.server.set_password(verifier.clone());
            running.verifier = verifier;
        }
        form.clear();
        form.root.set_visible(false);
        if let Some(toast) = &self.0.toast {
            toast.show("Password changed. Every viewer was disconnected.", false);
        }
    }

    /// Enter in the port field: move the server there, or say why not.
    fn apply_port(&self) {
        let w = self.widgets();
        let port = match parse_port(&w.port.text()) {
            Ok(port) => port,
            Err(error) => return self.port_error(&error),
        };
        if self.phase() != Phase::On {
            return;
        }
        let (current, verifier) = match self.0.running.borrow().as_ref() {
            Some(running) => (running.server.local_port(), running.verifier.clone()),
            None => return,
        };
        ui::clear_feedback(&w.port_feedback);
        w.port_feedback.set_visible(false);
        if port == current {
            return;
        }
        let config = self.server_config(port);
        self.set_phase(Phase::Restarting);
        let hold = self.hold();
        self.run(
            "virtual-deck-restart",
            move || {
                hold();
                Outcome::Restarted(port, start_server(config, verifier))
            },
            move |error| Outcome::Restarted(port, Err(error)),
        );
    }

    /// The server for the new port started (or not). The old one keeps
    /// running until the new one serves the deck.
    fn restarted(&self, port: u16, result: Result<Started, String>) {
        if self.phase() != Phase::Restarting {
            return;
        }
        self.set_phase(Phase::On);
        let mut started = match result {
            Ok(started) => started,
            Err(error) => return self.port_error(&error),
        };
        self.take_stored_password(&mut started);
        let old = self.0.running.borrow_mut().take();
        match self.launch(started) {
            Ok(running) => {
                self.0.running.replace(Some(running));
                if let Some(old) = old {
                    self.run(
                        "virtual-deck-stop",
                        move || {
                            stop_running(old);
                            Outcome::Stopped { last: false }
                        },
                        |_| Outcome::Stopped { last: false },
                    );
                }
                self.show_running();
                // The menu keeps showing the port the server runs on, and says so.
                if let Err(error) = self.open_store().and_then(|mut store| store.set_port(port)) {
                    self.port_error(&port_not_remembered(port, &error));
                }
            }
            Err(error) => {
                self.0.running.replace(old);
                self.port_error(&error);
            }
        }
    }
}

#[cfg(test)]
impl VirtualDeckPanel {
    pub(crate) fn test_widgets(&self) -> &Widgets {
        self.widgets()
    }

    pub(crate) fn test_phase(&self) -> Phase {
        self.phase()
    }

    pub(crate) fn test_url(&self) -> Option<String> {
        self.url()
    }

    pub(crate) fn test_port(&self) -> Option<u16> {
        self.0
            .running
            .borrow()
            .as_ref()
            .map(|running| running.server.local_port())
    }

    pub(crate) fn test_confirmation(&self) -> Option<gtk::MessageDialog> {
        self.0.confirmation.borrow().clone()
    }

    /// Hold every server start or restart begun from now on before it starts
    /// its server, until the returned hold is released or dropped.
    pub(crate) fn test_hold_servers(&self) -> hold::ServerHold {
        let gate = hold::Gate::held();
        self.0.hold.replace(Some(gate.clone()));
        hold::ServerHold::new(gate)
    }

    /// Whether every worker thread has been joined.
    pub(crate) fn test_idle(&self) -> bool {
        self.reap();
        self.0.workers.borrow().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewer(browser: &str) -> Viewer {
        Viewer {
            id: 1,
            address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 73)),
            browser: browser.into(),
            since: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn the_header_counts_viewers_in_words() {
        assert_eq!(badge_text(0), "Virtual deck on · 0 viewers");
        assert_eq!(badge_text(1), "Virtual deck on · 1 viewer");
        assert_eq!(badge_text(2), "Virtual deck on · 2 viewers");
    }

    #[test]
    fn a_viewer_row_names_its_address_and_since_when() {
        let detail = viewer_detail(&viewer("Android · Chrome 131"));
        assert!(detail.starts_with("192.168.1.73 · since "), "{detail}");
        let time = detail.rsplit(' ').next().unwrap();
        assert_eq!(time.len(), 5, "{detail}");
        assert_eq!(&time[2..3], ":", "{detail}");
    }

    #[test]
    fn phones_and_tablets_get_a_phone_and_the_rest_a_computer() {
        for browser in [
            "Android · Chrome 131",
            "iOS · Safari 17",
            "iPadOS · Safari 17",
        ] {
            assert_eq!(viewer_icon(browser), "phone-symbolic", "{browser}");
        }
        for browser in [
            "macOS · Safari 18",
            "Linux · Firefox 133",
            "Unknown browser",
        ] {
            assert_eq!(viewer_icon(browser), "computer-symbolic", "{browser}");
        }
    }

    #[test]
    fn a_new_password_needs_8_characters_first_then_a_matching_repeat() {
        assert_eq!(
            checked_verifier("short", ""),
            Err("At least 8 characters.".to_string())
        );
        assert_eq!(
            checked_verifier("long enough", "long enougj"),
            Err(MISMATCH.to_string())
        );
        let verifier = checked_verifier("long enough", "long enough").unwrap();
        assert_eq!(verifier.iterations(), sdeck_plus::virtual_deck::ITERATIONS);
        assert!(!format!("{verifier:?}").contains("long enough"));
    }
}
