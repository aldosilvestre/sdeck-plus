use super::FeatureRuntime;
use gtk::{prelude::*, Entry, Label, ResponseType};
use sdeck_plus::{layers::LayerStore, organization::Organization};
use std::io;

#[derive(Clone, Copy)]
pub(super) enum PageEdit {
    Create,
    Rename,
}

pub(super) enum Removal {
    Page(String),
    Folder { page: String, name: String },
    Profile(String),
}

impl FeatureRuntime {
    pub(super) fn change_organization(
        &self,
        edit: impl FnOnce(&mut Organization, &mut LayerStore) -> io::Result<()>,
    ) -> Result<(), String> {
        let mut organization = self
            .organization
            .borrow()
            .clone()
            .ok_or("Organization writes are disabled.")?;
        let mut layers = self.layers.borrow().clone();
        edit(&mut organization, &mut layers).map_err(|error| error.to_string())?;
        layers
            .sync_touchscreen_profiles(&organization)
            .map_err(|error| error.to_string())?;
        let removed: Vec<_> = self
            .layers
            .borrow()
            .configurations()
            .filter(|configuration| !layers.has_configuration_owner(&configuration.name))
            .map(|configuration| configuration.name.clone())
            .collect();
        if !removed.is_empty() {
            self.artwork.remove_layers(&removed)?;
        }
        self.cancel_key_holds();
        *self.layers.borrow_mut() = layers;
        *self.organization.borrow_mut() = Some(organization);
        self.manual_profile.set(true);
        self.refresh_selection();
        self.persist(self.layers.borrow().clone());
        Ok(())
    }

    pub(super) fn edit_page(&self, mode: PageEdit) {
        let Some(parent) = self
            .profile_tabs
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        else {
            return;
        };
        let key = self.layers.borrow().active().to_owned();
        let Some(info) = self
            .organization
            .borrow()
            .as_ref()
            .map(|organization| organization.current(&self.layers.borrow()))
        else {
            return;
        };
        let (title, action, icon, name) = match mode {
            PageEdit::Create => (
                "New page",
                "Create page",
                "document-new-symbolic",
                String::new(),
            ),
            PageEdit::Rename => (
                "Rename page",
                "Save name",
                "document-edit-symbolic",
                info.page_name,
            ),
        };
        let dialog = gtk::Dialog::builder()
            .title(title)
            .transient_for(&parent)
            .modal(true)
            .destroy_with_parent(true)
            .resizable(false)
            .default_width(360)
            .build();
        dialog.add_css_class("page-name-dialog");
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog
            .add_button(action, ResponseType::Accept)
            .add_css_class("suggested-action");
        dialog.set_default_response(ResponseType::Accept);
        let content = dialog.content_area();
        content.set_spacing(16);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.append(&crate::ui::section_heading(icon, title));
        let entry = Entry::builder()
            .text(&name)
            .placeholder_text("Page name")
            .hexpand(true)
            .build();
        content.append(&crate::labeled("Page name", &entry));
        let error = Label::new(None);
        error.set_xalign(0.0);
        error.set_wrap(true);
        error.add_css_class("feedback-error");
        error.set_visible(false);
        content.append(&error);
        let callback = self.clone();
        let input = entry.clone();
        dialog.connect_response(move |dialog, response| {
            if response != ResponseType::Accept {
                dialog.close();
                return;
            }
            let result = callback.change_organization(|organization, layers| {
                if !layers.select(&key) {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "The page is no longer available.",
                    ));
                }
                match mode {
                    PageEdit::Create => organization
                        .create_named_page(layers, &input.text())
                        .map(|_| ()),
                    PageEdit::Rename => organization.rename_page(layers, &input.text()),
                }
            });
            match result {
                Ok(()) => {
                    callback.report(
                        match mode {
                            PageEdit::Create => "Page created.",
                            PageEdit::Rename => "Page renamed.",
                        },
                        false,
                    );
                    dialog.close();
                }
                Err(message) => {
                    error.set_text(&message);
                    error.set_visible(true);
                }
            }
        });
        let dialog_ref = dialog.downgrade();
        entry.connect_activate(move |_| {
            if let Some(dialog) = dialog_ref.upgrade() {
                dialog.response(ResponseType::Accept);
            }
        });
        dialog.present();
        entry.grab_focus();
        entry.select_region(0, -1);
    }

    pub(crate) fn confirm_profile_removal(&self) {
        let Some(profile) = self
            .profile_settings_picker
            .selected_item()
            .and_then(|item| item.downcast::<gtk::StringObject>().ok())
            .map(|item| item.string().to_string())
        else {
            return;
        };
        self.confirm_removal(Removal::Profile(profile));
    }

    /// The profile the rail currently shows, when organization is available.
    pub(crate) fn current_profile(&self) -> Option<String> {
        self.organization
            .borrow()
            .as_ref()
            .map(|organization| organization.current(&self.layers.borrow()).profile)
    }

    /// Ask before deleting the profile shown now (Base is never offered).
    pub(crate) fn confirm_current_profile_removal(&self) {
        if let Some(profile) = self.current_profile() {
            if profile != sdeck_plus::layers::DEFAULT_LAYER {
                self.confirm_removal(Removal::Profile(profile));
            }
        }
    }

    /// What goes with a profile, counted from the pages and bindings it owns.
    fn profile_removal_detail(&self, profile: &str) -> String {
        let layers = self.layers.borrow();
        let organization = self.organization.borrow();
        let (pages, bindings) = layers
            .layers()
            .iter()
            .filter(|(key, _)| {
                organization
                    .as_ref()
                    .is_some_and(|organization| organization.profile_for_key(key) == profile)
            })
            .fold((0, 0), |(pages, bindings), (_, layer)| {
                (pages + 1, bindings + layer.bindings.len())
            });
        if pages == 0 {
            return "Everything in it goes with it. This cannot be undone.".to_owned();
        }
        let plural = |count: usize, noun: &str| {
            format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
        };
        format!(
            "{} and {} go with it. This cannot be undone.",
            plural(pages, "page"),
            plural(bindings, "binding")
        )
    }

    pub(super) fn confirm_removal(&self, target: Removal) {
        let Some(parent) = self
            .profile_tabs
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        else {
            return;
        };
        let (name, detail) = match &target {
            Removal::Page(_) => {
                let Some(name) = self
                    .organization
                    .borrow()
                    .as_ref()
                    .map(|organization| organization.current(&self.layers.borrow()).page_name)
                else {
                    return;
                };
                (
                    name,
                    "Its folders, bindings and artwork go with it. This cannot be undone."
                        .to_owned(),
                )
            }
            Removal::Folder { name, .. } => (
                name.clone(),
                "Its contents go with it. Other pages' folders are kept. This cannot be undone."
                    .to_owned(),
            ),
            Removal::Profile(name) => (name.clone(), self.profile_removal_detail(name)),
        };
        let dialog = crate::ui::delete_confirmation(&parent, &name, &detail);
        let callback = self.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                let result = callback.change_organization(|organization, layers| match &target {
                    Removal::Page(key) => {
                        if !layers.select(key) {
                            return Err(io::Error::new(
                                io::ErrorKind::NotFound,
                                "The page is no longer available.",
                            ));
                        }
                        organization.remove_page(layers)
                    }
                    Removal::Folder { page, name } => {
                        if !layers.select(page) {
                            return Err(io::Error::new(
                                io::ErrorKind::NotFound,
                                "The owning page is no longer available.",
                            ));
                        }
                        organization.remove_folder(layers, name)
                    }
                    Removal::Profile(name) => organization.remove_profile(layers, name),
                });
                match result {
                    Ok(()) => callback.report("Deleted.", false),
                    Err(message) => callback.report(&format!("Could not delete: {message}"), true),
                }
            }
            dialog.close();
        });
        dialog.present();
    }
}
