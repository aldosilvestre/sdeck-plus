use sdeck_plus::{
    binding_editor::{
        action_from_fields, control_summary, events_for_control, existing_action,
        format_application_args, trigger_from_selection, ActionKind, ControlKind, EventKind,
    },
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

mod app_icon;
mod autostart;
mod feature_editor;
mod feature_runtime;
mod tray;
mod tray_preference;
mod ui;

const APP_ID: &str = "dev.streamdeck.EventMonitor";
const APP_NAME: &str = "sdeck-plus";
const DEFAULT_WIDTH: i32 = 1000;
const DEFAULT_HEIGHT: i32 = 700;

enum LayerCommand {
    Persist(LayerStore, Option<sdeck_plus::organization::Organization>),
    SaveBinding(LayerStore, Option<sdeck_plus::organization::Organization>),
    SaveProfile(LayerStore, Option<sdeck_plus::organization::Organization>),
    Execute { binding_id: String, action: Action },
    ObsSave(sdeck_plus::obs::ObsConfig),
    ObsProbe(sdeck_plus::obs::ObsConfig),
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
    let event = match result {
        Ok(()) => MonitorEvent::AppEvent {
            name: "Action dispatched.".into(),
        },
        Err(error) => MonitorEvent::LayerError(format!("Could not execute layer action: {error}")),
    };
    if let Err(error) = output.send(event) {
        eprintln!("Action result delivery failed: {error}");
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
    app.connect_activate(move |app| {
        let visible = !background.replace(false) || tray_error.borrow().is_some();
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
    };
    let widgets = build_widgets(
        window.clone(),
        layers.clone(),
        artwork.clone(),
        layer_sender.clone(),
    );
    let organization =
        sdeck_plus::organization::Organization::open_at(layers.borrow().root(), &layers.borrow());
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
    feature_runtime: feature_runtime::FeatureRuntime,
    organization_writes: Vec<Button>,
}

fn build_widgets(
    window: ApplicationWindow,
    layers: Rc<RefCell<LayerStore>>,
    artwork: ArtworkUi,
    layer_sender: mpsc::Sender<LayerCommand>,
) -> MonitorWidgets {
    let updating_layers = Rc::new(Cell::new(false));
    let toast = ui::Toast::new();
    let root = GtkBox::new(Orientation::Vertical, 16);
    root.set_margin_top(24);
    root.set_margin_bottom(24);
    root.set_margin_start(24);
    root.set_margin_end(24);
    root.add_css_class("app-shell");

    let header = GtkBox::new(Orientation::Horizontal, 12);
    let heading = GtkBox::new(Orientation::Vertical, 4);
    let title = Label::new(Some("STREAM DECK +"));
    title.add_css_class("title");
    title.set_xalign(0.0);
    heading.append(&title);
    header.append(&heading);
    let status = Label::new(Some("SEARCHING"));
    status.add_css_class("status");
    status.add_css_class("status-idle");
    status.set_hexpand(true);
    status.set_halign(gtk::Align::End);
    header.append(&status);
    let help_body = GtkBox::new(Orientation::Vertical, 12);
    for (icon, title, explanation) in [
        ("input-gaming-symbolic", "1. Select a control", "Click a key, dial or the touch strip on the preview. Dials have separate press and rotation events."),
        ("preferences-system-symbolic", "2. Configure an action", "Choose an event and action. More actions contains System, Multimedia, Organization, Device and OBS."),
        ("document-save-symbolic", "3. Save or test", "Save binding stores your configuration. Test action runs it immediately. Artwork is configured separately."),
    ] {
        help_body.append(&ui::section_heading(icon, title));
        help_body.append(&ui::helper(explanation));
    }
    let help_popover = ui::popover("Setup help", &help_body);
    let help_menu = ui::menu_button("help-browser-symbolic", "Help", &help_popover);
    header.append(&help_menu);
    root.append(&header);

    let connection_detail = Label::new(Some("Looking for USB device · VID 0FD9 / PID 0084"));
    connection_detail.add_css_class("muted");
    connection_detail.set_xalign(0.0);
    connection_detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
    connection_detail.set_max_width_chars(70);
    heading.append(&connection_detail);

    let layer_card = card();
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
    let create_layer = ui::button("document-save-symbolic", "Save new profile");
    let remove_layer = ui::button("edit-delete-symbolic", "Remove selected");
    remove_layer.add_css_class("destructive-action");
    remove_layer.set_has_frame(false);
    let management = GtkBox::new(Orientation::Vertical, 12);
    management.append(&ui::helper(
        "Profiles keep separate bindings and artwork. Add folders and pages to organize a profile.",
    ));
    management.append(&labeled("New profile name", &layer_name));
    management.append(&create_layer);
    let control_model = gtk::StringList::new(&["Button", "Encoder", "Touchpad"]);
    let control_picker = gtk::DropDown::new(Some(control_model.clone()), gtk::Expression::NONE);
    let control_number_model = gtk::StringList::new(&["1", "2", "3", "4", "5", "6", "7", "8"]);
    let control_number_picker =
        gtk::DropDown::new(Some(control_number_model.clone()), gtk::Expression::NONE);
    let event_model = gtk::StringList::new(&["Press"]);
    let event_picker = gtk::DropDown::new(Some(event_model.clone()), gtk::Expression::NONE);
    let action_model = gtk::StringList::new(&[
        "Move to layer",
        "Run command",
        "Open application",
        "Shortcut",
        "Emit legacy event",
        "More actions",
        "Play sound",
    ]);
    let action_picker = gtk::DropDown::new(Some(action_model.clone()), gtk::Expression::NONE);
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
    let super_mod = CheckButton::with_label("SUPER");
    let shift_mod = CheckButton::with_label("SHIFT");
    let ctrl_mod = CheckButton::with_label("CTRL");
    let alt_mod = CheckButton::with_label("ALT");
    let binding_status = Label::new(Some("Select a physical control to inspect its binding."));
    binding_status.add_css_class("muted");
    binding_status.set_xalign(0.0);
    binding_status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    binding_status.set_max_width_chars(32);
    binding_status.connect_label_notify(|label| label.set_tooltip_text(Some(&label.text())));
    let save_binding = ui::button("document-save-symbolic", "Save binding");
    save_binding.add_css_class("suggested-action");
    let remove_binding = ui::button("edit-delete-symbolic", "Remove");
    remove_binding.set_tooltip_text(Some("Remove binding"));
    remove_binding.set_has_frame(false);
    remove_binding.add_css_class("destructive-action");
    let control_number_row = labeled("Number", &control_number_picker);
    let layer_target_row = labeled("Destination profile or page", &layer_target_picker);
    let text_row = labeled("Command / key / shortcut key / legacy event", &text_entry);
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
    let modifier_buttons = GtkBox::new(Orientation::Horizontal, 8);
    modifier_buttons.append(&super_mod);
    modifier_buttons.append(&shift_mod);
    modifier_buttons.append(&ctrl_mod);
    modifier_buttons.append(&alt_mod);
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
    management.append(feature_runtime.management());
    management.append(&remove_layer);
    let management_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(360)
        .min_content_width(320)
        .child(&management)
        .build();
    let management_popover = ui::popover("Profiles, pages and folders", &management_scroll);
    let management_menu = ui::menu_button("folder-new-symbolic", "Organize", &management_popover);
    layer_row.append(&management_menu);
    // Integration settings are global, not part of every action's form.
    let obs_popover = ui::popover("OBS Studio", &feature_runtime.obs_panel);
    feature_runtime.obs_panel.set_expanded(true);
    let obs_menu = ui::menu_button("camera-video-symbolic", "OBS", &obs_popover);
    layer_row.append(&obs_menu);
    feature_runtime
        .profile_tabs()
        .set_action_widget(&layer_row, gtk::PackType::End);
    let navigation_row = GtkBox::new(Orientation::Horizontal, 8);
    feature_runtime.indicator.set_hexpand(true);
    navigation_row.append(&feature_runtime.indicator);
    navigation_row.append(feature_runtime.controls());
    layer_card.append(&navigation_row);
    if let Some(app) = window.application() {
        let cleanup = feature_runtime.clone();
        app.connect_shutdown(move |_| cleanup.cancel_timer());
    }

    let settings_card = card();
    settings_card.add_css_class("editor-card");
    let settings_heading = Label::new(Some(&control_heading(ControlKind::Button, Some(1))));
    settings_heading.add_css_class("section-label");
    settings_heading.add_css_class("control-heading");
    settings_heading.set_xalign(0.0);
    settings_card.append(&settings_heading);
    // Match the 414px chassis plus the preview card's padding and border.
    settings_card.set_size_request(300, 428);
    settings_card.set_spacing(12);
    settings_card.set_hexpand(true);
    settings_card.set_valign(gtk::Align::Start);
    let selector_row = GtkBox::new(Orientation::Horizontal, 8);
    selector_row.append(&labeled("Event", &binding_ui.event_picker));
    selector_row.append(&labeled("Action", &binding_ui.action_picker));
    settings_card.append(&selector_row);
    settings_card.append(&binding_ui.action_guidance);
    settings_card.append(&binding_ui.layer_target_row);
    settings_card.append(&binding_ui.text_row);
    settings_card.append(&binding_ui.application_row);
    settings_card.append(&binding_ui.args_row);
    settings_card.append(&binding_ui.modifier_row);
    settings_card.append(&binding_ui.feature_editor.root);
    let binding_buttons = GtkBox::new(Orientation::Horizontal, 8);
    let test_action = ui::button("media-playback-start-symbolic", "Test action");
    binding_buttons.append(&save_binding);
    binding_buttons.append(&test_action);
    binding_buttons.append(&remove_binding);
    settings_card.append(&binding_buttons);
    settings_card.append(&binding_ui.status);
    let artwork_content = GtkBox::new(Orientation::Vertical, 8);
    let target_picker = add_artwork_controls(&artwork_content, &window, &artwork);
    let artwork_section = gtk::Expander::new(Some("Artwork and touch zones"));
    artwork_section.set_child(Some(&artwork_content));
    settings_card.append(&artwork_section);
    let layer_hint = Label::new(Some(
        "Touch gestures bind the whole strip. Artwork targets are independent of encoder actions.",
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
        test_action.connect_clicked(move |_| match ui.binding_from_widgets(&layers) {
            Ok((layer, trigger, action)) => {
                runtime.execute(binding_identity(&layer, &trigger), action)
            }
            Err(error) => ui.status.set_text(&error),
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
        create_layer.connect_clicked(move |_| {
            let value = name.text().to_string();
            let trimmed = value.trim().to_owned();
            let result = layers.borrow_mut().create(&value);
            if let Err(error) = result {
                runtime.report(&format!("Could not create layer: {error}"), true);
            } else {
                let names = layers.borrow().names();
                // Keep picker indices aligned with LayerStore's sorted names.
                updating_layers.set(true);
                replace_string_list(&model, &names);
                ui.refresh_layer_targets(&names);
                let index = layer_index(&layers, &trimmed);
                picker.set_selected(index);
                updating_layers.set(false);
                if let Some(snapshot) = select_layer(&layers, index as usize) {
                    runtime.save_profile(snapshot);
                    runtime.edit_profile(&trimmed);
                }
                ui.load_selected_binding(&layers);
                ui.refresh_summaries(&layers);
                artwork.refresh(false);
                name.set_text("");
            }
        });
    }
    {
        let layers = layers.clone();
        let model = layer_model.clone();
        let picker = layer_picker.clone();
        let runtime = feature_runtime.clone();
        let ui = binding_ui.clone();
        let artwork = artwork.clone();
        let updating_layers = updating_layers.clone();
        let window = window.clone();
        remove_layer.set_sensitive(artwork.store.borrow().is_some());
        remove_layer.connect_clicked(move |_| {
            let index = picker.selected() as usize;
            if index == 0 {
                return;
            }
            let Some(name) = dropdown_value(&model, picker.selected()) else {
                return;
            };
            let confirmation = gtk::MessageDialog::builder()
                .transient_for(&window)
                .modal(true)
                .message_type(gtk::MessageType::Question)
                .text(format!("Remove \"{name}\"?"))
                .secondary_text("This removes its bindings and artwork. This cannot be undone.")
                .build();
            confirmation.add_button("Keep", ResponseType::Cancel);
            confirmation.add_button("Remove", ResponseType::Accept);
            confirmation
                .widget_for_response(ResponseType::Accept)
                .unwrap()
                .add_css_class("destructive-action");
            confirmation.set_default_response(ResponseType::Cancel);
            let layers = layers.clone();
            let model = model.clone();
            let picker = picker.clone();
            let runtime = runtime.clone();
            let ui = ui.clone();
            let artwork = artwork.clone();
            let updating_layers = updating_layers.clone();
            let window = window.clone();
            confirmation.connect_response(move |dialog, response| {
                if response == ResponseType::Accept {
                    // Resolve the named target again, not a potentially changed index.
                    let index = layers
                        .borrow()
                        .names()
                        .iter()
                        .position(|candidate| candidate == &name);
                    if let Some(index) = index {
                        let removed = artwork.remove_layer(index);
                        match removed {
                            Ok(Some(snapshot)) => {
                                runtime.persist(snapshot);
                                updating_layers.set(true);
                                model.remove(index as u32);
                                picker.set_selected(0);
                                updating_layers.set(false);
                                let names = layers.borrow().names();
                                ui.refresh_layer_targets(&names);
                                ui.load_selected_binding(&layers);
                                ui.refresh_summaries(&layers);
                                artwork.refresh(false);
                            }
                            Ok(None) => {}
                            Err(error) => show_artwork_error(
                                &window,
                                &format!(
                        "Layer not removed: {error}. Check artwork storage permissions and retry."
                    ),
                            ),
                        }
                    }
                }
                dialog.close();
            });
            confirmation.present();
        });
    }
    {
        let layers = layers.clone();
        let ui = binding_ui.clone();
        ui.control_picker.clone().connect_selected_notify(move |_| {
            ui.update_for_control(true);
            ui.load_selected_binding(&layers);
        });
    }
    {
        let layers = layers.clone();
        let ui = binding_ui.clone();
        ui.control_number_picker
            .clone()
            .connect_selected_notify(move |_| ui.load_selected_binding(&layers));
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
        save_binding.connect_clicked(move |_| match ui.binding_from_widgets(&layers) {
            Ok((layer, trigger, action)) => {
                let snapshot = {
                    let mut store = layers.borrow_mut();
                    store.bind(&layer, trigger, action).map(|_| store.clone())
                };
                match snapshot {
                    Ok(snapshot) => {
                        if runtime.save_binding(snapshot) {
                            ui.status.set_text("Binding updated; saving to disk…");
                        } else {
                            let error = "Could not save binding: saving worker is unavailable.";
                            ui.error(error);
                            toast.show(error, true);
                        }
                        ui.refresh_summaries(&layers);
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
        });
    }
    {
        let layers = layers.clone();
        let runtime = feature_runtime.clone();
        let ui = binding_ui.clone();
        remove_binding.connect_clicked(move |_| match ui.selected_layer_and_trigger(&layers) {
            Ok((layer, trigger)) => {
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
                        ui::feedback(&ui.status, false);
                        ui.refresh_summaries(&layers);
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
                feedback.set_visible(status.is_visible() || timer.is_visible());
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
    body.set_valign(gtk::Align::Start);
    body.add_css_class("configuration-grid");
    let sidebar = GtkBox::new(Orientation::Vertical, 6);
    sidebar.set_hexpand(true);
    sidebar.set_valign(gtk::Align::Start);
    sidebar.add_css_class("settings-sidebar");
    let device_area = card();
    device_area.add_css_class("device-canvas");
    device_area.set_valign(gtk::Align::Fill);
    // Reference image coordinates scaled to a compact physical front. Fixed
    // geometry keeps loaded artwork and long live messages from resizing hardware.
    let chassis = gtk::Fixed::new();
    chassis.set_halign(gtk::Align::Center);
    chassis.set_valign(gtk::Align::Center);
    chassis.set_vexpand(true);
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
    let touch_status = Label::new(Some("Waiting for touch input"));
    touch_status.add_css_class("touch-display");
    touch_status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    touch_status.set_max_width_chars(32);
    touch_status.set_size_request(328, 16);
    bezel.put(&touch_status, 23.0, 3.0);
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
        selectable_widgets.push(button.upcast());
        encoder_labels.push(label);
    }
    for (index, widget) in selectable_widgets.iter().take(8).enumerate() {
        connect_artwork_selection(
            &widget.clone().downcast::<Button>().unwrap(),
            &target_picker,
            ArtworkTarget::Keypad((index + 1) as u8),
        );
    }
    connect_artwork_selection(
        &strip_button,
        &target_picker,
        ArtworkTarget::TouchStripBackground,
    );
    for (index, button) in zone_select_buttons.iter().enumerate() {
        connect_artwork_selection(
            button,
            &target_picker,
            ArtworkTarget::EncoderZone((index + 1) as u8),
        );
    }
    // Summaries remain available through tooltips, never underneath the front.
    for (index, widget) in selectable_widgets.iter().enumerate() {
        let widget = widget.clone();
        let tooltip = move |text: &str| {
            if index < 8 {
                format!("Button {} · {text}", index + 1)
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

    let selected_widgets = Rc::new(selectable_widgets);
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
        1,
    );
    for (index, zone_button) in zone_select_buttons.into_iter().enumerate() {
        connect_control_selection(
            &touch_status,
            zone_button.clone(),
            &binding_ui,
            &layers,
            &settings_heading,
            selected_widgets.clone(),
            8,
            ControlKind::Touch,
            1,
        );
        let heading = settings_heading.clone();
        zone_button.connect_clicked(move |_| {
            heading.set_text(&format!("Configuring: Touchpad region {}", index + 1));
        });
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

    body.insert(&device_area, -1);
    sidebar.append(&settings_card);
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
        .child(&root)
        .build();
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
        feature_runtime,
        organization_writes: vec![create_layer, remove_layer],
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
    let MonitorWidgets {
        status,
        toast,
        binding_status,
        connection_detail,
        feature_runtime,
        key_labels,
        encoder_labels,
        touch_status,
        ..
    } = widgets;
    let ticks = Rc::new(RefCell::new([0i64; 4]));
    glib::timeout_add_local(Duration::from_millis(40), move || {
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
                MonitorEvent::Connection(state) => match state {
                    ConnectionState::Searching => {
                        status.set_text("SEARCHING");
                        status.remove_css_class("status-online");
                        status.add_css_class("status-idle");
                        connection_detail.set_text("Looking for USB device · VID 0FD9 / PID 0084");
                    }
                    ConnectionState::Connected => {
                        status.set_text("CONNECTED");
                        status.remove_css_class("status-idle");
                        status.add_css_class("status-online");
                        connection_detail
                            .set_text("Stream Deck + connected · HID input and artwork active");
                        artwork.refresh(true);
                    }
                    ConnectionState::Unavailable(detail) => {
                        status.set_text("UNAVAILABLE");
                        status.remove_css_class("status-online");
                        status.add_css_class("status-idle");
                        connection_detail.set_text(&detail);
                    }
                    ConnectionState::Disconnected(detail) => {
                        status.set_text("DISCONNECTED");
                        status.remove_css_class("status-online");
                        status.add_css_class("status-idle");
                        connection_detail.set_text(&detail);
                    }
                },
                MonitorEvent::Input(event) => {
                    if let Some((identity, action)) = matching_binding(&layers, &event) {
                        feature_runtime.execute(identity, action);
                    }
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
                        }
                        protocol::Event::TouchTap { x, y } => {
                            touch_status.set_text(&format!("TAP     ·     X {x}   /   Y {y}"))
                        }
                        protocol::Event::TouchPress { x, y } => touch_status
                            .set_text(&format!("PRESS & HOLD     ·     X {x}   /   Y {y}")),
                        protocol::Event::TouchFlick {
                            start_x,
                            start_y,
                            end_x,
                            end_y,
                        } => touch_status.set_text(&format!(
                            "FLICK     ·     ({start_x}, {start_y})  →  ({end_x}, {end_y})"
                        )),
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
                MonitorEvent::BindingSaveResult(result) => {
                    let (message, failed) = match result {
                        Ok(()) => ("Binding saved successfully.".to_owned(), false),
                        Err(error) => (format!("Could not save binding: {error}"), true),
                    };
                    binding_status.set_text(&message);
                    ui::feedback(&binding_status, failed);
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
    super_mod: CheckButton,
    shift_mod: CheckButton,
    ctrl_mod: CheckButton,
    alt_mod: CheckButton,
    status: Label,
    control_number_row: GtkBox,
    layer_target_row: GtkBox,
    text_row: GtkBox,
    application_row: GtkBox,
    args_row: GtkBox,
    modifier_row: GtkBox,
    feature_editor: feature_editor::FeatureEditor,
    action_guidance: Label,
    // Same order as the physical selection widgets: eight buttons, touch, four encoders.
    summaries: Vec<Label>,
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
        super_mod: CheckButton,
        shift_mod: CheckButton,
        ctrl_mod: CheckButton,
        alt_mod: CheckButton,
        status: Label,
        control_number_row: GtkBox,
        layer_target_row: GtkBox,
        text_row: GtkBox,
        application_row: GtkBox,
        args_row: GtkBox,
        modifier_row: GtkBox,
    ) -> Self {
        status.connect_label_notify(ui::clear_feedback);
        Self {
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
            action_guidance: ui::helper(""),
            summaries: (0..13)
                .map(|_| {
                    let label = Label::new(None);
                    label.add_css_class("muted");
                    label.set_xalign(0.0);
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    label.set_width_chars(10);
                    label.set_max_width_chars(12);
                    label.set_tooltip_text(Some(
                        "Assignments in the selected layer. CW = clockwise; CCW = counterclockwise. Exact ticks are legacy overrides. Select an event for full action details.",
                    ));
                    label
                })
                .collect(),
        }
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
            (0..13)
                .map(|index| {
                    let (control, number) = match index {
                        0..=7 => (ControlKind::Button, index + 1),
                        8 => (ControlKind::Touch, 1),
                        _ => (ControlKind::Encoder, index - 8),
                    };
                    control_summary(&store, &layer, control, number as u8)
                })
                .collect::<Vec<_>>()
        };
        for (label, text) in self.summaries.iter().zip(texts) {
            label.set_text(&text.replace('\n', " · "));
            label.set_tooltip_text(Some(&text));
        }
    }

    fn update_for_control(&self, reset_event: bool) {
        let previous_event = dropdown_value(&self.event_model, self.event_picker.selected());
        let control = self.control_kind();
        let numbers: Vec<String> = match control {
            ControlKind::Button => (1..=8).map(|n| n.to_string()).collect(),
            ControlKind::Encoder => (1..=4).map(|n| n.to_string()).collect(),
            ControlKind::Touch => Vec::new(),
        };
        replace_string_list(&self.control_number_model, &numbers);
        self.control_number_row
            .set_visible(!matches!(control, ControlKind::Touch));
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
                if let Some(number) = number {
                    if number != 1 {
                        return Err("touch strip does not have numbered controls".into());
                    }
                }
                self.control_picker.set_selected(2);
                self.update_for_control(reset_event);
                0
            }
        };
        if !matches!(control, ControlKind::Touch) {
            self.control_number_picker.set_selected(selected as u32);
        }
        Ok(())
    }

    fn update_action_visibility(&self) {
        let kind = self.action_kind();
        self.feature_editor
            .show_direct_sound(self.action_picker.selected() == 6);
        self.action_guidance.set_text(match kind {
            ActionKind::MoveToLayer => {
                "Switch to another profile or page when this control is used."
            }
            ActionKind::RunCommand => "Run a shell command with /bin/sh. Saving does not run it.",
            ActionKind::OpenApplication => "Enter the executable path and one argument per line.",
            ActionKind::Shortcut => "Type a key or full chord. The checkboxes add modifiers.",
            ActionKind::EmitLegacy => "Emit a named internal event for an existing legacy binding.",
            ActionKind::Extended => "",
        });
        self.action_guidance
            .set_visible(!matches!(kind, ActionKind::Extended));
        if let Some(label) = self
            .text_row
            .first_child()
            .and_then(|child| child.downcast::<Label>().ok())
        {
            label.set_text(match kind {
                ActionKind::RunCommand => "Shell command",
                ActionKind::Shortcut => "Shortcut",
                ActionKind::EmitLegacy => "Event name",
                ActionKind::MoveToLayer | ActionKind::OpenApplication | ActionKind::Extended => "",
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
            ActionKind::MoveToLayer | ActionKind::OpenApplication | ActionKind::Extended => "",
        }));
    }

    fn refresh_layer_targets(&self, names: &[String]) {
        replace_string_list(&self.layer_target_model, names);
        self.layer_target_picker.set_selected(0);
    }

    fn load_selected_binding(&self, layers: &RefCell<LayerStore>) {
        self.clear_action_fields();
        let Ok((layer, trigger)) = self.selected_layer_and_trigger(layers) else {
            self.status.set_text("Select a valid control event.");
            return;
        };
        let action = {
            let store = layers.borrow();
            existing_action(&store, &layer, &trigger)
        };
        match action {
            Some(action) => {
                self.populate_action(&action);
                self.status.set_text("Binding loaded for editing.");
            }
            None => self
                .status
                .set_text("No binding exists for that control event."),
        }
        self.update_action_visibility();
    }

    fn selected_layer_and_trigger(
        &self,
        layers: &RefCell<LayerStore>,
    ) -> Result<(String, Trigger), String> {
        let layer = dropdown_value(&self.layer_model, self.layer_picker.selected())
            .ok_or_else(|| "Select a layer.".to_string())?;
        if !layers.borrow().layers().contains_key(&layer) {
            return Err("Selected layer no longer exists.".into());
        }
        let number = if matches!(self.control_kind(), ControlKind::Touch) {
            1
        } else {
            dropdown_value(
                &self.control_number_model,
                self.control_number_picker.selected(),
            )
            .and_then(|value| value.parse::<u8>().ok())
            .ok_or_else(|| "Select a control number.".to_string())?
        };
        let trigger = trigger_from_selection(self.control_kind(), number, self.event_kind())
            .map_err(|error| error.0)?;
        Ok((layer, trigger))
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

    fn clear_action_fields(&self) {
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
        ControlKind::Touch => "Configuring: Touchpad".into(),
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
    button.connect_clicked(move |_| {
        if let Err(error) = ui.select_control(control, Some(number), false) {
            ui.status.set_text(&error);
            return;
        }
        set_selected_control(&selected_widgets, selected_index);
        heading.set_text(&control_heading(control, Some(number)));
        ui.load_selected_binding(&layers);
    });
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
) -> Option<(String, Action)> {
    let store = layers.borrow();
    store
        .matching_binding(event)
        .map(|(trigger, action)| (binding_identity(store.active(), &trigger), action))
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
}

impl ArtworkUi {
    fn capture_owner(&self) -> ChooserOwner {
        let name = self.layers.borrow().active().to_owned();
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

    fn refresh(&self, force: bool) {
        let layer = self.layers.borrow().active().to_owned();
        if !force && self.last_layer.borrow().as_deref() == Some(&layer) {
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
                        store.assigned_path_in_layer(&layer, *target),
                    )
                })
                .collect::<Vec<_>>();
            (paths, desired_artwork_commands(store, &layer))
        };
        for (picture, path) in paths {
            set_picture_path(&picture, path);
        }
        let _ = self.output.send(ArtworkCommand::Replace(commands));
        *self.last_layer.borrow_mut() = Some(layer);
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
        let active = self.layers.borrow().active() == owner.name;
        if active {
            self.refresh(true);
        }
        Ok(())
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
    if !layers.borrow().layers().contains_key(owner) {
        return Err(format!("The chooser's layer '{owner}' was removed. Close it and choose an image for an existing layer."));
    }
    store
        .as_mut()
        .ok_or("Artwork storage is unavailable; repair it and restart.")?
        .assign_in_layer(owner, target, path)
        .map_err(|error| error.to_string())?;
    Ok(())
}

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

fn connect_artwork_selection(button: &Button, picker: &gtk::DropDown, target: ArtworkTarget) {
    let picker = picker.clone();
    button.connect_clicked(move |_| {
        picker.set_selected(
            ArtworkTarget::ALL
                .iter()
                .position(|item| *item == target)
                .unwrap() as u32,
        );
    });
}

fn add_artwork_controls(
    parent: &GtkBox,
    window: &ApplicationWindow,
    artwork: &ArtworkUi,
) -> gtk::DropDown {
    let targets = gtk::StringList::new(
        &ArtworkTarget::ALL
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
    parent.append(&labeled("Artwork target (active layer)", &picker));
    let selected = artwork.selected_target.clone();
    picker.connect_selected_notify(move |picker| {
        if let Some(target) = ArtworkTarget::ALL.get(picker.selected() as usize) {
            selected.set(*target);
        }
    });
    let controls = GtkBox::new(Orientation::Horizontal, 4);
    controls.add_css_class("artwork-controls");
    let choose = ui::button("image-x-generic-symbolic", "Image…");
    choose.set_tooltip_text(Some("Choose image for this region in the active layer"));
    let clear = ui::button("edit-clear-symbolic", "Clear");
    clear.set_tooltip_text(Some("Clear artwork for this region in the active layer"));
    controls.append(&choose);
    controls.append(&clear);
    parent.append(&controls);

    let available = artwork.store.borrow().is_some();
    choose.set_sensitive(available);
    clear.set_sensitive(available);
    let parent_window = window.clone();
    let artwork_ref = artwork.clone();
    choose.connect_clicked(move |_| {
        let owner = artwork_ref.capture_owner();
        let chooser = FileChooserNative::new(
            Some("Choose artwork"),
            Some(&parent_window),
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
                        show_artwork_error(
                            &error_parent,
                            &format!("Could not assign image: {error}"),
                        );
                    }
                }
            }
        });
        chooser.show();
    });
    let artwork = artwork.clone();
    let parent_window = window.clone();
    clear.connect_clicked(move |_| {
        let layer = artwork.layers.borrow().active().to_owned();
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
                show_artwork_error(&parent_window, &format!("Could not clear artwork: {error}"))
            }
        }
    });
    picker
}

fn desired_artwork_commands(store: &ArtworkStore, layer: &str) -> Vec<ArtworkCommand> {
    let mut commands = (1..=8)
        .map(|key| {
            let target = ArtworkTarget::Keypad(key);
            match store.assigned_path_in_layer(layer, target) {
                Some(path) => ArtworkCommand::Assign { target, path },
                None => ArtworkCommand::Clear { target },
            }
        })
        .collect::<Vec<_>>();
    commands.extend(touch_strip_commands(store, layer));
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
    gtk::style_context_add_provider_for_display(
        &gtk::prelude::WidgetExt::display(window),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdeck_plus::layers::SwipeDirection;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static GTK_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn gtk_test_lock() -> MutexGuard<'static, ()> {
        GTK_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("GTK test lock should not be poisoned")
    }

    fn test_binding_ui() -> Option<BindingUi> {
        if gtk::init().is_err() {
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
            "Move to layer",
            "Run command",
            "Open application",
            "Shortcut",
            "Emit legacy event",
            "More actions",
            "Play sound",
        ]);
        let action_picker = gtk::DropDown::new(Some(action_model), gtk::Expression::NONE);
        let layer_target_model = gtk::StringList::new(&["Base"]);
        let layer_target_picker =
            gtk::DropDown::new(Some(layer_target_model.clone()), gtk::Expression::NONE);
        let text_entry = Entry::new();
        let application_entry = Entry::new();
        let args_view = TextView::new();
        let super_mod = CheckButton::with_label("SUPER");
        let shift_mod = CheckButton::with_label("SHIFT");
        let ctrl_mod = CheckButton::with_label("CTRL");
        let alt_mod = CheckButton::with_label("ALT");
        let control_number_row = labeled("Number", &control_number_picker);
        let layer_target_row = labeled("Destination layer", &layer_target_picker);
        let text_row = labeled("Command / key / shortcut key / legacy event", &text_entry);
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
        let _guard = gtk_test_lock();
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
        ui.feature_editor.assert_roundtrips();
        ui.action_picker.set_selected(5);
        ui.update_action_visibility();
        assert!(ui.feature_editor.root.is_visible());
        assert!(!ui.layer_target_row.is_visible());
        assert!(!ui.text_row.is_visible());
        assert!(!ui.application_row.is_visible());
        assert!(!ui.args_row.is_visible());
        assert!(!ui.modifier_row.is_visible());
        assert_legacy_keys_load_without_rewriting_and_save_as_shortcuts(&ui);
        ui.control_picker.set_selected(2);
        ui.update_for_control(true);
        assert!(!ui.control_number_row.is_visible());

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
        eprintln!("GTK artwork preview assertions executed");
        assert_summaries_refresh_for_binding_and_layer_changes(&ui);
        eprintln!("GTK assertions executed: selection, styles, summaries and refresh paths");
        assert!(ui.summaries[0].text().contains("Unassigned"));
        assert_production_layout_fits();
        assert_binding_save_toasts();
        assert_profile_tabs_and_focus();
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
                    .is_some_and(|label| label.text() == "Profile to save")
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
        button("Save profile").emit_clicked();
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
        button("Save profile").emit_clicked();
        assert_eq!(tabs.n_pages(), 2);
        let mut entries = Vec::new();
        collect_entries(widgets.root.upcast_ref(), &mut entries);
        let name = entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("New profile name"))
            .unwrap();
        name.set_text("Manual");
        button("Save new profile").emit_clicked();
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
            .find(|button| button_caption(button).as_deref() == Some("Save new profile"))
            .unwrap();
        let save_profile = buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some("Save profile"))
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
                MonitorEvent::AppEvent { .. }
            ));
        }
        // Then: each binding begins with the first chord and advances independently.
        let recorded = recorded.lock().unwrap();
        assert_eq!(recorded.len(), 4);
        assert_eq!(recorded[0], recorded[1]);
        assert_eq!(recorded[2], recorded[3]);
        assert_ne!(recorded[0], recorded[2]);
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
                "Move to layer",
                "Run command",
                "Open application",
                "Shortcut",
                "Emit legacy event",
                "More actions",
                "Play sound",
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

    fn assert_production_layout_fits() {
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
        };
        let (sender, commands) = mpsc::channel();
        let widgets = build_widgets(window.clone(), layers.clone(), artwork.clone(), sender);
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
        assert_eq!(
            (window.default_width(), window.default_height()),
            (DEFAULT_WIDTH, DEFAULT_HEIGHT)
        );
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
            .unwrap();
        if std::env::var_os("STREAMDECK_QA_DIR").is_some() {
            widgets.artwork_section.set_expanded(false);
            let _ = rendered_pixels(&window, &widgets.root);
        }
        assert_normal_no_scroll(&widgets, &content, &artwork, "empty");
        assert_inactive_backgrounds(&window, &widgets, &content, &artwork);
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
        assert_normal_no_scroll(&widgets, &content, &artwork, "loaded long values");
        for (width, height) in [(1840, 980), (1280, 720), (960, 640), (640, 480)] {
            widgets.root.allocate(width, height, -1, None);
            assert_eq!(widgets.root.width(), width);
            assert_eq!(widgets.root.height(), height);
            assert!(widgets.binding_ui.action_picker.is_visible());
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
                button.emit_clicked();
                assert_eq!(ui.control_picker.selected(), 2);
                assert_eq!(ui.event_model.n_items(), 7);
            }
        }
        let button = key_button(&widgets.key_labels[7]);
        button.emit_clicked();
        assert_eq!(ui.control_number_picker.selected(), 7);
        assert_eq!(ui.text_entry.text(), command);
        assert_eq!(
            ui.summaries[7].tooltip_text().unwrap(),
            ui.summaries[7].text()
        );
        eprintln!("GTK production selection and full-editor assertions executed");
        assert_normal_no_scroll(&widgets, &content, &artwork, "cleared long values");
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
        std::fs::create_dir_all(&directory).unwrap();
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
        widgets.root.set_size_request(1000, 700);
        widgets.binding_ui.action_picker.set_selected(0);
        widgets.binding_ui.update_action_visibility();
        widgets.root.vadjustment().set_value(0.0);
        for (name, menu) in [
            ("help", &widgets.help_menu),
            ("organization", &widgets.management_menu),
            ("obs", &widgets.obs_menu),
        ] {
            menu.popup();
            let panel = menu.popover().unwrap();
            rendered_texture(window, &panel)
                .save_to_png(directory.join(format!("popover-{name}.png")))
                .unwrap();
            let close = panel
                .child()
                .unwrap()
                .first_child()
                .unwrap()
                .last_child()
                .unwrap()
                .downcast::<Button>()
                .unwrap();
            close.emit_clicked();
            assert!(!panel.is_visible(), "popover Close must dismiss its panel");
        }
        eprintln!("GTK exact-size captures executed: 640x480, 1000x700, 1280x800, 1840x980, all actions and three popovers");
        window.set_child(gtk::Widget::NONE);
        stage.remove(&widgets.root);
        widgets.root.set_size_request(-1, -1);
        window.set_child(Some(&widgets.root));
    }

    fn assert_feature_integration(
        window: &ApplicationWindow,
        widgets: &MonitorWidgets,
        layers: &Rc<RefCell<LayerStore>>,
        commands: &mpsc::Receiver<LayerCommand>,
    ) {
        use sdeck_plus::desktop_actions::{DesktopAction, MediaCommand};
        use sdeck_plus::features::FeatureAction;
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
            assert_eq!(snapshot.layers()[&layer].bindings[&trigger], expected);
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

        find("New page").emit_clicked();
        assert!(widgets.feature_runtime.indicator.text().contains("Page 2"));
        assert_eq!(
            ui.layer_picker.selected(),
            layer_index(layers, layers.borrow().active())
        );
        let mut entries = Vec::new();
        collect_entries(
            widgets.feature_runtime.management().upcast_ref(),
            &mut entries,
        );
        let page_name = entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("Page name"))
            .unwrap();
        assert_eq!(page_name.text(), "Page 2");
        while commands.try_recv().is_ok() {}
        let active = layers.borrow().active().to_owned();
        page_name.set_text("Audio Mix");
        find("Rename page").emit_clicked();
        assert_eq!(layers.borrow().active(), active);
        assert!(widgets
            .feature_runtime
            .indicator
            .text()
            .contains("Audio Mix"));
        let LayerCommand::Persist(snapshot, Some(metadata)) = commands.try_recv().unwrap() else {
            panic!("renaming must submit the updated metadata for persistence");
        };
        assert_eq!(metadata.current(&snapshot).page_name, "Audio Mix");
        assert_eq!(snapshot.active(), active);
        assert!(
            commands.try_recv().is_err(),
            "renaming must not execute actions"
        );
        page_name.set_text("");
        find("Rename page").emit_clicked();
        assert!(
            commands.try_recv().is_err(),
            "invalid names must not be persisted"
        );
        assert!(widgets
            .feature_runtime
            .status
            .has_css_class("feedback-error"));
        assert!(widgets
            .feature_runtime
            .indicator
            .text()
            .contains("Audio Mix"));
        page_name.set_text("Keyboard Mix");
        page_name.emit_activate();
        let LayerCommand::Persist(snapshot, Some(metadata)) = commands.try_recv().unwrap() else {
            panic!("Enter must persist the page name from the editor");
        };
        assert_eq!(metadata.current(&snapshot).page_name, "Keyboard Mix");
        assert!(widgets
            .feature_runtime
            .indicator
            .text()
            .contains("Keyboard Mix"));
        entries
            .iter()
            .find(|entry| entry.placeholder_text().as_deref() == Some("New folder name"))
            .unwrap()
            .set_text("Studio");
        find("New folder").emit_clicked();
        assert!(widgets.feature_runtime.indicator.text().contains("Studio"));
        assert_eq!(page_name.text(), "Page 1");
        find("Parent").emit_clicked();
        find("Next").emit_clicked();
        assert!(widgets.feature_runtime.indicator.text().contains("Page 2"));
        assert_eq!(page_name.text(), "Keyboard Mix");
        find("Previous").emit_clicked();
        assert!(widgets.feature_runtime.indicator.text().contains("Page 1"));
        assert_eq!(page_name.text(), "Page 1");
        eprintln!("GTK page renaming executed: metadata snapshot, invalid-name rejection and numeric navigation");

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
        assert_eq!(picker.model().unwrap().n_items(), 13);
        let mut buttons = Vec::new();
        collect_layout_buttons(content, &mut buttons);
        let clear = buttons
            .iter()
            .find(|b| button_caption(b).as_deref() == Some("Clear"))
            .unwrap();
        assert!(clear.is_sensitive());
        for (index, target) in ArtworkTarget::ALL.into_iter().enumerate() {
            picker.set_selected(index as u32);
            assert_eq!(artwork.selected_target.get(), target);
            let owner = artwork.capture_owner();
            picker.set_selected(((index + 1) % 13) as u32);
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
        picker.set_selected(0);
        clear.emit_clicked();
        assert!(artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Base", ArtworkTarget::Keypad(1))
            .is_some());
        assert!(artwork
            .store
            .borrow()
            .as_ref()
            .unwrap()
            .assigned_path_in_layer("Studio", ArtworkTarget::Keypad(1))
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
                    assert_eq!(widgets.binding_ui.event_model.n_items(), 7);
                    let heading = sidebar_settings(widgets)
                        .first_child()
                        .unwrap()
                        .downcast::<Label>()
                        .unwrap();
                    assert_eq!(
                        heading.text().as_str(),
                        format!("Configuring: Touchpad region {zone}")
                    );
                    key_button(&widgets.key_labels[0]).emit_clicked();
                    assert_eq!(heading.text().as_str(), "Configuring: Key 1");
                }
                ArtworkTarget::TouchStripBackground => {
                    picker.set_selected(
                        ArtworkTarget::ALL.iter().position(|t| t == target).unwrap() as u32,
                    );
                    assert_eq!(artwork.selected_target.get(), *target);
                }
            }
        }
        eprintln!("GTK shared editor executed: all 13 assign/clear paths, key/zone/background selection, encoder independence, chooser target/layer capture and active-layer Clear");
    }

    fn assert_normal_no_scroll(
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
            eprintln!("GTK no-scroll {phase} action {action}: height-for-width(924)={minimum}, allocation={}x{}, upper/page={}/{}, {}/{}", content.width(), content.height(), horizontal.upper(), horizontal.page_size(), vertical.upper(), vertical.page_size());
            assert!(
                horizontal.upper() <= horizontal.page_size(),
                "normal width scrolls"
            );
            assert!(
                vertical.upper() <= vertical.page_size(),
                "normal height scrolls"
            );
            assert_eq!(horizontal.value(), 0.0);
            assert_eq!(vertical.value(), 0.0);
            assert_visible_bounds(content, &widgets.root);
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
            let settings_bounds = settings.compute_bounds(content).unwrap();
            let device_bounds = device.compute_bounds(content).unwrap();
            eprintln!(
                "GTK aligned panels {phase} action {action}: settings={}, preview={}, top={}/{}",
                settings_bounds.height(),
                device_bounds.height(),
                settings_bounds.y(),
                device_bounds.y()
            );
            assert_eq!(settings.clone().downcast::<GtkBox>().unwrap().spacing(), 12);
            assert_eq!(settings_bounds.y(), device_bounds.y());
            assert!(
                settings_bounds.height() >= device_bounds.height(),
                "settings must be at least as tall as the preview"
            );
            if matches!(action, 0 | 1 | 4) {
                assert_eq!(
                    settings_bounds.height(),
                    device_bounds.height(),
                    "compact settings and preview lower edges must align"
                );
            }
            let bounds = content.compute_bounds(&widgets.root).unwrap();
            assert_eq!((bounds.x(), bounds.y()), (24.0, 24.0));
            assert_eq!((bounds.width(), bounds.height()), (952.0, 652.0));
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
            assert!(widgets.binding_ui.action_picker.is_visible());
            assert!(widgets.binding_ui.event_picker.is_visible());
        }
        eprintln!("GTK 1000x700 {phase} five-action no-scroll assertions executed");
        widgets.artwork_section.set_expanded(true);
    }

    fn assert_visible_bounds(widget: &gtk::Widget, root: &gtk::ScrolledWindow) {
        if !widget.is_visible() {
            return;
        }
        if let Some(label) = widget.downcast_ref::<Label>() {
            assert_ne!(label.text(), "LAST EVENT");
        }
        assert!(!widget.has_css_class("events-card"));
        let bounds = widget.compute_bounds(root).unwrap();
        assert!(
            bounds.x() >= -1.0
                && bounds.y() >= -1.0
                && bounds.x() + bounds.width() <= root.width() as f32 + 1.0
                && bounds.y() + bounds.height() <= root.height() as f32 + 1.0,
            "widget {} outside client: {bounds:?}",
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
        wait_for_frame(widget);
        let snapshot = gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(widget)).snapshot(
            &snapshot,
            widget.width() as f64,
            widget.height() as f64,
        );
        let node = snapshot.to_node().expect("rendered GTK widget");
        window.renderer().unwrap().render_texture(
            &node,
            Some(&gtk::graphene::Rect::new(
                0.0,
                0.0,
                widget.width() as f32,
                widget.height() as f32,
            )),
        )
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
        button.label().or_else(|| {
            button
                .child()
                .and_then(|child| child.downcast::<GtkBox>().ok())
                .and_then(|content| content.last_child())
                .and_then(|child| child.downcast::<Label>().ok())
                .map(|label| label.text())
        })
    }

    fn device_canvas(widgets: &MonitorWidgets) -> gtk::Widget {
        key_button(&widgets.key_labels[0])
            .parent()
            .unwrap()
            .parent()
            .unwrap()
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

    fn assert_sidebar_hierarchy(widgets: &MonitorWidgets, content: &gtk::Widget) {
        let settings = sidebar_settings(widgets);
        assert!(settings.next_sibling().is_none(), "no event card");
        let sidebar = settings.parent().unwrap().downcast::<GtkBox>().unwrap();
        assert_eq!(sidebar.orientation(), Orientation::Vertical);
        assert_eq!(sidebar.spacing(), 6);
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
            body.last_child()
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
            (24, 24, 24, 24)
        );
        for panel in [device_canvas(widgets), settings] {
            let panel = panel.downcast::<GtkBox>().unwrap();
            assert!(panel.has_css_class("card"));
            assert_eq!(
                panel.spacing(),
                if panel.has_css_class("device-canvas") {
                    4
                } else {
                    12
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
        for panel in [&device, &settings] {
            let bounds = panel.compute_bounds(content).unwrap();
            let first = panel
                .first_child()
                .unwrap()
                .compute_bounds(content)
                .unwrap();
            if panel.has_css_class("device-canvas") {
                assert_eq!((first.width(), first.height()), (426.0, 414.0));
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
                (first.x() - bounds.x() - 13.0).abs() <= 1.0,
                "shared CSS inner padding: {first:?}, {bounds:?}"
            );
            assert!(
                (first.y() - bounds.y() - 13.0).abs() <= 1.0,
                "shared CSS top padding: {first:?}, {bounds:?}"
            );
        }
        let settings = settings.compute_bounds(content).unwrap();
        let device = device.compute_bounds(content).unwrap();
        if settings.y() == device.y() {
            assert!((settings.x() - device.x() - device.width() - 16.0).abs() <= 1.0);
        } else {
            assert!((settings.x() - device.x()).abs() <= 1.0);
            assert!((settings.y() - device.y() - device.height() - 16.0).abs() <= 1.0);
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
        assert_eq!((front.width(), front.height()), (426.0, 414.0));
        assert!((front.width() / front.height() - 774.0 / 752.0).abs() < 0.02);
        for column in 0..4 {
            assert_eq!(key_bounds[column].width(), 60.0);
            assert_eq!(key_bounds[column + 4].x(), key_bounds[column].x());
            assert_eq!(key_bounds[column + 4].y() - key_bounds[column].y(), 67.0);
            if column < 3 {
                assert_eq!(key_bounds[column + 1].x() - key_bounds[column].x(), 91.0);
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
        assert_eq!((bezel_bounds.width(), bezel_bounds.height()), (374.0, 77.0));
        assert!(strip.x() > bezel_bounds.x() && strip.y() > bezel_bounds.y());
        assert!(strip.x() + strip.width() < bezel_bounds.x() + bezel_bounds.width());
        let rail = chassis.last_child().unwrap();
        assert!(rail.has_css_class("hardware-rail"));
        let mut knob = rail.first_child();
        for key in key_bounds.iter().take(4) {
            let current = knob.unwrap();
            let bounds = current.compute_bounds(content).unwrap();
            assert_eq!((bounds.width(), bounds.height()), (46.0, 46.0));
            assert_eq!(
                bounds.x() + bounds.width() / 2.0,
                key.x() + key.width() / 2.0
            );
            knob = current.next_sibling();
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
                        | "Remove"
                        | "Save new profile"
                        | "Remove selected"
                        | "Test action"
                        | "Previous"
                        | "Next"
                        | "Parent"
                        | "New page"
                        | "Rename page"
                        | "Save profile"
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
        let vertical = viewport.vadjustment();
        let horizontal = viewport.hadjustment();
        horizontal.set_value(0.0);
        vertical.set_value(0.0);
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
            "control is horizontally clipped: {bounds:?}"
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
        assert_eq!(ui.summaries[0].text(), "Press: Layer Studio");
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
        assert_eq!(ui.summaries[0].text(), "Press: Layer Studio");
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
        assert!(!ui.control_number_row.is_visible());
        assert_eq!(ui.event_model.n_items(), 7);
        assert!(ui
            .select_control(ControlKind::Button, Some(9), true)
            .is_err());
        assert!(ui
            .select_control(ControlKind::Encoder, Some(5), true)
            .is_err());
        assert!(ui
            .select_control(ControlKind::Touch, Some(2), true)
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
        if gtk::init().is_err() {
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
