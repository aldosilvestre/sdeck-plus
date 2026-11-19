use super::{tests::Fixture, LayerStore, NavigationAction};
use crate::{
    features::{FeatureAction, MultiStep},
    layers::{Action, PageReturn, Trigger},
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

#[test]
fn folder_deletion_drops_only_the_open_folder_step_of_a_multi_action() {
    // Given: one multi action keeps another action step, one does not.
    let mut f = Fixture::new();
    f.folder("Tools");
    f.layers.select("Base");
    let open = MultiStep::Action(Action::Feature(FeatureAction::Navigation(
        NavigationAction::OpenFolder("Tools".into()),
    )));
    let emit = MultiStep::Action(Action::Emit("keep".into()));
    let wait = MultiStep::Wait { milliseconds: 100 };
    let multi = |steps| Action::Feature(FeatureAction::Multi { steps });
    let page_return = Some(PageReturn {
        layer: "Base".into(),
        seconds: 5,
    });
    for (key, steps) in [
        (1, vec![emit.clone(), wait.clone(), open.clone()]),
        (2, vec![wait.clone(), open]),
    ] {
        let trigger = Trigger::KeyPress(key);
        f.layers
            .bind("Base", trigger.clone(), multi(steps))
            .unwrap();
        f.layers
            .set_return("Base", &trigger, page_return.clone())
            .unwrap();
    }
    // When
    f.organization
        .remove_folder(&mut f.layers, "Tools")
        .unwrap();
    f.layers.persist().unwrap();
    // Then: the first keeps its other steps and return; the second is unbound.
    let reopened = LayerStore::open_at(&f.root).unwrap();
    assert!(
        reopened.conflicts().is_empty(),
        "{:?}",
        reopened.conflicts()
    );
    for layers in [&f.layers, &reopened] {
        assert_eq!(
            layers.binding("Base", &Trigger::KeyPress(1)),
            Some(multi(vec![emit.clone(), wait.clone()]))
        );
        assert_eq!(
            layers.page_return("Base", &Trigger::KeyPress(1)),
            page_return
        );
        assert_eq!(layers.binding("Base", &Trigger::KeyPress(2)), None);
        assert_eq!(layers.page_return("Base", &Trigger::KeyPress(2)), None);
    }
}
