//! Configuration transfer entry points; persistence belongs to the runtime worker.
use crate::{feature_runtime::FeatureRuntime, ui, virtual_deck_ui::VirtualDeckPanel};
use gtk::{glib, prelude::*, ResponseType};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

#[derive(Clone)]
pub(crate) struct ConfigurationMenu(Rc<MenuState>);

struct MenuState {
    button: gtk::MenuButton,
    window: glib::WeakRef<gtk::ApplicationWindow>,
    help: gtk::Dialog,
    updates: gtk::Dialog,
    updates_content: gtk::Box,
    runtime: RefCell<Option<FeatureRuntime>>,
    chooser: RefCell<Option<gtk::FileChooserNative>>,
    confirmation: RefCell<Option<gtk::Dialog>>,
    /// The "Virtual Deck" section, once the window gives it one.
    virtual_deck: RefCell<Option<VirtualDeckPanel>>,
}

#[derive(Clone, Copy)]
enum Transfer {
    Import,
    Export,
}

impl Drop for MenuState {
    fn drop(&mut self) {
        if let Some(chooser) = self.chooser.get_mut().take() {
            chooser.destroy();
        }
        if let Some(dialog) = self.confirmation.get_mut().take() {
            dialog.destroy();
        }
        self.help.destroy();
        self.updates.destroy();
    }
}

impl ConfigurationMenu {
    pub(crate) fn new(window: &gtk::ApplicationWindow, help_body: &gtk::Box) -> Self {
        let help = ui::command_row("help-browser-symbolic", "Help");
        let updates = ui::command_row("software-update-available-symbolic", "Updates");
        let import = ui::command_row("document-open-symbolic", "Import configuration...");
        let export = ui::command_row("document-save-symbolic", "Export configuration...");
        // Headerless, like the artboard's menus: rows only, dismissed by Escape or a click outside.
        let panel = ui::command_menu(&[
            help.clone(),
            updates.clone(),
            import.clone(),
            export.clone(),
        ]);
        let button = ui::menu_button("open-menu-symbolic", "Menu", &panel);
        let what = "Menu: help, updates, and importing or exporting the configuration.";
        button.set_tooltip_text(Some(what));
        button.update_property(&[gtk::accessible::Property::Description(what)]);
        let help_dialog = gtk::Dialog::builder()
            .title("Setup help")
            .transient_for(window)
            .modal(true)
            .destroy_with_parent(true)
            .default_width(480)
            .build();
        ui::style_dialog(&help_dialog);
        ui::dismiss_button(&help_dialog, "Close", ResponseType::Close);
        help_dialog.set_default_response(ResponseType::Close);
        let content = help_dialog.content_area();
        let help_scroller = ui::panel_body(help_body);
        help_scroller.set_max_content_height(420);
        content.append(&help_scroller);
        help_dialog.connect_response(|dialog, _| dialog.hide());
        help_dialog.connect_close_request(|dialog| {
            dialog.hide();
            glib::Propagation::Stop
        });
        let updates_dialog = gtk::Dialog::builder()
            .title("Updates")
            .transient_for(window)
            .modal(true)
            .destroy_with_parent(true)
            .default_width(480)
            .build();
        ui::style_dialog(&updates_dialog);
        ui::dismiss_button(&updates_dialog, "Close", ResponseType::Close);
        updates_dialog.set_default_response(ResponseType::Close);
        let updates_content = updates_dialog.content_area();
        updates_dialog.connect_response(|dialog, _| dialog.hide());
        updates_dialog.connect_close_request(|dialog| {
            dialog.hide();
            glib::Propagation::Stop
        });
        let menu = Self(Rc::new(MenuState {
            button,
            window: window.downgrade(),
            help: help_dialog,
            updates: updates_dialog,
            updates_content,
            runtime: RefCell::new(None),
            chooser: RefCell::new(None),
            confirmation: RefCell::new(None),
            virtual_deck: RefCell::new(None),
        }));
        let weak = Rc::downgrade(&menu.0);
        help.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                state.button.popdown();
                state.help.present();
            }
        });
        let weak = Rc::downgrade(&menu.0);
        updates.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                Self(state).present_updates();
            }
        });
        for (item, transfer) in [(import, Transfer::Import), (export, Transfer::Export)] {
            let weak = Rc::downgrade(&menu.0);
            item.connect_clicked(move |_| {
                if let Some(state) = weak.upgrade() {
                    let menu = Self(state);
                    menu.0.button.popdown();
                    if let Some(chooser) = menu.create_chooser(transfer) {
                        chooser.show();
                    }
                }
            });
        }
        menu
    }

    pub(crate) fn add_updates(&self, body: &gtk::Box) {
        self.0.updates_content.append(body);
    }

    /// Put the Virtual Deck's switch and details below Export configuration...,
    /// after a hairline, as in design/virtual-classic/04-all-in-menu.html. Only
    /// the app's window has one; the menu keeps it alive.
    pub(crate) fn attach_virtual_deck(&self, panel: &VirtualDeckPanel) {
        let Some(list) = self
            .0
            .button
            .popover()
            .and_then(|popover| popover.child())
            .and_downcast::<gtk::Box>()
        else {
            return;
        };
        if let Some(previous) = self.0.virtual_deck.replace(Some(panel.clone())) {
            list.remove(previous.section());
            if let Some(separator) = list.last_child() {
                list.remove(&separator);
            }
        }
        list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        list.append(panel.section());
        let what =
            "Menu: help, updates, importing or exporting the configuration, and the Virtual Deck.";
        self.0.button.set_tooltip_text(Some(what));
        self.0
            .button
            .update_property(&[gtk::accessible::Property::Description(what)]);
    }

    /// The virtual deck section, when the menu has one.
    #[cfg(test)]
    pub(crate) fn virtual_deck(&self) -> Option<VirtualDeckPanel> {
        self.0.virtual_deck.borrow().clone()
    }

    pub(crate) fn present_updates(&self) {
        self.0.button.popdown();
        self.0.updates.present();
    }

    pub(crate) fn bind_updates_button(&self, button: &gtk::Button) {
        let weak = Rc::downgrade(&self.0);
        button.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                Self(state).present_updates();
            }
        });
    }

    pub(crate) fn button(&self) -> &gtk::MenuButton {
        &self.0.button
    }

    pub(crate) fn bind(&self, runtime: FeatureRuntime) {
        self.0.runtime.replace(Some(runtime));
    }

    /// The runtime import and export act on.
    #[cfg(test)]
    pub(crate) fn bound_runtime(&self) -> Option<FeatureRuntime> {
        self.0.runtime.borrow().clone()
    }

    fn create_chooser(&self, transfer: Transfer) -> Option<gtk::FileChooserNative> {
        let window = self.0.window.upgrade()?;
        if let Some(chooser) = self.0.chooser.borrow_mut().take() {
            chooser.destroy();
        }
        let (title, action, accept) = match transfer {
            Transfer::Import => ("Import configuration", gtk::FileChooserAction::Open, "Open"),
            Transfer::Export => ("Export configuration", gtk::FileChooserAction::Save, "Save"),
        };
        let chooser = gtk::FileChooserNative::new(
            Some(title),
            Some(&window),
            action,
            Some(accept),
            Some("Cancel"),
        );
        chooser.set_modal(true);
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("SDeck Plus configuration (*.sdeck-plus.json)"));
        filter.add_pattern("*.sdeck-plus.json");
        chooser.add_filter(&filter);
        chooser.set_filter(&filter);
        if let Transfer::Export = transfer {
            chooser.set_current_name("configuration.sdeck-plus.json");
        }
        // The operation belongs to the session shown when the user started it.
        let target = self.0.runtime.borrow().clone();
        let weak = Rc::downgrade(&self.0);
        chooser.connect_response(move |chooser, response| {
            if let Some(state) = weak.upgrade() {
                let menu = Self(state);
                let current = menu.0.chooser.borrow().as_ref() == Some(chooser);
                if current {
                    menu.0.chooser.borrow_mut().take();
                    if response == ResponseType::Accept {
                        match chooser.file().and_then(|file| file.path()) {
                            Some(path) => menu.selected_path_for(transfer, path, target.clone()),
                            None => {
                                if let Some(runtime) = target.as_ref() {
                                    runtime.report("Choose a local configuration file.", true);
                                }
                            }
                        }
                    }
                }
            }
            chooser.destroy();
        });
        self.0.chooser.replace(Some(chooser.clone()));
        Some(chooser)
    }

    fn selected_path_for(&self, transfer: Transfer, path: PathBuf, target: Option<FeatureRuntime>) {
        match transfer {
            Transfer::Import => self.confirm_import(path, target),
            Transfer::Export => {
                if let Some(runtime) = target {
                    runtime.export_configuration(path);
                }
            }
        }
    }

    fn confirm_import(&self, path: PathBuf, target: Option<FeatureRuntime>) {
        let Some(window) = self.0.window.upgrade() else {
            return;
        };
        if let Some(dialog) = self.0.confirmation.borrow_mut().take() {
            dialog.destroy();
        }
        let dialog = gtk::MessageDialog::builder()
            .transient_for(&window)
            .modal(true)
            .destroy_with_parent(true)
            .message_type(gtk::MessageType::Question)
            .text("Import configuration?")
            .secondary_text(
                "Importing replaces profiles, actions, artwork assignments and settings. \
                 Reference-only imports keep local files; missing files must be selected locally. \
                 Your previous configuration will be retained as a backup.",
            )
            .build();
        ui::style_confirm_dialog(&dialog);
        dialog.set_default_size(440, -1);
        ui::dismiss_button(dialog.upcast_ref(), "Cancel", ResponseType::Cancel);
        dialog
            .add_button("Import", ResponseType::Accept)
            .add_css_class("destructive-action");
        dialog.set_default_response(ResponseType::Cancel);
        let file = gtk::Label::new(Some(&path.to_string_lossy()));
        file.set_xalign(0.0);
        file.set_wrap(true);
        file.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        file.set_max_width_chars(48);
        file.set_selectable(true);
        if let Some(area) = dialog.message_area().downcast_ref::<gtk::Box>() {
            area.append(&file);
        }
        let weak = Rc::downgrade(&self.0);
        dialog.connect_response(move |dialog, response| {
            if let Some(state) = weak.upgrade() {
                let current = state.confirmation.borrow().as_ref()
                    == Some(dialog.upcast_ref::<gtk::Dialog>());
                if current {
                    state.confirmation.borrow_mut().take();
                    if response == ResponseType::Accept {
                        if let Some(runtime) = target.as_ref() {
                            runtime.import_configuration(path.clone());
                        }
                    }
                }
            }
            dialog.destroy();
        });
        let weak = Rc::downgrade(&self.0);
        dialog.connect_close_request(move |dialog| {
            if let Some(state) = weak.upgrade() {
                if state.confirmation.borrow().as_ref() == Some(dialog.upcast_ref::<gtk::Dialog>())
                {
                    state.confirmation.borrow_mut().take();
                }
            }
            glib::Propagation::Proceed
        });
        self.0.confirmation.replace(Some(dialog.clone().upcast()));
        dialog.present();
    }

    #[cfg(test)]
    pub(crate) fn chooser_for_test(&self, import: bool) -> Option<gtk::FileChooserNative> {
        self.create_chooser(if import {
            Transfer::Import
        } else {
            Transfer::Export
        })
    }

    #[cfg(test)]
    pub(crate) fn select_path_for_test(&self, import: bool, path: PathBuf) {
        let target = self.0.runtime.borrow().clone();
        self.selected_path_for(
            if import {
                Transfer::Import
            } else {
                Transfer::Export
            },
            path,
            target,
        );
    }

    #[cfg(test)]
    pub(crate) fn confirmation_for_test(&self) -> Option<gtk::Dialog> {
        self.0.confirmation.borrow().clone()
    }

    #[cfg(test)]
    pub(crate) fn help_for_test(&self) -> gtk::Dialog {
        self.0.help.clone()
    }

    #[cfg(test)]
    pub(crate) fn updates_for_test(&self) -> gtk::Dialog {
        self.0.updates.clone()
    }
}
