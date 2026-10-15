use super::*;
use sdeck_plus::local_artwork::ArtworkStore;
use std::{io, path::Path};

pub(super) fn persist(
    original: &LayerStore,
    artwork_root: &Path,
    layer: &str,
    first: u8,
    second: u8,
) -> io::Result<()> {
    if !original.diagnostics().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Repair the profile configuration before swapping keys.",
        ));
    }
    let mut next = original.clone();
    next.swap_keys(layer, first, second)?;
    let mut artwork = ArtworkStore::open_at_for_layer(artwork_root, Some(layer))?;
    next.persist()?;
    if let Err(error) = artwork.swap_keys_in_layer(layer, first, second) {
        original.persist().map_err(|rollback| {
            io::Error::other(format!(
                "Artwork swap failed: {error}. Restoring bindings also failed: {rollback}"
            ))
        })?;
        return Err(error);
    }
    Ok(())
}

impl FeatureRuntime {
    pub(crate) fn attach_key_swap_root(&self, root: &gtk::Widget) {
        *self.key_swap_root.borrow_mut() = root.downgrade();
    }

    pub(crate) fn swap_keys(&self, layer: &str, first: u8, second: u8) -> bool {
        if self.key_swap_pending.get() || first == second || self.layers.borrow().active() != layer
        {
            return false;
        }
        let artwork_root = {
            let artwork = self.artwork.store.borrow();
            let Some(artwork) = artwork.as_ref() else {
                self.report(
                    "Artwork storage is unavailable; keys were not swapped.",
                    true,
                );
                return false;
            };
            artwork.root().to_path_buf()
        };
        let store = self.layers.borrow().clone();
        self.key_swap_pending.set(true);
        if !self.submit(LayerCommand::SwapKeys {
            store,
            artwork_root,
            layer: layer.into(),
            first,
            second,
        }) {
            self.key_swap_pending.set(false);
            return false;
        }
        if let Some(root) = self.key_swap_root.borrow().upgrade() {
            root.set_sensitive(false);
        }
        self.report("Swapping keys…", false);
        true
    }

    pub(crate) fn complete_key_swap(
        &self,
        layer: &str,
        first: u8,
        second: u8,
        result: Result<(), String>,
    ) {
        self.key_swap_pending.set(false);
        if let Some(root) = self.key_swap_root.borrow().upgrade() {
            root.set_sensitive(true);
        }
        let result = result.and_then(|()| {
            let root = self
                .artwork
                .store
                .borrow()
                .as_ref()
                .ok_or("Artwork storage is unavailable")?
                .root()
                .to_path_buf();
            let artwork = ArtworkStore::open_at_for_layer(root, Some(layer))
                .map_err(|error| error.to_string())?;
            self.layers
                .borrow_mut()
                .swap_keys(layer, first, second)
                .map_err(|error| error.to_string())?;
            *self.artwork.store.borrow_mut() = Some(artwork);
            if matches!(self.artwork.selected_target.get(),
                sdeck_plus::local_artwork::ArtworkTarget::Keypad(key) if key == first || key == second)
            {
                if let Some(editor) = self.artwork.text_editor.borrow().upgrade() {
                    editor.load_context(true);
                }
            }
            if self.ui.control_picker.selected() == 0
                && [u32::from(first - 1), u32::from(second - 1)]
                    .contains(&self.ui.control_number_picker.selected())
                && self.layers.borrow().active() == layer
            {
                self.ui.load_selected_binding(&self.layers);
            }
            self.ui.refresh_summaries(&self.layers);
            self.artwork.refresh(true);
            Ok(())
        });
        match result {
            Ok(()) => self.report(&format!("Keys {first} and {second} swapped."), false),
            Err(error) => self.report(&format!("Could not swap keys: {error}"), true),
        }
        self.reconcile_focus();
        if self.key_swap_deferred_persist.replace(false) {
            self.persist(self.layers.borrow().clone());
        }
    }
}
