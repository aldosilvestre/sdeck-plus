use sdeck_plus::{
    binding_editor::{
        action_from_fields, control_summary, events_for_control, existing_action,
        format_application_args, trigger_from_selection, ActionKind, ControlKind, EventKind,
    },
    binding_icons::BindingIcon,
    layers::{Action, LayerStore, Trigger},
    local_artwork::{ArtworkStore, ArtworkTarget},
    protocol,
};

use gtk::{
    glib, prelude::*, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, Entry,
    FileChooserAction, FileChooserNative, Label, Orientation, Picture, ResponseType, TextView,
};
use sdeck_plus::event_api::{self, ArtworkCommand, ConnectionState, MonitorEvent};
use std::thread;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

mod action_catalog;
mod app_icon;
mod autostart;
mod button_swap;
mod button_text_editor;
mod configuration_menu;
mod dial_presets;
mod feature_editor;
mod feature_runtime;
mod shortcut_reference;
mod timer_artwork;
mod tray;
mod tray_preference;
mod ui;
// Layout containers live beside ui.rs but are mounted here, because the
// ui_showcase example includes ui.rs by path.
#[path = "ui/binding_icon.rs"]
mod binding_icon;
#[path = "ui/clamp.rs"]
mod clamp;
#[path = "ui/scaled_bin.rs"]
mod scaled_bin;

const APP_ID: &str = "dev.streamdeck.EventMonitor";
const APP_NAME: &str = "sdeck-plus";
const DEFAULT_WIDTH: i32 = 1000;
const DEFAULT_HEIGHT: i32 = 700;
/// The hardware preview is always shown at its natural size, so it never
/// changes with the window or the configuration beside it.
const PREVIEW_MAX_SCALE: f64 = 1.0;
/// Width of each dial's assigned-action caption in the preview rail.
const DIAL_LEGEND_WIDTH: i32 = 86;
/// Action icon drawn on a preview knob face.
const DIAL_ICON_SIZE: i32 = 20;
/// Widest configuration shell (including its margins) before it centers.
const SHELL_MAX_WIDTH: i32 = 1272;

enum LayerCommand {
    Persist(LayerStore, Option<sdeck_plus::organization::Organization>),
    SaveBinding(LayerStore, Option<sdeck_plus::organization::Organization>),
    SaveProfile(LayerStore, Option<sdeck_plus::organization::Organization>),
    SwapKeys {
        store: LayerStore,
        artwork_root: std::path::PathBuf,
        layer: String,
        first: u8,
        second: u8,
    },
    Execute {
        binding_id: String,
        action: Action,
    },
    ObsSave(sdeck_plus::obs::ObsConfig),
    ObsProbe(sdeck_plus::obs::ObsConfig),
    ExportConfiguration {
        store: LayerStore,
        organization: Option<sdeck_plus::organization::Organization>,
        destination: std::path::PathBuf,
    },
    ImportConfiguration {
        root: std::path::PathBuf,
        source: std::path::PathBuf,
    },
    Shutdown,
}

fn checked_layer_store(store: LayerStore) -> std::io::Result<LayerStore> {
    if store.diagnostics().is_empty() {
        Ok(store)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            store.diagnostics().join("; "),
        ))
    }
}

#[cfg(test)]
fn execute_layer_action(
    executor: &mut sdeck_plus::layers::ActionExecutor<
        impl sdeck_plus::layers::ProcessRunner,
        impl sdeck_plus::layers::KeyboardSender,
    >,
    action: &Action,
    output: &mpsc::Sender<MonitorEvent>,
) {
    execute_layer_action_for(executor, "", action, output);
}

fn execute_layer_action_for(
    executor: &mut sdeck_plus::layers::ActionExecutor<
        impl sdeck_plus::layers::ProcessRunner,
        impl sdeck_plus::layers::KeyboardSender,
    >,
    binding_id: &str,
    action: &Action,
    output: &mpsc::Sender<MonitorEvent>,
) {
    let result = match action {
        Action::Feature(sdeck_plus::features::FeatureAction::Obs(action)) => {
            sdeck_plus::obs::ObsConfig::load()
                .and_then(|config| sdeck_plus::obs::Client::new(config).execute(action))
        }
        _ => executor.execute_for(binding_id, action),
    };
    let succeeded = result.is_ok();
    let event = match result {
        Ok(()) => MonitorEvent::AppEvent {
            name: "Action dispatched.".into(),
        },
        Err(error) => MonitorEvent::LayerError(format!("Could not execute layer action: {error}")),
    };
    if let Err(error) = output.send(event) {
        eprintln!("Action result delivery failed: {error}");
    }
    if succeeded {
        if let Err(error) = output.send(MonitorEvent::ActionDispatched) {
            eprintln!("Action completion delivery failed: {error}");
        }
    }
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.add_main_option(
        "background",
        glib::Char::from(b'b'),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Start in the system tray without opening configuration",
        None,
    );
    let background = Rc::new(Cell::new(false));
    let option = background.clone();
    app.connect_handle_local_options(move |_, options| {
        option.set(options.contains("background"));
        -1
    });
    let tray_error = Rc::new(RefCell::new(None));
    let startup_error = tray_error.clone();
    app.connect_startup(move |app| {
        if let Err(error) = app_icon::register().and_then(|()| tray::install(app)) {
            eprintln!("Could not start system tray: {error}");
            *startup_error.borrow_mut() = Some(error.to_string());
        }
    });
    let first_activation = Cell::new(true);
    app.connect_activate(move |app| {
        // Only the first activation honors the flag and the saved preference;
        // launching again while running opens the window.
        let hidden = background.replace(false) | tray_preference::start_in_tray();
        let hidden = first_activation.replace(false) && hidden;
        let visible = !hidden || tray_error.borrow().is_some();
        build_ui_with_visibility(app, visible);
        if let Some(error) = tray_error.borrow_mut().take() {
            if let Some(window) = configuration_window(app) {
                show_artwork_error(&window, &format!("System tray unavailable: {error}. Enable a StatusNotifier tray in your desktop and restart. Closing this window will exit the application."));
            }
        }
    });
    app.run()
}

#[derive(Default)]
struct DeviceLifecycle {
    hid: Option<event_api::HidController>,
    polling: Option<glib::SourceId>,
    layer_sender: Option<mpsc::Sender<LayerCommand>>,
    layer_worker: Option<thread::JoinHandle<()>>,
    layer_stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    focus: Option<sdeck_plus::app_focus::FocusObserver>,
}

impl DeviceLifecycle {
    fn shutdown(&mut self) {
        if let Some(mut focus) = self.focus.take() {
            focus.stop();
        }
        self.layer_stop
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(sender) = self.layer_sender.take() {
            if let Err(error) = sender.send(LayerCommand::Shutdown) {
                eprintln!("Layer shutdown delivery failed: {error}");
            }
        }
        if let Some(hid) = &self.hid {
            hid.request_stop();
        }
        if let Some(source) = self.polling.take() {
            source.remove();
        }
        if let Some(mut hid) = self.hid.take() {
            hid.shutdown();
        }
        if let Some(worker) = self.layer_worker.take() {
            if worker.join().is_err() {
                eprintln!("Layer worker panicked during shutdown");
            }
        }
    }
}

fn install_device_cleanup(app: &Application, lifecycle: Rc<RefCell<DeviceLifecycle>>) {
    app.connect_shutdown(move |_| lifecycle.borrow_mut().shutdown());
}

fn configuration_window(app: &Application) -> Option<ApplicationWindow> {
    app.windows()
        .into_iter()
        .find(|window| window.widget_name() == "configuration")
        .and_then(|window| window.downcast::<ApplicationWindow>().ok())
}

fn build_ui(app: &Application) {
    build_ui_with_visibility(app, true);
}

fn keep_running_on_close(app: &Application) -> bool {
    app.action_state("keep-running")
        .and_then(|state| state.get::<bool>())
        .unwrap_or(false)
}

fn configure_window_close(window: &ApplicationWindow, app: &Application) {
    window.set_hide_on_close(keep_running_on_close(app));
    window.connect_close_request(glib::clone!(
        #[weak]
        app,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |window| {
            if keep_running_on_close(&app) {
                window.set_visible(false);
            } else {
                app.quit();
            }
            glib::Propagation::Stop
        }
    ));
}

fn build_ui_with_visibility(app: &Application, visible: bool) {
    if let Some(window) = configuration_window(app) {
        if visible {
            window.present();
        }
        return;
    }
    let window = ApplicationWindow::builder()
        .application(app)
        .name("configuration")
        .title(APP_NAME)
        .default_width(DEFAULT_WIDTH)
        .default_height(DEFAULT_HEIGHT)
        .build();
    configure_window_close(&window, app);
    // Validate before starting either worker: failure cannot leave an orphan.
    let layers = match LayerStore::open().and_then(checked_layer_store) {
        Ok(store) => Rc::new(RefCell::new(store)),
        Err(error) => {
            window.set_child(Some(&Label::new(Some(
                "Configuration unavailable. Writes are disabled.",
            ))));
            window.present();
            show_artwork_error(&window, &format!("Could not open layer configuration: {error}. Check permissions and repair layers.conf in the app data directory, then restart. No fallback will be saved."));
            return;
        }
    };
    // Every launch starts on Base (its first page), whatever was saved last.
    layers
        .borrow_mut()
        .select(sdeck_plus::layers::DEFAULT_LAYER);
    let (sender, receiver) = mpsc::channel();
    let app_event_sender = sender.clone();
    let hid = event_api::start_hid_monitor(sender);
    let artwork_sender = hid.artwork_sender();
    let device_sender = hid.control_sender();
    let lifecycle = Rc::new(RefCell::new(DeviceLifecycle {
        hid: Some(hid),
        ..DeviceLifecycle::default()
    }));
    install_device_cleanup(app, lifecycle.clone());
    let (layer_sender, layer_receiver) = mpsc::channel::<LayerCommand>();
    let layer_result_sender = app_event_sender.clone();
    let layer_stop = lifecycle.borrow().layer_stop.clone();
    let worker = thread::Builder::new()
        .name("layer-worker".into())
        .spawn(move || {
            feature_runtime::run_worker(layer_receiver, layer_result_sender, layer_stop);
        });
    match worker {
        Ok(worker) => {
            lifecycle.borrow_mut().layer_worker = Some(worker);
            lifecycle.borrow_mut().layer_sender = Some(layer_sender.clone());
        }
        Err(error) => {
            lifecycle.borrow_mut().shutdown();
            window.set_child(Some(&Label::new(Some(&format!(
                "Could not start action worker: {error}"
            )))));
            window.present();
            return;
        }
    }

    let (store, artwork_open_error) = match ArtworkStore::open_for_layer(Some(layers.borrow().active())) {
        Ok(store) => (Some(store), None),
        Err(error) => (None, Some(format!("Could not open or migrate artwork: {error}. Artwork writes and layer removal are disabled. Check permissions and repair the artwork manifest/assets in the app data directory, then restart. Original files have not been replaced."))),
    };
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(store)),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output: artwork_sender.clone(),
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
        info: Rc::default(),
    };
    let organization =
        sdeck_plus::organization::Organization::open_at(layers.borrow().root(), &layers.borrow());
    if let (Ok(organization), Some(store)) = (&organization, artwork.store.borrow_mut().as_mut()) {
        if let Err(error) = layers
            .borrow_mut()
            .initialize_touchscreens(organization, store)
        {
            lifecycle.borrow_mut().shutdown();
            window.set_child(Some(&Label::new(Some(
                "Touchscreen configuration unavailable. Writes are disabled.",
            ))));
            window.present();
            show_artwork_error(&window, &format!("Could not initialize touchscreen pages: {error}. Original configurations have been retained; repair the app data directory and restart."));
            return;
        }
    }
    let widgets = build_widgets(
        window.clone(),
        layers.clone(),
        artwork.clone(),
        layer_sender.clone(),
    );
    widgets.feature_runtime.configure(
        organization,
        sdeck_plus::obs::ObsConfig::load(),
        device_sender,
        app_event_sender.clone(),
    );
    if !widgets.feature_runtime.organization_available() {
        for button in &widgets.organization_writes {
            button.set_sensitive(false);
        }
    }
    let focus_output = app_event_sender.clone();
    match sdeck_plus::app_focus::FocusObserver::start(move |result| {
        let _ = focus_output.send(MonitorEvent::ApplicationFocus(result));
    }) {
        Ok(observer) => lifecycle.borrow_mut().focus = Some(observer),
        Err(error) => widgets
            .feature_runtime
            .focus_changed(Err(error.to_string())),
    }
    install_style(&window);
    window.set_child(Some(&widgets.toast.wrap(&widgets.root)));
    if visible {
        window.present();
    }
    if let Some(error) = artwork_open_error {
        show_artwork_error(&window, &error);
    }
    lifecycle.borrow_mut().polling = Some(start_event_updates(
        widgets,
        receiver,
        layers,
        artwork,
        layer_sender,
        app_event_sender,
    ));
}

// Shared production construction: no HID, action workers, disk reads or presentation.
struct MonitorWidgets {
    root: gtk::ScrolledWindow,
    toast: ui::Toast,
    binding_status: Label,
    status: Label,
    connection_detail: Label,
    #[cfg(test)]
    layer_picker: gtk::DropDown,
    #[cfg(test)]
    binding_ui: BindingUi,
    #[cfg(test)]
    obs_menu: gtk::MenuButton,
    #[cfg(test)]
    help_menu: gtk::MenuButton,
    #[cfg(test)]
    management_menu: gtk::MenuButton,
    #[cfg(test)]
    artwork_section: gtk::Expander,
    #[cfg(test)]
    target_picker: gtk::DropDown,
    key_labels: Vec<Label>,
    encoder_labels: Vec<Label>,
    touch_status: Label,
    key_widgets: Vec<gtk::Widget>,
    encoder_widgets: Vec<gtk::Widget>,
    zone_widgets: Vec<gtk::Widget>,
    feature_runtime: feature_runtime::FeatureRuntime,
    organization_writes: Vec<Button>,
    // Keep the weakly referenced editor callbacks alive with the monitor widgets.
    _text_editor: Rc<button_text_editor::ButtonTextEditor>,
    _configuration_menu: configuration_menu::ConfigurationMenu,
}

fn build_widgets(
    window: ApplicationWindow,
    layers: Rc<RefCell<LayerStore>>,
    artwork: ArtworkUi,
    layer_sender: mpsc::Sender<LayerCommand>,
) -> MonitorWidgets {
    let updating_layers = Rc::new(Cell::new(false));
    let toast = ui::Toast::new();
    let conflicts = layers.borrow().conflicts().len();
    if conflicts > 0 {
        toast.show(
            &format!(
                "Configuration conflicts: {conflicts} button setting(s) were removed. Details are in {} in the configuration folder.",
                sdeck_plus::layers::CONFLICT_LOG
            ),
            true,
        );
    }
    let root = GtkBox::new(Orientation::Vertical, 16);
    // The complete desk fills the window up to SHELL_MAX_WIDTH (see the clamp
    // below), then centers; it still shrinks to narrow desktops.
    root.set_margin_top(16);
    root.set_margin_bottom(16);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.add_css_class("app-shell");
    ui::track_motion(&root);

    let header = GtkBox::new(Orientation::Horizontal, 12);
    let heading = GtkBox::new(Orientation::Vertical, 4);
    let title = Label::new(Some("SDECK-PLUS"));
    title.add_css_class("title");
    title.set_xalign(0.0);
    heading.append(&title);
    header.append(&heading);
    let status = Label::new(None);
    status.add_css_class("status");
    status.set_hexpand(true);
    status.set_halign(gtk::Align::End);
    header.append(&status);
    let help_body = GtkBox::new(Orientation::Vertical, 16);
    for (icon, title, explanation) in [
        ("input-gaming-symbolic", "1. Select a control", "Click a key, dial or the touch strip on the preview. Dials have separate press and rotation events."),
        ("preferences-system-symbolic", "2. Configure an action", "Drag an action from the right onto a key or a dial (a dial asks for clockwise, counterclockwise or press). Actions without parameters save on drop; otherwise complete the settings below and Save binding. You can also click a catalog action for the selected control, or use the Action selector for any control."),
        ("document-save-symbolic", "3. Save or test", "Save binding stores your configuration. Test action runs it immediately. Artwork is configured separately."),
        ("view-grid-symbolic", "Touch strip pages", "Touchscreen pages belong to the profile. Select the touch strip to add or choose a Touch page. Swiping left or right changes page unless you bind those swipes to something else."),
    ] {
        let step = GtkBox::new(Orientation::Vertical, 8);
        step.append(&ui::section_heading(icon, title));
        step.append(&ui::helper(explanation));
        help_body.append(&step);
    }
    let configuration_menu = configuration_menu::ConfigurationMenu::new(&window, &help_body);
    let help_menu = configuration_menu.button().clone();
    header.append(&help_menu);
    root.append(&header);

    let connection_detail = Label::new(None);
    show_connection_state(&status, &connection_detail, &ConnectionState::Searching);
    connection_detail.add_css_class("muted");
    connection_detail.set_xalign(0.0);
    connection_detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
    connection_detail.set_max_width_chars(70);
    heading.append(&connection_detail);

    let layer_card = card();
    layer_card.add_css_class("navigation-card");
    layer_card.set_spacing(8);
    let layer_row = GtkBox::new(Orientation::Horizontal, 8);
    let layer_model = gtk::StringList::new(&[]);
    for name in layers.borrow().names() {
        layer_model.append(&name);
    }
    let layer_picker = gtk::DropDown::new(Some(layer_model.clone()), gtk::Expression::NONE);
    bound_layer_picker(&layer_picker);
    layer_picker.set_hexpand(true);
    layer_picker.set_enable_search(true);
    let selected = layers
        .borrow()
        .names()
        .iter()
        .position(|n| n == layers.borrow().active())
        .unwrap_or(0);
    layer_picker.set_selected(selected as u32);
    let layer_name = Entry::builder()
        .placeholder_text("New profile name")
        .width_chars(16)
        .hexpand(true)
        .build();
    let create_layer = ui::button("list-add-symbolic", "Create profile");
    create_layer.add_css_class("suggested-action");
    let management = GtkBox::new(Orientation::Vertical, 16);
    let creation = GtkBox::new(Orientation::Vertical, 12);
    let name_row = labeled("Profile name", &layer_name);
    ui::field_help(&name_row, "New profile help",
        "Create a separate set of bindings and artwork. The new profile starts empty. Add pages and folders from the page bar after creating it.");
    creation.append(&name_row);
    creation.append(&create_layer);
    let creation_error = ui::helper("");
    creation_error.set_visible(false);
    creation.append(&creation_error);
    let error = creation_error.downgrade();
    let known = layers.clone();
    let create = create_layer.downgrade();
    layer_name.connect_changed(move |name| {
        let (Some(error), Some(create)) = (error.upgrade(), create.upgrade()) else {
            return;
        };
        let value = name.text();
        let value = value.trim();
        let duplicate = known.borrow().names().iter().any(|n| n == value);
        create.set_sensitive(!value.is_empty() && !duplicate);
        error.set_visible(duplicate);
        if duplicate {
            error.set_text(&format!("A profile named {value} already exists."));
            ui::feedback(&error, true);
        }
    });
    create_layer.set_sensitive(false);
    let control_model = gtk::StringList::new(&["Button", "Encoder", "Touchpad"]);
    let control_picker = gtk::DropDown::new(Some(control_model.clone()), gtk::Expression::NONE);
    let control_number_model = gtk::StringList::new(&["1", "2", "3", "4", "5", "6", "7", "8"]);
    let control_number_picker =
        gtk::DropDown::new(Some(control_number_model.clone()), gtk::Expression::NONE);
    let event_model = gtk::StringList::new(&["Press"]);
    let event_picker = gtk::DropDown::new(Some(event_model.clone()), gtk::Expression::NONE);
    let action_model = gtk::StringList::new(&[
        "Go to profile or page",
        "Run command",
        "Open application",
        "Shortcut",
        "Send named event",
        "More actions",
        "Play sound",
        "None",
    ]);
    let action_picker = gtk::DropDown::new(Some(action_model.clone()), gtk::Expression::NONE);
    action_picker.set_selected(7);
    let layer_target_model = gtk::StringList::new(&[]);
    for name in layers.borrow().names() {
        layer_target_model.append(&name);
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
    let binding_status = Label::new(Some("Select a physical control to inspect its binding."));
    binding_status.add_css_class("binding-status");
    binding_status.set_xalign(0.0);
    binding_status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    binding_status.set_max_width_chars(32);
    binding_status.connect_label_notify(|label| label.set_tooltip_text(Some(&label.text())));
    let save_binding = binding_tool(
        "sdeck-save-symbolic",
        "Save binding",
        "Saves this control's binding without running the action.",
    );
    save_binding.add_css_class("suggested-action");
    let remove_binding = binding_tool(
        "sdeck-remove-symbolic",
        "Remove binding",
        "Deletes this control's binding. You can undo it right after.",
    );
    remove_binding.add_css_class("destructive-action");
    remove_binding.add_css_class("remove-binding");
    let control_number_row = labeled("Number", &control_number_picker);
    let layer_target_row = labeled("Destination profile or page", &layer_target_picker);
    let text_row = labeled("Command / key / shortcut key / event name", &text_entry);
    let application_row = labeled("Application executable", &application_entry);
    let args_scroller = gtk::ScrolledWindow::builder()
        .min_content_height(48)
        .child(&args_view)
        .build();
    let args_row = labeled("Arguments (one per line)", &args_scroller);
    let args_label = args_row.first_child().unwrap().downcast::<Label>().unwrap();
    args_label.set_mnemonic_widget(Some(&args_view));
    args_view.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
        args_label.upcast_ref()
    ])]);
    // One aligned, linked row of toggles instead of a ragged check grid.
    let modifier_buttons = GtkBox::new(Orientation::Horizontal, 0);
    modifier_buttons.add_css_class("linked");
    modifier_buttons.add_css_class("modifier-toggles");
    modifier_buttons.set_halign(gtk::Align::Start);
    for toggle in [&ctrl_mod, &alt_mod, &shift_mod, &super_mod] {
        modifier_buttons.append(toggle);
    }
    let modifier_row = labeled("Shortcut modifiers", &modifier_buttons);

    let binding_ui = BindingUi::new(
        layer_picker.clone(),
        layer_model.clone(),
        control_picker,
        control_number_picker,
        control_number_model,
        event_picker,
        event_model,
        action_picker,
        layer_target_picker,
        layer_target_model,
        text_entry,
        application_entry,
        args_view,
        super_mod,
        shift_mod,
        ctrl_mod,
        alt_mod,
        binding_status,
        control_number_row,
        layer_target_row,
        text_row,
        application_row,
        args_row,
        modifier_row,
    );
    binding_ui.refresh_layer_targets(&layers.borrow().names());
    binding_ui.update_for_control(false);
    binding_ui.update_action_visibility();
    binding_ui.load_selected_binding(&layers);
    binding_ui.refresh_summaries(&layers);
    let feature_runtime = feature_runtime::FeatureRuntime::new(
        layers.clone(),
        binding_ui.clone(),
        artwork.clone(),
        layer_sender.clone(),
        updating_layers.clone(),
    );
    configuration_menu.bind(feature_runtime.clone());
    let profile_tasks = gtk::Stack::builder()
        .vhomogeneous(false)
        .hhomogeneous(true)
        .build();
    profile_tasks.add_titled(feature_runtime.management(), Some("edit"), "Edit profile");
    profile_tasks.add_titled(&creation, Some("new"), "New profile");
    let task_picker = gtk::StackSwitcher::builder().stack(&profile_tasks).build();
    task_picker.add_css_class("popover-tabs");
    task_picker.set_halign(gtk::Align::Fill);
    management.append(&task_picker);
    management.append(&profile_tasks);
    let management_scroll = ui::panel_body(&management);
    // The header, subtitle and footer divider take room; keep the forms scroll-free.
    management_scroll.set_max_content_height(344);
    let management_popover = ui::popover("Profiles", &management_scroll);
    management_popover.add_css_class("organize-popover");
    ui::refine_popover_header(
        &management_popover,
        "preferences-system-symbolic",
        "Choose a profile to edit, or create a new one.",
    );
    let management_menu = ui::menu_button(
        "preferences-system-symbolic",
        "Organize",
        &management_popover,
    );
    // Profile tools stay secondary to the profile tabs they sit beside.
    management_menu.set_has_frame(false);
    layer_row.append(&management_menu);
    // Integration settings are global, not part of every action's form.
    let obs_popover = ui::popover("OBS Studio", &ui::panel_body(&feature_runtime.obs_panel));
    feature_runtime.obs_panel.set_expanded(true);
    let obs_menu = ui::menu_button("camera-video-symbolic", "OBS", &obs_popover);
    obs_menu.set_has_frame(false);
    layer_row.append(&obs_menu);
    feature_runtime
        .profile_tabs()
        .set_action_widget(&layer_row, gtk::PackType::End);
    // Navigation (linked arrows + breadcrumb) leads; page management trails.
    let navigation_row = GtkBox::new(Orientation::Horizontal, 12);
    feature_runtime.indicator.set_hexpand(true);
    navigation_row.append(feature_runtime.page_navigation());
    navigation_row.append(&feature_runtime.indicator);
    navigation_row.append(feature_runtime.controls());
    layer_card.append(&navigation_row);
    if let Some(app) = window.application() {
        let cleanup = feature_runtime.clone();
        app.connect_shutdown(move |_| {
            cleanup.cancel_key_holds();
            cleanup.cancel_timer();
        });
    }

    let settings_card = card();
    settings_card.add_css_class("editor-card");
    let settings_heading = Label::new(Some(&control_heading(ControlKind::Button, Some(1))));
    settings_heading.add_css_class("section-label");
    settings_heading.add_css_class("control-heading");
    settings_heading.set_xalign(0.0);
    settings_card.append(&settings_heading);
    settings_card.append(feature_runtime.touchscreen_controls());
    // Keep a usable form width without padding short actions to the chassis height.
    settings_card.set_size_request(300, -1);
    settings_card.set_spacing(6);
    settings_card.set_hexpand(true);
    settings_card.set_valign(gtk::Align::Start);
    let selector_row = GtkBox::new(Orientation::Horizontal, 12);
    selector_row.append(&labeled("Event", &binding_ui.event_picker));
    selector_row.append(&labeled("Action", &binding_ui.action_picker));
    // A key has one event and takes its action from the catalog, so only
    // dials and the touch strip show these selectors.
    selector_row.set_visible(false);
    settings_card.append(&selector_row);
    settings_card.append(&binding_ui.hold_toggle);
    settings_card.append(&binding_ui.action_guidance);
    settings_card.append(&binding_ui.layer_target_row);
    let action_fields = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .focusable(false)
        .min_children_per_line(1)
        .max_children_per_line(if matches!(binding_ui.action_picker.selected(), 2 | 3) {
            2
        } else {
            1
        })
        .homogeneous(true)
        .column_spacing(16)
        .row_spacing(12)
        .hexpand(true)
        .build();
    action_fields.add_css_class("action-fields");
    for row in [
        &binding_ui.text_row,
        &binding_ui.modifier_row,
        &binding_ui.application_row,
        &binding_ui.args_row,
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
    action_fields.set_visible(matches!(binding_ui.action_picker.selected(), 1..=4));
    let fields = action_fields.downgrade();
    binding_ui
        .action_picker
        .connect_selected_notify(move |picker| {
            if let Some(fields) = fields.upgrade() {
                fields.set_max_children_per_line(if matches!(picker.selected(), 2 | 3) {
                    2
                } else {
                    1
                });
                fields.set_visible(matches!(picker.selected(), 1..=4));
            }
        });
    settings_card.append(&action_fields);
    settings_card.append(&binding_ui.feature_editor.root);
    let options_heading = inspector_heading("Options");
    binding_ui
        .return_toggle
        .bind_property("visible", &options_heading, "visible")
        .sync_create()
        .build();
    settings_card.append(&options_heading);
    settings_card.append(&binding_ui.return_toggle);
    settings_card.append(&binding_ui.return_row);
    let binding_buttons = GtkBox::new(Orientation::Horizontal, 8);
    let test_action = binding_tool(
        "media-playback-start-symbolic",
        "Test action",
        "Runs this action now. Saving a binding never runs it.",
    );
    binding_buttons.append(&save_binding);
    binding_buttons.append(&test_action);
    binding_buttons.append(&remove_binding);
    // The status line sits right under Save/Test/Remove so results stay visible.
    // The toolbar tops the right-hand panel, above its tabs.
    let binding_feedback = GtkBox::new(Orientation::Vertical, 8);
    binding_feedback.add_css_class("binding-toolbar");
    binding_feedback.append(&binding_buttons);
    binding_feedback.append(&binding_ui.status);
    binding_feedback.append(&binding_ui.undo_remove);
    let artwork_content = GtkBox::new(Orientation::Vertical, 12);
    let target_picker = add_artwork_controls(&artwork_content, &window, &artwork);
    let text_editor = button_text_editor::ButtonTextEditor::new(&artwork);
    *artwork.text_editor.borrow_mut() = Rc::downgrade(&text_editor);
    text_editor.follow_page_label(&feature_runtime.indicator);
    text_editor.attach_image_controls(&window);
    let appearance = GtkBox::new(Orientation::Vertical, 4);
    appearance.add_css_class("appearance-section");
    // Keys are styled from their right-click menu and dials have no artwork,
    // so only the touch strip and its sections show this section.
    appearance.set_visible(false);
    appearance.append(&gtk::Separator::new(Orientation::Horizontal));
    let appearance_heading = inspector_heading("Appearance");
    appearance_heading.set_hexpand(true);
    let appearance_tools = GtkBox::new(Orientation::Horizontal, 12);
    appearance_tools.append(&appearance_heading);
    appearance.append(&appearance_tools);
    let editor = Rc::downgrade(&text_editor);
    let appearance_weak = appearance.downgrade();
    let selector_row_weak = selector_row.downgrade();
    let feature_editor = binding_ui.feature_editor.clone();
    // The initial control is a key.
    feature_editor.set_pickers_hidden(true);
    binding_ui
        .control_picker
        .connect_selected_notify(move |picker| {
            if let Some(row) = selector_row_weak.upgrade() {
                row.set_visible(picker.selected() != 0);
            }
            feature_editor.set_pickers_hidden(picker.selected() == 0);
            if let Some(editor) = editor.upgrade() {
                editor.set_control_available(picker.selected() == 0);
            }
            if let Some(appearance) = appearance_weak.upgrade() {
                // Only the touch strip and its sections have artwork to set here.
                appearance.set_visible(picker.selected() == 2);
            }
        });
    let artwork_section = gtk::Expander::new(Some("Artwork and touch zones"));
    artwork_section.set_child(Some(&artwork_content));
    appearance.append(&artwork_section);
    settings_card.append(&appearance);
    let layer_hint = Label::new(Some(
        "Touchscreen pages belong to the profile, independently of keypad pages and folders.",
    ));
    layer_hint.add_css_class("muted");
    layer_hint.set_xalign(0.0);
    layer_hint.set_wrap(true);
    layer_hint.set_max_width_chars(32);
    artwork_content.append(&layer_hint);
    {
        let ui = binding_ui.clone();
        let layers = layers.clone();
        let runtime = feature_runtime.clone();
        test_action.connect_clicked(move |_| {
            match ui
                .binding_from_widgets(&layers)
                .and_then(|binding| ui.page_return_from_widgets().map(|page| (binding, page)))
            {
                Ok(((layer, trigger, action), page_return)) => runtime.execute_with_return(
                    binding_identity(&layer, &trigger),
                    action,
                    page_return,
                ),
                Err(error) => ui.status.set_text(&error),
            }
        });
    }
    {
        let layers = layers.clone();
        let runtime = feature_runtime.clone();
        let ui = binding_ui.clone();
        let artwork = artwork.clone();
        let updating_layers = updating_layers.clone();
        layer_picker.connect_selected_notify(move |picker| {
            if updating_layers.get() {
                return;
            }
            let index = picker.selected() as usize;
            runtime.cancel_key_holds();
            if let Some(snapshot) = select_layer(&layers, index) {
                runtime.persist(snapshot);
            }
            ui.load_selected_binding(&layers);
            ui.refresh_summaries(&layers);
            artwork.refresh(false);
        });
    }
    {
        let layers = layers.clone();
        let name = layer_name.clone();
        let model = layer_model.clone();
        let picker = layer_picker.clone();
        let runtime = feature_runtime.clone();
        let ui = binding_ui.clone();
        let artwork = artwork.clone();
        let updating_layers = updating_layers.clone();
        let tasks = profile_tasks.downgrade();
        create_layer.connect_clicked(move |_| {
            let value = name.text().to_string();
            let trimmed = value.trim().to_owned();
            let result = layers.borrow_mut().create(&value);
            if let Err(error) = result {
                let message = if layers.borrow().names().contains(&trimmed) {
                    format!("A profile named {trimmed} already exists.")
                } else if error.to_string().contains("limit") {
                    "Profile limit reached. Delete a profile to create another.".to_owned()
                } else {
                    format!("Could not create profile: {error}")
                };
                creation_error.set_text(&message);
                creation_error.set_visible(true);
                ui::feedback(&creation_error, true);
                runtime.report(&message, true);
            } else {
                name.set_text("");
                creation_error.set_visible(false);
                let names = layers.borrow().names();
                // Keep picker indices aligned with LayerStore's sorted names.
                updating_layers.set(true);
                replace_string_list(&model, &names);
                ui.refresh_layer_targets(&names);
                let index = layer_index(&layers, &trimmed);
                picker.set_selected(index);
                updating_layers.set(false);
                runtime.cancel_key_holds();
                if let Some(snapshot) = select_layer(&layers, index as usize) {
                    runtime.save_profile(snapshot);
                    runtime.edit_profile(&trimmed);
                }
                ui.load_selected_binding(&layers);
                ui.refresh_summaries(&layers);
                artwork.refresh(false);
                name.set_text("");
                if let Some(tasks) = tasks.upgrade() {
                    tasks.set_visible_child_name("edit");
                }
            }
        });
    }
    {
        let layers = layers.clone();
        let ui = binding_ui.clone();
        ui.control_picker.clone().connect_selected_notify(move |_| {
            ui.reset_hold_mode();
            ui.update_for_control(true);
            ui.load_selected_binding(&layers);
        });
    }
    {
        let layers = layers.clone();
        let ui = binding_ui.clone();
        ui.control_number_picker
            .clone()
            .connect_selected_notify(move |_| {
                ui.reset_hold_mode();
                ui.load_selected_binding(&layers);
            });
    }
    {
        let layers = layers.clone();
        let ui = binding_ui.clone();
        ui.hold_toggle.clone().connect_toggled(move |_| {
            if !ui.hold_reset.get() {
                ui.load_selected_binding(&layers);
            }
        });
    }
    {
        let layers = layers.clone();
        let ui = binding_ui.clone();
        ui.event_picker
            .clone()
            .connect_selected_notify(move |_| ui.load_selected_binding(&layers));
    }
    {
        let ui = binding_ui.clone();
        ui.action_picker
            .clone()
            .connect_selected_notify(move |_| ui.update_action_visibility());
    }
    {
        let layers = layers.clone();
        let runtime = feature_runtime.clone();
        let ui = binding_ui.clone();
        let toast = toast.clone();
        let artwork = artwork.clone();
        save_binding.connect_clicked(move |_| {
            match ui
                .binding_from_widgets(&layers)
                .and_then(|binding| ui.page_return_from_widgets().map(|page| (binding, page)))
            {
                Ok(((layer, trigger, action), page_return)) => {
                    let info = ui.info_from_widgets();
                    if info == Some(sdeck_plus::info::InfoSource::Weather) {
                        let saved = sdeck_plus::info::save_location(
                            layers.borrow().root(),
                            &ui.info_location.text(),
                        );
                        if let Err(error) = saved {
                            let message = format!("Could not save weather location: {error}");
                            ui.error(&message);
                            toast.show(&message, true);
                            return;
                        }
                    }
                    let snapshot = {
                        let mut store = layers.borrow_mut();
                        // Sounds and files are kept as app-owned links, never as the real path.
                        let root = store.root().to_path_buf();
                        let action = sdeck_plus::asset_links::link_action(&root, action);
                        action
                            .and_then(|action| store.bind(&layer, trigger.clone(), action))
                            .and_then(|_| store.set_return(&layer, &trigger, page_return))
                            .and_then(|_| store.set_info(&layer, &trigger, info))
                            .map(|_| store.clone())
                    };
                    match snapshot {
                        Ok(snapshot) => {
                            ui.mark_clean();
                            if runtime.save_binding(snapshot) {
                                ui.status.set_text("Binding updated; saving to disk…");
                            } else {
                                let error = "Could not save binding: saving worker is unavailable.";
                                ui.error(error);
                                toast.show(error, true);
                            }
                            ui.refresh_summaries(&layers);
                            artwork.refresh(true);
                        }
                        Err(error) => {
                            let message = format!("Could not save binding: {error}");
                            ui.error(&message);
                            toast.show(&message, true);
                        }
                    }
                }
                Err(error) => {
                    ui.error(&error);
                    toast.show(&error, true);
                }
            }
        });
    }
    {
        let layers = layers.clone();
        let runtime = feature_runtime.clone();
        let ui = binding_ui.clone();
        let artwork = artwork.clone();
        remove_binding.connect_clicked(move |_| match ui.selected_layer_and_trigger(&layers) {
            Ok((layer, trigger)) => {
                let previous = {
                    let store = layers.borrow();
                    store
                        .binding(&layer, &trigger)
                        .map(|action| RemovedBinding {
                            layer: layer.clone(),
                            trigger: trigger.clone(),
                            action,
                            page_return: store.page_return(&layer, &trigger),
                            info: store.info_source(&layer, &trigger),
                        })
                };
                let snapshot = {
                    let mut store = layers.borrow_mut();
                    store
                        .unbind(&layer, &trigger)
                        .map(|removed| (removed, store.clone()))
                };
                match snapshot {
                    Ok((true, snapshot)) => {
                        runtime.persist(snapshot);
                        ui.clear_action_fields();
                        ui.status.set_text("Binding removed.");
                        ui.mark_clean();
                        *ui.removed.borrow_mut() = previous;
                        ui.undo_remove.set_visible(ui.removed.borrow().is_some());
                        ui::feedback(&ui.status, false);
                        ui.refresh_summaries(&layers);
                        artwork.refresh(true);
                    }
                    Ok((false, _)) => ui
                        .status
                        .set_text("No binding exists for that control event."),
                    Err(error) => ui.error(&format!("Could not remove binding: {error}")),
                }
            }
            Err(error) => ui.error(&error),
        });
    }
    {
        let layers = layers.clone();
        let runtime = feature_runtime.clone();
        let ui = binding_ui.clone();
        let artwork = artwork.clone();
        let toast = toast.clone();
        binding_ui.undo_remove.clone().connect_clicked(move |_| {
            let Some(removed) = ui.removed.borrow_mut().take() else {
                return;
            };
            ui.undo_remove.set_visible(false);
            let snapshot = {
                let mut store = layers.borrow_mut();
                store
                    .bind(
                        &removed.layer,
                        removed.trigger.clone(),
                        removed.action.clone(),
                    )
                    .and_then(|_| {
                        store.set_return(
                            &removed.layer,
                            &removed.trigger,
                            removed.page_return.clone(),
                        )
                    })
                    .and_then(|_| store.set_info(&removed.layer, &removed.trigger, removed.info))
                    .map(|_| store.clone())
            };
            match snapshot {
                Ok(snapshot) => {
                    let saving = runtime.save_binding(snapshot);
                    ui.load_selected_binding(&layers);
                    ui.refresh_summaries(&layers);
                    artwork.refresh(true);
                    if saving {
                        ui.status.set_text("Binding restored; saving to disk…");
                    } else {
                        ui.error("Could not save binding: saving worker is unavailable.");
                    }
                }
                Err(error) => {
                    let message = format!("Could not restore binding: {error}");
                    ui.error(&message);
                    toast.show(&message, true);
                }
            }
        });
    }
    let feedback = GtkBox::new(Orientation::Horizontal, 8);
    feature_runtime.status.set_hexpand(true);
    feedback.append(&feature_runtime.status);
    feedback.append(&feature_runtime.timer_status);
    root.append(&feedback);
    feedback.set_visible(false);
    for changing in [&feature_runtime.status, &feature_runtime.timer_status] {
        let feedback = feedback.downgrade();
        let status = feature_runtime.status.clone();
        let timer = feature_runtime.timer_status.clone();
        changing.connect_visible_notify(move |_| {
            if let Some(feedback) = feedback.upgrade() {
                feedback.set_visible(status.get_visible() || timer.get_visible());
            }
        });
    }

    let body = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(1)
        .max_children_per_line(2)
        .column_spacing(16)
        .row_spacing(16)
        .build();
    // The preview and its inspector stay together; the action catalog wraps
    // beneath them on narrow desktops.
    body.set_vexpand(true);
    body.add_css_class("configuration-grid");
    let sidebar = GtkBox::new(Orientation::Vertical, 6);
    sidebar.set_hexpand(true);
    sidebar.set_valign(gtk::Align::Start);
    sidebar.add_css_class("settings-sidebar");
    let device_area = card();
    device_area.add_css_class("device-canvas");
    // Reference image coordinates scaled to a compact physical front. Fixed
    // geometry keeps loaded artwork and long live messages from resizing hardware.
    let chassis = gtk::Fixed::new();
    chassis.set_halign(gtk::Align::Center);
    chassis.set_valign(gtk::Align::Center);
    chassis.add_css_class("hardware-chassis");
    chassis.set_size_request(426, 414);
    device_area.append(&chassis);
    let brand = Label::new(Some("STREAM DECK +"));
    brand.add_css_class("hardware-brand");
    brand.set_size_request(426, 24);
    chassis.put(&brand, 0.0, 6.0);
    let mut selectable_widgets: Vec<gtk::Widget> = Vec::new();
    let mut key_labels = Vec::new();
    for index in 0..8 {
        let select_button = Button::new();
        select_button.add_css_class("control-selectable");
        select_button.add_css_class("key-surface");
        select_button.set_overflow(gtk::Overflow::Hidden);
        select_button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Button {}",
            index + 1
        ))]);
        select_button.set_size_request(60, 60);
        let picture = artwork_picture();
        picture.set_visible(false);
        let surface = gtk::Overlay::new();
        surface.set_size_request(60, 60);
        surface.add_overlay(&picture);
        let label = Label::new(None);
        label.add_css_class("control-tile");
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(4);
        // Keep live-state bookkeeping without drawing names over key artwork.
        label.set_visible(false);
        surface.add_overlay(&label);
        select_button.set_child(Some(&surface));
        chassis.put(
            &select_button,
            48.0 + (index % 4) as f64 * 91.0,
            38.0 + (index / 4) as f64 * 67.0,
        );
        register_picture(&artwork, ArtworkTarget::Keypad((index + 1) as u8), &picture);
        selectable_widgets.push(select_button.upcast());
        key_labels.push(label);
    }
    let bezel = gtk::Fixed::new();
    bezel.add_css_class("hardware-bezel");
    bezel.set_size_request(374, 77);
    chassis.put(&bezel, 27.0, 177.0);
    let touch_status = Label::new(Some("Touch strip · waiting for input"));
    touch_status.add_css_class("touch-display");
    touch_status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    touch_status.set_max_width_chars(32);
    touch_status.set_hexpand(true);
    touch_status.set_size_request(-1, 16);
    // The live status and the strip's gesture badges share the header line.
    let strip_header = GtkBox::new(Orientation::Horizontal, 6);
    strip_header.set_size_request(328, 16);
    strip_header.append(&touch_status);
    strip_header.append(&binding_ui.gesture_badges);
    bezel.put(&strip_header, 23.0, 3.0);
    let strip_button = Button::new();
    strip_button.add_css_class("control-selectable");
    strip_button.add_css_class("strip-surface");
    let strip = gtk::Overlay::new();
    strip.set_size_request(328, 41);
    strip.add_css_class("strip-surface");
    let background = artwork_picture();
    background.set_visible(false);
    let background_surface = gtk::Overlay::new();
    background_surface.add_overlay(&background);
    strip_button.set_child(Some(&background_surface));
    strip.set_child(Some(&strip_button));
    register_picture(&artwork, ArtworkTarget::TouchStripBackground, &background);
    let zones = GtkBox::new(Orientation::Horizontal, 0);
    zones.set_homogeneous(true);
    let mut zone_select_buttons = Vec::new();
    for zone in 1..=4 {
        let zone_button = Button::new();
        zone_button.add_css_class("control-selectable");
        zone_button.add_css_class("strip-zone");
        let visual = gtk::Overlay::new();
        let picture = artwork_picture();
        picture.set_visible(false);
        visual.add_overlay(&picture);
        zone_button.set_child(Some(&visual));
        zone_button.set_tooltip_text(Some(&format!("Touch zone {zone}")));
        zone_button.set_hexpand(true);
        zone_button.set_vexpand(true);
        register_picture(&artwork, ArtworkTarget::EncoderZone(zone), &picture);
        zones.append(&zone_button);
        zone_select_buttons.push(zone_button);
    }
    strip.add_overlay(&zones);
    bezel.put(&strip, 23.0, 25.0);
    selectable_widgets.push(strip_button.clone().upcast());
    let rail = gtk::Fixed::new();
    rail.add_css_class("hardware-rail");
    rail.set_size_request(374, 108);
    chassis.put(&rail, 27.0, 273.0);
    let mut encoder_labels = Vec::new();
    for index in 0..4 {
        let button = Button::new();
        button.add_css_class("control-selectable");
        button.add_css_class("encoder");
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Encoder {}",
            index + 1
        ))]);
        button.set_size_request(46, 46);
        let label = Label::new(None);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(4);
        label.add_css_class("control-tile");
        // Keep dial state bookkeeping without drawing legends on the knobs.
        label.set_visible(false);
        button.set_child(Some(&label));
        rail.put(&button, 28.0 + index as f64 * 91.0, 34.0);
        // The assigned action, centered under the knob inside the rail. It is
        // preview-only and never drawn on the physical device.
        // The same action's icon on the knob face; clicks still reach the knob.
        rail.put(
            &binding_ui.dial_icons[index],
            28.0 + 23.0 - f64::from(DIAL_ICON_SIZE) / 2.0 + index as f64 * 91.0,
            34.0 + 23.0 - f64::from(DIAL_ICON_SIZE) / 2.0,
        );
        let legend = &binding_ui.dial_legends[index];
        legend.set_size_request(DIAL_LEGEND_WIDTH, 16);
        rail.put(
            legend,
            28.0 + 23.0 - f64::from(DIAL_LEGEND_WIDTH) / 2.0 + index as f64 * 91.0,
            84.0,
        );
        selectable_widgets.push(button.upcast());
        encoder_labels.push(label);
    }
    for (index, widget) in selectable_widgets.iter().take(8).enumerate() {
        connect_artwork_selection(
            &widget.clone().downcast::<Button>().unwrap(),
            &target_picker,
            &artwork,
            ArtworkTarget::Keypad((index + 1) as u8),
        );
    }
    connect_artwork_selection(
        &strip_button,
        &target_picker,
        &artwork,
        ArtworkTarget::TouchStripBackground,
    );
    for (index, button) in zone_select_buttons.iter().enumerate() {
        connect_artwork_selection(
            button,
            &target_picker,
            &artwork,
            ArtworkTarget::EncoderZone((index + 1) as u8),
        );
    }
    // Summaries remain available through tooltips, never underneath the front.
    for (index, widget) in selectable_widgets.iter().enumerate() {
        let widget = widget.clone();
        let tooltip = move |text: &str| {
            if index < 8 {
                format!(
                    "Button {} · {text}\nDrag onto another key to swap saved settings and artwork.",
                    index + 1
                )
            } else if index >= 9 {
                format!("Encoder {} · {text}", index - 8)
            } else {
                text.to_owned()
            }
        };
        widget.set_tooltip_text(Some(&tooltip(&binding_ui.summaries[index].text())));
        binding_ui.summaries[index].connect_label_notify(move |label| {
            widget.set_tooltip_text(Some(&tooltip(&label.text())))
        });
    }

    let key_widgets = selectable_widgets[..8].to_vec();
    let encoder_widgets = selectable_widgets[9..].to_vec();
    selectable_widgets.extend(
        zone_select_buttons
            .iter()
            .map(|zone| zone.clone().upcast::<gtk::Widget>()),
    );
    let zone_widgets: Vec<gtk::Widget> = zone_select_buttons
        .iter()
        .map(|zone| zone.clone().upcast())
        .collect();
    let selected_widgets = Rc::new(selectable_widgets);
    button_swap::connect(&selected_widgets[..8], &artwork, &feature_runtime, &root);
    {
        // Right-click selects the key like a left click would, unless it already is.
        let ui = binding_ui.clone();
        let artwork = artwork.clone();
        let keys: Vec<_> = selected_widgets[..8]
            .iter()
            .map(ObjectExt::downgrade)
            .collect();
        text_editor.connect_context_menu(
            &selected_widgets[..8],
            Rc::new(move |index| {
                let number = (index + 1) as u8;
                let current =
                    ui.control_kind() == ControlKind::Button && ui.control_number() == Some(number);
                if !current {
                    if let Some(button) = keys
                        .get(index)
                        .and_then(glib::WeakRef::upgrade)
                        .and_then(|key| key.downcast::<Button>().ok())
                    {
                        button.emit_clicked();
                    }
                }
                artwork.selected_target.get() == ArtworkTarget::Keypad(number)
            }),
        );
    }
    set_selected_control(&selected_widgets, 0);
    for index in 0..8 {
        connect_control_selection(
            &key_labels[index],
            selected_widgets[index]
                .clone()
                .downcast::<Button>()
                .expect("button selectable"),
            &binding_ui,
            &layers,
            &settings_heading,
            selected_widgets.clone(),
            index,
            ControlKind::Button,
            (index + 1) as u8,
        );
    }
    connect_control_selection(
        &touch_status,
        strip_button,
        &binding_ui,
        &layers,
        &settings_heading,
        selected_widgets.clone(),
        8,
        ControlKind::Touch,
        0,
    );
    // Each of the four touch sections is its own control.
    for (index, zone_button) in zone_select_buttons.into_iter().enumerate() {
        connect_control_selection(
            &touch_status,
            zone_button,
            &binding_ui,
            &layers,
            &settings_heading,
            selected_widgets.clone(),
            13 + index,
            ControlKind::Touch,
            (index + 1) as u8,
        );
    }
    for index in 0..4 {
        connect_control_selection(
            &encoder_labels[index],
            selected_widgets[index + 9]
                .clone()
                .downcast::<Button>()
                .expect("encoder selectable"),
            &binding_ui,
            &layers,
            &settings_heading,
            selected_widgets.clone(),
            index + 9,
            ControlKind::Encoder,
            (index + 1) as u8,
        );
    }

    // The fixed hardware geometry is presented through one uniform scale, so
    // spare width/height enlarges the whole preview without moving controls.
    let preview = scaled_bin::ScaledBin::new(&device_area, PREVIEW_MAX_SCALE);
    preview.set_hexpand(true);
    preview.set_vexpand(false);
    preview.add_css_class("device-preview");
    let workspace = GtkBox::new(Orientation::Vertical, 12);
    workspace.set_hexpand(true);
    workspace.append(&preview);
    // A tall form scrolls here, within its own region, instead of stretching the page.
    let settings_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(480)
        .hexpand(true)
        .valign(gtk::Align::Start)
        .child(&settings_card)
        .build();
    settings_scroll.add_css_class("settings-scroll");
    workspace.append(&settings_scroll);
    body.insert(&workspace, -1);
    // The right-hand panel: draggable actions, dial presets and per-binding options.
    let side_panel = card();
    side_panel.add_css_class("action-catalog");
    side_panel.set_size_request(250, -1);
    side_panel.set_valign(gtk::Align::Start);
    let tabs = gtk::Stack::builder()
        .vhomogeneous(false)
        .hhomogeneous(true)
        .build();
    tabs.add_titled(
        &action_catalog::connect(
            selected_widgets.clone(),
            &binding_ui,
            &layers,
            &settings_heading,
            &save_binding,
        ),
        Some("actions"),
        "Actions",
    );
    tabs.add_titled(
        &dial_presets::connect(
            selected_widgets.clone(),
            &binding_ui,
            &layers,
            &settings_heading,
            &feature_runtime,
            &artwork,
        ),
        Some("dials"),
        "Dials",
    );
    let binding_page = GtkBox::new(Orientation::Vertical, 12);
    binding_page.append(&ui::helper(
        "Options saved with the selected control's binding. Use Save binding to apply them.",
    ));
    binding_page.append(&binding_ui.info_toggle);
    binding_page.append(&binding_ui.info_row);
    let unavailable = ui::helper("Live information is not available for this control event.");
    binding_ui
        .info_toggle
        .bind_property("visible", &unavailable, "visible")
        .sync_create()
        .invert_boolean()
        .build();
    binding_page.append(&unavailable);
    tabs.add_titled(&binding_page, Some("binding"), "Binding");
    let switcher = gtk::StackSwitcher::builder().stack(&tabs).build();
    switcher.add_css_class("sidebar-tabs");
    side_panel.append(&binding_feedback);
    side_panel.append(&switcher);
    side_panel.append(&tabs);
    sidebar.append(&side_panel);
    body.insert(&sidebar, -1);
    let profile_content = GtkBox::new(Orientation::Vertical, 16);
    profile_content.append(&layer_card);
    profile_content.append(&body);
    feature_runtime.attach_profile_content(&profile_content);
    root.append(feature_runtime.profile_tabs());

    // All thirteen pictures are registered independently of the shared editor.
    artwork.refresh(false);

    // A two-axis viewport is the fallback for unusually small allocations or fonts.
    // Normal desktop sizes fit the compact horizontal canvas without horizontal scrolling.
    let viewport = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&clamp::Clamp::new(&root, SHELL_MAX_WIDTH))
        .build();
    feature_runtime.attach_configuration_root(viewport.upcast_ref());
    // Keep the form within about one window of height; the preview stays put.
    let scroll = settings_scroll.downgrade();
    viewport
        .vadjustment()
        .connect_page_size_notify(move |adjustment| {
            if let Some(scroll) = scroll.upgrade() {
                scroll.set_max_content_height((adjustment.page_size() as i32 - 240).max(320));
            }
        });
    MonitorWidgets {
        root: viewport,
        toast,
        binding_status: binding_ui.status.clone(),
        status,
        connection_detail,
        #[cfg(test)]
        layer_picker,
        #[cfg(test)]
        binding_ui,
        #[cfg(test)]
        obs_menu,
        #[cfg(test)]
        help_menu,
        #[cfg(test)]
        management_menu,
        #[cfg(test)]
        artwork_section,
        #[cfg(test)]
        target_picker,
        key_labels,
        encoder_labels,
        touch_status,
        key_widgets,
        encoder_widgets,
        zone_widgets,
        feature_runtime,
        organization_writes: vec![create_layer],
        _text_editor: text_editor,
        _configuration_menu: configuration_menu,
    }
}

fn start_event_updates(
    widgets: MonitorWidgets,
    receiver: mpsc::Receiver<MonitorEvent>,
    layers: Rc<RefCell<LayerStore>>,
    artwork: ArtworkUi,
    _layer_sender: mpsc::Sender<LayerCommand>,
    _app_event_sender: mpsc::Sender<MonitorEvent>,
) -> glib::SourceId {
    let ticks = Rc::new(RefCell::new([0i64; 4]));
    glib::timeout_add_local(Duration::from_millis(40), move || {
        let MonitorWidgets {
            status,
            toast,
            binding_status,
            connection_detail,
            feature_runtime,
            key_labels,
            encoder_labels,
            touch_status,
            key_widgets,
            encoder_widgets,
            zone_widgets,
            ..
        } = &widgets;
        let pending: Vec<_> = receiver.try_iter().collect();
        for message in &pending {
            if let MonitorEvent::ApplicationFocus(result) = message {
                feature_runtime.focus_changed(result.clone());
            }
        }
        for message in pending {
            match message {
                MonitorEvent::ApplicationFocus(_) => {}
                MonitorEvent::Heartbeat => {}
                MonitorEvent::ActionDispatched => {
                    feature_runtime.sync_info_from(&layers.borrow());
                }
                MonitorEvent::ConfigurationExportResult { path, result } => {
                    let failed = result.is_err();
                    feature_runtime.complete_configuration_export(&path, result);
                    toast.show(&feature_runtime.status.text(), failed);
                }
                MonitorEvent::ConfigurationImportResult { root, result } => {
                    let imported = feature_runtime.complete_configuration_import(&root, result);
                    if imported {
                        for button in &widgets.organization_writes {
                            button.set_sensitive(true);
                        }
                    }
                    toast.show(&feature_runtime.status.text(), !imported);
                }
                MonitorEvent::KeySwapResult {
                    layer,
                    first,
                    second,
                    result,
                } => {
                    feature_runtime.complete_key_swap(&layer, first, second, result);
                }
                MonitorEvent::Connection(state) => {
                    if !matches!(state, ConnectionState::Connected) {
                        feature_runtime.cancel_key_holds();
                    }
                    if show_connection_state(status, connection_detail, &state) {
                        artwork.refresh(true);
                    }
                }
                MonitorEvent::Input(event) => {
                    feature_runtime.execute_input(&event);
                    match event {
                        protocol::Event::Key { index, pressed } => {
                            if let Some(label) = key_labels.get(index) {
                                label.set_text(&format!(
                                    "KEY {} · {}",
                                    index + 1,
                                    if pressed { "PRESSED" } else { "IDLE" }
                                ));
                                set_active(label, pressed);
                            }
                            if let Some(widget) = key_widgets.get(index) {
                                set_pressed(widget, pressed);
                            }
                        }
                        protocol::Event::EncoderButton { index, pressed } => {
                            if let Some(label) = encoder_labels.get(index) {
                                label.set_text(&format!(
                                    "DIAL {}\n{} · {} ticks",
                                    index + 1,
                                    if pressed { "PRESSED" } else { "IDLE" },
                                    ticks.borrow()[index]
                                ));
                                set_active(label, pressed);
                            }
                            if let Some(widget) = encoder_widgets.get(index) {
                                set_pressed(widget, pressed);
                            }
                        }
                        protocol::Event::Rotation {
                            index,
                            ticks: delta,
                        } => {
                            let mut totals = ticks.borrow_mut();
                            totals[index] += i64::from(delta);
                            if let Some(label) = encoder_labels.get(index) {
                                label.set_text(&format!(
                                    "DIAL {}\n{} ticks · {delta:+}",
                                    index + 1,
                                    totals[index]
                                ));
                            }
                            if let Some(widget) = encoder_widgets.get(index) {
                                flash(widget, "control-rotated");
                            }
                        }
                        protocol::Event::TouchTap { x, y } => {
                            touch_status.set_text(&format!("TAP     ·     X {x}   /   Y {y}"));
                            flash_zones(zone_widgets, x, x);
                        }
                        protocol::Event::TouchPress { x, y } => {
                            touch_status
                                .set_text(&format!("PRESS & HOLD     ·     X {x}   /   Y {y}"));
                            flash_zones(zone_widgets, x, x);
                        }
                        protocol::Event::TouchFlick {
                            start_x,
                            start_y,
                            end_x,
                            end_y,
                        } => {
                            touch_status.set_text(&format!(
                                "FLICK     ·     ({start_x}, {start_y})  →  ({end_x}, {end_y})"
                            ));
                            flash_zones(zone_widgets, start_x.min(end_x), start_x.max(end_x));
                        }
                    }
                }
                MonitorEvent::ArtworkResult { target, result } => match result {
                    Ok(()) => {}
                    Err(error) => {
                        connection_detail.set_text(&format!(
                            "Artwork update failed for {target:?} (local assignment retained): {error}",
                        ));
                    }
                },
                MonitorEvent::DeviceResult { action, result } => match result {
                    Ok(()) => feature_runtime
                        .report(&format!("Device: {} applied.", action.summary()), false),
                    Err(error) => feature_runtime.report(
                        &format!("Device: {} failed: {error}", action.summary()),
                        true,
                    ),
                },
                MonitorEvent::AppEvent { name } => feature_runtime.report(&name, false),
                MonitorEvent::Info { source, value } => artwork.update_info(source, value),
                MonitorEvent::BindingSaveResult(result) => {
                    let (message, failed) = match result {
                        Ok(()) => ("Binding saved successfully.".to_owned(), false),
                        Err(error) => (format!("Could not save binding: {error}"), true),
                    };
                    binding_status.set_text(&message);
                    ui::feedback(binding_status, failed);
                    toast.show(&message, failed);
                }
                MonitorEvent::ProfileSaveResult(result) => {
                    let (message, failed) = match result {
                        Ok(()) => ("Profile saved successfully.".to_owned(), false),
                        Err(error) => (format!("Could not save profile: {error}"), true),
                    };
                    feature_runtime.report_profile_save(&message, failed);
                    toast.show(&message, failed);
                }
                MonitorEvent::LayerError(error) => {
                    connection_detail.set_text(&error);
                    feature_runtime.report(&error, true);
                }
            }
        }
        feature_runtime.poll_key_holds();
        glib::ControlFlow::Continue
    })
}

#[derive(Clone)]
struct BindingUi {
    layer_picker: gtk::DropDown,
    layer_model: gtk::StringList,
    control_picker: gtk::DropDown,
    control_number_picker: gtk::DropDown,
    control_number_model: gtk::StringList,
    event_picker: gtk::DropDown,
    event_model: gtk::StringList,
    action_picker: gtk::DropDown,
    layer_target_picker: gtk::DropDown,
    layer_target_model: gtk::StringList,
    text_entry: Entry,
    application_entry: Entry,
    args_view: TextView,
    super_mod: gtk::ToggleButton,
    shift_mod: gtk::ToggleButton,
    ctrl_mod: gtk::ToggleButton,
    alt_mod: gtk::ToggleButton,
    status: Label,
    control_number_row: GtkBox,
    layer_target_row: GtkBox,
    text_row: GtkBox,
    application_row: GtkBox,
    args_row: GtkBox,
    modifier_row: GtkBox,
    feature_editor: feature_editor::FeatureEditor,
    action_guidance: Label,
    hold_toggle: CheckButton,
    hold_reset: Rc<Cell<bool>>,
    return_toggle: CheckButton,
    return_target: gtk::DropDown,
    return_model: gtk::StringList,
    return_seconds: Entry,
    return_row: GtkBox,
    info_toggle: CheckButton,
    info_source: gtk::DropDown,
    info_location: Entry,
    info_row: GtkBox,
    info_location_row: GtkBox,
    // Same order as the physical selection widgets: eight buttons, touch, four encoders.
    summaries: Vec<Label>,
    /// Short assigned-action captions drawn under the four preview dials.
    dial_legends: Vec<Label>,
    /// The caption's action icon, drawn on each preview knob (preview-only).
    dial_icons: Vec<gtk::DrawingArea>,
    /// Swipe, tap and hold badges in the preview strip's header line.
    gesture_badges: GtkBox,
    shortcut_preview: Label,
    /// Fingerprint of the editor as last loaded or saved; `None` until the first load.
    baseline: std::rc::Rc<RefCell<Option<String>>>,
    undo_remove: Button,
    removed: Rc<RefCell<Option<RemovedBinding>>>,
}

/// What `Remove` deleted, kept so the feedback line can offer to bring it back.
#[derive(Clone)]
struct RemovedBinding {
    layer: String,
    trigger: Trigger,
    action: Action,
    page_return: Option<sdeck_plus::layers::PageReturn>,
    info: Option<sdeck_plus::info::InfoSource>,
}

impl BindingUi {
    #[allow(clippy::too_many_arguments)]
    fn new(
        layer_picker: gtk::DropDown,
        layer_model: gtk::StringList,
        control_picker: gtk::DropDown,
        control_number_picker: gtk::DropDown,
        control_number_model: gtk::StringList,
        event_picker: gtk::DropDown,
        event_model: gtk::StringList,
        action_picker: gtk::DropDown,
        layer_target_picker: gtk::DropDown,
        layer_target_model: gtk::StringList,
        text_entry: Entry,
        application_entry: Entry,
        args_view: TextView,
        super_mod: gtk::ToggleButton,
        shift_mod: gtk::ToggleButton,
        ctrl_mod: gtk::ToggleButton,
        alt_mod: gtk::ToggleButton,
        status: Label,
        control_number_row: GtkBox,
        layer_target_row: GtkBox,
        text_row: GtkBox,
        application_row: GtkBox,
        args_row: GtkBox,
        modifier_row: GtkBox,
    ) -> Self {
        // The closed button names the chosen More action instead of "•••";
        // menu rows keep one icon per action type.
        let more_icon = Rc::new(Cell::new(BindingIcon::More));
        let closed_more = more_icon.clone();
        binding_icon::icon_dropdown_with(
            &action_picker,
            Rc::new(move |position| match position {
                5 => closed_more.get(),
                _ => BindingIcon::for_action_type(position),
            }),
            Rc::new(BindingIcon::for_action_type),
        );
        status.connect_label_notify(ui::clear_feedback);
        let action_guidance = ui::helper("");
        shortcut_reference::connect(&action_guidance);
        let return_toggle = CheckButton::with_label("Return to a page afterwards");
        return_toggle.set_tooltip_text(Some(
            "After this action runs, move once to the page you choose.",
        ));
        let return_model = gtk::StringList::new(&[]);
        let return_target = gtk::DropDown::new(Some(return_model.clone()), gtk::Expression::NONE);
        bound_layer_picker(&return_target);
        let return_seconds = Entry::builder()
            .text("60")
            .placeholder_text("90")
            .width_chars(8)
            .build();
        let return_row = GtkBox::new(Orientation::Vertical, 12);
        return_row.append(&labeled(
            "Return to (profile, folder or page)",
            &return_target,
        ));
        return_row.append(&labeled(
            "After how many seconds (1–86400)",
            &return_seconds,
        ));
        return_row.set_visible(false);
        let row = return_row.clone();
        return_toggle.connect_toggled(move |toggle| row.set_visible(toggle.is_active()));
        let info_toggle = CheckButton::with_label("Show live information on the key");
        info_toggle.set_tooltip_text(Some(
            "Replaces the image with a live picture, such as the volume or the time. Dials, and a tap or hold on a touch section, draw it on that section.",
        ));
        let labels: Vec<&str> = sdeck_plus::info::InfoSource::ALL
            .iter()
            .map(|source| source.label())
            .collect();
        let info_source = gtk::DropDown::from_strings(&labels);
        let info_location = Entry::builder()
            .placeholder_text("Madrid or 40.4,-3.7")
            .hexpand(true)
            .build();
        let info_location_row = labeled(
            "Weather location (city or latitude,longitude)",
            &info_location,
        );
        info_location_row.set_visible(false);
        let info_row = GtkBox::new(Orientation::Vertical, 12);
        info_row.append(&labeled("Information to show", &info_source));
        info_row.append(&info_location_row);
        info_row.set_visible(false);
        let refresh_info = {
            let toggle = info_toggle.clone();
            let source = info_source.clone();
            let row = info_row.clone();
            let location = info_location_row.clone();
            move || {
                let on = toggle.is_active() && toggle.get_visible();
                row.set_visible(on);
                location.set_visible(
                    on && sdeck_plus::info::InfoSource::ALL.get(source.selected() as usize)
                        == Some(&sdeck_plus::info::InfoSource::Weather),
                );
            }
        };
        let refresh = refresh_info.clone();
        info_toggle.connect_toggled(move |_| refresh());
        info_source.connect_selected_notify(move |_| refresh_info());
        let undo_remove = ui::button("edit-undo-symbolic", "Undo remove");
        undo_remove.set_tooltip_text(Some("Restore the binding you just removed"));
        undo_remove.set_halign(gtk::Align::Start);
        undo_remove.set_visible(false);
        let shortcut_preview = Label::new(None);
        shortcut_preview.add_css_class("muted");
        shortcut_preview.set_xalign(0.0);
        shortcut_preview.set_visible(false);
        modifier_row.append(&shortcut_preview);
        let ui = Self {
            layer_picker,
            layer_model,
            control_picker,
            control_number_picker,
            control_number_model,
            event_picker,
            event_model,
            action_picker,
            layer_target_picker,
            layer_target_model,
            text_entry,
            application_entry,
            args_view,
            super_mod,
            shift_mod,
            ctrl_mod,
            alt_mod,
            status,
            control_number_row,
            layer_target_row,
            text_row,
            application_row,
            args_row,
            modifier_row,
            feature_editor: feature_editor::FeatureEditor::new(),
            action_guidance,
            hold_toggle: sdeck_plus::key_hold_editor::hold_toggle(),
            hold_reset: Rc::new(Cell::new(false)),
            return_toggle,
            return_target,
            return_model,
            return_seconds,
            return_row,
            info_toggle,
            info_source,
            info_location,
            info_row,
            info_location_row,
            summaries: (0..13)
                .map(|_| {
                    let label = Label::new(None);
                    label.add_css_class("muted");
                    label.set_xalign(0.0);
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    label.set_width_chars(10);
                    label.set_max_width_chars(12);
                    label.set_tooltip_text(Some(
                        "Assignments in the selected page. CW = clockwise; CCW = counterclockwise. Exact ticks are legacy overrides. Select an event for full action details.",
                    ));
                    label
                })
                .collect(),
            dial_legends: (0..4)
                .map(|_| {
                    let label = Label::new(None);
                    label.add_css_class("dial-legend");
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    label.set_max_width_chars(12);
                    label.set_visible(false);
                    label
                })
                .collect(),
            gesture_badges: {
                let row = GtkBox::new(Orientation::Horizontal, 4);
                row.add_css_class("gesture-badges");
                row.set_valign(gtk::Align::Center);
                row
            },
            dial_icons: (0..4)
                .map(|_| {
                    let icon = binding_icon::binding_icon(BindingIcon::Generic);
                    icon.add_css_class("dial-icon");
                    icon.set_size_request(DIAL_ICON_SIZE, DIAL_ICON_SIZE);
                    icon.set_can_target(false);
                    icon.set_visible(false);
                    icon
                })
                .collect(),
            shortcut_preview,
            baseline: std::rc::Rc::new(RefCell::new(None)),
            undo_remove,
            removed: Rc::new(RefCell::new(None)),
        };
        ui.connect_dirty_tracking();
        let picker = ui.action_picker.downgrade();
        ui.feature_editor.connect_icon_changed(move |icon| {
            let icon = icon.unwrap_or(BindingIcon::More);
            more_icon.set(icon);
            if let Some(picker) = picker.upgrade().filter(|picker| picker.selected() == 5) {
                binding_icon::set_closed_icon(&picker, icon);
            }
        });
        ui
    }

    fn error(&self, message: &str) {
        self.status.set_text(message);
        ui::feedback(&self.status, true);
    }

    fn refresh_summaries(&self, layers: &RefCell<LayerStore>) {
        // Snapshot text before setting GTK properties, which can invoke callbacks.
        let texts = {
            let store = layers.borrow();
            let layer = dropdown_value(&self.layer_model, self.layer_picker.selected())
                .unwrap_or_else(|| store.active().to_owned());
            let summaries = (0..13)
                .map(|index| {
                    let (control, number) = match index {
                        0..=7 => (ControlKind::Button, index + 1),
                        8 => (ControlKind::Touch, 0),
                        _ => (ControlKind::Encoder, index - 8),
                    };
                    control_summary(&store, &layer, control, number as u8)
                })
                .collect::<Vec<_>>();
            let legends = (1..=4)
                .map(|dial| {
                    sdeck_plus::binding_editor::dial_action(&store, &layer, dial).map(|action| {
                        (
                            sdeck_plus::binding_editor::dial_legend(&store, &layer, dial)
                                .unwrap_or_default(),
                            BindingIcon::for_saved_action(&action),
                        )
                    })
                })
                .collect::<Vec<_>>();
            let gestures = sdeck_plus::binding_editor::strip_gestures(&store, &layer);
            (summaries, legends, gestures)
        };
        let (texts, legends, gestures) = texts;
        while let Some(child) = self.gesture_badges.first_child() {
            self.gesture_badges.remove(&child);
        }
        for gesture in &gestures {
            self.gesture_badges.append(&binding_icon::gesture_badge(
                gesture.gesture.icon(),
                gesture.icon,
                &gesture.description(),
                gesture.default,
            ));
        }
        for (label, text) in self.summaries.iter().zip(texts) {
            label.set_text(&text.replace('\n', " · "));
            label.set_tooltip_text(Some(&text));
        }
        for ((label, icon), legend) in self.dial_legends.iter().zip(&self.dial_icons).zip(legends) {
            let text = legend.as_ref().map(|(text, _)| text.as_str());
            label.set_text(text.unwrap_or_default());
            label.set_tooltip_text(text);
            label.set_visible(legend.is_some());
            match legend
                .map(|(_, icon)| icon)
                .filter(|icon| icon.shown_on_controls())
            {
                Some(shown) => {
                    binding_icon::set_binding_icon(icon, shown);
                    icon.set_visible(true);
                }
                None => icon.set_visible(false),
            }
        }
    }

    /// Keys, dials and a tap or hold on one touch section can show information.
    fn info_supported(&self) -> bool {
        match self.control_kind() {
            ControlKind::Button => !self.hold_toggle.is_active(),
            ControlKind::Encoder => true,
            ControlKind::Touch => {
                matches!(
                    self.event_kind(),
                    EventKind::TouchTap | EventKind::TouchPress
                ) && self.control_number().is_some_and(|number| number > 0)
            }
        }
    }

    fn update_info_visibility(&self) {
        let supported = self.info_supported();
        self.info_toggle.set_visible(supported);
        let on = supported && self.info_toggle.is_active();
        self.info_row.set_visible(on);
        self.info_location_row.set_visible(
            on && self.info_from_widgets() == Some(sdeck_plus::info::InfoSource::Weather),
        );
    }

    fn info_from_widgets(&self) -> Option<sdeck_plus::info::InfoSource> {
        if !self.info_supported() || !self.info_toggle.is_active() {
            return None;
        }
        let index = usize::try_from(self.info_source.selected()).ok()?;
        sdeck_plus::info::InfoSource::ALL.get(index).copied()
    }

    fn populate_info(&self, value: Option<sdeck_plus::info::InfoSource>, location: &str) {
        let index = value
            .and_then(|source| {
                sdeck_plus::info::InfoSource::ALL
                    .iter()
                    .position(|s| *s == source)
            })
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(0);
        self.info_source.set_selected(index);
        self.info_location.set_text(location);
        self.info_toggle.set_active(value.is_some());
        self.update_info_visibility();
    }

    fn reset_hold_mode(&self) {
        self.hold_reset.set(true);
        self.hold_toggle.set_active(false);
        self.hold_reset.set(false);
    }

    fn update_for_control(&self, reset_event: bool) {
        self.hold_toggle
            .set_visible(self.control_kind() == ControlKind::Button);
        let previous_event = dropdown_value(&self.event_model, self.event_picker.selected());
        let control = self.control_kind();
        let numbers: Vec<String> = match control {
            ControlKind::Button => (1..=8).map(|n| n.to_string()).collect(),
            ControlKind::Encoder => (1..=4).map(|n| n.to_string()).collect(),
            // "Any" is the whole strip; 1 through 4 are its sections.
            ControlKind::Touch => ["Any", "1", "2", "3", "4"].map(str::to_owned).to_vec(),
        };
        replace_string_list(&self.control_number_model, &numbers);
        self.control_number_row.set_visible(true);
        let events: Vec<String> = events_for_control(control)
            .into_iter()
            .map(event_label)
            .map(str::to_owned)
            .collect();
        replace_string_list(&self.event_model, &events);
        if reset_event {
            self.event_picker.set_selected(0);
        } else if let Some(previous) = previous_event {
            select_string_or_first(&self.event_model, &self.event_picker, &previous);
        } else {
            self.event_picker.set_selected(0);
        }
    }

    fn select_control(
        &self,
        control: ControlKind,
        number: Option<u8>,
        reset_event: bool,
    ) -> Result<(), String> {
        // The catalog reselects its target before applying an action. Rebuilding
        // the number model for the same key emits selection notifications that
        // reset hold mode, incorrectly replacing the short action instead.
        if control == ControlKind::Button
            && self.control_kind() == control
            && self.control_number() == number
        {
            return Ok(());
        }
        let selected = match control {
            ControlKind::Button => {
                let number = number.ok_or_else(|| "Select a button number.".to_string())?;
                if !(1..=8).contains(&number) {
                    return Err("button number must be 1 through 8".into());
                }
                self.control_picker.set_selected(0);
                self.update_for_control(reset_event);
                number - 1
            }
            ControlKind::Encoder => {
                let number = number.ok_or_else(|| "Select an encoder number.".to_string())?;
                if !(1..=4).contains(&number) {
                    return Err("encoder number must be 1 through 4".into());
                }
                self.control_picker.set_selected(1);
                self.update_for_control(reset_event);
                number - 1
            }
            ControlKind::Touch => {
                let number = number.unwrap_or(0);
                if number > 4 {
                    return Err("touch section must be 0 (whole strip) through 4".into());
                }
                self.control_picker.set_selected(2);
                self.update_for_control(reset_event);
                number
            }
        };
        self.control_number_picker.set_selected(u32::from(selected));
        Ok(())
    }

    fn update_action_visibility(&self) {
        let kind = self.action_kind();
        self.return_toggle.set_visible(kind != ActionKind::None);
        self.return_row
            .set_visible(kind != ActionKind::None && self.return_toggle.is_active());
        self.feature_editor
            .show_direct_sound(self.action_picker.selected() == 6);
        self.action_guidance.set_text(match kind {
            ActionKind::None => {
                "Do nothing when this control is used. Live information can still be shown."
            }
            ActionKind::MoveToLayer => {
                "Switch to another profile or page when this control is used."
            }
            ActionKind::RunCommand => "Run a shell command with /bin/sh. Saving does not run it.",
            ActionKind::OpenApplication => "Enter the executable path and one argument per line.",
            ActionKind::Shortcut => "Type a key or full chord. The checkboxes add modifiers.",
            ActionKind::EmitLegacy => "Send a named internal event. Mainly used by older bindings.",
            ActionKind::Extended => "",
        });
        if matches!(kind, ActionKind::Shortcut) {
            self.action_guidance.set_markup(
                "Type a key or chord. <a href=\"shortcut-reference\">Key names and aliases</a>",
            );
        }
        // "Do nothing" needs no explaining on a key, whose selectors are hidden.
        let idle_key = kind == ActionKind::None && self.control_kind() == ControlKind::Button;
        self.action_guidance
            .set_visible(!matches!(kind, ActionKind::Extended) && !idle_key);
        if let Some(label) = self
            .text_row
            .first_child()
            .and_then(|child| child.downcast::<Label>().ok())
        {
            label.set_text(match kind {
                ActionKind::RunCommand => "Shell command",
                ActionKind::Shortcut => "Shortcut",
                ActionKind::EmitLegacy => "Event name",
                ActionKind::None
                | ActionKind::MoveToLayer
                | ActionKind::OpenApplication
                | ActionKind::Extended => "",
            });
        }
        self.feature_editor
            .root
            .set_visible(matches!(kind, ActionKind::Extended));
        self.layer_target_row
            .set_visible(matches!(kind, ActionKind::MoveToLayer));
        self.application_row
            .set_visible(matches!(kind, ActionKind::OpenApplication));
        self.args_row
            .set_visible(matches!(kind, ActionKind::OpenApplication));
        let shortcut = matches!(kind, ActionKind::Shortcut);
        self.modifier_row.set_visible(shortcut);
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
            | ActionKind::Extended => "",
        }));
    }

    fn refresh_layer_targets(&self, names: &[String]) {
        replace_string_list(&self.layer_target_model, names);
        self.layer_target_picker.set_selected(0);
        replace_string_list(&self.return_model, names);
        self.return_target.set_selected(0);
    }

    fn load_selected_binding(&self, layers: &RefCell<LayerStore>) {
        self.undo_remove.set_visible(false);
        *self.removed.borrow_mut() = None;
        self.feature_editor
            .set_touchscreen(self.control_kind() == ControlKind::Touch);
        self.clear_action_fields();
        let Ok((layer, trigger)) = self.selected_layer_and_trigger(layers) else {
            self.status.set_text("Select a valid control event.");
            self.mark_clean();
            return;
        };
        let action = {
            let store = layers.borrow();
            existing_action(&store, &layer, &trigger)
        };
        match action {
            Some(action) => {
                // The editor names the real file, not our link to it.
                let action =
                    sdeck_plus::asset_links::unlink_action(layers.borrow().root(), &action);
                self.populate_action(&action);
                self.populate_return(layers.borrow().page_return(&layer, &trigger));
                let store = layers.borrow();
                self.populate_info(
                    store.info_source(&layer, &trigger),
                    &sdeck_plus::info::load_location(store.root()).unwrap_or_default(),
                );
                self.status.set_text("Binding loaded for editing.");
            }
            None => {
                self.populate_action(&Action::None);
                self.status
                    .set_text("No binding exists for that control event.");
            }
        }
        self.update_action_visibility();
        self.mark_clean();
    }

    /// Snapshot of every editable field, compared with the last loaded or saved state.
    fn fingerprint(&self) -> String {
        let buffer = self.args_view.buffer();
        [
            self.hold_toggle.is_active().to_string(),
            self.action_picker.selected().to_string(),
            self.layer_target_picker.selected().to_string(),
            self.text_entry.text().to_string(),
            self.application_entry.text().to_string(),
            buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string(),
            self.shortcut_modifiers().join("+"),
            self.return_toggle.is_active().to_string(),
            self.return_target.selected().to_string(),
            self.return_seconds.text().to_string(),
            self.info_toggle.is_active().to_string(),
            self.info_source.selected().to_string(),
            self.info_location.text().to_string(),
            self.feature_editor.snapshot(),
        ]
        .join("\u{1f}")
    }

    /// Treat the current fields as saved: later edits are measured against them.
    fn mark_clean(&self) {
        *self.baseline.borrow_mut() = Some(self.fingerprint());
    }

    fn on_edited(&self) {
        self.update_shortcut_preview();
        if self.is_dirty() {
            self.status.set_text("Unsaved changes.");
            // Set after the text: any status change clears earlier feedback tones.
            self.status.add_css_class("feedback-warning");
        }
    }

    /// True when an editor field differs from the binding last loaded or saved.
    fn is_dirty(&self) -> bool {
        self.baseline
            .borrow()
            .as_ref()
            .is_some_and(|saved| *saved != self.fingerprint())
    }

    fn update_shortcut_preview(&self) {
        let text = match sdeck_plus::keyboard::parse_chord(
            &self.shortcut_modifiers(),
            &self.text_entry.text(),
        ) {
            Ok(chord) if self.action_kind() == ActionKind::Shortcut => {
                format!("Presses {}", chord.label())
            }
            _ => String::new(),
        };
        self.shortcut_preview.set_text(&text);
        self.shortcut_preview.set_visible(!text.is_empty());
    }

    fn connect_dirty_tracking(&self) {
        let notify = {
            let ui = self.clone();
            move || ui.on_edited()
        };
        for entry in [
            &self.text_entry,
            &self.application_entry,
            &self.return_seconds,
            &self.info_location,
        ] {
            let notify = notify.clone();
            entry.connect_changed(move |_| notify());
        }
        for picker in [
            &self.action_picker,
            &self.layer_target_picker,
            &self.return_target,
            &self.info_source,
        ] {
            let notify = notify.clone();
            picker.connect_selected_notify(move |_| notify());
        }
        for toggle in [
            &self.super_mod,
            &self.shift_mod,
            &self.ctrl_mod,
            &self.alt_mod,
        ] {
            let notify = notify.clone();
            toggle.connect_toggled(move |_| notify());
        }
        for check in [&self.return_toggle, &self.info_toggle] {
            let notify = notify.clone();
            check.connect_toggled(move |_| notify());
        }
        let args_notify = notify.clone();
        self.args_view
            .buffer()
            .connect_changed(move |_| args_notify());
        self.feature_editor.connect_changed(notify);
    }

    /// Selected control number; 0 means the whole touch strip.
    fn control_number(&self) -> Option<u8> {
        let value = dropdown_value(
            &self.control_number_model,
            self.control_number_picker.selected(),
        )?;
        if value == "Any" {
            Some(0)
        } else {
            value.parse().ok()
        }
    }

    fn selected_layer_and_trigger(
        &self,
        layers: &RefCell<LayerStore>,
    ) -> Result<(String, Trigger), String> {
        let layer = dropdown_value(&self.layer_model, self.layer_picker.selected())
            .ok_or_else(|| "Select a profile or page.".to_string())?;
        if !layers.borrow().layers().contains_key(&layer) {
            return Err("Selected profile or page no longer exists.".into());
        }
        let number = self
            .control_number()
            .ok_or_else(|| "Select a control number.".to_string())?;
        let trigger = if self.control_kind() == ControlKind::Button {
            sdeck_plus::key_hold_editor::key_trigger(number, self.hold_toggle.is_active())
        } else {
            trigger_from_selection(self.control_kind(), number, self.event_kind())
        }
        .map_err(|error| error.0)?;
        let owner = layers
            .borrow()
            .binding_layer(&layer, &trigger)
            .map(|data| data.name.clone())
            .unwrap_or(layer);
        Ok((owner, trigger))
    }

    fn binding_from_widgets(
        &self,
        layers: &RefCell<LayerStore>,
    ) -> Result<(String, Trigger, Action), String> {
        let (layer, trigger) = self.selected_layer_and_trigger(layers)?;
        if matches!(self.action_kind(), ActionKind::Extended) {
            return Ok((
                layer,
                trigger,
                Action::Feature(self.feature_editor.parse()?),
            ));
        }
        let destination = dropdown_value(
            &self.layer_target_model,
            self.layer_target_picker.selected(),
        )
        .unwrap_or_default();
        let args_buffer = self.args_view.buffer();
        let args = args_buffer
            .text(&args_buffer.start_iter(), &args_buffer.end_iter(), false)
            .to_string();
        let modifiers = self.shortcut_modifiers();
        let action = {
            let store = layers.borrow();
            action_from_fields(
                &store,
                self.action_kind(),
                &destination,
                &self.application_entry.text(),
                &args,
                &self.text_entry.text(),
                &modifiers,
            )
            .map_err(|error| error.0)?
        };
        Ok((layer, trigger, action))
    }

    fn page_return_from_widgets(&self) -> Result<Option<sdeck_plus::layers::PageReturn>, String> {
        if self.action_kind() == ActionKind::None || !self.return_toggle.is_active() {
            return Ok(None);
        }
        let layer = dropdown_value(&self.return_model, self.return_target.selected())
            .ok_or("Select the page to return to.")?;
        let seconds = self
            .return_seconds
            .text()
            .parse()
            .map_err(|error| format!("Return delay in seconds: {error}"))?;
        let value = sdeck_plus::layers::PageReturn { layer, seconds };
        value.validate().map_err(|error| error.to_string())?;
        Ok(Some(value))
    }

    fn populate_return(&self, value: Option<sdeck_plus::layers::PageReturn>) {
        match &value {
            Some(value) => select_string(&self.return_model, &self.return_target, &value.layer),
            None => self.return_target.set_selected(0),
        }
        self.return_seconds
            .set_text(&value.as_ref().map_or(60, |value| value.seconds).to_string());
        self.return_toggle.set_active(value.is_some());
    }

    fn clear_action_fields(&self) {
        self.populate_info(None, "");
        self.populate_return(None);
        self.feature_editor.reset();
        self.text_entry.set_text("");
        self.application_entry.set_text("");
        let buffer = self.args_view.buffer();
        buffer.set_text("");
        self.super_mod.set_active(false);
        self.shift_mod.set_active(false);
        self.ctrl_mod.set_active(false);
        self.alt_mod.set_active(false);
    }

    fn populate_action(&self, action: &Action) {
        match action {
            Action::None => self.action_picker.set_selected(7),
            Action::Feature(feature) => {
                self.action_picker.set_selected(
                    if matches!(
                        feature,
                        sdeck_plus::features::FeatureAction::Desktop(
                            sdeck_plus::desktop_actions::DesktopAction::PlayAudio { .. }
                        )
                    ) {
                        6
                    } else {
                        5
                    },
                );
                self.feature_editor.populate(feature);
            }
            Action::SwitchLayer(target) => {
                self.action_picker.set_selected(0);
                select_string(&self.layer_target_model, &self.layer_target_picker, target);
            }
            Action::Command(command) => {
                self.action_picker.set_selected(1);
                self.text_entry.set_text(command);
            }
            Action::KeyPress { key } => {
                // Inspect legacy names verbatim; only an explicit save canonicalizes them.
                self.action_picker.set_selected(3);
                self.text_entry.set_text(key);
                self.super_mod.set_active(false);
                self.shift_mod.set_active(false);
                self.ctrl_mod.set_active(false);
                self.alt_mod.set_active(false);
            }
            Action::Launch { executable, args } => {
                self.action_picker.set_selected(2);
                self.application_entry
                    .set_text(&executable.to_string_lossy());
                self.args_view
                    .buffer()
                    .set_text(&format_application_args(args));
            }
            Action::Shortcut { modifiers, key } => {
                self.action_picker.set_selected(3);
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
            Action::Emit(name) => {
                self.action_picker.set_selected(4);
                self.text_entry.set_text(name);
            }
        }
        self.update_action_visibility();
    }

    fn control_kind(&self) -> ControlKind {
        match self.control_picker.selected() {
            1 => ControlKind::Encoder,
            2 => ControlKind::Touch,
            _ => ControlKind::Button,
        }
    }

    fn event_kind(&self) -> EventKind {
        let label = dropdown_value(&self.event_model, self.event_picker.selected())
            .unwrap_or_else(|| "Press".into());
        if label == "Press" && matches!(self.control_kind(), ControlKind::Encoder) {
            EventKind::EncoderPress
        } else {
            event_from_label(&label)
        }
    }

    fn action_kind(&self) -> ActionKind {
        match self.action_picker.selected() {
            1 => ActionKind::RunCommand,
            2 => ActionKind::OpenApplication,
            3 => ActionKind::Shortcut,
            4 => ActionKind::EmitLegacy,
            5 | 6 => ActionKind::Extended,
            7 => ActionKind::None,
            _ => ActionKind::MoveToLayer,
        }
    }

    fn shortcut_modifiers(&self) -> Vec<String> {
        let mut modifiers = Vec::new();
        if self.super_mod.is_active() {
            modifiers.push("SUPER".into());
        }
        if self.shift_mod.is_active() {
            modifiers.push("SHIFT".into());
        }
        if self.ctrl_mod.is_active() {
            modifiers.push("CTRL".into());
        }
        if self.alt_mod.is_active() {
            modifiers.push("ALT".into());
        }
        modifiers
    }
}

// Layer names remain lossless in the model/editor; they must not resize the canvas.
fn bound_layer_picker(picker: &gtk::DropDown) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let label = Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_width_chars(14);
        label.set_max_width_chars(18);
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let value = item
            .item()
            .unwrap()
            .downcast::<gtk::StringObject>()
            .unwrap();
        let label = item.child().unwrap().downcast::<Label>().unwrap();
        label.set_text(&value.string());
        label.set_tooltip_text(Some(&value.string()));
    });
    picker.set_factory(Some(&factory));
    picker.set_list_factory(Some(&factory));
}

/// A shortcut modifier as a named toggle; it joins a linked row in the editor.
fn modifier_toggle(name: &str) -> gtk::ToggleButton {
    let toggle = gtk::ToggleButton::with_label(name);
    toggle.set_tooltip_text(Some(&format!("Hold {name} with the key")));
    toggle
}

/// Small section heading inside the inspector (Options, Appearance).
fn inspector_heading(text: &str) -> Label {
    let heading = Label::new(Some(text));
    heading.set_xalign(0.0);
    heading.add_css_class("inspector-heading");
    heading
}

fn labeled(label: &str, child: &impl IsA<gtk::Widget>) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 4);
    let label = Label::new(Some(label));
    label.set_wrap(true);
    label.set_max_width_chars(32);
    label.add_css_class("muted");
    label.add_css_class("field-label");
    label.set_xalign(0.0);
    label.set_mnemonic_widget(Some(child));
    child
        .as_ref()
        .update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
    row.append(&label);
    row.append(child);
    row
}

fn replace_string_list(model: &gtk::StringList, values: &[String]) {
    while model.n_items() > 0 {
        model.remove(0);
    }
    for value in values {
        model.append(value);
    }
}

fn dropdown_value(model: &gtk::StringList, selected: u32) -> Option<String> {
    model.string(selected).map(|value| value.to_string())
}

fn select_string(model: &gtk::StringList, picker: &gtk::DropDown, value: &str) {
    for index in 0..model.n_items() {
        if model.string(index).as_deref() == Some(value) {
            picker.set_selected(index);
            return;
        }
    }
}

fn select_string_or_first(model: &gtk::StringList, picker: &gtk::DropDown, value: &str) {
    select_string(model, picker, value);
    if picker.selected() >= model.n_items()
        || model.string(picker.selected()).as_deref() != Some(value)
    {
        picker.set_selected(0);
    }
}

fn control_heading(control: ControlKind, number: Option<u8>) -> String {
    match control {
        ControlKind::Button => format!("Configuring: Key {}", number.unwrap_or(1)),
        ControlKind::Encoder => format!("Configuring: Dial {}", number.unwrap_or(1)),
        ControlKind::Touch => match number {
            Some(zone @ 1..=4) => format!("Configuring: Touchpad section {zone}"),
            _ => "Configuring: Touchpad".into(),
        },
    }
}

fn set_selected_control(widgets: &[gtk::Widget], selected_index: usize) {
    for (index, widget) in widgets.iter().enumerate() {
        if index == selected_index {
            widget.add_css_class("control-selected");
        } else {
            widget.remove_css_class("control-selected");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn connect_control_selection(
    _live_label: &Label,
    button: Button,
    ui: &BindingUi,
    layers: &Rc<RefCell<LayerStore>>,
    heading: &Label,
    selected_widgets: Rc<Vec<gtk::Widget>>,
    selected_index: usize,
    control: ControlKind,
    number: u8,
) {
    let ui = ui.clone();
    let layers = layers.clone();
    let heading = heading.clone();
    let guard = ui.clone();
    let switch: Rc<dyn Fn()> = Rc::new(move || {
        if let Err(error) = ui.select_control(control, Some(number), false) {
            ui.status.set_text(&error);
            return;
        }
        set_selected_control(&selected_widgets, selected_index);
        heading.set_text(&control_heading(control, Some(number)));
        ui.load_selected_binding(&layers);
    });
    button.connect_clicked(move |button| {
        if guard.is_dirty() {
            confirm_discard(button, switch.clone());
        } else {
            switch();
        }
    });
}

/// Selecting another control reloads the editor, so unsaved edits need a choice first.
fn confirm_discard(anchor: &Button, proceed: Rc<dyn Fn()>) {
    let Some(parent) = anchor
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        proceed();
        return;
    };
    let dialog = gtk::MessageDialog::builder()
        .transient_for(&parent)
        .modal(true)
        .destroy_with_parent(true)
        .message_type(gtk::MessageType::Question)
        .text("Discard unsaved changes?")
        .secondary_text(
            "You edited this binding without saving it. Selecting another control discards those edits.",
        )
        .build();
    dialog.add_css_class("confirm-dialog");
    let mut child = dialog.message_area().first_child();
    while let Some(widget) = child {
        if let Some(label) = widget.downcast_ref::<Label>() {
            label.set_justify(gtk::Justification::Center);
        }
        child = widget.next_sibling();
    }
    dialog.set_resizable(false);
    dialog.set_default_size(360, -1);
    dialog.add_button("Keep editing", gtk::ResponseType::Cancel);
    dialog
        .add_button("Discard changes", gtk::ResponseType::Accept)
        .add_css_class("destructive-action");
    dialog.set_default_response(gtk::ResponseType::Cancel);
    dialog.connect_response(move |dialog, response| {
        dialog.close();
        if response == gtk::ResponseType::Accept {
            proceed();
        }
    });
    dialog.present();
}

fn event_label(event: EventKind) -> &'static str {
    match event {
        EventKind::ButtonPress | EventKind::EncoderPress => "Press",
        EventKind::EncoderClockwise => "Clockwise",
        EventKind::EncoderCounterClockwise => "Counterclockwise",
        EventKind::TouchTap => "Tap",
        EventKind::TouchPress => "Press and hold",
        EventKind::TouchFlick => "Generic swipe/flick",
        EventKind::TouchSwipeLeft => "Swipe left",
        EventKind::TouchSwipeRight => "Swipe right",
        EventKind::TouchSwipeUp => "Swipe up",
        EventKind::TouchSwipeDown => "Swipe down",
    }
}

fn event_from_label(label: &str) -> EventKind {
    match label {
        "Clockwise" => EventKind::EncoderClockwise,
        "Counterclockwise" => EventKind::EncoderCounterClockwise,
        "Tap" => EventKind::TouchTap,
        "Press and hold" => EventKind::TouchPress,
        "Generic swipe/flick" => EventKind::TouchFlick,
        "Swipe left" => EventKind::TouchSwipeLeft,
        "Swipe right" => EventKind::TouchSwipeRight,
        "Swipe up" => EventKind::TouchSwipeUp,
        "Swipe down" => EventKind::TouchSwipeDown,
        "Press" => EventKind::ButtonPress,
        _ => EventKind::ButtonPress,
    }
}

fn select_layer(layers: &RefCell<LayerStore>, index: usize) -> Option<LayerStore> {
    let name = {
        let store = layers.borrow();
        store.names().get(index).cloned()
    }?;
    let mut store = layers.borrow_mut();
    if store.select(&name) {
        Some(store.clone())
    } else {
        None
    }
}

fn binding_identity(layer: &str, trigger: &Trigger) -> String {
    format!("{}:{layer}:{trigger:?}", layer.len())
}

fn matching_binding(
    layers: &RefCell<LayerStore>,
    event: &protocol::Event,
) -> Option<(String, Action, Option<sdeck_plus::layers::PageReturn>)> {
    let store = layers.borrow();
    store.matching_binding(event).map(|(trigger, action)| {
        let page_return = store.page_return(store.active(), &trigger);
        (
            binding_identity(
                store
                    .binding_layer(store.active(), &trigger)
                    .map_or(store.active(), |data| data.name.as_str()),
                &trigger,
            ),
            action,
            page_return,
        )
    })
}

#[cfg(test)]
fn matching_action(layers: &RefCell<LayerStore>, event: &protocol::Event) -> Option<Action> {
    let action = {
        let store = layers.borrow();
        store.matching_action(event)
    };
    action
}

fn layer_index(layers: &RefCell<LayerStore>, target: &str) -> u32 {
    let index = {
        let store = layers.borrow();
        store
            .names()
            .iter()
            .position(|name| name == target)
            .unwrap_or(0)
    };
    index as u32
}

#[cfg(test)]
fn bind_selected_layer(
    layers: &RefCell<LayerStore>,
    index: usize,
    trigger: Trigger,
    action: Action,
) -> Option<LayerStore> {
    let name = {
        let store = layers.borrow();
        store.names().get(index).cloned()
    }?;
    let mut store = layers.borrow_mut();
    if store.bind(&name, trigger, action).is_ok() {
        Some(store.clone())
    } else {
        None
    }
}

#[cfg(test)]
fn remove_selected_layer(layers: &RefCell<LayerStore>, index: usize) -> Option<LayerStore> {
    let name = {
        let store = layers.borrow();
        store.names().get(index).cloned()
    }?;
    let mut store = layers.borrow_mut();
    if store.remove(&name).unwrap_or(false) {
        Some(store.clone())
    } else {
        None
    }
}

struct ChooserOwner {
    target: ArtworkTarget,
    name: String,
    incarnation: Rc<()>,
}

#[derive(Clone)]
struct ArtworkUi {
    store: Rc<RefCell<Option<ArtworkStore>>>,
    layers: Rc<RefCell<LayerStore>>,
    pictures: Rc<RefCell<Vec<(ArtworkTarget, Picture)>>>,
    output: mpsc::Sender<ArtworkCommand>,
    last_layer: Rc<RefCell<Option<String>>>,
    chooser_owners: Rc<RefCell<std::collections::BTreeMap<String, Rc<()>>>>,
    selected_target: Rc<Cell<ArtworkTarget>>,
    text_editor: Rc<RefCell<std::rc::Weak<button_text_editor::ButtonTextEditor>>>,
    /// Latest reading of every live information source.
    info: Rc<RefCell<InfoState>>,
}

/// Latest reading of every live information source (the map it derefs to),
/// plus the preview's scrolling Now playing titles.
#[derive(Default)]
struct InfoState {
    values: std::collections::BTreeMap<sdeck_plus::info::InfoSource, sdeck_plus::info::InfoValue>,
    marquee: PreviewMarquee,
    countdown: Option<timer_artwork::CountdownArtwork>,
}

impl std::ops::Deref for InfoState {
    type Target =
        std::collections::BTreeMap<sdeck_plus::info::InfoSource, sdeck_plus::info::InfoValue>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl std::ops::DerefMut for InfoState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

/// Preview pictures whose title scrolls, redrawn by one ~10 fps GTK timeout
/// that exists only while at least one title scrolls.
#[derive(Default)]
struct PreviewMarquee {
    running: std::collections::HashMap<ArtworkTarget, PreviewTitle>,
    timer: Option<glib::SourceId>,
}

struct PreviewTitle {
    value: sdeck_plus::info::InfoValue,
    /// Title and column widths from `info_render::marquee_extent`.
    extent: (f64, f64),
    started: std::time::Instant,
    /// Whole-pixel offset last drawn.
    offset: f64,
}

impl ArtworkUi {
    /// Keys and touch sections that show live information on `layer`.
    fn info_targets(&self, layer: &str) -> Vec<(ArtworkTarget, sdeck_plus::info::InfoSource)> {
        let store = self.layers.borrow();
        let keys = (1..=8).filter_map(|key| {
            let source = store.info_source(layer, &Trigger::KeyPress(key))?;
            Some((ArtworkTarget::Keypad(key), source))
        });
        let zones = (1..=4).filter_map(|zone| {
            let source = store.zone_info_source(layer, zone)?;
            Some((ArtworkTarget::EncoderZone(zone), source))
        });
        keys.chain(zones).collect()
    }

    fn info_value(&self, source: sdeck_plus::info::InfoSource) -> sdeck_plus::info::InfoValue {
        self.info
            .borrow()
            .get(&source)
            .cloned()
            .unwrap_or_else(|| source.placeholder())
    }

    /// The target's own artwork, drawn under its live picture.
    fn info_background(&self, layer: &str, target: ArtworkTarget) -> Option<std::path::PathBuf> {
        let layers = self.layers.borrow();
        let owner = layers.artwork_owner(layer, target);
        self.store
            .borrow()
            .as_ref()?
            .assigned_path_in_layer(owner, target)
    }

    fn indicated_zone(&self, layer: &str, zone: u8) -> bool {
        let (previous, next) = self.layers.borrow().touchscreen_navigation(layer);
        (zone == 1 && previous) || (zone == 4 && next)
    }

    fn touchscreen_zone_command(&self, layer: &str, zone: u8) -> ArtworkCommand {
        let (previous, next) = self.layers.borrow().touchscreen_navigation(layer);
        let source = self.layers.borrow().zone_info_source(layer, zone);
        ArtworkCommand::ComposeTouchscreenZone {
            zone,
            path: self.info_background(layer, ArtworkTarget::EncoderZone(zone)),
            strip_path: self.info_background(layer, ArtworkTarget::TouchStripBackground),
            value: source.map(|source| self.info_value(source)),
            previous,
            next,
        }
    }

    fn info_command(
        &self,
        layer: &str,
        target: ArtworkTarget,
        source: sdeck_plus::info::InfoSource,
    ) -> ArtworkCommand {
        if let ArtworkTarget::EncoderZone(zone) = target {
            if self.indicated_zone(layer, zone) {
                return self.touchscreen_zone_command(layer, zone);
            }
        }
        self.info_value_command(layer, target, self.info_value(source))
    }

    fn info_value_command(
        &self,
        layer: &str,
        target: ArtworkTarget,
        value: sdeck_plus::info::InfoValue,
    ) -> ArtworkCommand {
        ArtworkCommand::ComposeInfo {
            target,
            path: self.info_background(layer, target),
            strip_path: if matches!(target, ArtworkTarget::EncoderZone(_)) {
                self.info_background(layer, ArtworkTarget::TouchStripBackground)
            } else {
                None
            },
            value,
        }
    }

    /// Redraw keys and touch sections that show information over their own
    /// artwork; the sections go last so they draw over the strip background.
    fn with_info(&self, layer: &str, commands: Vec<ArtworkCommand>) -> Vec<ArtworkCommand> {
        let targets = self.info_targets(layer);
        let shown = |target: &ArtworkTarget| targets.iter().any(|(shown, _)| shown == target);
        let info = |wanted: fn(&ArtworkTarget) -> bool| -> Vec<ArtworkCommand> {
            targets
                .iter()
                .filter(|(target, _)| wanted(target))
                .map(|(target, source)| self.info_command(layer, *target, *source))
                .collect()
        };
        let mut other = Vec::new();
        let mut touch = Vec::new();
        for command in commands {
            let target = match &command {
                ArtworkCommand::Assign { target, .. } | ArtworkCommand::Clear { target } => *target,
                ArtworkCommand::ComposeButton { key, .. } => ArtworkTarget::Keypad(*key),
                _ => {
                    other.push(command);
                    continue;
                }
            };
            if shown(&target) {
                continue;
            }
            if matches!(target, ArtworkTarget::Keypad(_)) {
                other.push(command);
            } else {
                touch.push(command);
            }
        }
        other.extend(info(|target| matches!(target, ArtworkTarget::Keypad(_))));
        other.extend(touch);
        other.extend(info(|target| {
            matches!(target, ArtworkTarget::EncoderZone(_))
        }));
        for zone in [1, 4] {
            if self.indicated_zone(layer, zone) {
                other.retain(|command| !matches!(command,
                    ArtworkCommand::Assign { target: ArtworkTarget::EncoderZone(shown), .. }
                    | ArtworkCommand::Clear { target: ArtworkTarget::EncoderZone(shown) }
                    | ArtworkCommand::ComposeInfo { target: ArtworkTarget::EncoderZone(shown), .. }
                    | ArtworkCommand::ComposeTouchscreenZone { zone: shown, .. } if *shown == zone
                ));
                other.push(self.touchscreen_zone_command(layer, zone));
            }
        }
        self.with_countdown(layer, self.with_icons(layer, other))
    }

    /// Replace the pictures of keys and touch sections without artwork by
    /// their binding icon, exactly where the preview shows one. Keys keep
    /// their position in the snapshot; sections go after the strip
    /// background, and an indicated section's icon command carries its page
    /// indicators instead of a separate indicator command.
    fn with_icons(&self, layer: &str, mut commands: Vec<ArtworkCommand>) -> Vec<ArtworkCommand> {
        let strip_path = self.info_background(layer, ArtworkTarget::TouchStripBackground);
        for target in (1..=8)
            .map(ArtworkTarget::Keypad)
            .chain((1..=4).map(ArtworkTarget::EncoderZone))
        {
            let Some(icon) = self.control_icon(layer, target) else {
                continue;
            };
            let text = self
                .store
                .borrow()
                .as_ref()
                .and_then(|store| store.button_text_in_layer(layer, target).cloned());
            if icon.icon != BindingIcon::MissingFile
                && !sdeck_plus::binding_icons::key_has_room(text.as_ref())
            {
                continue;
            }
            let command = ArtworkCommand::ComposeIcon {
                target,
                icon: icon.icon,
                caption: icon.caption,
                text,
                strip_path: matches!(target, ArtworkTarget::EncoderZone(_))
                    .then(|| strip_path.clone())
                    .flatten(),
                previous: icon.previous,
                next: icon.next,
            };
            let replaced = commands.iter().position(|existing| match existing {
                ArtworkCommand::Assign { target: shown, .. }
                | ArtworkCommand::Clear { target: shown } => *shown == target,
                ArtworkCommand::ComposeButton { key, .. } => target == ArtworkTarget::Keypad(*key),
                ArtworkCommand::ComposeTouchscreenZone { zone, .. } => {
                    target == ArtworkTarget::EncoderZone(*zone)
                }
                _ => false,
            });
            match replaced {
                Some(index) => commands[index] = command,
                None => commands.push(command),
            }
        }
        commands
    }

    /// Draw the same picture in the window's preview of `target`.
    fn paint_info_preview(&self, target: ArtworkTarget, value: &sdeck_plus::info::InfoValue) {
        self.paint_info_preview_at(target, value, std::time::Instant::now());
    }

    /// [`Self::paint_info_preview`] as of `now`: a scrolling title is drawn at
    /// its marquee phase, and keeps scrolling until something else shows.
    fn paint_info_preview_at(
        &self,
        target: ArtworkTarget,
        value: &sdeck_plus::info::InfoValue,
        now: std::time::Instant,
    ) {
        let layer = self.layers.borrow().active().to_owned();
        let indicators = |zone: u8| {
            let (previous, next) = self.layers.borrow().touchscreen_navigation(&layer);
            (zone == 1 && previous, zone == 4 && next)
        };
        let section = match target {
            ArtworkTarget::Keypad(_) => None,
            ArtworkTarget::EncoderZone(zone) => Some(indicators(zone)),
            ArtworkTarget::TouchStripBackground => return,
        };
        let elapsed = self.preview_marquee_phase(target, value, section, now);
        if let ArtworkTarget::EncoderZone(zone) = target {
            if self.indicated_zone(&layer, zone) {
                self.paint_touchscreen_zone(&layer, zone, Some(value), elapsed);
                return;
            }
        }
        let background = self.info_background(&layer, target);
        let image = match target {
            ArtworkTarget::Keypad(_) => sdeck_plus::info_render::compose_info_image_at(
                value,
                background.as_deref(),
                elapsed,
            ),
            ArtworkTarget::EncoderZone(_) => sdeck_plus::info_render::compose_zone_image_at(
                value,
                background.as_deref(),
                false,
                false,
                elapsed,
            ),
            ArtworkTarget::TouchStripBackground => return,
        };
        match image {
            Ok(image) => {
                let pictures = self.pictures.borrow();
                if let Some((_, picture)) = pictures.iter().find(|(shown, _)| *shown == target) {
                    picture.set_paintable(Some(&gtk::gdk::Texture::for_pixbuf(&image)));
                    picture.set_visible(true);
                }
            }
            Err(error) => eprintln!("Could not draw live information preview: {error}"),
        }
    }

    /// Follow the title `target` now shows: start or keep its marquee (and
    /// the timer) when it scrolls, forget it otherwise. Returns its phase.
    fn preview_marquee_phase(
        &self,
        target: ArtworkTarget,
        value: &sdeck_plus::info::InfoValue,
        section: Option<(bool, bool)>,
        now: std::time::Instant,
    ) -> Duration {
        let extent = sdeck_plus::info_render::marquee_extent(value, section);
        let mut info = self.info.borrow_mut();
        let marquee = &mut info.marquee;
        let Some(extent) = extent else {
            marquee.running.remove(&target);
            return Duration::ZERO;
        };
        let title = marquee
            .running
            .entry(target)
            .and_modify(|title| {
                if title.value != *value || title.extent != extent {
                    title.value = value.clone();
                    title.extent = extent;
                    title.started = now;
                }
            })
            .or_insert_with(|| PreviewTitle {
                value: value.clone(),
                extent,
                started: now,
                offset: 0.0,
            });
        let elapsed = now.saturating_duration_since(title.started);
        title.offset = sdeck_plus::info_render::marquee_offset(extent.0, extent.1, elapsed).round();
        let idle = marquee.timer.is_none();
        drop(info);
        if idle {
            let ui = self.clone();
            let timer = glib::timeout_add_local(Duration::from_millis(100), move || {
                ui.tick_preview_marquee(std::time::Instant::now())
            });
            self.info.borrow_mut().marquee.timer = Some(timer);
        }
        elapsed
    }

    /// Redraw the preview titles whose position moved by `now`. The timer
    /// stops once no title scrolls or their window is gone.
    fn tick_preview_marquee(&self, now: std::time::Instant) -> glib::ControlFlow {
        let layer = self.layers.borrow().active().to_owned();
        let shown = self.info_targets(&layer);
        let running: Vec<_> = self
            .info
            .borrow()
            .marquee
            .running
            .iter()
            .map(|(target, title)| (*target, title.extent, title.started, title.offset))
            .collect();
        for (target, (full, column), started, offset) in running {
            let alive = self
                .pictures
                .borrow()
                .iter()
                .any(|(shown, picture)| *shown == target && picture.root().is_some());
            let source = shown
                .iter()
                .find(|(shown, _)| *shown == target)
                .map(|(_, source)| *source);
            match source.filter(|_| alive && !self.countdown_shows(&layer, target)) {
                Some(source) => {
                    let elapsed = now.saturating_duration_since(started);
                    let moved =
                        sdeck_plus::info_render::marquee_offset(full, column, elapsed).round();
                    // A window hidden in the tray keeps its pictures; skip
                    // drawing them until they are shown again.
                    let visible = self
                        .pictures
                        .borrow()
                        .iter()
                        .any(|(shown, picture)| *shown == target && picture.is_mapped());
                    if visible && moved != offset {
                        self.paint_info_preview_at(target, &self.info_value(source), now);
                    }
                }
                None => {
                    self.info.borrow_mut().marquee.running.remove(&target);
                }
            }
        }
        let mut info = self.info.borrow_mut();
        if info.marquee.running.is_empty() {
            // Returning Break removes the source; only forget its id.
            info.marquee.timer = None;
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    }

    fn paint_touchscreen_zone(
        &self,
        layer: &str,
        zone: u8,
        value: Option<&sdeck_plus::info::InfoValue>,
        elapsed: Duration,
    ) {
        let target = ArtworkTarget::EncoderZone(zone);
        let background = self.info_background(layer, target);
        let (previous, next) = self.layers.borrow().touchscreen_navigation(layer);
        let icon = value
            .is_none()
            .then(|| self.control_icon(layer, target))
            .flatten();
        match sdeck_plus::image_output::compose_touchscreen_zone_at(
            zone,
            background.as_deref(),
            value,
            previous,
            next,
            elapsed,
        )
        .and_then(|indicators| match &icon {
            // The page indicators still paint last, over the binding icon.
            Some(icon) => with_icon_under(icon, &indicators),
            None => Ok(indicators),
        }) {
            Ok(image) => {
                if let Some((_, picture)) = self
                    .pictures
                    .borrow()
                    .iter()
                    .find(|(shown, _)| *shown == target)
                {
                    picture.set_paintable(Some(&gtk::gdk::Texture::for_pixbuf(&image)));
                    picture.set_visible(true);
                }
            }
            Err(error) => eprintln!("Could not draw touchscreen page indicator: {error}"),
        }
    }

    fn paint_navigation_previews(&self, layer: &str) {
        for zone in [1, 4] {
            // Information previews already paint their indicators with their value.
            if self.indicated_zone(layer, zone)
                && self.layers.borrow().zone_info_source(layer, zone).is_none()
            {
                self.paint_touchscreen_zone(layer, zone, None, Duration::ZERO);
            }
        }
    }

    fn paint_info_previews(&self, layer: &str) {
        for (target, source) in self.info_targets(layer) {
            if !self.countdown_shows(layer, target) {
                self.paint_info_preview(target, &self.info_value(source));
            }
        }
        self.paint_countdown_preview(layer);
    }

    /// Store a new reading and redraw only what shows it, on the device and in the window.
    fn update_info(
        &self,
        source: sdeck_plus::info::InfoSource,
        value: sdeck_plus::info::InfoValue,
    ) {
        let previous = self.info.borrow_mut().insert(source, value.clone());
        if previous.as_ref() == Some(&value) {
            return;
        }
        let layer = self.layers.borrow().active().to_owned();
        for (target, shown) in self.info_targets(&layer) {
            if shown == source && !self.countdown_shows(&layer, target) {
                self.paint_info_preview(target, &value);
                let _ = self.output.send(self.info_command(&layer, target, source));
            }
        }
    }

    fn capture_owner(&self) -> ChooserOwner {
        let name = {
            let layers = self.layers.borrow();
            layers
                .artwork_owner(layers.active(), self.selected_target.get())
                .to_owned()
        };
        let incarnation = self
            .chooser_owners
            .borrow_mut()
            .entry(name.clone())
            .or_insert_with(|| Rc::new(()))
            .clone();
        ChooserOwner {
            target: self.selected_target.get(),
            name,
            incarnation,
        }
    }

    #[cfg(test)]
    fn remove_layer(&self, index: usize) -> Result<Option<LayerStore>, String> {
        let name = self.layers.borrow().names().get(index).cloned();
        let removed = remove_layer_with_artwork(&self.layers, &mut self.store.borrow_mut(), index)?;
        if removed.is_some() {
            if let Some(name) = name {
                // Open choosers retain the old token; recreation gets a new one.
                self.chooser_owners.borrow_mut().remove(&name);
            }
        }
        Ok(removed)
    }

    fn remove_layers(&self, names: &[String]) -> Result<(), String> {
        self.store
            .borrow_mut()
            .as_mut()
            .ok_or("Artwork storage is unavailable; repair it and restart.")?
            .remove_layers(names)
            .map_err(|error| error.to_string())?;
        self.chooser_owners
            .borrow_mut()
            .retain(|name, _| !names.contains(name));
        Ok(())
    }

    fn refresh(&self, force: bool) {
        let layer = self.layers.borrow().active().to_owned();
        let touch_owner = self
            .layers
            .borrow()
            .touchscreen_owner(&layer)
            .unwrap_or(&layer)
            .to_owned();
        let (previous, next) = self.layers.borrow().touchscreen_navigation(&layer);
        let cache_key = format!("{}:{layer}:{touch_owner}:{previous}:{next}", layer.len());
        if !force && self.last_layer.borrow().as_deref() == Some(&cache_key) {
            return;
        }
        let (paths, commands) = {
            let store = self.store.borrow();
            let Some(store) = store.as_ref() else {
                return;
            };
            let paths = self
                .pictures
                .borrow()
                .iter()
                .map(|(target, picture)| {
                    (
                        picture.clone(),
                        store.assigned_path_in_layer(
                            self.layers.borrow().artwork_owner(&layer, *target),
                            *target,
                        ),
                        store.button_text_in_layer(&layer, *target).cloned(),
                        self.control_icon(&layer, *target),
                    )
                })
                .collect::<Vec<_>>();
            (
                paths,
                desired_artwork_commands_for_owners(store, &layer, &touch_owner),
            )
        };
        let commands = self.with_info(&layer, commands);
        for (picture, path, text, icon) in paths {
            set_button_picture(&picture, path, text.as_ref(), icon.as_ref());
        }
        self.paint_info_previews(&layer);
        self.paint_navigation_previews(&layer);
        let _ = self.output.send(ArtworkCommand::Replace(commands));
        *self.last_layer.borrow_mut() = Some(cache_key);
        if let Some(editor) = self.text_editor.borrow().upgrade() {
            editor.load_context(false);
            editor.refresh_preview();
        }
    }

    /// The binding icon a key or touch section shows on `layer` when it has
    /// no artwork of its own and no live information. Derived at render
    /// time from the binding; it is never written into the artwork stores.
    fn control_icon(&self, layer: &str, target: ArtworkTarget) -> Option<ControlIcon> {
        let action = {
            let layers = self.layers.borrow();
            match target {
                ArtworkTarget::Keypad(key) => layers.binding(layer, &Trigger::KeyPress(key)),
                ArtworkTarget::EncoderZone(zone) => layers
                    .binding(layer, &Trigger::ZoneTap(zone))
                    .or_else(|| layers.binding(layer, &Trigger::ZonePress(zone))),
                ArtworkTarget::TouchStripBackground => None,
            }
        }?;
        let icon = BindingIcon::for_saved_action(&action);
        // A missing file is an error to show over the control's own picture;
        // every other icon only fills a control that has none.
        let own_picture =
            icon != BindingIcon::MissingFile && self.info_background(layer, target).is_some();
        if !icon.shown_on_controls() || own_picture || self.shows_info(layer, target) {
            return None;
        }
        let (previous, next) = self.layers.borrow().touchscreen_navigation(layer);
        Some(ControlIcon {
            target,
            icon,
            caption: if icon == BindingIcon::MissingFile {
                "Missing file".to_owned()
            } else {
                sdeck_plus::binding_editor::action_caption(&action)
            },
            previous: target == ArtworkTarget::EncoderZone(1) && previous,
            next: target == ArtworkTarget::EncoderZone(4) && next,
        })
    }

    /// The 120x120 key picture for `text` as the key shows it: over its own
    /// artwork, else over its binding icon tile (when the text leaves room for
    /// the icon), else on the neutral black key. Also returns whether the key
    /// shows anything. Derived when drawing; nothing is stored.
    fn key_text_image(
        &self,
        layer: &str,
        target: ArtworkTarget,
        text: &sdeck_plus::button_text::ButtonText,
    ) -> Result<(gtk::gdk_pixbuf::Pixbuf, bool), String> {
        let path = self.info_background(layer, target);
        let icon = self.control_icon(layer, target);
        let missing = icon
            .as_ref()
            .is_some_and(|icon| icon.icon == BindingIcon::MissingFile);
        if path.is_none() || missing {
            if let Some(icon) = icon {
                if let Some(image) = icon.compose(Some(text))? {
                    return Ok((image, true));
                }
            }
        }
        let shown = path.is_some() || !text.text.is_empty();
        let image = sdeck_plus::image_output::compose_button_image(path.as_deref(), text)?;
        Ok((image, shown))
    }

    /// Whether `target` shows live information on `layer`.
    fn shows_info(&self, layer: &str, target: ArtworkTarget) -> bool {
        self.info_targets(layer)
            .iter()
            .any(|(shown, _)| *shown == target)
    }

    fn restore_button_preview(&self, target: &ArtworkTarget) {
        let layer = self.layers.borrow().active().to_owned();
        if let Some((_, source)) = self
            .info_targets(&layer)
            .into_iter()
            .find(|(shown, _)| shown == target)
        {
            self.paint_info_preview(*target, &self.info_value(source));
            return;
        }
        let (path, text) = {
            let store = self.store.borrow();
            let Some(store) = store.as_ref() else {
                return;
            };
            (
                store.assigned_path_in_layer(
                    self.layers.borrow().artwork_owner(&layer, *target),
                    *target,
                ),
                store.button_text_in_layer(&layer, *target).cloned(),
            )
        };
        let icon = self.control_icon(&layer, *target);
        for (picture_target, picture) in self.pictures.borrow().iter() {
            if picture_target == target {
                set_button_picture(picture, path.clone(), text.as_ref(), icon.as_ref());
            }
        }
        if !matches!(target, ArtworkTarget::Keypad(_)) {
            self.paint_navigation_previews(&layer);
        }
    }

    fn import(&self, owner: &ChooserOwner, path: &std::path::Path) -> Result<(), String> {
        let current = self
            .chooser_owners
            .borrow()
            .get(&owner.name)
            .is_some_and(|incarnation| Rc::ptr_eq(incarnation, &owner.incarnation));
        if !current {
            return Err(format!("The chooser's layer '{}' was removed or replaced. Close it and choose an image for an existing layer.", owner.name));
        }
        assign_chooser_artwork(
            &self.layers,
            &mut self.store.borrow_mut(),
            &owner.name,
            owner.target,
            path,
        )?;
        let active = {
            let layers = self.layers.borrow();
            layers.artwork_owner(layers.active(), owner.target) == owner.name
        };
        if active {
            self.refresh(true);
        }
        Ok(())
    }
}

/// A binding icon derived for one key or touch section without artwork.
#[derive(Clone, Debug, PartialEq)]
struct ControlIcon {
    target: ArtworkTarget,
    icon: BindingIcon,
    caption: String,
    /// A page indicator paints at this section's outer left/right edge.
    previous: bool,
    next: bool,
}

impl ControlIcon {
    /// The key image or transparent touch-section foreground, or `None` when
    /// user button text leaves no room for an icon.
    fn compose(
        &self,
        text: Option<&sdeck_plus::button_text::ButtonText>,
    ) -> Result<Option<gtk::gdk_pixbuf::Pixbuf>, String> {
        match self.target {
            ArtworkTarget::Keypad(_) => {
                sdeck_plus::binding_icons::compose_key_image(self.icon, &self.caption, text)
            }
            ArtworkTarget::EncoderZone(_) => sdeck_plus::binding_icons::compose_zone_image(
                self.icon,
                &self.caption,
                self.previous,
                self.next,
            )
            .map(Some),
            ArtworkTarget::TouchStripBackground => Ok(None),
        }
    }
}

/// Composite a touch section's foreground (transparent apart from its page
/// indicators) over that section's binding icon.
fn with_icon_under(
    icon: &ControlIcon,
    foreground: &gtk::gdk_pixbuf::Pixbuf,
) -> Result<gtk::gdk_pixbuf::Pixbuf, String> {
    let Some(base) = icon.compose(None)? else {
        return Ok(foreground.clone());
    };
    foreground.composite(
        &base,
        0,
        0,
        base.width(),
        base.height(),
        0.0,
        0.0,
        1.0,
        1.0,
        gtk::gdk_pixbuf::InterpType::Nearest,
        255,
    );
    Ok(base)
}

fn set_button_picture(
    picture: &Picture,
    path: Option<std::path::PathBuf>,
    text: Option<&sdeck_plus::button_text::ButtonText>,
    icon: Option<&ControlIcon>,
) {
    let missing = icon.is_some_and(|icon| icon.icon == BindingIcon::MissingFile);
    if path.is_none() || missing {
        match icon.map(|icon| icon.compose(text)).transpose() {
            Ok(Some(Some(image))) => {
                picture.set_paintable(Some(&gtk::gdk::Texture::for_pixbuf(&image)));
                picture.set_visible(true);
                return;
            }
            Ok(_) => {}
            Err(error) => eprintln!("Could not compose binding icon preview: {error}"),
        }
    }
    match text {
        Some(text) => match sdeck_plus::image_output::compose_button_image(path.as_deref(), text) {
            Ok(image) => {
                picture.set_paintable(Some(&gtk::gdk::Texture::for_pixbuf(&image)));
                picture.set_visible(true);
            }
            Err(error) => eprintln!("Could not compose button preview: {error}"),
        },
        None => set_picture_path(picture, path),
    }
}

fn set_picture_path(picture: &Picture, path: Option<std::path::PathBuf>) {
    picture.set_filename(path.as_deref());
    if let Some(path) = &path {
        // Use the same first-frame decoder as the hardware image pipeline.
        match gtk::gdk_pixbuf::Pixbuf::from_file(path) {
            Ok(frame) => picture.set_paintable(Some(&gtk::gdk::Texture::for_pixbuf(&frame))),
            Err(error) => eprintln!(
                "Could not decode artwork preview {}: {error}",
                path.display()
            ),
        }
    }
    picture.set_visible(path.is_some());
}

fn assign_chooser_artwork(
    layers: &RefCell<LayerStore>,
    store: &mut Option<ArtworkStore>,
    owner: &str,
    target: ArtworkTarget,
    path: &std::path::Path,
) -> Result<(), String> {
    if !layers.borrow().has_configuration_owner(owner) {
        return Err(format!("The chooser's layer '{owner}' was removed. Close it and choose an image for an existing layer."));
    }
    store
        .as_mut()
        .ok_or("Artwork storage is unavailable; repair it and restart.")?
        .assign_in_layer(owner, target, path)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
fn remove_layer_with_artwork(
    layers: &RefCell<LayerStore>,
    artwork: &mut Option<ArtworkStore>,
    index: usize,
) -> Result<Option<LayerStore>, String> {
    let name = layers.borrow().names().get(index).cloned();
    let Some(name) = name.filter(|name| name != "Base") else {
        return Ok(None);
    };
    artwork
        .as_mut()
        .ok_or("Artwork storage is unavailable; repair it and restart.")?
        .remove_layer(&name)
        .map_err(|error| error.to_string())?;
    Ok(remove_selected_layer(layers, index))
}

fn register_picture(artwork: &ArtworkUi, target: ArtworkTarget, picture: &Picture) {
    artwork
        .pictures
        .borrow_mut()
        .push((target, picture.clone()));
}

fn connect_artwork_selection(
    button: &Button,
    picker: &gtk::DropDown,
    artwork: &ArtworkUi,
    target: ArtworkTarget,
) {
    let (picker, artwork) = (picker.clone(), artwork.clone());
    button.connect_clicked(move |_| {
        match ArtworkTarget::TOUCH.iter().position(|item| *item == target) {
            Some(index) => picker.set_selected(index as u32),
            // Keys are not in the picker; their right-click menu reads this target.
            None => {
                artwork.selected_target.set(target);
                if let Some(editor) = artwork.text_editor.borrow().upgrade() {
                    editor.load_context(false);
                }
            }
        }
    });
}

/// An icon-only binding tool whose tooltip names it, then says what it does.
fn binding_tool(icon: &str, name: &str, explanation: &str) -> Button {
    let button = ui::icon_button(icon, name);
    button.set_tooltip_text(Some(&format!("{name}\n{explanation}")));
    button
}

fn add_artwork_controls(
    parent: &GtkBox,
    window: &ApplicationWindow,
    artwork: &ArtworkUi,
) -> gtk::DropDown {
    let targets = gtk::StringList::new(
        &ArtworkTarget::TOUCH
            .map(|target| match target {
                ArtworkTarget::Keypad(key) => format!("Key {key}"),
                ArtworkTarget::TouchStripBackground => "Touch strip background".into(),
                ArtworkTarget::EncoderZone(zone) => format!("Touch zone {zone}"),
            })
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let picker = gtk::DropDown::new(Some(targets), gtk::Expression::NONE);
    picker.add_css_class("artwork-target");
    let layers = artwork.layers.clone();
    binding_icon::target_dropdown(
        &picker,
        Rc::new(move |target| {
            let layers = layers.borrow();
            let layer = layers.active();
            let action = match target {
                ArtworkTarget::Keypad(key) => layers.binding(layer, &Trigger::KeyPress(key)),
                ArtworkTarget::EncoderZone(zone) => layers
                    .binding(layer, &Trigger::ZoneTap(zone))
                    .or_else(|| layers.binding(layer, &Trigger::ZonePress(zone))),
                ArtworkTarget::TouchStripBackground => None,
            }?;
            let icon = BindingIcon::for_action(&action);
            icon.shown_on_controls()
                .then(|| (icon, sdeck_plus::binding_editor::action_caption(&action)))
        }),
    );
    parent.append(&labeled("Artwork target (active page)", &picker));
    let artwork_selection = artwork.clone();
    picker.connect_selected_notify(move |picker| {
        if let Some(target) = ArtworkTarget::TOUCH.get(picker.selected() as usize) {
            artwork_selection.selected_target.set(*target);
            if let Some(editor) = artwork_selection.text_editor.borrow().upgrade() {
                editor.load_context(false);
            }
        }
    });
    let controls = GtkBox::new(Orientation::Horizontal, 4);
    controls.add_css_class("artwork-controls");
    let choose = ui::button("image-x-generic-symbolic", "Image…");
    choose.set_tooltip_text(Some(
        "Choose an image for the current keypad or profile touchscreen page",
    ));
    let clear = ui::button("edit-clear-symbolic", "Clear");
    clear.set_tooltip_text(Some(
        "Clear the image on the current keypad or profile touchscreen page",
    ));
    controls.append(&choose);
    controls.append(&clear);
    parent.append(&controls);

    let available = artwork.store.borrow().is_some();
    choose.set_sensitive(available);
    clear.set_sensitive(available);
    let (parent_window, artwork_ref) = (window.clone(), artwork.clone());
    choose.connect_clicked(move |_| choose_artwork_image(&parent_window, &artwork_ref));
    let (parent_window, artwork_ref) = (window.clone(), artwork.clone());
    clear.connect_clicked(move |_| clear_artwork_image(&parent_window, &artwork_ref));
    picker
}

/// Ask for an image and assign it to the selected control.
fn choose_artwork_image(parent_window: &ApplicationWindow, artwork_ref: &ArtworkUi) {
    let owner = artwork_ref.capture_owner();
    let chooser = FileChooserNative::new(
        Some("Choose artwork"),
        Some(parent_window),
        FileChooserAction::Open,
        Some("Open"),
        Some("Cancel"),
    );
    let image_filter = gtk::FileFilter::new();
    image_filter.set_name(Some("PNG, JPG/JPEG and GIF images"));
    image_filter.add_mime_type("image/png");
    image_filter.add_mime_type("image/jpeg");
    image_filter.add_mime_type("image/gif");
    chooser.add_filter(&image_filter);
    let supported_filter = gtk::FileFilter::new();
    supported_filter.set_name(Some("All supported image formats"));
    supported_filter.add_pixbuf_formats();
    chooser.add_filter(&supported_filter);
    let artwork = artwork_ref.clone();
    let error_parent = parent_window.clone();
    chooser.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            if let Some(path) = dialog.file().and_then(|file| file.path()) {
                if let Err(error) = artwork.import(&owner, &path) {
                    show_artwork_error(&error_parent, &format!("Could not assign image: {error}"));
                }
            }
        }
    });
    chooser.show();
}

/// Remove the image of the selected control on the active page.
fn clear_artwork_image(parent_window: &ApplicationWindow, artwork: &ArtworkUi) {
    let layer = {
        let layers = artwork.layers.borrow();
        layers
            .artwork_owner(layers.active(), artwork.selected_target.get())
            .to_owned()
    };
    let cleared = {
        let mut store = artwork.store.borrow_mut();
        store
            .as_mut()
            .ok_or_else(|| "Artwork storage is unavailable".to_owned())
            .and_then(|store| {
                store
                    .clear_in_layer(&layer, artwork.selected_target.get())
                    .map_err(|error| error.to_string())
            })
    };
    match cleared {
        Ok(()) => artwork.refresh(true),
        Err(error) => {
            show_artwork_error(parent_window, &format!("Could not clear artwork: {error}"))
        }
    }
}

#[cfg(test)]
fn desired_artwork_commands(store: &ArtworkStore, layer: &str) -> Vec<ArtworkCommand> {
    desired_artwork_commands_for_owners(store, layer, layer)
}

fn desired_artwork_commands_for_owners(
    store: &ArtworkStore,
    layer: &str,
    touch_owner: &str,
) -> Vec<ArtworkCommand> {
    let mut commands = (1..=8)
        .map(|key| {
            let target = ArtworkTarget::Keypad(key);
            if let Some(text) = store.button_text_in_layer(layer, target) {
                return ArtworkCommand::ComposeButton {
                    key,
                    path: store.assigned_path_in_layer(layer, target),
                    text: text.clone(),
                };
            }
            match store.assigned_path_in_layer(layer, target) {
                Some(path) => ArtworkCommand::Assign { target, path },
                None => ArtworkCommand::Clear { target },
            }
        })
        .collect::<Vec<_>>();
    commands.extend(touch_strip_commands(store, touch_owner));
    commands
}

fn touch_strip_commands(store: &ArtworkStore, layer: &str) -> Vec<ArtworkCommand> {
    let mut commands = Vec::new();
    let background = store.assigned_path_in_layer(layer, ArtworkTarget::TouchStripBackground);
    match background {
        Some(path) => commands.push(ArtworkCommand::Assign {
            target: ArtworkTarget::TouchStripBackground,
            path,
        }),
        None => commands.push(ArtworkCommand::Clear {
            target: ArtworkTarget::TouchStripBackground,
        }),
    }
    for zone in 1..=4 {
        let target = ArtworkTarget::EncoderZone(zone);
        if let Some(path) = store.assigned_path_in_layer(layer, target) {
            commands.push(ArtworkCommand::Assign { target, path });
        }
    }
    commands
}

fn show_artwork_error(window: &ApplicationWindow, message: &str) {
    let dialog = gtk::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .message_type(gtk::MessageType::Error)
        .buttons(gtk::ButtonsType::Close)
        .text(message)
        .build();
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show();
}

/// Presents the device connection in plain language: a semantic badge plus a
/// short next step. Technical identifiers and worker diagnostics stay in the
/// tooltip. Returns whether the device is connected.
fn show_connection_state(status: &Label, detail: &Label, state: &ConnectionState) -> bool {
    const USB_ID: &str = "USB device VID 0FD9 / PID 0084";
    let not_connected = "Stream Deck + not connected — plug it in";
    let (badge, tone, text, hint) = match state {
        ConnectionState::Searching => (
            "Not connected",
            "status-idle",
            not_connected,
            format!("Looking for the {USB_ID}"),
        ),
        // Discovery reports a missing device as unavailable on every retry.
        ConnectionState::Unavailable(reason) if reason.starts_with("Device not found") => (
            "Not connected",
            "status-idle",
            not_connected,
            reason.clone(),
        ),
        ConnectionState::Unavailable(reason) => (
            "Unavailable",
            "status-error",
            "Stream Deck + found but unavailable — hover for details",
            format!("{reason}\n{USB_ID}"),
        ),
        ConnectionState::Disconnected(reason) => (
            "Disconnected",
            "status-idle",
            "Stream Deck + disconnected — plug it back in",
            format!("{reason}\n{USB_ID}"),
        ),
        ConnectionState::Connected => (
            "Connected",
            "status-online",
            "Stream Deck + connected — keys, dials and touch strip are live",
            format!("{USB_ID} · HID input and artwork active"),
        ),
    };
    status.set_text(badge);
    for class in ["status-idle", "status-online", "status-error"] {
        status.remove_css_class(class);
    }
    status.add_css_class(tone);
    detail.set_text(text);
    for label in [status, detail] {
        label.set_tooltip_text(Some(&hint));
    }
    matches!(state, ConnectionState::Connected)
}

fn set_pressed(widget: &gtk::Widget, pressed: bool) {
    if pressed {
        widget.add_css_class("control-pressed");
    } else {
        widget.remove_css_class("control-pressed");
    }
}

/// Briefly highlights a control for momentary input such as rotation or taps.
fn flash(widget: &gtk::Widget, class: &'static str) {
    widget.add_css_class(class);
    let widget = widget.downgrade();
    glib::timeout_add_local_once(Duration::from_millis(300), move || {
        if let Some(widget) = widget.upgrade() {
            widget.remove_css_class(class);
        }
    });
}

/// The touch strip is 800 px wide and split into four 200 px regions.
fn flash_zones(zones: &[gtk::Widget], from_x: u16, to_x: u16) {
    for zone in usize::from(from_x / 200).min(3)..=usize::from(to_x / 200).min(3) {
        if let Some(widget) = zones.get(zone) {
            flash(widget, "control-touched");
        }
    }
}

fn set_active(label: &Label, active: bool) {
    // Hidden key/dial labels retain state; their surface renders the live highlight.
    if !label.is_visible() {
        if let Some(surface) = label.parent() {
            if active {
                surface.add_css_class("control-active");
            } else {
                surface.remove_css_class("control-active");
            }
        }
    }
    if active {
        label.add_css_class("control-active");
    } else {
        label.remove_css_class("control-active");
    }
}

#[cfg(test)]
fn parse_trigger(value: &str) -> Result<Trigger, ()> {
    let parts: Vec<_> = value.trim().split(':').collect();
    match parts.as_slice() {
        ["k", n] => n.parse::<u8>().map(Trigger::KeyPress).map_err(|_| ()),
        ["b", n] => n.parse::<u8>().map(Trigger::EncoderPress).map_err(|_| ()),
        ["r", n, "+"] => Ok(Trigger::EncoderClockwise(n.parse().map_err(|_| ())?)),
        ["r", n, "-"] => Ok(Trigger::EncoderCounterClockwise(n.parse().map_err(|_| ())?)),
        ["r", n, ticks] => Ok(Trigger::Rotation(
            n.parse().map_err(|_| ())?,
            ticks.parse().map_err(|_| ())?,
        )),
        ["tap"] => Ok(Trigger::TouchTap),
        ["press"] => Ok(Trigger::TouchPress),
        ["flick"] => Ok(Trigger::TouchFlick),
        ["swipe", "left"] => Ok(Trigger::TouchSwipe(
            sdeck_plus::layers::SwipeDirection::Left,
        )),
        ["swipe", "right"] => Ok(Trigger::TouchSwipe(
            sdeck_plus::layers::SwipeDirection::Right,
        )),
        ["swipe", "up"] => Ok(Trigger::TouchSwipe(sdeck_plus::layers::SwipeDirection::Up)),
        ["swipe", "down"] => Ok(Trigger::TouchSwipe(
            sdeck_plus::layers::SwipeDirection::Down,
        )),
        _ => Err(()),
    }
}

#[cfg(test)]
fn parse_action(value: &str) -> Result<Action, ()> {
    let value = value.trim();
    if let Some(target) = value.strip_prefix("switch:") {
        return Ok(Action::SwitchLayer(target.trim().to_owned()));
    }
    if let Some(name) = value.strip_prefix("emit:") {
        return Ok(Action::Emit(name.to_owned()));
    }
    if let Some(command) = value.strip_prefix("launch:") {
        let mut parts = command.split('|');
        let executable = parts.next().filter(|s| !s.is_empty()).ok_or(())?;
        return Ok(Action::Launch {
            executable: executable.into(),
            args: parts.map(str::to_owned).collect(),
        });
    }
    if let Some(command) = value.strip_prefix("command:") {
        return Ok(Action::Command(command.to_owned()));
    }
    if let Some(key) = value.strip_prefix("key:") {
        return Ok(Action::KeyPress {
            key: key.trim().to_owned(),
        });
    }
    if let Some(shortcut) = value.strip_prefix("shortcut:") {
        let (modifiers, key) = shortcut.split_once(':').ok_or(())?;
        let modifiers = modifiers
            .split('+')
            .map(str::trim)
            .filter(|modifier| !modifier.is_empty())
            .map(str::to_owned)
            .collect();
        return Ok(Action::Shortcut {
            modifiers,
            key: key.trim().to_owned(),
        });
    }
    Err(())
}

// GTK's cover fit mirrors the device's aspect-preserving center crop.
fn artwork_picture() -> Picture {
    let picture = Picture::new();
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture
}

fn card() -> GtkBox {
    let panel = GtkBox::new(Orientation::Vertical, 4);
    panel.add_css_class("card");
    panel
}

fn install_style(window: &ApplicationWindow) {
    let provider = gtk::CssProvider::new();
    provider.connect_parsing_error(|_, section, error| {
        eprintln!("GTK CSS parse error at {section:?}: {error}");
    });
    provider.load_from_data(include_str!("ui.css"));
    let display = gtk::prelude::WidgetExt::display(window);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    install_light_overrides(&display);
}

/// Overrides the dark palette whenever the host GTK theme is not dark.
fn install_light_overrides(display: &gtk::gdk::Display) {
    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    if INSTALLED.replace(true) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.connect_parsing_error(|_, section, error| {
        eprintln!("GTK CSS parse error at {section:?}: {error}");
    });
    provider.load_from_data(include_str!("ui-light.css"));
    let settings = gtk::Settings::for_display(display);
    let display = display.clone();
    let sync = Rc::new(move |settings: &gtk::Settings| {
        let dark = settings.is_gtk_application_prefer_dark_theme()
            || settings
                .gtk_theme_name()
                .is_some_and(|name| name.to_lowercase().contains("dark"));
        gtk::style_context_remove_provider_for_display(&display, &provider);
        if !dark {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }
    });
    sync(&settings);
    for property in ["gtk-theme-name", "gtk-application-prefer-dark-theme"] {
        let sync = sync.clone();
        settings.connect_notify_local(Some(property), move |settings, _| sync(settings));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdeck_plus::layers::SwipeDirection;
    use std::sync::Mutex;
    mod action_catalog_tests;
    mod asset_link_save_tests;
    mod binding_icon_tests;
    mod binding_toolbar_tests;
    mod button_swap_tests;
    mod button_text_popup_tests;
    mod catalog_overwrite_tests;
    mod configuration_transfer_tests;
    mod control_feedback_tests;
    mod dial_action_tests;
    mod dial_preset_tests;
    mod documentation_screenshots;
    mod folder_panel_tests;
    mod hover_help_tests;
    mod key_hold_integration_tests;
    mod key_hold_ui_tests;
    mod live_info_tests;
    mod multimedia_action_tests;
    mod organize_popover_tests;
    mod page_return_tests;
    mod page_ui_tests;
    mod preview_layout_tests;
    #[path = "profile_manager_tests.rs"]
    mod profile_manager_tests;
    #[path = "shortcut_reference_tests.rs"]
    mod shortcut_reference_tests;
    mod shortcut_table_tests;
    mod timer_countdown_tests;
    mod timer_notification_tests;
    mod touchscreen_tests;
    mod ui_refresh_tests;

    include!("button_text_editor_tests.rs");

    fn run_gtk_test(test: fn()) {
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        // A mutex serializes calls but libtest still gives each test a new thread.
        // gtk-rs dispatches the entire body to one owner and resumes panics here.
        gtk::test_synced(test);
    }

    #[test]
    fn gtk_dispatch_preserves_owner_and_propagates_panics() {
        run_gtk_test(|| assert!(gtk::is_initialized_main_thread()));
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let owner = gtk::test_synced(|| thread::current().id());
        let panic = std::panic::catch_unwind(|| run_gtk_test(|| panic!("GTK test sentinel")))
            .expect_err("GTK assertions must fail the calling test");
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"GTK test sentinel"));
        let next_owner = thread::spawn(|| gtk::test_synced(|| thread::current().id()))
            .join()
            .unwrap();
        assert_eq!(owner, next_owner, "a panic must not poison later dispatch");
    }

    fn init_gtk_for_test() -> bool {
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return false;
        }
        gtk::init().expect("GTK initialization failed despite an advertised display");
        true
    }

    fn test_binding_ui() -> Option<BindingUi> {
        if !init_gtk_for_test() {
            return None;
        }
        // Pixel assertions inspect final states, not theme transition timing.
        gtk::Settings::default()
            .expect("GTK settings")
            .set_gtk_enable_animations(false);
        let layer_model = gtk::StringList::new(&["Base"]);
        let layer_picker = gtk::DropDown::new(Some(layer_model.clone()), gtk::Expression::NONE);
        let control_model = gtk::StringList::new(&["Button", "Encoder", "Touchpad"]);
        let control_picker = gtk::DropDown::new(Some(control_model), gtk::Expression::NONE);
        let control_number_model = gtk::StringList::new(&["1", "2", "3", "4", "5", "6", "7", "8"]);
        let control_number_picker =
            gtk::DropDown::new(Some(control_number_model.clone()), gtk::Expression::NONE);
        let event_model = gtk::StringList::new(&["Press"]);
        let event_picker = gtk::DropDown::new(Some(event_model.clone()), gtk::Expression::NONE);
        let action_model = gtk::StringList::new(&[
            "Go to profile or page",
            "Run command",
            "Open application",
            "Shortcut",
            "Send named event",
            "More actions",
            "Play sound",
            "None",
        ]);
        let action_picker = gtk::DropDown::new(Some(action_model), gtk::Expression::NONE);
        action_picker.set_selected(7);
        let layer_target_model = gtk::StringList::new(&["Base"]);
        let layer_target_picker =
            gtk::DropDown::new(Some(layer_target_model.clone()), gtk::Expression::NONE);
        let text_entry = Entry::new();
        let application_entry = Entry::new();
        let args_view = TextView::new();
        let [ctrl_mod, alt_mod, shift_mod, super_mod] =
            ["Ctrl", "Alt", "Shift", "Super"].map(modifier_toggle);
        let control_number_row = labeled("Number", &control_number_picker);
        let layer_target_row = labeled("Destination profile or page", &layer_target_picker);
        let text_row = labeled("Command / key / shortcut key / event name", &text_entry);
        let application_row = labeled("Application executable", &application_entry);
        let args_scroller = gtk::ScrolledWindow::builder().child(&args_view).build();
        let args_row = labeled("Application arguments", &args_scroller);
        let modifier_buttons = GtkBox::new(Orientation::Horizontal, 8);
        modifier_buttons.append(&super_mod);
        modifier_buttons.append(&shift_mod);
        modifier_buttons.append(&ctrl_mod);
        modifier_buttons.append(&alt_mod);
        let modifier_row = labeled("Shortcut modifiers", &modifier_buttons);
        Some(BindingUi::new(
            layer_picker,
            layer_model,
            control_picker,
            control_number_picker,
            control_number_model,
            event_picker,
            event_model,
            action_picker,
            layer_target_picker,
            layer_target_model,
            text_entry,
            application_entry,
            args_view,
            super_mod,
            shift_mod,
            ctrl_mod,
            alt_mod,
            Label::new(None),
            control_number_row,
            layer_target_row,
            text_row,
            application_row,
            args_row,
            modifier_row,
        ))
    }

    #[test]
    fn binding_editor_hides_labeled_rows_for_inactive_controls_and_actions() {
        run_gtk_test(assert_binding_editor_hides_labeled_rows);
    }

    fn assert_binding_editor_hides_labeled_rows() {
        let Some(ui) = test_binding_ui() else {
            assert!(
                std::env::var_os("DISPLAY").is_none()
                    && std::env::var_os("WAYLAND_DISPLAY").is_none(),
                "GTK initialization failed despite an advertised display session"
            );
            eprintln!("skipping GTK widget visibility test: no display session available");
            return;
        };
        let mut lifecycle = DeviceLifecycle::default();
        let fired = Rc::new(Cell::new(false));
        let callback_fired = fired.clone();
        lifecycle.polling = Some(glib::idle_add_local(move || {
            callback_fired.set(true);
            glib::ControlFlow::Continue
        }));
        let app = Application::new(None::<&str>, gtk::gio::ApplicationFlags::NON_UNIQUE);
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        app_icon::register().unwrap();
        assert!(
            gtk::IconTheme::for_display(&gtk::gdk::Display::default().unwrap()).has_icon(APP_NAME)
        );
        let icon = app_icon::tray_pixmap().unwrap();
        assert_eq!(
            (icon.width, icon.height, icon.data.len()),
            (64, 64, 64 * 64 * 4)
        );
        let window = ApplicationWindow::builder().application(&app).build();
        let lifecycle = Rc::new(RefCell::new(lifecycle));
        install_device_cleanup(&app, lifecycle.clone());
        let quit = gtk::gio::SimpleAction::new("quit", None);
        quit.connect_activate(glib::clone!(
            #[weak]
            app,
            move |_, _| app.quit()
        ));
        app.add_action(&quit);
        let close_preference = tray::install_close_preference(&app, true);
        window.set_widget_name("configuration");
        configure_window_close(&window, &app);
        window.present();
        // These calls must reuse the existing window, before configuration I/O
        // or worker startup. No device or user configuration is accessed here.
        build_ui(&app);
        build_ui(&app);
        assert_eq!(app.windows().len(), 1);
        assert!(lifecycle.borrow().polling.is_some());
        window.close();
        assert!(!window.is_visible());
        assert!(lifecycle.borrow().polling.is_some());
        build_ui(&app);
        assert!(window.is_visible());
        assert_eq!(app.windows().len(), 1);
        tray::apply_close_preference(&app, &close_preference, false);
        assert!(!window.hides_on_close());
        assert!(!keep_running_on_close(&app));
        tray::apply_close_preference(&app, &close_preference, true);
        assert!(window.hides_on_close());
        let preference_root =
            std::env::temp_dir().join(format!("sdeck-tray-gtk-{}", std::process::id()));
        std::fs::create_dir(&preference_root).unwrap();
        let preference_path = preference_root.join("tray.json");
        std::fs::write(&preference_path, b"malformed").unwrap();
        assert!(
            tray::save_close_preference(&app, &close_preference, &preference_path, false).is_err()
        );
        assert!(keep_running_on_close(&app));
        assert!(window.hides_on_close());
        assert_eq!(std::fs::read(&preference_path).unwrap(), b"malformed");
        std::fs::remove_file(&preference_path).unwrap();
        std::fs::write(preference_path.with_extension("json.tmp"), b"occupied").unwrap();
        assert!(
            tray::save_close_preference(&app, &close_preference, &preference_path, false).is_err()
        );
        assert!(keep_running_on_close(&app));
        assert!(window.hides_on_close());
        std::fs::remove_file(preference_path.with_extension("json.tmp")).unwrap();
        tray::save_close_preference(&app, &close_preference, &preference_path, false).unwrap();
        assert!(!keep_running_on_close(&app));
        assert!(!window.hides_on_close());
        assert!(!tray_preference::load(&preference_path).unwrap());
        tray::save_close_preference(&app, &close_preference, &preference_path, true).unwrap();
        std::fs::remove_dir_all(preference_root).unwrap();
        tray::show_about(&app);
        tray::show_about(&app);
        assert_eq!(app.windows().len(), 2);
        if let Some(directory) = std::env::var_os("STREAMDECK_TRAY_QA_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            let settings = gtk::Settings::default().unwrap();
            let original = settings.is_gtk_application_prefer_dark_theme();
            let original_theme = settings.gtk_theme_name();
            let about = app
                .windows()
                .into_iter()
                .find(|dialog| dialog.widget_name() == "about")
                .unwrap();
            for (name, dark) in [("light", false), ("dark", true)] {
                settings.set_gtk_theme_name(Some(if dark { "Adwaita-dark" } else { "Adwaita" }));
                settings.set_gtk_application_prefer_dark_theme(dark);
                rendered_texture(&window, &about)
                    .save_to_png(directory.join(format!("about-{name}.png")))
                    .unwrap();
            }
            settings.set_gtk_application_prefer_dark_theme(original);
            settings.set_gtk_theme_name(original_theme.as_deref());
        }
        window.close();
        build_ui_with_visibility(&app, false);
        assert!(!window.is_visible());
        build_ui(&app);
        assert!(window.is_visible());
        assert_eq!(app.windows().len(), 2);
        for dialog in app.windows() {
            if dialog.widget_name() == "about" {
                dialog.close();
            }
        }
        let hold = app.hold();
        let (shutdown_sent, shutdown_received) = mpsc::channel();
        app.connect_shutdown(move |_| shutdown_sent.send(()).unwrap());
        let (worker_sent, worker_received) = mpsc::channel();
        let (layer_sender, layer_receiver) = mpsc::channel();
        lifecycle.borrow_mut().layer_sender = Some(layer_sender);
        lifecycle.borrow_mut().layer_worker = Some(thread::spawn(move || {
            assert!(matches!(
                layer_receiver.recv().unwrap(),
                LayerCommand::Shutdown
            ));
            worker_sent.send(()).unwrap();
        }));
        let timed_out = Rc::new(Cell::new(false));
        let deadline_hit = timed_out.clone();
        let deadline_app = app.clone();
        let deadline = glib::timeout_add_local_once(Duration::from_secs(5), move || {
            deadline_hit.set(true);
            deadline_app.quit();
        });
        app.connect_activate(glib::clone!(
            #[weak]
            window,
            #[strong]
            lifecycle,
            #[strong]
            close_preference,
            move |app| {
                window.close();
                assert!(!window.is_visible());
                assert!(lifecycle.borrow().polling.is_some());
                let app = app.clone();
                let lifecycle = lifecycle.clone();
                let close_preference = close_preference.clone();
                let window = window.clone();
                glib::idle_add_local_once(move || {
                    assert!(lifecycle.borrow().polling.is_some());
                    tray::apply_close_preference(&app, &close_preference, false);
                    build_ui(&app);
                    assert!(window.is_visible());
                    assert!(!window.hides_on_close());
                    window.close();
                });
            }
        ));
        assert_eq!(app.run_with_args::<&str>(&[]), glib::ExitCode::SUCCESS);
        assert!(!timed_out.get(), "close must quit even with a tray hold");
        deadline.remove();
        shutdown_received
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        worker_received
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        drop(hold);
        assert!(lifecycle.borrow().polling.is_none());
        assert!(lifecycle.borrow().hid.is_none());
        while glib::MainContext::default().pending() {
            glib::MainContext::default().iteration(false);
        }
        eprintln!("GTK lifecycle cleanup executed: checked close retained polling; reopening reused window; unchecked close quit with hold and joined worker; shutdown removed polling (poll fired: {})", fired.get());
        let fallback = Application::new(None::<&str>, gtk::gio::ApplicationFlags::NON_UNIQUE);
        fallback.register(None::<&gtk::gio::Cancellable>).unwrap();
        let fallback_window = ApplicationWindow::builder().application(&fallback).build();
        configure_window_close(&fallback_window, &fallback);
        assert!(!fallback_window.hides_on_close());
        let fallback_hold = fallback.hold();
        let (fallback_sent, fallback_received) = mpsc::channel();
        fallback.connect_shutdown(move |_| fallback_sent.send(()).unwrap());
        fallback.connect_activate(move |_| {
            fallback_window.present();
            fallback_window.close();
        });
        let fallback_timed_out = Rc::new(Cell::new(false));
        let expired = fallback_timed_out.clone();
        let fallback_app = fallback.clone();
        let deadline = glib::timeout_add_local_once(Duration::from_secs(5), move || {
            expired.set(true);
            fallback_app.quit();
        });
        assert_eq!(fallback.run_with_args::<&str>(&[]), glib::ExitCode::SUCCESS);
        assert!(
            !fallback_timed_out.get(),
            "unavailable tray must quit on close"
        );
        deadline.remove();
        fallback_received
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        drop(fallback_hold);
        eprintln!("GTK tray-unavailable close quit despite application hold");
        assert_shortcut_only_menu(&ui);
        ui.action_picker.set_selected(5);
        ui.update_action_visibility();
        ui.feature_editor.assert_roundtrips();
        assert!(ui.feature_editor.root.is_visible());
        assert!(!ui.layer_target_row.is_visible());
        assert!(!ui.text_row.is_visible());
        assert!(!ui.application_row.is_visible());
        assert!(!ui.args_row.is_visible());
        assert!(!ui.modifier_row.is_visible());
        assert_legacy_keys_load_without_rewriting_and_save_as_shortcuts(&ui);
        assert_unsaved_edits_are_flagged_and_shortcut_is_previewed(&ui);
        assert_discard_prompt_guards_unsaved_edits();
        ui.control_picker.set_selected(2);
        ui.update_for_control(true);
        assert!(ui.control_number_row.is_visible());
        assert_eq!(ui.control_number_model.n_items(), 5);
        ui.control_number_picker.set_selected(0);
        assert_eq!(ui.control_number(), Some(0));
        ui.control_number_picker.set_selected(4);
        assert_eq!(ui.control_number(), Some(4));
        ui.control_number_picker.set_selected(0);

        ui.action_picker.set_selected(0);
        ui.update_action_visibility();
        assert!(ui.layer_target_row.is_visible());
        assert!(!ui.text_row.is_visible());
        assert!(!ui.application_row.is_visible());
        assert!(!ui.args_row.is_visible());
        assert!(!ui.modifier_row.is_visible());

        ui.action_picker.set_selected(2);
        ui.update_action_visibility();
        assert!(!ui.layer_target_row.is_visible());
        assert!(!ui.text_row.is_visible());
        assert!(ui.application_row.is_visible());
        assert!(ui.args_row.is_visible());
        assert!(!ui.modifier_row.is_visible());

        assert_physical_shortcut_widgets(&ui);
        shortcut_reference_tests::assert_reference_dialog();
        profile_manager_tests::assert_compact_profile_manager();
        control_feedback_tests::assert_control_feedback();
        assert_profile_tabs_and_focus();
        assert_binding_save_toasts();
        assert_select_control_validates_ranges_and_preserves_valid_events();
        assert_direct_button_click_selects_button_8_and_loads_existing_assignment();
        assert_direct_selection_css_is_distinct_from_live_active_state();
        let picture = Picture::new();
        set_picture_path(&picture, Some(std::path::PathBuf::from("previous.png")));
        assert!(picture.is_visible());
        set_picture_path(&picture, None);
        assert!(!picture.is_visible());
        assert!(picture.file().is_none());
        assert_gif_artwork_preview();
        assert_artwork_refresh_switch_and_reconnect();
        assert_button_text_editor();
        eprintln!("GTK artwork preview assertions executed");
        assert_summaries_refresh_for_binding_and_layer_changes(&ui);
        eprintln!("GTK assertions executed: selection, styles, summaries and refresh paths");
        assert!(ui.summaries[0].text().contains("Unassigned"));
        assert_production_layout_fits();
    }

    fn assert_profile_tabs_and_focus() {
        let (root, mut store, artwork_store, _) = artwork_fixture("profile-focus");
        if !store.layers().contains_key("Studio") {
            store.create("Studio").unwrap();
        }
        store.select("Base");
        let layers = Rc::new(RefCell::new(store));
        let window = ApplicationWindow::builder()
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .build();
        install_style(&window);
        let (output, _device) = mpsc::channel();
        let artwork = ArtworkUi {
            store: Rc::new(RefCell::new(Some(artwork_store))),
            layers: layers.clone(),
            pictures: Rc::new(RefCell::new(Vec::new())),
            output,
            last_layer: Rc::new(RefCell::new(None)),
            chooser_owners: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
            selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
            text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
            info: Rc::default(),
        };
        let (requests, commands) = mpsc::channel();
        let widgets = build_widgets(window.clone(), layers.clone(), artwork, requests);
        window.set_child(Some(&widgets.root));
        window.present();
        let runtime = &widgets.feature_runtime;
        let tabs = runtime.profile_tabs();
        assert_eq!(tabs.n_pages(), 2);
        tabs.set_current_page(Some(1));
        assert_eq!(layers.borrow().active(), "Studio");
        let mut buttons = Vec::new();
        collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
        let button = |caption: &str| {
            buttons
                .iter()
                .find(|button| button_caption(button).as_deref() == Some(caption))
                .unwrap()
        };
        button("New page").emit_clicked();
        let dialog = current_dialog();
        assert_eq!(
            layers.borrow().active(),
            "Studio",
            "opening must not create a page"
        );
        page_dialog_entry(&dialog).set_text("Studio tools");
        dialog.response(ResponseType::Accept);
        let studio_page = layers.borrow().active().to_owned();
        assert_ne!(studio_page, "Studio");
        assert_eq!(tabs.n_pages(), 2, "pages must not become profile tabs");
        tabs.set_current_page(Some(0));
        assert_eq!(layers.borrow().active(), "Base");
        tabs.set_current_page(Some(1));
        assert_eq!(layers.borrow().active(), studio_page);
        let manager = runtime.management();
        let mut child = manager.first_child();
        let mut picker = None;
        while let Some(widget) = child {
            if let Some(row) = widget.downcast_ref::<GtkBox>() {
                if row
                    .first_child()
                    .and_then(|child| child.downcast::<Label>().ok())
                    .is_some_and(|label| label.text() == "Profile to edit")
                {
                    picker = row
                        .last_child()
                        .and_then(|child| child.downcast::<gtk::DropDown>().ok());
                }
            }
            child = widget.next_sibling();
        }
        let picker = picker.unwrap();
        let mut entries = Vec::new();
        collect_entries(manager.upcast_ref(), &mut entries);
        let application = entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("Window class or executable"))
            .unwrap();
        picker.set_selected(0);
        assert!(
            !application.is_sensitive(),
            "Base must remain unconditional"
        );
        picker.set_selected(1);
        application.set_text("ui_showcase");
        button("Save changes").emit_clicked();
        assert_eq!(tabs.n_pages(), 2, "configured profiles must stay visible");
        assert_eq!(layers.borrow().active(), studio_page);
        assert!(
            application.is_sensitive(),
            "associated profiles remain configurable"
        );
        let mut saved = None;
        while let Ok(command) = commands.try_recv() {
            if let LayerCommand::SaveProfile(snapshot, Some(organization)) = command {
                saved = Some((snapshot, organization));
            }
        }
        let (snapshot, organization) = saved.unwrap();
        assert_eq!(
            organization.profile_application("Studio"),
            Some("ui_showcase")
        );
        assert_eq!(snapshot.active(), studio_page);
        let focus = sdeck_plus::app_focus::FocusedApplication {
            window_class: "dev.streamdeck.UiShowcase".into(),
            executable: Some("ui_showcase".into()),
        };
        runtime.focus_changed(Ok(Some(focus.clone())));
        assert_eq!(tabs.n_pages(), 2);
        assert_eq!(layers.borrow().active(), studio_page);
        for (caption, invalid) in [
            ("New page", false),
            ("New page", true),
            ("Rename page", false),
            ("Rename page", true),
        ] {
            runtime.focus_changed(Ok(Some(focus.clone())));
            while commands.try_recv().is_ok() {}
            button(caption).emit_clicked();
            let dialog = current_dialog();
            if invalid {
                page_dialog_entry(&dialog).set_text("");
                dialog.response(ResponseType::Accept);
                assert!(dialog.is_visible());
            }
            dialog.response(ResponseType::Cancel);
            assert!(
                commands.try_recv().is_err(),
                "cancel/invalid input must not persist"
            );
            runtime.focus_changed(Ok(Some(sdeck_plus::app_focus::FocusedApplication {
                window_class: "kitty".into(),
                executable: Some("kitty".into()),
            })));
            assert_eq!(
                layers.borrow().active(),
                "Base",
                "canceled name edits must preserve automatic focus selection"
            );
        }
        runtime.focus_changed(Ok(Some(focus.clone())));
        tabs.set_current_page(Some(0));
        runtime.focus_changed(Ok(Some(focus.clone())));
        assert_eq!(
            layers.borrow().active(),
            "Base",
            "duplicate focus must preserve manual choice"
        );
        runtime.focus_changed(Ok(None));
        assert_eq!(tabs.n_pages(), 2, "focus changes must not remove tabs");
        tabs.set_current_page(Some(1));
        assert_eq!(
            layers.borrow().active(),
            studio_page,
            "associated tabs remain manually selectable without focus"
        );
        runtime.focus_changed(Ok(Some(sdeck_plus::app_focus::FocusedApplication {
            window_class: "unrelated-app".into(),
            executable: None,
        })));
        assert_eq!(
            layers.borrow().active(),
            studio_page,
            "unrelated focus must not discard a manually selected editing page"
        );
        widgets.binding_ui.action_picker.set_selected(4);
        widgets.binding_ui.text_entry.set_text("manual-page-save");
        button("Save binding").emit_clicked();
        assert_eq!(layers.borrow().active(), studio_page);
        assert_eq!(tabs.current_page(), Some(1));
        let saved = commands
            .try_iter()
            .find_map(|command| match command {
                LayerCommand::SaveBinding(snapshot, _) => Some(snapshot),
                _ => None,
            })
            .unwrap();
        assert_eq!(saved.active(), studio_page);
        assert_eq!(
            saved.layers()[&studio_page].bindings[&Trigger::KeyPress(1)],
            Action::Emit("manual-page-save".into())
        );
        tabs.set_current_page(Some(0));
        runtime.execute(
            "blocked-switch".into(),
            Action::SwitchLayer(studio_page.clone()),
        );
        assert_eq!(layers.borrow().active(), studio_page);
        runtime.focus_changed(Ok(Some(focus)));
        assert_eq!(layers.borrow().active(), studio_page);
        runtime.focus_changed(Err("test unavailable".into()));
        assert_eq!(tabs.n_pages(), 2);
        assert_eq!(layers.borrow().active(), "Base");
        picker.set_selected(1);
        application.set_text("unsaved-app");
        runtime.focus_changed(Ok(Some(sdeck_plus::app_focus::FocusedApplication {
            window_class: "other-app".into(),
            executable: None,
        })));
        assert_eq!(
            application.text(),
            "unsaved-app",
            "focus refresh must preserve the application draft"
        );
        application.set_text("");
        button("Save changes").emit_clicked();
        assert_eq!(tabs.n_pages(), 2);
        let mut entries = Vec::new();
        collect_entries(widgets.root.upcast_ref(), &mut entries);
        let name = entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("New profile name"))
            .unwrap();
        name.set_text("Manual");
        button("Create profile").emit_clicked();
        assert_eq!(tabs.n_pages(), 3);
        assert_eq!(layers.borrow().active(), "Manual");
        assert_eq!(
            picker
                .selected_item()
                .unwrap()
                .downcast::<gtk::StringObject>()
                .unwrap()
                .string(),
            "Manual"
        );
        runtime.focus_changed(Ok(None));
        assert_eq!(tabs.n_pages(), 3);
        assert!(layers.borrow().layers().contains_key("Manual"));
        window.destroy();
        std::fs::remove_dir_all(root).unwrap();
        eprintln!("GTK profile tabs executed: saved creation, grouped pages, persistent tabs, manual selection and drafts across focus changes");
    }

    fn assert_binding_save_toasts() {
        let (root, store, artwork_store, _) = artwork_fixture("save-toast");
        let layers = Rc::new(RefCell::new(store));
        let window = ApplicationWindow::builder()
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .build();
        install_style(&window);
        let (artwork_output, _artwork_events) = mpsc::channel();
        let artwork = ArtworkUi {
            store: Rc::new(RefCell::new(Some(artwork_store))),
            layers: layers.clone(),
            pictures: Rc::new(RefCell::new(Vec::new())),
            output: artwork_output,
            last_layer: Rc::new(RefCell::new(None)),
            chooser_owners: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
            selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
            text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
            info: Rc::default(),
        };
        let (commands, requests) = mpsc::channel();
        let (output, events) = mpsc::channel();
        let widgets = build_widgets(
            window.clone(),
            layers.clone(),
            artwork.clone(),
            commands.clone(),
        );
        let ui = widgets.binding_ui.clone();
        let toast = widgets.toast.clone();
        let viewport = widgets.root.clone();
        let management_menu = widgets.management_menu.clone();
        let surface = toast.wrap(&viewport);
        let stage = gtk::Fixed::new();
        stage.put(&surface, 0.0, 0.0);
        surface.set_size_request(DEFAULT_WIDTH, DEFAULT_HEIGHT);
        window.set_child(Some(&stage));
        let (device, _device_requests) = mpsc::channel();
        widgets.feature_runtime.configure(
            sdeck_plus::organization::Organization::open_at(
                layers.borrow().root(),
                &layers.borrow(),
            ),
            Ok(sdeck_plus::obs::ObsConfig::default()),
            device,
            output.clone(),
        );
        key_button(&widgets.key_labels[0]).emit_clicked();
        ui.action_picker.set_selected(4);
        ui.text_entry.set_text("toast-test");
        let mut buttons = Vec::new();
        collect_layout_buttons(viewport.upcast_ref(), &mut buttons);
        let save = buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some("Save binding"))
            .unwrap();
        let create_profile = buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some("Create profile"))
            .unwrap();
        let save_profile = buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some("Save changes"))
            .unwrap();
        let mut entries = Vec::new();
        collect_entries(viewport.upcast_ref(), &mut entries);
        let profile_name = entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("New profile name"))
            .unwrap();
        let profile_application = entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("Window class or executable"))
            .unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_output = output.clone();
        let worker =
            thread::spawn(move || feature_runtime::run_worker(requests, worker_output, stop));
        let polling = start_event_updates(
            widgets,
            events,
            layers.clone(),
            artwork,
            commands.clone(),
            output,
        );
        window.present();
        wait_for_frame(&window);
        let directory = std::env::var_os("STREAMDECK_QA_DIR").map(std::path::PathBuf::from);
        let path = layers.borrow().root().join("layers.conf");
        for scenario in 0..5 {
            let profile = scenario >= 2;
            let failed = scenario == 1 || scenario == 4;
            let metadata_path = layers.borrow().root().join("organization.json");
            let metadata_before = std::fs::read(&metadata_path).ok();
            if failed {
                std::fs::rename(&path, path.with_extension("backup")).unwrap();
                std::fs::create_dir(&path).unwrap();
                std::fs::write(path.join("sentinel"), b"keep").unwrap();
            }
            toast.root.set_reveal_child(false);
            if scenario == 2 {
                profile_name.set_text("Saved profile");
            } else if scenario == 3 {
                profile_application.set_text("ui_showcase");
            } else if scenario == 4 {
                profile_application.set_text("firefox");
            }
            // Subscribe before clicking, then wait for the actual worker result.
            let loop_ = glib::MainLoop::new(None, false);
            let arrived = Rc::new(Cell::new(false));
            let received = arrived.clone();
            let done = loop_.clone();
            let signal = toast.root.connect_reveal_child_notify(move |root| {
                if root.reveals_child() {
                    received.set(true);
                    done.quit();
                }
            });
            let timed_out = Rc::new(Cell::new(false));
            let flag = timed_out.clone();
            let done = loop_.clone();
            let timeout = glib::timeout_add_local_once(Duration::from_secs(5), move || {
                flag.set(true);
                done.quit();
            });
            match scenario {
                0 | 1 => save.emit_clicked(),
                2 => create_profile.emit_clicked(),
                3 | 4 => save_profile.emit_clicked(),
                _ => unreachable!(),
            }
            assert!(
                !toast.root.reveals_child(),
                "queued saves must not report success"
            );
            if !arrived.get() {
                loop_.run();
            }
            toast.root.disconnect(signal);
            if !timed_out.get() {
                timeout.remove();
            }
            assert!(arrived.get(), "save result did not reach the toast");
            assert_eq!(toast.label.has_css_class("feedback-error"), failed);
            if failed {
                assert_eq!(std::fs::read(path.join("sentinel")).unwrap(), b"keep");
                assert_eq!(std::fs::read(&metadata_path).ok(), metadata_before);
                std::fs::remove_dir_all(&path).unwrap();
                std::fs::rename(path.with_extension("backup"), &path).unwrap();
            } else {
                let reopened = LayerStore::open_at(layers.borrow().root()).unwrap();
                assert_eq!(
                    reopened.layers()["Base"].bindings[&Trigger::KeyPress(1)],
                    Action::Emit("toast-test".into())
                );
                if profile {
                    assert!(reopened.layers().contains_key("Saved profile"));
                    let metadata = sdeck_plus::organization::Organization::open_at(
                        layers.borrow().root(),
                        &reopened,
                    )
                    .unwrap();
                    assert_eq!(
                        metadata.profile_application("Saved profile"),
                        if scenario == 3 {
                            Some("ui_showcase")
                        } else {
                            None
                        }
                    );
                }
            }
            for (width, height) in [(1000, 700), (640, 480)] {
                surface.set_size_request(width, height);
                wait_for_frame(&surface);
                surface.allocate(width, height, -1, None);
                let before = (viewport.width(), viewport.height());
                let texture = rendered_texture(&window, &surface);
                assert_eq!((texture.width(), texture.height()), (width, height));
                assert_eq!((viewport.width(), viewport.height()), before);
                let bounds = toast.root.compute_bounds(&surface).unwrap();
                assert!(bounds.x() >= 0.0 && bounds.y() >= 0.0);
                assert!(bounds.x() + bounds.width() <= width as f32 + 1.0);
                assert!(bounds.y() + bounds.height() <= height as f32 + 1.0);
                if let Some(directory) = &directory {
                    texture
                        .save_to_png(
                            directory.join(format!("save-result-{scenario}-{width}x{height}.png")),
                        )
                        .unwrap();
                }
                if scenario == 3 {
                    let panel = management_menu.popover().unwrap();
                    let autohide = panel.is_autohide();
                    // The user's desktop focus must not dismiss an owned capture.
                    panel.set_autohide(false);
                    management_menu.popup();
                    let panel_content = panel.child().unwrap();
                    wait_for_frame(&panel);
                    assert!(panel.is_visible() && panel.is_mapped());
                    assert!(panel_content.is_drawable());
                    // Native popup surfaces do not expose a render node reliably.
                    // Render the same widgets in a fixed component host instead.
                    let panel_size = (panel_content.width(), panel_content.height());
                    assert!(panel_size.0 > 0 && panel_size.1 > 0);
                    panel.set_child(gtk::Widget::NONE);
                    let capture_stage = gtk::Fixed::new();
                    panel_content.set_size_request(panel_size.0, panel_size.1);
                    capture_stage.put(&panel_content, 0.0, 0.0);
                    let capture_window = ApplicationWindow::builder().child(&capture_stage).build();
                    capture_window.present();
                    wait_for_frame(&panel_content);
                    let scroller = save_profile
                        .ancestor(gtk::ScrolledWindow::static_type())
                        .unwrap()
                        .downcast::<gtk::ScrolledWindow>()
                        .unwrap();
                    let scroll_to = |button: &Button| {
                        let bounds = button.compute_bounds(&scroller).unwrap();
                        if bounds.y() < 0.0
                            || bounds.y() + bounds.height() > scroller.height() as f32
                        {
                            let adjustment = scroller.vadjustment();
                            adjustment.set_value(adjustment.value() + f64::from(bounds.y()));
                            wait_for_frame(&scroller);
                        }
                        let bounds = button.compute_bounds(&scroller).unwrap();
                        assert!(bounds.y() >= -1.0);
                        assert!(bounds.y() + bounds.height() <= scroller.height() as f32 + 1.0);
                    };
                    scroll_to(create_profile);
                    assert!(panel_content.is_drawable());
                    if let Some(directory) = &directory {
                        rendered_texture(&capture_window, &panel_content)
                            .save_to_png(
                                directory
                                    .join(format!("profile-create-panel-{width}x{height}.png")),
                            )
                            .unwrap();
                    }
                    scroll_to(save_profile);
                    if let Some(directory) = &directory {
                        rendered_texture(&capture_window, &panel_content)
                            .save_to_png(
                                directory.join(format!("profile-save-panel-{width}x{height}.png")),
                            )
                            .unwrap();
                    }
                    capture_stage.remove(&panel_content);
                    panel_content.set_size_request(-1, -1);
                    panel.set_child(Some(&panel_content));
                    capture_window.destroy();
                    management_menu.popdown();
                    panel.set_autohide(autohide);
                }
            }
        }
        let saved = std::fs::read(&path).unwrap();
        ui.action_picker.set_selected(1);
        ui.update_action_visibility();
        ui.text_entry.set_text("");
        save.emit_clicked();
        assert!(toast.root.reveals_child());
        assert!(toast.label.has_css_class("feedback-error"));
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        commands.send(LayerCommand::Shutdown).unwrap();
        worker.join().unwrap();
        ui.text_entry.set_text("toast-test");
        save.emit_clicked();
        assert!(toast.root.reveals_child());
        assert!(toast.label.has_css_class("feedback-error"));
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        ui::assert_toast_lifecycle(&toast);
        polling.remove();
        window.destroy();
        std::fs::remove_dir_all(root).unwrap();
        eprintln!("GTK save toast executed: binding and profile creation/modification, real disk success/error, reopen, validation, closed worker, timer and dismissal");
    }

    #[test]
    fn keyboard_worker_failure_reports_layer_error_without_process_fallback() {
        struct NoProcess;
        impl sdeck_plus::layers::ProcessRunner for NoProcess {
            fn run(&mut self, _: &[String]) -> std::io::Result<()> {
                panic!("keyboard action must not execute a process");
            }
        }
        struct DeniedKeyboard;
        impl sdeck_plus::layers::KeyboardSender for DeniedKeyboard {
            fn send(&mut self, _: &sdeck_plus::keyboard::Chord) -> std::io::Result<()> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Could not open /dev/uinput: access denied",
                ))
            }
        }
        let mut executor = sdeck_plus::layers::ActionExecutor::new(NoProcess, DeniedKeyboard);
        let (output, receiver) = mpsc::channel();
        execute_layer_action(
            &mut executor,
            &Action::Shortcut {
                modifiers: vec![],
                key: "CTRL + D".into(),
            },
            &output,
        );
        let MonitorEvent::LayerError(error) = receiver.try_recv().unwrap() else {
            panic!("expected the existing LayerError boundary");
        };
        assert!(error.contains("Could not execute layer action"));
        assert!(error.contains("/dev/uinput: access denied"));
    }

    #[test]
    fn explicit_shutdown_ends_worker_even_with_live_channel_senders() {
        // Given: a real, lazy worker and a sender retained by UI closures.
        let (sender, receiver) = mpsc::channel();
        let retained_sender = sender.clone();
        let (output, _events) = mpsc::channel();
        let (finished, completion) = mpsc::channel();
        let worker = thread::spawn(move || {
            feature_runtime::run_worker(receiver, output, Default::default());
            finished.send(()).unwrap();
        });
        // When: close explicitly requests shutdown.
        sender.send(LayerCommand::Shutdown).unwrap();
        // Then: the executor drops before the retained sender disappears.
        completion.recv_timeout(Duration::from_secs(3)).unwrap();
        worker.join().unwrap();
        assert!(retained_sender.send(LayerCommand::Shutdown).is_err());
    }

    #[test]
    fn worker_toggle_identity_is_independent_for_equal_actions() {
        use sdeck_plus::desktop_actions::DesktopAction;
        use sdeck_plus::features::FeatureAction;
        struct NoProcess;
        impl sdeck_plus::layers::ProcessRunner for NoProcess {
            fn run(&mut self, _: &[String]) -> std::io::Result<()> {
                panic!("toggle cannot spawn a process");
            }
        }
        struct Recorder(std::sync::Arc<Mutex<Vec<String>>>);
        impl sdeck_plus::layers::KeyboardSender for Recorder {
            fn send(&mut self, chord: &sdeck_plus::keyboard::Chord) -> std::io::Result<()> {
                self.0.lock().unwrap().push(format!("{chord:?}"));
                Ok(())
            }
        }
        // Given: equal toggle actions on two distinct physical bindings.
        let recorded = std::sync::Arc::new(Mutex::new(Vec::new()));
        let mut executor =
            sdeck_plus::layers::ActionExecutor::new(NoProcess, Recorder(recorded.clone()));
        let action = Action::Feature(FeatureAction::Desktop(DesktopAction::ShortcutToggle {
            first: "CTRL+A".into(),
            second: "CTRL+B".into(),
        }));
        let first = binding_identity("Base", &Trigger::KeyPress(1));
        let second = binding_identity("Base", &Trigger::KeyPress(2));
        let (output, events) = mpsc::channel();
        // When: both bindings are explicitly executed twice.
        for identity in [&first, &second, &first, &second] {
            execute_layer_action_for(&mut executor, identity, &action, &output);
            assert!(matches!(
                events.try_recv().unwrap(),
                MonitorEvent::AppEvent { name } if name == "Action dispatched."
            ));
            assert!(matches!(
                events.try_recv().unwrap(),
                MonitorEvent::ActionDispatched
            ));
            assert!(matches!(events.try_recv(), Err(mpsc::TryRecvError::Empty)));
        }
        // Then: each binding begins with the first chord and advances independently.
        let chord = |text| {
            format!(
                "{:?}",
                sdeck_plus::keyboard::parse_chord(&[], text).unwrap()
            )
        };
        assert_eq!(
            *recorded.lock().unwrap(),
            [
                chord("CTRL+A"),
                chord("CTRL+A"),
                chord("CTRL+B"),
                chord("CTRL+B")
            ]
        );
    }

    #[test]
    fn shutdown_preserves_latest_metadata_snapshot_without_queued_effects() {
        use sdeck_plus::organization::Organization;
        // Given: a saved page snapshot followed by a queued effect, during close.
        let root = std::env::temp_dir().join(format!(
            "streamdeck-shutdown-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = LayerStore::open_at(&root).unwrap();
        let mut organization = Organization::open_at(&root, &store).unwrap();
        organization.create_page(&mut store).unwrap();
        let (sender, receiver) = mpsc::channel();
        sender
            .send(LayerCommand::Persist(
                store.clone(),
                Some(organization.clone()),
            ))
            .unwrap();
        store
            .bind(
                store.active().to_owned().as_str(),
                Trigger::KeyPress(1),
                Action::Emit("closing-save".into()),
            )
            .unwrap();
        organization.rename_page(&store, "Closing page").unwrap();
        sender
            .send(LayerCommand::SaveBinding(store, Some(organization)))
            .unwrap();
        sender
            .send(LayerCommand::Execute {
                binding_id: "cancelled".into(),
                action: Action::Emit("cancelled".into()),
            })
            .unwrap();
        sender.send(LayerCommand::Shutdown).unwrap();
        let (output, events) = mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        // When: the real worker drains shutdown state.
        feature_runtime::run_worker(receiver, output, stop);
        // Then: metadata and selected layer persist, without admitting the effect.
        let store = LayerStore::open_at(&root).unwrap();
        assert_eq!(
            Organization::open_at(&root, &store)
                .unwrap()
                .current(&store)
                .page,
            2
        );
        assert!(events.try_recv().is_err());
        assert_eq!(
            store.layers()[store.active()].bindings[&Trigger::KeyPress(1)],
            Action::Emit("closing-save".into())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn assert_shortcut_only_menu(ui: &BindingUi) {
        let model = ui.action_picker.model().unwrap();
        let labels: Vec<_> = (0..model.n_items())
            .map(|index| {
                model
                    .item(index)
                    .unwrap()
                    .downcast::<gtk::StringObject>()
                    .unwrap()
                    .string()
                    .to_string()
            })
            .collect();
        assert_eq!(
            labels,
            [
                "Go to profile or page",
                "Run command",
                "Open application",
                "Shortcut",
                "Send named event",
                "More actions",
                "Play sound",
                "None",
            ]
        );
        for (index, kind) in [
            ActionKind::MoveToLayer,
            ActionKind::RunCommand,
            ActionKind::OpenApplication,
            ActionKind::Shortcut,
            ActionKind::EmitLegacy,
            ActionKind::Extended,
            ActionKind::Extended,
            ActionKind::None,
        ]
        .into_iter()
        .enumerate()
        {
            ui.action_picker.set_selected(index as u32);
            assert_eq!(ui.action_kind(), kind);
            ui.update_action_visibility();
            assert_eq!(ui.layer_target_row.is_visible(), index == 0);
            assert_eq!(ui.application_row.is_visible(), index == 2);
            assert_eq!(ui.args_row.is_visible(), index == 2);
            assert_eq!(ui.modifier_row.is_visible(), index == 3);
            assert_eq!(ui.text_row.is_visible(), matches!(index, 1 | 3 | 4));
        }
    }

    fn assert_unsaved_edits_are_flagged_and_shortcut_is_previewed(ui: &BindingUi) {
        let root = std::env::temp_dir().join(format!(
            "streamdeck-dirty-editor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let layers = RefCell::new(LayerStore::open_at(&root).unwrap());
        ui.layer_picker.set_selected(0);
        ui.select_control(ControlKind::Button, Some(1), true)
            .unwrap();
        ui.load_selected_binding(&layers);
        assert!(!ui.is_dirty(), "loading must leave the editor clean");
        assert_ne!(ui.status.text(), "Unsaved changes.");
        ui.action_picker.set_selected(3);
        ui.text_entry.set_text("p");
        ui.ctrl_mod.set_active(true);
        assert_eq!(ui.status.text(), "Unsaved changes.");
        assert_eq!(ui.shortcut_preview.text(), "Presses Ctrl + P");
        // Saving treats the current fields as the new baseline.
        ui.mark_clean();
        ui.status.set_text("Binding updated; saving to disk…");
        assert_eq!(ui.status.text(), "Binding updated; saving to disk…");
        ui.text_entry.set_text("q");
        assert_eq!(ui.status.text(), "Unsaved changes.");
        // Reloading the control discards the edits and clears the flag.
        ui.load_selected_binding(&layers);
        assert_ne!(ui.status.text(), "Unsaved changes.");
        ui.action_picker.set_selected(3);
        ui.text_entry.set_text("not-a-key");
        assert_eq!(ui.shortcut_preview.text(), "");
        assert!(!ui.shortcut_preview.is_visible());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn assert_discard_prompt_guards_unsaved_edits() {
        let window = gtk::Window::new();
        let anchor = Button::new();
        window.set_child(Some(&anchor));
        let switched = Rc::new(Cell::new(0));
        let proceed: Rc<dyn Fn()> = {
            let switched = switched.clone();
            Rc::new(move || switched.set(switched.get() + 1))
        };
        confirm_discard(&anchor, proceed.clone());
        discard_dialog().response(ResponseType::Cancel);
        assert_eq!(switched.get(), 0, "Keep editing must not switch controls");
        confirm_discard(&anchor, proceed.clone());
        discard_dialog().response(ResponseType::Accept);
        assert_eq!(switched.get(), 1, "Discard changes must switch controls");
        // With no window there is nobody to ask, so the switch just happens.
        confirm_discard(&Button::new(), proceed);
        assert_eq!(switched.get(), 2);
        window.destroy();
    }

    fn assert_legacy_keys_load_without_rewriting_and_save_as_shortcuts(ui: &BindingUi) {
        let root = std::env::temp_dir().join(format!(
            "streamdeck-legacy-editor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = LayerStore::open_at(&root).unwrap();
        let config = "streamdeck-layers-v1\nlayer\t42617365\nbinding\t42617365\tk:1\tkey:72657475726e\nbinding\t42617365\tk:2\tkey:58463836556e737570706f72746564\n";
        std::fs::write(root.join("layers.conf"), config).unwrap();
        drop(store);
        let layers = RefCell::new(LayerStore::open_at(&root).unwrap());
        assert!(layers.borrow().diagnostics().is_empty());
        ui.layer_picker.set_selected(0);
        for (number, key) in [(1, "return"), (2, "XF86Unsupported")] {
            // Stale checkbox state must not leak into a modifier-free legacy action.
            ui.ctrl_mod.set_active(true);
            ui.select_control(ControlKind::Button, Some(number), true)
                .unwrap();
            ui.load_selected_binding(&layers);
            assert_eq!(ui.action_kind(), ActionKind::Shortcut);
            assert_eq!(ui.text_entry.text(), key);
            assert!(ui.shortcut_modifiers().is_empty());
            assert_eq!(
                existing_action(&layers.borrow(), "Base", &Trigger::KeyPress(number)),
                Some(Action::KeyPress { key: key.into() })
            );
            assert_eq!(
                std::fs::read_to_string(root.join("layers.conf")).unwrap(),
                config
            );
            if number == 2 {
                assert!(ui.binding_from_widgets(&layers).is_err());
                assert_eq!(
                    std::fs::read_to_string(root.join("layers.conf")).unwrap(),
                    config
                );
            }
        }
        ui.select_control(ControlKind::Button, Some(1), true)
            .unwrap();
        ui.load_selected_binding(&layers);
        let (layer, trigger, action) = ui.binding_from_widgets(&layers).unwrap();
        assert_eq!(trigger, Trigger::KeyPress(1));
        assert_eq!(
            action,
            Action::Shortcut {
                modifiers: vec![],
                key: "ENTER".into()
            }
        );
        layers
            .borrow_mut()
            .bind(&layer, trigger, action.clone())
            .unwrap();
        layers.borrow().persist().unwrap();
        let reloaded = LayerStore::open_at(&root).unwrap();
        assert_eq!(
            existing_action(&reloaded, "Base", &Trigger::KeyPress(1)),
            Some(action)
        );
        assert_eq!(
            existing_action(&reloaded, "Base", &Trigger::KeyPress(2)),
            Some(Action::KeyPress {
                key: "XF86Unsupported".into()
            })
        );
        eprintln!("GTK legacy key inspection, unsupported-name preservation and explicit Shortcut save assertions executed");
    }

    fn assert_physical_shortcut_widgets(ui: &BindingUi) {
        let path = std::env::temp_dir().join(format!("streamdeck-chord-ui-{}", std::process::id()));
        let layers = RefCell::new(LayerStore::open_at(path).unwrap());
        ui.select_control(ControlKind::Button, Some(1), true)
            .unwrap();
        ui.action_picker.set_selected(3);
        ui.clear_action_fields();
        for (text, modifier, key) in [
            ("CTRL + D", "CTRL", "D"),
            ("SUPER + RETURN", "SUPER", "ENTER"),
        ] {
            ui.text_entry.set_text(text);
            let (_, _, action) = ui.binding_from_widgets(&layers).unwrap();
            assert_eq!(
                action,
                Action::Shortcut {
                    modifiers: vec![modifier.into()],
                    key: key.into()
                }
            );
        }
        ui.ctrl_mod.set_active(true);
        let (_, _, action) = ui.binding_from_widgets(&layers).unwrap();
        assert_eq!(
            action,
            Action::Shortcut {
                modifiers: vec!["CTRL".into(), "SUPER".into()],
                key: "ENTER".into()
            }
        );
        ui.text_entry.set_text("CTRL +");
        assert!(ui.binding_from_widgets(&layers).is_err());
        ui.clear_action_fields();
        eprintln!("GTK physical shortcut production parser assertions executed");
    }

    fn assert_visual_palette_contrast() {
        let dark = include_str!("ui.css");
        let light = include_str!("ui-light.css");
        for css in [dark, light] {
            let luminance = |token: &str| {
                let prefix = format!("@define-color {token} #");
                let hex = css
                    .lines()
                    .find_map(|line| line.strip_prefix(&prefix))
                    .unwrap();
                let channel = |offset| {
                    let value =
                        f64::from(u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
                            / 255.0;
                    if value <= 0.04045 {
                        value / 12.92
                    } else {
                        ((value + 0.055) / 1.055).powf(2.4)
                    }
                };
                0.2126 * channel(0) + 0.7152 * channel(2) + 0.0722 * channel(4)
            };
            for (foreground, background, minimum) in [
                ("desk_control_edge", "desk_field", 3.0),
                ("desk_control_edge", "desk_surface", 3.0),
                ("desk_cyan", "desk_surface", 3.0),
                ("desk_ink", "desk_surface", 4.5),
                ("desk_muted", "desk_surface", 4.5),
                ("desk_ink", "desk_selection", 4.5),
                ("desk_bg", "desk_violet", 4.5),
            ] {
                let a = luminance(foreground);
                let b = luminance(background);
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(
                    ratio >= minimum,
                    "{foreground}/{background}: {ratio:.2}:1 < {minimum}:1"
                );
            }
        }
    }

    fn assert_production_layout_fits() {
        assert_visual_palette_contrast();
        let window = ApplicationWindow::builder()
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .hide_on_close(true)
            .build();
        install_style(&window);
        let layers = Rc::new(RefCell::new(
            LayerStore::open_at(
                std::env::temp_dir().join(format!("streamdeck-layout-{}", std::process::id())),
            )
            .unwrap(),
        ));
        let (_, _, store, source) = artwork_fixture("shared-editor");
        let (output, _receiver) = mpsc::channel();
        let artwork = ArtworkUi {
            store: Rc::new(RefCell::new(Some(store))),
            layers: layers.clone(),
            pictures: Rc::new(RefCell::new(Vec::new())),
            output,
            last_layer: Rc::new(RefCell::new(None)),
            chooser_owners: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
            selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
            text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
            info: Rc::default(),
        };
        let (sender, commands) = mpsc::channel();
        let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), sender);
        let shell = widgets
            .root
            .child()
            .unwrap()
            .downcast::<gtk::Viewport>()
            .unwrap()
            .child()
            .unwrap()
            // The width clamp wraps the shell.
            .first_child()
            .unwrap();
        let motion = shell.settings();
        let animations = motion.is_gtk_enable_animations();
        motion.set_gtk_enable_animations(false);
        assert!(!shell.has_css_class("motion-enabled"));
        assert!(!widgets
            .help_menu
            .popover()
            .unwrap()
            .has_css_class("motion-enabled"));
        motion.set_gtk_enable_animations(true);
        assert!(shell.has_css_class("motion-enabled"));
        assert!(widgets
            .help_menu
            .popover()
            .unwrap()
            .has_css_class("motion-enabled"));
        motion.set_gtk_enable_animations(animations);
        window.set_child(Some(&widgets.root));
        widgets.artwork_section.set_expanded(true);
        let mut artwork_buttons = Vec::new();
        collect_layout_buttons(widgets.root.upcast_ref(), &mut artwork_buttons);
        assert_eq!(
            artwork_buttons
                .iter()
                .filter(|button| button_caption(button).as_deref() == Some("Image…"))
                .count(),
            1,
            "one shared artwork chooser, not thirteen per-target rows"
        );
        let mapped = Rc::new(Cell::new(false));
        let main_loop = glib::MainLoop::new(None, false);
        let loop_on_map = main_loop.clone();
        let mapped_on_signal = mapped.clone();
        let map_signal = window.connect_map(move |_| {
            mapped_on_signal.set(true);
            loop_on_map.quit();
        });
        let loop_on_timeout = main_loop.clone();
        let timed_out = Rc::new(Cell::new(false));
        let timeout_flag = timed_out.clone();
        let timeout = glib::timeout_add_local_once(Duration::from_secs(5), move || {
            timeout_flag.set(true);
            loop_on_timeout.quit();
        });
        // GTK may update the default size after the window manager configures it.
        assert_eq!(
            (window.default_width(), window.default_height()),
            (DEFAULT_WIDTH, DEFAULT_HEIGHT)
        );
        window.set_visible(true);
        if !mapped.get() {
            main_loop.run();
        }
        window.disconnect(map_signal);
        if !timed_out.get() {
            timeout.remove();
        }
        assert!(mapped.get(), "window did not map before timeout");
        wait_for_frame(&window);
        eprintln!(
            "GTK requested initial size 1000x700; window-manager allocation {}x{}",
            window.width(),
            window.height()
        );
        // Tiling window managers can override requested defaults. Exercise the
        // actual GTK client at the requested size independently of that policy.
        window.allocate(DEFAULT_WIDTH, DEFAULT_HEIGHT, -1, None);
        assert_eq!(
            (window.width(), window.height()),
            (DEFAULT_WIDTH, DEFAULT_HEIGHT)
        );
        eprintln!("GTK actual test client allocation 1000x700 executed");
        let content = widgets
            .root
            .child()
            .unwrap()
            .downcast::<gtk::Viewport>()
            .unwrap()
            .child()
            .unwrap()
            // The width clamp wraps the shell.
            .first_child()
            .unwrap();
        if std::env::var_os("STREAMDECK_QA_DIR").is_some() {
            widgets.artwork_section.set_expanded(false);
            let _ = rendered_pixels(&window, &widgets.root);
        }
        assert_scrollable_workspace(&widgets, &content, &artwork, "empty");
        assert_inactive_backgrounds(&window, &widgets, &content, &artwork);
        assert_compact_responsive_regions(&widgets, &content);
        assert_sidebar_hierarchy(&widgets, &content);
        assert_shared_artwork_editor(&widgets, &content, &artwork, &source);
        let (content_minimum, content_natural, _, _) = content.measure(Orientation::Horizontal, -1);
        eprintln!("GTK production content horizontal minimum={content_minimum}, natural={content_natural}");
        assert!(
            content_minimum <= 936,
            "compact canvas and sidebar exceed 960px: {content_minimum}"
        );
        let (minimum, natural, _, _) = widgets.root.measure(Orientation::Horizontal, -1);
        eprintln!("GTK production layout horizontal minimum={minimum}, natural={natural}");
        assert!(
            minimum <= 960,
            "production layout forces an oversized window: {minimum}"
        );
        widgets.root.allocate(960, 640, -1, None);
        assert_artwork_geometry(&artwork, &content, false);
        let empty_sizes: Vec<_> = artwork
            .pictures
            .borrow()
            .iter()
            .map(|(_, picture)| {
                let surface = picture.parent().unwrap();
                (surface.width(), surface.height())
            })
            .collect();
        // Stress real models and editors with long values, not just the empty defaults.
        let long_name = "A deliberately long layer name for layout regression";
        layers.borrow_mut().create(long_name).unwrap();
        let names = layers.borrow().names();
        let ui = &widgets.binding_ui;
        assert_shortcut_only_menu(ui);
        replace_string_list(&ui.layer_model, &names);
        ui.refresh_layer_targets(&names);
        widgets.layer_picker.set_selected(1);
        ui.status.set_text(&"long-status-".repeat(60));
        ui.text_entry.set_text(&"command ".repeat(100));
        ui.application_entry
            .set_text(&"/long-executable".repeat(60));
        ui.args_view.buffer().set_text(&"argument ".repeat(100));
        // Isolated PNG payloads exercise Picture's intrinsic aspect ratios, not
        // merely the size requests of empty visible widgets. No artwork store/device writes.
        let assets = std::env::temp_dir().join(format!(
            "streamdeck-layout-images-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&assets).unwrap();
        for (index, (target, picture)) in artwork.pictures.borrow().iter().enumerate() {
            let (width, height) = match target {
                ArtworkTarget::Keypad(_) => (120, 120),
                ArtworkTarget::TouchStripBackground => (800, 100),
                ArtworkTarget::EncoderZone(_) => (200, 100),
            };
            let image = gtk::gdk_pixbuf::Pixbuf::new(
                gtk::gdk_pixbuf::Colorspace::Rgb,
                false,
                8,
                width,
                height,
            )
            .unwrap();
            image.fill(0x4488ffff);
            let path = assets.join(format!("target-{index}.png"));
            image.savev(&path, "png", &[]).unwrap();
            set_picture_path(picture, Some(path));
            let paintable = picture
                .paintable()
                .expect("PNG payload loaded into Picture");
            assert_eq!(paintable.intrinsic_width(), width);
            assert_eq!(paintable.intrinsic_height(), height);
            assert!(picture.is_visible());
        }
        eprintln!("GTK production image payload assertions executed: 8 keys 120x120, strip 800x100, 4 zones 200x100");
        widgets.root.allocate(960, 640, -1, None);
        for ((_, picture), empty_size) in artwork.pictures.borrow().iter().zip(empty_sizes) {
            let surface = picture.parent().unwrap();
            assert_eq!(
                empty_size,
                (surface.width(), surface.height()),
                "loading artwork changes surface bounds"
            );
        }
        assert_scrollable_workspace(&widgets, &content, &artwork, "loaded long values");
        for (width, height) in [(1840, 980), (1280, 720), (960, 640), (900, 700), (640, 480)] {
            widgets.root.allocate(width, height, -1, None);
            assert_eq!(widgets.root.width(), width);
            assert_eq!(widgets.root.height(), height);
            // Keys take their action from the catalog, so the selector stays hidden.
            assert!(!widgets.binding_ui.action_picker.is_visible());
            assert!(widgets.feature_runtime.profile_tabs().is_visible());
            assert_eq!(artwork.pictures.borrow().len(), 13);
            assert_artwork_geometry(&artwork, &content, true);
            assert_sidebar_geometry(&widgets, &content);
            let horizontal = widgets.root.hadjustment();
            let vertical = widgets.root.vadjustment();
            eprintln!("GTK production allocation {width}x{height}: content {}x{}, horizontal upper/page={}/{}, vertical upper/page={}/{}",
                content.width(), content.height(), horizontal.upper(), horizontal.page_size(), vertical.upper(), vertical.page_size());
            assert!(
                horizontal.upper() <= horizontal.page_size(),
                "responsive configuration must not require horizontal scrolling"
            );
            assert!(ui.action_picker.compute_bounds(&content).is_some());
            assert!(widgets
                .feature_runtime
                .profile_tabs()
                .compute_bounds(&content)
                .is_some());
            for (_, picture) in artwork.pictures.borrow().iter() {
                let controls = picture.parent().unwrap();
                assert!(controls.compute_bounds(&content).is_some());
            }
            for action in 0..5 {
                ui.action_picker.set_selected(action);
                ui.update_action_visibility();
                widgets.root.allocate(width, height, -1, None);
                assert_sidebar_geometry(&widgets, &content);
                let fields = ui.text_row.parent().unwrap().parent().unwrap();
                assert_eq!(fields.is_visible(), action != 0);
                for row in [
                    &ui.text_row,
                    &ui.modifier_row,
                    &ui.application_row,
                    &ui.args_row,
                ] {
                    assert_eq!(row.parent().unwrap().is_visible(), row.is_visible());
                }
                if matches!(action, 1 | 4) {
                    let field = ui.text_row.compute_bounds(&content).unwrap();
                    let group = fields.compute_bounds(&content).unwrap();
                    assert!(
                        (field.width() - group.width()).abs() <= 1.0,
                        "standalone action field must fill its group: {field:?}, {group:?}"
                    );
                }
                if matches!(action, 2 | 3) {
                    let (first, second) = if action == 2 {
                        (&ui.application_row, &ui.args_row)
                    } else {
                        (&ui.text_row, &ui.modifier_row)
                    };
                    let first = first.compute_bounds(&content).unwrap();
                    let second = second.compute_bounds(&content).unwrap();
                    assert!(
                        second.x() >= first.x() + first.width() + 15.0
                            || second.y() >= first.y() + first.height() + 11.0,
                        "related action fields overlap or lose their gap: {first:?}, {second:?}"
                    );
                    if width >= 1000 {
                        assert!((first.y() - second.y()).abs() <= 1.0);
                    }
                }
                let (action_minimum, _, _, _) = content.measure(Orientation::Horizontal, -1);
                assert!(
                    action_minimum <= 936,
                    "action {action} has oversized editor: {action_minimum}"
                );
                eprintln!("GTK production image-backed action {action} at {width}x{height}: content {}x{}, minimum {action_minimum}, horizontal upper/page={}/{}, vertical upper/page={}/{}",
                    content.width(), content.height(), horizontal.upper(), horizontal.page_size(), vertical.upper(), vertical.page_size());
                if width >= 960 {
                    assert!(horizontal.upper() <= horizontal.page_size());
                }
                assert_reachable(&ui.action_picker, &widgets.root);
                for row in [
                    &ui.layer_target_row,
                    &ui.text_row,
                    &ui.application_row,
                    &ui.args_row,
                    &ui.modifier_row,
                ] {
                    if row.is_visible() {
                        assert_reachable(row, &widgets.root);
                    }
                }
                let mut buttons = Vec::new();
                collect_layout_buttons(&content, &mut buttons);
                for button in buttons {
                    assert_reachable(&button, &widgets.root);
                }
                eprintln!("GTK production image-backed action {action} editor/button reachability assertions executed at {width}x{height}");
            }
            assert_reachable(&widgets.layer_picker, &widgets.root);
            let mut buttons = Vec::new();
            collect_layout_buttons(&content, &mut buttons);
            assert_eq!(
                buttons
                    .iter()
                    .filter(|button| button_caption(button).as_deref() == Some("Image…"))
                    .count(),
                1
            );
            for button in buttons {
                assert_reachable(&button, &widgets.root);
            }
            vertical.set_value(0.0);
            eprintln!("GTK production layer/editor/artwork reachability assertions executed at {width}x{height}");
        }
        eprintln!("GTK image-backed five-action sizing matrix executed, including two-axis fallback at 640x480");
        // Wide and portrait source paintables must retain their intrinsic proportions;
        // cover performs the crop in the preview, never a rewrite of the source.
        for (index, (_, picture)) in artwork.pictures.borrow().iter().enumerate() {
            let (width, height) = if index % 2 == 0 { (320, 80) } else { (80, 320) };
            let source = gtk::gdk_pixbuf::Pixbuf::new(
                gtk::gdk_pixbuf::Colorspace::Rgb,
                false,
                8,
                width,
                height,
            )
            .unwrap();
            source.fill(0x88cc44ff);
            picture.set_paintable(Some(&gtk::gdk::Texture::for_pixbuf(&source)));
            assert_eq!(picture.paintable().unwrap().intrinsic_width(), width);
            assert_eq!(picture.paintable().unwrap().intrinsic_height(), height);
            assert_eq!(picture.content_fit(), gtk::ContentFit::Cover);
        }
        widgets.root.allocate(640, 480, -1, None);
        assert_artwork_geometry(&artwork, &content, true);
        eprintln!("GTK wide/portrait aspect-preserving cover assertions executed");
        let loaded_bounds: Vec<_> = artwork
            .pictures
            .borrow()
            .iter()
            .map(|(_, picture)| picture.parent().unwrap().compute_bounds(&content).unwrap())
            .collect();
        for (_, picture) in artwork.pictures.borrow().iter() {
            set_picture_path(picture, None);
        }
        widgets.root.allocate(640, 480, -1, None);
        assert_artwork_geometry(&artwork, &content, false);
        for ((_, picture), before) in artwork.pictures.borrow().iter().zip(loaded_bounds) {
            let after = picture.parent().unwrap().compute_bounds(&content).unwrap();
            assert_eq!(
                before.width(),
                after.width(),
                "clearing changes surface width"
            );
            assert_eq!(
                before.height(),
                after.height(),
                "clearing changes surface height"
            );
        }
        eprintln!("GTK empty/loaded/cleared artwork geometry assertions executed");
        let command = "echo full editor text ".repeat(40);
        layers
            .borrow_mut()
            .bind(
                long_name,
                Trigger::KeyPress(8),
                Action::Command(command.clone()),
            )
            .unwrap();
        ui.refresh_summaries(&layers);
        for label in &widgets.encoder_labels {
            let knob = label.parent().unwrap().downcast::<Button>().unwrap();
            assert!(knob.has_css_class("encoder"));
            assert_eq!(knob.width(), knob.height(), "encoder must remain circular");
            assert!(
                knob.child().unwrap().is::<Label>(),
                "knobs are artwork-free"
            );
        }
        for (target, picture) in artwork.pictures.borrow().iter() {
            if matches!(target, ArtworkTarget::EncoderZone(_)) {
                let button = picture
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .downcast::<Button>()
                    .unwrap();
                ui.mark_clean();
                button.emit_clicked();
                assert_eq!(ui.control_picker.selected(), 2);
                assert_eq!(ui.event_model.n_items(), 7);
            }
        }
        let button = key_button(&widgets.key_labels[7]);
        ui.mark_clean();
        button.emit_clicked();
        assert_eq!(ui.control_number_picker.selected(), 7);
        assert_eq!(ui.text_entry.text(), command);
        assert_eq!(
            ui.summaries[7].tooltip_text().unwrap(),
            ui.summaries[7].text()
        );
        eprintln!("GTK production selection and full-editor assertions executed");
        assert_scrollable_workspace(&widgets, &content, &artwork, "cleared long values");
        capture_frontend_matrix(&window, &widgets, &artwork);
        assert_feature_integration(&window, &widgets, &layers, &commands);
        window.close();
    }

    fn capture_frontend_matrix(
        window: &ApplicationWindow,
        widgets: &MonitorWidgets,
        artwork: &ArtworkUi,
    ) {
        let Some(directory) = std::env::var_os("STREAMDECK_QA_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        let settings = widgets.root.settings();
        let theme = settings.gtk_theme_name();
        let dark = settings.is_gtk_application_prefer_dark_theme();
        for (name, prefer_dark) in [("dark", true), ("light", false)] {
            settings.set_gtk_theme_name(Some(if prefer_dark {
                "Adwaita-dark"
            } else {
                "Adwaita"
            }));
            settings.set_gtk_application_prefer_dark_theme(prefer_dark);
            capture_frontend_variant(window, widgets, artwork, &directory.join(name));
        }
        settings.set_gtk_theme_name(theme.as_deref());
        settings.set_gtk_application_prefer_dark_theme(dark);
    }

    fn capture_frontend_variant(
        window: &ApplicationWindow,
        widgets: &MonitorWidgets,
        artwork: &ArtworkUi,
        directory: &std::path::Path,
    ) {
        std::fs::create_dir_all(directory).unwrap();
        let settings = widgets.root.settings();
        let animations = settings.is_gtk_enable_animations();
        settings.set_gtk_enable_animations(false);
        // GtkFixed keeps the capture child at its requested size, independently
        // of the tiling compositor's allocation of the hosting window.
        let stage = gtk::Fixed::new();
        window.set_child(gtk::Widget::NONE);
        stage.put(&widgets.root, 0.0, 0.0);
        window.set_child(Some(&stage));
        widgets.artwork_section.set_expanded(false);
        for (width, height) in [(640, 480), (1000, 700), (1280, 800), (1840, 980)] {
            widgets.root.set_size_request(width, height);
            let capture = |name: &str| {
                widgets.root.vadjustment().set_value(0.0);
                let texture = rendered_texture(window, &widgets.root);
                assert_eq!((texture.width(), texture.height()), (width, height));
                assert!(
                    widgets.root.hadjustment().upper() <= widgets.root.hadjustment().page_size()
                );
                texture
                    .save_to_png(directory.join(format!("{width}x{height}-{name}-top.png")))
                    .unwrap();
                let adjustment = widgets.root.vadjustment();
                if adjustment.upper() > adjustment.page_size() {
                    adjustment.set_value(adjustment.upper() - adjustment.page_size());
                    rendered_texture(window, &widgets.root)
                        .save_to_png(directory.join(format!("{width}x{height}-{name}-bottom.png")))
                        .unwrap();
                }
            };
            for (name, control, number, button) in [
                (
                    "key-8",
                    ControlKind::Button,
                    Some(8),
                    key_button(&widgets.key_labels[7]),
                ),
                (
                    "dial-4",
                    ControlKind::Encoder,
                    Some(4),
                    widgets.encoder_labels[3]
                        .parent()
                        .unwrap()
                        .downcast::<Button>()
                        .unwrap(),
                ),
                (
                    "touchpad",
                    ControlKind::Touch,
                    None,
                    widgets
                        .touch_status
                        .parent()
                        .unwrap()
                        .next_sibling()
                        .unwrap()
                        .downcast::<gtk::Overlay>()
                        .unwrap()
                        .child()
                        .unwrap()
                        .downcast::<Button>()
                        .unwrap(),
                ),
            ] {
                button.emit_clicked();
                let heading = sidebar_settings(widgets)
                    .first_child()
                    .unwrap()
                    .downcast::<Label>()
                    .unwrap();
                assert_eq!(heading.text().as_str(), control_heading(control, number));
                capture(&format!("control-{name}"));
            }
            for (target, picture) in artwork.pictures.borrow().iter() {
                if let ArtworkTarget::EncoderZone(zone) = target {
                    picture
                        .parent()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .downcast::<Button>()
                        .unwrap()
                        .emit_clicked();
                    capture(&format!("touchpad-region-{zone}"));
                }
            }
            key_button(&widgets.key_labels[7]).emit_clicked();
            for action in [0, 1, 2, 3, 4, 6] {
                widgets.binding_ui.action_picker.set_selected(action);
                widgets.binding_ui.update_action_visibility();
                capture(&if action == 6 {
                    "play-sound".to_owned()
                } else {
                    format!("legacy-{action}")
                });
            }
            for (index, action) in feature_editor::examples().into_iter().enumerate() {
                widgets.binding_ui.populate_action(&Action::Feature(action));
                capture(&format!("feature-{index}"));
            }
        }
        widgets.binding_ui.action_picker.set_selected(0);
        widgets.binding_ui.update_action_visibility();
        for (width, height) in [(640, 480), (1000, 700), (1280, 800), (1840, 980)] {
            widgets.root.set_size_request(width, height);
            widgets.root.vadjustment().set_value(0.0);
            wait_for_frame(&widgets.root);
            for (name, menu) in [
                ("help", &widgets.help_menu),
                ("organization", &widgets.management_menu),
                ("folders", widgets.feature_runtime.folder_menu()),
                ("obs", &widgets.obs_menu),
            ] {
                let panel = menu.popover().unwrap();
                // Capture on a shared desktop without outside focus changes
                // dismissing the panel between its top and scrolled-end frames.
                let autohide = panel.is_autohide();
                panel.set_autohide(false);
                menu.popup();
                let texture = rendered_texture(window, &panel);
                eprintln!(
                    "GTK panel {name} at {width}x{height}: {}x{}",
                    panel.width(),
                    panel.height()
                );
                texture
                    .save_to_png(directory.join(format!("{width}x{height}-popover-{name}.png")))
                    .unwrap();
                if width == 1000 && name == "organization" {
                    if let Some(docs) = std::env::var_os("STREAMDECK_QA_DOCS_DIR") {
                        let docs = std::path::PathBuf::from(docs);
                        std::fs::create_dir_all(&docs).unwrap();
                        texture
                            .save_to_png(docs.join("organize-profiles-pages.png"))
                            .unwrap();
                    }
                }
                assert!(panel.width() <= width, "{name} exceeds the client width");
                assert!(panel.height() <= height, "{name} exceeds the client height");
                let content = panel.child().unwrap();
                let header = content.first_child().unwrap();
                let close = header.last_child().unwrap().downcast::<Button>().unwrap();
                // The configuration menu is a compact button group, while
                // organization/folders/OBS wrap their longer forms in a scroller.
                if let Ok(scroller) = content
                    .last_child()
                    .unwrap()
                    .downcast::<gtk::ScrolledWindow>()
                {
                    scroller.vadjustment().set_value(
                        scroller.vadjustment().upper() - scroller.vadjustment().page_size(),
                    );
                }
                rendered_texture(window, &panel)
                    .save_to_png(directory.join(format!("{width}x{height}-popover-{name}-end.png")))
                    .unwrap();
                let close_bounds = close.compute_bounds(&content).unwrap();
                assert!(close_bounds.y() >= 0.0);
                assert!(close_bounds.y() + close_bounds.height() <= header.height() as f32);
                close.emit_clicked();
                assert!(!panel.is_visible(), "popover Close must dismiss its panel");
                panel.set_autohide(autohide);
            }
            page_ui_tests::capture_page_workflow(window, widgets, (width, height));
        }
        eprintln!("GTK exact-size captures executed: 640x480, 1000x700, 1280x800, 1840x980, all actions, four popovers and page workflows");
        window.set_child(gtk::Widget::NONE);
        stage.remove(&widgets.root);
        widgets.root.set_size_request(-1, -1);
        window.set_child(Some(&widgets.root));
        settings.set_gtk_enable_animations(animations);
    }

    fn assert_feature_integration(
        window: &ApplicationWindow,
        widgets: &MonitorWidgets,
        layers: &Rc<RefCell<LayerStore>>,
        commands: &mpsc::Receiver<LayerCommand>,
    ) {
        use sdeck_plus::desktop_actions::{DesktopAction, MediaCommand};
        use sdeck_plus::features::FeatureAction;
        use sdeck_plus::organization::NavigationAction;
        let ui = &widgets.binding_ui;
        ui.action_picker.set_selected(5);
        ui.feature_editor.assert_roundtrips();
        let feature = FeatureAction::Desktop(DesktopAction::Media(MediaCommand::Mute));
        ui.populate_action(&Action::Feature(feature.clone()));
        assert_eq!(ui.action_picker.selected(), 5);
        assert!(ui.feature_editor.root.is_visible());
        assert!(!ui.modifier_row.is_visible());
        let (layer, trigger, action) = ui.binding_from_widgets(layers).unwrap();
        assert_eq!(action, Action::Feature(feature));
        while commands.try_recv().is_ok() {}
        let mut buttons = Vec::new();
        collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
        let find = |label: &str| {
            buttons
                .iter()
                .find(|button| button_caption(button).as_deref() == Some(label))
                .unwrap()
        };
        find("Save binding").emit_clicked();
        assert!(matches!(
            commands.try_recv().unwrap(),
            LayerCommand::SaveBinding(_, Some(_))
        ));
        assert!(commands.try_recv().is_err(), "saving must not execute");
        ui.load_selected_binding(layers);
        assert_eq!(ui.action_picker.selected(), 5);
        find("Test action").emit_clicked();
        let LayerCommand::Execute {
            binding_id,
            action: tested,
        } = commands.try_recv().unwrap()
        else {
            panic!("explicit Test action must dispatch");
        };
        assert_eq!(binding_id, binding_identity(&layer, &trigger));
        assert_eq!(tested, action);
        assert!(commands.try_recv().is_err());

        // The direct sound option uses the existing typed audio action and save path.
        ui.action_picker.set_selected(6);
        assert!(ui.feature_editor.root.is_visible());
        assert!(!ui.application_row.is_visible());
        assert!(!ui.text_row.is_visible());
        let mut sound_entries = Vec::new();
        collect_entries(ui.feature_editor.root.upcast_ref(), &mut sound_entries);
        let path = sound_entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("/home/user/Music/track.ogg"))
            .unwrap();
        assert!(ui.binding_from_widgets(layers).is_err());
        for extension in ["wav", "mp3", "m4a", "flac", "ogg"] {
            let filename = format!("/tmp/recorded sound.{extension}");
            path.set_text(&filename);
            let expected = Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio {
                path: filename.into(),
            }));
            assert_eq!(ui.binding_from_widgets(layers).unwrap().2, expected);
            find("Save binding").emit_clicked();
            let LayerCommand::SaveBinding(snapshot, Some(_)) = commands.try_recv().unwrap() else {
                panic!("sound save must persist without executing");
            };
            // The saved binding holds the app's link; the link names the chosen file.
            let stored = &snapshot.layers()[&layer].bindings[&trigger];
            assert_ne!(stored, &expected, "the real path is not what is stored");
            assert_eq!(
                sdeck_plus::asset_links::unlink_action(snapshot.root(), stored),
                expected
            );
            assert!(commands.try_recv().is_err());
            ui.load_selected_binding(layers);
            assert_eq!(ui.action_picker.selected(), 6);
            assert_eq!(ui.binding_from_widgets(layers).unwrap().2, expected);
            find("Test action").emit_clicked();
            let LayerCommand::Execute { action, .. } = commands.try_recv().unwrap() else {
                panic!("explicit sound test must dispatch playback");
            };
            assert_eq!(action, expected);
            assert!(commands.try_recv().is_err());
        }

        // Remove deletes the binding, and the feedback line can bring it back.
        let removed_action = layers.borrow().binding(&layer, &trigger).unwrap();
        find("Remove binding").emit_clicked();
        while commands.try_recv().is_ok() {}
        assert!(layers.borrow().binding(&layer, &trigger).is_none());
        let undo = ui.undo_remove.clone();
        assert!(undo.is_visible());
        undo.emit_clicked();
        assert_eq!(
            layers.borrow().binding(&layer, &trigger),
            Some(removed_action)
        );
        assert!(matches!(
            commands.try_recv().unwrap(),
            LayerCommand::SaveBinding(..)
        ));
        assert!(!undo.is_visible());
        while commands.try_recv().is_ok() {}

        let original = layers.borrow().active().to_owned();
        let keys = layers.borrow().names();
        let folder_count = widgets
            .feature_runtime
            .folder_menu()
            .child()
            .unwrap()
            .last_child()
            .unwrap()
            .downcast::<Label>()
            .unwrap();
        let previous_count = folder_count.text().parse::<usize>().unwrap();
        find("New page").emit_clicked();
        let dialog = current_dialog();
        assert_eq!(
            layers.borrow().names(),
            keys,
            "prompting must not create a page"
        );
        dialog.response(ResponseType::Cancel);
        assert_eq!(layers.borrow().active(), original);
        assert!(commands.try_recv().is_err());
        find("New page").emit_clicked();
        let dialog = current_dialog();
        let page_name = page_dialog_entry(&dialog);
        dialog.response(ResponseType::Accept);
        assert!(dialog.is_visible(), "invalid names keep the prompt open");
        assert_eq!(layers.borrow().names(), keys);
        assert!(commands.try_recv().is_err());
        page_name.set_text("Audio Mix");
        dialog.response(ResponseType::Accept);
        let active = layers.borrow().active().to_owned();
        assert_ne!(active, original);
        let LayerCommand::Persist(snapshot, Some(metadata)) = commands.try_recv().unwrap() else {
            panic!("named page creation persists metadata without executing");
        };
        assert_eq!(metadata.current(&snapshot).page_name, "Audio Mix");
        assert_eq!(metadata.current(&snapshot).page, 2);
        assert_eq!(ui.layer_picker.selected(), layer_index(layers, &active));
        assert!(commands.try_recv().is_err());

        find("Rename page").emit_clicked();
        let dialog = current_dialog();
        let page_name = page_dialog_entry(&dialog);
        assert_eq!(page_name.text(), "Audio Mix");
        page_name.set_text("");
        dialog.response(ResponseType::Accept);
        assert!(dialog.is_visible());
        assert_eq!(layers.borrow().active(), active);
        assert!(commands.try_recv().is_err());
        page_name.set_text("Keyboard Mix");
        page_name.emit_activate();
        let LayerCommand::Persist(snapshot, Some(metadata)) = commands.try_recv().unwrap() else {
            panic!("Enter must save the valid page name from its dialog");
        };
        assert_eq!(metadata.current(&snapshot).page_name, "Keyboard Mix");
        assert_eq!(snapshot.active(), active);
        assert!(commands.try_recv().is_err());

        let mut entries = Vec::new();
        let folder_panel = widgets.feature_runtime.folder_menu().popover().unwrap();
        assert_eq!(folder_count.text().parse::<usize>().unwrap(), 0);
        collect_entries(folder_panel.upcast_ref(), &mut entries);
        let folder_name = entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("New folder name"))
            .unwrap();
        folder_name.set_text("Studio");
        find("New folder").emit_clicked();
        assert_eq!(
            layers.borrow().active(),
            active,
            "creating keeps the owning page selected"
        );
        assert_eq!(folder_count.text().parse::<usize>().unwrap(), 1);
        assert_eq!(folder_name.text(), "", "a created name is cleared");
        folder_button(widgets, "Open folder Studio").emit_clicked();
        let folder_key = layers.borrow().active().to_owned();
        assert_ne!(folder_key, active);
        assert_eq!(folder_count.text().parse::<usize>().unwrap(), 0);
        find("Parent").emit_clicked();
        assert_eq!(folder_count.text().parse::<usize>().unwrap(), 1);
        assert_eq!(
            layers.borrow().active(),
            active,
            "Parent returns to the owning page"
        );
        find("Previous").emit_clicked();
        assert_eq!(layers.borrow().active(), original);
        assert_eq!(
            folder_count.text().parse::<usize>().unwrap(),
            previous_count
        );
        find("Next").emit_clicked();
        assert_eq!(layers.borrow().active(), active);
        assert_eq!(folder_count.text().parse::<usize>().unwrap(), 1);
        while commands.try_recv().is_ok() {}
        folder_button(widgets, "Link folder Studio to control").emit_clicked();
        let LayerCommand::SaveBinding(snapshot, Some(_)) = commands.try_recv().unwrap() else {
            panic!("linking must save without entering the folder");
        };
        assert_eq!(snapshot.active(), active);
        assert_eq!(layers.borrow().active(), active);
        assert_eq!(
            ui.binding_from_widgets(layers).unwrap().2,
            Action::Feature(FeatureAction::Navigation(NavigationAction::OpenFolder(
                "Studio".into()
            )))
        );
        assert!(commands.try_recv().is_err());
        folder_button(widgets, "Open folder Studio").emit_clicked();
        assert_eq!(layers.borrow().active(), folder_key);
        assert_eq!(folder_count.text().parse::<usize>().unwrap(), 0);
        find("Parent").emit_clicked();
        while commands.try_recv().is_ok() {}
        folder_button(widgets, "Delete folder Studio").emit_clicked();
        current_dialog().response(ResponseType::Cancel);
        assert!(layers.borrow().layers().contains_key(&folder_key));
        assert_eq!(folder_count.text().parse::<usize>().unwrap(), 1);
        assert!(commands.try_recv().is_err());
        folder_button(widgets, "Delete folder Studio").emit_clicked();
        current_dialog().response(ResponseType::Accept);
        assert!(!layers.borrow().layers().contains_key(&folder_key));
        assert_eq!(layers.borrow().active(), active);
        assert_eq!(folder_count.text().parse::<usize>().unwrap(), 0);
        assert!(matches!(
            commands.try_recv().unwrap(),
            LayerCommand::Persist(_, Some(_))
        ));
        assert!(commands.try_recv().is_err());
        find("Delete page").emit_clicked();
        current_dialog().response(ResponseType::Cancel);
        assert!(layers.borrow().layers().contains_key(&active));
        assert!(commands.try_recv().is_err());
        find("Delete page").emit_clicked();
        current_dialog().response(ResponseType::Accept);
        assert!(!layers.borrow().layers().contains_key(&active));
        assert_eq!(layers.borrow().active(), original);
        assert!(matches!(
            commands.try_recv().unwrap(),
            LayerCommand::Persist(_, Some(_))
        ));
        assert!(commands.try_recv().is_err());
        eprintln!("GTK named-page prompts, rename, local folder links, parent navigation and confirmed deletions executed");

        widgets.feature_runtime.execute(
            "timer-test".into(),
            Action::Feature(FeatureAction::Timer { seconds: 60 }),
        );
        widgets
            .feature_runtime
            .report("A separate action failed.", true);
        assert!(widgets
            .feature_runtime
            .status
            .has_css_class("feedback-error"));
        widgets.feature_runtime.report("Action completed.", false);
        assert!(widgets
            .feature_runtime
            .status
            .has_css_class("feedback-success"));
        assert!(!widgets
            .feature_runtime
            .status
            .has_css_class("feedback-error"));
        widgets.feature_runtime.status.set_text("Action queued.");
        assert!(!widgets
            .feature_runtime
            .status
            .has_css_class("feedback-success"));
        assert!(widgets.feature_runtime.timer_status.text().contains("60"));
        widgets.feature_runtime.execute(
            "timer-test".into(),
            Action::Feature(FeatureAction::CancelTimer),
        );
        assert!(!widgets.feature_runtime.timer_active());
        let complete = Rc::new(Cell::new(false));
        let loop_ = glib::MainLoop::new(None, false);
        let loop_on_completion = loop_.clone();
        let completed_signal = complete.clone();
        let signal = widgets
            .feature_runtime
            .timer_status
            .connect_label_notify(move |label| {
                if label.text() == "Timer complete." {
                    completed_signal.set(true);
                    loop_on_completion.quit();
                }
            });
        let timed_out = Rc::new(Cell::new(false));
        let timeout_flag = timed_out.clone();
        let loop_on_timeout = loop_.clone();
        let timeout = glib::timeout_add_local_once(Duration::from_secs(4), move || {
            timeout_flag.set(true);
            loop_on_timeout.quit();
        });
        widgets.feature_runtime.execute(
            "timer-test".into(),
            Action::Feature(FeatureAction::Timer { seconds: 1 }),
        );
        loop_.run();
        widgets.feature_runtime.timer_status.disconnect(signal);
        if !timed_out.get() {
            timeout.remove();
        }
        assert!(complete.get());
        assert!(widgets
            .feature_runtime
            .timer_status
            .has_css_class("feedback-success"));
        assert!(!widgets.feature_runtime.timer_active());
        widgets.feature_runtime.cancel_timer();
        let mut obs_entries = Vec::new();
        collect_entries(
            widgets.feature_runtime.obs_panel.upcast_ref(),
            &mut obs_entries,
        );
        assert_eq!(obs_entries.len(), 2);
        assert!(
            !obs_entries[1].property::<bool>("visibility"),
            "OBS password must be masked"
        );
        obs_entries[0].set_text("ws://127.0.0.1:4456");
        obs_entries[1].set_text("fixture-password");
        while commands.try_recv().is_ok() {}
        find("Test connection").emit_clicked();
        let LayerCommand::ObsProbe(config) = commands.try_recv().unwrap() else {
            panic!("connection test only probes");
        };
        assert_eq!(config.endpoint, "ws://127.0.0.1:4456");
        assert_eq!(config.password, "fixture-password");
        assert!(commands.try_recv().is_err());
        find("Save settings").emit_clicked();
        assert!(matches!(
            commands.try_recv().unwrap(),
            LayerCommand::ObsSave(_)
        ));
        assert!(commands.try_recv().is_err());
        capture_feature_states(window, widgets, layers);
        widgets.feature_runtime.execute(
            "timer-test".into(),
            Action::Feature(FeatureAction::Timer { seconds: 60 }),
        );
        window.close();
        assert!(
            widgets.feature_runtime.timer_active(),
            "closing configuration retains timer source"
        );
        widgets.feature_runtime.cancel_timer();
        assert!(!widgets.feature_runtime.timer_active());
        eprintln!("GTK feature integration: all typed roundtrips, save/reload, explicit Test dispatch, organization controls, timer completion/cancel/hide");
    }

    /// A "Page folders" row action, looked up fresh because rows rebuild
    /// whenever the selected page's folders change.
    fn folder_button(widgets: &MonitorWidgets, caption: &str) -> Button {
        fn collect(widget: &gtk::Widget, buttons: &mut Vec<Button>) {
            if let Some(button) = widget.downcast_ref::<Button>() {
                buttons.push(button.clone());
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                child = current.next_sibling();
                collect(&current, buttons);
            }
        }
        let panel = widgets.feature_runtime.folder_menu().popover().unwrap();
        let mut buttons = Vec::new();
        collect(panel.upcast_ref(), &mut buttons);
        buttons
            .into_iter()
            .find(|button| button.tooltip_text().as_deref() == Some(caption))
            .unwrap_or_else(|| panic!("folder action {caption:?} is listed"))
    }

    fn collect_entries(widget: &gtk::Widget, entries: &mut Vec<Entry>) {
        if let Some(entry) = widget.downcast_ref::<Entry>() {
            entries.push(entry.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            collect_entries(&current, entries);
        }
    }

    fn capture_feature_states(
        window: &ApplicationWindow,
        widgets: &MonitorWidgets,
        layers: &Rc<RefCell<LayerStore>>,
    ) {
        let Some(directory) = std::env::var_os("STREAMDECK_QA_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        // Keep the real widgets mapped, but let their test viewport dimensions
        // be independent of a tiling window manager's native allocation.
        let stage = gtk::Fixed::new();
        window.set_child(None::<&gtk::Widget>);
        stage.put(&widgets.root, 0.0, 0.0);
        window.set_child(Some(&stage));
        for (width, height) in [(1000, 700), (1280, 800)] {
            widgets.root.set_size_request(width, height);
            let capture = |name: &str| {
                widgets.root.allocate(width, height, -1, None);
                let texture = rendered_texture(window, &widgets.root);
                assert_eq!((texture.width(), texture.height()), (width, height));
                let path = directory.join(format!("{name}-{width}x{height}.png"));
                texture.save_to_png(&path).unwrap();
            };
            widgets.binding_ui.action_picker.set_selected(0);
            widgets.binding_ui.update_action_visibility();
            capture("main-navigation");
            widgets.obs_menu.popup();
            let popover = widgets.obs_menu.popover().unwrap();
            let texture = rendered_texture(window, &popover);
            texture
                .save_to_png(directory.join(format!("obs-settings-{width}x{height}.png")))
                .unwrap();
            widgets.obs_menu.popdown();
            widgets.binding_ui.action_picker.set_selected(5);
            widgets.binding_ui.update_action_visibility();
            for category in 0..5 {
                widgets.binding_ui.feature_editor.select_category(category);
                capture(&format!("category-{category}-default"));
            }
            for (index, action) in feature_editor::examples().iter().enumerate() {
                widgets.binding_ui.feature_editor.populate(action);
                assert_eq!(widgets.binding_ui.feature_editor.parse().unwrap(), *action);
                capture(&format!("feature-{index:02}-populated"));
            }
            let ui = &widgets.binding_ui;
            widgets.encoder_labels[2]
                .ancestor(Button::static_type())
                .unwrap()
                .downcast::<Button>()
                .unwrap()
                .emit_clicked();
            ui.event_picker.set_selected(2);
            ui.populate_action(&Action::Feature(
                sdeck_plus::features::FeatureAction::Desktop(
                    sdeck_plus::desktop_actions::DesktopAction::Media(
                        sdeck_plus::desktop_actions::MediaCommand::VolumeDown,
                    ),
                ),
            ));
            assert_eq!(
                ui.binding_from_widgets(layers).unwrap().1,
                Trigger::EncoderCounterClockwise(3)
            );
            capture("encoder-counterclockwise-volume");
            ui.event_picker.set_selected(1);
            ui.populate_action(&Action::Feature(
                sdeck_plus::features::FeatureAction::Device(
                    sdeck_plus::device_control::DeviceAction::AdjustBrightness(5),
                ),
            ));
            assert_eq!(
                ui.binding_from_widgets(layers).unwrap().1,
                Trigger::EncoderClockwise(3)
            );
            capture("encoder-clockwise-brightness");
            widgets
                .touch_status
                .parent()
                .unwrap()
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Overlay>()
                .unwrap()
                .child()
                .unwrap()
                .downcast::<Button>()
                .unwrap()
                .emit_clicked();
            ui.event_picker.set_selected(3);
            ui.populate_action(&Action::Feature(
                sdeck_plus::features::FeatureAction::Navigation(
                    sdeck_plus::organization::NavigationAction::NextPage,
                ),
            ));
            assert_eq!(
                ui.binding_from_widgets(layers).unwrap().1,
                Trigger::TouchSwipe(sdeck_plus::layers::SwipeDirection::Left)
            );
            capture("touch-swipe-navigation");
        }
        window.set_child(None::<&gtk::Widget>);
        stage.remove(&widgets.root);
        widgets.root.set_size_request(-1, -1);
        window.set_child(Some(&widgets.root));
        eprintln!(
            "GTK actual Gsk rendered feature captures saved to {}",
            directory.display()
        );
    }

    fn assert_shared_artwork_editor(
        widgets: &MonitorWidgets,
        content: &gtk::Widget,
        artwork: &ArtworkUi,
        source: &std::path::Path,
    ) {
        let picker = &widgets.target_picker;
        assert_eq!(picker.model().unwrap().n_items(), 5);
        let mut buttons = Vec::new();
        collect_layout_buttons(content, &mut buttons);
        let clear = buttons
            .iter()
            .find(|b| button_caption(b).as_deref() == Some("Clear"))
            .unwrap();
        assert!(clear.is_sensitive());
        // Key 1 is the initial target and the picker starts on entry 0: move it so
        // selecting entry 0 below is a real change.
        picker.set_selected(1);
        for (index, target) in ArtworkTarget::TOUCH.into_iter().enumerate() {
            picker.set_selected(index as u32);
            assert_eq!(artwork.selected_target.get(), target);
            let owner = artwork.capture_owner();
            picker.set_selected(((index + 1) % 5) as u32);
            // Encoder selection changes only the binding, never the image target.
            let encoder = widgets.encoder_labels[index % 4]
                .parent()
                .unwrap()
                .downcast::<Button>()
                .unwrap();
            let explicit_target = artwork.selected_target.get();
            encoder.emit_clicked();
            assert_eq!(artwork.selected_target.get(), explicit_target);
            assert_eq!(widgets.binding_ui.control_picker.selected(), 1);
            artwork.import(&owner, source).unwrap();
            assert!(artwork
                .store
                .borrow()
                .as_ref()
                .unwrap()
                .assigned_path_in_layer("Base", target)
                .is_some());
            picker.set_selected(index as u32);
            clear.emit_clicked();
            assert!(artwork
                .store
                .borrow()
                .as_ref()
                .unwrap()
                .assigned_path_in_layer("Base", target)
                .is_none());
        }
        // The same shared Clear must use the active layer, unlike a captured chooser.
        artwork.layers.borrow_mut().create("Studio").unwrap();
        picker.set_selected(0);
        let owner = artwork.capture_owner();
        artwork.layers.borrow_mut().select("Studio");
        key_button(&widgets.key_labels[7]).emit_clicked();
        assert_eq!(artwork.selected_target.get(), ArtworkTarget::Keypad(8));
        artwork.import(&owner, source).unwrap();
        // Keys do not move the picker: change entry, then return to the strip.
        picker.set_selected(1);
        picker.set_selected(0);
        assert_eq!(
            artwork.selected_target.get(),
            ArtworkTarget::TouchStripBackground
        );
        clear.emit_clicked();
        assert!(artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Base", ArtworkTarget::TouchStripBackground)
            .is_some());
        assert!(artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Studio", ArtworkTarget::TouchStripBackground)
            .is_none());
        // Restore the layer models so the existing long-value matrix stays meaningful.
        artwork
            .remove_layer(layer_index(&artwork.layers, "Studio") as usize)
            .unwrap();
        artwork.layers.borrow_mut().select("Base");
        clear.emit_clicked();
        for (target, picture) in artwork.pictures.borrow().iter() {
            match target {
                ArtworkTarget::Keypad(key) => {
                    let button = key_button(&widgets.key_labels[*key as usize - 1]);
                    button.emit_clicked();
                    assert_eq!(artwork.selected_target.get(), *target);
                    assert_eq!(
                        widgets.binding_ui.control_number_picker.selected(),
                        *key as u32 - 1
                    );
                }
                ArtworkTarget::EncoderZone(zone) => {
                    picture
                        .parent()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .downcast::<Button>()
                        .unwrap()
                        .emit_clicked();
                    assert_eq!(artwork.selected_target.get(), *target);
                    assert_eq!(widgets.binding_ui.control_picker.selected(), 2);
                    assert_eq!(widgets.binding_ui.control_number(), Some(*zone));
                    assert_eq!(widgets.binding_ui.event_model.n_items(), 7);
                    let heading = sidebar_settings(widgets)
                        .first_child()
                        .unwrap()
                        .downcast::<Label>()
                        .unwrap();
                    assert_eq!(
                        heading.text().as_str(),
                        control_heading(ControlKind::Touch, Some(*zone))
                    );
                    key_button(&widgets.key_labels[0]).emit_clicked();
                    assert_eq!(
                        heading.text().as_str(),
                        control_heading(ControlKind::Button, Some(1))
                    );
                }
                ArtworkTarget::TouchStripBackground => {
                    // Keys leave the picker alone: move it so entry 0 is a real change.
                    picker.set_selected(1);
                    picker.set_selected(
                        ArtworkTarget::TOUCH
                            .iter()
                            .position(|t| t == target)
                            .unwrap() as u32,
                    );
                    assert_eq!(artwork.selected_target.get(), *target);
                }
            }
        }
        eprintln!("GTK shared editor executed: all 13 assign/clear paths (keys via their buttons), key/zone/background selection, encoder independence, chooser target/layer capture and active-layer Clear");
    }

    fn assert_scrollable_workspace(
        widgets: &MonitorWidgets,
        content: &gtk::Widget,
        artwork: &ArtworkUi,
        phase: &str,
    ) {
        widgets.artwork_section.set_expanded(false);
        for action in 0..5 {
            widgets.binding_ui.action_picker.set_selected(action);
            widgets.binding_ui.update_action_visibility();
            widgets
                .root
                .allocate(DEFAULT_WIDTH, DEFAULT_HEIGHT, -1, None);
            let minimum = content.measure(Orientation::Vertical, DEFAULT_WIDTH - 48).0;
            let horizontal = widgets.root.hadjustment();
            let vertical = widgets.root.vadjustment();
            // Editing below the fold may have scrolled to the focused field.
            // Reset the viewport for the geometry matrix, not application state.
            vertical.set_value(0.0);
            eprintln!("GTK scrollable workspace {phase} action {action}: height-for-width(924)={minimum}, allocation={}x{}, upper/page={}/{}, {}/{}", content.width(), content.height(), horizontal.upper(), horizontal.page_size(), vertical.upper(), vertical.page_size());
            assert!(
                horizontal.upper() <= horizontal.page_size(),
                "normal width scrolls"
            );
            // The requested inspector-below-preview layout intentionally scrolls.
            assert!(vertical.upper() > vertical.page_size());
            assert_eq!(horizontal.value(), 0.0);
            assert_eq!(vertical.value(), 0.0);
            assert_visible_bounds(content, &widgets.root);
            let editor = settings_region(widgets).compute_bounds(content).unwrap();
            let preview = device_canvas(widgets).compute_bounds(content).unwrap();
            assert!(editor.y() >= preview.y() + preview.height() + 11.0);
            assert_eq!(widgets.key_labels.len(), 8);
            for (index, label) in widgets.key_labels.iter().enumerate() {
                let button = key_button(label);
                assert!(!label.is_visible(), "key name overlay visible: {phase}");
                assert!(button
                    .tooltip_text()
                    .unwrap()
                    .contains(&(index + 1).to_string()));
                for pressed in [true, false] {
                    label.set_text(&format!(
                        "KEY {} · {}",
                        index + 1,
                        if pressed { "PRESSED" } else { "IDLE" }
                    ));
                    set_active(label, pressed);
                    assert!(
                        !label.is_visible(),
                        "live update revealed key name: {phase}"
                    );
                    assert_eq!(
                        label.parent().unwrap().has_css_class("control-active"),
                        pressed
                    );
                }
            }
            eprintln!("GTK hidden key overlays and live updates executed: {phase}");
            assert_eq!(widgets.encoder_labels.len(), 4);
            let selected = widgets
                .key_labels
                .iter()
                .map(key_button)
                .find(|button| button.has_css_class("control-selected"));
            for (index, label) in widgets.encoder_labels.iter().enumerate() {
                let knob = label.parent().unwrap().downcast::<Button>().unwrap();
                assert!(!label.is_visible(), "dial legend visible: {phase}");
                assert!(knob
                    .tooltip_text()
                    .unwrap()
                    .contains(&format!("Encoder {}", index + 1)));
                // The checks above poke the editor directly; treat it as saved.
                widgets.binding_ui.mark_clean();
                knob.emit_clicked();
                assert!(knob.has_css_class("control-selected"));
                label.set_text(&format!("DIAL {}\n3 ticks · +3", index + 1));
                assert!(
                    !label.is_visible(),
                    "rotation revealed dial legend: {phase}"
                );
                for pressed in [true, false] {
                    label.set_text(&format!(
                        "DIAL {}\n{} · 3 ticks",
                        index + 1,
                        if pressed { "PRESSED" } else { "IDLE" }
                    ));
                    set_active(label, pressed);
                    assert!(!label.is_visible(), "press revealed dial legend: {phase}");
                    assert_eq!(knob.has_css_class("control-active"), pressed);
                    assert!(knob.has_css_class("control-selected"));
                }
            }
            if let Some(button) = selected {
                button.emit_clicked();
            }
            eprintln!("GTK hidden dial legends, tooltip identities and selected/live surfaces executed: {phase}");
            assert_eq!(widgets.binding_ui.summaries.len(), 13);
            assert_artwork_geometry(artwork, content, phase == "loaded long values");
            let settings = sidebar_settings(widgets);
            let device = device_canvas(widgets);
            let settings_bounds = settings_region(widgets).compute_bounds(content).unwrap();
            let device_bounds = device.compute_bounds(content).unwrap();
            eprintln!(
                "GTK aligned panels {phase} action {action}: settings={}, preview={}, top={}/{}",
                settings_bounds.height(),
                device_bounds.height(),
                settings_bounds.y(),
                device_bounds.y()
            );
            assert_eq!(settings.clone().downcast::<GtkBox>().unwrap().spacing(), 6);
            assert!(
                (settings_bounds.y() - device_bounds.y() - device_bounds.height() - 12.0).abs()
                    <= 1.0
            );
            let scale = preview_scale(widgets);
            assert!(
                (device_bounds.height() - 428.0 * scale).abs() <= 1.0,
                "preview card scales uniformly, never stretches: {device_bounds:?} at {scale}"
            );
            let bounds = content.compute_bounds(&widgets.root).unwrap();
            assert!(
                bounds.y() <= 16.0,
                "scrolling may move the shell above its top margin"
            );
            assert!(
                bounds.height() > 668.0,
                "inspector is reachable through vertical scrolling"
            );
            assert_eq!(bounds.width(), 968.0, "shell uses the full window width");
            assert!((bounds.x() + bounds.width() / 2.0 - 500.0).abs() <= 1.0);
            let mut buttons = Vec::new();
            collect_layout_buttons(content, &mut buttons);
            for caption in ["Image…", "Clear"] {
                assert_eq!(
                    buttons
                        .iter()
                        .filter(|button| button_caption(button).as_deref() == Some(caption))
                        .count(),
                    0,
                    "artwork controls stay inside their collapsed disclosure"
                );
            }
            assert!(widgets.feature_runtime.profile_tabs().is_visible());
            assert!(widgets
                .feature_runtime
                .profile_tabs()
                .current_page()
                .is_some());
            // A selected key has one event and takes its action from the catalog.
            assert!(!widgets.binding_ui.action_picker.is_visible());
            assert!(!widgets.binding_ui.event_picker.is_visible());
        }
        eprintln!("GTK 1000x700 {phase} five-action scrollable workspace assertions executed");
        widgets.artwork_section.set_expanded(true);
    }

    fn assert_visible_bounds(widget: &gtk::Widget, root: &gtk::ScrolledWindow) {
        // Overlay scrollbars belong to their scroller, not to the content measure.
        if !widget.is_visible() || widget.is::<gtk::Scrollbar>() {
            return;
        }
        if let Some(label) = widget.downcast_ref::<Label>() {
            assert_ne!(label.text(), "LAST EVENT");
        }
        assert!(!widget.has_css_class("events-card"));
        let bounds = widget.compute_bounds(root).unwrap();
        assert!(
            bounds.x() >= -1.0 && bounds.x() + bounds.width() <= root.width() as f32 + 1.0,
            "widget {} outside client width: {bounds:?}",
            widget.type_().name()
        );
        // Internal entry/textview contents may intentionally scroll long editable text.
        if widget.is::<Entry>() || widget.is::<TextView>() || widget.is::<gtk::DropDown>() {
            return;
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            assert_visible_bounds(&current, root);
        }
    }

    fn rendered_pixels(window: &ApplicationWindow, widget: &impl IsA<gtk::Widget>) -> Vec<u8> {
        let widget = widget.as_ref();
        let texture = rendered_texture(window, widget);
        if let Some(directory) = std::env::var_os("STREAMDECK_QA_DIR") {
            static FRAME: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            let index = FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            texture
                .save_to_png(directory.join(format!("render-{index}.png")))
                .unwrap();
        }
        let mut pixels = vec![0; widget.width() as usize * widget.height() as usize * 4];
        texture.download(&mut pixels, widget.width() as usize * 4);
        pixels
    }

    fn wait_for_frame(widget: &impl IsA<gtk::Widget>) {
        let clock = widget.as_ref().frame_clock().expect("mapped frame clock");
        let main_loop = glib::MainLoop::new(None, false);
        let painted = Rc::new(Cell::new(false));
        let painted_signal = painted.clone();
        let loop_on_paint = main_loop.clone();
        let baseline = clock.frame_counter();
        let signal = clock.connect_after_paint(move |clock| {
            if clock.frame_counter() > baseline {
                painted_signal.set(true);
                loop_on_paint.quit();
            }
        });
        let timed_out = Rc::new(Cell::new(false));
        let timeout_flag = timed_out.clone();
        let loop_on_timeout = main_loop.clone();
        let timeout = glib::timeout_add_local_once(Duration::from_secs(5), move || {
            timeout_flag.set(true);
            loop_on_timeout.quit();
        });
        widget.as_ref().queue_draw();
        clock.request_phase(
            gtk::gdk::FrameClockPhase::UPDATE
                | gtk::gdk::FrameClockPhase::LAYOUT
                | gtk::gdk::FrameClockPhase::PAINT
                | gtk::gdk::FrameClockPhase::AFTER_PAINT,
        );
        main_loop.run();
        clock.disconnect(signal);
        if !timed_out.get() {
            timeout.remove();
        }
        assert!(painted.get(), "GTK did not render before timeout");
    }

    fn rendered_texture(
        window: &ApplicationWindow,
        widget: &impl IsA<gtk::Widget>,
    ) -> gtk::gdk::Texture {
        let widget = widget.as_ref();
        let paintable = gtk::WidgetPaintable::new(Some(widget));
        let renderer = window.renderer().unwrap();
        let main_loop = glib::MainLoop::new(None, false);
        let captured = Rc::new(RefCell::new(None));
        let result = captured.clone();
        let done = main_loop.clone();
        let target = widget.clone();
        let capture = Rc::new(move || {
            let snapshot = gtk::Snapshot::new();
            gtk::WidgetPaintable::new(Some(&target)).snapshot(
                &snapshot,
                f64::from(target.width()),
                f64::from(target.height()),
            );
            if let Some(node) = snapshot.to_node() {
                *result.borrow_mut() = Some(renderer.render_texture(
                    &node,
                    Some(&gtk::graphene::Rect::new(
                        0.0,
                        0.0,
                        target.width() as f32,
                        target.height() as f32,
                    )),
                ));
                done.quit();
            }
        });
        let clock = widget.frame_clock().unwrap();
        let phases = gtk::gdk::FrameClockPhase::UPDATE
            | gtk::gdk::FrameClockPhase::LAYOUT
            | gtk::gdk::FrameClockPhase::PAINT
            | gtk::gdk::FrameClockPhase::AFTER_PAINT;
        let redraw = clock.clone();
        let invalidated =
            paintable.connect_invalidate_contents(move |_| redraw.request_phase(phases));
        let baseline = clock.frame_counter();
        let callback = capture.clone();
        let painted = clock.connect_after_paint(move |clock| {
            if clock.frame_counter() > baseline {
                callback();
            }
        });
        let expired = Rc::new(Cell::new(false));
        let flag = expired.clone();
        let done = main_loop.clone();
        let deadline = glib::timeout_add_local_once(Duration::from_secs(10), move || {
            flag.set(true);
            done.quit();
        });
        widget.queue_draw();
        clock.request_phase(phases);
        if captured.borrow().is_none() {
            main_loop.run();
        }
        paintable.disconnect(invalidated);
        clock.disconnect(painted);
        if !expired.get() {
            deadline.remove();
        }
        assert!(
            !expired.get(),
            "GTK content was not published: {} visible={} mapped={} size={}x{}",
            widget.type_().name(),
            widget.is_visible(),
            widget.is_mapped(),
            widget.width(),
            widget.height()
        );
        let texture = captured.borrow_mut().take().expect("published GTK content");
        texture
    }

    fn assert_inactive_backgrounds(
        window: &ApplicationWindow,
        widgets: &MonitorWidgets,
        content: &gtk::Widget,
        artwork: &ArtworkUi,
    ) {
        if let Some(directory) = std::env::var_os("STREAMDECK_QA_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            rendered_texture(window, &widgets.root)
                .save_to_png(std::path::PathBuf::from(directory).join("profile-tabs-shell.png"))
                .unwrap();
        }
        let background = |widget: &gtk::Widget, x: usize, y: usize| {
            let pixels = rendered_pixels(window, widget);
            let offset = (y * widget.width() as usize + x) * 4;
            pixels[offset..offset + 4].to_vec()
        };
        let expected = background(widgets.root.upcast_ref(), 5, 5);
        assert_eq!(expected[3], 255);
        assert_eq!(background(window.upcast_ref(), 5, 5), expected);
        let viewport = widgets.root.child().unwrap();
        assert_eq!(background(&viewport, 5, 5), expected);
        assert_eq!(
            background(content, 2, content.height() as usize - 2),
            expected
        );
        let settings = sidebar_settings(widgets);
        let device = device_canvas(widgets);
        let elevated = background(&settings, 3, settings.height() as usize / 2);
        assert_eq!(elevated[3], 255);
        assert_ne!(elevated, expected, "cards separate from the shell tonally");
        for panel in [&settings, &device] {
            assert_eq!(background(panel, 3, panel.height() as usize / 2), elevated);
        }
        // Hardware surfaces intentionally contrast with the dark application shell.
        let chassis = device.first_child().unwrap();
        assert!(chassis.has_css_class("hardware-chassis"));
        assert_ne!(background(&chassis, 10, 100), expected);
        for (_, picture) in artwork.pictures.borrow().iter() {
            assert_eq!(picture.content_fit(), gtk::ContentFit::Cover);
        }
        let focused = key_button(&widgets.key_labels[0]);
        assert!(focused.has_css_class("control-selected"));
        // Signal-only activation does not focus like a physical button click.
        // Bring the sampled control into the viewport before comparing pixels.
        assert!(focused.grab_focus());
        window.set_focus_visible(false);
        let selected = rendered_pixels(window, &widgets.root);
        focused.remove_css_class("control-selected");
        let unselected = rendered_pixels(window, &widgets.root);
        assert!(selected != unselected, "selection remains visible");
        focused.add_css_class("control-selected");
        let idle = rendered_pixels(window, &widgets.root);
        set_active(&widgets.key_labels[0], true);
        assert!(
            idle != rendered_pixels(window, &widgets.root),
            "live state remains visible"
        );
        set_active(&widgets.key_labels[0], false);
        assert!(focused.grab_focus());
        window.set_focus_visible(false);
        let before = rendered_pixels(window, &widgets.root);
        window.set_focus_visible(true);
        let after = rendered_pixels(window, &widgets.root);
        assert!(focused
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_VISIBLE));
        assert!(
            before != after,
            "keyboard focus must remain visually distinct"
        );
        window.set_focus_visible(false);
        eprintln!("GTK rendered inactive background equality and keyboard-focus distinction assertions executed");
    }

    fn button_caption(button: &Button) -> Option<glib::GString> {
        button
            .label()
            .or_else(|| {
                button
                    .child()
                    .and_then(|child| child.downcast::<GtkBox>().ok())
                    .and_then(|content| content.last_child())
                    .and_then(|child| child.downcast::<Label>().ok())
                    .map(|label| label.text())
            })
            // Icon-only buttons are named by the first line of their tooltip.
            .or_else(|| {
                button
                    .tooltip_text()
                    .map(|tip| tip.lines().next().unwrap_or_default().into())
            })
    }

    /// The discard prompt specifically, so a stray dialog from another test cannot answer it.
    fn discard_dialog() -> gtk::MessageDialog {
        gtk::Window::toplevels()
            .snapshot()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::MessageDialog>().ok())
            .find(|dialog| {
                dialog.is_visible()
                    && dialog.property::<Option<String>>("text").as_deref()
                        == Some("Discard unsaved changes?")
            })
            .expect("unsaved edits must prompt before switching")
    }

    fn current_dialog() -> gtk::Dialog {
        gtk::Window::toplevels()
            .snapshot()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::Dialog>().ok())
            .find(|dialog| dialog.is_visible())
            .expect("the action opens a native dialog")
    }

    fn page_dialog_entry(dialog: &gtk::Dialog) -> Entry {
        let mut entries = Vec::new();
        collect_entries(dialog.content_area().upcast_ref(), &mut entries);
        entries
            .into_iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("Page name"))
            .expect("the dialog contains its named-page input")
    }

    fn device_canvas(widgets: &MonitorWidgets) -> gtk::Widget {
        key_button(&widgets.key_labels[0])
            .parent()
            .unwrap()
            .parent()
            .unwrap()
    }

    /// Uniform presentation scale of the hardware preview, from the chassis bounds.
    fn preview_scale(widgets: &MonitorWidgets) -> f32 {
        let chassis = key_button(&widgets.key_labels[0]).parent().unwrap();
        let preview = device_canvas(widgets)
            .parent()
            .unwrap()
            .downcast::<scaled_bin::ScaledBin>()
            .unwrap();
        let bounds = chassis.compute_bounds(&preview).unwrap();
        assert!((bounds.width() / 426.0 - preview.scale() as f32).abs() < 0.01);
        bounds.width() / 426.0
    }

    fn key_button(label: &Label) -> Button {
        label
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .downcast::<Button>()
            .unwrap()
    }

    /// The configuration region: the form's own scroller, which stays put while
    /// the form scrolls inside it.
    fn settings_region(widgets: &MonitorWidgets) -> gtk::Widget {
        sidebar_settings(widgets)
            .ancestor(gtk::ScrolledWindow::static_type())
            .expect("configuration region scroller")
    }

    fn sidebar_settings(widgets: &MonitorWidgets) -> gtk::Widget {
        widgets
            .binding_ui
            .action_picker
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
    }

    fn assert_compact_responsive_regions(widgets: &MonitorWidgets, content: &gtk::Widget) {
        for (width, height) in [(1840, 980), (1000, 700), (640, 480)] {
            widgets.root.allocate(width, height, -1, None);
            let device = device_canvas(widgets).compute_bounds(content).unwrap();
            let editor = settings_region(widgets).compute_bounds(content).unwrap();
            let shell = content.compute_bounds(&widgets.root).unwrap();
            let header = content
                .first_child()
                .unwrap()
                .compute_bounds(content)
                .unwrap();
            let tabs = widgets.feature_runtime.profile_tabs();
            let tab_header = tabs.first_child().unwrap().compute_bounds(content).unwrap();
            let body = device_canvas(widgets)
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap();
            let pagebar = body
                .parent()
                .unwrap()
                .first_child()
                .unwrap()
                .compute_bounds(content)
                .unwrap();
            for bar in [header, tab_header, pagebar] {
                assert!((bar.x()).abs() <= 1.0);
                assert!(
                    (bar.width() - shell.width()).abs() <= 1.0,
                    "configuration chrome must share the shell measure"
                );
            }
            if width == 1840 {
                assert_eq!(
                    shell.width(),
                    (SHELL_MAX_WIDTH - 32) as f32,
                    "wide configuration shell stops at its maximum measure: {shell:?}"
                );
                assert!((shell.x() + shell.width() / 2.0 - width as f32 / 2.0).abs() <= 1.0);
                assert!(device.x() >= 0.0 && editor.x() >= 0.0);
            } else if width == 640 {
                assert_eq!(shell.width(), 608.0, "narrow shell uses available width");
            }
            let scale = preview_scale(widgets);
            assert!((1.0..=PREVIEW_MAX_SCALE as f32).contains(&scale));
            assert!(
                (device.width() - 440.0 * scale).abs() <= 1.0
                    && (device.height() - 428.0 * scale).abs() <= 1.0,
                "preview scales uniformly instead of stretching: {device:?} at {scale}"
            );
            let cell = device_canvas(widgets).parent().unwrap();
            assert!(
                cell.width() as f32 - device.width() <= 64.0
                    || cell.height() as f32 - device.height() <= 64.0,
                "preview must fill its region in one dimension: {device:?}, cell {}x{}",
                cell.width(),
                cell.height()
            );
            // Vertical room belongs to the inspector rather than preview expansion.
            assert!(
                editor.width() <= 720.0,
                "below-preview editor needs a bounded reading measure: {editor:?}"
            );
            assert!((editor.y() - device.y() - device.height() - 12.0).abs() <= 1.0);
            assert!(widgets.root.hadjustment().upper() <= widgets.root.hadjustment().page_size());
        }
        let previous = widgets.binding_ui.action_picker.selected();
        widgets.binding_ui.action_picker.set_selected(7);
        widgets.root.allocate(1000, 700, -1, None);
        assert!(
            sidebar_settings(widgets).height() < 428,
            "empty inspector should use its natural height"
        );
        widgets.binding_ui.action_picker.set_selected(previous);
        widgets.root.allocate(1000, 700, -1, None);
        let button = ui::icon_button("go-previous-symbolic", "Previous page");
        let caption = ui::button("document-new-symbolic", "New page");
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.append(&button);
        row.append(&caption);
        let window = gtk::Window::new();
        window.set_child(Some(&row));
        window.present();
        wait_for_frame(&window);
        assert_eq!(
            button.width(),
            button.height(),
            "icon-only hit area must be square"
        );
        assert_eq!(button.height(), caption.height());
        let icon = button.child().unwrap();
        let bounds = icon.compute_bounds(&button).unwrap();
        assert!((bounds.x() + bounds.width() / 2.0 - button.width() as f32 / 2.0).abs() <= 1.0);
        window.destroy();
    }

    fn assert_sidebar_hierarchy(widgets: &MonitorWidgets, content: &gtk::Widget) {
        let settings = sidebar_settings(widgets);
        assert!(settings.next_sibling().is_none(), "no event card");
        // The card sits in its own vertical scroller inside the workspace column.
        let sidebar = settings
            .ancestor(gtk::ScrolledWindow::static_type())
            .unwrap()
            .parent()
            .unwrap()
            .downcast::<GtkBox>()
            .unwrap();
        assert_eq!(sidebar.orientation(), Orientation::Vertical);
        assert_eq!(sidebar.spacing(), 12);
        let body = sidebar
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .downcast::<gtk::FlowBox>()
            .unwrap();
        assert_eq!(body.min_children_per_line(), 1);
        assert_eq!(body.max_children_per_line(), 2);
        assert_eq!(body.column_spacing(), 16);
        assert_eq!(body.row_spacing(), 16);
        assert_eq!(
            body.first_child()
                .unwrap()
                .downcast::<gtk::FlowBoxChild>()
                .unwrap()
                .child(),
            Some(sidebar.upcast())
        );
        let shell = content.clone().downcast::<GtkBox>().unwrap();
        assert_eq!(shell.spacing(), 16);
        assert_eq!(
            (
                shell.margin_top(),
                shell.margin_bottom(),
                shell.margin_start(),
                shell.margin_end()
            ),
            (16, 16, 16, 16)
        );
        for panel in [device_canvas(widgets), settings] {
            let panel = panel.downcast::<GtkBox>().unwrap();
            assert!(panel.has_css_class("card"));
            assert_eq!(
                panel.spacing(),
                if panel.has_css_class("device-canvas") {
                    4
                } else {
                    6
                }
            );
            assert_eq!(
                (
                    panel.margin_top(),
                    panel.margin_bottom(),
                    panel.margin_start(),
                    panel.margin_end()
                ),
                (0, 0, 0, 0)
            );
        }
        assert_eq!(widgets.root.hscrollbar_policy(), gtk::PolicyType::Automatic);
        assert_eq!(widgets.root.vscrollbar_policy(), gtk::PolicyType::Automatic);
        eprintln!("GTK sidebar hierarchy and spacing assertions executed");
    }

    fn assert_sidebar_geometry(widgets: &MonitorWidgets, content: &gtk::Widget) {
        let settings = sidebar_settings(widgets);
        let device = device_canvas(widgets);
        // Viewport and scroller of the configuration region, then the workspace column.
        let profile_content = settings
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let navigation = profile_content.first_child().unwrap();
        let navigation_bounds = navigation.compute_bounds(content).unwrap();
        let header = widgets
            .feature_runtime
            .profile_tabs()
            .first_child()
            .unwrap();
        let header_bounds = header.compute_bounds(content).unwrap();
        assert!(
            navigation_bounds.y() - header_bounds.y() - header_bounds.height() >= 11.0,
            "profile tabs and page navigation need a visible gap"
        );
        let region = device.parent().unwrap().parent().unwrap();
        let region_bounds = region.compute_bounds(content).unwrap();
        assert!(
            region_bounds.y() - navigation_bounds.y() - navigation_bounds.height() >= 15.0,
            "page navigation and the main regions must remain separate"
        );
        for panel in [&device, &settings] {
            let bounds = panel.compute_bounds(content).unwrap();
            let first = panel
                .first_child()
                .unwrap()
                .compute_bounds(content)
                .unwrap();
            if panel.has_css_class("device-canvas") {
                let scale = preview_scale(widgets);
                assert!(
                    (first.width() - 426.0 * scale).abs() <= 1.0
                        && (first.height() - 414.0 * scale).abs() <= 1.0
                );
                assert!(
                    (first.x() + first.width() / 2.0 - bounds.x() - bounds.width() / 2.0).abs()
                        <= 1.0,
                    "device must be horizontally centered: {first:?}, {bounds:?}"
                );
                assert!(
                    (first.y() + first.height() / 2.0 - bounds.y() - bounds.height() / 2.0).abs()
                        <= 1.0,
                    "device must be vertically centered: {first:?}, {bounds:?}"
                );
                continue;
            }
            assert!(
                (first.x() - bounds.x() - 17.0).abs() <= 1.0,
                "shared CSS inner padding: {first:?}, {bounds:?}"
            );
            assert!(
                (first.y() - bounds.y() - 17.0).abs() <= 1.0,
                "shared CSS top padding: {first:?}, {bounds:?}"
            );
        }
        let cell = device.parent().unwrap().compute_bounds(content).unwrap();
        // The region (its scroller) stays put while the form scrolls inside it.
        let settings = settings
            .ancestor(gtk::ScrolledWindow::static_type())
            .unwrap()
            .compute_bounds(content)
            .unwrap();
        let device = device.compute_bounds(content).unwrap();
        if (settings.y() - device.y()).abs() <= 0.5 {
            // A height-limited preview is centered in its column.
            let slack = (cell.width() - device.width()) / 2.0;
            assert!(
                (settings.x() - device.x() - device.width() - slack - 16.0).abs() <= 1.0,
                "column gap: {settings:?}, {device:?}, {cell:?}"
            );
        } else {
            assert!(
                (settings.y() - device.y() - device.height() - 12.0).abs() <= 1.0,
                "preview and inspector keep a 12px gap: {settings:?}, {device:?}"
            );
        }
        eprintln!("GTK sidebar geometry assertions executed");
    }

    fn assert_artwork_geometry(artwork: &ArtworkUi, content: &gtk::Widget, loaded: bool) {
        let pictures = artwork.pictures.borrow();
        let strip = pictures
            .iter()
            .find(|(target, _)| *target == ArtworkTarget::TouchStripBackground)
            .unwrap()
            .1
            .parent()
            .unwrap()
            .compute_bounds(content)
            .unwrap();
        let key_bounds: Vec<_> = (1..=8)
            .map(|key| {
                pictures
                    .iter()
                    .find(|(target, _)| *target == ArtworkTarget::Keypad(key))
                    .unwrap()
                    .1
                    .parent()
                    .unwrap()
                    .compute_bounds(content)
                    .unwrap()
            })
            .collect();
        let chassis = pictures[0]
            .1
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let front = chassis.compute_bounds(content).unwrap();
        // The preview may be scaled uniformly; hardware coordinates are unchanged.
        let scale = front.width() / 426.0;
        assert!((1.0..=PREVIEW_MAX_SCALE as f32).contains(&scale));
        let close = |actual: f32, expected: f32| (actual - expected * scale).abs() <= 0.5;
        assert!(
            close(front.height(), 414.0),
            "uniform preview scale: {front:?}"
        );
        assert!((front.width() / front.height() - 774.0 / 752.0).abs() < 0.02);
        for column in 0..4 {
            assert!(close(key_bounds[column].width(), 60.0));
            assert!((key_bounds[column + 4].x() - key_bounds[column].x()).abs() <= 0.5);
            assert!(close(
                key_bounds[column + 4].y() - key_bounds[column].y(),
                67.0
            ));
            if column < 3 {
                assert!(close(
                    key_bounds[column + 1].x() - key_bounds[column].x(),
                    91.0
                ));
            }
        }
        let bezel = pictures
            .iter()
            .find(|(t, _)| *t == ArtworkTarget::TouchStripBackground)
            .unwrap()
            .1
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        assert!(
            bezel.has_css_class("hardware-bezel"),
            "one continuous bezel surrounds the artwork strip"
        );
        let bezel_bounds = bezel.compute_bounds(content).unwrap();
        assert!(close(bezel_bounds.width(), 374.0) && close(bezel_bounds.height(), 77.0));
        assert!(strip.x() > bezel_bounds.x() && strip.y() > bezel_bounds.y());
        assert!(strip.x() + strip.width() < bezel_bounds.x() + bezel_bounds.width());
        let rail = chassis.last_child().unwrap();
        assert!(rail.has_css_class("hardware-rail"));
        // The rail also holds each dial's legend; check the knobs themselves.
        let mut knobs = Vec::new();
        let mut child = rail.first_child();
        while let Some(current) = child {
            if current.has_css_class("encoder") {
                knobs.push(current.clone());
            }
            child = current.next_sibling();
        }
        assert_eq!(knobs.len(), 4);
        for (key, current) in key_bounds.iter().take(4).zip(&knobs) {
            let bounds = current.compute_bounds(content).unwrap();
            assert!(close(bounds.width(), 46.0) && close(bounds.height(), 46.0));
            assert!((bounds.x() + bounds.width() / 2.0 - key.x() - key.width() / 2.0).abs() <= 0.5);
        }
        for (target, picture) in pictures.iter() {
            let surface = picture.parent().unwrap();
            let bounds = surface.compute_bounds(content).unwrap();
            if *target == ArtworkTarget::Keypad(1) || *target == ArtworkTarget::EncoderZone(1) {
                eprintln!(
                    "GTK artwork surface {target:?}: {}x{} loaded={loaded}",
                    bounds.width(),
                    bounds.height()
                );
            }
            assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
            match target {
                ArtworkTarget::Keypad(_) => assert!(
                    (bounds.width() - bounds.height()).abs() <= 1.0,
                    "key surface must be square: {bounds:?}"
                ),
                ArtworkTarget::EncoderZone(_) => {
                    assert!(
                        (bounds.height() - strip.height()).abs() <= 1.0,
                        "zone must fill strip height: {bounds:?}, strip {strip:?}"
                    );
                    assert!((bounds.width() * 4.0 - strip.width()).abs() <= 4.0);
                }
                ArtworkTarget::TouchStripBackground => {
                    assert!((bounds.width() - bounds.height() * 8.0).abs() <= 8.0);
                    let mut painted_controls = Vec::new();
                    collect_layout_buttons(&surface, &mut painted_controls);
                    assert!(
                        painted_controls.is_empty(),
                        "artwork controls belong outside painted strip"
                    );
                }
            }
            if loaded {
                let preview = picture.compute_bounds(content).unwrap();
                assert!((preview.width() - bounds.width()).abs() <= 1.0);
                assert!((preview.height() - bounds.height()).abs() <= 1.0);
            }
        }
    }

    fn collect_layout_buttons(widget: &gtk::Widget, buttons: &mut Vec<Button>) {
        if let Some(button) = widget.downcast_ref::<Button>() {
            if matches!(
                button_caption(button).as_deref(),
                Some(
                    "Image…"
                        | "Clear"
                        | "Save binding"
                        | "Remove binding"
                        | "Create profile"
                        | "Delete profile"
                        | "Test action"
                        | "Previous"
                        | "Next"
                        | "Parent"
                        | "New page"
                        | "Rename page"
                        | "Delete page"
                        | "Save changes"
                        | "New folder"
                        | "Save settings"
                        | "Test connection"
                )
            ) {
                buttons.push(button.clone());
            }
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            collect_layout_buttons(&widget, buttons);
            child = widget.next_sibling();
        }
    }

    fn assert_reachable(widget: &impl IsA<gtk::Widget>, viewport: &gtk::ScrolledWindow) {
        // A hidden control (for example Appearance while a key is selected) has
        // no position to reach.
        if !widget.as_ref().is_visible() {
            return;
        }
        let vertical = viewport.vadjustment();
        let horizontal = viewport.hadjustment();
        horizontal.set_value(0.0);
        vertical.set_value(0.0);
        // A control inside a region that scrolls on its own (the configuration
        // form) is reached by scrolling that region first, as a person would.
        let mut parent = widget.as_ref().parent();
        while let Some(ancestor) = parent {
            if let Some(inner) = ancestor.downcast_ref::<gtk::ScrolledWindow>() {
                if inner != viewport {
                    inner.vadjustment().set_value(0.0);
                    inner.allocate(inner.width(), inner.height(), -1, None);
                    if let Some(bounds) = widget.compute_bounds(inner) {
                        let adjustment = inner.vadjustment();
                        adjustment.set_value(adjustment.value() + f64::from(bounds.y()));
                    }
                }
            }
            parent = ancestor.parent();
        }
        viewport.queue_allocate();
        viewport.allocate(viewport.width(), viewport.height(), -1, None);
        let bounds = widget
            .compute_bounds(viewport)
            .expect("control belongs to production viewport");
        horizontal.set_value(f64::from(bounds.x()));
        vertical.set_value(f64::from(bounds.y()));
        viewport.queue_allocate();
        viewport.allocate(viewport.width(), viewport.height(), -1, None);
        let bounds = widget.compute_bounds(viewport).unwrap();
        assert!(
            bounds.x() >= 0.0 && bounds.x() + bounds.width() <= viewport.width() as f32,
            "control is horizontally clipped: {bounds:?} ({} in a {}px viewport, scrollable width {})",
            widget.type_().name(),
            viewport.width(),
            horizontal.upper()
        );
        assert!(
            bounds.y() >= -1.0 && bounds.y() + bounds.height() <= viewport.height() as f32 + 1.0,
            "control cannot be scrolled into view: {bounds:?}"
        );
    }

    fn artwork_fixture(
        tag: &str,
    ) -> (
        std::path::PathBuf,
        LayerStore,
        ArtworkStore,
        std::path::PathBuf,
    ) {
        let root = std::env::temp_dir().join(format!(
            "t4-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut layers = LayerStore::open_at(root.join("layers")).unwrap();
        layers.create("Studio").unwrap();
        let artwork = ArtworkStore::open_at_for_layer(root.join("artwork"), Some("Base")).unwrap();
        let source = root.join("source.png");
        let image =
            gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 2, 2).unwrap();
        image.fill(0x4488ffff);
        image.savev(&source, "png", &[]).unwrap();
        (root, layers, artwork, source)
    }

    fn assert_gif_artwork_preview() {
        let (root, _, _, _) = artwork_fixture("gif-preview");
        let path = root.join("source.gif");
        // A GIF with a 2x2 canvas and a smaller first image frame.
        std::fs::write(
            &path,
            [
                0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x02, 0x00, 0x02, 0x00, 0x80, 0x00, 0x00, 0x00,
                0x00, 0x00, 0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c,
                0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00,
                0x3b,
            ],
        )
        .unwrap();
        let picture = artwork_picture();
        set_picture_path(&picture, Some(path));
        assert!(picture.is_visible());
        assert!(
            picture.paintable().is_some(),
            "saved GIF artwork must decode in the app as it does on the device"
        );
        let frame = picture.paintable().unwrap();
        assert_eq!(
            (frame.intrinsic_width(), frame.intrinsic_height()),
            (2, 2),
            "the preview must show the decoded GIF, not a loading placeholder"
        );
        set_picture_path(&picture, None);
        assert!(picture.paintable().is_none());
        assert!(!picture.is_visible());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn assert_artwork_refresh_switch_and_reconnect() {
        let (root, layers, mut store, source) = artwork_fixture("gtk");
        let jpeg_source = root.join("source.jpg");
        gtk::gdk_pixbuf::Pixbuf::from_file(&source)
            .unwrap()
            .savev(&jpeg_source, "jpeg", &[])
            .unwrap();
        let source = jpeg_source;
        for target in ArtworkTarget::ALL {
            store.assign_in_layer("Base", target, &source).unwrap();
        }
        let (output, receiver) = mpsc::channel();
        let layers = Rc::new(RefCell::new(layers));
        let artwork = ArtworkUi {
            store: Rc::new(RefCell::new(Some(store))),
            layers: layers.clone(),
            pictures: Rc::new(RefCell::new(
                ArtworkTarget::ALL
                    .iter()
                    .map(|target| (*target, Picture::new()))
                    .collect(),
            )),
            output,
            last_layer: Rc::new(RefCell::new(None)),
            chooser_owners: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
            selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
            text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
            info: Rc::default(),
        };
        artwork.refresh(false);
        assert_eq!(artwork.pictures.borrow().len(), 13);
        assert!(artwork
            .pictures
            .borrow()
            .iter()
            .all(|(_, picture)| picture.is_visible() && picture.file().is_some()));
        for (_, picture) in artwork.pictures.borrow().iter() {
            let paintable = picture
                .paintable()
                .expect("JPEG artwork must decode in GTK");
            assert_eq!(
                (paintable.intrinsic_width(), paintable.intrinsic_height()),
                (2, 2)
            );
        }
        assert!(
            matches!(receiver.try_recv().unwrap(), ArtworkCommand::Replace(commands) if commands.len() == 13)
        );
        let picker = gtk::DropDown::new(
            Some(gtk::StringList::new(&["Base", "Studio"])),
            gtk::Expression::NONE,
        );
        {
            let artwork = artwork.clone();
            let layers = layers.clone();
            picker.connect_selected_notify(move |picker| {
                select_layer(&layers, picker.selected() as usize);
                artwork.refresh(false);
            });
        }
        let base_owner = artwork.capture_owner();
        // Input-driven switching must release its mutable borrow before notify.
        let switched = layers.borrow_mut().apply_switch("Studio");
        assert!(switched);
        picker.set_selected(1);
        artwork.refresh(false);
        assert!(
            matches!(receiver.try_recv().unwrap(), ArtworkCommand::Replace(commands) if commands.len() == 9 && commands.iter().all(|command| matches!(command, ArtworkCommand::Clear { .. })))
        );
        assert!(
            receiver.try_recv().is_err(),
            "synchronous picker refresh must not duplicate the snapshot"
        );
        assert!(artwork
            .pictures
            .borrow()
            .iter()
            .all(|(_, picture)| !picture.is_visible() && picture.file().is_none()));
        artwork.import(&base_owner, &source).unwrap();
        assert!(
            receiver.try_recv().is_err(),
            "inactive chooser owner must not change active device state"
        );
        artwork.refresh(true); // Reconnect to an empty active layer.
        assert!(
            matches!(receiver.try_recv().unwrap(), ArtworkCommand::Replace(commands) if commands.len() == 9 && commands.iter().all(|command| matches!(command, ArtworkCommand::Clear { .. })))
        );
        assert!(artwork
            .pictures
            .borrow()
            .iter()
            .all(|(_, picture)| !picture.is_visible()));

        let owner = artwork.capture_owner();
        artwork.remove_layer(1).unwrap().unwrap();
        layers.borrow_mut().create("Studio").unwrap();
        layers.borrow_mut().select("Studio");
        let replacement_owner = artwork.capture_owner();
        artwork.selected_target.set(ArtworkTarget::Keypad(2));
        assert!(
            artwork.import(&owner, &source).is_err(),
            "a chooser opened before removal must reject a same-name replacement layer"
        );
        assert!(artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Studio", ArtworkTarget::Keypad(1))
            .is_none());
        artwork.import(&replacement_owner, &source).unwrap();
        assert!(
            artwork
                .store
                .borrow()
                .as_ref()
                .unwrap()
                .assigned_path_in_layer("Studio", ArtworkTarget::Keypad(1))
                .is_some(),
            "chooser must retain its opening target, not a later selection"
        );
        std::fs::create_dir(root.join("artwork/artwork.conf.tmp")).unwrap();
        assert!(artwork.remove_layer(1).is_err());
        let retained_owner = artwork.capture_owner();
        assert!(Rc::ptr_eq(
            &replacement_owner.incarnation,
            &retained_owner.incarnation
        ));
        eprintln!("GTK chooser incarnation assertions executed");
    }

    #[test]
    fn invalid_layer_storage_is_rejected_without_saving_fallback() {
        let (root, _, _, _) = artwork_fixture("invalid-layers");
        let path = root.join("layers/layers.conf");
        std::fs::write(&path, "invalid original configuration").unwrap();
        let recovered = LayerStore::open_at(root.join("layers")).unwrap();
        assert!(checked_layer_store(recovered).is_err());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "invalid original configuration"
        );
    }

    #[test]
    fn successful_layer_removal_cleans_artwork_before_recreation() {
        let (_, layers, store, source) = artwork_fixture("remove-success");
        let layers = RefCell::new(layers);
        let mut artwork = Some(store);
        let key = ArtworkTarget::Keypad(1);
        assign_chooser_artwork(&layers, &mut artwork, "Base", key, &source).unwrap();
        assign_chooser_artwork(&layers, &mut artwork, "Studio", key, &source).unwrap();
        layers.borrow_mut().select("Studio");
        let snapshot = remove_layer_with_artwork(&layers, &mut artwork, 1)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.active(), "Base");
        assert!(assign_chooser_artwork(&layers, &mut artwork, "Studio", key, &source).is_err());
        layers.borrow_mut().create("Studio").unwrap();
        let store = artwork.as_ref().unwrap();
        assert!(store.assigned_path_in_layer("Studio", key).is_none());
        assert!(store.assigned_path_in_layer("Base", key).is_some());
    }

    #[test]
    fn layer_artwork_import_clear_and_removal_guards_are_isolated() {
        let (root, layers, store, source) = artwork_fixture("ownership");
        let layers = RefCell::new(layers);
        let mut artwork = Some(store);
        let key = ArtworkTarget::Keypad(1);
        assign_chooser_artwork(&layers, &mut artwork, "Base", key, &source).unwrap();
        assign_chooser_artwork(&layers, &mut artwork, "Studio", key, &source).unwrap();
        artwork
            .as_mut()
            .unwrap()
            .clear_in_layer("Studio", key)
            .unwrap();
        assert!(artwork
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Base", key)
            .is_some());
        assert!(artwork
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Studio", key)
            .is_none());
        let before = std::fs::read(root.join("artwork/artwork.conf")).unwrap();
        assert!(remove_layer_with_artwork(&layers, &mut artwork, 0)
            .unwrap()
            .is_none());
        assert!(remove_layer_with_artwork(&layers, &mut artwork, 99)
            .unwrap()
            .is_none());
        assert_eq!(
            std::fs::read(root.join("artwork/artwork.conf")).unwrap(),
            before
        );
        assert!(remove_layer_with_artwork(&layers, &mut None, 0)
            .unwrap()
            .is_none());
        assert!(remove_layer_with_artwork(&layers, &mut None, 1).is_err());
        std::fs::create_dir(root.join("artwork/artwork.conf.tmp")).unwrap();
        assert!(remove_layer_with_artwork(&layers, &mut artwork, 1).is_err());
        assert!(layers.borrow().layers().contains_key("Studio"));
        assert_eq!(
            std::fs::read(root.join("artwork/artwork.conf")).unwrap(),
            before
        );
        // A removed chooser owner is rejected before even reading the source.
        layers.borrow_mut().remove("Studio").unwrap();
        let error = assign_chooser_artwork(
            &layers,
            &mut artwork,
            "Studio",
            key,
            &root.join("missing.png"),
        )
        .unwrap_err();
        assert!(error.contains("was removed"));
        assert_eq!(
            std::fs::read(root.join("artwork/artwork.conf")).unwrap(),
            before
        );
    }

    fn assert_summaries_refresh_for_binding_and_layer_changes(ui: &BindingUi) {
        let root =
            std::env::temp_dir().join(format!("streamdeck-summary-refresh-{}", std::process::id()));
        let mut store = LayerStore::open_at(&root).unwrap();
        store.create("Studio").unwrap();
        let layers = Rc::new(RefCell::new(store));
        replace_string_list(&ui.layer_model, &layers.borrow().names());
        ui.layer_picker.set_selected(0);
        {
            let ui = ui.clone();
            let layers = layers.clone();
            ui.layer_picker
                .clone()
                .connect_selected_notify(move |picker| {
                    select_layer(&layers, picker.selected() as usize);
                    ui.refresh_summaries(&layers);
                });
        }
        {
            let layers = layers.clone();
            ui.summaries[0].connect_label_notify(move |_| {
                // GTK property notifications are synchronous: refresh must release
                // the store borrow before updating labels.
                assert!(layers.try_borrow_mut().is_ok());
            });
        }
        ui.refresh_summaries(&layers);
        assert_eq!(ui.summaries.len(), 13);
        for label in &ui.summaries {
            assert!(label.text().contains("Unassigned"));
        }
        layers
            .borrow_mut()
            .bind(
                "Base",
                Trigger::KeyPress(1),
                Action::SwitchLayer("Studio".into()),
            )
            .unwrap();
        ui.refresh_summaries(&layers);
        assert_eq!(ui.summaries[0].text(), "Press: Go to Studio");
        layers
            .borrow_mut()
            .bind(
                "Studio",
                Trigger::EncoderClockwise(4),
                Action::Command("date".into()),
            )
            .unwrap();
        // Exercise the input-driven switch and its synchronous picker callback.
        let action = matching_action(
            &layers,
            &protocol::Event::Key {
                index: 0,
                pressed: true,
            },
        )
        .unwrap();
        let Action::SwitchLayer(target) = action else {
            panic!("expected switch")
        };
        assert!(layers.borrow_mut().apply_switch(&target));
        ui.layer_picker.set_selected(layer_index(&layers, &target));
        assert!(ui.summaries[0].text().contains("Unassigned"));
        assert!(ui.summaries[12].text().contains("CW: Run date"));
        layers
            .borrow_mut()
            .bind("Studio", Trigger::TouchTap, Action::Emit("touch".into()))
            .unwrap();
        ui.refresh_summaries(&layers);
        assert!(ui.summaries[8].text().contains("Tap: Emit touch"));
        let live_label = Label::new(Some("PRESSED"));
        set_active(&live_label, true);
        assert!(ui.summaries[8].text().contains("Tap: Emit touch"));
        assert!(!ui.summaries[8].has_css_class("control-active"));
        layers
            .borrow_mut()
            .unbind("Studio", &Trigger::EncoderClockwise(4))
            .unwrap();
        ui.refresh_summaries(&layers);
        assert!(ui.summaries[12].text().contains("CW: Unassigned"));
        layers.borrow_mut().create("Empty").unwrap();
        let names = layers.borrow().names();
        replace_string_list(&ui.layer_model, &names);
        ui.layer_picker.set_selected(layer_index(&layers, "Empty"));
        assert_eq!(layers.borrow().active(), "Empty");
        assert!(ui.summaries[0].text().contains("Unassigned"));
        let index = ui.layer_picker.selected() as usize;
        assert!(remove_selected_layer(&layers, index).is_some());
        ui.layer_model.remove(index as u32);
        ui.layer_picker.set_selected(0);
        ui.refresh_summaries(&layers);
        assert_eq!(ui.summaries[0].text(), "Press: Go to Studio");
        assert!(remove_selected_layer(&layers, 0).is_none());
        // Removing a destination also removes inbound switches from Base.
        let studio = layer_index(&layers, "Studio");
        assert!(remove_selected_layer(&layers, studio as usize).is_some());
        ui.layer_model.remove(studio);
        ui.refresh_summaries(&layers);
        assert_eq!(ui.summaries[0].text(), "Press: Unassigned");
        std::fs::remove_dir_all(root).unwrap();
    }

    fn assert_select_control_validates_ranges_and_preserves_valid_events() {
        let Some(ui) = test_binding_ui() else {
            eprintln!("skipping GTK select_control test because gtk::init failed");
            return;
        };

        ui.select_control(ControlKind::Encoder, Some(4), true)
            .expect("encoder 4 should be selectable");
        assert_eq!(ui.control_kind(), ControlKind::Encoder);
        assert_eq!(ui.control_number_picker.selected(), 3);
        assert_eq!(ui.event_model.n_items(), 3);
        assert_eq!(
            dropdown_value(&ui.event_model, 1).as_deref(),
            Some("Clockwise")
        );
        ui.event_picker.set_selected(1);
        ui.select_control(ControlKind::Encoder, Some(2), false)
            .expect("switching encoders should preserve a valid encoder event");
        assert_eq!(
            dropdown_value(&ui.event_model, ui.event_picker.selected()).as_deref(),
            Some("Clockwise")
        );
        ui.select_control(ControlKind::Button, Some(8), false)
            .expect("button 8 should be selectable");
        assert_eq!(ui.control_number_picker.selected(), 7);
        assert_eq!(
            dropdown_value(&ui.event_model, ui.event_picker.selected()).as_deref(),
            Some("Press")
        );
        ui.select_control(ControlKind::Touch, Some(1), false)
            .expect("the touch strip should be selectable");
        assert!(ui.control_number_row.is_visible());
        assert_eq!(ui.event_model.n_items(), 7);
        assert!(ui
            .select_control(ControlKind::Button, Some(9), true)
            .is_err());
        assert!(ui
            .select_control(ControlKind::Encoder, Some(5), true)
            .is_err());
        assert!(ui
            .select_control(ControlKind::Touch, Some(5), true)
            .is_err());
    }

    fn assert_direct_button_click_selects_button_8_and_loads_existing_assignment() {
        let Some(ui) = test_binding_ui() else {
            eprintln!("skipping GTK direct-selection test because gtk::init failed");
            return;
        };
        let root =
            std::env::temp_dir().join(format!("streamdeck-direct-select-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = LayerStore::open_at(&root).unwrap();
        store
            .bind(
                "Base",
                Trigger::KeyPress(8),
                Action::Command("echo eight".into()),
            )
            .unwrap();
        let layers = Rc::new(RefCell::new(store));
        let button = Button::new();
        let selected_widgets = Rc::new(vec![button.clone().upcast::<gtk::Widget>()]);
        let heading = Label::new(None);
        connect_control_selection(
            &Label::new(None),
            button.clone(),
            &ui,
            &layers,
            &heading,
            selected_widgets.clone(),
            0,
            ControlKind::Button,
            8,
        );

        button.emit_clicked();

        assert_eq!(
            heading.text().as_str(),
            control_heading(ControlKind::Button, Some(8))
        );
        assert!(selected_widgets[0].has_css_class("control-selected"));
        let (_, trigger) = ui.selected_layer_and_trigger(&layers).unwrap();
        assert_eq!(trigger, Trigger::KeyPress(8));
        assert_eq!(ui.action_kind(), ActionKind::RunCommand);
        assert_eq!(ui.text_entry.text().as_str(), "echo eight");
        std::fs::remove_dir_all(root).unwrap();
    }

    fn assert_direct_selection_css_is_distinct_from_live_active_state() {
        if !init_gtk_for_test() {
            eprintln!("skipping GTK CSS state test because gtk::init failed");
            return;
        }
        let label = Label::new(Some("BUTTON 1"));
        let button = Button::new();
        let widgets = vec![button.clone().upcast::<gtk::Widget>()];

        set_active(&label, true);
        set_selected_control(&widgets, 0);

        assert!(label.has_css_class("control-active"));
        assert!(!label.has_css_class("control-selected"));
        assert!(button.has_css_class("control-selected"));
        assert!(!button.has_css_class("control-active"));
    }

    #[test]
    fn selecting_new_layer_releases_read_borrow_before_mutating() {
        let root = std::env::temp_dir().join(format!("streamdeck-select-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = LayerStore::open_at(&root).unwrap();
        store.create("Studio").unwrap();
        let layers = RefCell::new(store);

        let snapshot = select_layer(&layers, 1).expect("new layer should be selectable");

        assert_eq!(layers.borrow().active(), "Studio");
        assert_eq!(snapshot.active(), "Studio");
        assert!(select_layer(&layers, 2).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn binding_selected_layer_releases_read_borrow_and_switches_from_base() {
        let root = std::env::temp_dir().join(format!("streamdeck-bind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = LayerStore::open_at(&root).unwrap();
        store.create("Studio").unwrap();
        let layers = RefCell::new(store);
        let trigger = parse_trigger("k:1").expect("UI key trigger should parse");
        let action = parse_action("switch:Studio").expect("UI switch action should parse");

        let snapshot = bind_selected_layer(&layers, 0, trigger, action)
            .expect("selected Base layer should accept the binding");

        assert_eq!(snapshot.active(), "Base");
        let action = layers.borrow().matching_action(&protocol::Event::Key {
            index: 0,
            pressed: true,
        });
        assert_eq!(action, Some(Action::SwitchLayer("Studio".into())));
        let Some(Action::SwitchLayer(target)) = action else {
            panic!("pressed Base key should switch to Studio");
        };
        assert!(layers.borrow_mut().apply_switch(&target));
        assert_eq!(layers.borrow().active(), "Studio");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn matching_switch_action_releases_read_borrow_before_mutating() {
        let root =
            std::env::temp_dir().join(format!("streamdeck-event-switch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = LayerStore::open_at(&root).unwrap();
        store.create("Studio").unwrap();
        store
            .bind(
                "Base",
                Trigger::KeyPress(1),
                Action::SwitchLayer("Studio".into()),
            )
            .unwrap();
        let layers = RefCell::new(store);

        let action = matching_action(
            &layers,
            &protocol::Event::Key {
                index: 0,
                pressed: true,
            },
        );
        let Some(Action::SwitchLayer(target)) = action else {
            panic!("pressed Base key should return a switch action");
        };
        assert!(layers.borrow_mut().apply_switch(&target));
        assert_eq!(layers.borrow().active(), "Studio");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn switch_layer_index_releases_borrow_before_synchronous_selection_callback() {
        let root =
            std::env::temp_dir().join(format!("streamdeck-selector-notify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = LayerStore::open_at(&root).unwrap();
        store.create("Studio").unwrap();
        store
            .bind(
                "Base",
                Trigger::KeyPress(1),
                Action::SwitchLayer("Studio".into()),
            )
            .unwrap();
        let layers = RefCell::new(store);

        let Some(Action::SwitchLayer(target)) = matching_action(
            &layers,
            &protocol::Event::Key {
                index: 0,
                pressed: true,
            },
        ) else {
            panic!("pressed Base key should switch to Studio");
        };
        assert!(layers.borrow_mut().apply_switch(&target));

        // GTK's set_selected emits selected-notify synchronously. Model that callback
        // by selecting through the same helper only after its borrow has ended.
        let selected_index = layer_index(&layers, &target);
        assert_eq!(selected_index, 1);
        let snapshot = select_layer(&layers, selected_index as usize)
            .expect("synchronous selected-notify callback should select Studio");
        assert_eq!(layers.borrow().active(), "Studio");
        assert_eq!(snapshot.active(), "Studio");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_selected_studio_releases_read_borrow_and_returns_to_base() {
        let root = std::env::temp_dir().join(format!("streamdeck-remove-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = LayerStore::open_at(&root).unwrap();
        store.create("Studio").unwrap();
        assert!(store.select("Studio"));
        let layers = RefCell::new(store);

        let snapshot =
            remove_selected_layer(&layers, 1).expect("selected Studio layer should be removed");

        assert_eq!(layers.borrow().active(), "Base");
        assert_eq!(snapshot.active(), "Base");
        assert_eq!(layers.borrow().names(), vec!["Base"]);
        assert!(layers.borrow_mut().select("Base"));
        assert!(remove_selected_layer(&layers, 0).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transitional_trigger_parser_accepts_directional_events() {
        assert_eq!(parse_trigger("r:1:+"), Ok(Trigger::EncoderClockwise(1)));
        assert_eq!(
            parse_trigger("r:2:-"),
            Ok(Trigger::EncoderCounterClockwise(2))
        );
        assert_eq!(
            parse_trigger("swipe:left"),
            Ok(Trigger::TouchSwipe(SwipeDirection::Left))
        );
        assert_eq!(
            parse_trigger("swipe:right"),
            Ok(Trigger::TouchSwipe(SwipeDirection::Right))
        );
        assert_eq!(parse_trigger("swipe:diagonal"), Err(()));
    }

    #[test]
    fn empty_layer_plan_clears_every_key_and_the_full_strip() {
        let root = std::env::temp_dir().join(format!("t4-empty-{}", std::process::id()));
        let store = ArtworkStore::open_at_for_layer(root, Some("Base")).unwrap();
        let commands = desired_artwork_commands(&store, "Empty");
        assert_eq!(commands.len(), 9);
        for (index, command) in commands.iter().take(8).enumerate() {
            assert!(
                matches!(command, ArtworkCommand::Clear { target: ArtworkTarget::Keypad(key) } if *key == (index + 1) as u8)
            );
        }
        assert!(matches!(
            commands[8],
            ArtworkCommand::Clear {
                target: ArtworkTarget::TouchStripBackground
            }
        ));
    }

    #[test]
    fn touch_strip_composition_uploads_background_before_saved_zones() {
        let root = std::env::temp_dir().join(format!("touch-artwork-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.png");
        let image =
            gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 2, 2).unwrap();
        image.savev(&source, "png", &[]).unwrap();
        let mut store = ArtworkStore::open_at_for_layer(root.join("store"), Some("Base")).unwrap();
        store
            .assign_in_layer("Base", ArtworkTarget::TouchStripBackground, &source)
            .unwrap();
        store
            .assign_in_layer("Base", ArtworkTarget::EncoderZone(2), &source)
            .unwrap();

        let commands = touch_strip_commands(&store, "Base");
        assert!(matches!(
            commands.first(),
            Some(ArtworkCommand::Assign {
                target: ArtworkTarget::TouchStripBackground,
                ..
            })
        ));
        assert!(matches!(
            commands.get(1),
            Some(ArtworkCommand::Assign {
                target: ArtworkTarget::EncoderZone(2),
                ..
            })
        ));
        assert_eq!(commands.len(), 2);

        store
            .clear_in_layer("Base", ArtworkTarget::EncoderZone(2))
            .unwrap();
        let background_only = touch_strip_commands(&store, "Base");
        assert_eq!(background_only.len(), 1);
        assert!(matches!(
            background_only[0],
            ArtworkCommand::Assign {
                target: ArtworkTarget::TouchStripBackground,
                ..
            }
        ));
        store
            .assign_in_layer("Base", ArtworkTarget::EncoderZone(2), &source)
            .unwrap();
        store
            .clear_in_layer("Base", ArtworkTarget::TouchStripBackground)
            .unwrap();
        let commands = touch_strip_commands(&store, "Base");
        assert!(matches!(
            commands.first(),
            Some(ArtworkCommand::Clear {
                target: ArtworkTarget::TouchStripBackground
            })
        ));
        assert!(matches!(
            commands.get(1),
            Some(ArtworkCommand::Assign {
                target: ArtworkTarget::EncoderZone(2),
                ..
            })
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
