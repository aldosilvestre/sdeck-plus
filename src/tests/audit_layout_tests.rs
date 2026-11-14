//! Layout findings of the design audit, checked in a real render: the editor form keeps its
//! dropdowns clear of the overlay scrollbar, and the running window keeps the 16px shell margin.
use super::preview_model_tests::model_fixture;
use super::*;
use sdeck_plus::device_model::PLUS;

fn descendants(root: &gtk::Widget, found: &mut Vec<gtk::Widget>) {
    let mut child = root.first_child();
    while let Some(widget) = child {
        found.push(widget.clone());
        descendants(&widget, found);
        child = widget.next_sibling();
    }
}

/// The documentation screenshot `touch-strip.png`: the touch strip selected, Swipe left bound to
/// More actions > Organization > Next page, with the form scrolled to its top.
#[test]
fn touch_strip_dropdowns_stay_clear_of_the_form_scrollbar() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("audit-strip", &PLUS, |_, _| {}) else {
            return;
        };
        f.widgets
            .touch_status
            .parent()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Overlay>()
            .unwrap()
            .child()
            .unwrap()
            .downcast::<Button>()
            .unwrap()
            .emit_clicked();
        f.widgets.binding_ui.event_picker.set_selected(3);
        f.widgets.binding_ui.action_picker.set_selected(5);
        wait_for_frame(&f.window);
        let scroll = descendant_with_class(f.widgets.root.upcast_ref(), "settings-scroll")
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        scroll.vadjustment().set_value(0.0);
        wait_for_frame(&f.window);
        let mut all = Vec::new();
        descendants(scroll.upcast_ref(), &mut all);
        let bar = all
            .iter()
            .find(|w| {
                w.downcast_ref::<gtk::Scrollbar>()
                    .is_some_and(|s| s.orientation() == gtk::Orientation::Vertical)
            })
            .cloned()
            .expect("the form's vertical scrollbar");
        // The overlay scrollbar is hidden until the form overflows or the pointer moves, so the
        // dropdowns must stay clear of the strip it would occupy, whether it shows or not.
        let (_, bar_width, _, _) = bar.measure(gtk::Orientation::Horizontal, -1);
        assert!(bar_width > 0, "the scrollbar has a natural width");
        let bar_left = (scroll.width() - bar_width) as f32;
        let dropdowns: Vec<_> = all
            .iter()
            .filter(|w| w.is::<gtk::DropDown>() && w.is_mapped())
            .collect();
        assert!(dropdowns.len() >= 3, "event, category and action dropdowns");
        for dropdown in dropdowns {
            let bounds = dropdown.compute_bounds(&scroll).unwrap();
            let right = bounds.x() + bounds.width();
            eprintln!("dropdown right {right} scrollbar left {bar_left}");
            assert!(
                right <= bar_left,
                "a dropdown reaches the scrollbar: right edge {right}, scrollbar left {bar_left}"
            );
        }
    });
}

/// The shell margin is on the running root content, so window-content screenshots crop it.
#[test]
fn the_running_window_keeps_the_16px_shell_margin() {
    run_gtk_test(|| {
        let Some(f) = model_fixture("audit-shell", &PLUS, |_, _| {}) else {
            return;
        };
        wait_for_frame(&f.window);
        let root: gtk::Widget = f.widgets.root.clone().upcast();
        let shell = descendant_with_class(&root, "app-shell").expect("the app shell");
        let bounds = shell.compute_bounds(&root).unwrap();
        eprintln!("shell bounds in root: {bounds:?}");
        assert!(
            bounds.x() >= 16.0 && bounds.y() >= 16.0,
            "the shell content starts 16px inside the root: {bounds:?}"
        );
        let first = shell.first_child().unwrap();
        let inner = first.compute_bounds(&root).unwrap();
        assert!(
            inner.x() >= 16.0 && inner.y() >= 16.0,
            "the first content starts 16px inside the window content: {inner:?}"
        );
    });
}
