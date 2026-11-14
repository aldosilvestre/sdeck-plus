//! The action catalog stays beside the hardware preview at every window width,
//! so an action can always be dragged from the catalog onto a key or dial.
//! Below the side-by-side minimum the content column scrolls horizontally.
use super::preview_model_tests::{class_of, model_fixture, preview_of};
use super::*;
use sdeck_plus::device_model::{DeviceModel, PLUS, PLUS_XL};

fn assert_beside(model: &'static DeviceModel, width: i32) -> f64 {
    let Some(f) = model_fixture("beside", model, |_, _| {}) else {
        return 0.0;
    };
    let root: gtk::Widget = f.widgets.root.clone().upcast();
    // The fixture pins the default size; release it so narrower sizes can apply.
    root.set_size_request(-1, -1);
    root.allocate(width, 700, -1, None);
    let content = f.widgets.scroller.child().unwrap();
    let bin = preview_of(&root);
    let preview = bin.compute_bounds(&content).unwrap();
    let card = class_of(&root, "device-canvas")
        .compute_bounds(&content)
        .unwrap();
    let side = class_of(&root, "settings-sidebar")
        .compute_bounds(&content)
        .unwrap();
    assert!(
        side.x() >= card.x() + card.width(),
        "{} at {width}: the catalog sits right of the preview card: {side:?} vs {card:?}",
        model.id
    );
    assert!(
        (side.y() - preview.y()).abs() <= 1.0,
        "{} at {width}: the catalog shares the preview's top row: {side:?} vs {preview:?}",
        model.id
    );
    let hadjustment = f.widgets.scroller.hadjustment();
    if width >= DEFAULT_WIDTH {
        assert!(
            hadjustment.upper() <= hadjustment.page_size(),
            "{} at {width}: the default window needs no horizontal scrolling",
            model.id
        );
    }
    // The catalog never drops below its 250px minimum.
    assert!(side.width() >= 250.0, "{} at {width}: {side:?}", model.id);
    content.measure(Orientation::Horizontal, -1).0 as f64
}

#[test]
fn the_catalog_sits_beside_the_preview_at_every_width_for_both_models() {
    run_gtk_test(|| {
        for model in [&PLUS, &PLUS_XL] {
            for width in [1120, 1000, 900, 800, 640] {
                let minimum = assert_beside(model, width);
                eprintln!("{} at {width}: content minimum width {minimum}", model.id);
            }
        }
    });
}

/// Opt-in window captures for visual review; unset = no files written.
#[test]
fn catalog_beside_preview_captures_when_explicitly_requested() {
    run_gtk_test(|| {
        let Some(directory) = std::env::var_os("STREAMDECK_LAYOUT_QA_DIR") else {
            return;
        };
        app_icon::register().unwrap();
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        for (model, width, dark) in [
            (&PLUS, 1120, true),
            (&PLUS, 900, true),
            (&PLUS, 800, true),
            (&PLUS_XL, 1000, true),
            (&PLUS, 1120, false),
        ] {
            let _look = super::preview_model_tests::ExplicitLook::apply_with(dark);
            let Some(f) = model_fixture("beside-capture", model, |_, _| {}) else {
                return;
            };
            f.widgets.root.set_size_request(width, 700);
            f.widgets.root.allocate(width, 700, -1, None);
            wait_for_frame(&f.window);
            let texture = rendered_texture(&f.window, &f.widgets.root);
            let theme = if dark { "dark" } else { "light" };
            texture
                .save_to_png(directory.join(format!("{}-{width}x700-{theme}.png", model.id)))
                .unwrap();
        }
    });
}

/// Down to preview card + 16 + the 250px minimum (plus the content margins and the rail),
/// the catalog shrinks to its minimum instead of overflowing: no horizontal scrolling.
#[test]
fn the_catalog_shrinks_to_its_250px_minimum_before_the_column_scrolls() {
    run_gtk_test(|| {
        for model in [&PLUS, &PLUS_XL] {
            let Some(f) = model_fixture("shrink", model, |_, _| {}) else {
                return;
            };
            let root: gtk::Widget = f.widgets.root.clone().upcast();
            root.set_size_request(-1, -1);
            root.allocate(DEFAULT_WIDTH, 700, -1, None);
            let rail = DEFAULT_WIDTH - f.widgets.scroller.width();
            let card = class_of(&root, "device-canvas").width();
            // 16px content margins on both sides, 16px between the columns.
            let width = rail + 16 + card + 16 + 250 + 16;
            root.allocate(width, 700, -1, None);
            let content = f.widgets.scroller.child().unwrap();
            let side = class_of(&root, "settings-sidebar")
                .compute_bounds(&content)
                .unwrap();
            let hadjustment = f.widgets.scroller.hadjustment();
            assert!(
                hadjustment.upper() <= hadjustment.page_size(),
                "{}: no horizontal scrolling at window {width} (content minimum {})",
                model.id,
                content.measure(Orientation::Horizontal, -1).0
            );
            assert!(
                (250.0..300.0).contains(&side.width()),
                "{}: the catalog is at its minimum, not its natural 352: {side:?}",
                model.id
            );
        }
    });
}

/// The catalog's horizontal minimum never depends on the height it is allocated: the content
/// grid sizes its columns from it, so a height-dependent minimum made the catalog overhang its
/// slot in one of two stable layouts (645px high wanted 505px, 657px high fit in 250px).
#[test]
fn the_catalog_minimum_width_does_not_depend_on_its_height() {
    run_gtk_test(|| {
        for model in [&PLUS, &PLUS_XL] {
            let Some(f) = model_fixture("height-free", model, |_, _| {}) else {
                return;
            };
            let root: gtk::Widget = f.widgets.root.clone().upcast();
            root.set_size_request(-1, -1);
            root.allocate(DEFAULT_WIDTH, 700, -1, None);
            let side = class_of(&root, "settings-sidebar");
            let minimum = |height: i32| side.measure(Orientation::Horizontal, height).0;
            let free = minimum(-1);
            for height in [645, 657, 700, 900] {
                assert_eq!(
                    minimum(height),
                    free,
                    "{}: minimum width at height {height}",
                    model.id
                );
            }
            assert!(free <= 250, "{}: the catalog minimum is {free}", model.id);
        }
    });
}
