use super::{action, invalid, LayerStore, NavigationAction, Organization};
use crate::{features::FeatureAction, layers::Action};
use std::io;

impl Organization {
    /// Sorted direct child folders belonging to the selected page.
    pub fn folders(&self, layers: &LayerStore) -> Vec<String> {
        self.views
            .iter()
            .filter(|(key, view)| {
                layers.layers().contains_key(*key)
                    && view.parent_page.as_deref() == Some(layers.active())
            })
            .filter_map(|(_, view)| view.folder.last().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Remove the selected page and its owned descendants.
    ///
    /// # Errors
    /// Base is protected. Errors leave both stores unchanged.
    pub fn remove_page(&mut self, layers: &mut LayerStore) -> io::Result<()> {
        if layers.active() == crate::layers::DEFAULT_LAYER {
            return Err(invalid("Base page is protected"));
        }
        let active = self.active_view(layers);
        let removed = self.descendants([layers.active().to_owned()]);
        let selection = self
            .views
            .iter()
            .filter(|(key, view)| {
                !removed.contains(*key)
                    && layers.layers().contains_key(*key)
                    && view.same_folder(&active)
            })
            .min_by_key(|(_, view)| (view.page.abs_diff(active.page), view.page))
            .map(|(key, _)| key.clone())
            .or_else(|| {
                active
                    .parent_page
                    .clone()
                    .filter(|key| !removed.contains(key) && layers.layers().contains_key(key))
            })
            .or_else(|| {
                self.views
                    .iter()
                    .filter(|(key, view)| {
                        !removed.contains(*key)
                            && layers.layers().contains_key(*key)
                            && view.profile == active.profile
                    })
                    .min_by_key(|(_, view)| (view.folder.len(), view.page))
                    .map(|(key, _)| key.clone())
            })
            .unwrap_or_else(|| crate::layers::DEFAULT_LAYER.to_owned());
        self.remove_views(layers, &removed)?;
        layers.select(&selection);
        Ok(())
    }

    /// Remove only the selected page's named child folder and its descendants.
    ///
    /// # Errors
    /// Invalid or missing local folder names leave both stores unchanged.
    pub fn remove_folder(&mut self, layers: &mut LayerStore, name: &str) -> io::Result<()> {
        action::validate_folder(name)?;
        let children: Vec<_> = self
            .views
            .iter()
            .filter(|(key, view)| {
                layers.layers().contains_key(*key)
                    && view.parent_page.as_deref() == Some(layers.active())
                    && view.folder.last().is_some_and(|folder| folder == name)
            })
            .map(|(key, _)| key.clone())
            .collect();
        if children.is_empty() {
            return Err(invalid("unknown local folder"));
        }
        let owner = layers.active().to_owned();
        let links: Vec<_> = layers.layers().get(&owner)
            .ok_or_else(|| invalid("unknown owning page"))?.bindings.iter()
            .filter(|(_, action)| matches!(action,
                Action::Feature(FeatureAction::Navigation(NavigationAction::OpenFolder(folder))) if folder == name)
            ).map(|(trigger, _)| trigger.clone()).collect();
        let removed = self.descendants(children);
        self.remove_views(layers, &removed)?;
        for trigger in links {
            layers.unbind(&owner, &trigger)?;
        }
        Ok(())
    }

    /// Rename the selected page's direct child folder `old` to `new`.
    ///
    /// Layer keys stay unchanged (like `rename_page`). The folder's pages, the
    /// folder path of every descendant and the owner page's `OpenFolder(old)`
    /// bindings move to the new name. Renaming to the same name is a no-op.
    ///
    /// # Errors
    /// Invalid names, unknown local folders and names already used by another
    /// folder of the selected page leave both stores unchanged.
    pub fn rename_folder(
        &mut self,
        layers: &mut LayerStore,
        old: &str,
        new: &str,
    ) -> io::Result<()> {
        action::validate_folder(new)?;
        let owner = layers.active().to_owned();
        let children: Vec<_> = self
            .views
            .iter()
            .filter(|(key, view)| {
                layers.layers().contains_key(*key)
                    && view.parent_page.as_deref() == Some(owner.as_str())
                    && view.folder.last().is_some_and(|folder| folder == old)
            })
            .map(|(key, _)| key.clone())
            .collect();
        let Some(depth) = children.first().map(|key| self.views[key].folder.len() - 1) else {
            return Err(invalid("unknown local folder"));
        };
        if old == new {
            return Ok(());
        }
        if self.folders(layers).iter().any(|folder| folder == new) {
            return Err(invalid("folder already exists"));
        }
        // Stage both stores so any error leaves them untouched.
        let mut views = self.views.clone();
        for key in self.descendants(children) {
            if let Some(segment) = views
                .get_mut(&key)
                .and_then(|view| view.folder.get_mut(depth))
                .filter(|segment| *segment == old)
            {
                new.clone_into(segment);
            }
        }
        super::storage::validate(&views)?;
        let mut updated = layers.clone();
        let links: Vec<_> = layers.layers().get(&owner)
            .ok_or_else(|| invalid("unknown owning page"))?.bindings.iter()
            .filter(|(_, action)| matches!(action,
                Action::Feature(FeatureAction::Navigation(NavigationAction::OpenFolder(folder))) if folder == old)
            ).map(|(trigger, _)| trigger.clone()).collect();
        for trigger in links {
            let action = Action::Feature(FeatureAction::Navigation(NavigationAction::OpenFolder(
                new.to_owned(),
            )));
            updated.bind_configuration(&owner, trigger, action)?;
        }
        *layers = updated;
        self.views = views;
        Ok(())
    }

    /// Remove all pages and folders belonging to a profile.
    ///
    /// # Errors
    /// Base and unknown profiles are rejected without changing either store.
    pub fn remove_profile(&mut self, layers: &mut LayerStore, profile: &str) -> io::Result<()> {
        if profile == crate::layers::DEFAULT_LAYER {
            return Err(invalid("Base profile is protected"));
        }
        let removed: std::collections::BTreeSet<_> = layers
            .layers()
            .keys()
            .filter(|key| self.profile_for_key(key) == profile)
            .cloned()
            .collect();
        if removed.is_empty() {
            return Err(invalid("unknown profile"));
        }
        self.remove_views(layers, &removed)
    }

    fn descendants(
        &self,
        roots: impl IntoIterator<Item = String>,
    ) -> std::collections::BTreeSet<String> {
        let mut removed: std::collections::BTreeSet<_> = roots.into_iter().collect();
        loop {
            let before = removed.len();
            for (key, view) in &self.views {
                if view
                    .parent_page
                    .as_ref()
                    .is_some_and(|owner| removed.contains(owner))
                {
                    removed.insert(key.clone());
                }
            }
            if removed.len() == before {
                return removed;
            }
        }
    }

    fn remove_views(
        &mut self,
        layers: &mut LayerStore,
        removed: &std::collections::BTreeSet<String>,
    ) -> io::Result<()> {
        if removed.contains(crate::layers::DEFAULT_LAYER) {
            return Err(invalid("Base page is protected"));
        }
        // Stage LayerStore removal so its binding cleanup and any error are atomic.
        let mut updated = layers.clone();
        for key in removed {
            updated.remove(key)?;
        }
        *layers = updated;
        self.sync(layers);
        Ok(())
    }
}
