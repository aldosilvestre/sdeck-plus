//! The preview geometry of every registry model: the Stream Deck + reproduces
//! the shipped numbers exactly, and every other model is laid out sanely.
use sdeck_plus::{
    device_model::{DeviceModel, CLASSIC, PLUS, PLUS_XL, VIRTUAL_CLASSIC},
    preview_layout::{ControlRef, PreviewLayout, Rect, MIN_HIT_PX},
};

fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(x, y, w, h)
}

/// Captured from the widgets of the Stream Deck + preview before the layout
/// engine existed (chassis coordinates).
#[test]
fn plus_layout_reproduces_the_shipped_geometry() {
    let layout = PreviewLayout::for_model(&PLUS);
    assert_eq!(layout.size, (426, 414));
    assert_eq!(layout.brand, "STREAM DECK +");
    assert_eq!(layout.brand_rect, r(0, 6, 426, 24));
    let keys: Vec<Rect> = [38, 105]
        .into_iter()
        .flat_map(|y| [48, 139, 230, 321].map(|x| r(x, y, 60, 60)))
        .collect();
    assert_eq!(layout.keys, keys);
    let strip = layout.strip.as_ref().unwrap();
    assert_eq!(strip.bezel, r(27, 177, 374, 77));
    assert_eq!(strip.header, r(50, 180, 328, 16));
    assert_eq!(strip.rect, r(50, 202, 328, 41));
    assert!(strip.touch);
    assert_eq!(
        strip.zones,
        [50, 132, 214, 296].map(|x| r(x, 202, 82, 41)).to_vec()
    );
    let rail = layout.dial_rail.as_ref().unwrap();
    assert_eq!(rail.rail, r(27, 273, 374, 108));
    assert_eq!(
        rail.dials,
        [55, 146, 237, 328].map(|x| r(x, 307, 46, 46)).to_vec()
    );
    // The icon is centred on the knob, the caption sits under it.
    assert_eq!(rail.icons[0], r(68, 320, 20, 20));
    assert_eq!(rail.legends[0], r(35, 357, 86, 16));
    assert_eq!(rail.legends[3], r(308, 357, 86, 16));
}

/// L33 + L50: the virtual deck's preview reads "VIRTUAL DECK", never
/// "Classic"; the real Classic keeps its name.
#[test]
fn the_virtual_deck_preview_is_titled_virtual_deck_and_the_classic_keeps_its_name() {
    let brand = PreviewLayout::for_model(&VIRTUAL_CLASSIC).brand;
    assert_eq!(brand, "VIRTUAL DECK");
    assert!(!brand.to_lowercase().contains("classic"), "{brand}");
    assert_eq!(
        PreviewLayout::for_model(&CLASSIC).brand,
        "STREAM DECK CLASSIC"
    );
}

#[test]
fn xl_layout_has_nine_columns_four_rows_and_six_aligned_sections() {
    let layout = PreviewLayout::for_model(&PLUS_XL);
    assert_eq!(layout.brand, "STREAM DECK + XL");
    assert_eq!(layout.size, (556, 456));
    assert_eq!(layout.key_count(), 36);
    for (index, key) in layout.keys.iter().enumerate() {
        let (col, row) = ((index % 9) as i32, (index / 9) as i32);
        assert_eq!(
            *key,
            r(74 + col * 46, 38 + row * 46, 40, 40),
            "key {}",
            index + 1
        );
    }
    let strip = layout.strip.as_ref().unwrap();
    assert_eq!(strip.zones.len(), 6);
    assert_eq!(strip.rect.w, 6 * 76);
    let rail = layout.dial_rail.as_ref().unwrap();
    assert_eq!(rail.dials.len(), 6);
    for (zone, dial) in strip.zones.iter().zip(&rail.dials) {
        // Each dial is centred under the section it belongs to.
        assert_eq!(zone.x + zone.w / 2, dial.x + dial.w / 2);
    }
    // Zones are 200 of the strip's 1200 device pixels each: equal widths.
    assert!(strip.zones.iter().all(|zone| zone.w == strip.zones[0].w));
    assert_eq!(strip.zones[5].right(), strip.rect.right());
}

#[test]
fn controls_follow_the_widget_order_keys_strip_sections_dials() {
    let controls = PreviewLayout::for_model(&PLUS).controls();
    assert_eq!(controls.len(), 17);
    assert_eq!(controls[0], ControlRef::Key(1));
    assert_eq!(controls[7], ControlRef::Key(8));
    assert_eq!(controls[8], ControlRef::Strip);
    assert_eq!(controls[9], ControlRef::Zone(1));
    assert_eq!(controls[12], ControlRef::Zone(4));
    assert_eq!(controls[13], ControlRef::Dial(1));
    assert_eq!(controls[16], ControlRef::Dial(4));
    let xl = PreviewLayout::for_model(&PLUS_XL).controls();
    assert_eq!(xl.len(), 36 + 1 + 6 + 6);
    assert_eq!(xl[35], ControlRef::Key(36));
}

#[test]
fn accessible_names_follow_the_device_numbering() {
    assert_eq!(ControlRef::Key(36).accessible_name().unwrap(), "Button 36");
    assert_eq!(ControlRef::Dial(6).accessible_name().unwrap(), "Encoder 6");
    assert_eq!(
        ControlRef::Zone(6).accessible_name().unwrap(),
        "Touch zone 6"
    );
    assert_eq!(ControlRef::Strip.accessible_name(), None);
}

#[test]
fn controls_a_model_lacks_are_absent() {
    let plus = PreviewLayout::for_model(&PLUS);
    assert!(plus.has(ControlRef::Key(8)) && !plus.has(ControlRef::Key(9)));
    assert!(!plus.has(ControlRef::Key(0)));
    assert!(plus.has(ControlRef::Dial(4)) && !plus.has(ControlRef::Dial(5)));
    assert!(plus.has(ControlRef::Zone(4)) && !plus.has(ControlRef::Zone(5)));
    assert!(plus.has(ControlRef::Strip));
}

fn every_rect(layout: &PreviewLayout) -> Vec<(String, Rect)> {
    let mut rects = vec![("brand".to_owned(), layout.brand_rect)];
    rects.extend(
        layout
            .keys
            .iter()
            .enumerate()
            .map(|(index, key)| (format!("key {}", index + 1), *key)),
    );
    if let Some(strip) = &layout.strip {
        rects.push(("bezel".into(), strip.bezel));
        rects.push(("header".into(), strip.header));
        rects.push(("strip".into(), strip.rect));
    }
    if let Some(rail) = &layout.dial_rail {
        rects.push(("rail".into(), rail.rail));
    }
    if let Some(background) = layout.background {
        rects.push(("background".into(), background));
    }
    rects
}

#[test]
fn every_model_has_a_sane_deterministic_layout() {
    for model in DeviceModel::all() {
        let layout = PreviewLayout::for_model(model);
        assert_eq!(
            layout,
            PreviewLayout::for_model(model),
            "{} is deterministic",
            model.id
        );
        let chassis = layout.chassis();
        assert!(layout.size.0 > 0 && layout.size.1 > 0, "{}", model.id);

        // Counts follow the model's own accessors.
        assert_eq!(
            layout.key_count(),
            usize::from(model.keys()),
            "{}",
            model.id
        );
        assert_eq!(
            layout.dial_count(),
            usize::from(model.dials),
            "{}",
            model.id
        );
        assert_eq!(
            layout.zone_count(),
            usize::from(model.zones()),
            "{}",
            model.id
        );
        assert_eq!(layout.strip.is_some(), model.has_strip(), "{}", model.id);
        assert_eq!(layout.dial_rail.is_some(), model.dials > 0, "{}", model.id);
        assert_eq!(layout.brand, model.name.to_uppercase());

        // Everything lies on the chassis, and the blocks never overlap.
        let blocks = every_rect(&layout);
        for (name, rect) in &blocks {
            assert!(
                chassis.contains(rect),
                "{}: {name} {rect:?} leaves the chassis",
                model.id
            );
        }
        // The header and the strip live inside the bezel (a window is its own
        // frame); the rest is disjoint.
        let loose: Vec<_> = blocks
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "header" | "strip"))
            .collect();
        for (index, (first, a)) in loose.iter().enumerate() {
            for (second, b) in &loose[index + 1..] {
                assert!(!a.overlaps(b), "{}: {first} overlaps {second}", model.id);
            }
        }
        if let Some(strip) = &layout.strip {
            assert!(strip.bezel.contains(&strip.header) && strip.bezel.contains(&strip.rect));
            if strip.touch {
                assert!(!strip.header.overlaps(&strip.rect), "{}", model.id);
            } else {
                // An empty window shows its caption inside itself.
                assert_eq!(strip.bezel, strip.rect, "{}", model.id);
                assert!(strip.rect.contains(&strip.header), "{}", model.id);
            }
            for (index, zone) in strip.zones.iter().enumerate() {
                assert!(
                    strip.rect.contains(zone),
                    "{}: zone {}",
                    model.id,
                    index + 1
                );
                if let Some(next) = strip.zones.get(index + 1) {
                    assert!(!zone.overlaps(next), "{}: zones overlap", model.id);
                }
            }
        }
        if let Some(rail) = &layout.dial_rail {
            for (index, dial) in rail.dials.iter().enumerate() {
                assert!(rail.rail.contains(dial), "{}: dial {}", model.id, index + 1);
                assert!(
                    dial.contains(&rail.icons[index]),
                    "{}: icon {}",
                    model.id,
                    index + 1
                );
                assert!(rail.rail.contains(&rail.legends[index]));
                assert!(
                    !dial.overlaps(&rail.legends[index]),
                    "{}: caption",
                    model.id
                );
                if let Some(next) = rail.dials.get(index + 1) {
                    assert!(!dial.overlaps(next), "{}: dials overlap", model.id);
                    assert!(!rail.legends[index].overlaps(&rail.legends[index + 1]));
                }
            }
        }

        // Every interactive control stays a usable target at the preview scale.
        for control in layout.controls() {
            let rect = layout.rect_of(control).expect("listed controls exist");
            let scale = layout.scale;
            assert!(
                f64::from(rect.w) * scale >= f64::from(MIN_HIT_PX)
                    && f64::from(rect.h) * scale >= f64::from(MIN_HIT_PX),
                "{}: {control:?} {rect:?} is too small",
                model.id
            );
        }
        assert_eq!(
            layout.controls().len(),
            usize::from(model.keys())
                + usize::from(model.dials)
                + usize::from(model.zones())
                + usize::from(model.has_touch())
                + usize::from(model.has_info_window())
                + usize::from(model.touch_buttons)
                + usize::from(model.has_keypad_background())
        );
        assert_eq!(
            layout.background.is_some(),
            model.has_keypad_background(),
            "{}",
            model.id
        );
        // Touch buttons sit on the chassis, beside the display but never on
        // its frame, and cover no other control or block.
        assert_eq!(
            layout.touch_button_count(),
            usize::from(model.touch_buttons),
            "{}",
            model.id
        );
        for (index, button) in layout.touch_buttons.iter().enumerate() {
            assert!(chassis.contains(button), "{}: touch button", model.id);
            for (name, rect) in &blocks {
                assert!(
                    !button.overlaps(rect),
                    "{}: touch button {} overlaps {name}",
                    model.id,
                    index + 1
                );
            }
            for other in &layout.touch_buttons[index + 1..] {
                assert!(!button.overlaps(other), "{}", model.id);
            }
        }
    }
}

#[test]
fn layouts_are_horizontally_symmetric() {
    for model in DeviceModel::all() {
        let layout = PreviewLayout::for_model(model);
        let width = layout.size.0;
        // The Stream Deck + inherits 3 px of asymmetry from its reference image.
        let tolerance = if model.id == PLUS.id { 3 } else { 0 };
        let keys_left = layout.keys.iter().map(|key| key.x).min();
        let keys_right = layout.keys.iter().map(|key| key.right()).max();
        if let (Some(left), Some(right)) = (keys_left, keys_right) {
            assert!(
                (left - (width - right)).abs() <= tolerance,
                "{} keys",
                model.id
            );
        }
        if let Some(strip) = &layout.strip {
            assert!(
                (strip.bezel.x - (width - strip.bezel.right())).abs() <= tolerance.max(2),
                "{} bezel",
                model.id
            );
        }
        if let Some(rail) = &layout.dial_rail {
            let first = rail.dials.first().unwrap();
            let last = rail.dials.last().unwrap();
            let left = first.x - rail.rail.x;
            let right = rail.rail.right() - last.right();
            assert!(
                (left - right).abs() <= tolerance.max(5),
                "{} dials",
                model.id
            );
        }
    }
}

#[test]
fn neo_window_is_a_selectable_display_between_its_keys_and_touch_buttons() {
    let neo = DeviceModel::by_id("neo").expect("registry lists the Neo");
    let layout = PreviewLayout::for_model(neo);
    let strip = layout.strip.as_ref().expect("the window is drawn");
    assert!(!strip.touch && strip.zones.is_empty());
    // The window is no touch strip: it is selected for its picture and
    // information, never for touch gestures.
    assert!(!layout.has(ControlRef::Strip));
    assert_eq!(layout.rect_of(ControlRef::Window), Some(strip.rect));
    // Drawn at half its 248x58 pixels, so pictures keep their proportions,
    // and it is its own frame: no bezel or header line around it. Its pads sit
    // 10 px either side, vertically centred (the odd half pixel downwards, so
    // Tab reaches the window first), and the row spans the keys.
    let window = strip.rect;
    assert_eq!(window.w * 58, window.h * 248, "248x58-proportioned");
    assert_eq!(window, r(86, 146, 124, 29));
    assert_eq!(strip.bezel, window);
    assert_eq!(strip.header, r(86, 152, 124, 16));
    assert_eq!(
        layout.touch_buttons,
        vec![r(48, 147, 28, 28), r(220, 147, 28, 28)]
    );
    for pad in &layout.touch_buttons {
        assert!(!pad.overlaps(&window), "{pad:?}");
    }
    assert_eq!(layout.touch_buttons[0].x, layout.keys[0].x);
    assert_eq!(layout.touch_buttons[1].right(), layout.keys[3].right());
    assert_eq!(layout.size, (296, 208));
    assert_eq!(
        ControlRef::Window.accessible_name().as_deref(),
        Some("Info window")
    );
    let mut expected: Vec<_> = (1..=8).map(ControlRef::Key).collect();
    expected.extend([
        ControlRef::Window,
        ControlRef::TouchButton(1),
        ControlRef::TouchButton(2),
    ]);
    assert_eq!(layout.controls(), expected);
    assert_eq!(layout.touch_button_count(), 2);
    // Models with a touch strip, or no display, have no window control.
    for model in [&PLUS, &PLUS_XL]
        .into_iter()
        .chain(["classic", "xl"].map(|id| DeviceModel::by_id(id).unwrap()))
    {
        let layout = PreviewLayout::for_model(model);
        assert!(!layout.has(ControlRef::Window), "{}", model.id);
        assert!(!layout.controls().contains(&ControlRef::Window));
    }
}

/// The Stream Deck Classic's keypad background (S6) is selected from a slim
/// "Background" bar under its keys, after them in the order of the controls.
/// No other model has one, and the XL keeps its size.
#[test]
fn the_classic_offers_its_background_as_a_control_under_its_keys() {
    let classic = DeviceModel::by_id("classic").unwrap();
    let layout = PreviewLayout::for_model(classic);
    let bar = layout
        .background
        .expect("the Classic shows its background control");
    // As wide as the keys and 12 px under the last row, like a display.
    assert_eq!(layout.keys[10], r(48, 142, 44, 44));
    assert_eq!(bar, r(48, 198, 252, 28));
    assert_eq!(layout.size, (348, 250));
    assert_eq!(layout.rect_of(ControlRef::Background), Some(bar));
    assert_eq!(
        ControlRef::Background.accessible_name().as_deref(),
        Some("Background")
    );
    let mut expected: Vec<_> = (1..=15).map(ControlRef::Key).collect();
    expected.push(ControlRef::Background);
    assert_eq!(layout.controls(), expected);
    for model in DeviceModel::all()
        .iter()
        .filter(|model| model.id != "classic")
    {
        let layout = PreviewLayout::for_model(model);
        assert_eq!(layout.background, None, "{}", model.id);
        assert!(!layout.has(ControlRef::Background), "{}", model.id);
        assert!(!layout.controls().contains(&ControlRef::Background));
    }
    let xl = PreviewLayout::for_model(DeviceModel::by_id("xl").unwrap());
    assert_eq!(xl.size, (504, 262));
}

#[test]
fn models_without_a_strip_or_dials_omit_them() {
    for id in ["classic", "xl", "mini"] {
        let layout = PreviewLayout::for_model(DeviceModel::by_id(id).unwrap());
        assert!(layout.strip.is_none() && layout.dial_rail.is_none(), "{id}");
    }
    let classic = PreviewLayout::for_model(DeviceModel::by_id("classic").unwrap());
    assert_eq!(classic.key_count(), 15);
    let mini = PreviewLayout::for_model(DeviceModel::by_id("mini").unwrap());
    assert_eq!(mini.key_count(), 6);
}
