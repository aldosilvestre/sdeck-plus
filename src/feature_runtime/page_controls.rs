use super::{
    pages::{PageEdit, Removal},
    FeatureRuntime,
};
use crate::ui;
use gtk::{prelude::*, Box as GtkBox, Button, Entry, Orientation};
use sdeck_plus::organization::{NavigationAction, Organization};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
pub(super) struct PageControls {
    /// Page management (folders, create, rename, delete), trailing the breadcrumb.
    pub(super) root: GtkBox,
    /// Linked previous/next/parent group, leading the breadcrumb.
    pub(super) navigation_group: GtkBox,
    navigation: [(Button, NavigationAction); 3],
    create: Button,
    rename: Button,
    pub(super) delete: Button,
    pub(super) folders: GtkBox,
    pub(super) folder_menu: gtk::MenuButton,
    pub(super) folder_name: Entry,
    folder_count: gtk::Label,
    folder_context: gtk::Label,
    folder_model: gtk::StringList,
    pub(super) folder_picker: gtk::DropDown,
    create_folder: Button,
    open_folder: Button,
    link_folder: Button,
    delete_folder: Button,
    owner: Rc<RefCell<Option<String>>>,
}

impl PageControls {
    pub(super) fn new() -> Self {
        let root = GtkBox::new(Orientation::Horizontal, 8);
        let navigation_group = GtkBox::new(Orientation::Horizontal, 0);
        navigation_group.add_css_class("linked");
        navigation_group.add_css_class("page-navigation");
        let navigation = [
            (
                ui::icon_button("go-previous-symbolic", "Previous"),
                NavigationAction::PreviousPage,
            ),
            (
                ui::icon_button("go-next-symbolic", "Next"),
                NavigationAction::NextPage,
            ),
            (
                ui::icon_button("go-up-symbolic", "Parent"),
                NavigationAction::ParentFolder,
            ),
        ];
        for (button, _) in &navigation {
            navigation_group.append(button);
        }
        let folders = GtkBox::new(Orientation::Vertical, 16);
        let folder_context = ui::helper("Folders belong to the selected page.");
        folders.append(&folder_context);
        let folder_name = Entry::builder()
            .placeholder_text("New folder name")
            .width_chars(16)
            .hexpand(true)
            .build();
        folders.append(&crate::labeled("New folder name", &folder_name));
        let create_folder = ui::button("folder-new-symbolic", "New folder");
        folders.append(&create_folder);
        folders.append(&gtk::Separator::new(Orientation::Horizontal));
        let folder_model = gtk::StringList::new(&[]);
        let folder_picker = gtk::DropDown::new(Some(folder_model.clone()), gtk::Expression::NONE);
        folder_picker.set_hexpand(true);
        folders.append(&crate::labeled("Folders on this page", &folder_picker));
        let actions = GtkBox::new(Orientation::Horizontal, 8);
        let open_folder = ui::button("folder-open-symbolic", "Open folder");
        let link_folder = ui::button("insert-link-symbolic", "Link to control");
        link_folder.set_tooltip_text(Some(
            "Save this folder on the selected control and event without opening it",
        ));
        let delete_folder = ui::button("user-trash-symbolic", "Delete folder");
        delete_folder.add_css_class("destructive-action");
        for button in [&open_folder, &link_folder, &delete_folder] {
            actions.append(button);
        }
        folders.append(&actions);
        let panel = ui::popover("Page folders", &ui::panel_body(&folders));
        let folder_count = gtk::Label::new(Some("0"));
        let folder_button = GtkBox::new(Orientation::Horizontal, 8);
        folder_button.append(&gtk::Image::from_icon_name("folder-symbolic"));
        folder_button.append(&gtk::Label::new(Some("Folders")));
        folder_button.append(&folder_count);
        let folder_menu = gtk::MenuButton::builder()
            .child(&folder_button)
            .tooltip_text("Page folders")
            .popover(&panel)
            .build();
        folder_menu.add_css_class("icon-action");
        folder_menu.add_css_class("page-folder-menu");
        folder_menu.update_property(&[gtk::accessible::Property::Label("Page folders")]);
        root.append(&folder_menu);
        let create = ui::button("document-new-symbolic", "New page");
        root.append(&create);
        // Rename/delete are less frequent: lighter frameless icons, still named,
        // and deletion still asks for confirmation.
        let rename = ui::icon_button("document-edit-symbolic", "Rename page");
        let delete = ui::icon_button("user-trash-symbolic", "Delete page");
        delete.add_css_class("destructive-action");
        let page_edits = GtkBox::new(Orientation::Horizontal, 2);
        page_edits.add_css_class("page-edits");
        for button in [&rename, &delete] {
            button.add_css_class("flat");
            page_edits.append(button);
        }
        root.append(&page_edits);
        Self {
            root,
            navigation_group,
            navigation,
            create,
            rename,
            delete,
            folders,
            folder_menu,
            folder_name,
            folder_count,
            folder_context,
            folder_model,
            folder_picker,
            create_folder,
            open_folder,
            link_folder,
            delete_folder,
            owner: Rc::new(RefCell::new(None)),
        }
    }

    pub(super) fn bind(&self, runtime: &FeatureRuntime) {
        for (button, action) in &self.navigation {
            let runtime = runtime.clone();
            let action = action.clone();
            button.connect_clicked(move |_| runtime.navigate(&action));
        }
        let callback = runtime.clone();
        self.create
            .connect_clicked(move |_| callback.edit_page(PageEdit::Create));
        let callback = runtime.clone();
        self.rename
            .connect_clicked(move |_| callback.edit_page(PageEdit::Rename));
        let callback = runtime.clone();
        self.delete.connect_clicked(move |_| {
            let key = callback.layers.borrow().active().to_owned();
            callback.confirm_removal(Removal::Page(key));
        });
        let callback = runtime.clone();
        self.create_folder
            .connect_clicked(move |_| callback.create_folder());
        let callback = runtime.clone();
        self.folder_name
            .connect_activate(move |_| callback.create_folder());
        let callback = runtime.clone();
        self.open_folder.connect_clicked(move |_| {
            if let Some(name) = callback.selected_folder() {
                callback.page_controls.folder_menu.popdown();
                callback.navigate(&NavigationAction::OpenFolder(name));
            }
        });
        let callback = runtime.clone();
        self.link_folder
            .connect_clicked(move |_| callback.link_folder());
        let callback = runtime.clone();
        self.delete_folder.connect_clicked(move |_| {
            if let Some(name) = callback.selected_folder() {
                let page = callback.layers.borrow().active().to_owned();
                callback.page_controls.folder_menu.popdown();
                callback.confirm_removal(Removal::Folder { page, name });
            }
        });
    }

    pub(super) fn refresh(
        &self,
        organization: &Organization,
        layers: &sdeck_plus::layers::LayerStore,
        artwork_available: bool,
    ) {
        let info = organization.current(layers);
        let changed = self.owner.borrow().as_deref() != Some(layers.active());
        if changed {
            self.folder_name.set_text("");
            *self.owner.borrow_mut() = Some(layers.active().to_owned());
        }
        self.folder_context.set_text(&format!(
            "Folders belong to \"{}\". Link saves navigation on the selected control.",
            info.page_name
        ));
        let selected = self
            .folder_picker
            .selected_item()
            .and_then(|item| item.downcast::<gtk::StringObject>().ok())
            .map(|item| item.string().to_string());
        let names = organization.folders(layers);
        self.folder_count.set_text(&names.len().to_string());
        let caption = format!("Page folders ({})", names.len());
        self.folder_menu.set_tooltip_text(Some(&caption));
        self.folder_menu
            .update_property(&[gtk::accessible::Property::Label(&caption)]);
        crate::replace_string_list(&self.folder_model, &names);
        if !changed {
            if let Some(selected) = selected {
                crate::select_string(&self.folder_model, &self.folder_picker, &selected);
            }
        }
        let available = !names.is_empty();
        self.folder_picker.set_sensitive(available);
        self.open_folder.set_sensitive(available);
        self.link_folder.set_sensitive(available);
        self.delete_folder
            .set_sensitive(available && artwork_available);
        self.delete.set_sensitive(
            layers.active() != sdeck_plus::layers::DEFAULT_LAYER && artwork_available,
        );
    }
}
