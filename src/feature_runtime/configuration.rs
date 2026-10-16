use super::*;
use sdeck_plus::local_artwork::ArtworkStore;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Import,
    Export,
}

impl FeatureRuntime {
    pub(crate) fn attach_configuration_root(&self, root: &gtk::Widget) {
        *self.configuration_root.borrow_mut() = root.downgrade();
    }

    fn begin_configuration(&self, operation: Operation) -> bool {
        if self.configuration_operation.get().is_some() || self.key_swap_pending.get() {
            self.report(
                "Wait for the current configuration operation to finish.",
                true,
            );
            return false;
        }
        self.configuration_operation.set(Some(operation));
        if operation == Operation::Import {
            self.cancel_timer();
        }
        self.artwork.chooser_owners.borrow_mut().clear();
        if let Some(root) = self.configuration_root.borrow().upgrade() {
            root.set_sensitive(false);
        }
        true
    }

    fn finish_configuration(&self) {
        self.configuration_operation.set(None);
        if let Some(root) = self.configuration_root.borrow().upgrade() {
            root.set_sensitive(true);
        }
    }

    pub(crate) fn export_configuration(&self, destination: PathBuf) {
        if !self.begin_configuration(Operation::Export) {
            return;
        }
        let store = self.layers.borrow().clone();
        let organization = self.organization.borrow().clone();
        if !self.submit(LayerCommand::ExportConfiguration {
            store,
            organization,
            destination,
        }) {
            self.finish_configuration();
        } else {
            self.report("Exporting configuration...", false);
        }
    }

    pub(crate) fn import_configuration(&self, source: PathBuf) {
        if !self.begin_configuration(Operation::Import) {
            return;
        }
        let root = self.layers.borrow().root().to_path_buf();
        if !self.submit(LayerCommand::ImportConfiguration { root, source }) {
            self.finish_configuration();
        } else {
            self.report("Importing configuration...", false);
        }
    }

    pub(crate) fn complete_configuration_export(&self, path: &Path, result: Result<(), String>) {
        self.finish_configuration();
        match result {
            Ok(()) => self.report(
                &format!("Configuration exported to {}.", path.display()),
                false,
            ),
            Err(error) => self.report(&format!("Could not export configuration: {error}"), true),
        }
        self.reconcile_focus();
    }

    pub(crate) fn complete_configuration_import(
        &self,
        root: &Path,
        result: Result<Option<PathBuf>, String>,
    ) -> bool {
        let backup = match result {
            Ok(backup) => backup,
            Err(error) => {
                self.finish_configuration();
                self.report(&format!("Could not import configuration: {error}"), true);
                return false;
            }
        };
        if let Err(error) = self.reload_configuration(root) {
            // Disk installation succeeded. Keep writes blocked rather than let
            // stale in-memory state overwrite the imported configuration.
            self.report(&format!(
                "Configuration imported, but reload failed: {error}. Restart the application before editing."
            ), true);
            return false;
        }
        self.finish_configuration();
        let message = match backup {
            Some(path) => format!(
                "Configuration imported. Previous configuration: {}.",
                path.display()
            ),
            None => "Configuration imported.".into(),
        };
        self.report(&message, false);
        true
    }

    fn reload_configuration(&self, root: &Path) -> std::io::Result<()> {
        let layers = LayerStore::open_at(root)?;
        if !layers.diagnostics().is_empty() || !layers.conflicts().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "imported bindings contain errors",
            ));
        }
        let organization = Organization::open_at(root, &layers)?;
        let artwork = ArtworkStore::open_at_for_layer(root, Some(layers.active()))?;
        let obs = ObsConfig::load_at(root)?;
        *self.layers.borrow_mut() = layers;
        *self.organization.borrow_mut() = Some(organization);
        *self.artwork.store.borrow_mut() = Some(artwork);
        *self.artwork.last_layer.borrow_mut() = None;
        self.artwork.info.borrow_mut().clear();
        self.profile_memory.borrow_mut().clear();
        self.manual_profile.set(true);
        self.page_controls.root.set_sensitive(true);
        self.page_controls.folders.set_sensitive(true);
        self.organization_management.set_sensitive(true);
        self.obs_valid.set(true);
        self.obs_save.set_sensitive(true);
        self.obs_test.set_sensitive(true);
        self.endpoint.set_text(&obs.endpoint);
        self.password.set_text(&obs.password);
        self.sync_info_from(&self.layers.borrow());
        self.refresh_selection();
        if let Some(editor) = self.artwork.text_editor.borrow().upgrade() {
            editor.load_context(true);
        }
        Ok(())
    }
}
