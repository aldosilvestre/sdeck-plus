use super::{tests::Fixture, NavigationAction};
use crate::{
    features::FeatureAction,
    layers::{Action, Trigger},
};

#[test]
fn folder_deletion_unlinks_only_its_owning_page() {
    // Given: two pages link to different folders with the same name.
    let mut f = Fixture::new();
    let first_folder = f.folder("Tools");
    f.navigate(NavigationAction::ParentFolder);
    let action = Action::Feature(FeatureAction::Navigation(NavigationAction::OpenFolder(
        "Tools".into(),
    )));
    f.layers
        .bind("Base", Trigger::KeyPress(1), action.clone())
        .unwrap();
    let second = f.page();
    let second_folder = f.folder("Tools");
    f.navigate(NavigationAction::ParentFolder);
    f.layers
        .bind(&second, Trigger::KeyPress(1), action.clone())
        .unwrap();
    f.layers
        .bind(&second, Trigger::KeyPress(2), Action::Emit("keep".into()))
        .unwrap();
    // When
    f.organization
        .remove_folder(&mut f.layers, "Tools")
        .unwrap();
    // Then: the other page's folder and link remain, and unrelated actions stay.
    assert!(!f.layers.layers().contains_key(&second_folder));
    assert!(f.layers.layers().contains_key(&first_folder));
    assert_eq!(
        f.layers.layers()["Base"].bindings[&Trigger::KeyPress(1)],
        action
    );
    assert!(!f.layers.layers()[&second]
        .bindings
        .contains_key(&Trigger::KeyPress(1)));
    assert_eq!(
        f.layers.layers()[&second].bindings[&Trigger::KeyPress(2)],
        Action::Emit("keep".into())
    );
}
