//! Configuration transfer entry points; persistence belongs to the runtime worker.
use crate::{feature_runtime::FeatureRuntime, ui};
use gtk::{glib, prelude::*, ResponseType};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

#[derive(Clone)]
pub(crate) struct ConfigurationMenu(Rc<MenuState>);

struct MenuState {
    button: gtk::MenuButton,
    window: glib::WeakRef<gtk::ApplicationWindow>,
    help: gtk::Dialog,
    runtime: RefCell<Option<FeatureRuntime>>,
    chooser: RefCell<Option<gtk::FileChooserNative>>,
    confirmation: RefCell<Option<gtk::Dialog>>,
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
    }
}

impl ConfigurationMenu {
    pub(crate) fn new(window: &gtk::ApplicationWindow, help_body: &gtk::Box) -> Self {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let help = ui::button("help-browser-symbolic", "Help");
        let import = ui::button("document-open-symbolic", "Import configuration...");
        let export = ui::button("document-save-symbolic", "Export configuration...");
        for item in [&help, &import, &export] {
            body.append(item);
        }
        let panel = ui::popover("Menu", &body);
        let button = ui::menu_button("open-menu-symbolic", "Menu", &panel);
        let help_dialog = gtk::Dialog::builder()
            .title("Setup help")
            .transient_for(window)
            .modal(true)
            .destroy_with_parent(true)
            .default_width(480)
            .build();
        help_dialog.add_button("Close", ResponseType::Close);
        help_dialog.set_default_response(ResponseType::Close);
        let content = help_dialog.content_area();
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.append(&ui::panel_body(help_body));
        help_dialog.connect_response(|dialog, _| dialog.hide());
        help_dialog.connect_close_request(|dialog| {
            dialog.hide();
            glib::Propagation::Stop
        });
        let menu = Self(Rc::new(MenuState {
            button,
            window: window.downgrade(),
            help: help_dialog,
            runtime: RefCell::new(None),
            chooser: RefCell::new(None),
            confirmation: RefCell::new(None),
        }));
        let weak = Rc::downgrade(&menu.0);
        help.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                state.button.popdown();
                state.help.present();
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

    pub(crate) fn button(&self) -> &gtk::MenuButton {
        &self.0.button
    }

    pub(crate) fn bind(&self, runtime: FeatureRuntime) {
        self.0.runtime.replace(Some(runtime));
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
        let weak = Rc::downgrade(&self.0);
        chooser.connect_response(move |chooser, response| {
            if let Some(state) = weak.upgrade() {
                let menu = Self(state);
                let current = menu.0.chooser.borrow().as_ref() == Some(chooser);
                if current {
                    menu.0.chooser.borrow_mut().take();
                    if response == ResponseType::Accept {
                        match chooser.file().and_then(|file| file.path()) {
                            Some(path) => menu.selected_path(transfer, path),
                            None => {
                                if let Some(runtime) = menu.0.runtime.borrow().as_ref() {
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

    fn selected_path(&self, transfer: Transfer, path: PathBuf) {
        match transfer {
            Transfer::Import => self.confirm_import(path),
            Transfer::Export => {
                if let Some(runtime) = self.0.runtime.borrow().as_ref() {
                    runtime.export_configuration(path);
                }
            }
        }
    }

    fn confirm_import(&self, path: PathBuf) {
        let Some(window) = self.0.window.upgrade() else {
            return;
        };
        if let Some(dialog) = self.0.confirmation.borrow_mut().take() {
            dialog.destroy();
        }
        let dialog = gtk::Dialog::builder()
            .title("Import configuration?")
            .transient_for(&window)
            .modal(true)
            .destroy_with_parent(true)
            .resizable(false)
            .default_width(440)
            .build();
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog
            .add_button("Import", ResponseType::Accept)
            .add_css_class("destructive-action");
        dialog.set_default_response(ResponseType::Cancel);
        let content = dialog.content_area();
        content.set_spacing(12);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.append(&ui::helper(
            "Importing replaces all profiles, actions, artwork and settings. \
             Your previous configuration will be retained as a backup.",
        ));
        let file = gtk::Label::new(Some(&path.to_string_lossy()));
        file.set_xalign(0.0);
        file.set_wrap(true);
        file.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        file.set_max_width_chars(48);
        file.set_selectable(true);
        content.append(&file);
        let weak = Rc::downgrade(&self.0);
        dialog.connect_response(move |dialog, response| {
            if let Some(state) = weak.upgrade() {
                let current = state.confirmation.borrow().as_ref() == Some(dialog);
                if current {
                    state.confirmation.borrow_mut().take();
                    if response == ResponseType::Accept {
                        if let Some(runtime) = state.runtime.borrow().as_ref() {
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
                if state.confirmation.borrow().as_ref() == Some(dialog) {
                    state.confirmation.borrow_mut().take();
                }
            }
            glib::Propagation::Proceed
        });
        self.0.confirmation.replace(Some(dialog.clone()));
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
        self.selected_path(
            if import {
                Transfer::Import
            } else {
                Transfer::Export
            },
            path,
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
}
