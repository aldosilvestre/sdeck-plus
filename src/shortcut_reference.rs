//! Discoverable, read-only reference for the physical shortcut parser.
use crate::ui;
use gtk::{prelude::*, Box as GtkBox, Dialog, Label, Orientation, ResponseType};
use sdeck_plus::keyboard::{supported_key_aliases, supported_key_names};

pub(crate) fn connect(guidance: &Label) {
    guidance.add_css_class("shortcut-guidance");
    guidance.connect_activate_link(|label, uri| {
        if uri != "shortcut-reference" {
            return gtk::glib::Propagation::Proceed;
        }
        if let Some(parent) = label
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        {
            show(&parent);
        }
        gtk::glib::Propagation::Stop
    });
}

fn show(parent: &gtk::Window) {
    let dialog = Dialog::builder()
        .title("Shortcut key reference")
        .transient_for(parent)
        .modal(true)
        .default_width(560)
        .default_height(420)
        .build();
    let area = dialog.content_area();
    area.set_margin_top(16);
    area.set_margin_bottom(16);
    area.set_margin_start(16);
    area.set_margin_end(16);
    area.set_spacing(16);
    area.append(&ui::helper(
        "Names are case-insensitive. Enter one key or a chord such as CTRL + mute_mic. Modifier checkboxes add CTRL, SHIFT, ALT or SUPER.",
    ));
    let list = GtkBox::new(Orientation::Vertical, 16);
    list.append(&ui::section_heading(
        "input-keyboard-symbolic",
        "Key aliases",
    ));
    // One table: the canonical key, then every alias that resolves to it.
    let table = GtkBox::new(Orientation::Vertical, 0);
    table.add_css_class("reference-table");
    let key_column = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
    let header = GtkBox::new(Orientation::Horizontal, 0);
    header.add_css_class("reference-head");
    header.append(&cell("Key", "reference-key", &key_column, false));
    header.append(&cell(
        "Also accepted as",
        "reference-aliases",
        &gtk::SizeGroup::new(gtk::SizeGroupMode::None),
        false,
    ));
    table.append(&header);
    for canonical in supported_key_names() {
        let aliases = supported_key_aliases()
            .filter(|(_, target)| *target == canonical)
            .map(|(alias, _)| alias)
            .collect::<Vec<_>>();
        if aliases.is_empty() {
            continue;
        }
        let row = GtkBox::new(Orientation::Horizontal, 0);
        row.add_css_class("reference-row");
        row.append(&cell(canonical, "reference-key", &key_column, true));
        row.append(&cell(
            &aliases.join(", "),
            "reference-aliases",
            &gtk::SizeGroup::new(gtk::SizeGroupMode::None),
            true,
        ));
        table.append(&row);
    }
    list.append(&table);
    list.append(&ui::section_heading(
        "input-keyboard-symbolic",
        "Physical key names",
    ));
    let names = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(6)
        .row_spacing(6)
        .min_children_per_line(1)
        .max_children_per_line(12)
        .homogeneous(false)
        .build();
    for name in supported_key_names() {
        let chip = Label::new(Some(name));
        chip.add_css_class("reference-chip");
        chip.set_selectable(true);
        let item = gtk::FlowBoxChild::new();
        item.set_focusable(false);
        item.set_child(Some(&chip));
        names.insert(&item, -1);
    }
    list.append(&names);
    let modifiers = Label::new(Some(
        "Modifiers: CTRL / Control; SHIFT; ALT; SUPER / Win / Windows / Meta.",
    ));
    modifiers.set_xalign(0.0);
    modifiers.set_wrap(true);
    modifiers.set_selectable(true);
    list.append(&modifiers);
    list.append(&ui::helper(
        "PAUSE, NEXT and STOP keep their keyboard meanings. For media, use media_pause, next_track and media_stop. Desktop bindings determine the effect of each physical key.",
    ));
    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Always)
        .overlay_scrolling(false)
        .build();
    area.append(&scroller);
    let close = dialog.add_button("Close", ResponseType::Close);
    close.set_margin_end(16);
    close.set_margin_bottom(16);
    dialog.set_default_response(ResponseType::Close);
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
}

/// One table cell: a left-aligned, selectable label sharing its column width.
fn cell(text: &str, class: &str, column: &gtk::SizeGroup, selectable: bool) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class(class);
    label.set_xalign(0.0);
    label.set_valign(gtk::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(selectable);
    if class == "reference-aliases" {
        label.set_hexpand(true);
        label.set_max_width_chars(44);
    }
    column.add_widget(&label);
    label
}
