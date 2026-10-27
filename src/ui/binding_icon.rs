//! Binding vocabulary icons as GTK widgets: a themed drawing area and a
//! string dropdown whose rows carry one icon each.
use gtk::{glib, prelude::*, Box as GtkBox, Image, Label, Orientation};
use sdeck_plus::binding_icons::{self, BindingIcon};

/// A binding vocabulary icon drawn in the widget's own foreground color, so
/// it follows the light and dark palettes. It is decorative: the adjacent
/// label carries the accessible name.
pub(crate) fn binding_icon(icon: BindingIcon) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(16)
        .content_height(16)
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    area.add_css_class("binding-icon");
    set_binding_icon(&area, icon);
    area
}

pub(crate) fn set_binding_icon(area: &gtk::DrawingArea, icon: BindingIcon) {
    area.set_widget_name(icon.id());
    area.set_draw_func(move |area, cairo, width, height| {
        // GTK 4.8 has no Widget::color(); the style context gives the same ink.
        #[allow(deprecated)]
        let color = area.style_context().color();
        let ink = (
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            f64::from(color.alpha()),
        );
        let size = f64::from(width.min(height));
        let center = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        if let Err(error) = binding_icons::paint(cairo, icon, center, size, ink) {
            eprintln!("Could not draw binding icon: {error}");
        }
    });
    area.queue_draw();
}

/// Show `icon_for(position)` beside each row of a string dropdown, in the
/// closed button and in its menu. Menu rows keep GTK's check mark on the
/// selected entry; names and accessibility come from the row's label.
pub(crate) fn icon_dropdown(picker: &gtk::DropDown, icon_for: fn(u32) -> BindingIcon) {
    picker.set_factory(Some(&icon_row_factory(picker, icon_for, false)));
    picker.set_list_factory(Some(&icon_row_factory(picker, icon_for, true)));
}

fn icon_row_factory(
    picker: &gtk::DropDown,
    icon_for: fn(u32) -> BindingIcon,
    menu: bool,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(glib::clone!(
        #[weak]
        picker,
        move |_, object| {
            let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let row = GtkBox::new(Orientation::Horizontal, 8);
            row.add_css_class("icon-row");
            row.append(&binding_icon(BindingIcon::Generic));
            let label = Label::new(None);
            label.set_xalign(0.0);
            row.append(&label);
            if menu {
                let check = Image::from_icon_name("object-select-symbolic");
                check.set_opacity(0.0);
                row.append(&check);
                let item = item.downgrade();
                let check = check.downgrade();
                picker.connect_selected_notify(move |picker| {
                    if let (Some(item), Some(check)) = (item.upgrade(), check.upgrade()) {
                        sync_check(&item, &check, picker);
                    }
                });
            }
            item.set_child(Some(&row));
        }
    ));
    factory.connect_bind(glib::clone!(
        #[weak]
        picker,
        move |_, object| {
            let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(row) = item.child() else {
                return;
            };
            let text = item
                .item()
                .and_downcast::<gtk::StringObject>()
                .map(|value| value.string().to_string())
                .unwrap_or_default();
            if let Some(icon) = row.first_child().and_downcast::<gtk::DrawingArea>() {
                set_binding_icon(&icon, icon_for(item.position()));
            }
            let label = row.first_child().and_then(|icon| icon.next_sibling());
            if let Some(label) = label.clone().and_downcast::<Label>() {
                label.set_text(&text);
            }
            if let Some(check) = label
                .and_then(|label| label.next_sibling())
                .and_downcast::<Image>()
            {
                sync_check(item, &check, &picker);
            }
        }
    ));
    factory
}

fn sync_check(item: &gtk::ListItem, check: &Image, picker: &gtk::DropDown) {
    let selected = item.item().is_some() && item.position() == picker.selected();
    check.set_opacity(if selected { 1.0 } else { 0.0 });
}

/// A compact badge for one strip gesture: the gesture glyph, then its
/// action's icon. Default behavior (unbound swipe paging) is dimmed.
pub(crate) fn gesture_badge(
    gesture: BindingIcon,
    action: BindingIcon,
    description: &str,
    default: bool,
) -> GtkBox {
    let badge = GtkBox::new(Orientation::Horizontal, 2);
    badge.add_css_class("gesture-badge");
    if default {
        badge.add_css_class("default");
    }
    for icon in [gesture, action] {
        let area = binding_icon(icon);
        area.set_content_width(11);
        area.set_content_height(11);
        badge.append(&area);
    }
    badge.set_tooltip_text(Some(description));
    badge.update_property(&[gtk::accessible::Property::Label(description)]);
    badge
}
