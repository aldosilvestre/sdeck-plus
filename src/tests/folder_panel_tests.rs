//! "Page folders" popover: folder list, create validation, inline rename,
//! confirmed per-row deletion and the empty state.
use super::ui_refresh_tests::fixture;
use super::*;
use gtk::glib::translate::IntoGlib;

fn descendants<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    if let Some(widget) = widget.downcast_ref::<T>() {
        found.push(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        found.extend(descendants::<T>(&current));
    }
    found
}

fn panel(widgets: &MonitorWidgets) -> gtk::Widget {
    widgets
        .feature_runtime
        .folder_menu()
        .popover()
        .unwrap()
        .upcast()
}

fn shown(widget: &impl IsA<gtk::Widget>) -> bool {
    widget.get_visible()
}

fn rows(widgets: &MonitorWidgets) -> Vec<String> {
    descendants::<gtk::ListBoxRow>(&panel(widgets))
        .iter()
        .filter_map(|row| {
            descendants::<Label>(row.upcast_ref())
                .into_iter()
                .find(|label| label.has_css_class("folder-name"))
                .map(|label| label.text().to_string())
        })
        .collect()
}

fn name_entry(widgets: &MonitorWidgets) -> Entry {
    descendants::<Entry>(&panel(widgets))
        .into_iter()
        .find(|entry| entry.placeholder_text().as_deref() == Some("New folder name"))
        .expect("the create row has its name entry")
}

fn button(widgets: &MonitorWidgets, tooltip: &str) -> Button {
    descendants::<Button>(&panel(widgets))
        .into_iter()
        .find(|button| button.tooltip_text().as_deref() == Some(tooltip))
        .unwrap_or_else(|| panic!("missing folder control {tooltip:?}"))
}

fn errors(widgets: &MonitorWidgets) -> Vec<String> {
    descendants::<Label>(&panel(widgets))
        .into_iter()
        .filter(|label| label.has_css_class("feedback-error") && shown(label))
        .map(|label| label.text().to_string())
        .collect()
}

fn empty_state_shown(widgets: &MonitorWidgets) -> bool {
    descendants::<Label>(&panel(widgets))
        .into_iter()
        .find(|label| label.text() == "No folders on this page yet")
        .and_then(|label| label.parent())
        .is_some_and(|state| shown(&state))
}

fn rename_editor(widgets: &MonitorWidgets) -> Option<Entry> {
    descendants::<Entry>(&panel(widgets))
        .into_iter()
        .find(|entry| entry.placeholder_text().is_none() && shown(entry))
}

fn create(widgets: &MonitorWidgets, name: &str) {
    name_entry(widgets).set_text(name);
    button(widgets, "New folder").emit_clicked();
}

#[test]
fn folder_list_tracks_creation_with_live_validation_and_empty_state() {
    run_gtk_test(assert_folder_creation);
}

fn assert_folder_creation() {
    let Some(f) = fixture("folder-create", |_| {}) else {
        return;
    };
    let w = &f.widgets;
    let owner = f.layers.borrow().active().to_owned();
    // Given: a page without folders shows the empty state.
    assert!(rows(w).is_empty());
    assert!(empty_state_shown(w));
    let add = button(w, "New folder");
    assert!(!add.is_sensitive(), "blank names cannot be created");
    assert!(errors(w).is_empty(), "blank input shows no message");
    // When: a valid name is typed and created.
    name_entry(w).set_text("Studio tools");
    assert!(add.is_sensitive());
    assert!(errors(w).is_empty());
    add.emit_clicked();
    // Then: the list shows it, the entry clears and the owner stays selected.
    assert_eq!(rows(w), ["Studio tools"]);
    assert!(!empty_state_shown(w));
    assert_eq!(name_entry(w).text(), "");
    assert!(!add.is_sensitive());
    assert_eq!(f.layers.borrow().active(), owner);
    // Duplicate, invalid and blank names keep the button disabled.
    name_entry(w).set_text("Studio tools");
    assert!(!add.is_sensitive());
    assert_eq!(errors(w), ["A folder with this name already exists"]);
    name_entry(w).set_text("a/b");
    assert!(!add.is_sensitive());
    assert!(
        errors(w)
            .first()
            .is_some_and(|message| message.starts_with("Folder names")),
        "{:?}",
        errors(w)
    );
    name_entry(w).set_text("   ");
    assert!(!add.is_sensitive());
    assert!(errors(w).is_empty());
    // Enter creates too, and rows stay sorted.
    name_entry(w).set_text("Audio");
    name_entry(w).emit_activate();
    assert_eq!(rows(w), ["Audio", "Studio tools"]);
    let count = w
        .feature_runtime
        .folder_menu()
        .child()
        .and_then(|child| child.last_child())
        .and_downcast::<Label>()
        .unwrap();
    assert_eq!(count.text(), "2");
    // Every row action names its folder for assistive technology.
    for action in ["Open folder", "Rename folder", "Delete folder"] {
        let control = button(w, &format!("{action} Audio"));
        assert_eq!(
            control.tooltip_text().as_deref(),
            Some(&*format!("{action} Audio"))
        );
    }
    button(w, "Link folder Audio to control");
}

#[test]
fn inline_rename_saves_on_enter_and_cancels_on_escape() {
    run_gtk_test(assert_inline_rename);
}

fn assert_inline_rename() {
    let Some(f) = fixture("folder-rename", |_| {}) else {
        return;
    };
    let w = &f.widgets;
    create(w, "Studio");
    create(w, "Tools");
    assert!(rename_editor(w).is_none());
    // When: Rename swaps the label for a prefilled entry.
    button(w, "Rename folder Studio").emit_clicked();
    let editor = rename_editor(w).expect("rename shows an inline entry");
    assert_eq!(editor.text(), "Studio");
    // A duplicate is reported inline without leaving the edit.
    editor.set_text("Tools");
    editor.emit_activate();
    assert_eq!(errors(w), ["A folder with this name already exists"]);
    assert!(shown(&editor));
    assert_eq!(rows(w), ["Studio", "Tools"]);
    // Enter saves a valid name; the list rebuilds with it.
    editor.set_text("Live");
    editor.emit_activate();
    assert_eq!(rows(w), ["Live", "Tools"]);
    assert!(rename_editor(w).is_none());
    assert!(errors(w).is_empty());
    button(w, "Open folder Live").emit_clicked();
    assert_ne!(f.layers.borrow().active(), "Base");
    let mut buttons = Vec::new();
    collect_layout_buttons(w.root.upcast_ref(), &mut buttons);
    buttons
        .iter()
        .find(|button| button_caption(button).as_deref() == Some("Parent"))
        .unwrap()
        .emit_clicked();
    assert_eq!(f.layers.borrow().active(), "Base");
    // Escape cancels without renaming.
    button(w, "Rename folder Tools").emit_clicked();
    let editor = rename_editor(w).unwrap();
    editor.set_text("Other");
    let escape = editor
        .observe_controllers()
        .snapshot()
        .into_iter()
        .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
        .expect("the inline editor listens for Escape");
    let handled = escape.emit_by_name::<bool>(
        "key-pressed",
        &[
            &gtk::gdk::Key::Escape.into_glib(),
            &0u32,
            &gtk::gdk::ModifierType::empty(),
        ],
    );
    assert!(handled, "Escape stays inside the editor");
    assert!(!shown(&editor));
    assert_eq!(rows(w), ["Live", "Tools"]);
    assert!(shown(&button(w, "Rename folder Tools")));
}

#[test]
fn row_delete_confirms_before_removing_the_folder() {
    run_gtk_test(assert_row_delete);
}

fn assert_row_delete() {
    let Some(f) = fixture("folder-delete", |_| {}) else {
        return;
    };
    let w = &f.widgets;
    create(w, "Studio");
    button(w, "Delete folder Studio").emit_clicked();
    current_dialog().response(ResponseType::Cancel);
    assert_eq!(rows(w), ["Studio"], "Keep leaves the folder");
    button(w, "Delete folder Studio").emit_clicked();
    current_dialog().response(ResponseType::Accept);
    assert!(rows(w).is_empty());
    assert!(empty_state_shown(w));
}
