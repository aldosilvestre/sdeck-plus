//! Profile/folder/page metadata over existing layer keys.
//!
//! Bindings and artwork remain owned by their existing layer stores. Call
//! `sync` after externally creating or removing a layer; persist both stores
//! together. LayerStore's active key is the sole source of selection state.

use crate::layers::LayerStore;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::PathBuf};

mod action;
mod creation;
mod navigation;
mod ownership;
mod storage;
pub use action::NavigationAction;

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod folder_link_tests;
#[cfg(test)]
mod folder_rename_tests;
#[cfg(test)]
mod persistence_tests;
#[cfg(test)]
mod tests;

/// The active logical view, suitable for displaying in the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewInfo {
    pub profile: String,
    pub folder: Vec<String>,
    pub page: u16,
    pub page_name: String,
    /// Highest existing page number in this folder (deleted slots stay gaps).
    pub page_count: u16,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct View {
    profile: String,
    folder: Vec<String>,
    page: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    page_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_page: Option<String>,
}

impl View {
    fn root(key: &str) -> Self {
        Self {
            profile: key.into(),
            folder: Vec::new(),
            page: 1,
            page_name: None,
            parent_page: None,
        }
    }

    fn same_folder(&self, other: &Self) -> bool {
        self.profile == other.profile
            && self.folder == other.folder
            && self.parent_page == other.parent_page
    }

    fn label(&self) -> String {
        let mut parts = vec![self.profile.clone()];
        parts.extend(self.folder.iter().cloned());
        parts.push(self.page_name());
        parts.join(" / ")
    }

    fn page_name(&self) -> String {
        self.page_name
            .clone()
            .unwrap_or_else(|| format!("Page {}", self.page))
    }
}

/// Cloneable metadata snapshot; persistence can run off the GTK thread.
#[derive(Clone, Debug)]
pub struct Organization {
    root: Option<PathBuf>,
    views: BTreeMap<String, View>,
    profile_applications: BTreeMap<String, String>,
}

impl Organization {
    /// Read and validate organization.json without modifying it.
    ///
    /// # Errors
    /// Returns read/parse errors or invalid metadata errors. Missing files
    /// initialize legacy flat layers as independent root profiles.
    pub fn open_at(root: impl Into<PathBuf>, layers: &LayerStore) -> io::Result<Self> {
        let root = root.into();
        let metadata = storage::load(&root, layers)?;
        let mut organization = Self {
            root: Some(root),
            views: metadata.views,
            profile_applications: metadata.profile_applications,
        };
        organization.sync(layers);
        storage::validate(&organization.views)?;
        Ok(organization)
    }

    /// Initialize flat root profiles with no disk persistence.
    pub fn in_memory(layers: &LayerStore) -> Self {
        let mut organization = Self {
            root: None,
            views: BTreeMap::new(),
            profile_applications: BTreeMap::new(),
        };
        organization.sync(layers);
        organization
    }

    /// Remove deleted keys and adopt unknown existing layers as root profiles.
    /// Does not remove or rewrite surviving layers or renumber their pages.
    pub fn sync(&mut self, layers: &LayerStore) {
        self.views
            .retain(|key, _| layers.layers().contains_key(key));
        for key in layers.layers().keys() {
            self.views
                .entry(key.clone())
                .or_insert_with(|| View::root(key));
        }
        let profiles = self.profiles();
        self.profile_applications
            .retain(|profile, _| profiles.contains(profile));
    }

    /// Distinct profiles, independent of their backing pages and folders.
    pub fn profiles(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .views
            .values()
            .map(|view| view.profile.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if let Some(base) = names
            .iter()
            .position(|name| name == crate::layers::DEFAULT_LAYER)
        {
            let base = names.remove(base);
            names.insert(0, base);
        }
        names
    }

    pub fn profile_for_key(&self, key: &str) -> String {
        self.views
            .get(key)
            .map_or_else(|| key.to_owned(), |view| view.profile.clone())
    }

    /// Prefer the root first page, otherwise the first surviving coordinate.
    pub fn profile_key(&self, profile: &str) -> Option<String> {
        self.views
            .iter()
            .filter(|(_, view)| view.profile == profile)
            .min_by(|(_, left), (_, right)| {
                (&left.folder, left.page).cmp(&(&right.folder, right.page))
            })
            .map(|(key, _)| key.clone())
    }

    pub fn profile_application(&self, profile: &str) -> Option<&str> {
        self.profile_applications.get(profile).map(String::as_str)
    }

    pub fn has_application_profiles(&self) -> bool {
        !self.profile_applications.is_empty()
    }

    /// Set or clear a profile's optional focused application identity.
    ///
    /// # Errors
    /// Rejects unknown profiles, Base requirements, control characters and
    /// application identities longer than 128 bytes.
    pub fn set_profile_application(&mut self, profile: &str, application: &str) -> io::Result<()> {
        if !self.profiles().iter().any(|name| name == profile) {
            return Err(invalid("unknown profile"));
        }
        if application.chars().any(char::is_control) || application.trim().len() > 128 {
            return Err(invalid(
                "application identity must contain at most 128 printable bytes",
            ));
        }
        let application = application.trim();
        if application.is_empty() {
            self.profile_applications.remove(profile);
        } else {
            if profile == crate::layers::DEFAULT_LAYER {
                return Err(invalid(
                    "Base is always available and cannot require an application",
                ));
            }
            self.profile_applications
                .insert(profile.to_owned(), application.to_owned());
        }
        Ok(())
    }

    pub fn profile_matches(
        &self,
        profile: &str,
        focus: Option<&crate::app_focus::FocusedApplication>,
    ) -> bool {
        let (Some(required), Some(focus)) = (self.profile_application(profile), focus) else {
            return false;
        };
        required.eq_ignore_ascii_case(&focus.window_class)
            || focus
                .executable
                .as_deref()
                .is_some_and(|executable| required.eq_ignore_ascii_case(executable))
    }

    pub fn available_profiles(
        &self,
        focus: Option<&crate::app_focus::FocusedApplication>,
    ) -> Vec<String> {
        self.profiles()
            .into_iter()
            .filter(|profile| {
                self.profile_application(profile).is_none() || self.profile_matches(profile, focus)
            })
            .collect()
    }

    fn active_view(&self, layers: &LayerStore) -> View {
        self.views
            .get(layers.active())
            .cloned()
            .unwrap_or_else(|| View::root(layers.active()))
    }

    /// Describe selection, including an externally added layer before sync.
    pub fn current(&self, layers: &LayerStore) -> ViewInfo {
        let view = self.active_view(layers);
        let page_count = self
            .views
            .iter()
            .filter(|(key, candidate)| {
                layers.layers().contains_key(*key) && candidate.same_folder(&view)
            })
            .map(|(_, candidate)| candidate.page)
            .max()
            .unwrap_or(view.page);
        ViewInfo {
            label: view.label(),
            page_name: view.page_name(),
            profile: view.profile,
            folder: view.folder,
            page: view.page,
            page_count,
        }
    }

    /// Atomically replace organization.json; in-memory instances are a no-op.
    ///
    /// # Errors
    /// Returns serialization or filesystem errors without truncating the
    /// previous organization.json.
    pub fn persist(&self) -> io::Result<()> {
        match &self.root {
            Some(root) => storage::persist(root, &self.views, &self.profile_applications),
            None => Ok(()),
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
