use super::{tests::Fixture, LayerStore, NavigationAction, Organization};
use crate::{
    features::FeatureAction,
    layers::{Action, Trigger},
};

fn open_folder(name: &str) -> Action {
    Action::Feature(FeatureAction::Navigation(NavigationAction::OpenFolder(
        name.into(),
    )))
}

/// Base owns "Tools" (two pages, one nested "Deep" folder) and "Other".
fn nested_fixture() -> (Fixture, String, String, String) {
    let mut f = Fixture::new();
    let tools = f.folder("Tools");
    let tools_two = f.page();
    let deep = f.folder("Deep");
    f.layers.select("Base");
    f.folder("Other");
    f.layers.select("Base");
    (f, tools, tools_two, deep)
}

#[test]
fn renamed_folder_keeps_keys_and_updates_views_descendants_and_navigation() {
    // Given
    let (mut f, tools, tools_two, deep) = nested_fixture();
    let keys = f.layers.names();
    // When
    f.organization
        .rename_folder(&mut f.layers, "Tools", "Studio")
        .unwrap();
    // Then: layer keys and the selection are unchanged.
    assert_eq!(f.layers.names(), keys);
    assert_eq!(f.layers.active(), "Base");
    assert_eq!(f.organization.folders(&f.layers), ["Other", "Studio"]);
    for key in [&tools, &tools_two] {
        assert_eq!(f.organization.views[key].folder, ["Studio"]);
    }
    assert_eq!(f.organization.views[&deep].folder, ["Studio", "Deep"]);
    // Navigation uses the new name; the old one no longer resolves.
    assert!(!f.navigate(NavigationAction::OpenFolder("Tools".into())));
    assert!(f.navigate(NavigationAction::OpenFolder("Studio".into())));
    assert_eq!(f.layers.active(), tools);
    assert_eq!(
        f.organization.current(&f.layers).label,
        "Base / Studio / Page 1"
    );
    assert!(f.navigate(NavigationAction::NextPage));
    assert!(f.navigate(NavigationAction::OpenFolder("Deep".into())));
    assert_eq!(f.layers.active(), deep);
    assert_eq!(
        f.organization.current(&f.layers).label,
        "Base / Studio / Deep / Page 1"
    );
    assert!(f.navigate(NavigationAction::ParentFolder));
    assert!(f.navigate(NavigationAction::ParentFolder));
    assert_eq!(f.layers.active(), "Base");
}

#[test]
fn renamed_folder_links_follow_only_on_the_owning_page() {
    // Given: Base links Tools; another page links its own Tools.
    let mut f = Fixture::new();
    f.folder("Tools");
    f.layers.select("Base");
    f.layers
        .bind("Base", Trigger::KeyPress(1), open_folder("Tools"))
        .unwrap();
    f.layers
        .bind("Base", Trigger::KeyPress(2), open_folder("Other"))
        .unwrap();
    f.layers
        .bind("Base", Trigger::KeyPress(3), Action::Emit("keep".into()))
        .unwrap();
    let second = f.page();
    f.folder("Tools");
    f.layers.select(&second);
    f.layers
        .bind(&second, Trigger::KeyPress(1), open_folder("Tools"))
        .unwrap();
    f.layers.select("Base");
    // When
    f.organization
        .rename_folder(&mut f.layers, "Tools", "Studio")
        .unwrap();
    // Then
    let base = &f.layers.layers()["Base"].bindings;
    assert_eq!(base[&Trigger::KeyPress(1)], open_folder("Studio"));
    assert_eq!(base[&Trigger::KeyPress(2)], open_folder("Other"));
    assert_eq!(base[&Trigger::KeyPress(3)], Action::Emit("keep".into()));
    assert_eq!(
        f.layers.layers()[&second].bindings[&Trigger::KeyPress(1)],
        open_folder("Tools")
    );
    f.layers.select(&second);
    assert_eq!(f.organization.folders(&f.layers), ["Tools"]);
}

#[test]
fn rename_errors_leave_both_stores_unchanged() {
    for (old, new) in [
        ("Tools", ""),
        ("Tools", " Studio"),
        ("Tools", "a/b"),
        ("Tools", ".."),
        ("Tools", "bad\nname"),
        ("Tools", "Other"),
        ("Missing", "Studio"),
        ("Deep", "Studio"),
    ] {
        // Given
        let (mut f, ..) = nested_fixture();
        f.layers
            .bind("Base", Trigger::KeyPress(1), open_folder("Tools"))
            .unwrap();
        let views = f.organization.views.clone();
        let keys = f.layers.names();
        // When
        let result = f.organization.rename_folder(&mut f.layers, old, new);
        // Then
        assert!(result.is_err(), "{old:?} -> {new:?}");
        assert_eq!(f.organization.views, views);
        assert_eq!(f.layers.names(), keys);
        assert_eq!(f.layers.active(), "Base");
        assert_eq!(
            f.layers.layers()["Base"].bindings[&Trigger::KeyPress(1)],
            open_folder("Tools")
        );
    }
}

#[test]
fn renaming_to_the_same_name_is_a_no_op() {
    let (mut f, ..) = nested_fixture();
    let views = f.organization.views.clone();
    f.organization
        .rename_folder(&mut f.layers, "Tools", "Tools")
        .unwrap();
    assert_eq!(f.organization.views, views);
}

#[test]
fn renamed_folder_survives_when_saved_and_reopened() {
    // Given
    let mut f = Fixture::new();
    f.organization = Organization::open_at(&f.root, &f.layers).unwrap();
    let tools = f.folder("Tools");
    f.layers.select("Base");
    f.layers
        .bind("Base", Trigger::KeyPress(1), open_folder("Tools"))
        .unwrap();
    // When
    f.organization
        .rename_folder(&mut f.layers, "Tools", "Studio")
        .unwrap();
    f.layers.persist().unwrap();
    f.organization.persist().unwrap();
    let mut layers = LayerStore::open_at(&f.root).unwrap();
    let mut reopened = Organization::open_at(&f.root, &layers).unwrap();
    // Then
    assert_eq!(reopened.folders(&layers), ["Studio"]);
    assert_eq!(
        layers.layers()["Base"].bindings[&Trigger::KeyPress(1)],
        open_folder("Studio")
    );
    assert!(reopened
        .navigate(&mut layers, &NavigationAction::OpenFolder("Studio".into()))
        .unwrap());
    assert_eq!(layers.active(), tools);
}
