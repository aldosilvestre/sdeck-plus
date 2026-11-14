//! The profile dialogs the rail opens (a row's pencil, the "+" beside PROFILES).
use super::profile_row_tests::edit_button;
use super::rail_tests::new_profile_button;
use super::ui_refresh_tests::fixture;
use super::*;

/// The visible dialog of a fixture, found by title through the public window list.
pub(super) fn dialog_titled(window: &ApplicationWindow, title: &str) -> gtk::Window {
    gtk::Window::toplevels()
        .snapshot()
        .into_iter()
        .filter_map(|candidate| candidate.downcast::<gtk::Window>().ok())
        .find(|candidate| {
            candidate.is_visible()
                && candidate.title().as_deref() == Some(title)
                && candidate.transient_for().as_ref() == Some(window.upcast_ref())
        })
        .unwrap_or_else(|| panic!("{title} dialog is open"))
}

fn buttons_of(widget: &gtk::Widget, found: &mut Vec<Button>) {
    if let Some(button) = widget.downcast_ref::<Button>() {
        found.push(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        buttons_of(&current, found);
        child = current.next_sibling();
    }
}

#[test]
fn profile_dialog_capture() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("profile-dialog-capture", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") else {
            return;
        };
        let directory = std::path::Path::new(&directory);
        for (caption, file) in [
            ("Edit profile", "organize-edit.png"),
            ("New profile", "organize-new.png"),
        ] {
            if caption == "New profile" {
                new_profile_button(widgets).emit_clicked();
            } else {
                edit_button(widgets, "Base").emit_clicked();
            }
            let dialog = dialog_titled(&fixture.window, caption);
            wait_for_frame(&dialog);
            rendered_texture(&dialog, &dialog.child().unwrap())
                .save_to_png(directory.join(file))
                .unwrap();
            dialog.close();
        }
    });
}

#[test]
fn profile_dialogs_have_a_titled_header_and_a_clear_button_hierarchy() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("organize-style", |_| {}) else {
            return;
        };
        edit_button(&fixture.widgets, "Base").emit_clicked();
        let dialog = dialog_titled(&fixture.window, "Edit profile");
        assert!(dialog.has_css_class("profile-dialog"));
        let content = dialog.child().unwrap();
        // Title, then a one-line subtitle explaining the dialog.
        let title = content.first_child().unwrap().downcast::<Label>().unwrap();
        assert_eq!(title.text(), "Edit profile");
        let subtitle = title.next_sibling().unwrap().downcast::<Label>().unwrap();
        assert!(
            subtitle.has_css_class("organize-subtitle"),
            "{}",
            subtitle.text()
        );
        // Save is the primary action; Delete is destructive; both share the row evenly.
        let mut buttons = Vec::new();
        buttons_of(dialog.upcast_ref(), &mut buttons);
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
        let actions = save.parent().unwrap().downcast::<GtkBox>().unwrap();
        assert!(actions.is_homogeneous(), "equal-width footer buttons");
        assert_eq!(delete.parent().as_ref(), Some(actions.upcast_ref()));
        // The Base note stands out as a callout.
        let note = actions.prev_sibling().unwrap().downcast::<Label>().unwrap();
        assert!(note.has_css_class("profile-note"));
        dialog.close();
    });
}
