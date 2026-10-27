use super::*;

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
        if width == 1000 {
            if let Some(docs) = std::env::var_os("STREAMDECK_QA_DOCS_DIR") {
                let filename = match name {
                    "new-page" => Some("new-page-name.png"),
                    "rename-page" => Some("rename-page.png"),
                    "owned-folders" => Some("page-folders.png"),
                    _ => None,
                };
                if let Some(filename) = filename {
                    texture
                        .save_to_png(std::path::PathBuf::from(docs).join(filename))
                        .unwrap();
                }
            }
        }
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

    let profile_panel = widgets.management_menu.popover().unwrap();
    let mut entries = Vec::new();
    collect_entries(profile_panel.upcast_ref(), &mut entries);
    entries
        .iter()
        .find(|entry| entry.placeholder_text().as_deref() == Some("New profile name"))
        .unwrap()
        .set_text(&format!("QA profile {width}"));
    button("Create profile").emit_clicked();
    button("New page").emit_clicked();
    let dialog = current_dialog();
    page_dialog_entry(&dialog).set_text("Profile tools");
    dialog.response(ResponseType::Accept);
    name.set_text("QA child");
    button("New folder").emit_clicked();
    button("Delete profile").emit_clicked();
    let dialog = current_dialog();
    save("delete-profile", dialog.upcast_ref());
    dialog.response(ResponseType::Accept);
    eprintln!("GTK page workflow captured at {width}x{height}: prompts, validation, local folders, links and deletion");
}
