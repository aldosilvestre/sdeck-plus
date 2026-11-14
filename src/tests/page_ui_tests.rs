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
    folder_button(widgets, "Link folder QA folder to selected key or dial").emit_clicked();
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
    // Deleting a profile is its rail row's Delete icon (the dialog carries no second path).
    edit_profile.close();
    super::profile_row_tests::delete_button(widgets, &profile)
        .expect("the new profile's row Delete icon")
        .emit_clicked();
    let dialog = current_dialog();
    assert!(
        all_text(dialog.upcast_ref()).contains(&profile),
        "the confirmation names the profile being deleted"
    );
    save("delete-profile", dialog.upcast_ref());
    dialog.response(ResponseType::Accept);
    eprintln!("GTK page workflow captured at {width}x{height}: prompts, validation, local folders, links and deletion");
}

/// The page bar names the current page after "Save name", on the profile's
/// first page and on a created page (the real click path of the dialog).
#[test]
fn renaming_a_page_with_save_name_updates_the_page_indicator() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("rename-indicator", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        // As at startup: organization.json is read from the store's root.
        let (device, _device_requests) = mpsc::channel();
        let (events, _events) = mpsc::channel();
        widgets.feature_runtime.configure(
            sdeck_plus::organization::Organization::open_at(
                fixture.layers.borrow().root(),
                &fixture.layers.borrow(),
            ),
            Ok(sdeck_plus::obs::ObsConfig::default()),
            device,
            events,
        );
        let indicator = widgets.feature_runtime.indicator.clone();
        let click = |caption: &str| {
            let mut buttons = Vec::new();
            collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
            buttons
                .iter()
                .find(|button| button_caption(button).as_deref() == Some(caption))
                .unwrap_or_else(|| panic!("{caption} button"))
                .emit_clicked();
        };
        let save_name = |name: &str| {
            click("Rename page");
            let dialog = current_dialog();
            page_dialog_entry(&dialog).set_text(name);
            buttons_in(dialog.upcast_ref())
                .into_iter()
                .find(|button| button_caption(button).as_deref() == Some("Save name"))
                .expect("the Save name button")
                .emit_clicked();
            assert!(!dialog.is_visible(), "Save name closes the dialog");
        };

        assert!(
            indicator.text().contains("Page 1 of 1"),
            "{}",
            indicator.text()
        );
        save_name("Intro");
        assert_eq!(indicator.text(), "Base / Intro  ·  Page 1 of 1");
        assert_eq!(indicator.tooltip_text().as_deref(), Some("Base / Intro"));

        click("New page");
        let dialog = current_dialog();
        page_dialog_entry(&dialog).set_text("Second");
        dialog.response(ResponseType::Accept);
        assert!(indicator.text().contains("Second"), "{}", indicator.text());
        save_name("Renamed second");
        assert!(
            indicator.text().contains("Renamed second") && indicator.text().contains("Page 2 of 2"),
            "{}",
            indicator.text()
        );
        assert_eq!(
            indicator.tooltip_text().as_deref(),
            Some("Base / Renamed second")
        );

        // A profile whose first page is not Base.
        let names = fixture.layers.borrow().names();
        let studio = names.iter().position(|name| name == "Studio").unwrap();
        widgets.layer_picker.set_selected(studio as u32);
        assert_eq!(fixture.layers.borrow().active(), "Studio");
        save_name("Studio intro");
        assert_eq!(indicator.text(), "Studio / Studio intro  ·  Page 1 of 1");
    });
}

fn focus_of(class: &str) -> Result<Option<sdeck_plus::app_focus::FocusedApplication>, String> {
    Ok(Some(sdeck_plus::app_focus::FocusedApplication {
        window_class: class.into(),
        executable: Some(class.into()),
    }))
}

const OWN: &str = "dev.streamdeck.EventMonitor";

/// Renaming under an application-rule profile with "return to previous", while
/// the compositor reports focus moving to the app's own dialog and back.
#[test]
fn renaming_under_an_app_rule_profile_survives_focus_events_manual_tab() {
    run_gtk_test(|| rename_with_focus("manual tab", false, &[OWN], &[]));
}

#[test]
fn renaming_under_an_app_rule_profile_survives_focus_events_manual_tab_focus_back() {
    run_gtk_test(|| rename_with_focus("manual tab, focus back", false, &[OWN], &[OWN]));
}

#[test]
fn renaming_under_an_app_rule_profile_survives_focus_events_late_own_focus() {
    run_gtk_test(|| rename_with_focus("automatic, late own focus", true, &[OWN], &[OWN]));
}

#[test]
fn renaming_under_an_app_rule_profile_survives_focus_events_still_obs() {
    run_gtk_test(|| rename_with_focus("automatic, still obs", true, &["obs"], &["obs"]));
}

fn rename_with_focus(scenario: &str, via_focus: bool, while_open: &[&str], after_close: &[&str]) {
    {
        {
            let Some(fixture) = fixture("rename-focus", |_| {}) else {
                return;
            };
            let widgets = &fixture.widgets;
            let runtime = &widgets.feature_runtime;
            let mut organization = sdeck_plus::organization::Organization::open_at(
                fixture.layers.borrow().root(),
                &fixture.layers.borrow(),
            )
            .unwrap();
            organization
                .set_profile_application("Studio", "obs")
                .unwrap();
            organization
                .set_profile_returns_to_previous("Studio", true)
                .unwrap();
            let (device, _device_requests) = mpsc::channel();
            let (events, _events) = mpsc::channel();
            runtime.configure(
                Ok(organization),
                Ok(sdeck_plus::obs::ObsConfig::default()),
                device,
                events,
            );
            let indicator = runtime.indicator.clone();
            if via_focus {
                runtime.focus_changed(focus_of("obs"));
            } else {
                let studio = fixture
                    .layers
                    .borrow()
                    .names()
                    .iter()
                    .position(|name| name == "Studio")
                    .unwrap();
                widgets.layer_picker.set_selected(studio as u32);
            }
            assert_eq!(fixture.layers.borrow().active(), "Studio", "{scenario}");
            let mut buttons = Vec::new();
            collect_layout_buttons(widgets.root.upcast_ref(), &mut buttons);
            buttons
                .iter()
                .find(|button| button_caption(button).as_deref() == Some("Rename page"))
                .unwrap()
                .emit_clicked();
            let dialog = current_dialog();
            for class in while_open {
                runtime.focus_changed(focus_of(class));
            }
            page_dialog_entry(&dialog).set_text("Renamed under focus");
            buttons_in(dialog.upcast_ref())
                .into_iter()
                .find(|button| button_caption(button).as_deref() == Some("Save name"))
                .unwrap()
                .emit_clicked();
            assert!(!dialog.is_visible(), "{scenario}");
            for class in after_close {
                runtime.focus_changed(focus_of(class));
            }
            assert!(
                indicator.text().contains("Renamed under focus"),
                "{scenario}: {}",
                indicator.text()
            );
            let reopened = {
                let organization = widgets.feature_runtime.organization_snapshot();
                organization.current(&fixture.layers.borrow()).page_name
            };
            assert_eq!(reopened, "Renamed under focus", "{scenario}");
        }
    }
}
