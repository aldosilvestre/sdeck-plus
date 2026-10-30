//! The Organize popover: profile editing and creation panels.
use super::ui_refresh_tests::fixture;
use super::*;

#[test]
fn organize_popover_capture() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("organize-popover", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        let panel = widgets.management_menu.popover().unwrap();
        panel.set_autohide(false);
        let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") else {
            return;
        };
        let stack = {
            let mut found = Vec::new();
            collect_stacks(panel.upcast_ref(), &mut found);
            found.remove(0)
        };
        for name in ["edit", "new"] {
            stack.set_visible_child_name(name);
            let texture = rendered_text_popup(&panel, || widgets.management_menu.popup());
            texture
                .save_to_png(std::path::Path::new(&directory).join(format!("organize-{name}.png")))
                .unwrap();
            if name == "edit" {
                // Hover the help icon and capture the help it reveals.
                let help = {
                    let mut found = Vec::new();
                    collect_help(panel.upcast_ref(), &mut found);
                    found.remove(0)
                };
                let tip = help.popover().unwrap();
                let motion = {
                    let controllers = help.observe_controllers();
                    (0..controllers.n_items())
                        .find_map(|i| {
                            controllers
                                .item(i)?
                                .downcast::<gtk::EventControllerMotion>()
                                .ok()
                        })
                        .unwrap()
                };
                let texture = rendered_text_popup(&tip, || {
                    motion.emit_by_name::<()>("enter", &[&2.0f64, &2.0f64])
                });
                texture
                    .save_to_png(std::path::Path::new(&directory).join("organize-help.png"))
                    .unwrap();
                motion.emit_by_name::<()>("leave", &[]);
            }
            panel.popdown();
        }
    });
}

fn collect_help(widget: &gtk::Widget, found: &mut Vec<gtk::MenuButton>) {
    if let Some(button) = widget.downcast_ref::<gtk::MenuButton>() {
        if button.has_css_class("field-help") {
            found.push(button.clone());
        }
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect_help(&current, found);
        child = current.next_sibling();
    }
}

fn collect_stacks(widget: &gtk::Widget, found: &mut Vec<gtk::Stack>) {
    if let Some(stack) = widget.downcast_ref::<gtk::Stack>() {
        found.push(stack.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect_stacks(&current, found);
        child = current.next_sibling();
    }
}

#[test]
fn organize_popover_has_a_titled_header_and_a_clear_button_hierarchy() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("organize-style", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        let panel = widgets.management_menu.popover().unwrap();
        assert!(panel.has_css_class("organize-popover"));
        let content = panel.child().unwrap();
        // Header: icon, title, and an icon-only Close.
        let header = content.first_child().unwrap();
        let icon = header
            .first_child()
            .unwrap()
            .downcast::<gtk::Image>()
            .unwrap();
        assert_eq!(
            icon.icon_name().as_deref(),
            Some("preferences-system-symbolic")
        );
        let title = icon.next_sibling().unwrap().downcast::<Label>().unwrap();
        assert_eq!(title.text(), "Profiles");
        let close = header.last_child().unwrap().downcast::<Button>().unwrap();
        assert!(close.child().is_some_and(|child| child.is::<gtk::Image>()));
        assert_eq!(close.tooltip_text().as_deref(), Some("Close"));
        // A subtitle explains the panel under the header.
        let subtitle = header.next_sibling().unwrap().downcast::<Label>().unwrap();
        assert!(
            subtitle.has_css_class("organize-subtitle"),
            "{}",
            subtitle.text()
        );
        // Save is the primary action; Delete is destructive; both share the row evenly.
        let mut buttons = Vec::new();
        collect_layout_buttons(panel.upcast_ref(), &mut buttons);
        let named = |name: &str| {
            buttons
                .iter()
                .find(|button| button_caption(button).as_deref() == Some(name))
                .unwrap_or_else(|| panic!("{name}"))
                .clone()
        };
        let save = named("Save changes");
        let delete = named("Delete profile");
        assert!(save.has_css_class("suggested-action"));
        assert!(delete.has_css_class("destructive-action"));
        let row = save.parent().unwrap().downcast::<GtkBox>().unwrap();
        assert!(row.is_homogeneous(), "equal-width footer buttons");
        assert_eq!(delete.parent().as_ref(), Some(row.upcast_ref()));
        // The Base note stands out as a callout.
        let note = row.prev_sibling().unwrap().downcast::<Label>().unwrap();
        assert!(note.has_css_class("profile-note"));
    });
}
