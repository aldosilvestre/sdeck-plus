use super::{tests::Fixture, LayerStore, NavigationAction, Organization};
use crate::{
    features::{FeatureAction, MultiStep},
    layers::{encode_action, normalize_keyboard_action, Action, PageReturn, Trigger},
};
use std::fs;

/// Each page's bindings and page returns.
type Pages = Vec<(
    String,
    std::collections::BTreeMap<Trigger, Action>,
    std::collections::BTreeMap<Trigger, PageReturn>,
)>;

/// What a failed folder operation must leave as it was: views, pages, selection and disk.
pub(super) type Snapshot = (
    std::collections::BTreeMap<String, super::View>,
    Pages,
    String,
    Vec<u8>,
    Vec<u8>,
);

pub(super) fn snapshot(f: &Fixture) -> Snapshot {
    (
        f.organization.views.clone(),
        f.layers
            .layers()
            .values()
            .map(|page| {
                (
                    page.name.clone(),
                    page.bindings.clone(),
                    page.returns.clone(),
                )
            })
            .collect(),
        f.layers.active().to_owned(),
        fs::read(f.root.join("layers.conf")).unwrap(),
        fs::read(f.root.join("organization.json")).unwrap(),
    )
}

/// Base owns the folder "Tools"; its key 1 runs a multi action that presses a key
/// whose name was hand-edited into one no keyboard has, then opens "Tools"; its
/// key 2 opens "Tools". Both stores are saved and read back from disk.
///
/// Returns the fixture and the error saving that multi action again reports.
pub(super) fn folder_with_unreadable_key_step() -> (Fixture, String) {
    let mut f = Fixture::new();
    f.organization = Organization::open_at(&f.root, &f.layers).unwrap();
    f.folder("Tools");
    f.layers.select("Base");
    let open = Action::Feature(FeatureAction::Navigation(NavigationAction::OpenFolder(
        "Tools".into(),
    )));
    let multi = |key: &str| {
        Action::Feature(FeatureAction::Multi {
            steps: vec![
                MultiStep::Action(Action::KeyPress { key: key.into() }),
                MultiStep::Action(open.clone()),
            ],
        })
    };
    let Action::KeyPress { key } =
        normalize_keyboard_action(Action::KeyPress { key: "a".into() }).unwrap()
    else {
        unreachable!("a key press stays a key press");
    };
    f.layers
        .bind("Base", Trigger::KeyPress(1), multi(&key))
        .unwrap();
    f.layers
        .bind("Base", Trigger::KeyPress(2), open.clone())
        .unwrap();
    f.layers.persist().unwrap();
    f.organization.persist().unwrap();
    let path = f.root.join("layers.conf");
    let text = fs::read_to_string(&path).unwrap();
    let stored = encode_action(&multi(&key)).unwrap();
    assert!(text.contains(&stored), "{text}");
    let unreadable = Action::KeyPress {
        key: "no such key".into(),
    };
    let edited = encode_action(&multi("no such key")).unwrap();
    fs::write(&path, text.replace(&stored, &edited)).unwrap();
    f.layers = LayerStore::open_at(&f.root).unwrap();
    f.organization = Organization::open_at(&f.root, &f.layers).unwrap();
    // The edited step is kept as it was written, as legacy key names are.
    assert!(
        f.layers.conflicts().is_empty(),
        "{:?}",
        f.layers.conflicts()
    );
    assert_eq!(
        f.layers.binding("Base", &Trigger::KeyPress(1)),
        Some(multi("no such key"))
    );
    let error = normalize_keyboard_action(unreadable).unwrap_err();
    (f, format!("Step 1: {error}"))
}

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

#[test]
fn folder_deletion_blocked_by_an_unreadable_key_step_changes_nothing() {
    // Given
    let (mut f, expected) = folder_with_unreadable_key_step();
    let before = snapshot(&f);
    // When
    let error = f
        .organization
        .remove_folder(&mut f.layers, "Tools")
        .unwrap_err();
    // Then: the step is named, and the folder, both links and the files remain.
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(error.to_string(), expected);
    assert_eq!(snapshot(&f), before);
    assert_eq!(f.organization.folders(&f.layers), ["Tools"]);
}
