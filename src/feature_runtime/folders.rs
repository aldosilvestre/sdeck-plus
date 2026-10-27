use super::{folder_panel::FolderCommand, pages::Removal, FeatureRuntime};
use sdeck_plus::{features::FeatureAction, layers::Action, organization::NavigationAction};

impl FeatureRuntime {
    /// Route a "Page folders" popover command; store changes go through
    /// `change_organization` so layers and organization save together.
    pub(super) fn folder_command(&self, command: FolderCommand) -> Result<(), String> {
        match command {
            FolderCommand::Create(name) => self.create_folder(&name),
            FolderCommand::Open(name) => {
                self.page_controls.folder_menu.popdown();
                self.navigate(&NavigationAction::OpenFolder(name));
                Ok(())
            }
            FolderCommand::Link(name) => {
                self.link_folder(name);
                Ok(())
            }
            FolderCommand::Rename { old, new } => {
                let result = self.change_organization(|organization, layers| {
                    organization.rename_folder(layers, &old, &new)
                });
                match &result {
                    Ok(()) => self.report("Folder renamed.", false),
                    Err(message) => {
                        self.report(&format!("Could not rename folder: {message}"), true);
                    }
                }
                result
            }
            FolderCommand::Delete(name) => {
                let page = self.layers.borrow().active().to_owned();
                self.page_controls.folder_menu.popdown();
                self.confirm_removal(Removal::Folder { page, name });
                Ok(())
            }
        }
    }

    /// Create a child folder of the selected page and stay on that page, so
    /// the popover lists the new folder.
    fn create_folder(&self, name: &str) -> Result<(), String> {
        let result = self.change_organization(|organization, layers| {
            let owner = layers.active().to_owned();
            organization.create_folder(layers, name)?;
            layers.select(&owner);
            Ok(())
        });
        match &result {
            Ok(()) => self.report("Folder created.", false),
            Err(message) => self.report(&format!("Could not create folder: {message}"), true),
        }
        result
    }

    fn link_folder(&self, name: String) {
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
