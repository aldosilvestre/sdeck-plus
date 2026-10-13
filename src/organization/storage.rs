use super::{action::validate_folder, View};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Metadata {
    version: u16,
    #[serde(deserialize_with = "unique_map")]
    pub(super) views: BTreeMap<String, View>,
    #[serde(
        default,
        deserialize_with = "unique_map",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub(super) profile_applications: BTreeMap<String, String>,
}

fn unique_map<'de, D, T>(deserializer: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct UniqueMap<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for UniqueMap<T> {
        type Value = BTreeMap<String, T>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a map of unique metadata keys")
        }
        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            let mut views = BTreeMap::new();
            while let Some((key, view)) = map.next_entry::<String, T>()? {
                if views.insert(key, view).is_some() {
                    return Err(serde::de::Error::custom("duplicate metadata key"));
                }
            }
            Ok(views)
        }
    }
    deserializer.deserialize_map(UniqueMap(std::marker::PhantomData))
}

pub(super) fn load(root: &Path) -> io::Result<Metadata> {
    let path = root.join("organization.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // A dangling metadata symlink is unreadable, not a fresh install.
            match fs::symlink_metadata(&path) {
                Err(missing) if missing.kind() == io::ErrorKind::NotFound => {
                    return Ok(Metadata {
                        version: 1,
                        views: BTreeMap::new(),
                        profile_applications: BTreeMap::new(),
                    });
                }
                Ok(_) => return Err(error),
                Err(other) => return Err(other),
            }
        }
        Err(error) => return Err(error),
    };
    let metadata: Metadata = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if metadata.version != 1 {
        return Err(bad_metadata("unsupported organization version"));
    }
    validate(&metadata.views)?;
    validate_applications(&metadata.views, &metadata.profile_applications)?;
    Ok(metadata)
}

pub(super) fn validate(views: &BTreeMap<String, View>) -> io::Result<()> {
    let mut coordinates = BTreeSet::new();
    for (key, view) in views {
        if key == crate::layers::DEFAULT_LAYER
            && (view.profile != crate::layers::DEFAULT_LAYER
                || !view.folder.is_empty()
                || view.page != 1)
        {
            return Err(bad_metadata("Base profile coordinates are protected"));
        }
        if !valid_name(key) || !valid_name(&view.profile) || view.page == 0 {
            return Err(bad_metadata("invalid layer key, profile, or page"));
        }
        if view
            .page_name
            .as_ref()
            .is_some_and(|name| !valid_name(name) || name.trim() != name)
        {
            return Err(bad_metadata("invalid page name"));
        }
        for name in &view.folder {
            validate_folder(name)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        }
        if !coordinates.insert((&view.profile, &view.folder, view.page)) {
            return Err(bad_metadata("duplicate organization view"));
        }
    }
    Ok(())
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 64 && !name.chars().any(char::is_control)
}

fn bad_metadata(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn validate_applications(
    views: &BTreeMap<String, View>,
    applications: &BTreeMap<String, String>,
) -> io::Result<()> {
    for (profile, application) in applications {
        if profile == crate::layers::DEFAULT_LAYER
            || !views.values().any(|view| &view.profile == profile)
            || application.is_empty()
            || application.len() > 128
            || application.trim() != application
            || application.chars().any(char::is_control)
        {
            return Err(bad_metadata("invalid profile application"));
        }
    }
    Ok(())
}

pub(super) fn persist(
    root: &Path,
    views: &BTreeMap<String, View>,
    profile_applications: &BTreeMap<String, String>,
) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    validate(views)?;
    validate_applications(views, profile_applications)?;
    let bytes = serde_json::to_vec_pretty(&Metadata {
        version: 1,
        views: views.clone(),
        profile_applications: profile_applications.clone(),
    })
    .map_err(io::Error::other)?;
    fs::create_dir_all(root)?;
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp = root.join(format!(
        ".organization.json.{}.{sequence}.tmp",
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, root.join("organization.json"))
    })();
    drop(file);
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            fs::remove_file(temp)?;
            Err(error)
        }
    }
}
