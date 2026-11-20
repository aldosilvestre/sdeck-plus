//! The Stream Deck Neo's information window in the window: the preview draws
//! it as a named, selectable control between the keys and the touch buttons;
//! selecting it shows its Appearance (the page's picture) and the live
//! information option, saved per keypad page; the preview and the device show
//! the page's picture with its reading, follow page changes and a running
//! timer. No device is involved: the window runs over scratch stores, and
//! channels stand in for the layer worker and the device.
use super::preview_model_tests::{class_of, model_fixture, ModelFixture};
use super::*;
use sdeck_plus::{
    binding_editor::ControlKind,
    device_model::{DeviceModel, PLUS},
    info::{Glyph, InfoSource, InfoValue},
    preview_layout::{ControlRef, PreviewLayout},
};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn neo() -> &'static DeviceModel {
    DeviceModel::by_id("neo").unwrap()
}

const WINDOW: ArtworkTarget = ArtworkTarget::TouchStripBackground;

/// A 248x58 picture of one color, outside the stores.
fn picture(color: u32) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "neo-window-ui-{}-{}.png",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let image =
        gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 248, 58).unwrap();
    image.fill(color);
    image.savev(&path, "png", &[]).unwrap();
    path
}

fn control(f: &ModelFixture, control: ControlRef) -> Button {
    f.widgets
        .preview
        .control(control)
        .unwrap_or_else(|| panic!("{control:?} is on the shown preview"))
        .downcast::<Button>()
        .unwrap()
}

fn click(f: &ModelFixture, caption: &str) {
    let mut buttons = Vec::new();
    collect_layout_buttons(f.widgets.root.upcast_ref(), &mut buttons);
    buttons
        .iter()
        .find(|button| button_caption(button).as_deref() == Some(caption))
        .unwrap_or_else(|| panic!("{caption} button"))
        .emit_clicked();
}

fn heading(f: &ModelFixture) -> String {
    class_of(f.widgets.root.upcast_ref(), "control-heading")
        .downcast::<Label>()
        .unwrap()
        .text()
        .to_string()
}

fn info_caption(f: &ModelFixture) -> String {
    descendant_label(f.widgets.binding_ui.info_toggle.upcast_ref())
        .unwrap()
        .text()
        .to_string()
}

fn position(source: InfoSource) -> u32 {
    InfoSource::ALL.iter().position(|s| *s == source).unwrap() as u32
}

/// Every command the window sent towards the device since the last call,
/// with snapshots flattened.
fn sent(f: &ModelFixture) -> Vec<ArtworkCommand> {
    f.hardware
        .try_iter()
        .flat_map(|command| match command {
            ArtworkCommand::Replace(list) => list,
            other => vec![other],
        })
        .collect()
}

/// The commands among `commands` that draw the window.
fn window_commands(commands: &[ArtworkCommand]) -> Vec<ArtworkCommand> {
    commands
        .iter()
        .filter(|command| {
            matches!(
                command,
                ArtworkCommand::Assign { target: WINDOW, .. }
                    | ArtworkCommand::Clear { target: WINDOW }
                    | ArtworkCommand::ComposeInfo { target: WINDOW, .. }
            )
        })
        .cloned()
        .collect()
}

/// The size of the picture the preview's window shows, when it shows one.
fn window_preview(f: &ModelFixture) -> Option<(i32, i32)> {
    let pictures = f.artwork.pictures.borrow();
    let (_, picture) = pictures.iter().find(|(target, _)| *target == WINDOW)?;
    if !picture.get_visible() {
        return None;
    }
    picture
        .paintable()
        .map(|paintable| (paintable.intrinsic_width(), paintable.intrinsic_height()))
}

fn clock(time: &str) -> InfoValue {
    InfoValue {
        detail: "Thu 19 Nov".into(),
        ..InfoValue::text(Glyph::None, time)
    }
}

#[test]
fn the_neo_window_is_selected_like_a_control_and_saves_its_information_per_page() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("window-select", neo(), |_, _| {}) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        let root: gtk::Widget = f.widgets.root.clone().upcast();
        let window = control(&f, ControlRef::Window);
        assert!(window.has_css_class("control-selectable"));
        assert!(window.has_css_class("window-surface"));
        assert!(window.is_focusable(), "reachable with the keyboard");
        let rect = PreviewLayout::for_model(neo())
            .rect_of(ControlRef::Window)
            .unwrap();
        let chassis = class_of(&root, "hardware-chassis");
        let bounds = window.compute_bounds(&chassis).expect("on the chassis");
        assert_eq!(
            (
                bounds.x() as i32,
                bounds.y() as i32,
                bounds.width() as i32,
                bounds.height() as i32
            ),
            (rect.x, rect.y, rect.w, rect.h)
        );
        assert_eq!(
            window.tooltip_text().unwrap(),
            "Info window · No live information"
        );
        assert!(!class_of(&root, "appearance-section").is_visible());
        assert_eq!(info_caption(&f), "Show live information on the key");

        window.emit_clicked();
        assert_eq!(ui.control_kind(), ControlKind::Window);
        assert_eq!(ui.selected_control(), Some(ControlRef::Window));
        assert!(window.has_css_class("control-selected"));
        assert_eq!(heading(&f), "Configuring: Info window");
        assert_eq!(
            ui.selected_layer_and_trigger(&f.layers).unwrap(),
            ("Base".to_owned(), Trigger::Window)
        );
        // It runs nothing: no event, action or hold; its picture is in
        // Appearance and its reading in the Binding tab.
        assert!(!ui.event_picker.is_mapped() && !ui.action_picker.is_mapped());
        assert!(!ui.hold_toggle.is_visible());
        assert!(class_of(&root, "appearance-section").is_visible());
        assert_eq!(f.artwork.selected_target.get(), WINDOW);
        let picker = &f.widgets.target_picker;
        let targets = picker
            .model()
            .unwrap()
            .downcast::<gtk::StringList>()
            .unwrap();
        assert_eq!(
            targets.string(picker.selected()).unwrap().as_str(),
            "Info window"
        );
        assert!(ui.info_toggle.is_visible());
        assert_eq!(info_caption(&f), "Show live information on the window");
        assert!(!f
            .widgets
            .feature_runtime
            .touchscreen_controls()
            .is_visible());

        ui.info_toggle.set_active(true);
        ui.info_source.set_selected(position(InfoSource::Clock));
        while f.requests.as_ref().unwrap().try_recv().is_ok() {}
        sent(&f);
        click(&f, "Save binding");
        let saved: Vec<_> = f.requests.as_ref().unwrap().try_iter().collect();
        assert!(saved
            .iter()
            .any(|command| matches!(command, LayerCommand::SaveBinding(..))));
        assert!(
            !saved
                .iter()
                .any(|command| matches!(command, LayerCommand::Execute { .. })),
            "saving never runs anything"
        );
        assert_eq!(
            f.layers.borrow().binding("Base", &Trigger::Window),
            Some(Action::None)
        );
        assert_eq!(
            f.layers.borrow().info_source("Base", &Trigger::Window),
            Some(InfoSource::Clock)
        );
        assert_eq!(
            window.tooltip_text().unwrap(),
            "Info window · Live information: Time and date"
        );
        let drawn = window_commands(&sent(&f));
        assert!(
            drawn
                .iter()
                .any(|command| matches!(command, ArtworkCommand::ComposeInfo { .. })),
            "{drawn:?}"
        );
        assert_eq!(window_preview(&f), Some((248, 58)));

        // Remove clears it, Undo puts it back.
        click(&f, "Remove binding");
        assert_eq!(f.layers.borrow().binding("Base", &Trigger::Window), None);
        assert_eq!(
            f.layers.borrow().info_source("Base", &Trigger::Window),
            None
        );
        f.widgets.toast.action.emit_clicked();
        assert_eq!(
            f.layers.borrow().info_source("Base", &Trigger::Window),
            Some(InfoSource::Clock)
        );

        // A key keeps its own wording and no Appearance section.
        control(&f, ControlRef::Key(1)).emit_clicked();
        assert_eq!(heading(&f), "Configuring: Key 1");
        assert!(!class_of(&root, "appearance-section").is_visible());
        assert_eq!(info_caption(&f), "Show live information on the key");
        drop(f);

        // The Stream Deck + has no window: its strip keeps its wording.
        let Some(f) = model_fixture("window-plus", &PLUS, |_, _| {}) else {
            return;
        };
        assert!(f.widgets.preview.control(ControlRef::Window).is_none());
        let ui = &f.widgets.binding_ui;
        assert!(ui
            .select_control(ControlKind::Window, Some(1), true)
            .is_err());
        control(&f, ControlRef::Strip).emit_clicked();
        let picker = &f.widgets.target_picker;
        let targets = picker
            .model()
            .unwrap()
            .downcast::<gtk::StringList>()
            .unwrap();
        assert_eq!(
            targets.string(0).unwrap().as_str(),
            "Touch strip background"
        );
    });
}

#[test]
fn each_page_shows_its_own_window_picture_and_reading_in_the_preview_and_on_the_device() {
    run_gtk_test(|| {
        let (red, blue) = (picture(0xff0000ff), picture(0x0000ffff));
        let mut assigned = (None, None);
        let fixture = model_fixture("window-pages", neo(), |store, images| {
            store.create("Studio").unwrap();
            store.bind("Base", Trigger::Window, Action::None).unwrap();
            store
                .set_info("Base", &Trigger::Window, Some(InfoSource::Clock))
                .unwrap();
            assigned.0 = Some(images.assign_in_layer("Base", WINDOW, &red).unwrap());
            assigned.1 = Some(images.assign_in_layer("Studio", WINDOW, &blue).unwrap());
        });
        let Some(f) = fixture else {
            let _ = std::fs::remove_file(&red);
            let _ = std::fs::remove_file(&blue);
            return;
        };
        let (base, studio) = (assigned.0.unwrap(), assigned.1.unwrap());
        sent(&f);
        f.artwork.refresh(true);
        // Base: its own picture under the clock, sent as a whole window.
        let drawn = window_commands(&sent(&f));
        assert_eq!(drawn.len(), 1, "{drawn:?}");
        let ArtworkCommand::ComposeInfo { path, value, .. } = &drawn[0] else {
            panic!("the clock is drawn: {drawn:?}");
        };
        assert_eq!(path.as_ref(), Some(&base));
        assert_eq!(value.glyph, InfoSource::Clock.placeholder().glyph);
        assert_eq!(window_preview(&f), Some((248, 58)));
        // A new reading redraws only the window.
        f.artwork.update_info(InfoSource::Clock, clock("10:24"));
        let drawn = sent(&f);
        assert_eq!(drawn.len(), 1, "{drawn:?}");
        assert!(
            matches!(&drawn[0], ArtworkCommand::ComposeInfo { target: WINDOW, value, path, .. }
            if value.headline == "10:24" && path.as_ref() == Some(&base))
        );

        // Another page shows its own picture, without the Base reading.
        f.widgets
            .feature_runtime
            .execute("test".into(), Action::SwitchLayer("Studio".into()));
        let drawn = window_commands(&sent(&f));
        assert!(
            matches!(&drawn[..], [ArtworkCommand::Assign { path, .. }] if *path == studio),
            "{drawn:?}"
        );
        {
            let pictures = f.artwork.pictures.borrow();
            let (_, shown) = pictures
                .iter()
                .find(|(target, _)| *target == WINDOW)
                .unwrap();
            assert!(shown.get_visible());
            assert_eq!(shown.file().and_then(|file| file.path()), Some(studio));
        }
        f.artwork.update_info(InfoSource::Clock, clock("10:25"));
        assert!(window_commands(&sent(&f)).is_empty());

        // Back on Base, the reading returns.
        f.widgets
            .feature_runtime
            .execute("test".into(), Action::SwitchLayer("Base".into()));
        let drawn = window_commands(&sent(&f));
        assert!(
            matches!(&drawn[..], [ArtworkCommand::ComposeInfo { value, .. }]
            if value.headline == "10:25")
        );
        drop(f);
        let _ = std::fs::remove_file(&red);
        let _ = std::fs::remove_file(&blue);
    });
}

#[test]
fn a_running_timer_counts_down_in_the_window_and_gives_it_back_when_it_ends() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("window-timer", neo(), |store, _| {
            store.bind("Base", Trigger::Window, Action::None).unwrap();
            store
                .set_info("Base", &Trigger::Window, Some(InfoSource::Clock))
                .unwrap();
        }) else {
            return;
        };
        f.artwork.update_info(InfoSource::Clock, clock("10:24"));
        sent(&f);
        f.artwork.start_key_countdown("Base".into(), 1, 90);
        let drawn = sent(&f);
        let countdown = |target: ArtworkTarget, time: &str| {
            drawn.iter().any(|command| {
                matches!(command, ArtworkCommand::ComposeInfo { target: shown, value, .. }
                    if *shown == target && value.glyph == Glyph::Timer && value.headline == time)
            })
        };
        assert!(countdown(ArtworkTarget::Keypad(1), "01:30"), "{drawn:?}");
        assert!(countdown(WINDOW, "01:30"), "{drawn:?}");
        assert_eq!(window_preview(&f), Some((248, 58)));
        // The countdown keeps the window while it runs.
        f.artwork.update_info(InfoSource::Clock, clock("10:25"));
        assert!(window_commands(&sent(&f)).is_empty());
        f.artwork.update_countdown(89);
        let drawn = sent(&f);
        assert!(drawn.iter().any(|command| matches!(command,
            ArtworkCommand::ComposeInfo { target: WINDOW, value, .. } if value.headline == "01:29")));
        // When it ends the window shows its reading again.
        f.artwork.clear_countdown();
        let drawn = window_commands(&sent(&f));
        assert!(
            matches!(&drawn[..], [ArtworkCommand::ComposeInfo { value, .. }]
            if value.headline == "10:25"),
            "{drawn:?}"
        );
        drop(f);

        // A model without a window counts down on its key only.
        let Some(f) = model_fixture("window-timer-plus", &PLUS, |_, _| {}) else {
            return;
        };
        sent(&f);
        f.artwork.start_key_countdown("Base".into(), 1, 90);
        let drawn = sent(&f);
        assert!(drawn.iter().any(|command| matches!(
            command,
            ArtworkCommand::ComposeInfo {
                target: ArtworkTarget::Keypad(1),
                ..
            }
        )));
        assert!(window_commands(&drawn).is_empty(), "{drawn:?}");
    });
}
