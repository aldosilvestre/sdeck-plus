use super::*;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::PathBuf,
};

fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "asset-links-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn sound(path: &Path) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio {
        path: path.to_path_buf(),
    }))
}

fn open(target: &str) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Open {
        target: target.into(),
    }))
}

fn sound_path(action: &Action) -> PathBuf {
    match action {
        Action::Feature(FeatureAction::Desktop(DesktopAction::PlayAudio { path })) => path.clone(),
        other => panic!("not a sound: {other:?}"),
    }
}

fn open_target(action: &Action) -> String {
    match action {
        Action::Feature(FeatureAction::Desktop(DesktopAction::Open { target })) => target.clone(),
        other => panic!("not an open action: {other:?}"),
    }
}

#[test]
fn linking_a_sound_replaces_its_path_with_a_symlink_to_the_original() {
    let root = temp_root("sound");
    let original = root.join("my track.ogg");
    fs::write(&original, b"audio").unwrap();
    let linked = sound_path(&link_action(&root, sound(&original)).unwrap());
    assert!(linked.starts_with(root.join("links")), "{linked:?}");
    assert!(fs::symlink_metadata(&linked)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_link(&linked).unwrap(), original);
    assert_eq!(linked.extension().and_then(|e| e.to_str()), Some("ogg"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn linking_the_same_file_twice_reuses_one_link() {
    let root = temp_root("reuse");
    let original = root.join("a.wav");
    fs::write(&original, b"audio").unwrap();
    let first = sound_path(&link_action(&root, sound(&original)).unwrap());
    let second = sound_path(&link_action(&root, sound(&original)).unwrap());
    assert_eq!(first, second);
    assert_eq!(fs::read_dir(root.join("links")).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn different_files_with_the_same_name_get_different_links() {
    let root = temp_root("collide");
    let (one, two) = (root.join("one"), root.join("two"));
    fs::create_dir_all(&one).unwrap();
    fs::create_dir_all(&two).unwrap();
    fs::write(one.join("beep.wav"), b"1").unwrap();
    fs::write(two.join("beep.wav"), b"2").unwrap();
    let first = sound_path(&link_action(&root, sound(&one.join("beep.wav"))).unwrap());
    let second = sound_path(&link_action(&root, sound(&two.join("beep.wav"))).unwrap());
    assert_ne!(first, second);
    assert_eq!(fs::read_link(&first).unwrap(), one.join("beep.wav"));
    assert_eq!(fs::read_link(&second).unwrap(), two.join("beep.wav"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn open_targets_that_are_absolute_paths_are_linked_files_and_folders_alike() {
    let root = temp_root("open");
    let file = root.join("notes.txt");
    let folder = root.join("docs");
    fs::write(&file, b"x").unwrap();
    fs::create_dir_all(&folder).unwrap();
    for target in [&file, &folder] {
        let linked = open_target(&link_action(&root, open(target.to_str().unwrap())).unwrap());
        assert!(
            Path::new(&linked).starts_with(root.join("links")),
            "{linked}"
        );
        assert_eq!(fs::read_link(&linked).unwrap(), *target);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn text_that_is_not_an_absolute_path_and_other_actions_are_left_alone() {
    let root = temp_root("untouched");
    for action in [
        open("relative/file.txt"),
        open("~/file.txt"),
        open(""),
        Action::Command("echo hi".into()),
        Action::None,
    ] {
        assert_eq!(link_action(&root, action.clone()).unwrap(), action);
    }
    assert!(
        !root.join("links").exists(),
        "nothing to link, nothing created"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_action_already_pointing_into_links_is_not_linked_again() {
    let root = temp_root("again");
    let original = root.join("a.mp3");
    fs::write(&original, b"audio").unwrap();
    let linked = link_action(&root, sound(&original)).unwrap();
    assert_eq!(link_action(&root, linked.clone()).unwrap(), linked);
    assert_eq!(fs::read_dir(root.join("links")).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unlinking_shows_the_real_target_again() {
    let root = temp_root("unlink");
    let original = root.join("a.flac");
    fs::write(&original, b"audio").unwrap();
    let linked = link_action(&root, sound(&original)).unwrap();
    assert_eq!(unlink_action(&root, &linked), sound(&original));
    // A path that is not one of our links is shown as it is.
    assert_eq!(unlink_action(&root, &sound(&original)), sound(&original));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_deleted_original_is_reported_missing_through_its_link() {
    let root = temp_root("missing");
    let original = root.join("gone.ogg");
    fs::write(&original, b"audio").unwrap();
    let linked = link_action(&root, sound(&original)).unwrap();
    assert!(!missing_target(&linked));
    fs::remove_file(&original).unwrap();
    assert!(missing_target(&linked), "dangling link counts as missing");
    assert!(!missing_target(&Action::Command("x".into())));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_file_that_does_not_exist_still_gets_a_link_that_reports_missing() {
    let root = temp_root("dangling");
    let nowhere = root.join("never-there.wav");
    let linked = link_action(&root, sound(&nowhere)).unwrap();
    assert!(sound_path(&linked).starts_with(root.join("links")));
    assert!(missing_target(&linked));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn awkward_file_names_make_safe_link_names() {
    let root = temp_root("names");
    for name in [
        "spaced name.mp3",
        "-dash first.wav",
        ".hidden.ogg",
        "ünïcode ♪.flac",
        &format!("{}.ogg", "long".repeat(40)),
        "no_extension",
    ] {
        let original = root.join(name);
        fs::write(&original, b"audio").unwrap();
        let linked = sound_path(&link_action(&root, sound(&original)).unwrap());
        let file = linked.file_name().unwrap().to_str().unwrap().to_owned();
        assert!(file.len() <= 80, "{file}");
        assert!(
            file.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')),
            "{file}"
        );
        assert!(!file.starts_with(['-', '.']), "{file}");
        assert_eq!(fs::read_link(&linked).unwrap(), original);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_links_directory_is_private() {
    let root = temp_root("private");
    let original = root.join("a.wav");
    fs::write(&original, b"audio").unwrap();
    link_action(&root, sound(&original)).unwrap();
    let mode = fs::metadata(root.join("links"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_links_entry_that_is_a_regular_file_is_never_overwritten() {
    let root = temp_root("noclobber");
    let original = root.join("a.wav");
    fs::write(&original, b"audio").unwrap();
    let first = sound_path(&link_action(&root, sound(&original)).unwrap());
    // Replace our link with a regular file, as a stray copy would be.
    fs::remove_file(&first).unwrap();
    fs::write(&first, b"precious").unwrap();
    let second = sound_path(&link_action(&root, sound(&original)).unwrap());
    assert_ne!(first, second);
    assert_eq!(fs::read(&first).unwrap(), b"precious");
    let _ = symlink::<&Path, &Path>;
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn relative_and_home_relative_sounds_that_do_not_exist_are_missing() {
    // Playback resolves a relative path from its working directory and never
    // expands "~", so neither of these can play.
    for path in [
        "dry-fart.mp3",
        "~/Music/dry-fart.mp3",
        "no/such/dir/dry-fart.mp3",
    ] {
        assert!(missing_target(&sound(Path::new(path))), "{path}");
    }
    // A relative file that is there (cargo runs from the package root) is fine.
    assert!(!missing_target(&sound(Path::new("Cargo.toml"))));
}

#[test]
fn open_targets_that_are_urls_are_not_files_and_never_missing() {
    for target in [
        "https://example.com/a",
        "mailto:someone@example.org",
        "file:///nowhere",
    ] {
        assert!(!missing_target(&open(target)), "{target}");
    }
    assert!(missing_target(&open("/definitely/not/here.txt")));
    assert!(
        !missing_target(&open("")),
        "an empty target is another error"
    );
}
