//! Versioned, app-owned local artwork assignments.

use crate::button_text::ButtonText;
use gtk::gdk_pixbuf::Pixbuf;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CONFIG_VERSION: &str = "streamdeck-artwork-v1";
const LAYER_VERSION: &str = "streamdeck-artwork-v2";
const TEXT_VERSION: &str = "streamdeck-artwork-v3";
type Assignments = HashMap<(String, ArtworkTarget), String>;
type ButtonTexts = HashMap<(String, ArtworkTarget), ButtonText>;
type LoadedArtwork = (Assignments, ButtonTexts, bool, Vec<String>);

/// The complete set of stable artwork destinations supported by the app.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArtworkTarget {
    Keypad(u8),
    TouchStripBackground,
    EncoderZone(u8),
}

impl ArtworkTarget {
    pub const ALL: [Self; 13] = [
        Self::Keypad(1),
        Self::Keypad(2),
        Self::Keypad(3),
        Self::Keypad(4),
        Self::Keypad(5),
        Self::Keypad(6),
        Self::Keypad(7),
        Self::Keypad(8),
        Self::TouchStripBackground,
        Self::EncoderZone(1),
        Self::EncoderZone(2),
        Self::EncoderZone(3),
        Self::EncoderZone(4),
    ];

    fn id(self) -> String {
        match self {
            Self::Keypad(n) if (1..=8).contains(&n) => format!("keypad-{n}"),
            Self::TouchStripBackground => "touch-strip-background".into(),
            Self::EncoderZone(n) if (1..=4).contains(&n) => format!("encoder-zone-{n}"),
            _ => String::new(),
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.id() == id)
    }
}

/// Manages the versioned assignment file and copied, app-owned image assets.
pub struct ArtworkStore {
    root: PathBuf,
    assignments: Assignments,
    button_texts: ButtonTexts,
    diagnostics: Vec<String>,
    legacy: bool,
    compatibility_layer: String,
}

impl ArtworkStore {
    /// Compatibility API: v1 stays global and is never implicitly migrated.
    /// With v2, compatibility operations address Base. Invalid configurations
    /// produce diagnostics and cannot be overwritten by mutations.
    pub fn open() -> io::Result<Self> {
        Self::open_at(default_root()?)
    }

    /// Open a specific root; public primarily for isolated tests.
    pub fn open_at(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("assets"))?;
        let (assignments, button_texts, legacy, diagnostics) = load_assignments(&root, false)?;
        let compatibility_layer = if legacy { "" } else { "Base" }.into();
        Ok(Self {
            root,
            assignments,
            button_texts,
            diagnostics,
            legacy,
            compatibility_layer,
        })
    }

    /// Open layer-aware storage, migrating all v1 artwork into the supplied
    /// active layer. None falls back to Base; other layers remain empty.
    pub fn open_for_layer(active_layer: Option<&str>) -> io::Result<Self> {
        Self::open_at_for_layer(default_root()?, active_layer)
    }

    pub fn open_at_for_layer(
        root: impl Into<PathBuf>,
        active_layer: Option<&str>,
    ) -> io::Result<Self> {
        let root = root.into();
        let active_layer = active_layer.unwrap_or("Base");
        let (mut assignments, button_texts, legacy, diagnostics) = load_assignments(&root, true)?;
        fs::create_dir_all(root.join("assets"))?;
        if legacy {
            assignments = assignments
                .into_iter()
                .map(|((_, target), name)| ((active_layer.to_owned(), target), name))
                .collect();
            persist(&root, &assignments, &button_texts, false)?;
        }
        Ok(Self {
            root,
            assignments,
            button_texts,
            diagnostics,
            legacy: false,
            compatibility_layer: active_layer.to_owned(),
        })
    }

    /// Non-fatal configuration and asset problems encountered while loading.
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    /// Storage root for reopening the same artwork store.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn assigned_path(&self, target: ArtworkTarget) -> Option<PathBuf> {
        self.assigned_path_in_layer(&self.compatibility_layer, target)
    }

    pub fn assigned_path_in_layer(&self, layer: &str, target: ArtworkTarget) -> Option<PathBuf> {
        self.assignments
            .get(&(layer.to_owned(), target))
            .map(|name| self.root.join("assets").join(name))
    }

    pub fn button_text_in_layer(&self, layer: &str, target: ArtworkTarget) -> Option<&ButtonText> {
        self.button_texts.get(&(layer.to_owned(), target))
    }

    /// Atomically replace keypad text; empty text removes only the overlay.
    pub fn set_button_text_in_layer(
        &mut self,
        layer: &str,
        target: ArtworkTarget,
        text: ButtonText,
    ) -> io::Result<()> {
        self.check_mutation(layer)?;
        if self.legacy {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "text requires layer-aware migration",
            ));
        }
        if !matches!(target, ArtworkTarget::Keypad(1..=8)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "text requires keypad 1..=8",
            ));
        }
        text.validate()?;
        let mut next = self.button_texts.clone();
        let key = (layer.to_owned(), target);
        if text.text.is_empty() {
            next.remove(&key);
        } else {
            next.insert(key, text);
        }
        self.commit_with_text(self.assignments.clone(), next)
    }

    /// Decode and copy an image before atomically switching its assignment.
    /// On any failure, the previous valid assignment remains untouched.
    pub fn assign(
        &mut self,
        target: ArtworkTarget,
        source: impl AsRef<Path>,
    ) -> io::Result<PathBuf> {
        self.assign_in_layer(&self.compatibility_layer.clone(), target, source)
    }

    /// Assign to an explicit layer. Requires a layer-aware open API for v1.
    pub fn assign_in_layer(
        &mut self,
        layer: &str,
        target: ArtworkTarget,
        source: impl AsRef<Path>,
    ) -> io::Result<PathBuf> {
        self.check_mutation(layer)?;
        if target.id().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported artwork target",
            ));
        }
        let source = source.as_ref();
        Pixbuf::from_file(source).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let extension = source
            .extension()
            .and_then(|s| s.to_str())
            .filter(|s| {
                !s.is_empty() && s.len() <= 12 && s.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
            .unwrap_or("img");
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!("{}-{stamp}.{extension}", target.id());
        let assets = self.root.join("assets");
        let destination = assets.join(&name);
        // Never overwrite an existing owned asset, even on a name collision.
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        let copied = (|| {
            let mut input = fs::File::open(source)?;
            io::copy(&mut input, &mut output)?;
            output.sync_all()
        })();
        if let Err(error) = copied {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }
        let mut next = self.assignments.clone();
        next.insert((layer.to_owned(), target), name);
        if let Err(error) = self.commit(next) {
            let _ = fs::remove_file(destination);
            return Err(error);
        }
        Ok(destination)
    }

    pub fn clear(&mut self, target: ArtworkTarget) -> io::Result<()> {
        self.clear_in_layer(&self.compatibility_layer.clone(), target)
    }

    pub fn clear_in_layer(&mut self, layer: &str, target: ArtworkTarget) -> io::Result<()> {
        self.check_mutation(layer)?;
        let mut next = self.assignments.clone();
        next.remove(&(layer.to_owned(), target));
        self.commit(next)
    }

    /// Atomically swap keypad images and text within one layer.
    ///
    /// # Errors
    /// Invalid keys, invalid configuration, unmigrated storage, or persistence
    /// failures leave all assignments, text, and referenced assets intact.
    pub fn swap_keys_in_layer(&mut self, layer: &str, first: u8, second: u8) -> io::Result<()> {
        self.check_mutation(layer)?;
        if !(1..=8).contains(&first) || !(1..=8).contains(&second) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "key index out of range",
            ));
        }
        if self.legacy {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "key swaps require layer-aware migration",
            ));
        }
        if first == second {
            return Ok(());
        }
        let first = (layer.to_owned(), ArtworkTarget::Keypad(first));
        let second = (layer.to_owned(), ArtworkTarget::Keypad(second));
        let mut next = self.assignments.clone();
        let first_image = next.remove(&first);
        let second_image = next.remove(&second);
        if let Some(image) = first_image {
            next.insert(second.clone(), image);
        }
        if let Some(image) = second_image {
            next.insert(first.clone(), image);
        }
        let mut texts = self.button_texts.clone();
        let first_text = texts.remove(&first);
        let second_text = texts.remove(&second);
        if let Some(text) = first_text {
            texts.insert(second, text);
        }
        if let Some(text) = second_text {
            texts.insert(first, text);
        }
        self.commit_with_text(next, texts)
    }

    /// Copy touch-only references in one commit without deleting legacy owners
    /// or copying asset bytes. A failed migration can safely repeat this step.
    pub fn copy_touchscreen_assignments(&mut self, owners: &[(String, String)]) -> io::Result<()> {
        if owners.is_empty() {
            return Ok(());
        }
        let mut next = self.assignments.clone();
        for (from, to) in owners {
            self.check_mutation(from)?;
            self.check_mutation(to)?;
            for target in ArtworkTarget::ALL
                .into_iter()
                .filter(|target| !matches!(target, ArtworkTarget::Keypad(_)))
            {
                if let Some(name) = self.assignments.get(&(from.clone(), target)) {
                    next.insert((to.clone(), target), name.clone());
                }
            }
        }
        self.commit(next)
    }

    /// Remove all assignments before layer deletion. Reusing its name starts empty.
    /// Assets still referenced by any other layer or target are retained.
    pub fn remove_layer(&mut self, layer: &str) -> io::Result<()> {
        self.remove_layers(&[layer.to_owned()])
    }

    /// Remove a page/folder subtree's artwork in one atomic configuration commit.
    ///
    /// # Errors
    /// Invalid configuration or persistence errors leave every assignment intact.
    pub fn remove_layers(&mut self, layers: &[String]) -> io::Result<()> {
        for layer in layers {
            self.check_mutation(layer)?;
        }
        let mut next = self.assignments.clone();
        next.retain(|(name, _), _| !layers.contains(name));
        let mut texts = self.button_texts.clone();
        texts.retain(|(name, _), _| !layers.contains(name));
        self.commit_with_text(next, texts)
    }

    fn check_mutation(&self, layer: &str) -> io::Result<()> {
        if !self.diagnostics.is_empty() || (self.legacy && !layer.is_empty()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "artwork requires valid configuration and explicit layer-aware migration",
            ));
        }
        Ok(())
    }

    fn commit(&mut self, next: Assignments) -> io::Result<()> {
        self.commit_with_text(next, self.button_texts.clone())
    }

    fn commit_with_text(&mut self, next: Assignments, texts: ButtonTexts) -> io::Result<()> {
        persist(&self.root, &next, &texts, self.legacy)?;
        self.button_texts = texts;
        let old = std::mem::replace(&mut self.assignments, next);
        for name in old.values() {
            if !self.assignments.values().any(|remaining| remaining == name) {
                // Cleanup is best effort after the configuration commit, never before.
                let _ = fs::remove_file(self.root.join("assets").join(name));
            }
        }
        Ok(())
    }
}

fn default_root() -> io::Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no absolute XDG_DATA_HOME or HOME available",
            )
        })?;
    Ok(base.join("sdeck-plus"))
}

fn load_assignments(root: &Path, strict: bool) -> io::Result<LoadedArtwork> {
    let text = match fs::read_to_string(root.join("artwork.conf")) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((HashMap::new(), HashMap::new(), true, Vec::new()))
        }
        Err(error) => return Err(error),
    };
    let mut lines = text.lines();
    let version = lines.next();
    let legacy = match version {
        Some(CONFIG_VERSION) => true,
        Some(LAYER_VERSION) => false,
        Some(TEXT_VERSION) => false,
        _ => return invalid_config(strict, "unsupported artwork version"),
    };
    let mut result = HashMap::new();
    let mut button_texts = HashMap::new();
    let mut diagnostics = Vec::new();
    for line in lines {
        let fields: Vec<_> = line.split('\t').collect();
        if let [layer, id, "text", json] = fields.as_slice() {
            if version != Some(TEXT_VERSION) {
                return invalid_config(strict, "text row requires artwork v3");
            }
            let Some(layer) = decode_layer(layer) else {
                return invalid_config(strict, "invalid encoded layer name");
            };
            let Some(target @ ArtworkTarget::Keypad(1..=8)) = ArtworkTarget::from_id(id) else {
                return invalid_config(strict, "text requires keypad 1..=8");
            };
            let Ok(text) = serde_json::from_str::<ButtonText>(json) else {
                return invalid_config(strict, "invalid button text configuration");
            };
            if text.validate().is_err() || text.text.is_empty() {
                return invalid_config(strict, "invalid button text configuration");
            }
            if button_texts.insert((layer, target), text).is_some() {
                return invalid_config(strict, "duplicate button text assignment");
            }
            continue;
        }
        let (layer, id, name) = match fields.as_slice() {
            [id, name] if legacy => (String::new(), *id, *name),
            [layer, id, name] if !legacy => {
                let Some(layer) = decode_layer(layer) else {
                    return invalid_config(strict, "invalid encoded layer name");
                };
                (layer, *id, *name)
            }
            _ => return invalid_config(strict, "malformed artwork row"),
        };
        let Some(target) = ArtworkTarget::from_id(id) else {
            return invalid_config(strict, "unknown artwork target");
        };
        if name.is_empty()
            || name.starts_with('.')
            || name.contains(['/', '\\', '\t', '\n', '\r'])
            || Path::new(name).components().count() != 1
        {
            return invalid_config(strict, "unsafe artwork asset path");
        }
        let key = (layer, target);
        if result.contains_key(&key) {
            return invalid_config(strict, "duplicate artwork assignment");
        }
        let path = root.join("assets").join(name);
        if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) {
            if strict {
                return invalid_config(true, "missing or non-regular artwork asset");
            }
            diagnostics.push(format!(
                "Artwork asset for {id} is missing; assignment ignored"
            ));
            continue;
        }
        if strict {
            Pixbuf::from_file(&path)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        }
        result.insert(key, name.to_owned());
    }
    Ok((result, button_texts, legacy, diagnostics))
}

fn invalid_config(strict: bool, message: &str) -> io::Result<LoadedArtwork> {
    if strict {
        Err(io::Error::new(io::ErrorKind::InvalidData, message))
    } else {
        Ok((HashMap::new(), HashMap::new(), true, vec![message.into()]))
    }
}

// Hex-encoded UTF-8 makes arbitrary layer identities safe in tab-separated rows.
fn encode_layer(layer: &str) -> String {
    layer
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_layer(encoded: &str) -> Option<String> {
    if !encoded.len().is_multiple_of(2) || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let bytes: Option<Vec<_>> = (0..encoded.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

fn persist(
    root: &Path,
    assignments: &Assignments,
    button_texts: &ButtonTexts,
    legacy: bool,
) -> io::Result<()> {
    let mut rows: Vec<_> = assignments.iter().collect();
    rows.sort_by_key(|((layer, target), _)| (layer.as_str(), target.id()));
    let version = if legacy { CONFIG_VERSION } else { TEXT_VERSION };
    let mut text = format!("{version}\n");
    for ((layer, target), name) in rows {
        if !legacy {
            text.push_str(&encode_layer(layer));
            text.push('\t');
        }
        text.push_str(&format!("{}\t{}\n", target.id(), name));
    }
    let mut texts: Vec<_> = button_texts.iter().collect();
    texts.sort_by_key(|((layer, target), _)| (layer.as_str(), target.id()));
    for ((layer, target), overlay) in texts {
        let json = serde_json::to_string(overlay).map_err(io::Error::other)?;
        text.push_str(&format!(
            "{}\t{}\ttext\t{json}\n",
            encode_layer(layer),
            target.id()
        ));
    }
    let temp = root.join("artwork.conf.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, root.join("artwork.conf"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "artwork-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn png(path: &Path, color: &str) {
        let pixbuf = Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, true, 8, 2, 2).unwrap();
        pixbuf.fill(u32::from_str_radix(color, 16).unwrap());
        pixbuf.savev(path, "png", &[]).unwrap();
    }

    #[test]
    fn swap_keys_exchanges_images_and_text_durably_without_touching_other_pages() {
        let root = temp();
        let source = root.join("source.png");
        let mut store = ArtworkStore::open_at_for_layer(&root, None).unwrap();
        assert_eq!(store.root(), root);
        let mut assets = Vec::new();
        for (layer, target, color, label) in [
            ("Base", ArtworkTarget::Keypad(1), "ff0000ff", "First"),
            ("Base", ArtworkTarget::Keypad(8), "00ff00ff", "Second"),
            ("Base", ArtworkTarget::Keypad(3), "0000ffff", "Keep"),
            ("Other", ArtworkTarget::Keypad(1), "ffff00ff", "Other"),
        ] {
            png(&source, color);
            let path = store.assign_in_layer(layer, target, &source).unwrap();
            assets.push((path.clone(), fs::read(path).unwrap()));
            store
                .set_button_text_in_layer(
                    layer,
                    target,
                    ButtonText {
                        text: label.into(),
                        font_size: if label == "First" { 24 } else { 36 },
                        ..ButtonText::default()
                    },
                )
                .unwrap();
        }
        let mut expected_images = store.assignments.clone();
        let mut expected_texts = store.button_texts.clone();
        let first = ("Base".to_owned(), ArtworkTarget::Keypad(1));
        let second = ("Base".to_owned(), ArtworkTarget::Keypad(8));
        expected_images.insert(first.clone(), store.assignments[&second].clone());
        expected_images.insert(second.clone(), store.assignments[&first].clone());
        expected_texts.insert(first.clone(), store.button_texts[&second].clone());
        expected_texts.insert(second.clone(), store.button_texts[&first].clone());

        store.swap_keys_in_layer("Base", 1, 8).unwrap();

        assert_eq!(store.assignments, expected_images);
        assert_eq!(store.button_texts, expected_texts);
        let reopened = ArtworkStore::open_at_for_layer(store.root(), None).unwrap();
        assert_eq!(reopened.assignments, expected_images);
        assert_eq!(reopened.button_texts, expected_texts);
        for (path, bytes) in assets {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn swap_keys_moves_independent_empty_images_and_text() {
        let root = temp();
        let source = root.join("source.png");
        png(&source, "ff0000ff");
        let mut store = ArtworkStore::open_at_for_layer(&root, None).unwrap();
        let first = ArtworkTarget::Keypad(1);
        let second = ArtworkTarget::Keypad(2);
        let asset = store.assign_in_layer("Base", first, &source).unwrap();
        let text = ButtonText {
            text: "Text only".into(),
            ..ButtonText::default()
        };
        store
            .set_button_text_in_layer("Base", second, text.clone())
            .unwrap();
        store.swap_keys_in_layer("Base", 1, 2).unwrap();
        assert_eq!(store.assigned_path_in_layer("Base", first), None);
        assert_eq!(
            store.assigned_path_in_layer("Base", second),
            Some(asset.clone())
        );
        assert_eq!(store.button_text_in_layer("Base", first), Some(&text));
        assert_eq!(store.button_text_in_layer("Base", second), None);
        store.swap_keys_in_layer("Base", 2, 8).unwrap();
        assert_eq!(store.assigned_path_in_layer("Base", second), None);
        assert_eq!(
            store.assigned_path_in_layer("Base", ArtworkTarget::Keypad(8)),
            Some(asset.clone())
        );
        store.swap_keys_in_layer("Base", 1, 8).unwrap();
        let reopened = ArtworkStore::open_at_for_layer(&root, None).unwrap();
        assert_eq!(reopened.assignments, store.assignments);
        assert_eq!(reopened.button_texts, store.button_texts);
        assert!(asset.is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn swap_keys_noops_invalid_inputs_and_failed_manifest_preserve_old_data() {
        let root = temp();
        let source = root.join("source.png");
        png(&source, "ff0000ff");
        let mut store = ArtworkStore::open_at_for_layer(&root, None).unwrap();
        for key in [1, 8] {
            store
                .assign_in_layer("Base", ArtworkTarget::Keypad(key), &source)
                .unwrap();
            store
                .set_button_text_in_layer(
                    "Base",
                    ArtworkTarget::Keypad(key),
                    ButtonText {
                        text: format!("Saved {key}"),
                        ..ButtonText::default()
                    },
                )
                .unwrap();
        }
        let images = store.assignments.clone();
        let texts = store.button_texts.clone();
        let manifest = fs::read(root.join("artwork.conf")).unwrap();
        fs::create_dir(root.join("artwork.conf.tmp")).unwrap();
        for key in [1, 4, 8] {
            store.swap_keys_in_layer("Base", key, key).unwrap();
        }
        for (first, second) in [(0, 1), (1, 0), (9, 1), (1, 9), (0, 0), (9, 9)] {
            assert_eq!(
                store
                    .swap_keys_in_layer("Base", first, second)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert!(store.swap_keys_in_layer("Base", 1, 8).is_err());
        assert_eq!(store.assignments, images);
        assert_eq!(store.button_texts, texts);
        assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), manifest);
        for name in images.values() {
            assert!(root.join("assets").join(name).is_file());
        }
        let reopened = ArtworkStore::open_at_for_layer(&root, None).unwrap();
        assert_eq!(reopened.assignments, images);
        assert_eq!(reopened.button_texts, texts);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn swap_keys_rejects_legacy_and_invalid_configuration_without_overwriting() {
        let root = temp();
        let source = root.join("source.png");
        png(&source, "ff0000ff");
        let mut legacy = ArtworkStore::open_at(&root).unwrap();
        let asset = legacy.assign(ArtworkTarget::Keypad(1), &source).unwrap();
        let manifest = fs::read(root.join("artwork.conf")).unwrap();
        assert!(legacy.swap_keys_in_layer("Base", 1, 2).is_err());
        assert!(legacy.swap_keys_in_layer("", 1, 2).is_err());
        assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), manifest);
        fs::write(root.join("artwork.conf"), "broken\n").unwrap();
        let mut invalid = ArtworkStore::open_at(&root).unwrap();
        assert!(invalid.swap_keys_in_layer("Base", 1, 2).is_err());
        assert_eq!(
            fs::read_to_string(root.join("artwork.conf")).unwrap(),
            "broken\n"
        );
        assert!(asset.is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn text_restart_isolation_clear_and_layer_cleanup() {
        let root = temp();
        let source = root.join("source.png");
        png(&source, "008800ff");
        let mut store = ArtworkStore::open_at_for_layer(&root, Some("Base")).unwrap();
        let layer = "Live\t日本\n";
        let first = ButtonText {
            text: "Hello\n世界\t\"".into(),
            font_family: "Serif".into(),
            font_size: 36,
            orientation: crate::button_text::TextOrientation::Vertical,
            horizontal_alignment: crate::button_text::HorizontalAlignment::Right,
            vertical_alignment: crate::button_text::VerticalAlignment::Bottom,
        };
        let second = ButtonText {
            text: "Other".into(),
            ..ButtonText::default()
        };
        store
            .assign_in_layer(layer, ArtworkTarget::Keypad(1), &source)
            .unwrap();
        store
            .set_button_text_in_layer(layer, ArtworkTarget::Keypad(1), first.clone())
            .unwrap();
        store
            .set_button_text_in_layer(layer, ArtworkTarget::Keypad(2), second.clone())
            .unwrap();
        store
            .set_button_text_in_layer("Base", ArtworkTarget::Keypad(1), second.clone())
            .unwrap();
        store
            .clear_in_layer(layer, ArtworkTarget::Keypad(1))
            .unwrap();
        drop(store);
        let mut store = ArtworkStore::open_at_for_layer(&root, Some(layer)).unwrap();
        assert!(store
            .assigned_path_in_layer(layer, ArtworkTarget::Keypad(1))
            .is_none());
        assert_eq!(
            store.button_text_in_layer(layer, ArtworkTarget::Keypad(1)),
            Some(&first)
        );
        assert_eq!(
            store.button_text_in_layer(layer, ArtworkTarget::Keypad(2)),
            Some(&second)
        );
        assert_eq!(
            store.button_text_in_layer("Base", ArtworkTarget::Keypad(1)),
            Some(&second)
        );
        assert!(store
            .button_text_in_layer("Base", ArtworkTarget::Keypad(2))
            .is_none());
        store
            .set_button_text_in_layer(layer, ArtworkTarget::Keypad(2), ButtonText::default())
            .unwrap();
        store.remove_layer(layer).unwrap();
        let reopened = ArtworkStore::open_at(&root).unwrap();
        assert!(reopened
            .button_text_in_layer(layer, ArtworkTarget::Keypad(1))
            .is_none());
        assert!(reopened
            .button_text_in_layer(layer, ArtworkTarget::Keypad(2))
            .is_none());
        assert_eq!(
            reopened.button_text_in_layer("Base", ArtworkTarget::Keypad(1)),
            Some(&second)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_text_and_failed_transactions_preserve_images_and_text() {
        let root = temp();
        let source = root.join("source.png");
        png(&source, "ff0000ff");
        let mut store = ArtworkStore::open_at_for_layer(&root, None).unwrap();
        let target = ArtworkTarget::Keypad(1);
        let asset = store.assign_in_layer("Base", target, &source).unwrap();
        let text = ButtonText {
            text: "Saved".into(),
            ..ButtonText::default()
        };
        store
            .set_button_text_in_layer("Base", target, text.clone())
            .unwrap();
        let before = fs::read(root.join("artwork.conf")).unwrap();
        for invalid in [
            ButtonText {
                font_size: 5,
                ..text.clone()
            },
            ButtonText {
                font_size: 121,
                ..text.clone()
            },
            ButtonText {
                font_family: " ".into(),
                ..text.clone()
            },
            ButtonText {
                text: "é".repeat(1025),
                ..text.clone()
            },
        ] {
            assert!(store
                .set_button_text_in_layer("Base", target, invalid)
                .is_err());
        }
        for target in [
            ArtworkTarget::Keypad(0),
            ArtworkTarget::Keypad(9),
            ArtworkTarget::TouchStripBackground,
            ArtworkTarget::EncoderZone(1),
        ] {
            assert!(store
                .set_button_text_in_layer("Base", target, text.clone())
                .is_err());
        }
        fs::create_dir(root.join("artwork.conf.tmp")).unwrap();
        assert!(store
            .set_button_text_in_layer("Base", target, ButtonText::default())
            .is_err());
        assert!(store.clear_in_layer("Base", target).is_err());
        assert!(store.remove_layer("Base").is_err());
        assert_eq!(store.button_text_in_layer("Base", target), Some(&text));
        assert_eq!(
            store.assigned_path_in_layer("Base", target),
            Some(asset.clone())
        );
        assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), before);
        assert!(asset.is_file());
        fs::remove_dir(root.join("artwork.conf.tmp")).unwrap();
        let valid = String::from_utf8(before).unwrap();
        for row in [
            "42617365\tkeypad-1\ttext\t{}\n".to_owned(),
            format!(
                "42617365\tencoder-zone-1\ttext\t{}\n",
                serde_json::to_string(&text).unwrap()
            ),
            format!(
                "42617365\tkeypad-1\ttext\t{}\n",
                serde_json::to_string(&text).unwrap()
            ),
            format!(
                "42617365\tkeypad-2\ttext\t{}\n",
                serde_json::to_string(&ButtonText {
                    font_size: 0,
                    ..text.clone()
                })
                .unwrap()
            ),
        ] {
            let corrupt = format!("{valid}{row}");
            fs::write(root.join("artwork.conf"), &corrupt).unwrap();
            assert!(ArtworkStore::open_at_for_layer(&root, None).is_err());
            let mut compatibility = ArtworkStore::open_at(&root).unwrap();
            assert!(!compatibility.diagnostics().is_empty());
            assert!(compatibility
                .set_button_text_in_layer("Base", target, text.clone())
                .is_err());
            assert_eq!(
                fs::read_to_string(root.join("artwork.conf")).unwrap(),
                corrupt
            );
            assert!(asset.is_file());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn jpeg_key_backgrounds_survive_reopening_without_the_source() {
        let root = temp();
        let image = Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 2, 2).unwrap();
        image.fill(0x4488ffff);
        for extension in ["jpg", "jpeg"] {
            let source = root.join(format!("source.{extension}"));
            image.savev(&source, "jpeg", &[]).unwrap();
            let bytes = fs::read(&source).unwrap();
            let mut store = ArtworkStore::open_at_for_layer(&root, Some("Base")).unwrap();
            let assigned = store
                .assign_in_layer("Base", ArtworkTarget::Keypad(1), &source)
                .unwrap();
            fs::remove_file(source).unwrap();
            let reopened = ArtworkStore::open_at_for_layer(&root, Some("Base")).unwrap();
            assert_eq!(
                reopened.assigned_path_in_layer("Base", ArtworkTarget::Keypad(1)),
                Some(assigned.clone())
            );
            assert_eq!(fs::read(&assigned).unwrap(), bytes);
            let decoded = Pixbuf::from_file(assigned).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (2, 2));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn layer_migration_unicode_isolation_and_recreation() {
        let root = temp();
        let source = root.join("source.png");
        png(&source, "ff0000ff");
        let mut legacy = ArtworkStore::open_at(&root).unwrap();
        let original = legacy.assign(ArtworkTarget::Keypad(1), &source).unwrap();
        let before = fs::read(root.join("artwork.conf")).unwrap();
        drop(ArtworkStore::open_at(&root).unwrap());
        assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), before);
        let layer = "工作\tdeck\n🎛/..";
        let mut store = ArtworkStore::open_at_for_layer(&root, Some(layer)).unwrap();
        assert_eq!(
            store.assigned_path_in_layer(layer, ArtworkTarget::Keypad(1)),
            Some(original)
        );
        assert!(store
            .assigned_path_in_layer("Base", ArtworkTarget::Keypad(1))
            .is_none());
        let other = store
            .assign_in_layer("Base", ArtworkTarget::Keypad(1), &source)
            .unwrap();
        store
            .clear_in_layer(layer, ArtworkTarget::Keypad(1))
            .unwrap();
        assert!(other.is_file());
        store
            .assign_in_layer(layer, ArtworkTarget::EncoderZone(1), &source)
            .unwrap();
        store.remove_layer(layer).unwrap();
        let mut reopened = ArtworkStore::open_at_for_layer(&root, Some(layer)).unwrap();
        assert!(reopened
            .assigned_path_in_layer(layer, ArtworkTarget::EncoderZone(1))
            .is_none());
        assert_eq!(
            reopened.assigned_path_in_layer("Base", ArtworkTarget::Keypad(1)),
            Some(other)
        );
        reopened
            .assign_in_layer(layer, ArtworkTarget::Keypad(2), &source)
            .unwrap();
        let reopened = ArtworkStore::open_at_for_layer(&root, Some(layer)).unwrap();
        assert!(reopened
            .assigned_path_in_layer(layer, ArtworkTarget::Keypad(2))
            .is_some());
    }

    #[test]
    fn migration_validation_and_persistence_failures_preserve_originals() {
        let root = temp();
        let source = root.join("source.png");
        png(&source, "ff0000ff");
        let mut legacy = ArtworkStore::open_at(&root).unwrap();
        let asset = legacy.assign(ArtworkTarget::Keypad(1), &source).unwrap();
        let original = fs::read(root.join("artwork.conf")).unwrap();
        fs::create_dir(root.join("artwork.conf.tmp")).unwrap();
        assert!(ArtworkStore::open_at_for_layer(&root, Some("Live")).is_err());
        assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), original);
        assert!(asset.is_file());
        let mut corrupt = original.clone();
        corrupt.extend_from_slice(b"keypad-2\t../unsafe.png\n");
        fs::write(root.join("artwork.conf"), &corrupt).unwrap();
        assert!(ArtworkStore::open_at_for_layer(&root, Some("Live")).is_err());
        assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), corrupt);
        assert!(asset.is_file());
    }

    #[test]
    fn failed_layer_mutations_and_shared_asset_cleanup() {
        let root = temp();
        fs::create_dir_all(root.join("assets")).unwrap();
        let asset = root.join("assets/shared.png");
        png(&asset, "ff0000ff");
        fs::write(
            root.join("artwork.conf"),
            "streamdeck-artwork-v2\n41\tkeypad-1\tshared.png\n41\tkeypad-2\tshared.png\n42\tkeypad-1\tshared.png\n",
        )
        .unwrap();
        let mut store = ArtworkStore::open_at_for_layer(&root, Some("A")).unwrap();
        store.clear_in_layer("A", ArtworkTarget::Keypad(1)).unwrap();
        assert!(asset.is_file());
        let replacement = store
            .assign_in_layer("A", ArtworkTarget::Keypad(2), &asset)
            .unwrap();
        assert!(asset.is_file());
        store.remove_layer("A").unwrap();
        assert!(!replacement.exists());
        assert!(asset.is_file());
        let prior = fs::read(root.join("artwork.conf")).unwrap();
        fs::create_dir(root.join("artwork.conf.tmp")).unwrap();
        assert!(store
            .assign_in_layer("B", ArtworkTarget::Keypad(1), &asset)
            .is_err());
        assert!(store.clear_in_layer("B", ArtworkTarget::Keypad(1)).is_err());
        assert!(store.remove_layer("B").is_err());
        assert_eq!(
            store.assigned_path_in_layer("B", ArtworkTarget::Keypad(1)),
            Some(asset.clone())
        );
        assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), prior);
        assert_eq!(fs::read_dir(root.join("assets")).unwrap().count(), 1);
        // A new root exercises successful removal of the final shared reference.
        let second = temp();
        fs::create_dir_all(second.join("assets")).unwrap();
        fs::copy(&asset, second.join("assets/shared.png")).unwrap();
        fs::write(second.join("artwork.conf"), &prior).unwrap();
        let mut store = ArtworkStore::open_at_for_layer(&second, None).unwrap();
        store.remove_layer("B").unwrap();
        assert!(!second.join("assets/shared.png").exists());
    }

    #[test]
    fn failed_bulk_removal_preserves_every_assignment_and_asset() {
        // Given: a subtree and an unrelated page, with a blocked atomic commit.
        let root = temp();
        let source = root.join("source.png");
        png(&source, "ff0000ff");
        let mut store = ArtworkStore::open_at_for_layer(&root, None).unwrap();
        let pages = ["Owner", "Child", "Keep"];
        let paths: Vec<_> = pages
            .iter()
            .map(|name| {
                store
                    .assign_in_layer(name, ArtworkTarget::Keypad(1), &source)
                    .unwrap()
            })
            .collect();
        let before = fs::read(root.join("artwork.conf")).unwrap();
        fs::create_dir(root.join("artwork.conf.tmp")).unwrap();
        // When
        assert!(store
            .remove_layers(&["Owner".into(), "Child".into()])
            .is_err());
        // Then
        assert_eq!(fs::read(root.join("artwork.conf")).unwrap(), before);
        for (page, path) in pages.iter().zip(paths) {
            assert_eq!(
                store.assigned_path_in_layer(page, ArtworkTarget::Keypad(1)),
                Some(path.clone())
            );
            assert!(path.is_file());
        }
    }

    #[test]
    fn bulk_removal_clears_only_the_deleted_subtree_artwork() {
        // Given
        let root = temp();
        let source = root.join("source.png");
        png(&source, "ff0000ff");
        let mut store = ArtworkStore::open_at_for_layer(&root, None).unwrap();
        let pages = ["Owner", "Child", "Keep"];
        let paths: Vec<_> = pages
            .iter()
            .map(|name| {
                store
                    .assign_in_layer(name, ArtworkTarget::Keypad(1), &source)
                    .unwrap()
            })
            .collect();
        // When
        store
            .remove_layers(&["Owner".into(), "Child".into()])
            .unwrap();
        // Then
        for (page, path) in pages[..2].iter().zip(&paths[..2]) {
            assert!(store
                .assigned_path_in_layer(page, ArtworkTarget::Keypad(1))
                .is_none());
            assert!(!path.exists());
        }
        assert_eq!(
            store.assigned_path_in_layer("Keep", ArtworkTarget::Keypad(1)),
            Some(paths[2].clone())
        );
        assert!(paths[2].is_file());
    }

    #[test]
    fn fallback_migration_and_invalid_configs_are_non_destructive() {
        let root = temp();
        let source = root.join("source.png");
        png(&source, "ff0000ff");
        let mut legacy = ArtworkStore::open_at(&root).unwrap();
        let asset = legacy.assign(ArtworkTarget::Keypad(1), &source).unwrap();
        assert!(legacy
            .assign_in_layer("Live", ArtworkTarget::Keypad(1), &source)
            .is_err());
        let store = ArtworkStore::open_at_for_layer(&root, None).unwrap();
        assert_eq!(
            store.assigned_path_in_layer("Base", ArtworkTarget::Keypad(1)),
            Some(asset.clone())
        );
        let name = asset.file_name().unwrap().to_str().unwrap();
        for text in [
            format!("{CONFIG_VERSION}\nkeypad-1\tmissing.png\n"),
            format!("{CONFIG_VERSION}\nkeypad-1\t{name}\nkeypad-1\t{name}\n"),
            format!("{LAYER_VERSION}\nff\tkeypad-1\t{name}\n"),
            format!("{LAYER_VERSION}\néé\tkeypad-1\t{name}\n"),
            format!("{LAYER_VERSION}\n4\tkeypad-1\t{name}\n"),
        ] {
            fs::write(root.join("artwork.conf"), &text).unwrap();
            assert!(ArtworkStore::open_at_for_layer(&root, None).is_err());
            let mut compatibility = ArtworkStore::open_at(&root).unwrap();
            assert!(compatibility.clear(ArtworkTarget::Keypad(1)).is_err());
            assert_eq!(fs::read_to_string(root.join("artwork.conf")).unwrap(), text);
            assert!(asset.is_file());
        }
        fs::write(&asset, b"corrupt image").unwrap();
        let text = format!("{CONFIG_VERSION}\nkeypad-1\t{name}\n");
        fs::write(root.join("artwork.conf"), &text).unwrap();
        assert!(ArtworkStore::open_at_for_layer(&root, None).is_err());
        assert_eq!(fs::read_to_string(root.join("artwork.conf")).unwrap(), text);
        assert_eq!(fs::read(asset).unwrap(), b"corrupt image");
    }

    #[test]
    fn targets_are_exactly_thirteen_unique_stable_ids() {
        let ids: Vec<_> = ArtworkTarget::ALL.iter().map(|t| t.id()).collect();
        assert_eq!(ids.len(), 13);
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            13
        );
        assert_eq!(ids[0], "keypad-1");
        assert_eq!(ids[8], "touch-strip-background");
        assert_eq!(ids[12], "encoder-zone-4");
    }

    #[test]
    fn round_trip_import_and_source_removal() {
        let root = temp();
        let source = root.join("original.png");
        png(&source, "ff0000ff");
        let mut store = ArtworkStore::open_at(root.join("store")).unwrap();
        let copied = store.assign(ArtworkTarget::Keypad(1), &source).unwrap();
        fs::remove_file(source).unwrap();
        assert!(copied.is_file());
        let loaded = ArtworkStore::open_at(root.join("store")).unwrap();
        assert_eq!(loaded.assigned_path(ArtworkTarget::Keypad(1)), Some(copied));
    }

    #[test]
    fn unusual_source_extension_does_not_corrupt_assignments() {
        let root = temp();
        let source = root.join("picture.\tbroken\nconfig");
        png(&source, "ff0000ff");
        let mut store = ArtworkStore::open_at(root.join("store")).unwrap();
        let copied = store.assign(ArtworkTarget::Keypad(2), &source).unwrap();
        assert_eq!(copied.extension().and_then(|s| s.to_str()), Some("img"));
        assert_eq!(
            ArtworkStore::open_at(root.join("store"))
                .unwrap()
                .assigned_path(ArtworkTarget::Keypad(2)),
            Some(copied)
        );
    }

    #[test]
    fn replacement_clear_and_failed_replacement_preserve_previous() {
        let root = temp();
        let source = root.join("image.png");
        png(&source, "ff0000ff");
        let mut store = ArtworkStore::open_at(root.join("store")).unwrap();
        let first = store
            .assign(ArtworkTarget::EncoderZone(2), &source)
            .unwrap();
        fs::write(&source, b"not an image").unwrap();
        assert!(store
            .assign(ArtworkTarget::EncoderZone(2), &source)
            .is_err());
        assert_eq!(
            store.assigned_path(ArtworkTarget::EncoderZone(2)),
            Some(first.clone())
        );
        png(&source, "00ff00ff");
        let second = store
            .assign(ArtworkTarget::EncoderZone(2), &source)
            .unwrap();
        assert_ne!(first, second);
        store.clear(ArtworkTarget::EncoderZone(2)).unwrap();
        assert_eq!(store.assigned_path(ArtworkTarget::EncoderZone(2)), None);
        assert!(ArtworkStore::open_at(root.join("store"))
            .unwrap()
            .assigned_path(ArtworkTarget::EncoderZone(2))
            .is_none());
    }

    #[test]
    fn malformed_config_and_missing_assets_recover_empty() {
        let root = temp();
        let store_root = root.join("store");
        fs::create_dir_all(store_root.join("assets")).unwrap();
        fs::write(
            store_root.join("artwork.conf"),
            "broken\nkeypad-1\t../../outside.png\n",
        )
        .unwrap();
        let corrupt = ArtworkStore::open_at(&store_root).unwrap();
        assert!(corrupt.assigned_path(ArtworkTarget::Keypad(1)).is_none());
        assert!(!corrupt.diagnostics().is_empty());
        fs::write(
            store_root.join("artwork.conf"),
            format!("{CONFIG_VERSION}\nkeypad-1\tmissing.png\n"),
        )
        .unwrap();
        let missing = ArtworkStore::open_at(store_root).unwrap();
        assert!(missing.assigned_path(ArtworkTarget::Keypad(1)).is_none());
        assert!(!missing.diagnostics().is_empty());
    }
}
