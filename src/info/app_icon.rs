//! The icon of the application a media player belongs to: its MPRIS
//! `DesktopEntry` resolved through the desktop file's `Icon` to an image file
//! in the icon themes, decoded once off the GTK thread and cached.

use gtk::{
    gdk_pixbuf::Pixbuf,
    gio::{self, prelude::*},
    glib,
};
use std::{
    collections::{BTreeSet, HashMap},
    fmt, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Decoded size; the largest slot that draws it is 52px.
const DECODED: i32 = 128;
/// PNGs larger than this rank below every size up to it.
const LARGEST_PNG: u32 = 256;

/// A decoded application icon, cheap to clone and compare (by its file).
#[derive(Clone)]
pub struct AppIcon {
    path: Arc<Path>,
    width: i32,
    height: i32,
    /// Cairo ARGB32 pixels: native-endian, premultiplied, `width * 4` stride.
    argb: Arc<[u8]>,
}

impl AppIcon {
    /// Decode an SVG (rendered at 128x128) or a bitmap (scaled down to
    /// fit 128x128, never up), keeping its aspect.
    pub fn load(path: &Path) -> Option<Self> {
        let svg = path.extension().is_some_and(|extension| extension == "svg");
        let image = if svg {
            Pixbuf::from_file_at_scale(path, DECODED, DECODED, true).ok()?
        } else {
            let image = Pixbuf::from_file(path).ok()?;
            let largest = image.width().max(image.height());
            if largest > DECODED {
                let scale = |side: i32| (side * DECODED / largest).max(1);
                image.scale_simple(
                    scale(image.width()),
                    scale(image.height()),
                    gtk::gdk_pixbuf::InterpType::Bilinear,
                )?
            } else {
                image
            }
        };
        Self::from_pixbuf(&image, path)
    }

    /// An icon from pixels already decoded; `path` identifies it.
    pub fn from_pixbuf(image: &Pixbuf, path: &Path) -> Option<Self> {
        let (width, height) = (image.width(), image.height());
        let stride = usize::try_from(image.rowstride()).ok()?;
        let channels = usize::try_from(image.n_channels()).ok()?;
        let bytes = image.read_pixel_bytes();
        let mut argb = Vec::with_capacity(usize::try_from(width * height * 4).ok()?);
        for y in 0..usize::try_from(height).ok()? {
            for x in 0..usize::try_from(width).ok()? {
                let at = y * stride + x * channels;
                let alpha = if image.has_alpha() {
                    bytes[at + 3]
                } else {
                    255
                };
                let premultiply =
                    |channel: usize| u32::from(bytes[at + channel]) * u32::from(alpha) / 255;
                let pixel = u32::from(alpha) << 24
                    | premultiply(0) << 16
                    | premultiply(1) << 8
                    | premultiply(2);
                argb.extend_from_slice(&pixel.to_ne_bytes());
            }
        }
        Some(Self {
            path: path.into(),
            width,
            height,
            argb: argb.into(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// Cairo ARGB32 pixels (premultiplied, native-endian, `width * 4` stride).
    pub fn argb(&self) -> &[u8] {
        &self.argb
    }
}

impl PartialEq for AppIcon {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for AppIcon {}

impl fmt::Debug for AppIcon {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("AppIcon").field(&self.path).finish()
    }
}

/// Where desktop files and icon themes live, in search order.
#[derive(Clone, Debug)]
pub struct IconDirs {
    /// XDG data directories (each holds `applications/`, `icons/`, `pixmaps/`).
    pub data: Vec<PathBuf>,
    /// Themes to search, best first; `hicolor` is always searched last.
    pub themes: Vec<String>,
}

impl IconDirs {
    /// The user's data dirs (`$XDG_DATA_HOME`, flatpak exports,
    /// `$XDG_DATA_DIRS`) and icon theme (GNOME interface setting or GTK
    /// `settings.ini`), with the themes it inherits from.
    pub fn from_environment() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut data = Vec::new();
        match std::env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
            Some(dir) => data.push(PathBuf::from(dir)),
            None => data.extend(home.iter().map(|home| home.join(".local/share"))),
        }
        data.extend(
            home.iter()
                .map(|home| home.join(".local/share/flatpak/exports/share")),
        );
        data.push("/var/lib/flatpak/exports/share".into());
        let system = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|dirs| !dirs.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        data.extend(
            system
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
        );
        let mut unique = BTreeSet::new();
        data.retain(|dir| unique.insert(dir.clone()));
        let theme = current_theme(home.as_deref());
        let themes = with_inherited(&data, theme.into_iter().collect());
        Self { data, themes }
    }
}

fn current_theme(home: Option<&Path>) -> Option<String> {
    // A missing schema would abort GSettings, so look it up first.
    let from_settings = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup("org.gnome.desktop.interface", true))
        .filter(|schema| schema.has_key("icon-theme"))
        .map(|schema| {
            gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None)
                .string("icon-theme")
                .to_string()
        });
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(".config")));
    from_settings.filter(|theme| !theme.is_empty()).or_else(|| {
        let config = config?;
        ["gtk-4.0", "gtk-3.0"].iter().find_map(|version| {
            let text = fs::read_to_string(config.join(version).join("settings.ini")).ok()?;
            text.lines().find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "gtk-icon-theme-name")
                    .then(|| value.trim().trim_matches('"').to_owned())
                    .filter(|theme| !theme.is_empty())
            })
        })
    })
}

/// `themes` followed by the themes they inherit from (`index.theme`), once
/// each, then `hicolor`.
fn with_inherited(data: &[PathBuf], mut themes: Vec<String>) -> Vec<String> {
    let mut index = 0;
    while index < themes.len() && themes.len() < 16 {
        let inherited = data.iter().find_map(|dir| {
            let keys = glib::KeyFile::new();
            keys.load_from_file(
                dir.join("icons").join(&themes[index]).join("index.theme"),
                glib::KeyFileFlags::NONE,
            )
            .ok()?;
            keys.string("Icon Theme", "Inherits").ok()
        });
        for theme in inherited.iter().flat_map(|list| list.split(',')) {
            let theme = theme.trim().to_owned();
            if !theme.is_empty() && !themes.contains(&theme) {
                themes.push(theme);
            }
        }
        index += 1;
    }
    themes.retain(|theme| theme != "hicolor");
    themes.push("hicolor".into());
    themes
}

/// The desktop file for `entry` (`name` or `name.desktop`), as is or in
/// lower case, in the first data directory that has it.
fn desktop_file(dirs: &IconDirs, entry: &str) -> Option<PathBuf> {
    let entry = entry.strip_suffix(".desktop").unwrap_or(entry);
    if entry.is_empty() || entry.contains('/') {
        return None;
    }
    let names = [entry.to_owned(), entry.to_lowercase()];
    dirs.data.iter().find_map(|dir| {
        names
            .iter()
            .map(|name| dir.join("applications").join(format!("{name}.desktop")))
            .find(|path| path.is_file())
    })
}

/// The image file for `entry`'s desktop file `Icon`: an absolute path as is,
/// a theme name through the themes (scalable SVG first, else the largest PNG
/// up to 256px), then `pixmaps`.
pub fn resolve(dirs: &IconDirs, entry: &str) -> Option<PathBuf> {
    let info = gio::DesktopAppInfo::from_filename(desktop_file(dirs, entry)?)?;
    let icon = info.icon()?;
    if let Some(file) = icon.downcast_ref::<gio::FileIcon>() {
        return file.file().path().filter(|path| path.is_file());
    }
    let themed = icon.downcast_ref::<gio::ThemedIcon>()?;
    themed.names().iter().find_map(|name| {
        let name = name.as_str();
        if Path::new(name).is_absolute() {
            return Some(PathBuf::from(name)).filter(|path| path.is_file());
        }
        let name = name
            .strip_suffix(".png")
            .or_else(|| name.strip_suffix(".svg"))
            .unwrap_or(name);
        themed_file(dirs, name).or_else(|| pixmap(dirs, name))
    })
}

fn themed_file(dirs: &IconDirs, name: &str) -> Option<PathBuf> {
    dirs.themes.iter().find_map(|theme| {
        dirs.data
            .iter()
            .filter_map(|dir| best_in_theme(&dir.join("icons").join(theme), name))
            .max_by_key(|(rank, _)| *rank)
            .map(|(_, path)| path)
    })
}

/// The best `name` icon in one theme directory, ranked: scalable SVG, then
/// PNG by size up to [`LARGEST_PNG`], then larger PNGs.
fn best_in_theme(theme: &Path, name: &str) -> Option<(u32, PathBuf)> {
    let mut best: Option<(u32, PathBuf)> = None;
    for size_dir in fs::read_dir(theme).ok()?.flatten() {
        let label = size_dir.file_name().to_string_lossy().into_owned();
        let size = if label == "scalable" {
            None
        } else {
            let base = label.split('@').next().unwrap_or_default();
            let side = base.split_once('x').map_or(base, |(side, _)| side);
            match side.parse::<u32>() {
                Ok(side) => Some(side),
                Err(_) => continue,
            }
        };
        let Ok(contexts) = fs::read_dir(size_dir.path()) else {
            continue;
        };
        for context in contexts.flatten() {
            for (extension, rank) in [
                ("svg", 2 * LARGEST_PNG + 1),
                (
                    "png",
                    match size {
                        Some(side) if side <= LARGEST_PNG => LARGEST_PNG + side,
                        Some(_) => 1,
                        None => LARGEST_PNG,
                    },
                ),
            ] {
                let path = context.path().join(format!("{name}.{extension}"));
                if best.as_ref().is_some_and(|(found, _)| *found >= rank) || !path.is_file() {
                    continue;
                }
                best = Some((rank, path));
            }
        }
    }
    best
}

fn pixmap(dirs: &IconDirs, name: &str) -> Option<PathBuf> {
    dirs.data.iter().find_map(|dir| {
        ["svg", "png"]
            .iter()
            .map(|extension| dir.join("pixmaps").join(format!("{name}.{extension}")))
            .find(|path| path.is_file())
    })
}

/// Resolved and decoded icons per desktop entry, misses included, so the
/// info worker resolves each application once.
#[derive(Debug, Default)]
pub struct AppIcons {
    /// Read from the environment on first use.
    dirs: Option<IconDirs>,
    cache: HashMap<String, Option<AppIcon>>,
}

impl AppIcons {
    pub fn with_dirs(dirs: IconDirs) -> Self {
        Self {
            dirs: Some(dirs),
            cache: HashMap::new(),
        }
    }

    /// The icon of the first candidate entry that has one.
    pub fn lookup<'a>(&mut self, entries: impl IntoIterator<Item = &'a str>) -> Option<AppIcon> {
        for entry in entries {
            if let Some(found) = self.lookup_one(entry) {
                return Some(found);
            }
        }
        None
    }

    fn lookup_one(&mut self, entry: &str) -> Option<AppIcon> {
        if let Some(cached) = self.cache.get(entry) {
            return cached.clone();
        }
        let dirs = self.dirs.get_or_insert_with(IconDirs::from_environment);
        let found = resolve(dirs, entry).and_then(|path| AppIcon::load(&path));
        self.cache.insert(entry.to_owned(), found.clone());
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        dirs: IconDirs,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn fixture(name: &str) -> Fixture {
        let root =
            std::env::temp_dir().join(format!("sdeck-app-icon-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (user, system) = (root.join("user"), root.join("system"));
        for dir in [&user, &system] {
            fs::create_dir_all(dir.join("applications")).unwrap();
        }
        Fixture {
            dirs: IconDirs {
                data: vec![user, system],
                themes: vec!["Test".into(), "hicolor".into()],
            },
            root,
        }
    }

    fn desktop(dir: &Path, entry: &str, icon: &str) {
        fs::write(
            dir.join("applications").join(format!("{entry}.desktop")),
            format!("[Desktop Entry]\nType=Application\nName={entry}\nExec=true\nIcon={icon}\n"),
        )
        .unwrap();
    }

    fn png(path: &Path, side: i32, rgba: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let image = Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, true, 8, side, side).unwrap();
        image.fill(rgba);
        image.savev(path, "png", &[]).unwrap();
    }

    #[test]
    fn theme_icons_prefer_the_current_theme_svg_then_the_largest_png_up_to_256() {
        let fixture = fixture("theme");
        let system = &fixture.dirs.data[1];
        desktop(system, "player", "player");
        let hicolor = system.join("icons/hicolor");
        png(&hicolor.join("48x48/apps/player.png"), 48, 0xff0000ff);
        png(&hicolor.join("256x256/apps/player.png"), 64, 0xff0000ff);
        png(&hicolor.join("512x512/apps/player.png"), 64, 0xff0000ff);
        assert_eq!(
            resolve(&fixture.dirs, "player"),
            Some(hicolor.join("256x256/apps/player.png"))
        );
        // A scalable icon wins in its theme; the current theme wins over hicolor.
        fs::create_dir_all(hicolor.join("scalable/apps")).unwrap();
        fs::write(hicolor.join("scalable/apps/player.svg"), "<svg/>").unwrap();
        assert_eq!(
            resolve(&fixture.dirs, "player"),
            Some(hicolor.join("scalable/apps/player.svg"))
        );
        let themed = fixture.dirs.data[0].join("icons/Test/32x32@2/apps/player.png");
        png(&themed, 32, 0x00ff00ff);
        assert_eq!(resolve(&fixture.dirs, "player"), Some(themed));
        // Entries are case-insensitive and may carry the .desktop suffix.
        assert!(resolve(&fixture.dirs, "Player.desktop").is_some());
    }

    #[test]
    fn absolute_icons_pixmaps_and_misses() {
        let fixture = fixture("paths");
        let user = &fixture.dirs.data[0];
        let absolute = fixture.root.join("anywhere/app.png");
        png(&absolute, 16, 0x0000ffff);
        desktop(user, "com.example.Absolute", &absolute.to_string_lossy());
        assert_eq!(
            resolve(&fixture.dirs, "com.example.Absolute"),
            Some(absolute)
        );
        let legacy = fixture.dirs.data[1].join("pixmaps/legacy.png");
        png(&legacy, 16, 0x0000ffff);
        desktop(user, "legacy", "legacy");
        assert_eq!(resolve(&fixture.dirs, "legacy"), Some(legacy));
        desktop(user, "nameless", "no-such-icon");
        assert_eq!(resolve(&fixture.dirs, "nameless"), None);
        assert_eq!(resolve(&fixture.dirs, "not-installed"), None);
        assert_eq!(resolve(&fixture.dirs, "../escape"), None);
    }

    #[test]
    fn icons_are_decoded_once_per_entry_and_misses_are_cached() {
        let fixture = fixture("cache");
        let user = fixture.dirs.data[0].clone();
        let file = user.join("icons/hicolor/64x64/apps/cached.png");
        png(&file, 64, 0xff8000ff);
        desktop(&user, "cached", "cached");
        let mut icons = AppIcons::with_dirs(fixture.dirs.clone());
        let first = icons.lookup(["missing", "cached"]).unwrap();
        assert_eq!(first.path(), file);
        assert_eq!(first.size(), (64, 64));
        // Premultiplied opaque orange: alpha 255, red 255, green 128.
        assert_eq!(
            u32::from_ne_bytes(first.argb()[..4].try_into().unwrap()),
            0xffff8000
        );
        // Later lookups use the cache, hits and misses alike.
        fs::remove_file(&file).unwrap();
        desktop(&user, "missing", "cached");
        png(&file, 64, 0xff8000ff);
        let again = icons.lookup(["missing", "cached"]).unwrap();
        assert!(Arc::ptr_eq(&first.argb, &again.argb), "decoded once");
        assert_eq!(icons.lookup(["missing"]), None, "a miss stays a miss");
    }
}
