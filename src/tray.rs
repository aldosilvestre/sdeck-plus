use gtk::{gio, glib, prelude::*, Application};
use ksni::{blocking::TrayMethods, menu::*, MenuItem, Tray};

enum Command {
    Configure,
    About,
    Quit,
    ToggleKeepRunning,
    ToggleStartInTray,
    Error(String),
}

struct MonitorTray {
    commands: async_channel::Sender<Command>,
    autostart: bool,
    keep_running: bool,
    start_in_tray: bool,
    executable: std::path::PathBuf,
    icon: ksni::Icon,
}

impl MonitorTray {
    fn send(&self, command: Command) {
        if let Err(error) = self.commands.try_send(command) {
            eprintln!("Tray command delivery failed: {error}");
        }
    }
}

impl Tray for MonitorTray {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn title(&self) -> String {
        crate::APP_NAME.into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.icon.clone()]
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open configuration".into(),
                icon_name: "preferences-system".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Command::Configure)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Start at login".into(),
                checked: self.autostart,
                activate: Box::new(|tray: &mut Self| {
                    let enabled = !tray.autostart;
                    match crate::autostart::set_enabled(enabled, &tray.executable) {
                        Ok(()) => tray.autostart = enabled,
                        Err(error) => tray.send(Command::Error(format!(
                            "Could not change start at login: {error}"
                        ))),
                    }
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Keep running when window closes".into(),
                checked: self.keep_running,
                activate: Box::new(|tray: &mut Self| tray.send(Command::ToggleKeepRunning)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Start in tray".into(),
                checked: self.start_in_tray,
                activate: Box::new(|tray: &mut Self| tray.send(Command::ToggleStartInTray)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "About".into(),
                icon_name: "help-about".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Command::About)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Command::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn menu_about_to_show(&mut self) {
        match crate::autostart::is_enabled() {
            Ok(enabled) => self.autostart = enabled,
            Err(error) => self.send(Command::Error(format!(
                "Could not read start at login: {error}"
            ))),
        }
    }
}

pub fn install(app: &Application) -> Result<(), Box<dyn std::error::Error>> {
    let (commands, receiver) = async_channel::unbounded();
    let autostart = crate::autostart::is_enabled();
    let preference_path = crate::tray_preference::path();
    let preference = preference_path
        .as_ref()
        .map_err(|error| error.to_string())
        .and_then(|path| crate::tray_preference::load(path).map_err(|error| error.to_string()));
    let keep_running = *preference.as_ref().unwrap_or(&true);
    let start_in_tray = preference_path
        .as_ref()
        .ok()
        .and_then(|path| crate::tray_preference::load_start_in_tray(path).ok())
        .unwrap_or(true);
    let tray = MonitorTray {
        commands,
        autostart: *autostart.as_ref().unwrap_or(&false),
        keep_running,
        start_in_tray,
        executable: std::env::current_exe()?,
        icon: crate::app_icon::tray_pixmap()?,
    };
    let handle = tray.spawn()?;
    let shutdown_handle = handle.clone();
    let hold = std::cell::RefCell::new(Some(app.hold()));
    app.connect_shutdown(move |_| {
        shutdown_handle.shutdown().wait();
        hold.borrow_mut().take();
    });
    let quit = gio::SimpleAction::new("quit", None);
    quit.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| app.quit()
    ));
    app.add_action(&quit);
    let preference_action = install_close_preference(app, keep_running);
    let app = app.clone();
    glib::MainContext::default().spawn_local(async move {
        let mut start_in_tray = start_in_tray;
        if let Err(error) = autostart {
            show_error(&app, &format!("Could not read start at login: {error}"));
        }
        if let Err(error) = preference {
            show_error(&app, &format!("Could not read tray preference: {error}. Original preference has not been replaced."));
        }
        while let Ok(command) = receiver.recv().await {
            match command {
                Command::Configure => crate::build_ui(&app),
                Command::About => show_about(&app),
                Command::Quit => {
                    app.activate_action("quit", None);
                    break;
                }
                Command::ToggleKeepRunning => {
                    let enabled = !crate::keep_running_on_close(&app);
                    let result = match &preference_path {
                        Ok(path) => save_close_preference(&app, &preference_action, path, enabled),
                        Err(error) => Err(std::io::Error::new(error.kind(), error.to_string())),
                    };
                    match result {
                        Ok(()) => {
                            if handle.update(|tray| tray.keep_running = enabled).is_none() {
                                show_error(&app, "System tray stopped while applying close preference.");
                            }
                        }
                        Err(error) => show_error(&app, &format!("Could not save tray preference: {error}. Close behavior has not changed.")),
                    }
                }
                Command::ToggleStartInTray => {
                    let enabled = !start_in_tray;
                    let result = match &preference_path {
                        Ok(path) => crate::tray_preference::save_start_in_tray(path, enabled),
                        Err(error) => Err(std::io::Error::new(error.kind(), error.to_string())),
                    };
                    match result {
                        Ok(()) => {
                            start_in_tray = enabled;
                            if handle.update(|tray| tray.start_in_tray = enabled).is_none() {
                                show_error(&app, "System tray stopped while applying start preference.");
                            }
                        }
                        Err(error) => show_error(&app, &format!("Could not save tray preference: {error}. Start behavior has not changed.")),
                    }
                }
                Command::Error(message) => show_error(&app, &message),
            }
        }
    });
    Ok(())
}

pub(super) fn install_close_preference(app: &Application, enabled: bool) -> gio::SimpleAction {
    let action = gio::SimpleAction::new_stateful("keep-running", None, &enabled.to_variant());
    app.add_action(&action);
    action
}

pub(super) fn apply_close_preference(app: &Application, action: &gio::SimpleAction, enabled: bool) {
    if let Some(window) = crate::configuration_window(app) {
        window.set_hide_on_close(enabled);
    }
    action.set_state(&enabled.to_variant());
}

pub(super) fn save_close_preference(
    app: &Application,
    action: &gio::SimpleAction,
    path: &std::path::Path,
    enabled: bool,
) -> std::io::Result<()> {
    crate::tray_preference::save(path, enabled)?;
    apply_close_preference(app, action, enabled);
    Ok(())
}

fn show_error(app: &Application, message: &str) {
    crate::build_ui(app);
    if let Some(window) = crate::configuration_window(app) {
        crate::show_artwork_error(&window, message);
    }
}

pub(super) fn show_about(app: &Application) {
    if let Some(window) = app
        .windows()
        .into_iter()
        .find(|window| window.widget_name() == "about")
    {
        window.present();
        return;
    }
    let dialog = gtk::AboutDialog::builder()
        .application(app)
        .name("about")
        .program_name(crate::APP_NAME)
        .version(env!("CARGO_PKG_VERSION"))
        .comments(
            "Configure and control an Elgato Stream Deck + on Linux.\nLicensed under the MIT license.",
        )
        .authors(["Aldo Silvestre"])
        .copyright("Copyright (c) 2026 Aldo Silvestre")
        .license(include_str!("../LICENSE"))
        .wrap_license(true)
        .logo_icon_name(crate::APP_NAME)
        .build();
    if let Some(window) = crate::configuration_window(app) {
        dialog.set_transient_for(Some(&window));
    }
    dialog.present();
    GtkWindowExt::set_focus(&dialog, None::<&gtk::Widget>);
    // GTK selects the first metadata label by default; start with readable text.
    let mut children = vec![dialog.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = children.pop() {
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            if label.is_selectable() {
                label.select_region(0, 0);
            }
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            children.push(widget);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_routes_menu_actions_and_tracks_autostart_state() {
        assert!(std::hint::black_box(MonitorTray::MENU_ON_ACTIVATE));
        let (commands, receiver) = async_channel::unbounded();
        let mut tray = MonitorTray {
            commands,
            autostart: true,
            keep_running: true,
            start_in_tray: true,
            executable: "/test/sdeck-plus".into(),
            icon: ksni::Icon {
                width: 1,
                height: 1,
                data: vec![255, 0, 0, 0],
            },
        };
        let menu = tray.menu();
        assert_eq!(menu.len(), 7);
        if let MenuItem::Checkmark(item) = &menu[1] {
            assert!(item.checked);
        } else {
            panic!("autostart must be a checkmark");
        }
        if let MenuItem::Checkmark(item) = &menu[2] {
            assert!(item.checked);
            (item.activate)(&mut tray);
            assert!(matches!(
                receiver.try_recv().unwrap(),
                Command::ToggleKeepRunning
            ));
            assert!(
                tray.keep_running,
                "menu waits for disk and GTK acknowledgment"
            );
        } else {
            panic!("close preference must be a checkmark");
        }
        if let MenuItem::Checkmark(item) = &menu[3] {
            assert!(item.checked);
            (item.activate)(&mut tray);
            assert!(matches!(
                receiver.try_recv().unwrap(),
                Command::ToggleStartInTray
            ));
            assert!(tray.start_in_tray, "menu waits for disk acknowledgment");
        } else {
            panic!("start preference must be a checkmark");
        }
        tray.keep_running = false;
        assert!(matches!(&tray.menu()[2], MenuItem::Checkmark(item) if !item.checked));
        for (index, expected) in [(0, "configure"), (4, "about"), (6, "quit")] {
            if let MenuItem::Standard(item) = &menu[index] {
                (item.activate)(&mut tray);
                let command = receiver.try_recv().unwrap();
                assert!(matches!(
                    (command, expected),
                    (Command::Configure, "configure")
                        | (Command::About, "about")
                        | (Command::Quit, "quit")
                ));
            } else {
                panic!("action must be a standard menu item");
            }
        }
    }
}
