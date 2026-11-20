//! The Stream Deck Classic's keypad background (S6) in the window. The
//! preview draws a "Background" bar under the 15 keys as a named, selectable
//! control; selecting it shows its Appearance section, where the page's
//! background is chosen like other artwork. Each keypad page has its own.
//! The device gets the whole picture first (Update Full Screen Image) and then
//! every key, a key without a picture of its own drawn over its share of the
//! background; the preview shows the same shares. Remove clears the
//! background and the toast's Undo brings it back. No other model offers it.
//! No device is involved: the window runs over scratch stores, and channels
//! stand in for the layer worker and the device.
use super::preview_model_tests::ModelFixture;
use super::preview_model_tests::{chassis_of, class_of, controls_of, model_fixture, tab_order};
use super::*;
use sdeck_plus::{
    binding_editor::ControlKind,
    desktop_actions::{DesktopAction, MediaCommand},
    device_model::{DeviceModel, CLASSIC, PLUS, PLUS_XL, XL},
    features::FeatureAction,
    image_output,
    preview_layout::{ControlRef, PreviewLayout},
};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const BACKGROUND: ArtworkTarget = ArtworkTarget::TouchStripBackground;

/// A 480x272 picture of one color, outside the stores.
fn picture(color: u32) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "classic-background-ui-{}-{}.png",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let image =
        gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 480, 272).unwrap();
    image.fill(color);
    image.savev(&path, "png", &[]).unwrap();
    path
}

/// Removes the pictures made by [`picture`] however the test ends.
struct Pictures(Vec<std::path::PathBuf>);

impl Drop for Pictures {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
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

fn draws_background(command: &ArtworkCommand) -> bool {
    matches!(
        command,
        ArtworkCommand::Assign {
            target: BACKGROUND,
            ..
        } | ArtworkCommand::Clear { target: BACKGROUND }
    )
}

/// The page's background as stored.
fn stored(f: &ModelFixture, page: &str) -> Option<std::path::PathBuf> {
    f.artwork
        .store
        .borrow()
        .as_ref()
        .unwrap()
        .assigned_path_in_layer(page, BACKGROUND)
}

/// The size of what the preview picture of `target` shows, when it shows one.
fn shown(f: &ModelFixture, target: ArtworkTarget) -> Option<(i32, i32)> {
    let pictures = f.artwork.pictures.borrow();
    let (_, picture) = pictures.iter().find(|(shown, _)| *shown == target)?;
    if !picture.get_visible() {
        return None;
    }
    picture
        .paintable()
        .map(|paintable| (paintable.intrinsic_width(), paintable.intrinsic_height()))
}

/// Choose `path` as the background of the shown page, as the file chooser does.
fn choose(f: &ModelFixture, path: &std::path::Path) {
    control(f, ControlRef::Background).emit_clicked();
    let owner = f.artwork.capture_owner();
    f.artwork.import(&owner, path).unwrap();
}

fn media(command: MediaCommand) -> Action {
    Action::Feature(FeatureAction::Desktop(DesktopAction::Media(command)))
}

#[test]
fn the_classic_background_is_a_named_control_after_the_keys_that_opens_its_appearance() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("classic-background-select", &CLASSIC, |store, _| {
            store
                .bind(
                    "Base",
                    Trigger::KeyPress(1),
                    Action::Command("echo hi".into()),
                )
                .unwrap();
        }) else {
            return;
        };
        let ui = &f.widgets.binding_ui;
        let root: gtk::Widget = f.widgets.root.clone().upcast();
        let bar = control(&f, ControlRef::Background);
        assert!(bar.has_css_class("control-selectable"));
        assert!(bar.has_css_class("background-surface"));
        assert!(bar.is_focusable(), "reachable with the keyboard");
        assert_eq!(button_caption(&bar).as_deref(), Some("Background"));
        assert_eq!(
            bar.tooltip_text().unwrap(),
            "Background · Behind every key without a picture of its own"
        );
        let rect = PreviewLayout::for_model(&CLASSIC)
            .rect_of(ControlRef::Background)
            .unwrap();
        let chassis = chassis_of(&root);
        let bounds = bar.compute_bounds(&chassis).expect("on the chassis");
        assert_eq!(
            (
                bounds.x() as i32,
                bounds.y() as i32,
                bounds.width() as i32,
                bounds.height() as i32
            ),
            (rect.x, rect.y, rect.w, rect.h)
        );
        // Tab visits the keys row-major, then the background.
        let buttons = controls_of(&chassis);
        assert_eq!(buttons.len(), 16);
        assert_eq!(buttons.last(), Some(&bar));
        assert_eq!(tab_order(&f.window, &chassis, &buttons), buttons);
        assert!(!class_of(&root, "appearance-section").is_visible());
        // Key 1 runs a command: its field shows until the background is chosen.
        control(&f, ControlRef::Key(1)).emit_clicked();
        assert!(ui.text_row.is_visible());

        bar.emit_clicked();
        assert!(!ui.text_row.is_visible(), "the key's command field is gone");
        assert!(!ui.action_guidance.is_visible());
        assert!(!ui.return_toggle.is_visible());
        assert!(!ui.is_dirty());
        assert_eq!(ui.control_kind(), ControlKind::Background);
        assert_eq!(ui.selected_control(), Some(ControlRef::Background));
        assert!(bar.has_css_class("control-selected"));
        assert_eq!(heading(&f), "Configuring: Background");
        // It runs nothing: no event, action, hold or live information; its
        // picture is set in Appearance.
        assert!(!ui.event_picker.is_mapped() && !ui.action_picker.is_mapped());
        assert!(!ui.hold_toggle.is_visible());
        assert!(!ui.info_toggle.is_visible());
        assert!(class_of(&root, "appearance-section").is_visible());
        assert_eq!(
            f.widgets.artwork_section.label().as_deref(),
            Some("Background picture")
        );
        assert_eq!(f.artwork.selected_target.get(), BACKGROUND);
        let picker = &f.widgets.target_picker;
        let targets = picker
            .model()
            .unwrap()
            .downcast::<gtk::StringList>()
            .unwrap();
        assert_eq!(targets.n_items(), 1);
        assert_eq!(
            targets.string(picker.selected()).unwrap().as_str(),
            "Background"
        );
        // Save and Test say where the picture is chosen, and save nothing.
        while f.requests.as_ref().unwrap().try_recv().is_ok() {}
        click(&f, "Save binding");
        assert_eq!(
            f.widgets.toast.label.text(),
            "The background runs no action; choose its picture under Appearance."
        );
        assert!(f.requests.as_ref().unwrap().try_recv().is_err());
        // A key brings the key editor back.
        control(&f, ControlRef::Key(15)).emit_clicked();
        assert_eq!(heading(&f), "Configuring: Key 15");
        assert!(!class_of(&root, "appearance-section").is_visible());
    });
}

#[test]
fn a_page_background_shows_behind_every_key_without_a_picture_of_its_own() {
    run_gtk_test(|| {
        let pictures = Pictures(vec![picture(0xff0000ff), picture(0x00ff00ff)]);
        let (red, green) = (pictures.0[0].clone(), pictures.0[1].clone());
        let mut own = None;
        let fixture = model_fixture("classic-background-keys", &CLASSIC, |store, images| {
            own = Some(
                images
                    .assign_in_layer("Base", ArtworkTarget::Keypad(2), &green)
                    .unwrap(),
            );
            store
                .bind("Base", Trigger::KeyPress(3), media(MediaCommand::PlayPause))
                .unwrap();
            images
                .set_button_text_in_layer(
                    "Base",
                    ArtworkTarget::Keypad(4),
                    sdeck_plus::button_text::ButtonText {
                        text: "Hi".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        });
        let Some(f) = fixture else {
            return;
        };
        let own = own.unwrap();
        // Without a background the Classic gets key pictures only, as before.
        f.artwork.refresh(true);
        let before = sent(&f);
        assert!(!before.iter().any(draws_background), "{before:?}");
        assert!(matches!(
            &before[0],
            ArtworkCommand::Clear {
                target: ArtworkTarget::Keypad(1)
            }
        ));
        assert_eq!(shown(&f, ArtworkTarget::Keypad(1)), None);

        choose(&f, &red);
        let asset = stored(&f, "Base").expect("the page has its background");
        let commands = sent(&f);
        // The whole picture goes first: it covers every key, which follow.
        assert!(
            matches!(&commands[0], ArtworkCommand::Assign { target: BACKGROUND, path } if *path == asset),
            "{commands:?}"
        );
        assert_eq!(commands.iter().filter(|c| draws_background(c)).count(), 1);
        let share = |key| image_output::keypad_background_crop(&CLASSIC, &asset, key).unwrap();
        let key = |number: u8| {
            commands
                .iter()
                .find(|command| match command {
                    ArtworkCommand::Assign { target, .. }
                    | ArtworkCommand::Clear { target }
                    | ArtworkCommand::ComposeIcon { target, .. }
                    | ArtworkCommand::ComposeInfo { target, .. } => {
                        *target == ArtworkTarget::Keypad(number)
                    }
                    ArtworkCommand::ComposeButton { key, .. } => *key == number,
                    _ => false,
                })
                .unwrap_or_else(|| panic!("key {number} is drawn: {commands:?}"))
                .clone()
        };
        assert!(
            matches!(key(1), ArtworkCommand::Assign { path, .. } if path == share(1)),
            "an empty key shows its share"
        );
        assert!(
            matches!(key(2), ArtworkCommand::Assign { path, .. } if path == own),
            "a key with its own picture keeps it"
        );
        assert!(
            matches!(key(3), ArtworkCommand::ComposeIcon { path: Some(path), .. } if path == share(3)),
            "an icon is drawn over the key's share"
        );
        assert!(
            matches!(key(4), ArtworkCommand::ComposeButton { path: Some(path), .. } if path == share(4)),
            "text is drawn over the key's share"
        );
        for number in 5..=15 {
            assert!(
                matches!(key(number), ArtworkCommand::Assign { path, .. } if path == share(number)),
                "key {number}"
            );
        }
        // The preview shows the same: every key at its 72 px share, and the
        // bar shows the page's picture.
        assert_eq!(shown(&f, ArtworkTarget::Keypad(1)), Some((72, 72)));
        assert_eq!(shown(&f, ArtworkTarget::Keypad(15)), Some((72, 72)));
        assert_eq!(shown(&f, BACKGROUND), Some((480, 272)));
        // The device gets the share of key 1 in red, turned like any key.
        let ArtworkCommand::Assign { target, path } = key(1) else {
            unreachable!()
        };
        let jpeg = image_output::prepare_jpeg_for(&CLASSIC, target, &path).unwrap();
        let decoded = gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (72, 72));
        let bytes = decoded.read_pixel_bytes();
        let at = 36 * decoded.rowstride() as usize + 36 * decoded.n_channels() as usize;
        assert!(bytes[at] > 200 && bytes[at + 1] < 60, "red share");
    });
}

#[test]
fn each_keypad_page_has_its_own_background_and_a_page_without_one_clears_it() {
    run_gtk_test(|| {
        let pictures = Pictures(vec![picture(0xff0000ff)]);
        let red = pictures.0[0].clone();
        let mut base = None;
        let fixture = model_fixture("classic-background-pages", &CLASSIC, |store, images| {
            store.create("Studio").unwrap();
            base = Some(images.assign_in_layer("Base", BACKGROUND, &red).unwrap());
        });
        let Some(f) = fixture else {
            return;
        };
        let base = base.unwrap();
        sent(&f);
        f.artwork.refresh(true);
        let commands = sent(&f);
        assert!(
            matches!(&commands[0], ArtworkCommand::Assign { target: BACKGROUND, path } if *path == base)
        );
        assert_eq!(shown(&f, BACKGROUND), Some((480, 272)));

        // Another page has none: the device's LCD is cleared first, and its
        // keys are black again.
        f.widgets
            .feature_runtime
            .execute("test".into(), Action::SwitchLayer("Studio".into()));
        let commands = sent(&f);
        assert!(
            matches!(&commands[0], ArtworkCommand::Clear { target: BACKGROUND }),
            "{commands:?}"
        );
        assert_eq!(commands.iter().filter(|c| draws_background(c)).count(), 1);
        assert!(commands.iter().any(|command| matches!(
            command,
            ArtworkCommand::Clear {
                target: ArtworkTarget::Keypad(1)
            }
        )));
        assert_eq!(shown(&f, ArtworkTarget::Keypad(1)), None);
        assert_eq!(shown(&f, BACKGROUND), None);
        // Once cleared, later pages without one send nothing for it.
        f.artwork.refresh(true);
        assert!(!sent(&f).iter().any(draws_background));

        // Back on Base, its own background returns.
        f.widgets
            .feature_runtime
            .execute("test".into(), Action::SwitchLayer("Base".into()));
        let commands = sent(&f);
        assert!(
            matches!(&commands[0], ArtworkCommand::Assign { target: BACKGROUND, path } if *path == base),
            "{commands:?}"
        );
        assert_eq!(stored(&f, "Studio"), None);
    });
}

#[test]
fn removing_the_background_offers_undo_and_undo_refuses_after_a_later_change() {
    run_gtk_test(|| {
        let pictures = Pictures(vec![picture(0xff0000ff), picture(0x0000ffff)]);
        let (red, blue) = (pictures.0[0].clone(), pictures.0[1].clone());
        let fixture = model_fixture("classic-background-remove", &CLASSIC, |_, images| {
            images.assign_in_layer("Base", BACKGROUND, &red).unwrap();
        });
        let Some(f) = fixture else {
            return;
        };
        let first = stored(&f, "Base").unwrap();
        let toast = &f.widgets.toast;
        control(&f, ControlRef::Background).emit_clicked();
        sent(&f);
        click(&f, "Remove binding");
        assert_eq!(stored(&f, "Base"), None);
        assert!(!first.exists(), "the asset goes with it");
        assert!(toast.root.reveals_child());
        assert_eq!(toast.label.text(), "Background removed.");
        assert!(toast.action.is_visible());
        assert_eq!(toast.action.label().as_deref(), Some("Undo"));
        assert!(matches!(
            &sent(&f)[0],
            ArtworkCommand::Clear { target: BACKGROUND }
        ));

        toast.action.emit_clicked();
        let restored = stored(&f, "Base").expect("Undo brings the background back");
        assert_eq!(
            std::fs::read(&restored).unwrap(),
            std::fs::read(&red).unwrap()
        );
        assert!(
            matches!(&sent(&f)[0], ArtworkCommand::Assign { target: BACKGROUND, path } if *path == restored)
        );
        assert!(!toast.action.is_visible(), "Undo runs once");

        // Undo never replaces a background chosen after the removal.
        click(&f, "Remove binding");
        assert_eq!(toast.label.text(), "Background removed.");
        let undo = toast.action.clone();
        choose(&f, &blue);
        let later = stored(&f, "Base").unwrap();
        undo.emit_clicked();
        assert_eq!(stored(&f, "Base"), Some(later));
        assert_eq!(
            toast.label.text(),
            "Cannot undo: the background was changed after it was removed."
        );

        // A page without a background has nothing to remove.
        click(&f, "Remove binding");
        click(&f, "Remove binding");
        assert_eq!(toast.label.text(), "This page has no background.");
        assert!(!toast.action.is_visible());
    });
}

#[test]
fn no_other_model_offers_a_keypad_background() {
    run_gtk_test(|| {
        let neo = DeviceModel::by_id("neo").unwrap();
        for model in [&PLUS, &PLUS_XL, &XL, neo] {
            let Some(f) = model_fixture("classic-background-others", model, |_, _| {}) else {
                return;
            };
            assert!(
                f.widgets.preview.control(ControlRef::Background).is_none(),
                "{}",
                model.id
            );
            let targets = f
                .widgets
                .target_picker
                .model()
                .unwrap()
                .downcast::<gtk::StringList>()
                .unwrap();
            assert!(
                (0..targets.n_items())
                    .all(|index| targets.string(index).unwrap().as_str() != "Background"),
                "{}",
                model.id
            );
            assert_eq!(
                f.widgets
                    .binding_ui
                    .select_control(ControlKind::Background, Some(1), false),
                Err(format!("the {} has no keypad background", model.name))
            );
            let root: gtk::Widget = f.widgets.root.clone().upcast();
            assert!(!controls_of(&chassis_of(&root))
                .iter()
                .any(|button| button.has_css_class("background-surface")));
        }
    });
}
