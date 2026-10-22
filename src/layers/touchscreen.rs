//! Profile-owned touch pages. Legacy layer records remain recoverable after migration.
use super::*;
use crate::{
    local_artwork::{ArtworkStore, ArtworkTarget},
    organization::Organization,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const FILE: &str = "touchscreen.json";
const MAX_PAGES: usize = 64;
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct Touchscreens {
    profiles: BTreeMap<String, Profile>,
    layer_profiles: BTreeMap<String, String>,
    pub(super) invalid: bool,
}
#[derive(Clone, Debug)]
struct Profile {
    selected: usize,
    pages: Vec<Layer>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    profiles: Vec<StoredProfile>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredProfile {
    name: String,
    selected: usize,
    pages: Vec<StoredPage>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPage {
    owner: String,
    bindings: Vec<StoredBinding>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredBinding {
    trigger: String,
    action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    page_return: Option<(String, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    information: Option<String>,
}

impl Touchscreens {
    pub(super) fn load(root: &Path, layers: &BTreeMap<String, Layer>) -> io::Result<Self> {
        let path = root.join(FILE);
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        if metadata.len() > MAX_BYTES {
            return Err(invalid("touchscreen configuration is too large"));
        }
        let document: Document =
            serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)?;
        if document.version != 1 || document.profiles.len() > MAX_LAYERS {
            return Err(invalid(
                "unsupported or oversized touchscreen configuration",
            ));
        }
        let mut result = Self::default();
        let mut owners = BTreeSet::new();
        for profile in document.profiles {
            validate_name(&profile.name)?;
            if profile.pages.is_empty()
                || profile.pages.len() > MAX_PAGES
                || profile.selected >= profile.pages.len()
            {
                return Err(invalid("invalid touchscreen page selection or count"));
            }
            let mut pages = Vec::new();
            for page in profile.pages {
                validate_name(&page.owner)?;
                if !page.owner.starts_with("@touchscreen:")
                    || layers.contains_key(&page.owner)
                    || !owners.insert(page.owner.clone())
                {
                    return Err(invalid("invalid or duplicate touchscreen owner"));
                }
                let mut data = empty_page(page.owner);
                for binding in page.bindings {
                    let trigger =
                        decode_trigger(&binding.trigger).map_err(|error| invalid(&error))?;
                    let action = decode_action(&binding.action).map_err(|error| invalid(&error))?;
                    if !trigger.is_touchscreen()
                        || data
                            .bindings
                            .insert(trigger.clone(), action.clone())
                            .is_some()
                    {
                        return Err(invalid("invalid or duplicate touchscreen binding"));
                    }
                    if let Action::SwitchLayer(target) = action {
                        if !layers.contains_key(&target) {
                            return Err(invalid("unknown touchscreen target layer"));
                        }
                    }
                    if let Some((layer, seconds)) = binding.page_return {
                        let value = PageReturn { layer, seconds };
                        value.validate()?;
                        if !layers.contains_key(&value.layer) {
                            return Err(invalid("unknown touchscreen return page"));
                        }
                        data.returns.insert(trigger.clone(), value);
                    }
                    if let Some(source) = binding.information {
                        if !shows_information(&trigger) {
                            return Err(invalid("invalid touchscreen information trigger"));
                        }
                        let source = crate::info::InfoSource::from_id(&source)
                            .ok_or_else(|| invalid("unknown touchscreen information source"))?;
                        data.displays.insert(trigger, source);
                    }
                }
                pages.push(data);
            }
            if result
                .profiles
                .insert(
                    profile.name,
                    Profile {
                        selected: profile.selected,
                        pages,
                    },
                )
                .is_some()
            {
                return Err(invalid("duplicate touchscreen profile"));
            }
        }
        Ok(result)
    }

    pub(super) fn persist(&self, root: &Path) -> io::Result<()> {
        if self.invalid {
            return Err(invalid("repair touchscreen configuration before saving"));
        }
        if self.profiles.is_empty() {
            return Ok(());
        }
        let profiles = self
            .profiles
            .iter()
            .map(|(name, profile)| {
                let pages = profile
                    .pages
                    .iter()
                    .map(|page| {
                        let bindings = page
                            .bindings
                            .iter()
                            .map(|(trigger, action)| {
                                Ok(StoredBinding {
                                    trigger: encode_trigger(trigger),
                                    action: encode_action(action)?,
                                    page_return: page
                                        .returns
                                        .get(trigger)
                                        .map(|value| (value.layer.clone(), value.seconds)),
                                    information: page
                                        .displays
                                        .get(trigger)
                                        .map(|source| source.id().to_owned()),
                                })
                            })
                            .collect::<io::Result<Vec<_>>>()?;
                        Ok(StoredPage {
                            owner: page.name.clone(),
                            bindings,
                        })
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                Ok(StoredProfile {
                    name: name.clone(),
                    selected: profile.selected,
                    pages,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let bytes = serde_json::to_vec_pretty(&Document {
            version: 1,
            profiles,
        })
        .map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("touchscreen configuration is too large"));
        }
        let temp = root.join("touchscreen.json.tmp");
        use std::io::Write;
        let mut file = fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(temp, root.join(FILE))
    }

    pub(super) fn page(&self, layer: &str) -> Option<&Layer> {
        if let Some(profile) = self
            .layer_profiles
            .get(layer)
            .and_then(|name| self.profiles.get(name))
        {
            return profile.pages.get(profile.selected);
        }
        self.profiles
            .values()
            .flat_map(|profile| &profile.pages)
            .find(|page| page.name == layer)
    }
    pub(super) fn page_mut(&mut self, layer: &str) -> Option<&mut Layer> {
        if let Some(name) = self.layer_profiles.get(layer) {
            let profile = self.profiles.get_mut(name)?;
            return profile.pages.get_mut(profile.selected);
        }
        self.profiles
            .values_mut()
            .flat_map(|profile| &mut profile.pages)
            .find(|page| page.name == layer)
    }
    pub(super) fn all_pages(&self) -> impl Iterator<Item = &Layer> {
        self.profiles.values().flat_map(|profile| &profile.pages)
    }
    pub(super) fn remove_references(&mut self, name: &str) {
        for page in self
            .profiles
            .values_mut()
            .flat_map(|profile| &mut profile.pages)
        {
            page.bindings.retain(
                |_, action| !matches!(action, Action::SwitchLayer(target) if target == name),
            );
            page.returns.retain(|trigger, value| {
                value.layer != name && page.bindings.contains_key(trigger)
            });
            page.displays
                .retain(|trigger, _| page.bindings.contains_key(trigger));
        }
        self.layer_profiles.remove(name);
    }
}

fn empty_page(name: String) -> Layer {
    Layer {
        name,
        bindings: BTreeMap::new(),
        returns: BTreeMap::new(),
        displays: BTreeMap::new(),
    }
}
impl Trigger {
    pub fn is_touchscreen(&self) -> bool {
        matches!(
            self,
            Self::TouchTap
                | Self::TouchPress
                | Self::TouchFlick
                | Self::TouchSwipe(_)
                | Self::ZoneTap(_)
                | Self::ZonePress(_)
        )
    }
}
impl LayerStore {
    /// Establish ownership and migrate each previously unseen profile once.
    /// Artwork references are committed before the migration marker; legacy
    /// assignments and layer records are never removed by this operation.
    pub fn initialize_touchscreens(
        &mut self,
        organization: &Organization,
        artwork: &mut ArtworkStore,
    ) -> io::Result<()> {
        if self.touchscreens.invalid
            || !self.diagnostics.is_empty()
            || !artwork.diagnostics().is_empty()
        {
            return Err(invalid(
                "valid layers and artwork are required for touchscreen migration",
            ));
        }
        let mut next = self.touchscreens.clone();
        next.layer_profiles = self
            .layers
            .keys()
            .map(|key| (key.clone(), organization.profile_for_key(key)))
            .collect();
        let mut copies = Vec::new();
        for name in organization.profiles() {
            if next.profiles.contains_key(&name) {
                continue;
            }
            let mut keys = self
                .layers
                .keys()
                .filter(|key| organization.profile_for_key(key) == name)
                .cloned()
                .collect::<Vec<_>>();
            if let Some(root) = organization.profile_key(&name) {
                keys.sort_by_key(|key| (key != &root, key.clone()));
            }
            let mut pages: Vec<Layer> = Vec::new();
            let mut signatures = Vec::new();
            let mut selected = 0;
            for key in keys {
                let legacy = &self.layers[&key];
                let bindings = legacy
                    .bindings
                    .iter()
                    .filter(|(trigger, _)| trigger.is_touchscreen())
                    .map(|(trigger, action)| (trigger.clone(), action.clone()))
                    .collect();
                let returns = legacy
                    .returns
                    .iter()
                    .filter(|(trigger, _)| trigger.is_touchscreen())
                    .map(|(trigger, value)| (trigger.clone(), value.clone()))
                    .collect();
                let displays = legacy
                    .displays
                    .iter()
                    .filter(|(trigger, _)| trigger.is_touchscreen())
                    .map(|(trigger, source)| (trigger.clone(), *source))
                    .collect();
                let mut page = Layer {
                    name: String::new(),
                    bindings,
                    returns,
                    displays,
                };
                let images = ArtworkTarget::ALL
                    .into_iter()
                    .filter(|target| !matches!(target, ArtworkTarget::Keypad(_)))
                    .map(|target| artwork.assigned_path_in_layer(&key, target))
                    .collect::<Vec<_>>();
                let existing = pages
                    .iter()
                    .zip(&signatures)
                    .position(|(other, signature)| {
                        page.bindings == other.bindings
                            && page.returns == other.returns
                            && page.displays == other.displays
                            && &images == signature
                    });
                let index = if let Some(index) = existing {
                    index
                } else {
                    page.name = next_owner(&next, &pages, &self.layers)?;
                    copies.push((key.clone(), page.name.clone()));
                    signatures.push(images);
                    pages.push(page);
                    pages.len() - 1
                };
                if key == self.active {
                    selected = index;
                }
            }
            next.profiles.insert(name, Profile { selected, pages });
        }
        // Copy all references in one atomic artwork commit, with originals retained.
        artwork.copy_touchscreen_assignments(&copies)?;
        next.persist(&self.root)?;
        self.touchscreens = next;
        Ok(())
    }

    /// Update ownership after keypad organization edits without disk I/O.
    /// New profiles start blank; new keypad pages never remigrate touch data.
    pub fn sync_touchscreen_profiles(&mut self, organization: &Organization) -> io::Result<()> {
        if self.touchscreens.profiles.is_empty() {
            return Ok(());
        }
        let profiles = organization.profiles();
        self.touchscreens
            .profiles
            .retain(|name, _| profiles.contains(name));
        for name in profiles {
            if !self.touchscreens.profiles.contains_key(&name) {
                let owner = next_owner(&self.touchscreens, &[], &self.layers)?;
                self.touchscreens.profiles.insert(
                    name,
                    Profile {
                        selected: 0,
                        pages: vec![empty_page(owner)],
                    },
                );
            }
        }
        self.touchscreens.layer_profiles = self
            .layers
            .keys()
            .map(|key| (key.clone(), organization.profile_for_key(key)))
            .collect();
        Ok(())
    }

    pub fn touchscreen_owner(&self, layer: &str) -> Option<&str> {
        self.touchscreens.page(layer).map(|page| page.name.as_str())
    }
    pub fn touchscreen_pages(&self, profile: &str) -> Vec<String> {
        self.touchscreens
            .profiles
            .get(profile)
            .map(|profile| profile.pages.iter().map(|page| page.name.clone()).collect())
            .unwrap_or_default()
    }
    pub fn create_touchscreen_page(&mut self, profile: &str) -> io::Result<String> {
        let data = self
            .touchscreens
            .profiles
            .get(profile)
            .ok_or_else(|| invalid("unknown touchscreen profile"))?;
        if self.touchscreens.invalid || data.pages.len() >= MAX_PAGES {
            return Err(invalid("touchscreen page limit reached"));
        }
        let owner = next_owner(&self.touchscreens, &[], &self.layers)?;
        let data = self
            .touchscreens
            .profiles
            .get_mut(profile)
            .expect("checked");
        data.pages.push(empty_page(owner.clone()));
        data.selected = data.pages.len() - 1;
        Ok(owner)
    }
    pub fn select_touchscreen_page(&mut self, profile: &str, owner: &str) -> bool {
        let Some(profile) = self.touchscreens.profiles.get_mut(profile) else {
            return false;
        };
        let Some(index) = profile.pages.iter().position(|page| page.name == owner) else {
            return false;
        };
        profile.selected = index;
        true
    }
    /// Match keypad navigation: no wrap at either boundary.
    pub fn step_touchscreen_page(&mut self, profile: &str, next: bool) -> bool {
        let Some(profile) = self.touchscreens.profiles.get_mut(profile) else {
            return false;
        };
        let index = if next {
            profile.selected.checked_add(1)
        } else {
            profile.selected.checked_sub(1)
        };
        let Some(index) = index.filter(|index| *index < profile.pages.len()) else {
            return false;
        };
        profile.selected = index;
        true
    }
    pub fn binding_layer(&self, layer: &str, trigger: &Trigger) -> Option<&Layer> {
        if trigger.is_touchscreen() {
            if let Some(page) = self.touchscreens.page(layer) {
                return Some(page);
            }
        }
        self.layers.get(layer)
    }
    pub(super) fn binding_layer_mut(
        &mut self,
        layer: &str,
        trigger: &Trigger,
    ) -> Option<&mut Layer> {
        if trigger.is_touchscreen() && self.touchscreens.page(layer).is_some() {
            return self.touchscreens.page_mut(layer);
        }
        self.layers.get_mut(layer)
    }
    pub fn binding(&self, layer: &str, trigger: &Trigger) -> Option<Action> {
        self.binding_layer(layer, trigger)?
            .bindings
            .get(trigger)
            .cloned()
    }
    /// Iterate all persistent configurations, including unselected touch pages.
    pub fn configurations(&self) -> impl Iterator<Item = &Layer> {
        self.layers.values().chain(self.touchscreens.all_pages())
    }
    pub fn has_configuration_owner(&self, owner: &str) -> bool {
        self.layers.contains_key(owner)
            || self.touchscreens.all_pages().any(|page| page.name == owner)
    }
    pub fn artwork_owner<'a>(&'a self, layer: &'a str, target: ArtworkTarget) -> &'a str {
        if matches!(target, ArtworkTarget::Keypad(_)) {
            layer
        } else {
            self.touchscreen_owner(layer).unwrap_or(layer)
        }
    }
}

fn next_owner(
    touchscreens: &Touchscreens,
    pending: &[Layer],
    layers: &BTreeMap<String, Layer>,
) -> io::Result<String> {
    (1..=MAX_LAYERS * MAX_PAGES + MAX_LAYERS)
        .map(|number| format!("@touchscreen:{number}"))
        .find(|name| {
            !layers.contains_key(name)
                && !touchscreens.all_pages().any(|page| &page.name == name)
                && !pending.iter().any(|page| &page.name == name)
        })
        .ok_or_else(|| invalid("touchscreen owner space exhausted"))
}
