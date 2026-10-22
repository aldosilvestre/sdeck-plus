use super::*;
use crate::{
    button_text::ButtonText,
    info::InfoSource,
    layers::{PageReturn, Trigger},
    local_artwork::ArtworkTarget,
};
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use std::os::unix::fs::symlink;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sdeck-bundle-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        private_dir(&path).unwrap();
        Self(path)
    }

    fn root(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        private_dir(&path).unwrap();
        let layers = LayerStore::open_at(&path).unwrap();
        layers.persist().unwrap();
        path
    }

    fn bundle(&self, entries: &[(&str, &[u8])]) -> PathBuf {
        let bundle = Bundle {
            format: "sdeck-plus".into(),
            version: 1,
            files: entries
                .iter()
                .map(|(path, bytes)| Entry {
                    path: (*path).into(),
                    data: Some(STANDARD.encode(bytes)),
                })
                .collect(),
        };
        let path = self.0.join(format!(
            "{}.sdeck-plus.json",
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        write_private(&path, &serde_json::to_vec(&bundle).unwrap()).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

const LAYERS: &[u8] = b"streamdeck-layers-v1\nactive\t42617365\nlayer\t42617365\n";

fn contents(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let bundle: Bundle = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(bundle.format, "sdeck-plus");
    assert_eq!(bundle.version, 2);
    bundle
        .files
        .into_iter()
        .filter_map(|entry| {
            entry
                .data
                .map(|data| (entry.path, STANDARD.decode(data).unwrap()))
        })
        .collect()
}

fn assert_private(path: &Path, mode: u32) {
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        mode
    );
}

#[test]
fn export_preserves_configuration_and_external_audio_inputs() {
    let f = Fixture::new();
    let root = f.root("source");
    let before = fs::read(root.join("layers.conf")).unwrap();
    assert!(export(&root, &root.join("layers.conf")).is_err());
    assert_eq!(fs::read(root.join("layers.conf")).unwrap(), before);
    let audio = f.0.join("sound.wav");
    write_private(&audio, b"original audio").unwrap();
    let mut layers = LayerStore::open_at(&root).unwrap();
    bind_audio(&mut layers, "Base", Trigger::KeyPress(1), audio.clone()).unwrap();
    layers.persist().unwrap();
    assert!(export(&root, &audio).is_err());
    assert_eq!(fs::read(audio).unwrap(), b"original audio");
}

#[test]
fn relative_audio_uses_the_playback_working_directory() {
    let f = Fixture::new();
    let root = f.root("source");
    // Cargo runs tests from the package root. Different bytes in the config
    // directory distinguish playback's CWD semantics from config-relative paths.
    let relative = PathBuf::from("Cargo.toml");
    let expected = fs::read(&relative).unwrap();
    write_private(&root.join(&relative), b"wrong file in config directory").unwrap();
    let mut layers = LayerStore::open_at(&root).unwrap();
    bind_audio(&mut layers, "Base", Trigger::KeyPress(1), relative).unwrap();
    bind_audio(
        &mut layers,
        "Base",
        Trigger::KeyPress(2),
        "src/../Cargo.toml".into(),
    )
    .unwrap();
    layers.persist().unwrap();
    let bundle = f.0.join("export.sdeck-plus.json");
    export(&root, &bundle).unwrap();
    let files = contents(&bundle);
    let (_, bytes) = files
        .iter()
        .find(|(path, _)| path.starts_with("media/"))
        .unwrap();
    assert_eq!(bytes, &expected);
}

#[test]
fn roundtrip_preserves_all_stores_and_relocates_deduplicated_audio() {
    let f = Fixture::new();
    let original = f.root("original");
    let audio = f.0.join("sound.WAV");
    write_private(&audio, b"RIFF isolated audio fixture").unwrap();
    let mut layers = LayerStore::open_at(&original).unwrap();
    layers.create("Work").unwrap();
    layers.select("Work");
    let mut organization = Organization::open_at(&original, &layers).unwrap();
    let page = organization
        .create_named_page(&mut layers, "Second")
        .unwrap();
    let folder = organization.create_folder(&mut layers, "Tools").unwrap();
    organization
        .set_profile_application("Work", "org.example.App")
        .unwrap();
    for key in [1, 2] {
        bind_audio(&mut layers, &folder, Trigger::KeyPress(key), audio.clone()).unwrap();
    }
    layers
        .set_return(
            &folder,
            &Trigger::KeyPress(1),
            Some(PageReturn {
                layer: page.clone(),
                seconds: 17,
            }),
        )
        .unwrap();
    layers
        .set_info(&folder, &Trigger::KeyPress(1), Some(InfoSource::Weather))
        .unwrap();
    let command = Action::Command("printf 'never execute me'".into());
    layers
        .bind(&folder, Trigger::KeyPress(3), command.clone())
        .unwrap();
    let launch = Action::Launch {
        executable: "/external/application".into(),
        args: vec!["--flag".into()],
    };
    layers
        .bind("Base", Trigger::KeyPress(4), launch.clone())
        .unwrap();
    let open = Action::Feature(FeatureAction::Desktop(DesktopAction::Open {
        target: "/external/document".into(),
    }));
    layers
        .bind("Base", Trigger::KeyPress(5), open.clone())
        .unwrap();
    layers.persist().unwrap();
    organization.persist().unwrap();

    let image = f.0.join("image.png");
    let pixbuf = Pixbuf::new(Colorspace::Rgb, false, 8, 2, 2).unwrap();
    pixbuf.fill(0x336699ff);
    pixbuf.savev(&image, "png", &[]).unwrap();
    let mut artwork = ArtworkStore::open_at_for_layer(&original, Some(&folder)).unwrap();
    let asset = artwork
        .assign_in_layer(&folder, ArtworkTarget::Keypad(1), &image)
        .unwrap();
    let text = ButtonText {
        text: "Weather".into(),
        ..ButtonText::default()
    };
    artwork
        .set_button_text_in_layer(&folder, ArtworkTarget::Keypad(1), text.clone())
        .unwrap();
    let obs = ObsConfig {
        password: "credential fixture".into(),
        ..ObsConfig::default()
    };
    obs.save_at(&original).unwrap();
    crate::info::save_location(&original, "Buenos Aires").unwrap();
    write_private(&original.join("unrelated.txt"), b"excluded").unwrap();
    write_private(&original.join("layers.conf.tmp"), b"excluded").unwrap();
    write_private(&original.join("assets/unrelated.txt"), b"excluded").unwrap();
    let before = fs::read(original.join("layers.conf")).unwrap();
    let bundle = f.0.join("portable.sdeck-plus.json");
    export(&original, &bundle).unwrap();
    assert_private(&bundle, 0o600);
    assert_eq!(fs::read(original.join("layers.conf")).unwrap(), before);
    let exported: serde_json::Value = serde_json::from_slice(&fs::read(&bundle).unwrap()).unwrap();
    let image_path = format!("assets/{}", asset.file_name().unwrap().to_str().unwrap());
    let image_entry = exported["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == image_path)
        .unwrap();
    assert!(
        image_entry.get("data").is_none(),
        "exported images must contain only their filename"
    );
    let files = contents(&bundle);
    assert_eq!(
        files
            .keys()
            .filter(|path| path.starts_with("media/"))
            .count(),
        1
    );
    assert!(!files
        .keys()
        .any(|path| path.contains("unrelated") || path.ends_with(".tmp")));
    assert!(!files.keys().any(|path| path.starts_with("assets/")));
    let portable_layers = String::from_utf8(files["layers.conf"].clone()).unwrap();
    assert!(!portable_layers.contains(original.to_str().unwrap()));

    let destination = f.root("installed");
    write_private(&destination.join("previous-marker"), b"old configuration").unwrap();
    private_dir(&destination.join("assets")).unwrap();
    write_private(&destination.join(&image_path), &fs::read(&asset).unwrap()).unwrap();
    let backup = import(&bundle, &destination).unwrap().unwrap();
    assert_eq!(
        fs::read(backup.join("previous-marker")).unwrap(),
        b"old configuration"
    );
    assert!(!destination.join("previous-marker").exists());
    let imported = LayerStore::open_at(&destination).unwrap();
    assert_eq!(imported.active(), folder);
    assert_eq!(imported.names(), layers.names());
    assert_eq!(
        imported.page_return(&folder, &Trigger::KeyPress(1)),
        Some(PageReturn {
            layer: page,
            seconds: 17
        })
    );
    assert_eq!(
        imported.info_source(&folder, &Trigger::KeyPress(1)),
        Some(InfoSource::Weather)
    );
    assert_eq!(
        imported.layers()[&folder].bindings[&Trigger::KeyPress(3)],
        command
    );
    assert_eq!(
        imported.layers()["Base"].bindings[&Trigger::KeyPress(4)],
        launch
    );
    assert_eq!(
        imported.layers()["Base"].bindings[&Trigger::KeyPress(5)],
        open
    );
    for (_, _, path) in audio_bindings(&imported) {
        assert!(path.starts_with(destination.join("media")));
        assert_eq!(fs::read(path).unwrap(), b"RIFF isolated audio fixture");
    }
    assert_eq!(
        fs::read(destination.join("organization.json")).unwrap(),
        fs::read(original.join("organization.json")).unwrap()
    );
    let imported_artwork = ArtworkStore::open_at_for_layer(&destination, Some(&folder)).unwrap();
    assert_eq!(
        imported_artwork.button_text_in_layer(&folder, ArtworkTarget::Keypad(1)),
        Some(&text)
    );
    assert_eq!(ObsConfig::load_at(&destination).unwrap(), obs);
    assert_eq!(
        crate::info::load_location(&destination).as_deref(),
        Some("Buenos Aires")
    );
    assert_private(&destination, 0o700);
    for name in CONFIGS {
        assert_private(&destination.join(name), 0o600);
    }
    for dir in ["assets", "media"] {
        assert_private(&destination.join(dir), 0o700);
        for entry in fs::read_dir(destination.join(dir)).unwrap() {
            assert_private(&entry.unwrap().path(), 0o600);
        }
    }
    // A second export must portable-ize paths already inside the installed root.
    let again = f.0.join("again.sdeck-plus.json");
    export(&destination, &again).unwrap();
    assert_eq!(contents(&again), files);
}

#[test]
fn referenced_images_require_valid_local_assets_before_replacement() {
    let f = Fixture::new();
    let source = f.root("source");
    let image = f.0.join("image.png");
    Pixbuf::new(Colorspace::Rgb, false, 8, 1, 1)
        .unwrap()
        .savev(&image, "png", &[])
        .unwrap();
    let asset = ArtworkStore::open_at_for_layer(&source, Some("Base"))
        .unwrap()
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &image)
        .unwrap();
    let bundle = f.0.join("references.sdeck-plus.json");
    export(&source, &bundle).unwrap();
    let destination = f.root("installed");
    private_dir(&destination.join("assets")).unwrap();
    let local = destination.join("assets").join(asset.file_name().unwrap());
    let before = fs::read(destination.join("layers.conf")).unwrap();

    assert!(import(&bundle, &destination).is_err());
    assert_eq!(fs::read(destination.join("layers.conf")).unwrap(), before);
    write_private(&local, b"not an image").unwrap();
    assert!(import(&bundle, &destination).is_err());
    assert_eq!(fs::read(destination.join("layers.conf")).unwrap(), before);
    fs::remove_file(&local).unwrap();
    symlink(&asset, &local).unwrap();
    assert!(import(&bundle, &destination).is_err());
    assert_eq!(fs::read(destination.join("layers.conf")).unwrap(), before);
    assert!(!fs::read_dir(&f.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".stage-")
    }));
}

#[test]
fn version_one_embedded_images_import_without_local_assets() {
    let f = Fixture::new();
    let source = f.root("source");
    let image = f.0.join("image.png");
    Pixbuf::new(Colorspace::Rgb, false, 8, 1, 1)
        .unwrap()
        .savev(&image, "png", &[])
        .unwrap();
    let asset = ArtworkStore::open_at_for_layer(&source, Some("Base"))
        .unwrap()
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &image)
        .unwrap();
    let name = format!("assets/{}", asset.file_name().unwrap().to_str().unwrap());
    let manifest = fs::read(source.join("artwork.conf")).unwrap();
    let bytes = fs::read(&asset).unwrap();
    let bundle = f.bundle(&[
        ("layers.conf", LAYERS),
        ("artwork.conf", &manifest),
        (&name, &bytes),
    ]);
    let destination = f.0.join("installed");

    assert!(import(&bundle, &destination).unwrap().is_none());
    let imported = ArtworkStore::open_at_for_layer(&destination, Some("Base")).unwrap();
    let restored = imported
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(1))
        .unwrap();
    assert_eq!(fs::read(restored).unwrap(), bytes);
}

#[test]
fn first_install_and_extensionless_audio_are_supported() {
    let f = Fixture::new();
    let source = f.root("source");
    let audio = f.0.join("extensionless");
    write_private(&audio, b"audio").unwrap();
    let mut layers = LayerStore::open_at(&source).unwrap();
    bind_audio(&mut layers, "Base", Trigger::KeyPress(1), audio).unwrap();
    layers.persist().unwrap();
    let bundle = f.0.join("bundle.sdeck-plus.json");
    export(&source, &bundle).unwrap();
    assert!(contents(&bundle)
        .keys()
        .any(|path| path.starts_with("media/") && path.ends_with(".bin")));
    assert!(import(&bundle, &f.0.join("new")).unwrap().is_none());
}

#[test]
fn rejects_paths_duplicates_versions_and_bad_base64_without_replacement() {
    let f = Fixture::new();
    let root = f.root("existing");
    let before = fs::read(root.join("layers.conf")).unwrap();
    for path in [
        "/escape",
        "../escape",
        "assets/../escape",
        "assets/a/b",
        "media/../../escape",
        "assets/..",
        "assets/\\escape",
        "C:\\escape",
        "unknown.json",
    ] {
        let bundle = f.bundle(&[("layers.conf", LAYERS), (path, b"bad")]);
        assert!(import(&bundle, &root).is_err(), "{path}");
    }
    let duplicate = f.bundle(&[("layers.conf", LAYERS), ("layers.conf", LAYERS)]);
    assert!(import(&duplicate, &root).is_err());
    for json in [
        r#"{"format":"sdeck-plus","version":3,"files":[]}"#,
        r#"{"format":"other","version":1,"files":[]}"#,
        r#"{"format":"sdeck-plus","version":1,"files":[{"path":"layers.conf","data":"!"}]}"#,
        r#"{"format":"sdeck-plus","version":1,"files":[],"unexpected":true}"#,
        r#"{"format":"sdeck-plus","version":2,"files":[{"path":"layers.conf"}]}"#,
        r#"{"format":"sdeck-plus","version":2,"files":[{"path":"assets/../escape"}]}"#,
    ] {
        let path = f.0.join(format!(
            "invalid-{}",
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        write_private(&path, json.as_bytes()).unwrap();
        assert!(import(&path, &root).is_err());
    }
    assert_eq!(fs::read(root.join("layers.conf")).unwrap(), before);
    assert_eq!(
        fs::read_dir(&f.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".stage-"))
            .count(),
        0
    );
}

#[test]
fn rejects_missing_layers_diagnostics_and_recovered_conflicts() {
    let f = Fixture::new();
    let root = f.root("existing");
    for data in [
        b"bad version\n".as_slice(),
        b"streamdeck-layers-v1\nbad row\n",
    ] {
        let bundle = f.bundle(&[("layers.conf", data)]);
        assert!(import(&bundle, &root).is_err());
    }
    let empty = f.bundle(&[]);
    assert!(import(&empty, &root).is_err());
    assert_eq!(fs::read(root.join("layers.conf")).unwrap(), LAYERS);
    assert!(!root.join("conflicts.log").exists());
}

#[test]
fn rejects_malformed_optional_settings_and_missing_or_invalid_images() {
    let f = Fixture::new();
    let root = f.root("existing");
    for (name, data) in [
        ("organization.json", b"{}".as_slice()),
        ("obs.json", b"{}"),
        (
            "obs.json",
            br#"{"endpoint":"ws://remote.example:4455","password":"secret"}"#,
        ),
        ("info.json", br#"{"location":""}"#),
        ("info.json", br#"{"location":42}"#),
        ("info.json", br#"{"location":"ok","other":1}"#),
        (
            "artwork.conf",
            b"streamdeck-artwork-v2\n42617365\tkey-1\tabsent.png\n",
        ),
    ] {
        let bundle = f.bundle(&[("layers.conf", LAYERS), (name, data)]);
        assert!(import(&bundle, &root).is_err(), "{name}");
    }
    // Use a store-written manifest to ensure this exercises actual decoding.
    let source = f.root("source");
    let image = f.0.join("valid.png");
    Pixbuf::new(Colorspace::Rgb, false, 8, 1, 1)
        .unwrap()
        .savev(&image, "png", &[])
        .unwrap();
    let asset = ArtworkStore::open_at_for_layer(&source, Some("Base"))
        .unwrap()
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &image)
        .unwrap();
    let manifest = fs::read(source.join("artwork.conf")).unwrap();
    let asset_path = format!("assets/{}", asset.file_name().unwrap().to_str().unwrap());
    let missing = f.bundle(&[("layers.conf", LAYERS), ("artwork.conf", &manifest)]);
    assert!(import(&missing, &root).is_err());
    let invalid = f.bundle(&[
        ("layers.conf", LAYERS),
        ("artwork.conf", &manifest),
        (&asset_path, b"not an image"),
    ]);
    assert!(import(&invalid, &root).is_err());
}

#[test]
fn rejects_missing_or_external_import_audio() {
    let f = Fixture::new();
    for audio in [
        "media/absent.wav",
        "/external/sound.wav",
        "media/../sound.wav",
    ] {
        let source = f.root(&format!(
            "source-{}",
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut layers = LayerStore::open_at(&source).unwrap();
        bind_audio(&mut layers, "Base", Trigger::KeyPress(1), audio.into()).unwrap();
        layers.persist().unwrap();
        let bytes = fs::read(source.join("layers.conf")).unwrap();
        let bundle = f.bundle(&[("layers.conf", &bytes)]);
        assert!(import(&bundle, &f.0.join("destination")).is_err());
        assert!(!f.0.join("destination").exists());
    }
}

#[test]
fn export_rejects_symlinks_and_nonregular_inputs_and_preserves_destination() {
    let f = Fixture::new();
    let source = f.root("source");
    let destination = f.0.join("export.sdeck-plus.json");
    write_private(&destination, b"previous export").unwrap();
    let external = f.0.join("external");
    write_private(&external, b"external").unwrap();
    symlink(&external, source.join("obs.json")).unwrap();
    assert!(export(&source, &destination).is_err());
    fs::remove_file(source.join("obs.json")).unwrap();
    private_dir(&source.join("obs.json")).unwrap();
    assert!(export(&source, &destination).is_err());
    fs::remove_dir(source.join("obs.json")).unwrap();
    let mut layers = LayerStore::open_at(&source).unwrap();
    let audio = f.0.join("linked.wav");
    symlink(&external, &audio).unwrap();
    bind_audio(&mut layers, "Base", Trigger::KeyPress(1), audio).unwrap();
    layers.persist().unwrap();
    assert!(export(&source, &destination).is_err());
    let linked_directory = f.0.join("linked-directory");
    symlink(&source, &linked_directory).unwrap();
    bind_audio(
        &mut layers,
        "Base",
        Trigger::KeyPress(1),
        linked_directory.join("../external"),
    )
    .unwrap();
    layers.persist().unwrap();
    assert!(export(&source, &destination).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"previous export");
}

#[test]
fn export_validation_failure_does_not_repair_original_layers() {
    let f = Fixture::new();
    let source = f.root("source");
    fs::write(
        source.join("layers.conf"),
        b"streamdeck-layers-v1\nbad row\n",
    )
    .unwrap();
    let before = fs::read(source.join("layers.conf")).unwrap();
    let destination = f.0.join("out");
    assert!(export(&source, &destination).is_err());
    assert!(!destination.exists());
    assert_eq!(fs::read(source.join("layers.conf")).unwrap(), before);
    assert_eq!(fs::read_dir(&source).unwrap().count(), 1);
}

#[test]
fn enforces_file_entry_and_archive_size_limits() {
    let f = Fixture::new();
    let root = f.root("source");
    let oversized = f.0.join("oversized.sdeck-plus.json");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&oversized)
        .unwrap();
    file.set_len(MAX_BUNDLE as u64 + 1).unwrap();
    assert!(import(&oversized, &root).is_err());
    let config = OpenOptions::new()
        .write(true)
        .open(root.join("layers.conf"))
        .unwrap();
    config.set_len(MAX_CONFIG as u64 + 1).unwrap();
    assert!(export(&root, &f.0.join("out")).is_err());
    let mut files = BTreeMap::new();
    for index in 0..MAX_ENTRIES {
        add(&mut files, format!("media/{index}.wav"), Vec::new()).unwrap();
    }
    assert!(add(&mut files, "media/extra.wav".into(), Vec::new()).is_err());
    assert!(add(
        &mut BTreeMap::new(),
        "layers.conf".into(),
        vec![0; MAX_CONFIG + 1]
    )
    .is_err());
}

#[test]
fn install_failure_restores_old_root_and_keeps_stage_available() {
    let f = Fixture::new();
    let root = f.root("root");
    let stage = f.root("stage");
    let original = fs::read(root.join("layers.conf")).unwrap();
    let mut calls = 0;
    let error = install(&stage, &root, |from, to| {
        calls += 1;
        if calls == 2 {
            Err(io::Error::other("injected install failure"))
        } else {
            fs::rename(from, to)
        }
    })
    .unwrap_err();
    assert!(error.to_string().contains("injected install failure"));
    assert_eq!(calls, 3);
    assert_eq!(fs::read(root.join("layers.conf")).unwrap(), original);
    assert!(stage.exists());
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 2);
}

#[test]
fn rollback_failure_reports_retained_backup_for_recovery() {
    let f = Fixture::new();
    let root = f.root("root");
    let stage = f.root("stage");
    let mut calls = 0;
    let error = install(&stage, &root, |from, to| {
        calls += 1;
        if calls >= 2 {
            Err(io::Error::other("injected rename failure"))
        } else {
            fs::rename(from, to)
        }
    })
    .unwrap_err();
    let backup = fs::read_dir(&f.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".backup-")
        })
        .unwrap();
    assert!(backup.join("layers.conf").is_file());
    assert!(error.to_string().contains(backup.to_str().unwrap()));
    assert!(error.to_string().contains("rollback failed"));
    assert!(!root.exists());
}

#[test]
fn failed_export_rename_cleans_exclusive_temporary_file() {
    let f = Fixture::new();
    let destination = f.0.join("directory");
    private_dir(&destination).unwrap();
    assert!(atomic_export(&destination, b"bundle").is_err());
    assert!(destination.is_dir());
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}

#[test]
fn touchscreen_pages_and_unselected_audio_roundtrip_with_independent_owners() {
    let fixture = Fixture::new();
    let source = fixture.root("touch-source");
    let mut layers = LayerStore::open_at(&source).unwrap();
    let mut organization = Organization::open_at(&source, &layers).unwrap();
    let keypad = organization.create_page(&mut layers).unwrap();
    let mut artwork = ArtworkStore::open_at_for_layer(&source, Some("Base")).unwrap();
    layers
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    let first = layers.touchscreen_owner("Base").unwrap().to_owned();
    layers
        .bind("Base", Trigger::ZoneTap(1), Action::Emit("first".into()))
        .unwrap();
    let second = layers.create_touchscreen_page("Base").unwrap();
    let audio = fixture.0.join("touch.wav");
    fs::write(&audio, b"portable touch audio").unwrap();
    layers
        .bind(
            "Base",
            Trigger::ZoneTap(1),
            Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio {
                path: audio,
            })),
        )
        .unwrap();
    layers.select_touchscreen_page("Base", &first);
    layers.persist().unwrap();
    organization.persist().unwrap();
    let bundle = fixture.0.join("touch.sdeck-plus.json");
    export(&source, &bundle).unwrap();
    assert!(contents(&bundle).contains_key("touchscreen.json"));
    let target = fixture.0.join("touch-target");
    import(&bundle, &target).unwrap();
    let mut restored = LayerStore::open_at(&target).unwrap();
    let organization = Organization::open_at(&target, &restored).unwrap();
    let mut artwork = ArtworkStore::open_at_for_layer(&target, Some(restored.active())).unwrap();
    restored
        .initialize_touchscreens(&organization, &mut artwork)
        .unwrap();
    assert_eq!(
        restored.touchscreen_pages("Base"),
        vec![first.clone(), second.clone()]
    );
    assert_eq!(restored.active(), keypad);
    assert_eq!(restored.touchscreen_owner("Base"), Some(first.as_str()));
    assert_eq!(
        restored.binding("Base", &Trigger::ZoneTap(1)),
        Some(Action::Emit("first".into()))
    );
    restored.select_touchscreen_page("Base", &second);
    let Some(Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path }))) =
        restored.binding("Base", &Trigger::ZoneTap(1))
    else {
        panic!("touch audio missing")
    };
    assert!(path.starts_with(target.join("media")));
    assert_eq!(fs::read(path).unwrap(), b"portable touch audio");
}
