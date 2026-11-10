//! Sweep of whole-control removal and undo over every control kind and number.
use sdeck_plus::{
    binding_editor::{control_triggers, ControlKind},
    info::InfoSource,
    layers::{Action, LayerStore, PageReturn, SwipeDirection, Trigger},
    local_artwork::ArtworkStore,
    organization::Organization,
};
use std::{collections::BTreeSet, fs, path::Path};

const TICKS: [i8; 7] = [-128, -5, -1, 1, 2, 7, 127];

fn expected(control: ControlKind, n: u8) -> BTreeSet<Trigger> {
    match control {
        ControlKind::Button => [Trigger::KeyPress(n), Trigger::KeyHold(n)].into(),
        ControlKind::Encoder => [
            Trigger::EncoderPress(n),
            Trigger::EncoderClockwise(n),
            Trigger::EncoderCounterClockwise(n),
        ]
        .into_iter()
        .chain(TICKS.map(|t| Trigger::Rotation(n, t)))
        .collect(),
        ControlKind::Touch if n == 0 => [
            Trigger::TouchTap,
            Trigger::TouchPress,
            Trigger::TouchFlick,
            Trigger::TouchSwipe(SwipeDirection::Left),
            Trigger::TouchSwipe(SwipeDirection::Right),
            Trigger::TouchSwipe(SwipeDirection::Up),
            Trigger::TouchSwipe(SwipeDirection::Down),
        ]
        .into(),
        ControlKind::Touch => [Trigger::ZoneTap(n), Trigger::ZonePress(n)].into(),
    }
}

fn controls() -> Vec<(ControlKind, u8)> {
    (1..=8)
        .map(|n| (ControlKind::Button, n))
        .chain((1..=4).map(|n| (ControlKind::Encoder, n)))
        .chain((0..=4).map(|n| (ControlKind::Touch, n)))
        .collect()
}

fn dump(store: &LayerStore) -> String {
    store
        .configurations()
        .map(|layer| {
            format!(
                "{}\n{:?}\n{:?}\n{:?}\n",
                layer.name, layer.bindings, layer.returns, layer.displays
            )
        })
        .collect()
}

fn files(root: &Path) -> (Vec<u8>, Option<Vec<u8>>) {
    (
        fs::read(root.join("layers.conf")).unwrap(),
        fs::read(root.join("touchscreen.json")).ok(),
    )
}

fn fill(store: &mut LayerStore, layer: &str) {
    for (control, n) in controls() {
        for trigger in expected(control, n) {
            store
                .bind(
                    layer,
                    trigger.clone(),
                    Action::Emit(format!("{layer}-{trigger:?}")),
                )
                .unwrap();
            store
                .set_return(
                    layer,
                    &trigger,
                    Some(PageReturn {
                        layer: "Base".into(),
                        seconds: 7,
                    }),
                )
                .unwrap();
            // Rotation and the strip gestures cannot show information.
            let _ = store.set_info(layer, &trigger, Some(InfoSource::Clock));
        }
    }
}

#[test]
fn every_control_is_removed_whole_persisted_and_restored_exactly() {
    for (control, n) in controls() {
        let root = std::env::temp_dir().join(format!(
            "sdeck-sweep-{}-{control:?}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let mut store = LayerStore::open_at(&root).unwrap();
        store.create("Studio").unwrap();
        let mut organization = Organization::open_at(&root, &store).unwrap();
        organization.sync(&store);
        let mut artwork = ArtworkStore::open_at_for_layer(&root, Some(store.active())).unwrap();
        store
            .initialize_touchscreens(&organization, &mut artwork)
            .unwrap();
        fill(&mut store, "Base");
        fill(&mut store, "Studio");
        store.persist().unwrap();
        let before = dump(&store);
        let disk = files(&root);

        let planned: BTreeSet<_> = control_triggers(&store, "Base", control, n)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(planned, expected(control, n), "{control:?} {n}");

        let removed = store.unbind_control("Base", control, n).unwrap();
        let got: BTreeSet<_> = removed.iter().map(|item| item.trigger.clone()).collect();
        assert_eq!(got, planned, "{control:?} {n}");
        for trigger in &planned {
            assert!(store.binding("Base", trigger).is_none(), "{trigger:?}");
            assert!(store.page_return("Base", trigger).is_none(), "{trigger:?}");
            assert!(store.info_source("Base", trigger).is_none(), "{trigger:?}");
        }
        // Other controls on Base and every trigger on Studio stay bound.
        for (other, m) in controls().into_iter().filter(|c| *c != (control, n)) {
            let overlapping = expected(other, m).is_subset(&planned);
            for trigger in expected(other, m).difference(&planned) {
                assert!(
                    overlapping || store.binding("Base", trigger).is_some(),
                    "{trigger:?}"
                );
            }
        }
        for trigger in &planned {
            assert!(store.binding("Studio", trigger).is_some(), "{trigger:?}");
        }
        store.persist().unwrap();
        let reopened = LayerStore::open_at(&root).unwrap();
        for trigger in &planned {
            assert!(reopened.binding("Base", trigger).is_none(), "{trigger:?}");
        }
        assert!(store.unbind_control("Base", control, n).unwrap().is_empty());

        store.restore_removed(&removed).unwrap();
        store.persist().unwrap();
        assert_eq!(dump(&store), before, "{control:?} {n}");
        assert_eq!(files(&root), disk, "{control:?} {n}");
        fs::remove_dir_all(&root).unwrap();
    }
}
