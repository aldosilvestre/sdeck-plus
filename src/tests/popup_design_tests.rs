//! Popovers, menu and dropdown rows and dialogs follow design/overlays/03-compact-command.html:
//! form popovers carry no title bar, Close or arrow; rows are 38px; every dialog has ONE
//! right-aligned action row of natural-width buttons, the dismissal (with its Esc hint) first.
use super::profile_row_tests::{delete_button, edit_button};
use super::rail_tests::new_profile_button;
use super::ui_refresh_tests::{fixture, Fixture};
use super::*;

fn all<T: IsA<gtk::Widget> + glib::object::IsClass>(widget: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    super::profile_row_tests::collect(widget, &mut found);
    found
}

/// Border-box size (GTK `width()`/`height()` report the content box only).
fn outer(widget: &impl IsA<gtk::Widget>) -> (i32, i32) {
    let widget = widget.as_ref();
    (
        widget.measure(Orientation::Horizontal, -1).0,
        widget.measure(Orientation::Vertical, -1).0,
    )
}

fn caption(button: &Button) -> String {
    all::<Label>(button.upcast_ref())
        .first()
        .map(|label| label.text().to_string())
        .or_else(|| button.label().map(|label| label.to_string()))
        .unwrap_or_default()
}

fn has_hint(button: &Button, hint: &str) -> bool {
    all::<Label>(button.upcast_ref())
        .iter()
        .any(|label| label.has_css_class("key-hint") && label.text() == hint)
}

/// Wait for the real map signal (bounded), then a painted frame.
fn open_popover(panel: &gtk::Popover, open: impl FnOnce()) {
    let main_loop = glib::MainLoop::new(None, false);
    let done = main_loop.clone();
    let signal = panel.connect_map(move |_| done.quit());
    let expired = Rc::new(Cell::new(false));
    let flag = expired.clone();
    let done = main_loop.clone();
    let guard = glib::timeout_add_local_once(Duration::from_secs(5), move || {
        flag.set(true);
        done.quit();
    });
    open();
    if !panel.is_mapped() {
        main_loop.run();
    }
    panel.disconnect(signal);
    if !expired.get() {
        guard.remove();
    }
    assert!(panel.is_mapped(), "the popover maps");
    wait_for_frame(panel);
}

/// Run the Escape shortcut a widget's controllers carry, as the key press would.
fn escape_shortcut(widget: &impl IsA<gtk::Widget>) -> bool {
    let widget = widget.as_ref();
    let controllers = widget.observe_controllers();
    for index in 0..controllers.n_items() {
        let Some(controller) = controllers
            .item(index)
            .and_then(|item| item.downcast::<gtk::ShortcutController>().ok())
        else {
            continue;
        };
        for position in 0..controller.n_items() {
            let Some(shortcut) = controller
                .item(position)
                .and_then(|item| item.downcast::<gtk::Shortcut>().ok())
            else {
                continue;
            };
            let escape = shortcut
                .trigger()
                .is_some_and(|trigger| trigger.to_str() == "Escape");
            if escape {
                return shortcut.action().is_some_and(|action| {
                    action.activate(gtk::ShortcutActionFlags::empty(), widget, None)
                });
            }
        }
    }
    false
}

fn visible_message_dialog() -> Option<gtk::MessageDialog> {
    gtk::Window::toplevels()
        .snapshot()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::MessageDialog>().ok())
        .find(|dialog| dialog.is_visible())
}

fn titled_window(parent: &ApplicationWindow, title: &str) -> Option<gtk::Window> {
    gtk::Window::toplevels()
        .snapshot()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::Window>().ok())
        .find(|window| {
            window.is_visible()
                && window.title().as_deref() == Some(title)
                && window.transient_for().as_ref() == Some(parent.upcast_ref())
        })
}

/// The dialog's single action row holds exactly `captions`, in order, side by side at their
/// natural width, flush right inside the 14px padding; the first has the Esc hint.
fn assert_single_action_row(dialog: &gtk::Window, captions: &[&str], title_inset: bool) {
    wait_for_frame(dialog);
    let content = dialog.child().expect("dialog content");
    let rows: Vec<gtk::Box> = all::<gtk::Box>(&content)
        .into_iter()
        .filter(|row| {
            row.has_css_class("dialog-action-area") || row.has_css_class("dialog-actions")
        })
        .collect();
    assert_eq!(rows.len(), 1, "one action row");
    let row = &rows[0];
    let buttons: Vec<Button> = all::<Button>(row.upcast_ref());
    let names: Vec<String> = buttons.iter().map(caption).collect();
    assert_eq!(names, captions, "action order");
    assert!(has_hint(&buttons[0], "Esc"), "{} shows Esc", names[0]);
    for button in &buttons[1..] {
        assert!(!has_hint(button, "Esc"), "only the dismissal shows Esc");
    }
    let stray: Vec<String> = all::<Button>(&content)
        .iter()
        .filter(|button| button.parent().as_ref() != Some(row.upcast_ref()))
        .map(caption)
        .filter(|name| captions.contains(&name.as_str()))
        .collect();
    assert!(stray.is_empty(), "actions outside the row: {stray:?}");
    let bounds: Vec<_> = buttons
        .iter()
        .map(|button| button.compute_bounds(&content).expect("button bounds"))
        .collect();
    let first_y = bounds[0].y();
    for (button, bounds) in buttons.iter().zip(&bounds) {
        let natural = button.measure(Orientation::Horizontal, -1).1;
        assert!(
            bounds.width() <= natural as f32 + 1.0,
            "{} is natural width: {} vs {natural}",
            caption(button),
            bounds.width()
        );
        assert!(
            (bounds.y() - first_y).abs() < 1.0,
            "all actions share one row"
        );
    }
    // Insets are measured from the content's outer edge: its margins plus its CSS padding
    // (a widget's own bounds start at its padding edge, so padding shows as a negative x).
    let own = content.compute_bounds(&content).expect("content bounds");
    let outer_left = own.x() - content.margin_start() as f32;
    let outer_right = own.x() + own.width() + content.margin_end() as f32;
    let last = bounds.last().unwrap();
    let right_gap = outer_right - (last.x() + last.width());
    assert!(
        (right_gap - 14.0).abs() <= 1.0,
        "{names:?} sit flush right inside the 14px padding, gap {right_gap}"
    );
    for pair in bounds.windows(2) {
        let gap = pair[1].x() - (pair[0].x() + pair[0].width());
        assert!((gap - 8.0).abs() <= 1.0, "8px between actions, got {gap}");
    }
    if title_inset {
        let title = all::<Label>(&content)
            .into_iter()
            .find(|label| label.is_visible() && !label.text().is_empty())
            .expect("a title label");
        let x = title.compute_bounds(&content).unwrap().x() - outer_left;
        assert!((x - 14.0).abs() <= 1.0, "14px padding, title at {x}");
    }
}

fn names(fixture: &Fixture) -> Vec<String> {
    fixture.layers.borrow().names()
}

fn with_studio(store: &mut LayerStore) {
    if !store.layers().contains_key("Studio") {
        store.create("Studio").unwrap();
    }
}

#[test]
fn form_popovers_have_no_close_no_arrow_and_their_title_inside() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-forms", |_| {}) else {
            return;
        };
        let widgets = &fixture.widgets;
        let cases = [
            ("OBS Studio", widgets.obs_menu.popover().unwrap()),
            (
                "Page folders",
                widgets.feature_runtime.folder_menu().popover().unwrap(),
            ),
            ("Button text", widgets._text_editor.panel.clone()),
        ];
        for (title, panel) in cases {
            assert!(!panel.has_arrow(), "{title}: no arrow");
            assert!(!panel.has_css_class("organize-popover"));
            let content = panel.child().and_downcast::<gtk::Box>().expect(title);
            assert_eq!(
                (content.margin_top(), content.margin_start()),
                (12, 12),
                "{title}: 12px inner padding"
            );
            let heading = all::<Label>(content.upcast_ref())
                .into_iter()
                .find(|label| label.has_css_class("popover-title"))
                .unwrap_or_else(|| panic!("{title}: title inside the body"));
            assert_eq!(heading.text(), title);
            let subtitle = all::<Label>(content.upcast_ref())
                .into_iter()
                .find(|label| label.has_css_class("popover-subtitle"))
                .unwrap_or_else(|| panic!("{title}: muted subtitle"));
            assert!(!subtitle.text().is_empty());
            let closes: Vec<_> = all::<Button>(content.upcast_ref())
                .into_iter()
                .filter(|button| caption(button) == "Close")
                .collect();
            assert!(closes.is_empty(), "{title}: no Close button");
            assert!(
                heading.compute_bounds(&content).unwrap().y() <= 1.0,
                "{title}: the title is the first line"
            );
        }
        // Escape and an outside click close the autohiding ones (GTK's default); the text
        // editor keeps its font dialog's focus change from dismissing it and closes on Escape.
        assert!(widgets.obs_menu.popover().unwrap().is_autohide());
        assert!(widgets
            .feature_runtime
            .folder_menu()
            .popover()
            .unwrap()
            .is_autohide());
    });
}

#[test]
fn escape_closes_the_button_text_popover_without_saving() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-text-escape", |_| {}) else {
            return;
        };
        let editor = &fixture.widgets._text_editor;
        let before = names(&fixture);
        key_button(&fixture.widgets.key_labels[0]).emit_clicked();
        open_popover(&editor.panel, || editor.popup());
        assert!(escape_shortcut(&editor.panel), "Escape is handled");
        assert!(!editor.panel.is_visible(), "Escape closes the popover");
        assert_eq!(names(&fixture), before, "nothing was saved");
    });
}

#[test]
fn menu_rows_are_38px_with_hairline_separators_and_no_invented_hints() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-menu-rows", |_| {}) else {
            return;
        };
        let menu = crate::configuration_menu::ConfigurationMenu::new(
            &fixture.window,
            &gtk::Box::new(gtk::Orientation::Vertical, 0),
        );
        let popover = menu.button().popover().expect("menu popover");
        assert!(!popover.has_arrow());
        let rows = all::<Button>(popover.upcast_ref());
        assert_eq!(rows.len(), 4);
        for row in &rows {
            assert_eq!(outer(row).1, 38, "{} is 38px high", caption(row));
        }
        assert!(
            all::<Label>(popover.upcast_ref())
                .iter()
                .all(|label| !label.has_css_class("key-hint")),
            "no shortcut exists, so no kbd hint is drawn"
        );
        // A divided menu: the hairline plus 6px above and below.
        let sectioned = ui::command_menu_sections(&[
            &[ui::command_row("help-browser-symbolic", "One")],
            &[ui::command_row("help-browser-symbolic", "Two")],
        ]);
        let separator = all::<gtk::Separator>(sectioned.upcast_ref())
            .into_iter()
            .next()
            .expect("a separator");
        assert_eq!(outer(&separator).1, 13, "1px line with 6px margins");
        assert!(!sectioned.has_arrow());
    });
}

#[test]
fn the_key_context_menu_uses_the_same_38px_row() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-key-menu", |_| {}) else {
            return;
        };
        let key = fixture.widgets.key_widgets[2].clone();
        let controllers = key.observe_controllers();
        let gesture = (0..controllers.n_items())
            .filter_map(|index| {
                controllers
                    .item(index)?
                    .downcast::<gtk::GestureClick>()
                    .ok()
            })
            .find(|gesture| gesture.button() == 3)
            .expect("right-click gesture");
        gesture.emit_by_name::<()>("pressed", &[&1i32, &4.0f64, &4.0f64]);
        let menu = all::<gtk::Popover>(key.upcast_ref())
            .into_iter()
            .find(|popover| popover.has_css_class("key-context-menu"))
            .expect("key menu");
        assert!(!menu.has_arrow());
        let entry = all::<Button>(menu.upcast_ref())
            .into_iter()
            .next()
            .expect("menu entry");
        assert_eq!(caption(&entry), "Add text and image…");
        assert!(entry.has_css_class("command-row"), "an ordinary menu row");
        assert_eq!(outer(&entry).1, 38);
        assert!(!entry.has_css_class("suggested-action"));
        menu.popdown();
    });
}

#[test]
fn dropdown_list_rows_are_38px() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-dropdown-rows", |_| {}) else {
            return;
        };
        // Keys hide the Action selector (their actions come from the catalog); dials show it.
        fixture
            .widgets
            .binding_ui
            .select_control(ControlKind::Encoder, Some(1), true)
            .unwrap();
        let picker = fixture.widgets.binding_ui.action_picker.clone();
        let mut ancestor = picker.parent();
        while let Some(widget) = ancestor {
            if let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>() {
                if let Some(bounds) = picker.compute_bounds(scroll) {
                    let adjustment = scroll.vadjustment();
                    adjustment.set_value(adjustment.value() + f64::from(bounds.y()) - 40.0);
                    wait_for_frame(scroll);
                }
            }
            ancestor = widget.parent();
        }
        let panel = all::<gtk::Popover>(picker.upcast_ref())
            .into_iter()
            .next()
            .expect("dropdown popover");
        open_popover(&panel, || {
            assert!(picker.activate(), "native dropdown activation");
        });
        assert!(!panel.has_arrow(), "dropdown popups have no arrow");
        let rows = |panel: &gtk::Popover| -> Vec<gtk::Widget> {
            let mut found = Vec::new();
            fn walk(widget: &gtk::Widget, found: &mut Vec<gtk::Widget>) {
                if widget.css_name() == "row" && widget.is_mapped() {
                    found.push(widget.clone());
                }
                let mut child = widget.first_child();
                while let Some(current) = child {
                    walk(&current, found);
                    child = current.next_sibling();
                }
            }
            walk(panel.upcast_ref(), &mut found);
            found
        };
        // The list creates its rows in idle steps: wait for them (bounded, no fixed sleep).
        let started = std::time::Instant::now();
        while rows(&panel).is_empty() && started.elapsed() < Duration::from_secs(3) {
            glib::MainContext::default().iteration(false);
            wait_for_frame(&panel);
        }
        let rows = rows(&panel);
        assert!(!rows.is_empty(), "the list shows rows");
        for row in rows {
            assert_eq!(outer(&row).1, 38, "dropdown rows are 38px high");
        }
        panel.popdown();
    });
}

#[test]
fn delete_and_other_confirmations_have_one_right_aligned_action_row() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-confirm", with_studio) else {
            return;
        };
        let before = names(&fixture);
        let dialog = ui::delete_confirmation(&fixture.window, "Studio", "Gone for good.");
        dialog.present();
        assert_single_action_row(dialog.upcast_ref(), &["Cancel", "Delete"], true);
        let mut buttons = all::<Button>(dialog.upcast_ref());
        buttons.retain(|button| caption(button) == "Delete");
        assert!(buttons[0].has_css_class("destructive-action"));
        assert_eq!(
            dialog.default_widget().map(|widget| widget.type_().name()),
            Some("GtkButton"),
            "a default action is set"
        );
        dialog.close();
        assert_eq!(names(&fixture), before, "Escape changes nothing");
    });
}

#[test]
fn the_import_confirmation_and_artwork_error_follow_the_dialog_layout() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-import", |_| {}) else {
            return;
        };
        let before = names(&fixture);
        let menu = crate::configuration_menu::ConfigurationMenu::new(
            &fixture.window,
            &gtk::Box::new(gtk::Orientation::Vertical, 0),
        );
        menu.select_path_for_test(true, std::path::PathBuf::from("/tmp/none.sdeck-plus.json"));
        let dialog = menu
            .confirmation_for_test()
            .expect("import confirmation")
            .downcast::<gtk::MessageDialog>()
            .expect("message dialog");
        dialog.present();
        assert_single_action_row(dialog.upcast_ref(), &["Cancel", "Import"], true);
        dialog.response(ResponseType::Cancel);
        assert_eq!(names(&fixture), before);
        show_artwork_error(&fixture.window, "Could not read the image.");
        let error = visible_message_dialog().expect("artwork error dialog");
        assert!(
            error.has_css_class("confirm-dialog"),
            "styled like the rest"
        );
        assert_single_action_row(error.upcast_ref(), &["Close"], true);
        error.close();
    });
}

#[test]
fn the_page_name_dialog_has_cancel_with_esc_then_the_primary() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-page-name", |_| {}) else {
            return;
        };
        let before = names(&fixture);
        let mut buttons = Vec::new();
        collect_layout_buttons(fixture.widgets.root.upcast_ref(), &mut buttons);
        buttons
            .iter()
            .find(|button| button_caption(button).as_deref() == Some("New page"))
            .expect("New page")
            .emit_clicked();
        let dialog = gtk::Window::toplevels()
            .snapshot()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::Dialog>().ok())
            .find(|dialog| dialog.is_visible() && dialog.has_css_class("page-name-dialog"))
            .expect("page-name dialog");
        assert_single_action_row(dialog.upcast_ref(), &["Cancel", "Create page"], true);
        dialog.close();
        assert_eq!(names(&fixture), before, "Escape creates nothing");
    });
}

#[test]
fn the_profile_dialogs_put_their_actions_in_one_row() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-profile", with_studio) else {
            return;
        };
        let before = names(&fixture);
        new_profile_button(&fixture.widgets).emit_clicked();
        let dialog = titled_window(&fixture.window, "New profile").expect("New profile");
        assert_single_action_row(&dialog, &["Cancel", "Create profile"], true);
        let create = all::<Button>(dialog.upcast_ref())
            .into_iter()
            .find(|button| caption(button) == "Create profile")
            .unwrap();
        assert!(create.has_css_class("suggested-action"));
        assert!(escape_shortcut(&dialog), "Escape is handled");
        assert!(!dialog.is_visible());
        assert_eq!(names(&fixture), before, "Escape creates nothing");

        edit_button(&fixture.widgets, "Base").emit_clicked();
        let dialog = titled_window(&fixture.window, "Edit profile").expect("Edit profile");
        assert_single_action_row(&dialog, &["Cancel", "Save changes"], true);
        let captions: Vec<String> = all::<Button>(dialog.upcast_ref())
            .iter()
            .map(caption)
            .collect();
        assert!(
            !captions.contains(&"Delete profile".to_string()),
            "Delete lives on the profile row: {captions:?}"
        );
        assert!(
            delete_button(&fixture.widgets, "Studio").is_some(),
            "the row's Delete icon is still the way to delete"
        );
        assert!(escape_shortcut(&dialog));
        assert_eq!(names(&fixture), before, "Escape changes nothing");
    });
}

#[test]
fn setup_help_pads_its_content_and_closes_with_esc() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-help", |_| {}) else {
            return;
        };
        let menu = crate::configuration_menu::ConfigurationMenu::new(
            &fixture.window,
            &gtk::Box::new(gtk::Orientation::Vertical, 0),
        );
        let help = all::<Button>(menu.button().popover().unwrap().upcast_ref())
            .into_iter()
            .find(|button| caption(button) == "Help")
            .expect("Help row");
        help.emit_clicked();
        let dialog = titled_window(&fixture.window, "Setup help").expect("Setup help dialog");
        assert_single_action_row(&dialog, &["Close"], false);
        let content = dialog.child().unwrap();
        let scroller = all::<gtk::ScrolledWindow>(&content)
            .into_iter()
            .next()
            .expect("help body");
        let own = content.compute_bounds(&content).unwrap();
        let x = scroller.compute_bounds(&content).unwrap().x() - own.x();
        assert!((x - 14.0).abs() <= 1.0, "14px padding, body at {x}");
        dialog.close();
        assert!(!dialog.is_visible());
    });
}

#[test]
fn the_shortcut_reference_closes_with_an_esc_hint_inside_the_padding() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("popup-reference", |_| {}) else {
            return;
        };
        let guidance = Label::new(None);
        fixture.window.set_child(Some(&guidance));
        crate::shortcut_reference::connect(&guidance);
        let handled =
            guidance.emit_by_name::<bool>("activate-link", &[&"shortcut-reference".to_string()]);
        assert!(handled);
        let dialog = titled_window(&fixture.window, "Shortcut key reference")
            .expect("shortcut reference dialog");
        assert_single_action_row(&dialog, &["Close"], false);
        dialog.close();
    });
}

/// Opt-in capture (STREAMDECK_POPUP_QA_DIR): the popovers, rows and dialogs of this feature.
#[test]
fn popups_capture() {
    run_gtk_test(|| {
        let Some(directory) = std::env::var_os("STREAMDECK_POPUP_QA_DIR") else {
            return;
        };
        let Some(fixture) = fixture("popup-capture", with_studio) else {
            return;
        };
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        // The artboards are dark: capture the dark palette.
        let settings = gtk::Settings::default().unwrap();
        settings.set_gtk_theme_name(Some("Adwaita-dark"));
        settings.set_gtk_application_prefer_dark_theme(true);
        let widgets = &fixture.widgets;
        let save = |name: &str, panel: &gtk::Popover| {
            wait_for_frame(panel);
            rendered_texture(&fixture.window, panel)
                .save_to_png(directory.join(format!("{name}.png")))
                .unwrap();
        };
        let window_save = |name: &str, window: &gtk::Window| {
            wait_for_frame(window);
            rendered_texture(window, window)
                .save_to_png(directory.join(format!("{name}.png")))
                .unwrap();
        };
        for (name, menu) in [
            ("obs-settings", widgets.obs_menu.clone()),
            (
                "page-folders",
                widgets.feature_runtime.folder_menu().clone(),
            ),
        ] {
            let panel = menu.popover().unwrap();
            panel.set_autohide(false);
            open_popover(&panel, || menu.popup());
            save(name, &panel);
            menu.popdown();
        }
        let editor = &widgets._text_editor;
        key_button(&widgets.key_labels[0]).emit_clicked();
        open_popover(&editor.panel, || editor.popup());
        save("button-text", &editor.panel);
        editor.panel.popdown();
        let key = widgets.key_widgets[2].clone();
        let controllers = key.observe_controllers();
        let gesture = (0..controllers.n_items())
            .filter_map(|index| {
                controllers
                    .item(index)?
                    .downcast::<gtk::GestureClick>()
                    .ok()
            })
            .find(|gesture| gesture.button() == 3)
            .unwrap();
        gesture.emit_by_name::<()>("pressed", &[&1i32, &4.0f64, &4.0f64]);
        let menu = all::<gtk::Popover>(key.upcast_ref())
            .into_iter()
            .find(|popover| popover.has_css_class("key-context-menu"))
            .unwrap();
        open_popover(&menu, || {});
        save("key-context-menu", &menu);
        menu.popdown();
        let configuration = crate::configuration_menu::ConfigurationMenu::new(
            &fixture.window,
            &gtk::Box::new(gtk::Orientation::Vertical, 0),
        );
        let panel = configuration.button().popover().unwrap();
        panel.set_autohide(false);
        let anchor = configuration.button().clone();
        fixture.window.set_titlebar(Some(&anchor));
        open_popover(&panel, || anchor.popup());
        save("menu-popover", &panel);
        anchor.popdown();
        widgets
            .binding_ui
            .select_control(ControlKind::Encoder, Some(1), true)
            .unwrap();
        let picker = widgets.binding_ui.action_picker.clone();
        let dropdown = all::<gtk::Popover>(picker.upcast_ref())
            .into_iter()
            .next()
            .unwrap();
        dropdown.set_autohide(false);
        open_popover(&dropdown, || {
            picker.activate();
        });
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_millis(1500) {
            glib::MainContext::default().iteration(false);
            wait_for_frame(&dropdown);
            if all::<gtk::Widget>(dropdown.upcast_ref())
                .iter()
                .any(|row| row.css_name() == "row" && row.is_mapped())
            {
                break;
            }
        }
        save("action-dropdown", &dropdown);
        dropdown.popdown();
        let confirm = ui::delete_confirmation(
            &fixture.window,
            "Studio",
            "1 page and 0 bindings go with it. This cannot be undone.",
        );
        confirm.present();
        window_save("delete-confirmation", confirm.upcast_ref());
        confirm.close();
        new_profile_button(widgets).emit_clicked();
        window_save(
            "new-profile",
            &titled_window(&fixture.window, "New profile").unwrap(),
        );
        edit_button(widgets, "Base").emit_clicked();
        window_save(
            "edit-profile",
            &titled_window(&fixture.window, "Edit profile").unwrap(),
        );
        let help = all::<Button>(configuration.button().popover().unwrap().upcast_ref())
            .into_iter()
            .find(|button| caption(button) == "Help")
            .unwrap();
        help.emit_clicked();
        window_save(
            "setup-help",
            &titled_window(&fixture.window, "Setup help").unwrap(),
        );
    });
}
