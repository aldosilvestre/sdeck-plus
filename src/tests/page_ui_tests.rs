use super::ui_refresh_tests::fixture;
use super::*;

/// The QA-gated page workflow over the production widgets. It runs only when
/// `STREAMDECK_QA_DIR` names a directory, and writes its captures there.
#[test]
fn page_workflow_captures_when_a_qa_directory_is_set() {
    if std::env::var_os("STREAMDECK_QA_DIR").is_none() {
        return;
    }
    run_gtk_test(|| {
        let Some(fixture) = fixture("page-workflow", |_| {}) else {
            return;
        };
        // Bundled action icons resolve through the app resource path, as at startup.
        crate::app_icon::register().unwrap();
        capture_page_workflow(
            &fixture.window,
            &fixture.widgets,
            (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        );
    });
}

/// Every button below `widget`, in tree order (popover rows included).
fn buttons_in(widget: &gtk::Widget) -> Vec<Button> {
    let mut found: Vec<Button> = widget
        .downcast_ref::<Button>()
        .cloned()
        .into_iter()
        .collect();
    let mut child = widget.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        found.extend(buttons_in(&current));
    }
    found
}

/// Every label's text below `widget`, joined.
fn all_text(widget: &gtk::Widget) -> String {
    let mut text = String::new();
    if let Some(label) = widget.downcast_ref::<Label>() {
        text.push_str(&label.text());
        text.push('\n');
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        text.push_str(&all_text(&current));
    }
    text
}

pub(super) fn capture_page_workflow(
    window: &ApplicationWindow,
    widgets: &MonitorWidgets,
    allocation: (i32, i32),
) {
    let Some(directory) = std::env::var_os("STREAMDECK_QA_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let (width, height) = allocation;
    let save = |name: &str, widget: &gtk::Widget| {
        let texture = rendered_texture(window, widget);
        texture
            .save_to_png(directory.join(format!("{width}x{height}-workflow-{name}.png")))
            .unwrap();
    };
    let mut buttons = Vec::new();
    collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
    let button = |caption: &str| {
        buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some(caption))
            .unwrap()
    };

    button("New page").emit_clicked();
    let dialog = current_dialog();
    save("new-page", dialog.upcast_ref());
    dialog.response(ResponseType::Accept);
    assert!(dialog.is_visible());
    save("invalid-page-name", dialog.upcast_ref());
    page_dialog_entry(&dialog).set_text("QA page");
    dialog.response(ResponseType::Accept);
    save("named-page", widgets.root.upcast_ref());

    button("Rename page").emit_clicked();
    let dialog = current_dialog();
    save("rename-page", dialog.upcast_ref());
    let entry = page_dialog_entry(&dialog);
    entry.set_text("");
    dialog.response(ResponseType::Accept);
    save("invalid-rename", dialog.upcast_ref());
    entry.set_text("QA page renamed");
    entry.emit_activate();
    assert!(!dialog.is_visible());
    save("renamed-page", widgets.root.upcast_ref());

    let menu = widgets.feature_runtime.folder_menu();
    let panel = menu.popover().unwrap();
    let autohide = panel.is_autohide();
    panel.set_autohide(false);
    let mut entries = Vec::new();
    collect_entries(panel.upcast_ref(), &mut entries);
    let name = entries
        .iter()
        .find(|entry| entry.placeholder_text().as_deref() == Some("New folder name"))
        .unwrap();
    name.set_text("QA folder");
    menu.popup();
    save("new-folder", panel.upcast_ref());
    // Creating keeps the owning page selected and the popover open.
    button("New folder").emit_clicked();
    save("folder-count", widgets.root.upcast_ref());
    save("owned-folders", panel.upcast_ref());
    folder_button(widgets, "Link folder QA folder to control").emit_clicked();
    save("linked-folder", panel.upcast_ref());
    menu.popdown();
    panel.set_autohide(autohide);
    folder_button(widgets, "Delete folder QA folder").emit_clicked();
    let dialog = current_dialog();
    save("delete-folder", dialog.upcast_ref());
    dialog.response(ResponseType::Accept);
    save("empty-folders", widgets.root.upcast_ref());
    button("Delete page").emit_clicked();
    let dialog = current_dialog();
    save("delete-page", dialog.upcast_ref());
    dialog.response(ResponseType::Accept);

    // New and Edit profile are dedicated modals: the rail's "+" and a row's pencil open them.
    let open_command = |command: &str| {
        match command {
            "New profile" => super::rail_tests::new_profile_button(widgets).emit_clicked(),
            _ => super::profile_row_tests::edit_button(widgets, "Base").emit_clicked(),
        }
        gtk::Window::toplevels()
            .snapshot()
            .into_iter()
            .filter_map(|candidate| candidate.downcast::<gtk::Window>().ok())
            .find(|candidate| {
                candidate.is_visible()
                    && candidate.title().as_deref() == Some(command)
                    && candidate.transient_for().as_ref() == Some(window.upcast_ref())
            })
            .unwrap_or_else(|| panic!("{command} dialog opens"))
    };
    let dialog_button = |dialog: &gtk::Window, caption: &str| {
        buttons_in(&dialog.child().unwrap())
            .into_iter()
            .find(|button| button_caption(button).as_deref() == Some(caption))
            .unwrap_or_else(|| panic!("{caption} button in the {} dialog", dialog.title().unwrap()))
    };
    let profile = format!("QA profile {width}");
    let new_profile = open_command("New profile");
    let mut entries = Vec::new();
    collect_entries(&new_profile.child().unwrap(), &mut entries);
    entries
        .iter()
        .find(|entry| entry.placeholder_text().as_deref() == Some("New profile name"))
        .expect("the New profile name field")
        .set_text(&profile);
    save("new-profile", new_profile.upcast_ref());
    dialog_button(&new_profile, "Create profile").emit_clicked();
    assert!(
        !new_profile.is_visible(),
        "creating closes the New profile dialog"
    );
    button("New page").emit_clicked();
    let dialog = current_dialog();
    page_dialog_entry(&dialog).set_text("Profile tools");
    dialog.response(ResponseType::Accept);
    name.set_text("QA child");
    button("New folder").emit_clicked();
    let edit_profile = open_command("Edit profile");
    save("edit-profile", edit_profile.upcast_ref());
    dialog_button(&edit_profile, "Delete profile").emit_clicked();
    let dialog = current_dialog();
    assert!(
        all_text(dialog.upcast_ref()).contains(&profile),
        "the confirmation names the profile being deleted"
    );
    save("delete-profile", dialog.upcast_ref());
    dialog.response(ResponseType::Accept);
    eprintln!("GTK page workflow captured at {width}x{height}: prompts, validation, local folders, links and deletion");
}
