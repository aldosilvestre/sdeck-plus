//! The preview follows the shown device model: the Stream Deck + keeps its
//! shipped look, the XL+ gets 36 keys, six dials and six touch sections, and a
//! model change rebuilds the preview without leaving anything behind.
use super::*;
use gtk::prelude::GtkWindowExt;
use sdeck_plus::preview_layout::ControlRef;
use sdeck_plus::{
    device_model::{DeviceModel, PLUS, PLUS_XL},
    local_artwork::ArtworkStore,
    preview_layout::PreviewLayout,
};
use ui_refresh_tests::fixture;

pub(super) fn descendants_of<T: IsA<gtk::Widget> + glib::object::IsClass>(
    widget: &gtk::Widget,
) -> Vec<T> {
    let mut found = Vec::new();
    if let Ok(this) = widget.clone().downcast::<T>() {
        found.push(this);
    }
    let mut child = widget.first_child();
    while let Some(next) = child {
        found.extend(descendants_of::<T>(&next));
        child = next.next_sibling();
    }
    found
}

/// Removes what a fixture created, whichever way the test ends.
pub(super) struct Cleanup {
    root: std::path::PathBuf,
    window: ApplicationWindow,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.window.destroy();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A production window over `model`'s stores, at the default window size.
pub(super) struct ModelFixture {
    pub(super) cleanup: Cleanup,
    pub(super) window: ApplicationWindow,
    pub(super) widgets: MonitorWidgets,
    pub(super) layers: Rc<RefCell<LayerStore>>,
    pub(super) artwork: ArtworkUi,
    pub(super) requests: Option<mpsc::Receiver<LayerCommand>>,
    pub(super) commands: mpsc::Sender<LayerCommand>,
}

pub(super) fn model_fixture(
    tag: &str,
    model: &'static DeviceModel,
    setup: impl FnOnce(&mut LayerStore, &mut ArtworkStore),
) -> Option<ModelFixture> {
    if !init_gtk_for_test() {
        return None;
    }
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let root = std::env::temp_dir().join(format!(
        "preview-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut store = LayerStore::open_at_model(root.join("layers"), model).unwrap();
    let mut images =
        ArtworkStore::open_at_for_layer_model(root.join("artwork"), Some("Base"), model).unwrap();
    setup(&mut store, &mut images);
    let layers = Rc::new(RefCell::new(store));
    let (output, _hardware) = mpsc::channel();
    let artwork = ArtworkUi {
        store: Rc::new(RefCell::new(Some(images))),
        layers: layers.clone(),
        pictures: Rc::new(RefCell::new(Vec::new())),
        output,
        last_layer: Rc::new(RefCell::new(None)),
        chooser_owners: Rc::new(RefCell::new(Default::default())),
        selected_target: Rc::new(Cell::new(ArtworkTarget::Keypad(1))),
        text_editor: Rc::new(RefCell::new(std::rc::Weak::new())),
        info: Rc::default(),
    };
    let (commands, requests) = mpsc::channel();
    let window = ApplicationWindow::builder()
        .default_width(DEFAULT_WIDTH)
        .default_height(DEFAULT_HEIGHT)
        .build();
    install_style(&window);
    let widgets = build_widgets(
        window.clone(),
        layers.clone(),
        artwork.clone(),
        commands.clone(),
    );
    let stage = gtk::Fixed::new();
    widgets.root.set_size_request(DEFAULT_WIDTH, DEFAULT_HEIGHT);
    stage.put(&widgets.root, 0.0, 0.0);
    window.set_child(Some(&stage));
    window.present();
    wait_for_frame(&window);
    Some(ModelFixture {
        cleanup: Cleanup {
            root,
            window: window.clone(),
        },
        window,
        widgets,
        layers,
        artwork,
        requests: Some(requests),
        commands,
    })
}

/// The ScaledBin that holds the preview card.
pub(super) fn preview_of(root: &gtk::Widget) -> gtk::Widget {
    descendants_of::<gtk::Widget>(root)
        .into_iter()
        .find(|widget| widget.has_css_class("device-preview"))
        .expect("the preview")
}

pub(super) fn class_of(root: &gtk::Widget, class: &str) -> gtk::Widget {
    descendants_of::<gtk::Widget>(root)
        .into_iter()
        .find(|widget| widget.has_css_class(class))
        .unwrap_or_else(|| panic!("no widget with class {class}"))
}

/// What a rendered preview looks like, for comparisons and files.
pub(super) struct Rendered {
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) pixels: Vec<u8>,
}

impl Rendered {
    pub(super) fn hash(&self) -> u64 {
        self.pixels
            .iter()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            })
    }

    /// More than one colour is drawn.
    pub(super) fn is_blank(&self) -> bool {
        self.pixels
            .chunks(4)
            .all(|pixel| pixel == &self.pixels[..4])
    }
}

/// Render the preview of `window` (no device involved) and, when `png` is
/// given, write it there. The documentation screenshots use the same path.
pub(super) fn render_preview(
    window: &ApplicationWindow,
    preview: &gtk::Widget,
    png: Option<&std::path::Path>,
) -> Rendered {
    wait_for_frame(window);
    let texture = rendered_texture(window, preview);
    if let Some(path) = png {
        texture.save_to_png(path).unwrap();
    }
    Rendered {
        width: preview.width(),
        height: preview.height(),
        pixels: rendered_pixels(window, preview),
    }
}

/// Selectable controls of `chassis` in widget (and so Tab) order.
fn controls_of(chassis: &gtk::Widget) -> Vec<Button> {
    descendants_of::<Button>(chassis)
        .into_iter()
        .filter(|button| button.has_css_class("control-selectable"))
        .collect()
}

fn kind_of(button: &Button) -> &'static str {
    if button.has_css_class("key-surface") {
        "key"
    } else if button.has_css_class("strip-zone") {
        "zone"
    } else if button.has_css_class("strip-surface") {
        "strip"
    } else if button.has_css_class("encoder") {
        "dial"
    } else {
        "other"
    }
}

fn bounds_in(widget: &impl IsA<gtk::Widget>, chassis: &gtk::Widget) -> (i32, i32, i32, i32) {
    let bounds = widget.compute_bounds(chassis).expect("bounds");
    (
        bounds.x().round() as i32,
        bounds.y().round() as i32,
        bounds.width().round() as i32,
        bounds.height().round() as i32,
    )
}

fn chassis_of(root: &gtk::Widget) -> gtk::Widget {
    class_of(root, "hardware-chassis")
}

/// Counts by kind, in order of first appearance.
fn shape(buttons: &[Button]) -> Vec<(&'static str, usize)> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for button in buttons {
        let kind = kind_of(button);
        match counts.last_mut() {
            Some((last, count)) if *last == kind => *count += 1,
            _ => counts.push((kind, 1)),
        }
    }
    counts
}

/// The controls Tab visits inside the chassis, from its start.
fn tab_order(
    window: &ApplicationWindow,
    chassis: &gtk::Widget,
    controls: &[Button],
) -> Vec<Button> {
    GtkWindowExt::set_focus(window, None::<&gtk::Widget>);
    let mut visited: Vec<Button> = Vec::new();
    for _ in 0..controls.len() + 2 {
        if !chassis.child_focus(gtk::DirectionType::TabForward) {
            break;
        }
        let Some(focused) = GtkWindowExt::focus(window) else {
            break;
        };
        let Ok(button) = focused.downcast::<Button>() else {
            continue;
        };
        if visited.contains(&button) || !controls.contains(&button) {
            break;
        }
        visited.push(button);
    }
    visited
}

const PLUS_GOLDEN_HASH: u64 = 0x9d8dea7cc9a97e57;

/// The look settings the golden is rendered with, so the picture does not
/// depend on the host's theme, font or display settings (nor on HOME).
/// Restores what it replaced when dropped.
struct ExplicitLook {
    settings: gtk::Settings,
    saved: Vec<(&'static str, glib::Value)>,
}

impl ExplicitLook {
    fn apply() -> Self {
        let settings = gtk::Settings::default().unwrap();
        let wanted: [(&'static str, glib::Value); 9] = [
            ("gtk-theme-name", "Adwaita".to_value()),
            ("gtk-application-prefer-dark-theme", false.to_value()),
            ("gtk-font-name", "Sans 10".to_value()),
            ("gtk-xft-dpi", (96 * 1024).to_value()),
            ("gtk-xft-antialias", 1.to_value()),
            ("gtk-xft-hinting", 1.to_value()),
            ("gtk-xft-hintstyle", "hintslight".to_value()),
            ("gtk-xft-rgba", "none".to_value()),
            ("gtk-enable-animations", false.to_value()),
        ];
        let mut saved = Vec::new();
        for (name, value) in wanted {
            saved.push((name, settings.property_value(name)));
            settings.set_property_from_value(name, &value);
        }
        Self { settings, saved }
    }
}

impl Drop for ExplicitLook {
    fn drop(&mut self) {
        for (name, value) in &self.saved {
            self.settings.set_property_from_value(name, value);
        }
    }
}

/// Captured from the unmodified Stream Deck + preview (T4a code) rendered with
/// [`ExplicitLook`] at the default window size: 520x428 around the 440x428 card.
#[test]
fn plus_preview_is_identical_to_the_shipped_one() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let _look = ExplicitLook::apply();
        let Some(f) = fixture("plus-golden", |_| {}) else {
            return;
        };
        let root: gtk::Widget = f.widgets.root.clone().upcast();
        let chassis = chassis_of(&root);
        assert_eq!((chassis.width(), chassis.height()), (426, 414));
        let card = class_of(&root, "device-canvas");
        let card_bounds = card.compute_bounds(&preview_of(&root)).unwrap();
        assert_eq!(
            (
                card_bounds.width().round() as i32,
                card_bounds.height().round() as i32
            ),
            (440, 428),
            "the card contract of DESIGN.md"
        );
        let buttons = controls_of(&chassis);
        assert_eq!(
            shape(&buttons),
            [("key", 8), ("strip", 1), ("zone", 4), ("dial", 4)]
        );
        let expected: Vec<(i32, i32, i32, i32)> = [38, 105]
            .into_iter()
            .flat_map(|y| [48, 139, 230, 321].map(|x| (x, y, 60, 60)))
            .chain([(50, 202, 328, 41)])
            .chain([50, 132, 214, 296].map(|x| (x, 202, 82, 41)))
            .chain([55, 146, 237, 328].map(|x| (x, 307, 46, 46)))
            .collect();
        let found: Vec<_> = buttons
            .iter()
            .map(|button| bounds_in(button, &chassis))
            .collect();
        assert_eq!(found, expected);
        let brand = class_of(&chassis, "hardware-brand")
            .downcast::<Label>()
            .unwrap();
        assert_eq!(brand.text(), "STREAM DECK +");
        assert_eq!(bounds_in(&brand, &chassis), (0, 6, 426, 24));
        assert_eq!(
            bounds_in(&class_of(&chassis, "hardware-bezel"), &chassis),
            (27, 177, 374, 77)
        );
        assert_eq!(
            bounds_in(&class_of(&chassis, "hardware-rail"), &chassis),
            (27, 273, 374, 108)
        );
        for (index, zone) in buttons[9..13].iter().enumerate() {
            assert_eq!(
                zone.tooltip_text().unwrap(),
                format!("Touch zone {}", index + 1)
            );
        }
        assert!(buttons[0].has_css_class("control-selected"));
        assert_eq!(
            tab_order(&f.window, &chassis, &buttons)
                .iter()
                .map(kind_of)
                .collect::<Vec<_>>(),
            buttons.iter().map(kind_of).collect::<Vec<_>>(),
            "Tab keeps visiting keys, the strip, its sections, then the dials"
        );
        let rendered = render_preview(&f.window, &preview_of(&root), None);
        assert_eq!((rendered.width, rendered.height), (520, 428));
        assert_eq!(
            rendered.hash(),
            PLUS_GOLDEN_HASH,
            "the Stream Deck + preview must render exactly as before"
        );
    });
}

#[test]
fn xl_preview_has_36_keys_six_dials_and_six_aligned_sections() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("xl-shape", &PLUS_XL, |_, _| {}) else {
            return;
        };
        let root: gtk::Widget = f.widgets.root.clone().upcast();
        let chassis = chassis_of(&root);
        let layout = PreviewLayout::for_model(&PLUS_XL);
        assert_eq!((chassis.width(), chassis.height()), layout.size);
        let buttons = controls_of(&chassis);
        assert_eq!(
            shape(&buttons),
            [("key", 36), ("strip", 1), ("zone", 6), ("dial", 6)]
        );
        let brand = class_of(&chassis, "hardware-brand")
            .downcast::<Label>()
            .unwrap();
        assert_eq!(brand.text(), "STREAM DECK + XL");
        // Every widget sits exactly where the layout put it.
        for (button, control) in buttons.iter().zip(layout.controls()) {
            let rect = layout.rect_of(control).unwrap();
            assert_eq!(
                bounds_in(button, &chassis),
                (rect.x, rect.y, rect.w, rect.h),
                "{control:?}"
            );
        }
        let zones = &buttons[37..43];
        for (index, zone) in zones.iter().enumerate() {
            assert_eq!(
                zone.tooltip_text().unwrap(),
                format!("Touch zone {}", index + 1)
            );
        }
        // Each section is centred over its dial.
        for (zone, dial) in zones.iter().zip(&buttons[43..]) {
            let (zone, dial) = (bounds_in(zone, &chassis), bounds_in(dial, &chassis));
            assert_eq!(zone.0 + zone.2 / 2, dial.0 + dial.2 / 2);
        }
        assert_eq!(f.widgets.preview.key_widgets().len(), 36);
        assert_eq!(f.widgets.preview.zone_widgets().len(), 6);
        assert_eq!(f.widgets.preview.dial_widgets().len(), 6);
        assert_eq!(f.widgets.preview.key_labels().len(), 36);
        assert_eq!(f.widgets.preview.dial_labels().len(), 6);
        // Only the preview's own pictures exist: 36 keys, the strip and 6 sections.
        assert_eq!(f.artwork.pictures.borrow().len(), 43);
        assert_eq!(
            tab_order(&f.window, &chassis, &buttons)
                .iter()
                .map(kind_of)
                .collect::<Vec<_>>(),
            buttons.iter().map(kind_of).collect::<Vec<_>>(),
            "Tab order: keys row-major, strip, sections, dials"
        );
        let visited = tab_order(&f.window, &chassis, &buttons);
        assert_eq!(
            visited, buttons,
            "every control is reachable in widget order"
        );
    });
}

#[test]
fn the_preview_contract_holds_at_the_default_window_for_both_models() {
    run_gtk_test(|| {
        for model in [&PLUS, &PLUS_XL] {
            let Some(f) = model_fixture("contract", model, |_, _| {}) else {
                return;
            };
            let root: gtk::Widget = f.widgets.root.clone().upcast();
            let bin = preview_of(&root);
            let layout = PreviewLayout::for_model(model);
            assert!(
                descendants_of::<gtk::ScrolledWindow>(&bin).is_empty(),
                "{}: the preview never scrolls inside its card",
                model.id
            );
            // The card is the chassis plus its 14 px frame, shown at scale 1.
            let card = class_of(&root, "device-canvas");
            let card_bounds = card.compute_bounds(&bin).unwrap();
            assert_eq!(
                (
                    card_bounds.width().round() as i32,
                    card_bounds.height().round() as i32
                ),
                (layout.size.0 + 14, layout.size.1 + 14),
                "{}",
                model.id
            );
            let chassis = chassis_of(&root);
            let scale = chassis.compute_transform(&bin).unwrap().to_2d().unwrap().0 as f32;
            assert!(
                (1.0..=PREVIEW_MAX_SCALE as f32).contains(&scale),
                "{}",
                model.id
            );
            assert!(
                (scale - 1.0).abs() < 0.001,
                "natural size at the default window"
            );
            // The preview sits beside the catalog, entirely inside the window.
            let window_bounds = bin
                .compute_bounds(&f.widgets.root.child().unwrap())
                .unwrap();
            assert!(window_bounds.x() + window_bounds.width() <= DEFAULT_WIDTH as f32);
            let sidebar = class_of(&root, "settings-sidebar");
            let side = sidebar
                .compute_bounds(&f.widgets.root.child().unwrap())
                .unwrap();
            assert!(
                side.x() >= window_bounds.x() + card_bounds.width(),
                "{}: the catalog stays in its own column",
                model.id
            );
            // Every control is a usable target and is not clipped.
            for button in controls_of(&chassis) {
                let bounds = button.compute_bounds(&bin).unwrap();
                assert!(
                    bounds.width() >= 24.0 && bounds.height() >= 24.0,
                    "{}",
                    model.id
                );
                assert!(bounds.x() >= 0.0 && bounds.x() + bounds.width() <= bin.width() as f32);
            }
            assert!(!render_preview(&f.window, &bin, None).is_blank());
        }
    });
}

/// Writes one PNG per model when asked; `cargo test` does not touch the docs.
#[test]
fn previews_render_to_images_for_every_supported_model_without_a_device() {
    run_gtk_test(|| {
        let directory =
            std::env::var_os("STREAMDECK_PREVIEW_PNG_DIR").map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut sizes = Vec::new();
        for model in DeviceModel::supported() {
            let Some(f) = model_fixture("render", model, |_, _| {}) else {
                return;
            };
            let preview = preview_of(f.widgets.root.upcast_ref());
            let png = directory
                .as_ref()
                .map(|directory| directory.join(format!("preview-{}.png", model.id)));
            let rendered = render_preview(&f.window, &preview, png.as_deref());
            assert!(!rendered.is_blank(), "{} renders something", model.id);
            assert!(
                rendered.width as usize * rendered.height as usize * 4 == rendered.pixels.len()
            );
            if let Some(png) = png {
                assert!(std::fs::metadata(png).unwrap().len() > 1000);
            }
            sizes.push((model.id, rendered.width, rendered.height, rendered.hash()));
        }
        assert_eq!(sizes.len(), 2);
        assert_ne!(sizes[0].3, sizes[1].3, "each model draws its own preview");
        assert!(
            sizes[1].1 > sizes[0].1,
            "the XL+ preview is wider than the +"
        );
    });
}

/// Run the main loop until `done`, which a real signal must make true.
fn pump_until(mut done: impl FnMut() -> bool) {
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let timeout = glib::timeout_add_local_once(Duration::from_secs(5), move || flag.set(true));
    let context = glib::MainContext::default();
    while !done() && !expired.get() {
        context.iteration(true);
    }
    assert!(
        done(),
        "GTK did not apply the activation before the timeout"
    );
    if !expired.get() {
        timeout.remove();
    }
}

/// Space or Enter on the focused control: GTK clicks the button after its
/// short pressed state, so the selection arrives with a later main-loop turn.
fn activate(preview: &Rc<preview::PreviewView>, control: ControlRef) {
    let widget = preview
        .control(control)
        .unwrap_or_else(|| panic!("{control:?} is not on the shown preview"));
    assert!(
        widget.activate(),
        "{control:?} activates like Space or Enter"
    );
    let view = preview.clone();
    pump_until(move || view.selected() == control);
}

fn shown_shape(root: &gtk::Widget) -> Vec<(&'static str, usize)> {
    shape(&controls_of(&chassis_of(root)))
}

#[test]
fn every_control_of_every_model_selects_when_activated_from_the_keyboard() {
    run_gtk_test(|| {
        for model in [&PLUS, &PLUS_XL] {
            let Some(f) = model_fixture("keyboard", model, |_, _| {}) else {
                return;
            };
            let preview = f.widgets.preview.clone();
            let layout = PreviewLayout::for_model(model);
            let ui = &f.widgets.binding_ui;
            let controls = layout.controls();
            let keyboard = [
                ControlRef::Key(1),
                ControlRef::Key(model.keys()),
                ControlRef::Strip,
                ControlRef::Zone(1),
                ControlRef::Zone(model.zones()),
                ControlRef::Dial(1),
                ControlRef::Dial(model.dials),
            ];
            for control in controls {
                // Every control answers a click; the keyboard path is the same
                // button activation, checked at both ends of each group.
                if keyboard.contains(&control) && preview.selected() != control {
                    activate(&preview, control);
                } else {
                    preview
                        .control(control)
                        .unwrap()
                        .downcast::<Button>()
                        .unwrap()
                        .emit_clicked();
                }
                assert_eq!(preview.selected(), control, "{}", model.id);
                assert_eq!(ui.selected_control(), Some(control), "{}", model.id);
                let selected: Vec<_> = layout
                    .controls()
                    .into_iter()
                    .filter(|other| {
                        preview
                            .control(*other)
                            .is_some_and(|widget| widget.has_css_class("control-selected"))
                    })
                    .collect();
                assert_eq!(selected, [control], "exactly one control is highlighted");
            }
        }
    });
}

#[test]
fn a_switch_rebuilds_the_preview_and_keeps_the_selection_only_when_it_still_exists() {
    run_gtk_test(|| {
        let Some(h) = model_session_tests::start("preview-switch", |_| {}) else {
            return;
        };
        let root: gtk::Widget = h.window.clone().upcast();
        let preview = h
            .session
            .runtime()
            .preview()
            .expect("the preview is attached");
        let ui = h.session.runtime().binding_ui().clone();
        let plus = [("key", 8), ("strip", 1), ("zone", 4), ("dial", 4)];
        let xl = [("key", 36), ("strip", 1), ("zone", 6), ("dial", 6)];
        assert_eq!(shown_shape(&root), plus);
        let shipped = render_preview(&h.window, &preview_of(&root), None);

        // A dial both models have stays selected; so does a section.
        activate(&preview, ControlRef::Dial(2));
        h.preview_as(&PLUS_XL);
        assert_eq!(h.model().id, "plus-xl");
        assert_eq!(shown_shape(&root), xl);
        assert_eq!(preview.selected(), ControlRef::Dial(2));
        assert_eq!(ui.selected_control(), Some(ControlRef::Dial(2)));
        assert!(preview
            .control(ControlRef::Dial(2))
            .unwrap()
            .has_css_class("control-selected"));
        let brand = class_of(&root, "hardware-brand")
            .downcast::<Label>()
            .unwrap();
        assert_eq!(brand.text(), "STREAM DECK + XL");
        assert_eq!(
            (
                ui.summaries.len(),
                ui.dial_legends.len(),
                ui.dial_icons.len()
            ),
            (43, 6, 6)
        );
        // The editor's numbers follow the model.
        assert_eq!(ui.control_number_model.n_items(), 6, "dials 1-6");

        // Controls only the XL+ has can be selected there, and reset on the way back.
        for control in [
            ControlRef::Key(30),
            ControlRef::Dial(6),
            ControlRef::Zone(6),
        ] {
            activate(&preview, control);
            assert_eq!(ui.selected_control(), Some(control));
            h.preview_as(&PLUS);
            assert_eq!(shown_shape(&root), plus);
            assert_eq!(
                preview.selected(),
                ControlRef::Key(1),
                "{control:?} does not exist on the +"
            );
            assert_eq!(ui.selected_control(), Some(ControlRef::Key(1)));
            assert!(preview
                .control(ControlRef::Key(1))
                .unwrap()
                .has_css_class("control-selected"));
            h.preview_as(&PLUS_XL);
        }
        activate(&preview, ControlRef::Zone(3));
        h.preview_as(&PLUS);
        assert_eq!(
            preview.selected(),
            ControlRef::Zone(3),
            "a section the + has is kept"
        );
        assert_eq!(ui.control_number_model.n_items(), 5, "Any and sections 1-4");
        let brand = class_of(&root, "hardware-brand")
            .downcast::<Label>()
            .unwrap();
        assert_eq!(brand.text(), "STREAM DECK +");
        // Back on the + the preview is the shipped one again.
        activate(&preview, ControlRef::Key(1));
        let rendered = render_preview(&h.window, &preview_of(&root), None);
        assert_eq!(
            (rendered.width, rendered.height),
            (shipped.width, shipped.height)
        );
        assert_eq!(
            rendered.hash(),
            shipped.hash(),
            "+ -> XL -> + renders as before"
        );
    });
}

#[test]
fn twenty_alternating_switches_free_every_replaced_preview_widget() {
    run_gtk_test(|| {
        let Some(h) = model_session_tests::start("preview-churn", |_| {}) else {
            return;
        };
        let root: gtk::Widget = h.window.clone().upcast();
        let runtime = h.session.runtime().clone();
        let preview = runtime.preview().unwrap();
        let ui = runtime.binding_ui().clone();
        let widgets_before = model_session_tests::count_widgets(&root);
        let view_refs = Rc::strong_count(&preview);
        let ui_refs = (
            Rc::strong_count(&ui.baseline),
            Rc::strong_count(&runtime.artwork_ui().pictures),
            Rc::strong_count(runtime.layers()),
        );
        let handlers = preview.bound_handlers();
        // The widgets every model shares live on by design.
        let shared: Vec<gtk::Widget> = [
            preview.touch_status().clone().upcast(),
            ui.gesture_badges.clone().upcast(),
        ]
        .into_iter()
        .chain(ui.dial_icons.all().iter().map(|icon| icon.clone().upcast()))
        .chain(
            ui.dial_legends
                .all()
                .iter()
                .map(|legend| legend.clone().upcast()),
        )
        .collect();
        let mut dead: Vec<glib::WeakRef<gtk::Widget>> = Vec::new();
        let mut rounds = Vec::new();
        for round in 0..20 {
            // Everything of the tree that is about to be replaced.
            let old = descendants_of::<gtk::Widget>(&chassis_of(&root));
            dead.extend(
                old.iter()
                    .filter(|widget| {
                        !shared
                            .iter()
                            .any(|shared| *widget == shared || widget.is_ancestor(shared))
                    })
                    .map(ObjectExt::downgrade),
            );
            drop(old);
            h.preview_as(if round % 2 == 0 { &PLUS_XL } else { &PLUS });
            rounds.push((
                preview.bound_handlers(),
                runtime.artwork_ui().pictures.borrow().len(),
            ));
            // Drive the editor and live paths through the new tree too.
            activate(&preview, ControlRef::Key(1));
            let flow = preview.key_widgets();
            assert!(!flow.is_empty());
        }
        assert_eq!(h.model().id, "plus");
        assert!(
            dead.iter().all(|weak| weak.upgrade().is_none()),
            "{} replaced widgets are still alive",
            dead.iter().filter(|weak| weak.upgrade().is_some()).count()
        );
        assert_eq!(model_session_tests::count_widgets(&root), widgets_before);
        assert_eq!(
            Rc::strong_count(&preview),
            view_refs,
            "nothing keeps the view alive"
        );
        assert_eq!(
            (
                Rc::strong_count(&ui.baseline),
                Rc::strong_count(&runtime.artwork_ui().pictures),
                Rc::strong_count(runtime.layers())
            ),
            ui_refs,
            "no handler of a dropped preview still holds shared state"
        );
        // The captions every model shares keep exactly the handlers of the model on show.
        assert_eq!(preview.bound_handlers(), handlers);
        for (index, (bound, pictures)) in rounds.iter().enumerate() {
            let xl = index % 2 == 0;
            assert_eq!(
                *bound,
                if xl { 36 + 1 + 6 } else { 8 + 1 + 4 },
                "round {index}"
            );
            assert_eq!(*pictures, if xl { 43 } else { 13 }, "round {index}");
        }
    });
}

fn controller<T: IsA<gtk::EventController> + IsA<glib::Object> + glib::object::IsClass>(
    widget: &gtk::Widget,
    moves: bool,
) -> T {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| controllers.item(index)?.downcast::<T>().ok())
        .find(|controller| {
            controller
                .upcast_ref::<gtk::EventController>()
                .downcast_ref::<gtk::DropTarget>()
                .is_none_or(|target| (target.actions() == gtk::gdk::DragAction::MOVE) == moves)
        })
        .expect("native controller")
}

#[test]
fn dragging_xl_key_9_onto_key_36_swaps_their_bindings_and_artwork() {
    run_gtk_test(|| {
        let source =
            std::env::temp_dir().join(format!("xl-swap-source-{}.png", std::process::id()));
        let image =
            gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 2, 2).unwrap();
        image.fill(0x4488ffff);
        image.savev(&source, "png", &[]).unwrap();
        let first_image = Rc::new(RefCell::new(None));
        let slot = first_image.clone();
        let image_source = source.clone();
        let Some(mut f) = model_fixture("xl-swap", &PLUS_XL, move |store, images| {
            for (key, text) in [(9, "first"), (36, "second")] {
                store
                    .bind("Base", Trigger::KeyPress(key), Action::Command(text.into()))
                    .unwrap();
            }
            store.persist().unwrap();
            *slot.borrow_mut() = Some(
                images
                    .assign_in_layer("Base", ArtworkTarget::Keypad(9), &image_source)
                    .unwrap(),
            );
        }) else {
            let _ = std::fs::remove_file(&source);
            return;
        };
        let first_image = first_image.borrow().clone().unwrap();
        let preview = f.widgets.preview.clone();
        let from = preview.control(ControlRef::Key(9)).unwrap();
        let to = preview.control(ControlRef::Key(36)).unwrap();
        let provider = controller::<gtk::DragSource>(&from, true)
            .emit_by_name::<Option<gtk::gdk::ContentProvider>>("prepare", &[&0.0f64, &0.0f64])
            .unwrap();
        let value = provider.value(glib::BoxedAnyObject::static_type()).unwrap();
        let boxed = glib::BoxedValue(value);
        let target = controller::<gtk::DropTarget>(&to, true);
        target.emit_by_name::<gtk::gdk::DragAction>("enter", &[&0.0f64, &0.0f64]);
        assert!(to.has_css_class("swap-target"));
        // Dropping a key on itself does nothing; on another key it queues the swap.
        let own = controller::<gtk::DropTarget>(&from, true);
        assert!(!own.emit_by_name::<bool>("drop", &[&boxed, &0.0f64, &0.0f64]));
        assert!(target.emit_by_name::<bool>("drop", &[&boxed, &0.0f64, &0.0f64]));
        assert!(!to.has_css_class("swap-target"));
        // The serial worker performs the exchange on disk, then the window applies it.
        f.commands.send(LayerCommand::Shutdown).unwrap();
        let (output, events) = mpsc::channel();
        feature_runtime::run_worker(
            f.requests.take().unwrap(),
            output,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let Ok(MonitorEvent::KeySwapResult {
            layer,
            first,
            second,
            result,
        }) = events.try_recv()
        else {
            panic!("the worker answers with the swap result");
        };
        assert_eq!((first, second), (9, 36));
        result.as_ref().expect("the swap persists");
        f.widgets
            .feature_runtime
            .complete_key_swap(&layer, first, second, result);
        let layers = f.layers.borrow();
        assert_eq!(
            layers.binding("Base", &Trigger::KeyPress(9)),
            Some(Action::Command("second".into()))
        );
        assert_eq!(
            layers.binding("Base", &Trigger::KeyPress(36)),
            Some(Action::Command("first".into()))
        );
        drop(layers);
        let root = f.cleanup.root.clone();
        let disk = LayerStore::open_at_model(root.join("layers"), &PLUS_XL).unwrap();
        assert_eq!(
            disk.binding("Base", &Trigger::KeyPress(36)),
            Some(Action::Command("first".into()))
        );
        let images =
            ArtworkStore::open_at_for_layer_model(root.join("artwork"), Some("Base"), &PLUS_XL)
                .unwrap();
        assert_eq!(
            images.assigned_path_in_layer("Base", ArtworkTarget::Keypad(36)),
            Some(first_image)
        );
        assert!(images
            .assigned_path_in_layer("Base", ArtworkTarget::Keypad(9))
            .is_none());
        let _ = std::fs::remove_file(&source);
    });
}

#[test]
fn dial_6_shows_its_action_caption_and_hover_summary_on_the_xl() {
    run_gtk_test(|| {
        let volume = Action::Feature(sdeck_plus::features::FeatureAction::Desktop(
            sdeck_plus::desktop_actions::DesktopAction::Media(
                sdeck_plus::desktop_actions::MediaCommand::VolumeUp,
            ),
        ));
        let bound = volume.clone();
        let Some(f) = model_fixture("xl-legend", &PLUS_XL, move |store, _| {
            store
                .bind("Base", Trigger::EncoderClockwise(6), bound)
                .unwrap();
            store
                .bind(
                    "Base",
                    Trigger::KeyPress(30),
                    Action::Command("echo thirty".into()),
                )
                .unwrap();
        }) else {
            return;
        };
        let root: gtk::Widget = f.widgets.root.clone().upcast();
        let ui = &f.widgets.binding_ui;
        let summary = ui
            .summary(ControlRef::Dial(6))
            .expect("dial 6 has a summary");
        assert!(summary.text().contains("Volume"), "{}", summary.text());
        let legend = &ui.dial_legends[5];
        let Action::Feature(action) = &volume else {
            unreachable!()
        };
        assert!(legend.is_visible());
        assert_eq!(legend.text(), action.summary());
        assert!(
            !ui.dial_legends[0].is_visible(),
            "unassigned dials stay unlabeled"
        );
        let chassis = chassis_of(&root);
        let dial = f.widgets.preview.control(ControlRef::Dial(6)).unwrap();
        assert_eq!(
            legend.parent(),
            dial.parent(),
            "caption and knob share the rail"
        );
        let (knob, caption) = (bounds_in(&dial, &chassis), bounds_in(legend, &chassis));
        assert!(
            caption.1 >= knob.1 + knob.3,
            "the caption sits below its dial"
        );
        assert!((caption.0 + caption.2 / 2 - knob.0 - knob.2 / 2).abs() <= 1);
        // The hover text names the control on the model's own numbering.
        assert_eq!(
            dial.tooltip_text().unwrap(),
            format!("Encoder 6 · {}", summary.text().replace('\n', " · ")),
        );
        let key = f.widgets.preview.control(ControlRef::Key(30)).unwrap();
        assert!(key.tooltip_text().unwrap().starts_with("Button 30 · "));
        assert!(key.tooltip_text().unwrap().contains("echo thirty"));
        // Unbinding clears the caption on the next refresh.
        f.layers
            .borrow_mut()
            .unbind("Base", &Trigger::EncoderClockwise(6))
            .unwrap();
        ui.refresh_summaries(&f.layers);
        assert!(!legend.is_visible());
        assert!(!dial.tooltip_text().unwrap().contains("Volume"));
    });
}

#[test]
fn xl_input_lights_key_30_dial_5_and_zone_6_and_ignores_what_the_model_lacks() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("xl-feedback", &PLUS_XL, |_, _| {}) else {
            return;
        };
        let ModelFixture {
            cleanup,
            widgets,
            layers,
            artwork,
            ..
        } = f;
        let preview = widgets.preview.clone();
        let (events, receiver) = mpsc::channel();
        let (layer_sender, _commands) = mpsc::channel();
        let polling = start_event_updates(
            widgets,
            receiver,
            layers,
            artwork,
            layer_sender,
            events.clone(),
        );
        let send = |event: protocol::Event| events.send(MonitorEvent::Input(event)).unwrap();
        let key = |number: u8| preview.control(ControlRef::Key(number)).unwrap();
        let dial = |number: u8| preview.control(ControlRef::Dial(number)).unwrap();
        let zone = |number: u8| preview.control(ControlRef::Zone(number)).unwrap();

        send(protocol::Event::Key {
            index: 29,
            pressed: true,
        });
        pump_until(|| key(30).has_css_class("control-pressed"));
        assert!(!key(1).has_css_class("control-pressed"));
        assert_eq!(preview.key_label(29).unwrap().text(), "KEY 30 · PRESSED");
        send(protocol::Event::Key {
            index: 29,
            pressed: false,
        });
        pump_until(|| !key(30).has_css_class("control-pressed"));

        send(protocol::Event::EncoderButton {
            index: 4,
            pressed: true,
        });
        pump_until(|| dial(5).has_css_class("control-pressed"));
        send(protocol::Event::EncoderButton {
            index: 4,
            pressed: false,
        });
        send(protocol::Event::Rotation { index: 4, ticks: 3 });
        pump_until(|| dial(5).has_css_class("control-rotated"));
        assert_eq!(
            preview.dial_label(4).unwrap().text(),
            "DIAL 5\n3 ticks · +3"
        );
        // Totals are kept per dial, sized for the widest model.
        send(protocol::Event::Rotation { index: 5, ticks: 2 });
        send(protocol::Event::Rotation { index: 5, ticks: 2 });
        pump_until(|| {
            preview
                .dial_label(5)
                .is_some_and(|label| label.text() == "DIAL 6\n4 ticks · +2")
        });
        assert!(!dial(1).has_css_class("control-rotated"));

        // 1100 px falls in the sixth 200 px section.
        send(protocol::Event::TouchTap { x: 1100, y: 10 });
        pump_until(|| zone(6).has_css_class("control-touched"));
        assert!((1..6).all(|number| !zone(number).has_css_class("control-touched")));
        assert!(preview.touch_status().text().starts_with("TAP"));
        pump_until(|| !zone(6).has_css_class("control-touched"));
        // A flick across the strip lights every section it crosses, clamped to the model's six.
        send(protocol::Event::TouchFlick {
            start_x: 100,
            start_y: 10,
            end_x: 65535,
            end_y: 10,
        });
        pump_until(|| (1..=6).all(|number| zone(number).has_css_class("control-touched")));
        // Controls a + has but an XL+ report cannot name are ignored without a panic.
        send(protocol::Event::Key {
            index: 36,
            pressed: true,
        });
        send(protocol::Event::Rotation { index: 6, ticks: 1 });
        send(protocol::Event::Key {
            index: 0,
            pressed: true,
        });
        pump_until(|| key(1).has_css_class("control-pressed"));
        polling.remove();
        drop(cleanup);
    });
}

#[test]
fn the_artwork_target_picker_and_clicks_follow_the_models_sections() {
    run_gtk_test(|| {
        let Some(h) = model_session_tests::start("preview-targets", |_| {}) else {
            return;
        };
        let preview = h.session.runtime().preview().unwrap();
        let artwork = h.session.runtime().artwork_ui().clone();
        let root: gtk::Widget = h.window.clone().upcast();
        // The expander keeps its content outside the widget chain while collapsed.
        let section = descendants_of::<gtk::Expander>(&root)
            .into_iter()
            .find(|expander| expander.label().as_deref() == Some("Artwork and touch zones"))
            .and_then(|expander| expander.child())
            .expect("the artwork section");
        let picker = descendants_of::<gtk::DropDown>(&section)
            .into_iter()
            .find(|picker| picker.has_css_class("artwork-target"))
            .expect("the artwork target picker");
        let names = |picker: &gtk::DropDown| -> Vec<String> {
            let model = picker.model().unwrap();
            (0..model.n_items())
                .map(|index| {
                    model
                        .item(index)
                        .unwrap()
                        .downcast::<gtk::StringObject>()
                        .unwrap()
                        .string()
                        .to_string()
                })
                .collect()
        };
        assert_eq!(names(&picker).len(), 5);
        h.preview_as(&PLUS_XL);
        let listed = names(&picker);
        assert_eq!(listed.len(), 7, "the strip and six sections");
        assert_eq!(listed[6], "Touch zone 6");
        // Clicking section 6 points the picker and the artwork target at it.
        preview
            .control(ControlRef::Zone(6))
            .unwrap()
            .downcast::<Button>()
            .unwrap()
            .emit_clicked();
        assert_eq!(artwork.selected_target.get(), ArtworkTarget::EncoderZone(6));
        assert_eq!(picker.selected(), 6);
        // The XL+ key 30 is an artwork target too.
        preview
            .control(ControlRef::Key(30))
            .unwrap()
            .downcast::<Button>()
            .unwrap()
            .emit_clicked();
        assert_eq!(artwork.selected_target.get(), ArtworkTarget::Keypad(30));
        // Going back to the + drops a target it does not have.
        artwork.selected_target.set(ArtworkTarget::EncoderZone(6));
        h.preview_as(&PLUS);
        assert_eq!(names(&picker).len(), 5);
        assert!(artwork.selected_target.get().is_valid_for(&PLUS));
    });
}
