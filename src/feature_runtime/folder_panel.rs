//! "Page folders" popover body: create row, folder list, inline rename and
//! empty state. Commands are routed to the runtime through one handler, so
//! every store change still goes through `change_organization`.

use crate::ui;
use gtk::{
    prelude::*, Box as GtkBox, Button, Entry, Image, Label, ListBox, ListBoxRow, Orientation,
    ScrolledWindow,
};
use sdeck_plus::organization::NavigationAction;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// A folder action requested from the popover.
pub(super) enum FolderCommand {
    Create(String),
    Open(String),
    Link(String),
    Rename { old: String, new: String },
    Delete(String),
}

/// Runs a command; `Err` carries a user-facing message for inline feedback.
pub(super) type FolderHandler = Rc<dyn Fn(FolderCommand) -> Result<(), String>>;

/// Folder names and delete availability the rows were last built from.
type BuiltRows = Option<(Vec<String>, bool)>;

pub(super) const DUPLICATE: &str = "A folder with this name already exists";

/// Why a typed folder name cannot be used. `Blank` shows no message.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum NameProblem {
    Blank,
    Message(String),
}

/// Validate a trimmed folder name against the page's existing folders.
/// `current` is the folder being renamed, which may keep its own name.
pub(super) fn check_name(
    name: &str,
    existing: &[String],
    current: Option<&str>,
) -> Result<(), NameProblem> {
    if name.is_empty() {
        return Err(NameProblem::Blank);
    }
    if let Err(error) = NavigationAction::OpenFolder(name.to_owned()).validate() {
        return Err(NameProblem::Message(sentence(&error.to_string())));
    }
    if current != Some(name) && existing.iter().any(|folder| folder == name) {
        return Err(NameProblem::Message(DUPLICATE.into()));
    }
    Ok(())
}

fn sentence(message: &str) -> String {
    let mut chars = message.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn show_problem(label: &Label, problem: Option<&str>) {
    label.set_text(problem.unwrap_or_default());
    label.set_visible(problem.is_some());
}

fn inline_error() -> Label {
    let label = Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_max_width_chars(38);
    label.add_css_class("helper");
    label.add_css_class("feedback-error");
    label.set_visible(false);
    label
}

#[derive(Clone)]
pub(super) struct FolderPanel {
    pub(super) root: GtkBox,
    context: Label,
    name: Entry,
    create: Button,
    create_error: Label,
    list: ListBox,
    scroller: ScrolledWindow,
    empty: GtkBox,
    names: Rc<RefCell<Vec<String>>>,
    deletable: Rc<Cell<bool>>,
    built: Rc<RefCell<BuiltRows>>,
    handler: Rc<RefCell<Option<FolderHandler>>>,
}

impl FolderPanel {
    pub(super) fn new() -> Self {
        let root = GtkBox::new(Orientation::Vertical, 12);
        root.add_css_class("folder-panel");
        root.set_width_request(340);
        let heading = GtkBox::new(Orientation::Vertical, 2);
        let context = Label::new(None);
        context.set_xalign(0.0);
        context.set_ellipsize(gtk::pango::EllipsizeMode::End);
        context.add_css_class("field-label");
        context.add_css_class("folder-context");
        heading.append(&context);
        heading.append(&ui::helper(
            "Link saves navigation on the selected control without opening it.",
        ));
        root.append(&heading);

        let create_row = GtkBox::new(Orientation::Horizontal, 0);
        create_row.add_css_class("linked");
        create_row.add_css_class("folder-create");
        let name = Entry::builder()
            .placeholder_text("New folder name")
            .hexpand(true)
            .build();
        name.update_property(&[gtk::accessible::Property::Label("New folder name")]);
        let create = ui::icon_button("folder-new-symbolic", "New folder");
        create.add_css_class("suggested-action");
        create.set_sensitive(false);
        create_row.append(&name);
        create_row.append(&create);
        let create_group = GtkBox::new(Orientation::Vertical, 4);
        create_group.append(&create_row);
        let create_error = inline_error();
        create_group.append(&create_error);
        root.append(&create_group);

        let list = ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        list.add_css_class("folder-list");
        list.update_property(&[gtk::accessible::Property::Label("Folders on this page")]);
        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true)
            .max_content_height(240)
            .child(&list)
            .build();
        scroller.add_css_class("folder-scroller");
        root.append(&scroller);

        let empty = GtkBox::new(Orientation::Vertical, 6);
        empty.add_css_class("folder-empty");
        empty.set_halign(gtk::Align::Center);
        let icon = Image::from_icon_name("folder-symbolic");
        icon.set_pixel_size(32);
        icon.add_css_class("dim-label");
        empty.append(&icon);
        let message = Label::new(Some("No folders on this page yet"));
        message.add_css_class("helper");
        empty.append(&message);
        root.append(&empty);

        let panel = Self {
            root,
            context,
            name,
            create,
            create_error,
            list,
            scroller,
            empty,
            names: Rc::default(),
            deletable: Rc::new(Cell::new(true)),
            built: Rc::default(),
            handler: Rc::default(),
        };
        panel.connect_create();
        let callback = panel.clone();
        panel.list.connect_row_activated(move |_, row| {
            let name = usize::try_from(row.index())
                .ok()
                .and_then(|index| callback.names.borrow().get(index).cloned());
            if let Some(name) = name {
                let _ = callback.run(FolderCommand::Open(name));
            }
        });
        panel.show_rows(&[]);
        panel
    }

    pub(super) fn set_handler(&self, handler: FolderHandler) {
        *self.handler.borrow_mut() = Some(handler);
    }

    fn run(&self, command: FolderCommand) -> Result<(), String> {
        // Clone the handler out so it may refresh this panel re-entrantly.
        let handler = self.handler.borrow().clone();
        handler.map_or(Ok(()), |handler| handler(command))
    }

    fn connect_create(&self) {
        let panel = self.clone();
        self.name.connect_changed(move |_| {
            panel.validate_create();
        });
        let panel = self.clone();
        self.name.connect_activate(move |_| panel.submit_create());
        let panel = self.clone();
        self.create.connect_clicked(move |_| panel.submit_create());
    }

    fn validate_create(&self) -> bool {
        let text = self.name.text();
        let result = check_name(text.trim(), &self.names.borrow(), None);
        self.create.set_sensitive(result.is_ok());
        match &result {
            Err(NameProblem::Message(message)) => show_problem(&self.create_error, Some(message)),
            Ok(()) | Err(NameProblem::Blank) => show_problem(&self.create_error, None),
        }
        result.is_ok()
    }

    fn submit_create(&self) {
        if !self.validate_create() {
            return;
        }
        let name = self.name.text().trim().to_owned();
        match self.run(FolderCommand::Create(name)) {
            Ok(()) => {
                self.name.set_text("");
                self.name.grab_focus();
            }
            Err(message) => show_problem(&self.create_error, Some(&message)),
        }
    }

    /// Clear typed input when the owning page changes.
    pub(super) fn reset_input(&self) {
        self.name.set_text("");
    }

    /// Show the selected page's folders; rows rebuild only when they change,
    /// so an unrelated refresh never interrupts an inline rename.
    pub(super) fn refresh(&self, page_name: &str, names: &[String], deletable: bool) {
        self.context
            .set_text(&format!("Folders on \u{201c}{page_name}\u{201d}"));
        self.context.set_tooltip_text(Some(page_name));
        let state = (names.to_vec(), deletable);
        if self.built.borrow().as_ref() != Some(&state) {
            *self.names.borrow_mut() = names.to_vec();
            self.deletable.set(deletable);
            self.show_rows(names);
            *self.built.borrow_mut() = Some(state);
        }
        self.validate_create();
    }

    fn show_rows(&self, names: &[String]) {
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        for name in names {
            self.list.append(&self.row(name));
        }
        self.scroller.set_visible(!names.is_empty());
        self.empty.set_visible(names.is_empty());
    }

    fn row(&self, name: &str) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.set_activatable(true);
        row.add_css_class("folder-row");
        row.set_tooltip_text(Some(&format!("Open folder {name}")));
        row.update_property(&[gtk::accessible::Property::Label(&format!("Folder {name}"))]);
        let content = GtkBox::new(Orientation::Horizontal, 10);
        let icon = Image::from_icon_name("folder-symbolic");
        icon.set_pixel_size(16);
        icon.add_css_class("folder-row-icon");
        content.append(&icon);
        let column = GtkBox::new(Orientation::Vertical, 4);
        column.set_hexpand(true);
        column.set_valign(gtk::Align::Center);
        let label = Label::new(Some(name));
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_width_chars(8);
        label.add_css_class("folder-name");
        let editor = Entry::builder().text(name).hexpand(true).build();
        editor.set_visible(false);
        editor.set_tooltip_text(Some("Enter saves · Escape cancels"));
        editor.update_property(&[gtk::accessible::Property::Label(&format!(
            "New name for folder {name}"
        ))]);
        let error = inline_error();
        column.append(&label);
        column.append(&editor);
        column.append(&error);
        content.append(&column);
        let actions = GtkBox::new(Orientation::Horizontal, 2);
        actions.set_valign(gtk::Align::Center);
        actions.add_css_class("folder-row-actions");
        let button = |icon: &str, caption: String| {
            let button = ui::icon_button(icon, &caption);
            button.add_css_class("flat");
            actions.append(&button);
            button
        };
        let open = button("folder-open-symbolic", format!("Open folder {name}"));
        let link = button(
            "sdeck-link-symbolic",
            format!("Link folder {name} to control"),
        );
        let rename = button("document-edit-symbolic", format!("Rename folder {name}"));
        let delete = button("user-trash-symbolic", format!("Delete folder {name}"));
        delete.add_css_class("destructive-action");
        delete.set_sensitive(self.deletable.get());
        content.append(&actions);
        row.set_child(Some(&content));

        for (button, command) in [
            (&open, FolderCommand::Open as fn(String) -> FolderCommand),
            (&link, FolderCommand::Link),
            (&delete, FolderCommand::Delete),
        ] {
            let panel = self.clone();
            let name = name.to_owned();
            button.connect_clicked(move |_| {
                let _ = panel.run(command(name.clone()));
            });
        }
        let edit = InlineRename {
            panel: self.clone(),
            old: name.to_owned(),
            label,
            editor,
            error,
            actions,
        };
        let start = edit.clone();
        rename.connect_clicked(move |_| start.begin());
        edit.connect();
        row
    }
}

/// One row's in-place rename: label swaps for a prefilled, selected entry.
#[derive(Clone)]
struct InlineRename {
    panel: FolderPanel,
    old: String,
    label: Label,
    editor: Entry,
    error: Label,
    actions: GtkBox,
}

impl InlineRename {
    fn begin(&self) {
        self.editor.set_text(&self.old);
        show_problem(&self.error, None);
        self.label.set_visible(false);
        self.actions.set_visible(false);
        self.editor.set_visible(true);
        self.editor.grab_focus();
        self.editor.select_region(0, -1);
    }

    fn cancel(&self) {
        if !self.editor.get_visible() {
            return;
        }
        self.editor.set_visible(false);
        show_problem(&self.error, None);
        self.label.set_visible(true);
        self.actions.set_visible(true);
    }

    fn save(&self) {
        let text = self.editor.text();
        let new = text.trim();
        let names = self.panel.names.borrow().clone();
        match check_name(new, &names, Some(&self.old)) {
            Err(NameProblem::Blank) => {
                show_problem(&self.error, Some("Enter a folder name"));
                return;
            }
            Err(NameProblem::Message(message)) => {
                show_problem(&self.error, Some(&message));
                return;
            }
            Ok(()) if new == self.old => {
                self.cancel();
                return;
            }
            Ok(()) => {}
        }
        let command = FolderCommand::Rename {
            old: self.old.clone(),
            new: new.to_owned(),
        };
        // On success the list rebuilds with the new name.
        if let Err(message) = self.panel.run(command) {
            show_problem(&self.error, Some(&message));
        }
    }

    fn connect(&self) {
        let edit = self.clone();
        self.editor.connect_activate(move |_| edit.save());
        let keys = gtk::EventControllerKey::new();
        let edit = self.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                edit.cancel();
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        self.editor.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        let edit = self.clone();
        focus.connect_leave(move |_| edit.cancel());
        self.editor.add_controller(focus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_checked_before_submitting() {
        let existing = vec!["Studio".to_owned()];
        assert_eq!(check_name("", &existing, None), Err(NameProblem::Blank));
        assert_eq!(
            check_name("Studio", &existing, None),
            Err(NameProblem::Message(DUPLICATE.into()))
        );
        assert_eq!(check_name("Studio", &existing, Some("Studio")), Ok(()));
        assert!(matches!(
            check_name("a/b", &existing, None),
            Err(NameProblem::Message(message)) if message.starts_with("Folder names")
        ));
        assert_eq!(check_name("Tools", &existing, None), Ok(()));
    }
}
