use super::{LayerStore, NavigationAction, Organization};
use std::io;

impl Organization {
    /// Select an existing view; return whether the active layer changed.
    ///
    /// Next/previous skip deleted slots and never wrap. Missing pages/folders
    /// and root's parent return false. Parent and child entry target page 1;
    /// if page 1 was deleted, entry returns false rather than crossing scope.
    ///
    /// # Errors
    /// Returns invalid-action errors before changing either store.
    pub fn navigate(
        &mut self,
        layers: &mut LayerStore,
        action: &NavigationAction,
    ) -> io::Result<bool> {
        action.validate()?;
        let mut target = self.active_view(layers);
        let candidates: Vec<_> = self
            .views
            .iter()
            .filter(|(key, _)| layers.layers().contains_key(*key))
            .collect();
        match action {
            NavigationAction::NextPage => {
                let next = candidates
                    .iter()
                    .filter(|(_, view)| view.same_folder(&target) && view.page > target.page)
                    .map(|(_, view)| view.page)
                    .min();
                let Some(page) = next else {
                    return Ok(false);
                };
                target.page = page;
            }
            NavigationAction::PreviousPage => {
                let previous = candidates
                    .iter()
                    .filter(|(_, view)| view.same_folder(&target) && view.page < target.page)
                    .map(|(_, view)| view.page)
                    .max();
                let Some(page) = previous else {
                    return Ok(false);
                };
                target.page = page;
            }
            NavigationAction::GoToPage(page) => target.page = *page,
            NavigationAction::OpenFolder(name) => {
                target.folder.push(name.clone());
                target.page = 1;
            }
            NavigationAction::ParentFolder => {
                if target.folder.pop().is_none() {
                    return Ok(false);
                }
                target.page = 1;
            }
        }
        let key = candidates
            .into_iter()
            .find(|(_, view)| view.same_folder(&target) && view.page == target.page)
            .map(|(key, _)| key.clone());
        // Unknown externally added layers still represent their own root.
        let key = key.or_else(|| {
            let active = self.active_view(layers);
            (target == active).then(|| layers.active().to_owned())
        });
        match key {
            Some(key) if key != layers.active() => Ok(layers.select(&key)),
            Some(_) | None => Ok(false),
        }
    }
}
