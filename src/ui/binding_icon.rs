//! Binding vocabulary icons as GTK widgets: a themed drawing area and a
//! string dropdown whose rows carry one icon each.
use gtk::{glib, prelude::*, Box as GtkBox, Image, Label, Orientation};
use sdeck_plus::binding_icons::{self, BindingIcon};
use sdeck_plus::local_artwork::ArtworkTarget;
use std::rc::Rc;

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

/// Dial icons sit on the device preview, so they use the hardware accent of
/// the icon's family (as keys and touch sections do); every other icon uses
/// the theme foreground.
pub(crate) fn set_binding_icon(area: &gtk::DrawingArea, icon: BindingIcon) {
    area.set_widget_name(icon.id());
    area.set_draw_func(move |area, cairo, width, height| {
        // GTK 4.8 has no Widget::color(); the style context gives the same ink.
        #[allow(deprecated)]
        let color = area.style_context().color();
        let ink = if area.has_css_class("dial-icon") {
            icon.accent()
        } else {
            (
                f64::from(color.red()),
                f64::from(color.green()),
                f64::from(color.blue()),
                f64::from(color.alpha()),
            )
        };
        let size = f64::from(width.min(height));
        let center = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        if let Err(error) = binding_icons::paint(cairo, icon, center, size, ink) {
            eprintln!("Could not draw binding icon: {error}");
        }
    });
    area.queue_draw();
}

/// An icon for each row position of a dropdown.
pub(crate) type RowIcon = Rc<dyn Fn(u32) -> BindingIcon>;

/// Show `icon_for(position)` beside each row of a string dropdown, in the
/// closed button and in its menu. Menu rows keep GTK's check mark on the
/// selected entry; names and accessibility come from the row's label.
pub(crate) fn icon_dropdown(
    picker: &gtk::DropDown,
    icon_for: impl Fn(u32) -> BindingIcon + 'static,
) {
    let icon_for: RowIcon = Rc::new(icon_for);
    icon_dropdown_with(picker, icon_for.clone(), icon_for);
}

/// Like [`icon_dropdown`], with a separate icon for the closed button.
pub(crate) fn icon_dropdown_with(picker: &gtk::DropDown, closed: RowIcon, menu: RowIcon) {
    picker.set_factory(Some(&icon_row_factory(picker, closed, false)));
    picker.set_list_factory(Some(&icon_row_factory(picker, menu, true)));
}

/// Redraw the closed button's icon, for example when what the selected row
/// stands for changes without a new selection.
pub(crate) fn set_closed_icon(picker: &gtk::DropDown, icon: BindingIcon) {
    let Some(button) = picker.first_child() else {
        return;
    };
    let mut pending = vec![button];
    while let Some(widget) = pending.pop() {
        if let Some(area) = widget.downcast_ref::<gtk::DrawingArea>() {
            if area.has_css_class("binding-icon") {
                set_binding_icon(area, icon);
                return;
            }
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            pending.push(next);
        }
    }
}

fn icon_row_factory(
    picker: &gtk::DropDown,
    icon_for: RowIcon,
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

/// The assigned action of an artwork target: its icon and summary.
pub(crate) type TargetAction = Rc<dyn Fn(ArtworkTarget) -> Option<(BindingIcon, String)>>;

/// A target marker (mini map of keys and strip with this control filled),
/// drawn in the theme foreground like [`binding_icon`].
fn target_marker(target: ArtworkTarget) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(26)
        .content_height(22)
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    area.add_css_class("target-marker");
    set_target_marker(&area, target);
    area
}

fn set_target_marker(area: &gtk::DrawingArea, target: ArtworkTarget) {
    area.set_widget_name(&binding_icons::target_marker_id(target));
    area.set_draw_func(move |area, cairo, width, height| {
        #[allow(deprecated)]
        let color = area.style_context().color();
        let ink = (
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            f64::from(color.alpha()),
        );
        // The map spans about 23x20 grid units: fit both directions.
        let size = (f64::from(width) * 24.0 / 23.0).min(f64::from(height) * 24.0 / 20.0);
        let center = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        if let Err(error) = binding_icons::paint_target(cairo, target, center, size, ink) {
            eprintln!("Could not draw artwork target marker: {error}");
        }
    });
    area.queue_draw();
}

/// Artwork targets with a marker before each name; menu rows also show the
/// target's assigned action icon after the name, read again whenever the
/// menu opens so saved bindings appear without rebuilding the list.
pub(crate) fn target_dropdown(picker: &gtk::DropDown, action: TargetAction) {
    for menu in [false, true] {
        let factory = gtk::SignalListItemFactory::new();
        let setup_action = action.clone();
        factory.connect_setup(glib::clone!(
            #[weak]
            picker,
            move |_, object| {
                let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let row = GtkBox::new(Orientation::Horizontal, 8);
                row.add_css_class("icon-row");
                row.append(&target_marker(ArtworkTarget::Keypad(1)));
                let label = Label::new(None);
                label.set_xalign(0.0);
                row.append(&label);
                if menu {
                    label.set_hexpand(true);
                    let assigned = binding_icon(BindingIcon::Generic);
                    assigned.add_css_class("target-action");
                    assigned.set_visible(false);
                    row.append(&assigned);
                    let check = Image::from_icon_name("object-select-symbolic");
                    check.set_opacity(0.0);
                    row.append(&check);
                    let weak_item = item.downgrade();
                    let weak_check = check.downgrade();
                    picker.connect_selected_notify(move |picker| {
                        if let (Some(item), Some(check)) =
                            (weak_item.upgrade(), weak_check.upgrade())
                        {
                            sync_check(&item, &check, picker);
                        }
                    });
                    let weak_item = item.downgrade();
                    let action = setup_action.clone();
                    row.connect_map(move |_| {
                        if let Some(item) = weak_item.upgrade() {
                            sync_target_action(&item, &assigned, &action);
                        }
                    });
                }
                item.set_child(Some(&row));
            }
        ));
        let bind_action = action.clone();
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
                let Some(target) = ArtworkTarget::ALL.get(item.position() as usize).copied() else {
                    return;
                };
                if let Some(marker) = row.first_child().and_downcast::<gtk::DrawingArea>() {
                    set_target_marker(&marker, target);
                }
                let label = row.first_child().and_then(|marker| marker.next_sibling());
                if let Some(label) = label.clone().and_downcast::<Label>() {
                    label.set_text(
                        &item
                            .item()
                            .and_downcast::<gtk::StringObject>()
                            .map(|value| value.string().to_string())
                            .unwrap_or_default(),
                    );
                }
                let assigned = label.and_then(|label| label.next_sibling());
                if let Some(area) = assigned.clone().and_downcast::<gtk::DrawingArea>() {
                    sync_target_action(item, &area, &bind_action);
                }
                if let Some(check) = assigned
                    .and_then(|area| area.next_sibling())
                    .and_downcast::<Image>()
                {
                    sync_check(item, &check, &picker);
                }
            }
        ));
        if menu {
            picker.set_list_factory(Some(&factory));
        } else {
            picker.set_factory(Some(&factory));
        }
    }
}

fn sync_target_action(item: &gtk::ListItem, area: &gtk::DrawingArea, action: &TargetAction) {
    let assigned = ArtworkTarget::ALL
        .get(item.position() as usize)
        .and_then(|target| action(*target));
    match assigned {
        Some((icon, summary)) => {
            set_binding_icon(area, icon);
            area.set_tooltip_text(Some(&summary));
            area.set_visible(true);
        }
        None => area.set_visible(false),
    }
}
