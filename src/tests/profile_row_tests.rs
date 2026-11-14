//! The profile rail rows: each carries its own Edit and Delete icons, "+" beside PROFILES is the
//! only New profile control and there is no Organize menu.
use super::organize_popover_tests::dialog_titled;
use super::ui_refresh_tests::{fixture, Fixture};
use super::*;

/// The rail's width before the rows grew their icons (default window minus the content column).
const PREVIOUS_RAIL_WIDTH: i32 = 166;

pub(super) fn collect<T: IsA<gtk::Widget> + glib::object::IsClass>(
    widget: &gtk::Widget,
    out: &mut Vec<T>,
) {
    if let Ok(found) = widget.clone().downcast::<T>() {
        out.push(found);
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect(&current, out);
        child = current.next_sibling();
    }
}

fn all<T: IsA<gtk::Widget> + glib::object::IsClass>(widget: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    collect(widget, &mut found);
    found
}

/// The tab label widget of a profile: the row that holds its name and icons.
pub(super) fn row_of(widgets: &MonitorWidgets, profile: &str) -> gtk::Widget {
    row_in(widgets.feature_runtime.profile_tabs(), profile)
}

fn row_in(tabs: &gtk::Notebook, profile: &str) -> gtk::Widget {
    (0..tabs.n_pages())
        .filter_map(|index| tabs.nth_page(Some(index)))
        .filter_map(|page| tabs.tab_label(&page))
        .find(|row| {
            all::<Label>(row)
                .iter()
                .any(|label| label.text() == profile)
        })
        .unwrap_or_else(|| panic!("profile row {profile}"))
}

fn row_button(widgets: &MonitorWidgets, profile: &str, class: &str) -> Option<Button> {
    all::<Button>(&row_of(widgets, profile))
        .into_iter()
        .find(|button| button.has_css_class(class))
}

pub(super) fn edit_button(widgets: &MonitorWidgets, profile: &str) -> Button {
    edit_button_in(widgets.feature_runtime.profile_tabs(), profile)
}

/// The Edit icon of a profile row, looked up in the tab strip (rows are rebuilt on changes).
pub(super) fn edit_button_in(tabs: &gtk::Notebook, profile: &str) -> Button {
    all::<Button>(&row_in(tabs, profile))
        .into_iter()
        .find(|button| button.has_css_class("profile-row-edit"))
        .unwrap_or_else(|| panic!("Edit button in the {profile} row"))
}

pub(super) fn delete_button(widgets: &MonitorWidgets, profile: &str) -> Option<Button> {
    row_button(widgets, profile, "profile-row-delete")
}

/// The dialog's "Profile to edit" picker (a hidden legacy page picker precedes it).
fn profile_picker(dialog: &gtk::Window) -> gtk::DropDown {
    all::<gtk::DropDown>(dialog.upcast_ref())
        .into_iter()
        .find(|picker| picker.is_visible())
        .expect("the profile picker")
}

fn with_profiles(store: &mut LayerStore) {
    for name in ["Studio", "Mixer"] {
        if !store.layers().contains_key(name) {
            store.create(name).unwrap();
        }
        store
            .bind(name, Trigger::KeyPress(1), Action::Command("true".into()))
            .unwrap();
    }
    store.select("Base");
}

fn open_confirmation() -> Option<gtk::MessageDialog> {
    gtk::Window::toplevels()
        .snapshot()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk::MessageDialog>().ok())
        .find(|dialog| dialog.is_visible())
}

fn profile_keys(fixture: &Fixture) -> Vec<String> {
    fixture.layers.borrow().layers().keys().cloned().collect()
}

fn image_names(widget: &gtk::Widget) -> Vec<String> {
    all::<gtk::Image>(widget)
        .iter()
        .filter_map(|image| image.icon_name())
        .map(|name| name.to_string())
        .collect()
}

#[test]
fn the_rail_has_no_organize_control_and_no_tools_heading() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("row-no-organize", with_profiles) else {
            return;
        };
        let widgets = &fixture.widgets;
        wait_for_frame(&fixture.window);
        let tabs: gtk::Widget = widgets.feature_runtime.profile_tabs().clone().upcast();
        // The rail is the tab strip; the profile pages beside it hold the editor's own menus.
        let rail = {
            let mut child = tabs.first_child();
            loop {
                let current = child.expect("the tab strip");
                if current.css_name() == "header" {
                    break current;
                }
                child = current.next_sibling();
            }
        };
        let menus = all::<gtk::MenuButton>(&rail);
        assert!(
            menus.iter().all(|menu| !menu
                .tooltip_text()
                .is_some_and(|tip| tip.contains("Organize"))),
            "no Organize menu"
        );
        assert_eq!(menus.len(), 1, "OBS is the only tool menu in the rail");
        assert!(all::<Label>(&rail)
            .iter()
            .all(|label| label.text() != "TOOLS"));
        let obs = widgets.obs_menu.compute_bounds(&widgets.root).unwrap();
        let heading = all::<Label>(&tabs)
            .into_iter()
            .find(|label| label.text() == "PROFILES")
            .unwrap()
            .compute_bounds(&widgets.root)
            .unwrap();
        assert!(obs.y() < heading.y(), "OBS sits above PROFILES");
    });
}

#[test]
fn each_profile_row_ends_with_edit_and_delete_icons_named_after_the_profile() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("row-icons", with_profiles) else {
            return;
        };
        let widgets = &fixture.widgets;
        wait_for_frame(&fixture.window);
        for profile in ["Studio", "Mixer"] {
            let edit = edit_button(widgets, profile);
            let delete = delete_button(widgets, profile).expect("Delete in a non-Base row");
            assert_eq!(edit.tooltip_text().unwrap(), format!("Edit {profile}"));
            assert_eq!(delete.tooltip_text().unwrap(), format!("Delete {profile}"));
            assert_eq!(image_names(edit.upcast_ref()), ["document-edit-symbolic"]);
            assert_eq!(image_names(delete.upcast_ref()), ["user-trash-symbolic"]);
            assert!(edit.has_css_class("icon-only") && delete.has_css_class("icon-only"));
            assert!(delete.is_sensitive());
            let name = all::<Label>(&row_of(widgets, profile))
                .into_iter()
                .find(|label| label.text() == profile)
                .unwrap()
                .compute_bounds(&widgets.root)
                .unwrap();
            let edit_at = edit.compute_bounds(&widgets.root).unwrap();
            let delete_at = delete.compute_bounds(&widgets.root).unwrap();
            assert!(
                edit_at.x() >= name.x() + name.width() - 0.5 && delete_at.x() > edit_at.x(),
                "name, then Edit, then Delete: {name:?} {edit_at:?} {delete_at:?}"
            );
            assert!(
                (edit_at.y() - delete_at.y()).abs() <= 1.0,
                "icons share the name's row"
            );
        }
    });
}

#[test]
fn base_can_be_edited_but_its_delete_is_off_and_explained() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("row-base", with_profiles) else {
            return;
        };
        let widgets = &fixture.widgets;
        assert_eq!(
            edit_button(widgets, "Base").tooltip_text().as_deref(),
            Some("Edit Base")
        );
        let delete = delete_button(widgets, "Base").expect("a disabled Delete keeps rows aligned");
        assert!(!delete.is_sensitive());
        assert_eq!(
            delete.tooltip_text().as_deref(),
            Some("Base cannot be deleted.")
        );
        let before = profile_keys(&fixture);
        delete.emit_clicked();
        assert!(open_confirmation().is_none());
        assert_eq!(profile_keys(&fixture), before);
    });
}

#[test]
fn edit_opens_the_edit_dialog_for_that_rows_profile_without_switching() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("row-edit", with_profiles) else {
            return;
        };
        let widgets = &fixture.widgets;
        assert_eq!(fixture.layers.borrow().active(), "Base");
        let before = profile_keys(&fixture);
        edit_button(widgets, "Studio").emit_clicked();
        let dialog = dialog_titled(&fixture.window, "Edit profile");
        let picker = profile_picker(&dialog);
        let shown = picker
            .selected_item()
            .and_downcast::<gtk::StringObject>()
            .unwrap()
            .string();
        assert_eq!(shown, "Studio");
        assert_eq!(fixture.layers.borrow().active(), "Base", "no switch");
        dialog.close();
        assert_eq!(profile_keys(&fixture), before, "closing changes nothing");
        edit_button(widgets, "Mixer").emit_clicked();
        let dialog = dialog_titled(&fixture.window, "Edit profile");
        let picker = profile_picker(&dialog);
        assert_eq!(
            picker
                .selected_item()
                .and_downcast::<gtk::StringObject>()
                .unwrap()
                .string(),
            "Mixer"
        );
        dialog.close();
    });
}

#[test]
fn delete_confirms_for_that_rows_profile_and_cancel_or_escape_keep_it() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("row-delete-cancel", with_profiles) else {
            return;
        };
        let widgets = &fixture.widgets;
        let delete = delete_button(widgets, "Studio").unwrap();
        let before = profile_keys(&fixture);
        delete.emit_clicked();
        let dialog = open_confirmation().expect("confirmation opens");
        assert!(dialog
            .text()
            .is_some_and(|text| text == "Delete \u{201c}Studio\u{201d}?"));
        dialog.response(ResponseType::Cancel);
        assert_eq!(profile_keys(&fixture), before, "Cancel keeps the profile");
        delete.emit_clicked();
        open_confirmation().expect("confirmation opens").close();
        assert_eq!(profile_keys(&fixture), before, "Escape keeps the profile");
        assert_eq!(fixture.layers.borrow().active(), "Base");
    });
}

#[test]
fn accepting_the_confirmation_removes_only_that_rows_profile() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("row-delete-accept", with_profiles) else {
            return;
        };
        let widgets = &fixture.widgets;
        delete_button(widgets, "Studio").unwrap().emit_clicked();
        open_confirmation()
            .expect("confirmation opens")
            .response(ResponseType::Accept);
        let keys = profile_keys(&fixture);
        assert!(!keys.contains(&"Studio".to_owned()), "{keys:?}");
        assert!(keys.contains(&"Mixer".to_owned()) && keys.contains(&"Base".to_owned()));
    });
}

#[test]
fn choosing_a_row_name_still_switches_profile() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("row-switch", with_profiles) else {
            return;
        };
        let widgets = &fixture.widgets;
        let tabs = widgets.feature_runtime.profile_tabs();
        let studio = row_of(widgets, "Studio");
        let index = (0..tabs.n_pages())
            .find(|index| {
                tabs.nth_page(Some(*index))
                    .and_then(|page| tabs.tab_label(&page))
                    .as_ref()
                    == Some(&studio)
            })
            .unwrap();
        // The tab's own gesture drives the notebook; the icons are separate buttons inside it.
        tabs.set_current_page(Some(index));
        assert_eq!(fixture.layers.borrow().active(), "Studio");
        assert!(
            all::<Button>(&studio).len() == 2,
            "icons are buttons that take their own clicks"
        );
    });
}

#[test]
fn the_rail_is_wider_than_before_and_keeps_every_name_readable() {
    run_gtk_test(|| {
        let Some(fixture) = fixture("row-width", with_profiles) else {
            return;
        };
        let widgets = &fixture.widgets;
        for (width, height) in [(DEFAULT_WIDTH, DEFAULT_HEIGHT), (1000, 700)] {
            widgets.root.set_size_request(width, height);
            wait_for_frame(&fixture.window);
            let rail = width - widgets.scroller.width();
            assert!(
                rail > PREVIOUS_RAIL_WIDTH + 30,
                "the rail grew from {PREVIOUS_RAIL_WIDTH}px: {rail}px at {width}"
            );
            for profile in ["Base", "Studio", "Mixer"] {
                let name = all::<Label>(&row_of(widgets, profile))
                    .into_iter()
                    .find(|label| label.text() == profile)
                    .unwrap();
                assert!(!name.layout().is_ellipsized(), "{profile} at {width}");
                let delete = delete_button(widgets, profile).unwrap();
                let bounds = delete.compute_bounds(&widgets.root).unwrap();
                assert!(
                    bounds.x() + bounds.width() <= (width - widgets.scroller.width()) as f32,
                    "icons stay inside the rail: {bounds:?}"
                );
            }
        }
    });
}
