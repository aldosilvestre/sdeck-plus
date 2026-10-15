use super::{tests::Fixture, *};

#[test]
fn invalid_page_names_leave_metadata_and_selection_unchanged() {
    for name in [
        "".to_owned(),
        " ".into(),
        "Mix\n".into(),
        "Mix\0".into(),
        "a".repeat(65),
        "é".repeat(33),
    ] {
        // Given
        let mut f = Fixture::new();
        let key = f.page();
        f.organization.rename_page(&f.layers, "Audio Mix").unwrap();
        let before = f.organization.views.clone();
        // When
        let result = f.organization.rename_page(&f.layers, &name);
        // Then
        assert!(result.is_err(), "{name:?}");
        assert_eq!(f.organization.views, before);
        assert_eq!(f.layers.active(), key);
    }
}

#[test]
fn folder_errors_preserve_metadata_when_names_are_invalid_or_duplicate() {
    for name in ["", " ", "..", ".", "a/b", "a\\b", " a", "a\n", "Tools"] {
        // Given
        let mut f = Fixture::new();
        f.folder("Tools");
        f.layers.select("Base");
        let before = f.organization.views.clone();
        let keys = f.layers.names();
        // When
        let result = f.organization.create_folder(&mut f.layers, name);
        // Then
        assert!(result.is_err(), "{name:?}");
        assert_eq!(f.organization.views, before);
        assert_eq!(f.layers.names(), keys);
        assert_eq!(f.layers.active(), "Base");
    }
}

#[test]
fn limit_errors_leave_unsynced_metadata_unchanged_when_creating_views() {
    for folder in [false, true] {
        // Given
        let mut f = Fixture::new();
        for n in 1..64 {
            f.layers.create(&format!("Legacy {n}")).unwrap();
        }
        let before = f.organization.views.clone();
        let keys = f.layers.names();
        // When
        let result = if folder {
            f.organization.create_folder(&mut f.layers, "Tools")
        } else {
            f.organization.create_page(&mut f.layers)
        };
        // Then
        assert!(result.is_err());
        assert_eq!(f.organization.views, before);
        assert_eq!(f.layers.names(), keys);
        assert_eq!(f.layers.active(), "Base");
    }
}

#[test]
fn malformed_actions_leave_selection_unchanged_when_navigated() {
    for action in [
        NavigationAction::GoToPage(0),
        NavigationAction::OpenFolder("../Tools".into()),
    ] {
        // Given
        let mut f = Fixture::new();
        let before = f.organization.views.clone();
        // When
        let result = f.organization.navigate(&mut f.layers, &action);
        // Then
        assert!(result.is_err());
        assert_eq!(f.layers.active(), "Base");
        assert_eq!(f.organization.views, before);
    }
}

#[test]
fn actions_roundtrip_when_serialized() {
    // Given
    let actions = [
        NavigationAction::NextPage,
        NavigationAction::PreviousPage,
        NavigationAction::GoToPage(3),
        NavigationAction::OpenFolder("Tools".into()),
        NavigationAction::ParentFolder,
    ];
    // When / Then
    for action in actions {
        let bytes = serde_json::to_vec(&action).unwrap();
        assert_eq!(
            serde_json::from_slice::<NavigationAction>(&bytes).unwrap(),
            action
        );
    }
}

#[test]
fn page_overflow_preserves_stores_when_appending() {
    // Given
    let mut f = Fixture::new();
    f.organization.views.get_mut("Base").unwrap().page = u16::MAX;
    let before = f.organization.views.clone();
    // When
    let result = f.organization.create_page(&mut f.layers);
    // Then
    assert!(result.is_err());
    assert_eq!(f.organization.views, before);
    assert_eq!(f.layers.names(), ["Base"]);
    assert_eq!(f.layers.active(), "Base");
}

#[test]
fn child_entry_stays_in_profile_when_other_profile_has_target() {
    // Given
    let mut f = Fixture::new();
    f.layers.create("Studio").unwrap();
    f.layers.select("Studio");
    f.folder("Tools");
    f.layers.select("Base");
    // When
    let changed = f.navigate(NavigationAction::OpenFolder("Tools".into()));
    // Then
    assert!(!changed);
    assert_eq!(f.layers.active(), "Base");
}

#[test]
fn parent_does_not_cross_scope_when_parent_page_one_is_deleted() {
    // Given
    let mut f = Fixture::new();
    let parent = f.folder("Tools");
    let child = f.folder("Audio");
    f.layers.remove(&parent).unwrap();
    f.organization.sync(&f.layers);
    // When
    let changed = f.navigate(NavigationAction::ParentFolder);
    // Then
    assert!(!changed);
    assert_eq!(f.layers.active(), child);
}

#[test]
fn invalid_named_page_and_removals_leave_both_stores_unchanged() {
    // Given
    let mut f = Fixture::new();
    f.folder("Tools");
    f.layers.select("Base");
    f.layers
        .bind(
            "Base",
            crate::layers::Trigger::KeyPress(1),
            crate::layers::Action::Emit("kept".into()),
        )
        .unwrap();
    let before = f.organization.views.clone();
    let keys = f.layers.names();
    let bindings = f.layers.layers()["Base"].bindings.clone();
    // When / Then
    for name in [
        "".to_owned(),
        " ".into(),
        "bad\n".into(),
        "a".repeat(65),
        "é".repeat(33),
    ] {
        assert!(f
            .organization
            .create_named_page(&mut f.layers, &name)
            .is_err());
    }
    assert!(f.organization.remove_page(&mut f.layers).is_err());
    for profile in ["Base", "Missing"] {
        assert!(f
            .organization
            .remove_profile(&mut f.layers, profile)
            .is_err());
    }
    for name in ["Missing", "..", "a/b"] {
        assert!(f.organization.remove_folder(&mut f.layers, name).is_err());
    }
    assert_eq!(f.organization.views, before);
    assert_eq!(f.layers.names(), keys);
    assert_eq!(f.layers.active(), "Base");
    assert_eq!(f.layers.layers()["Base"].bindings, bindings);
}
