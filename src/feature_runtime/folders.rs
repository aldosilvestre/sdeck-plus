use super::FeatureRuntime;
use gtk::prelude::*;
use sdeck_plus::{features::FeatureAction, layers::Action, organization::NavigationAction};

impl FeatureRuntime {
    pub(super) fn selected_folder(&self) -> Option<String> {
        self.page_controls
            .folder_picker
            .selected_item()
            .and_then(|item| item.downcast::<gtk::StringObject>().ok())
            .map(|item| item.string().to_string())
    }

    pub(super) fn create_folder(&self) {
        let name = self.page_controls.folder_name.text();
        match self.change_organization(|organization, layers| {
            organization.create_folder(layers, &name).map(|_| ())
        }) {
            Ok(()) => {
                self.page_controls.folder_menu.popdown();
                self.report("Folder created.", false);
            }
            Err(message) => self.report(&format!("Could not create folder: {message}"), true),
        }
    }

    pub(super) fn link_folder(&self) {
        let Some(name) = self.selected_folder() else {
            return;
        };
        let result =
            self.ui
                .selected_layer_and_trigger(&self.layers)
                .and_then(|(layer, trigger)| {
                    let action = Action::Feature(FeatureAction::Navigation(
                        NavigationAction::OpenFolder(name),
                    ));
                    let mut layers = self.layers.borrow_mut();
                    layers
                        .bind(&layer, trigger, action)
                        .map_err(|error| error.to_string())?;
                    Ok(layers.clone())
                });
        match result {
            Ok(snapshot) => {
                self.ui.load_selected_binding(&self.layers);
                self.ui.refresh_summaries(&self.layers);
                if self.save_binding(snapshot) {
                    self.report("Folder linked to the selected control; saving…", false);
                }
            }
            Err(message) => self.report(&format!("Could not link folder: {message}"), true),
        }
    }
}
