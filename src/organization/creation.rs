use super::{action, invalid, storage, LayerStore, Organization, View};
use std::io;

impl Organization {
    /// Append and select a blank page in the current profile/folder.
    ///
    /// # Errors
    /// Returns LayerStore's capacity errors or page-number exhaustion.
    /// Errors leave both stores and their selection unchanged.
    pub fn create_page(&mut self, layers: &mut LayerStore) -> io::Result<String> {
        let mut view = self.active_view(layers);
        view.page_name = None;
        view.page = self
            .current(layers)
            .page_count
            .checked_add(1)
            .ok_or_else(|| invalid("page number limit reached"))?;
        self.create_view(layers, view)
    }

    /// Create and select a blank page after validating its display name.
    ///
    /// # Errors
    /// Invalid names and page/layer limits leave both stores unchanged.
    pub fn create_named_page(&mut self, layers: &mut LayerStore, name: &str) -> io::Result<String> {
        if !storage::valid_name(name) {
            return Err(invalid(
                "page name must contain 1-64 bytes and no control characters",
            ));
        }
        let mut view = self.active_view(layers);
        view.page = self
            .current(layers)
            .page_count
            .checked_add(1)
            .ok_or_else(|| invalid("page number limit reached"))?;
        view.page_name = Some(name.trim().to_owned());
        self.create_view(layers, view)
    }

    /// Rename the selected page without changing its layer key or coordinates.
    ///
    /// # Errors
    /// Rejects blank names, control characters and names longer than 64 bytes
    /// before changing metadata. Surrounding spaces are trimmed.
    pub fn rename_page(&mut self, layers: &LayerStore, name: &str) -> io::Result<()> {
        if !storage::valid_name(name) {
            return Err(invalid(
                "page name must contain 1-64 bytes and no control characters",
            ));
        }
        let view = self
            .views
            .entry(layers.active().to_owned())
            .or_insert_with(|| View::root(layers.active()));
        view.page_name = Some(name.trim().to_owned());
        Ok(())
    }

    /// Create and enter a named child folder at page 1.
    ///
    /// # Errors
    /// Returns invalid-name, duplicate-folder, or LayerStore capacity errors.
    /// Folder names are single printable components, not paths.
    pub fn create_folder(&mut self, layers: &mut LayerStore, name: &str) -> io::Result<String> {
        action::validate_folder(name)?;
        let mut view = self.active_view(layers);
        view.parent_page = Some(layers.active().to_owned());
        view.folder.push(name.into());
        view.page = 1;
        view.page_name = None;
        if self.views.iter().any(|(key, candidate)| {
            layers.layers().contains_key(key) && candidate.same_folder(&view)
        }) {
            return Err(invalid("folder already exists"));
        }
        self.create_view(layers, view)
    }

    fn create_view(&mut self, layers: &mut LayerStore, view: View) -> io::Result<String> {
        let label = view.label();
        let mut ordinal = 1u16;
        let key = loop {
            let suffix = if ordinal == 1 {
                String::new()
            } else {
                format!(" ({ordinal})")
            };
            let mut end = 64usize.saturating_sub(suffix.len()).min(label.len());
            while !label.is_char_boundary(end) {
                end = end.saturating_sub(1);
            }
            let candidate = format!("{}{}", label[..end].trim_end(), suffix);
            if !layers.layers().contains_key(&candidate) {
                break candidate;
            }
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| invalid("layer key space exhausted"))?;
        };
        // LayerStore validates all failure conditions before insertion.
        layers.create(&key)?;
        self.sync(layers);
        self.views.insert(key.clone(), view);
        layers.select(&key);
        Ok(key)
    }
}
