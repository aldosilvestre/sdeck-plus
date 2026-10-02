//! Versioned, app-owned local artwork assignments.

use gtk::gdk_pixbuf::Pixbuf;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CONFIG_VERSION: &str = "streamdeck-artwork-v1";

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
    assignments: HashMap<ArtworkTarget, String>,
    diagnostics: Vec<String>,
}

impl ArtworkStore {
    /// Open the default app-specific XDG data directory (with a HOME fallback).
    /// Invalid/missing configuration is ignored; the store starts empty.
    pub fn open() -> io::Result<Self> {
        Self::open_at(default_root()?)
    }

    /// Open a specific root; public primarily for isolated tests.
    pub fn open_at(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("assets"))?;
        let (assignments, diagnostics) = load_assignments(&root)?;
        Ok(Self {
            root,
            assignments,
            diagnostics,
        })
    }

    /// Non-fatal configuration and asset problems encountered while loading.
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn assigned_path(&self, target: ArtworkTarget) -> Option<PathBuf> {
        self.assignments
            .get(&target)
            .map(|name| self.root.join("assets").join(name))
    }

    /// Decode and copy an image before atomically switching its assignment.
    /// On any failure, the previous valid assignment remains untouched.
    pub fn assign(
        &mut self,
        target: ArtworkTarget,
        source: impl AsRef<Path>,
    ) -> io::Result<PathBuf> {
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
        let temp = assets.join(format!(".{name}.tmp"));
        fs::copy(source, &temp)?;
        if let Err(error) = fs::rename(&temp, &destination) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        let old = self.assignments.insert(target, name.clone());
        if let Err(error) = persist(&self.root, &self.assignments) {
            if let Some(old_name) = old {
                self.assignments.insert(target, old_name);
            } else {
                self.assignments.remove(&target);
            }
            let _ = fs::remove_file(destination);
            return Err(error);
        }
        Ok(destination)
    }

    pub fn clear(&mut self, target: ArtworkTarget) -> io::Result<()> {
        let old = self.assignments.remove(&target);
        if let Err(error) = persist(&self.root, &self.assignments) {
            if let Some(name) = old {
                self.assignments.insert(target, name);
            }
            return Err(error);
        }
        if let Some(name) = old {
            let _ = fs::remove_file(self.root.join("assets").join(name));
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
    Ok(base.join("streamdeck-plus-monitor"))
}

fn load_assignments(root: &Path) -> io::Result<(HashMap<ArtworkTarget, String>, Vec<String>)> {
    let text = match fs::read_to_string(root.join("artwork.conf")) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((HashMap::new(), Vec::new()))
        }
        Err(error) => return Err(error),
    };
    let mut lines = text.lines();
    if lines.next() != Some(CONFIG_VERSION) {
        return Ok((HashMap::new(), vec!["Artwork configuration has an unsupported or invalid version; assignments were ignored".into()]));
    }
    let mut result = HashMap::new();
    let mut diagnostics = Vec::new();
    for line in lines {
        let Some((id, name)) = line.split_once('\t') else {
            return Ok((
                HashMap::new(),
                vec!["Artwork configuration is malformed; assignments were ignored".into()],
            ));
        };
        let Some(target) = ArtworkTarget::from_id(id) else {
            return Ok((
                HashMap::new(),
                vec![
                    "Artwork configuration contains an unknown target; assignments were ignored"
                        .into(),
                ],
            ));
        };
        if name.is_empty() || Path::new(name).components().count() != 1 || name.starts_with('.') {
            return Ok((
                HashMap::new(),
                vec![
                    "Artwork configuration contains an unsafe asset path; assignments were ignored"
                        .into(),
                ],
            ));
        }
        if !root.join("assets").join(name).is_file() {
            diagnostics.push(format!(
                "Artwork asset for {} is missing; that assignment was ignored",
                target.id()
            ));
            continue;
        }
        result.insert(target, name.to_owned());
    }
    Ok((result, diagnostics))
}

fn persist(root: &Path, assignments: &HashMap<ArtworkTarget, String>) -> io::Result<()> {
    let mut rows: Vec<_> = assignments.iter().collect();
    rows.sort_by_key(|(target, _)| target.id());
    let mut text = format!("{CONFIG_VERSION}\n");
    for (target, name) in rows {
        text.push_str(&format!("{}\t{}\n", target.id(), name));
    }
    let temp = root.join("artwork.conf.tmp");
    let mut file = fs::File::create(&temp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(temp, root.join("artwork.conf"))
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
