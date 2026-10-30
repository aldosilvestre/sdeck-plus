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
    assert_eq!(bundle.version, 4);
    bundle
        .files
        .into_iter()
        .filter_map(|entry| entry.data.map(|data| (entry.path, data.into_bytes())))
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
fn audio_and_files_behind_app_links_export_only_relative_references() {
    use crate::{
        asset_links::link_action, desktop_actions::DesktopAction, features::FeatureAction,
    };
    let f = Fixture::new();
    let root = f.root("source");
    let audio = f.0.join("linked sound.wav");
    write_private(&audio, b"linked audio").unwrap();
    let notes = f.0.join("notes.txt");
    write_private(&notes, b"notes").unwrap();
    let as_action = |action| Action::Feature(FeatureAction::Desktop(action));
    let sound = link_action(
        &root,
        as_action(DesktopAction::PlayAudio {
            path: audio.clone(),
        }),
    )
    .unwrap();
    let open = link_action(
        &root,
        as_action(DesktopAction::Open {
            target: notes.to_string_lossy().into_owned(),
        }),
    )
    .unwrap();
    let mut layers = LayerStore::open_at(&root).unwrap();
    layers
        .bind_configuration("Base", Trigger::KeyPress(1), sound)
        .unwrap();
    layers
        .bind_configuration("Base", Trigger::KeyPress(2), open)
        .unwrap();
    layers.persist().unwrap();
    let bundle = f.0.join("links.sdeck-plus.json");
    export(&root, &bundle).unwrap();
    let files = contents(&bundle);
    assert!(files.keys().all(|path| CONFIGS.contains(&path.as_str())));
    let exported = String::from_utf8_lossy(&files["layers.conf"]).into_owned();
    assert!(
        !exported.contains("links/"),
        "no reference into the data directory: {exported}"
    );
    // Only a type folder and basename survive on the other machine.
    let imported = f.root("elsewhere");
    import(&bundle, &imported).unwrap();
    let store = LayerStore::open_at(&imported).unwrap();
    assert_eq!(
        store.binding("Base", &Trigger::KeyPress(2)),
        Some(as_action(DesktopAction::Open {
            target: imported
                .join("archivos/notes.txt")
                .to_string_lossy()
                .into_owned()
        }))
    );
}

#[test]
fn relative_audio_exports_only_its_filename() {
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
    assert!(files.keys().all(|path| CONFIGS.contains(&path.as_str())));
    assert!(!files.values().any(|bytes| bytes == &expected));
    let destination = f.0.join("destination");
    import(&bundle, &destination).unwrap();
    let restored = LayerStore::open_at(&destination).unwrap();
    for (_, _, path) in audio_bindings(&restored) {
        assert_eq!(path, destination.join("sonidos/Cargo.toml"));
        assert!(!path.exists());
    }
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
    let image_path = format!("imagenes/{}", asset.file_name().unwrap().to_str().unwrap());
    assert_eq!(exported["version"], 4);
    let files = contents(&bundle);
    assert!(files.keys().all(|path| CONFIGS.contains(&path.as_str())));
    assert!(!files
        .keys()
        .any(|path| path.contains("unrelated") || path.ends_with(".tmp")));
    assert!(!files.keys().any(|path| path.starts_with("assets/")));
    let portable_layers = String::from_utf8(files["layers.conf"].clone()).unwrap();
    assert!(!portable_layers.contains(original.to_str().unwrap()));

    let destination = f.root("installed");
    write_private(&destination.join("previous-marker"), b"old configuration").unwrap();
    private_dir(&destination.join("imagenes")).unwrap();
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
        Action::Launch {
            executable: destination.join("archivos/application"),
            args: vec!["--flag".into()]
        }
    );
    assert_eq!(
        imported.layers()["Base"].bindings[&Trigger::KeyPress(5)],
        Action::Feature(FeatureAction::Desktop(DesktopAction::Open {
            target: destination
                .join("archivos/document")
                .to_string_lossy()
                .into_owned()
        }))
    );
    for (_, _, path) in audio_bindings(&imported) {
        assert_eq!(path, destination.join("sonidos/sound.WAV"));
        assert!(!path.exists());
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
fn referenced_images_allow_missing_files_and_preserve_existing_local_images() {
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
    import(&bundle, &destination).unwrap();
    let artwork = ArtworkStore::open_at_for_layer(&destination, Some("Base")).unwrap();
    let local = artwork
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(1))
        .unwrap();
    assert!(local.starts_with(destination.join("imagenes")));
    assert!(!local.exists());
    let before = fs::read(destination.join("layers.conf")).unwrap();
    write_private(&local, b"not an image").unwrap();
    assert!(import(&bundle, &destination).is_err());
    assert_eq!(fs::read(destination.join("layers.conf")).unwrap(), before);
    assert_eq!(fs::read(&local).unwrap(), b"not an image");
    fs::remove_file(&local).unwrap();
    symlink(&asset, &local).unwrap();
    import(&bundle, &destination).unwrap();
    assert_eq!(fs::read_link(&local).unwrap(), asset);
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
        .all(|path| CONFIGS.contains(&path.as_str())));
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
fn export_rejects_nonregular_configs_but_never_reads_sound_targets() {
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
    assert_eq!(fs::read(&destination).unwrap(), b"previous export");
    let mut layers = LayerStore::open_at(&source).unwrap();
    let audio = f.0.join("linked.wav");
    symlink(&external, &audio).unwrap();
    bind_audio(&mut layers, "Base", Trigger::KeyPress(1), audio).unwrap();
    layers.persist().unwrap();
    export(&source, &destination).unwrap();
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
    export(&source, &destination).unwrap();
    assert!(contents(&destination)
        .keys()
        .all(|path| CONFIGS.contains(&path.as_str())));
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
    layers
        .bind("Base", Trigger::EncoderPress(4), Action::None)
        .unwrap();
    let second = layers.create_touchscreen_page("Base").unwrap();
    let audio = fixture.0.join("touch.wav");
    fs::write(&audio, b"portable touch audio").unwrap();
    layers
        .bind(
            "Base",
            Trigger::ZoneTap(1),
            Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio {
                path: audio.clone(),
            })),
        )
        .unwrap();
    layers
        .bind(
            "Base",
            Trigger::EncoderClockwise(4),
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
    assert_eq!(
        restored.binding("Base", &Trigger::EncoderPress(4)),
        Some(Action::None)
    );
    restored.select_touchscreen_page("Base", &second);
    let Some(Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path }))) =
        restored.binding("Base", &Trigger::ZoneTap(1))
    else {
        panic!("touch audio missing")
    };
    assert_eq!(path, target.join("sonidos/touch.wav"));
    assert!(!path.exists());
    let Some(Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path }))) =
        restored.binding("Base", &Trigger::EncoderClockwise(4))
    else {
        panic!("dial audio missing")
    };
    assert_eq!(path, target.join("sonidos/touch.wav"));
    assert!(!path.exists());
}

#[test]
fn metadata_export_uses_type_folders_without_contents_or_parent_paths() {
    let f = Fixture::new();
    let root = f.root("source");
    let mut layers = LayerStore::open_at(&root).unwrap();
    let action = |a| Action::Feature(FeatureAction::Desktop(a));
    let sound = crate::asset_links::link_action(
        &root,
        action(DesktopAction::PlayAudio {
            path: f.0.join("private/music/cancionX.mp3"),
        }),
    )
    .unwrap();
    layers
        .bind_configuration("Base", Trigger::KeyPress(1), sound)
        .unwrap();
    layers
        .bind_configuration(
            "Base",
            Trigger::KeyPress(2),
            action(DesktopAction::Open {
                target: f
                    .0
                    .join("private/documents/notes.txt")
                    .to_string_lossy()
                    .into_owned(),
            }),
        )
        .unwrap();
    layers
        .bind_configuration(
            "Base",
            Trigger::KeyPress(3),
            Action::Launch {
                executable: f.0.join("private/bin/tool"),
                args: vec!["--flag".into()],
            },
        )
        .unwrap();
    layers.persist().unwrap();
    let before = fs::read(root.join("layers.conf")).unwrap();
    // A portable image assignment remains valid even before its local file exists.
    write_private(
        &root.join("artwork.conf"),
        b"streamdeck-artwork-v3\n42617365\tkeypad-1\t./imagenes/picture.png\n",
    )
    .unwrap();
    let bundle = f.0.join("references.json");
    export(&root, &bundle).unwrap();
    let envelope: Bundle = serde_json::from_slice(&fs::read(&bundle).unwrap()).unwrap();
    assert_eq!(envelope.version, 4);
    assert!(envelope
        .files
        .iter()
        .all(|entry| CONFIGS.contains(&entry.path.as_str())));
    let decoded: Vec<_> = envelope
        .files
        .iter()
        .map(|e| e.data.as_ref().unwrap().as_bytes().to_vec())
        .collect();
    assert!(decoded
        .iter()
        .all(|b| !String::from_utf8_lossy(b).contains(f.0.to_str().unwrap())));
    assert_eq!(fs::read(root.join("layers.conf")).unwrap(), before);
    let destination = f.root("destination");
    private_dir(&destination.join("sonidos")).unwrap();
    let local_sound = destination.join("sonidos/cancionX.mp3");
    symlink(f.0.join("local-only/original.mp3"), &local_sound).unwrap();
    let link_before = fs::read_link(&local_sound).unwrap();
    let backup = import(&bundle, &destination).unwrap().unwrap();
    assert!(backup.join("layers.conf").exists());
    assert_eq!(fs::read_link(&local_sound).unwrap(), link_before);
    let restored = LayerStore::open_at(&destination).unwrap();
    assert_eq!(
        restored.binding("Base", &Trigger::KeyPress(1)),
        Some(action(DesktopAction::PlayAudio {
            path: destination.join("sonidos/cancionX.mp3"),
        }))
    );
    assert_eq!(
        restored.binding("Base", &Trigger::KeyPress(2)),
        Some(action(DesktopAction::Open {
            target: destination
                .join("archivos/notes.txt")
                .to_string_lossy()
                .into_owned(),
        }))
    );
    assert_eq!(
        restored.binding("Base", &Trigger::KeyPress(3)),
        Some(Action::Launch {
            executable: destination.join("archivos/tool"),
            args: vec!["--flag".into()],
        })
    );
    let artwork = ArtworkStore::open_at_for_layer(&destination, Some("Base")).unwrap();
    assert!(artwork
        .assigned_path_in_layer("Base", ArtworkTarget::Keypad(1))
        .unwrap()
        .ends_with("imagenes/picture.png"));
    let again = f.0.join("again.json");
    export(&destination, &again).unwrap();
    assert_eq!(fs::read(&bundle).unwrap(), fs::read(&again).unwrap());
}

#[test]
fn export_rejects_ambiguous_basenames_without_changing_inputs_or_destination() {
    let f = Fixture::new();
    let root = f.root("source");
    let mut layers = LayerStore::open_at(&root).unwrap();
    for (key, folder) in [(1, "a"), (2, "b")] {
        bind_audio(
            &mut layers,
            "Base",
            Trigger::KeyPress(key),
            f.0.join(folder).join("same.mp3"),
        )
        .unwrap();
    }
    layers.persist().unwrap();
    let before = fs::read(root.join("layers.conf")).unwrap();
    let bundle = f.0.join("collision.json");
    write_private(&bundle, b"keep destination").unwrap();
    let error = export(&root, &bundle).unwrap_err();
    assert_eq!(
        error.to_string(),
        "multiple files map to ./sonidos/same.mp3; rename one before exporting"
    );
    assert_eq!(fs::read(&bundle).unwrap(), b"keep destination");
    assert_eq!(fs::read(root.join("layers.conf")).unwrap(), before);
}

#[test]
fn legacy_v2_embedded_audio_and_local_image_references_still_import() {
    let f = Fixture::new();
    let source = f.root("source");
    let image = f.0.join("legacy.png");
    Pixbuf::new(Colorspace::Rgb, false, 8, 1, 1)
        .unwrap()
        .savev(&image, "png", &[])
        .unwrap();
    let asset = ArtworkStore::open_at_for_layer(&source, Some("Base"))
        .unwrap()
        .assign_in_layer("Base", ArtworkTarget::Keypad(1), &image)
        .unwrap();
    let mut layers = LayerStore::open_at(&source).unwrap();
    bind_audio(
        &mut layers,
        "Base",
        Trigger::KeyPress(1),
        "media/old.mp3".into(),
    )
    .unwrap();
    layers.persist().unwrap();
    let image_entry = format!("assets/{}", asset.file_name().unwrap().to_str().unwrap());
    let entries = vec![
        Entry {
            path: "layers.conf".into(),
            data: Some(STANDARD.encode(fs::read(source.join("layers.conf")).unwrap())),
        },
        Entry {
            path: "artwork.conf".into(),
            data: Some(STANDARD.encode(fs::read(source.join("artwork.conf")).unwrap())),
        },
        Entry {
            path: image_entry.clone(),
            data: None,
        },
        Entry {
            path: "media/old.mp3".into(),
            data: Some(STANDARD.encode(b"legacy sound")),
        },
    ];
    let bundle = f.0.join("v2.json");
    write_private(
        &bundle,
        &serde_json::to_vec(&Bundle {
            format: "sdeck-plus".into(),
            version: 2,
            files: entries,
        })
        .unwrap(),
    )
    .unwrap();
    let target = f.root("target");
    let before = fs::read(target.join("layers.conf")).unwrap();
    assert!(import(&bundle, &target).is_err());
    assert_eq!(fs::read(target.join("layers.conf")).unwrap(), before);
    private_dir(&target.join("assets")).unwrap();
    write_private(&target.join(&image_entry), &fs::read(&asset).unwrap()).unwrap();
    import(&bundle, &target).unwrap();
    let store = LayerStore::open_at(&target).unwrap();
    assert_eq!(audio_bindings(&store)[0].2, target.join("media/old.mp3"));
    assert_eq!(
        fs::read(target.join("media/old.mp3")).unwrap(),
        b"legacy sound"
    );
    assert!(ArtworkStore::open_at_for_layer(&target, Some("Base"))
        .unwrap()
        .assigned_path(ArtworkTarget::Keypad(1))
        .unwrap()
        .is_file());
}

#[test]
fn v3_rejects_unsafe_references_payloads_and_symlinked_local_directories_without_data_loss() {
    let f = Fixture::new();
    let root = f.root("installed");
    private_dir(&root.join("assets")).unwrap();
    write_private(&root.join("assets/keep"), b"local data").unwrap();
    let before = fs::read(root.join("layers.conf")).unwrap();
    for bad in [
        "./sonidos/../escape.mp3",
        "/private/sound.mp3",
        "./archivos/wrong.mp3",
    ] {
        let source = f.root(&format!(
            "source-{}",
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut layers = LayerStore::open_at(&source).unwrap();
        bind_audio(&mut layers, "Base", Trigger::KeyPress(1), bad.into()).unwrap();
        layers.persist().unwrap();
        let bundle = Bundle {
            format: "sdeck-plus".into(),
            version: 3,
            files: vec![Entry {
                path: "layers.conf".into(),
                data: Some(STANDARD.encode(fs::read(source.join("layers.conf")).unwrap())),
            }],
        };
        let path = f.0.join("malformed.json");
        fs::write(&path, serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert!(import(&path, &root).is_err());
        assert_eq!(fs::read(root.join("layers.conf")).unwrap(), before);
        assert_eq!(fs::read(root.join("assets/keep")).unwrap(), b"local data");
    }
    let source = f.root("valid-source");
    let bundle = f.0.join("valid.json");
    export(&source, &bundle).unwrap();
    symlink(&f.0, root.join("sonidos")).unwrap();
    assert!(import(&bundle, &root).is_err());
    assert_eq!(fs::read(root.join("layers.conf")).unwrap(), before);
    assert_eq!(fs::read(root.join("assets/keep")).unwrap(), b"local data");
    assert_eq!(fs::read_link(root.join("sonidos")).unwrap(), f.0);
    let mut payload: Bundle = serde_json::from_slice(&fs::read(&bundle).unwrap()).unwrap();
    payload.files.push(Entry {
        path: "media/secret.mp3".into(),
        data: Some(STANDARD.encode(b"forbidden")),
    });
    fs::write(&bundle, serde_json::to_vec(&payload).unwrap()).unwrap();
    assert!(import(&bundle, &root).is_err());
    assert!(!root.join("media/secret.mp3").exists());
    assert_eq!(fs::read(root.join("layers.conf")).unwrap(), before);
}

#[test]
fn background_images_export_as_references_and_all_configuration_is_plain_text() {
    let f = Fixture::new();
    let root = f.root("source");
    let image = f.0.join("background.png");
    let pixbuf = Pixbuf::new(Colorspace::Rgb, false, 8, 2, 2).unwrap();
    pixbuf.fill(0x55aa33ff);
    pixbuf.savev(&image, "png", &[]).unwrap();
    let image_bytes = fs::read(&image).unwrap();
    let mut artwork = ArtworkStore::open_at_for_layer(&root, Some("Base")).unwrap();
    let targets = [
        ArtworkTarget::Keypad(1),
        ArtworkTarget::TouchStripBackground,
        ArtworkTarget::EncoderZone(1),
    ];
    let names: Vec<_> = targets
        .into_iter()
        .map(|target| {
            artwork
                .assign_in_layer("Base", target, &image)
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let bundle = f.0.join("backgrounds.json");
    export(&root, &bundle).unwrap();
    let serialized = fs::read(&bundle).unwrap();
    let envelope: Bundle = serde_json::from_slice(&serialized).unwrap();
    assert!(envelope
        .files
        .iter()
        .all(|entry| CONFIGS.contains(&entry.path.as_str())));
    let manifest = envelope
        .files
        .iter()
        .find(|entry| entry.path == "artwork.conf")
        .unwrap()
        .data
        .as_deref()
        .unwrap();
    assert!(manifest.starts_with("streamdeck-artwork-v3\n"));
    assert_eq!(envelope.version, 4);
    for name in &names {
        assert!(manifest.contains(&format!("./imagenes/{name}")));
    }
    assert!(!String::from_utf8_lossy(&serialized).contains(&STANDARD.encode(&image_bytes)));
    assert!(!String::from_utf8_lossy(&serialized).contains(image.to_str().unwrap()));
    for entry in &envelope.files {
        let text = entry.data.as_deref().unwrap();
        if entry.path.ends_with(".json") {
            serde_json::from_str::<serde_json::Value>(text).unwrap();
        } else {
            assert!(
                text.starts_with("streamdeck-"),
                "configuration must be raw text: {}",
                entry.path
            );
        }
    }
    let target = f.0.join("target");
    import(&bundle, &target).unwrap();
    let mut layers = LayerStore::open_at(&target).unwrap();
    let organization = Organization::open_at(&target, &layers).unwrap();
    let mut restored = ArtworkStore::open_at_for_layer(&target, Some("Base")).unwrap();
    layers
        .initialize_touchscreens(&organization, &mut restored)
        .unwrap();
    let touchscreen = layers.touchscreen_owner("Base").unwrap();
    for (control, name) in targets.into_iter().zip(names) {
        let owner = if matches!(control, ArtworkTarget::Keypad(_)) {
            "Base"
        } else {
            touchscreen
        };
        assert_eq!(
            restored.assigned_path_in_layer(owner, control),
            Some(target.join("imagenes").join(name))
        );
        assert!(!restored
            .assigned_path_in_layer(owner, control)
            .unwrap()
            .exists());
    }
}

#[test]
fn legacy_v3_base64_configuration_still_imports_relative_file_references() {
    let f = Fixture::new();
    let root = f.root("source");
    let mut layers = LayerStore::open_at(&root).unwrap();
    bind_audio(
        &mut layers,
        "Base",
        Trigger::KeyPress(1),
        f.0.join("private/song.mp3"),
    )
    .unwrap();
    layers.persist().unwrap();
    let path = f.0.join("legacy-v3.json");
    export(&root, &path).unwrap();
    let mut bundle: Bundle = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    bundle.version = 3;
    for entry in &mut bundle.files {
        entry.data = Some(STANDARD.encode(entry.data.take().unwrap().as_bytes()));
    }
    fs::write(&path, serde_json::to_vec(&bundle).unwrap()).unwrap();
    let destination = f.0.join("destination");
    import(&path, &destination).unwrap();
    let restored = LayerStore::open_at(&destination).unwrap();
    assert_eq!(
        audio_bindings(&restored)[0].2,
        destination.join("sonidos/song.mp3")
    );
    assert!(!destination.join("sonidos/song.mp3").exists());
}
