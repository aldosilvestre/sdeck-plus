//! Confirmation dialogs, the headerless Menu popover and the profile rows' Delete icons,
//! as drawn in design/overlays/03-compact-command.html.
use super::profile_row_tests::delete_button;
use super::ui_refresh_tests::fixture;
use super::*;

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

fn find_row(root: &gtk::Widget, caption: &str) -> Button {
    let mut found = Vec::new();
    buttons_of(root, &mut found);
    found
        .into_iter()
        .find(|button| {
            // A caption may sit beside a key hint inside the button's own content.
            button_caption(button).as_deref() == Some(caption)
                || button
                    .first_child()
                    .and_then(|content| content.first_child())
                    .and_then(|first| first.downcast::<Label>().ok())
                    .is_some_and(|label| label.text() == caption)
        })
        .unwrap_or_else(|| panic!("{caption} button"))
}

fn open_confirmation() -> Option<gtk::MessageDialog> {
    gtk::Window::toplevels()
        .snapshot()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::MessageDialog>().ok())
        .find(|dialog| dialog.is_visible())
}

fn labels_of(dialog: &gtk::MessageDialog) -> Vec<String> {
    let mut labels = Vec::new();
    let mut child = dialog.message_area().first_child();
    while let Some(widget) = child {
        if let Some(label) = widget.downcast_ref::<Label>() {
            labels.push(label.text().to_string());
        }
        child = widget.next_sibling();
    }
    labels
}

fn with_studio(store: &mut LayerStore) {
    for key in 1..=2 {
        store
            .bind(
                "Studio",
                Trigger::KeyPress(key),
                Action::Command("true".into()),
            )
            .unwrap();
    }
    store.select("Studio");
}

/// The Delete icon in a profile's rail row.
fn row_delete(fixture: &super::ui_refresh_tests::Fixture, profile: &str) -> Button {
    delete_button(&fixture.widgets, profile).unwrap_or_else(|| panic!("{profile} Delete icon"))
}

#[test]
fn deleting_a_profile_asks_in_the_artboard_wording() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("confirm-profile", with_studio) else {
            return;
        };
        row_delete(&fixture, "Studio").emit_clicked();
        let dialog = open_confirmation().expect("confirmation is open");
        assert!(dialog.has_css_class("confirm-dialog"));
        assert_eq!(
            labels_of(&dialog),
            [
                "Delete \u{201c}Studio\u{201d}?",
                "1 page and 2 bindings go with it. This cannot be undone."
            ]
        );
        let cancel = find_row(dialog.upcast_ref(), "Cancel");
        let delete = find_row(dialog.upcast_ref(), "Delete");
        assert!(delete.has_css_class("destructive-action"));
        assert!(!cancel.has_css_class("destructive-action"));
        let mut hints = Vec::new();
        let mut stack = vec![cancel.clone().upcast::<gtk::Widget>()];
        while let Some(widget) = stack.pop() {
            if let Some(label) = widget.downcast_ref::<Label>() {
                hints.push(label.text().to_string());
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                stack.push(current.clone());
                child = current.next_sibling();
            }
        }
        hints.sort();
        assert_eq!(hints, ["Cancel", "Esc"]);
        dialog.response(ResponseType::Cancel);
    });
}

#[test]
fn page_and_touchscreen_confirmations_use_the_same_buttons_and_quotes() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("confirm-page", with_studio) else {
            return;
        };
        let mut buttons = Vec::new();
        collect_layout_buttons(fixture.widgets.root.upcast_ref(), &mut buttons);
        let delete_page = buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some("Delete page"))
            .expect("Delete page")
            .clone();
        delete_page.emit_clicked();
        let dialog = open_confirmation().expect("confirmation is open");
        let labels = labels_of(&dialog);
        assert!(
            labels[0].starts_with('\u{201c}') || labels[0].starts_with("Delete \u{201c}"),
            "{labels:?}"
        );
        assert!(labels[1].ends_with("This cannot be undone."), "{labels:?}");
        find_row(dialog.upcast_ref(), "Cancel");
        assert!(find_row(dialog.upcast_ref(), "Delete").has_css_class("destructive-action"));
        dialog.response(ResponseType::Cancel);
    });
}

#[test]
fn cancel_and_escape_keep_the_profile_and_delete_removes_it() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("confirm-keep", with_studio) else {
            return;
        };
        let row = row_delete(&fixture, "Studio");
        let stores = || {
            fixture
                .layers
                .borrow()
                .layers()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };
        let before = stores();
        row.emit_clicked();
        open_confirmation().unwrap().response(ResponseType::Cancel);
        assert_eq!(stores(), before, "Cancel leaves the stores unchanged");
        row.emit_clicked();
        open_confirmation().unwrap().close();
        assert_eq!(stores(), before, "Escape leaves the stores unchanged");
        row.emit_clicked();
        open_confirmation().unwrap().response(ResponseType::Accept);
        assert!(!stores().contains(&"Studio".to_string()));
        assert!(stores().contains(&"Base".to_string()));
    });
}

#[test]
fn a_profile_delete_icon_is_destructive_and_off_for_base() {
    run_gtk_test(|| {
        let Some(base) = fixture("confirm-base", |_| {}) else {
            return;
        };
        let row = row_delete(&base, "Base");
        assert!(!row.is_sensitive(), "Base cannot be deleted");
        let Some(studio) = fixture("confirm-studio", with_studio) else {
            return;
        };
        let row = row_delete(&studio, "Studio");
        assert!(row.is_sensitive());
        assert!(row.has_css_class("destructive-action"));
        // No shortcut chip is faked on the icon.
        let mut chips = Vec::new();
        descendants_with_class(row.upcast_ref(), "key-hint", &mut chips);
        assert!(chips.is_empty());
    });
}

fn descendants_with_class(widget: &gtk::Widget, class: &str, found: &mut Vec<gtk::Widget>) {
    if widget.has_css_class(class) {
        found.push(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        descendants_with_class(&current, class, found);
        child = current.next_sibling();
    }
}

#[test]
fn the_menu_popover_is_headerless_and_keeps_its_items() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let window = ApplicationWindow::builder().default_width(400).build();
        install_style(&window);
        let menu = crate::configuration_menu::ConfigurationMenu::new(
            &window,
            &gtk::Box::new(gtk::Orientation::Vertical, 0),
        );
        let popover = menu.button().popover().expect("menu popover");
        let mut buttons = Vec::new();
        buttons_of(popover.upcast_ref(), &mut buttons);
        let mut captions: Vec<_> = buttons
            .iter()
            .filter_map(button_caption)
            .map(|caption| caption.to_string())
            .collect();
        captions.sort();
        assert_eq!(
            captions,
            [
                "Export configuration...",
                "Help",
                "Import configuration...",
                "Updates"
            ],
            "no Close row"
        );
        let mut titles = Vec::new();
        descendants_with_class(popover.upcast_ref(), "section-label", &mut titles);
        assert!(titles.is_empty(), "no title row");
        assert!(popover.has_css_class("command-menu"));
        window.destroy();
    });
}

#[test]
fn the_import_confirmation_reads_like_the_other_confirmations() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let window = ApplicationWindow::builder().default_width(400).build();
        install_style(&window);
        let menu = crate::configuration_menu::ConfigurationMenu::new(
            &window,
            &gtk::Box::new(gtk::Orientation::Vertical, 0),
        );
        menu.select_path_for_test(true, std::path::PathBuf::from("/tmp/none.sdeck-plus.json"));
        let dialog = menu.confirmation_for_test().expect("import confirmation");
        assert!(dialog.has_css_class("confirm-dialog"));
        let dialog = dialog
            .downcast::<gtk::MessageDialog>()
            .expect("message dialog");
        assert_eq!(labels_of(&dialog)[0], "Import configuration?");
        find_row(dialog.upcast_ref(), "Cancel");
        assert!(find_row(dialog.upcast_ref(), "Import").has_css_class("destructive-action"));
        dialog.response(ResponseType::Cancel);
        window.destroy();
    });
}

/// Opt-in capture (STREAMDECK_CATALOG_QA_DIR): confirmation and Menu popover.
#[test]
fn confirm_and_menu_capture() {
    run_gtk_test(|| {
        let Some(directory) = std::env::var_os("STREAMDECK_CATALOG_QA_DIR") else {
            return;
        };
        let Some(fixture) = fixture("confirm-capture", with_studio) else {
            return;
        };
        let directory = std::path::Path::new(&directory);
        let menu = crate::configuration_menu::ConfigurationMenu::new(
            &fixture.window,
            &gtk::Box::new(gtk::Orientation::Vertical, 0),
        );
        let panel = menu.button().popover().unwrap();
        panel.set_autohide(false);
        let anchor = menu.button().clone();
        fixture.window.set_titlebar(Some(&anchor));
        rendered_text_popup(&panel, || anchor.popup())
            .save_to_png(directory.join("menu-popover.png"))
            .unwrap();
        anchor.popdown();
        row_delete(&fixture, "Studio").emit_clicked();
        let dialog = open_confirmation().expect("confirmation");
        wait_for_frame(&dialog);
        rendered_texture(&dialog, &dialog.child().unwrap())
            .save_to_png(directory.join("confirm-delete-profile.png"))
            .unwrap();
        dialog.response(ResponseType::Cancel);
    });
}
