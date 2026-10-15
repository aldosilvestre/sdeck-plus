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
    for canonical in supported_key_names() {
        let aliases = supported_key_aliases()
            .filter(|(_, target)| *target == canonical)
            .map(|(alias, _)| alias)
            .collect::<Vec<_>>();
        if aliases.is_empty() {
            continue;
        }
        let row = GtkBox::new(Orientation::Vertical, 4);
        let name = Label::new(Some(canonical));
        name.add_css_class("section-label");
        name.set_xalign(0.0);
        name.set_selectable(true);
        row.append(&name);
        let values = Label::new(Some(&aliases.join(", ")));
        values.set_xalign(0.0);
        values.set_wrap(true);
        values.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        values.set_max_width_chars(48);
        values.set_selectable(true);
        row.append(&values);
        list.append(&row);
    }
    list.append(&ui::section_heading(
        "input-keyboard-symbolic",
        "Physical key names",
    ));
    let names = Label::new(Some(&supported_key_names().collect::<Vec<_>>().join(", ")));
    names.set_xalign(0.0);
    names.set_wrap(true);
    names.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    names.set_max_width_chars(48);
    names.set_selectable(true);
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
