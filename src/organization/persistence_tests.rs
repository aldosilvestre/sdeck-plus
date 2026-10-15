use super::{tests::Fixture, *};
use crate::layers::{Action, Trigger};
use std::fs;

#[test]
fn profile_application_round_trips_without_changing_pages_or_bindings() {
    let mut f = Fixture::new();
    f.organization = Organization::open_at(&f.root, &f.layers).unwrap();
    f.layers.create("Studio").unwrap();
    assert!(f.layers.select("Studio"));
    f.organization.sync(&f.layers);
    let page = f.page();
    f.organization.rename_page(&f.layers, "Audio Mix").unwrap();
    f.layers
        .bind(&page, Trigger::KeyPress(1), Action::Emit("kept".into()))
        .unwrap();
    f.organization
        .set_profile_application("Studio", "obs")
        .unwrap();
    f.layers.persist().unwrap();
    f.organization.persist().unwrap();
    let layers = LayerStore::open_at(&f.root).unwrap();
    let reopened = Organization::open_at(&f.root, &layers).unwrap();
    assert_eq!(reopened.profile_application("Studio"), Some("obs"));
    assert_eq!(reopened.current(&layers).page_name, "Audio Mix");
    assert_eq!(layers.active(), page);
    assert_eq!(
        layers.layers()[&page].bindings[&Trigger::KeyPress(1)],
        Action::Emit("kept".into())
    );
}

#[test]
fn invalid_profile_application_metadata_remains_untouched() {
    for applications in [
        r#"{"Base":"obs"}"#,
        r#"{"Missing":"obs"}"#,
        r#"{"Studio":""}"#,
        r#"{"Studio":"obs","Studio":"kitty"}"#,
    ] {
        let mut f = Fixture::new();
        f.layers.create("Studio").unwrap();
        let text = format!(
            r#"{{"version":1,"views":{{"Base":{{"profile":"Base","folder":[],"page":1}},"Studio":{{"profile":"Studio","folder":[],"page":1}}}},"profile_applications":{applications}}}"#
        );
        let path = f.root.join("organization.json");
        fs::write(&path, &text).unwrap();
        assert!(Organization::open_at(&f.root, &f.layers).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), text);
    }
}

#[test]
fn named_page_metadata_survives_reopening_without_changing_bindings() {
    // Given: named metadata points to the same backing layer as before.
    let mut f = Fixture::new();
    f.layers
        .bind("Base", Trigger::KeyPress(2), Action::Emit("kept".into()))
        .unwrap();
    f.layers.persist().unwrap();
    let layer_bytes = fs::read(f.root.join("layers.conf")).unwrap();
    fs::write(
        f.root.join("organization.json"),
        r#"{"version":1,"views":{"Base":{"profile":"Base","folder":[],"page":1,"page_name":"Audio Mix"}}}"#,
    )
    .unwrap();
    // When: load and persist through the production metadata store.
    let organization = Organization::open_at(&f.root, &f.layers).unwrap();
    organization.persist().unwrap();
    let layers = LayerStore::open_at(&f.root).unwrap();
    let reopened = Organization::open_at(&f.root, &layers).unwrap();
    // Then: the display name survives, without rewriting backing identities.
    assert_eq!(reopened.current(&layers).label, "Base / Audio Mix");
    assert_eq!(layers.active(), "Base");
    assert_eq!(
        layers.layers()["Base"].bindings[&Trigger::KeyPress(2)],
        Action::Emit("kept".into())
    );
    assert_eq!(fs::read(f.root.join("layers.conf")).unwrap(), layer_bytes);
}

#[test]
fn renamed_page_survives_when_saved_and_reopened() {
    // Given
    let mut f = Fixture::new();
    f.organization = Organization::open_at(&f.root, &f.layers).unwrap();
    let key = f.page();
    // When
    f.organization.rename_page(&f.layers, "Audio Mix").unwrap();
    f.layers.persist().unwrap();
    f.organization.persist().unwrap();
    let layers = LayerStore::open_at(&f.root).unwrap();
    let reopened = Organization::open_at(&f.root, &layers).unwrap();
    // Then
    assert_eq!(layers.active(), key);
    assert_eq!(reopened.current(&layers).page_name, "Audio Mix");
}

#[test]
fn legacy_page_numbers_load_when_names_are_absent() {
    // Given: the metadata written before page renaming was available.
    let f = Fixture::new();
    fs::write(
        f.root.join("organization.json"),
        r#"{"version":1,"views":{"Base":{"profile":"Base","folder":[],"page":1}}}"#,
    )
    .unwrap();
    // When
    let organization = Organization::open_at(&f.root, &f.layers).unwrap();
    // Then
    assert_eq!(organization.current(&f.layers).page_name, "Page 1");
}

#[test]
fn hierarchy_and_layer_bindings_survive_when_reopened() {
    // Given
    let mut f = Fixture::new();
    f.organization = Organization::open_at(&f.root, &f.layers).unwrap();
    f.folder("Tools");
    let key = f.page();
    f.layers
        .bind(&key, Trigger::KeyPress(2), Action::Emit("kept".into()))
        .unwrap();
    fs::write(f.root.join("artwork.fixture"), b"existing backing").unwrap();
    f.layers.persist().unwrap();
    f.organization.persist().unwrap();
    // When
    let layers = LayerStore::open_at(&f.root).unwrap();
    let organization = Organization::open_at(&f.root, &layers).unwrap();
    // Then
    assert_eq!(layers.active(), key);
    assert_eq!(organization.current(&layers).folder, ["Tools"]);
    assert_eq!(organization.current(&layers).page, 2);
    assert_eq!(
        layers.layers()[&key].bindings[&Trigger::KeyPress(2)],
        Action::Emit("kept".into())
    );
    assert_eq!(
        fs::read(f.root.join("artwork.fixture")).unwrap(),
        b"existing backing"
    );
}

#[test]
fn legacy_data_is_preserved_when_metadata_is_missing() {
    // Given
    let mut f = Fixture::new();
    f.layers.create("Custom").unwrap();
    f.layers.select("Custom");
    f.layers
        .bind(
            "Custom",
            Trigger::KeyPress(1),
            Action::Emit("legacy".into()),
        )
        .unwrap();
    f.layers.persist().unwrap();
    let bytes = fs::read(f.root.join("layers.conf")).unwrap();
    // When
    let organization = Organization::open_at(&f.root, &f.layers).unwrap();
    organization.persist().unwrap();
    // Then
    assert_eq!(organization.current(&f.layers).profile, "Custom");
    assert_eq!(organization.current(&f.layers).page, 1);
    assert_eq!(fs::read(f.root.join("layers.conf")).unwrap(), bytes);
}

#[test]
fn corrupt_metadata_is_preserved_when_open_fails() {
    for bytes in [
        "{",
        r#"{"version":3,"views":{}}"#,
        r#"{"version":1,"views":{"Base":{"profile":"Base","folder":[],"page":0}}}"#,
        r#"{"version":1,"views":{"Base":{"profile":"Base","folder":[".."],"page":1}}}"#,
        r#"{"version":1,"views":{"Base":{"profile":"Base","folder":[],"page":1},"Other":{"profile":"Base","folder":[],"page":1}}}"#,
        r#"{"version":1,"views":{"Base":{"profile":"","folder":[],"page":1}}}"#,
        r#"{"version":1,"views":{"Base":{"profile":"Base","folder":[],"page":1},"Base":{"profile":"Base","folder":[],"page":2}}}"#,
        r#"{"version":1,"views":{"Other":{"profile":"Base","folder":[],"page":1}}}"#,
        r#"{"version":1,"views":{"Base":{"profile":"Base","folder":[],"page":1,"unknown":true}}}"#,
        r#"{"version":1,"views":{"Base":{"profile":"Base","folder":[],"page":1,"page_name":""}}}"#,
        r#"{"version":1,"views":{"Base":{"profile":"Base","folder":[],"page":1,"page_name":"Mix\n"}}}"#,
    ] {
        // Given
        let mut f = Fixture::new();
        f.layers.create("Other").unwrap();
        let path = f.root.join("organization.json");
        fs::write(&path, bytes).unwrap();
        f.layers.persist().unwrap();
        let layers_bytes = fs::read(f.root.join("layers.conf")).unwrap();
        // When
        let result = Organization::open_at(&f.root, &f.layers);
        // Then
        assert!(result.is_err(), "{bytes}");
        assert_eq!(fs::read(&path).unwrap(), bytes.as_bytes());
        assert_eq!(fs::read(f.root.join("layers.conf")).unwrap(), layers_bytes);
    }
}

#[test]
fn unreadable_metadata_is_preserved_when_open_fails() {
    // Given: a directory cannot be read as metadata, even by root.
    let f = Fixture::new();
    let path = f.root.join("organization.json");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("sentinel"), b"keep").unwrap();
    // When
    let result = Organization::open_at(&f.root, &f.layers);
    // Then
    assert!(result.is_err());
    assert_eq!(fs::read(path.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn stale_metadata_is_removed_and_unknown_layers_adopted_when_synced() {
    // Given
    let mut f = Fixture::new();
    f.organization = Organization::open_at(&f.root, &f.layers).unwrap();
    let removed = f.folder("Tools");
    f.layers.remove(&removed).unwrap();
    f.layers.create("Unlisted").unwrap();
    f.layers.select("Unlisted");
    // When
    f.organization.sync(&f.layers);
    f.organization.persist().unwrap();
    let reopened = Organization::open_at(&f.root, &f.layers).unwrap();
    // Then
    assert!(!reopened.views.contains_key(&removed));
    assert_eq!(reopened.current(&f.layers).profile, "Unlisted");
}

#[test]
fn failed_rename_preserves_destination_when_persisting() {
    // Given
    let mut f = Fixture::new();
    f.organization = Organization::open_at(&f.root, &f.layers).unwrap();
    f.organization.persist().unwrap();
    // A failing final rename cannot replace this nonempty metadata directory.
    let path = f.root.join("organization.json");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("sentinel"), b"keep").unwrap();
    // When
    let result = f.organization.persist();
    // Then
    assert!(result.is_err());
    assert_eq!(fs::read(path.join("sentinel")).unwrap(), b"keep");
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 1);
}

#[test]
fn memory_only_persistence_does_not_create_metadata_when_called() {
    // Given
    let f = Fixture::new();
    // When
    f.organization.persist().unwrap();
    // Then
    assert!(!f.root.join("organization.json").exists());
}

#[test]
fn page_owned_folder_identity_round_trips_with_names_and_bindings() {
    // Given
    let mut f = Fixture::new();
    f.organization = Organization::open_at(&f.root, &f.layers).unwrap();
    let first = f.folder("Tools");
    f.layers.select("Base");
    let owner = f.page();
    let second = f.folder("Tools");
    f.layers
        .bind(&second, Trigger::KeyPress(1), Action::Emit("kept".into()))
        .unwrap();
    f.layers.select(&owner);
    f.organization.rename_page(&f.layers, "Owner").unwrap();
    let views = f.organization.views.clone();
    f.layers.persist().unwrap();
    f.organization.persist().unwrap();
    // When
    let bytes = fs::read(f.root.join("organization.json")).unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut layers = LayerStore::open_at(&f.root).unwrap();
    let mut reopened = Organization::open_at(&f.root, &layers).unwrap();
    // Then
    assert_eq!(metadata["version"], 2);
    assert_eq!(reopened.views, views);
    assert_eq!(reopened.folders(&layers), ["Tools"]);
    assert!(reopened
        .navigate(&mut layers, &NavigationAction::OpenFolder("Tools".into()))
        .unwrap());
    assert_eq!(layers.active(), second);
    assert_eq!(
        layers.layers()[&second].bindings[&Trigger::KeyPress(1)],
        Action::Emit("kept".into())
    );
    layers.select("Base");
    assert!(reopened
        .navigate(&mut layers, &NavigationAction::OpenFolder("Tools".into()))
        .unwrap());
    assert_eq!(layers.active(), first);
}

#[test]
fn legacy_folders_migrate_to_first_surviving_parent_page_without_rekeying() {
    for remove_first in [false, true] {
        // Given: legacy folders belonged to paths, not individual pages.
        let mut f = Fixture::new();
        f.layers.create("Studio").unwrap();
        f.layers.select("Studio");
        f.organization.sync(&f.layers);
        let root_second = f.page();
        let folder_first = f.folder("Tools");
        let folder_second = f.page();
        let child = f.folder("Audio");
        f.organization
            .rename_page(&f.layers, "Named child")
            .unwrap();
        f.organization
            .set_profile_application("Studio", "obs")
            .unwrap();
        f.layers
            .bind(&child, Trigger::KeyPress(1), Action::Emit("kept".into()))
            .unwrap();
        if remove_first {
            f.layers.remove("Studio").unwrap();
            f.layers.remove(&folder_first).unwrap();
        }
        let mut views = serde_json::to_value(&f.organization.views).unwrap();
        for view in views.as_object_mut().unwrap().values_mut() {
            view.as_object_mut().unwrap().remove("parent_page");
        }
        let legacy = serde_json::json!({
            "version": 1, "views": views, "profile_applications": {"Studio": "obs"}
        });
        let bytes = serde_json::to_vec(&legacy).unwrap();
        fs::write(f.root.join("organization.json"), &bytes).unwrap();
        fs::write(f.root.join("artwork.fixture"), b"keep").unwrap();
        f.layers.persist().unwrap();
        let layer_bytes = fs::read(f.root.join("layers.conf")).unwrap();
        // When
        let mut reopened = Organization::open_at(&f.root, &f.layers).unwrap();
        let owner = if remove_first {
            root_second.as_str()
        } else {
            "Studio"
        };
        let nested_owner = if remove_first {
            &folder_second
        } else {
            &folder_first
        };
        // Then: migration chooses page 1, or the lowest surviving page.
        assert_eq!(
            reopened.views[&folder_second].parent_page.as_deref(),
            Some(owner)
        );
        assert_eq!(
            reopened.views[&child].parent_page.as_ref(),
            Some(nested_owner)
        );
        assert_eq!(reopened.current(&f.layers).page_name, "Named child");
        assert!(reopened
            .navigate(&mut f.layers, &NavigationAction::ParentFolder)
            .unwrap());
        assert_eq!(f.layers.active(), nested_owner);
        assert_eq!(reopened.profile_application("Studio"), Some("obs"));
        assert_eq!(
            f.layers.layers()[&child].bindings[&Trigger::KeyPress(1)],
            Action::Emit("kept".into())
        );
        assert_eq!(fs::read(f.root.join("organization.json")).unwrap(), bytes);
        reopened.persist().unwrap();
        let again = Organization::open_at(&f.root, &f.layers).unwrap();
        assert_eq!(again.views, reopened.views);
        assert_eq!(fs::read(f.root.join("layers.conf")).unwrap(), layer_bytes);
        assert_eq!(fs::read(f.root.join("artwork.fixture")).unwrap(), b"keep");
    }
}

#[test]
fn invalid_owner_references_scopes_cycles_and_duplicates_are_rejected() {
    for case in [
        "missing",
        "cross-profile",
        "cycle",
        "root",
        "scope",
        "duplicate",
        "absent",
    ] {
        // Given
        let mut f = Fixture::new();
        let first = f.folder("Tools");
        let nested = f.folder("Audio");
        f.layers.select("Base");
        let second_root = f.page();
        let second = f.folder("Tools");
        f.layers.create("Studio").unwrap();
        f.organization.sync(&f.layers);
        let mut views = serde_json::to_value(&f.organization.views).unwrap();
        match case {
            "missing" => views[&first]["parent_page"] = "Missing".into(),
            "cross-profile" => views[&first]["parent_page"] = "Studio".into(),
            "cycle" => views[&first]["parent_page"] = nested.clone().into(),
            "root" => views["Base"]["parent_page"] = first.clone().into(),
            "scope" => views[&nested]["parent_page"] = "Base".into(),
            "duplicate" => views[&second]["parent_page"] = "Base".into(),
            "absent" => {
                views[&first].as_object_mut().unwrap().remove("parent_page");
            }
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&serde_json::json!({"version": 2, "views": views})).unwrap();
        let path = f.root.join("organization.json");
        fs::write(&path, &bytes).unwrap();
        let keys = f.layers.names();
        // When / Then
        assert!(Organization::open_at(&f.root, &f.layers).is_err(), "{case}");
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(f.layers.names(), keys);
        assert_eq!(
            f.organization.views[&second].parent_page.as_ref(),
            Some(&second_root)
        );
    }
}
