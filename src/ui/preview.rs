//! The hardware preview: widgets built from a model's [`PreviewLayout`].
//!
//! The view owns the chassis subtree and nothing else of the window. A model
//! change drops that subtree and builds the next one in the same card, so the
//! rest of the window (and every widget it owns) lives on.
//!
//! Ownership rules that keep a rebuild from leaking:
//! - widgets of the subtree are held only by [`Built`], by their parents and
//!   by GTK; handlers on them capture a [`Weak`] view, never a strong one, and
//!   never the widgets themselves;
//! - handlers connected to widgets that outlive the subtree (the summary
//!   captions) are recorded in [`Built`] and disconnected when it is dropped;
//! - the widgets every model shares (live status, gesture badges, dial icons
//!   and captions) are unparented before the old subtree goes and placed again.
use crate::*;
use sdeck_plus::{
    device_model::DeviceModel,
    preview_layout::{ControlRef, PreviewLayout, Rect},
};
use std::ops::Deref;

/// The largest count of a supported model, for widgets created once.
pub(crate) fn widest(count: impl Fn(&DeviceModel) -> u8) -> usize {
    DeviceModel::supported()
        .map(|model| usize::from(count(model)))
        .max()
        .unwrap_or_default()
}

/// Persistent widgets created for the widest supported model, of which the
/// first `shown` belong to the model on screen. Dereferences to those.
#[derive(Clone)]
pub(crate) struct ModelSlots<T> {
    items: Rc<Vec<T>>,
    shown: Rc<Cell<usize>>,
}

impl<T> ModelSlots<T> {
    pub(crate) fn new(items: Vec<T>, shown: usize) -> Self {
        let shown = shown.min(items.len());
        Self {
            items: Rc::new(items),
            shown: Rc::new(Cell::new(shown)),
        }
    }

    pub(crate) fn set_shown(&self, shown: usize) {
        self.shown.set(shown.min(self.items.len()));
    }

    /// Every slot, shown or not.
    pub(crate) fn all(&self) -> &[T] {
        &self.items
    }
}

impl<T> Deref for ModelSlots<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.items[..self.shown.get()]
    }
}

impl<'a, T> IntoIterator for &'a ModelSlots<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// The preview control behind an editor selection (`number` 0 is the strip).
pub(crate) fn control_of(kind: ControlKind, number: u8) -> Option<ControlRef> {
    match (kind, number) {
        (ControlKind::Button, number) => Some(ControlRef::Key(number)),
        (ControlKind::Encoder, number) => Some(ControlRef::Dial(number)),
        (ControlKind::Touch, 0) => Some(ControlRef::Strip),
        (ControlKind::Touch, number) => Some(ControlRef::Zone(number)),
    }
}

/// The editor's control kind and number for a preview control.
pub(crate) fn kind_and_number(control: ControlRef) -> (ControlKind, u8) {
    match control {
        ControlRef::Key(number) => (ControlKind::Button, number),
        ControlRef::Dial(number) => (ControlKind::Encoder, number),
        ControlRef::Strip => (ControlKind::Touch, 0),
        ControlRef::Zone(number) => (ControlKind::Touch, number),
    }
}

/// The status line's text before any input arrives.
pub(crate) const TOUCH_PLACEHOLDER: &str = "Touch strip · waiting for input";

/// [`TOUCH_PLACEHOLDER`] for a touch strip; a display that takes no touch has
/// nothing to wait for.
fn placeholder(model: &sdeck_plus::device_model::DeviceModel) -> &'static str {
    match model.strip {
        Some(strip) if !strip.touch => "Display",
        _ => TOUCH_PLACEHOLDER,
    }
}

/// One model's chassis subtree.
struct Built {
    chassis: gtk::Fixed,
    /// Every selectable control, in the order the widgets were created.
    controls: Vec<(ControlRef, Button)>,
    key_labels: Vec<Label>,
    dial_labels: Vec<Label>,
    /// Handlers on captions that outlive this subtree.
    handlers: Vec<(glib::WeakRef<Label>, glib::SignalHandlerId)>,
}

impl Drop for Built {
    fn drop(&mut self) {
        for (label, handler) in self.handlers.drain(..) {
            if let Some(label) = label.upgrade() {
                label.disconnect(handler);
            }
        }
    }
}

pub(crate) struct PreviewView {
    /// The card the chassis sits in; its size follows the chassis.
    card: GtkBox,
    bin: scaled_bin::ScaledBin,
    touch_status: Label,
    ui: BindingUi,
    layers: Rc<RefCell<LayerStore>>,
    artwork: ArtworkUi,
    /// The Appearance picker; clicking a key or section points it at that target.
    picker: gtk::DropDown,
    heading: Label,
    built: RefCell<Option<Built>>,
    selected: Cell<ControlRef>,
}

impl PreviewView {
    /// A view of the model `layers` holds. `heading` names the control being
    /// configured and is the editor's signal that the selection moved.
    pub(crate) fn new(
        ui: &BindingUi,
        layers: &Rc<RefCell<LayerStore>>,
        artwork: &ArtworkUi,
        picker: &gtk::DropDown,
        heading: &Label,
    ) -> Rc<Self> {
        let card = card();
        card.add_css_class("device-canvas");
        // The fixed geometry of the chassis keeps loaded artwork and long live
        // messages from resizing the hardware; the ScaledBin only ever shows it
        // at its natural size.
        let bin = scaled_bin::ScaledBin::new(&card, PREVIEW_MAX_SCALE);
        bin.set_hexpand(true);
        bin.set_vexpand(false);
        bin.add_css_class("device-preview");
        let touch_status = Label::new(Some(TOUCH_PLACEHOLDER));
        touch_status.add_css_class("touch-display");
        touch_status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        touch_status.set_max_width_chars(32);
        touch_status.set_hexpand(true);
        touch_status.set_size_request(-1, 16);
        let model = layers.borrow().model();
        touch_status.set_text(placeholder(model));
        ui.set_model(model);
        let view = Rc::new(Self {
            card,
            bin,
            touch_status,
            ui: ui.clone(),
            layers: layers.clone(),
            artwork: artwork.clone(),
            picker: picker.clone(),
            heading: heading.clone(),
            built: RefCell::new(None),
            selected: Cell::new(ControlRef::Key(1)),
        });
        view.install(model);
        // Catalogs and presets move the editor's selection themselves; the
        // heading they all update tells the preview where it went.
        let weak = Rc::downgrade(&view);
        heading.connect_label_notify(move |_| {
            if let Some(view) = weak.upgrade() {
                view.follow_editor();
            }
        });
        view
    }

    /// The widget to place in the window.
    pub(crate) fn widget(&self) -> &scaled_bin::ScaledBin {
        &self.bin
    }

    pub(crate) fn touch_status(&self) -> &Label {
        &self.touch_status
    }

    pub(crate) fn model(&self) -> &'static DeviceModel {
        self.ui.model.get()
    }

    pub(crate) fn layout(&self) -> PreviewLayout {
        PreviewLayout::for_model(self.model())
    }

    #[cfg(test)]
    pub(crate) fn selected(&self) -> ControlRef {
        self.selected.get()
    }

    pub(crate) fn control(&self, control: ControlRef) -> Option<gtk::Widget> {
        self.built
            .borrow()
            .as_ref()?
            .controls
            .iter()
            .find(|(shown, _)| *shown == control)
            .map(|(_, button)| button.clone().upcast())
    }

    fn widgets_where(&self, wanted: impl Fn(ControlRef) -> bool) -> Vec<gtk::Widget> {
        self.built.borrow().as_ref().map_or_else(Vec::new, |built| {
            built
                .controls
                .iter()
                .filter(|(control, _)| wanted(*control))
                .map(|(_, button)| button.clone().upcast())
                .collect()
        })
    }

    pub(crate) fn key_widgets(&self) -> Vec<gtk::Widget> {
        self.widgets_where(|control| matches!(control, ControlRef::Key(_)))
    }

    #[cfg(test)]
    pub(crate) fn dial_widgets(&self) -> Vec<gtk::Widget> {
        self.widgets_where(|control| matches!(control, ControlRef::Dial(_)))
    }

    pub(crate) fn zone_widgets(&self) -> Vec<gtk::Widget> {
        self.widgets_where(|control| matches!(control, ControlRef::Zone(_)))
    }

    /// The live-state caption of key `index` (zero-based).
    pub(crate) fn key_label(&self, index: usize) -> Option<Label> {
        self.built.borrow().as_ref()?.key_labels.get(index).cloned()
    }

    pub(crate) fn dial_label(&self, index: usize) -> Option<Label> {
        self.built
            .borrow()
            .as_ref()?
            .dial_labels
            .get(index)
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn key_labels(&self) -> Vec<Label> {
        self.built
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |built| built.key_labels.clone())
    }

    #[cfg(test)]
    pub(crate) fn dial_labels(&self) -> Vec<Label> {
        self.built
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |built| built.dial_labels.clone())
    }

    /// Every key, the strip, every dial and every section of the shown model,
    /// in the order the action catalog and the dial presets address them.
    /// `None` for a model without a strip or dials, which they do not serve.
    pub(crate) fn catalog_widgets(&self) -> Option<Rc<Vec<gtk::Widget>>> {
        let layout = self.layout();
        if layout.key_count() == 0
            || !layout.has(ControlRef::Strip)
            || layout.dial_count() == 0
            || layout.zone_count() == 0
        {
            return None;
        }
        let number = |count: usize| 1..=count as u8;
        let controls = number(layout.key_count())
            .map(ControlRef::Key)
            .chain([ControlRef::Strip])
            .chain(number(layout.dial_count()).map(ControlRef::Dial))
            .chain(number(layout.zone_count()).map(ControlRef::Zone))
            .map(|control| self.control(control))
            .collect::<Option<Vec<_>>>()?;
        Some(Rc::new(controls))
    }

    fn install(self: &Rc<Self>, model: &'static DeviceModel) {
        let built = self.build(PreviewLayout::for_model(model));
        self.card.append(&built.chassis);
        *self.built.borrow_mut() = Some(built);
        let selected = self.selected.get();
        self.set_selected(if self.layout().has(selected) {
            selected
        } else {
            ControlRef::Key(1)
        });
    }

    /// Highlight `control`, and only it.
    fn set_selected(&self, control: ControlRef) {
        self.selected.set(control);
        if let Some(built) = self.built.borrow().as_ref() {
            for (shown, button) in &built.controls {
                if *shown == control {
                    button.add_css_class("control-selected");
                } else {
                    button.remove_css_class("control-selected");
                }
            }
        }
    }

    fn follow_editor(&self) {
        if let Some(control) = self.ui.selected_control() {
            if self.layout().has(control) {
                self.set_selected(control);
            }
        }
    }

    /// Select `control` for editing, asking first when edits are unsaved.
    fn request_select(self: &Rc<Self>, control: ControlRef, anchor: &Button) {
        let weak = Rc::downgrade(self);
        let switch: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(view) = weak.upgrade() {
                view.select(control);
            }
        });
        if self.ui.is_dirty() {
            confirm_discard(anchor, switch);
        } else {
            switch();
        }
    }

    fn select(&self, control: ControlRef) {
        let (kind, number) = kind_and_number(control);
        if let Err(error) = self.ui.select_control(kind, Some(number), false) {
            self.ui.status.set_text(&error);
            return;
        }
        self.set_selected(control);
        self.heading.set_text(&control_heading(kind, Some(number)));
        self.ui.load_selected_binding(&self.layers);
    }

    fn build(self: &Rc<Self>, layout: PreviewLayout) -> Built {
        let chassis = gtk::Fixed::new();
        chassis.set_halign(gtk::Align::Center);
        chassis.set_valign(gtk::Align::Center);
        chassis.add_css_class("hardware-chassis");
        chassis.set_size_request(layout.size.0, layout.size.1);
        let mut built = Built {
            chassis: chassis.clone(),
            controls: Vec::new(),
            key_labels: Vec::new(),
            dial_labels: Vec::new(),
            handlers: Vec::new(),
        };
        let brand = Label::new(Some(&layout.brand));
        brand.add_css_class("hardware-brand");
        brand.set_size_request(layout.brand_rect.w, layout.brand_rect.h);
        chassis.put(
            &brand,
            f64::from(layout.brand_rect.x),
            f64::from(layout.brand_rect.y),
        );

        for (index, rect) in layout.keys.iter().enumerate() {
            let number = (index + 1) as u8;
            let key = Button::new();
            key.add_css_class("control-selectable");
            key.add_css_class("key-surface");
            key.set_overflow(gtk::Overflow::Hidden);
            if let Some(name) = ControlRef::Key(number).accessible_name() {
                key.update_property(&[gtk::accessible::Property::Label(&name)]);
            }
            key.set_size_request(rect.w, rect.h);
            let picture = artwork_picture();
            picture.set_visible(false);
            let surface = gtk::Overlay::new();
            surface.set_size_request(rect.w, rect.h);
            surface.add_overlay(&picture);
            let label = Label::new(None);
            label.add_css_class("control-tile");
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_max_width_chars(4);
            // Keep live-state bookkeeping without drawing names over key artwork.
            label.set_visible(false);
            surface.add_overlay(&label);
            key.set_child(Some(&surface));
            put(&chassis, &key, rect);
            register_picture(&self.artwork, ArtworkTarget::Keypad(number), &picture);
            connect_artwork_selection(
                &key,
                &self.picker,
                &self.artwork,
                ArtworkTarget::Keypad(number),
            );
            built.key_labels.push(label);
            self.bind(&mut built, ControlRef::Key(number), key);
        }

        if let Some(strip) = &layout.strip {
            let bezel = gtk::Fixed::new();
            bezel.add_css_class("hardware-bezel");
            bezel.set_size_request(strip.bezel.w, strip.bezel.h);
            put(&chassis, &bezel, &strip.bezel);
            // The live status and the strip's gesture badges share the header line.
            let header = GtkBox::new(Orientation::Horizontal, 6);
            header.set_size_request(strip.header.w, strip.header.h);
            header.append(&self.touch_status);
            header.append(&self.ui.gesture_badges);
            put_in(&bezel, &header, &strip.header, &strip.bezel);

            let overlay = gtk::Overlay::new();
            overlay.set_size_request(strip.rect.w, strip.rect.h);
            overlay.add_css_class("strip-surface");
            let background = artwork_picture();
            background.set_visible(false);
            let background_surface = gtk::Overlay::new();
            background_surface.add_overlay(&background);
            register_picture(
                &self.artwork,
                ArtworkTarget::TouchStripBackground,
                &background,
            );
            if strip.touch {
                let button = Button::new();
                button.add_css_class("control-selectable");
                button.add_css_class("strip-surface");
                button.set_child(Some(&background_surface));
                overlay.set_child(Some(&button));
                connect_artwork_selection(
                    &button,
                    &self.picker,
                    &self.artwork,
                    ArtworkTarget::TouchStripBackground,
                );
                let zones = GtkBox::new(Orientation::Horizontal, 0);
                zones.set_homogeneous(true);
                for zone in 1..=strip.zones.len() as u8 {
                    let zone_button = Button::new();
                    zone_button.add_css_class("control-selectable");
                    zone_button.add_css_class("strip-zone");
                    let visual = gtk::Overlay::new();
                    let picture = artwork_picture();
                    picture.set_visible(false);
                    visual.add_overlay(&picture);
                    zone_button.set_child(Some(&visual));
                    zone_button
                        .set_tooltip_text(ControlRef::Zone(zone).accessible_name().as_deref());
                    zone_button.set_hexpand(true);
                    zone_button.set_vexpand(true);
                    register_picture(&self.artwork, ArtworkTarget::EncoderZone(zone), &picture);
                    connect_artwork_selection(
                        &zone_button,
                        &self.picker,
                        &self.artwork,
                        ArtworkTarget::EncoderZone(zone),
                    );
                    zones.append(&zone_button);
                    self.bind(&mut built, ControlRef::Zone(zone), zone_button);
                }
                overlay.add_overlay(&zones);
                // The strip is selected before its sections, as it always was.
                let strip_entry = built.controls.len() - strip.zones.len();
                self.bind_at(&mut built, strip_entry, ControlRef::Strip, button);
            } else {
                // A plain window: drawn, never selected.
                overlay.set_child(Some(&background_surface));
            }
            put_in(&bezel, &overlay, &strip.rect, &strip.bezel);
        }

        if let Some(dials) = &layout.dial_rail {
            let rail = gtk::Fixed::new();
            rail.add_css_class("hardware-rail");
            rail.set_size_request(dials.rail.w, dials.rail.h);
            put(&chassis, &rail, &dials.rail);
            for (index, rect) in dials.dials.iter().enumerate() {
                let number = (index + 1) as u8;
                let knob = Button::new();
                knob.add_css_class("control-selectable");
                knob.add_css_class("encoder");
                if let Some(name) = ControlRef::Dial(number).accessible_name() {
                    knob.update_property(&[gtk::accessible::Property::Label(&name)]);
                }
                knob.set_size_request(rect.w, rect.h);
                let label = Label::new(None);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_max_width_chars(4);
                label.add_css_class("control-tile");
                // Keep dial state bookkeeping without drawing legends on the knobs.
                label.set_visible(false);
                knob.set_child(Some(&label));
                put_in(&rail, &knob, rect, &dials.rail);
                // The assigned action's icon on the knob face (clicks still reach
                // the knob) and its caption centered under it, both preview-only.
                if let Some(icon) = self.ui.dial_icons.all().get(index) {
                    put_in(&rail, icon, &dials.icons[index], &dials.rail);
                }
                if let Some(legend) = self.ui.dial_legends.all().get(index) {
                    legend.set_size_request(dials.legends[index].w, dials.legends[index].h);
                    put_in(&rail, legend, &dials.legends[index], &dials.rail);
                }
                built.dial_labels.push(label);
                self.bind(&mut built, ControlRef::Dial(number), knob);
            }
        }
        built
    }

    fn bind(self: &Rc<Self>, built: &mut Built, control: ControlRef, button: Button) {
        self.bind_at(built, built.controls.len(), control, button);
    }

    /// Register `button` as `control`, at `position` in the creation order.
    fn bind_at(
        self: &Rc<Self>,
        built: &mut Built,
        position: usize,
        control: ControlRef,
        button: Button,
    ) {
        let weak = Rc::downgrade(self);
        button.connect_clicked(move |button| {
            if let Some(view) = weak.upgrade() {
                view.request_select(control, button);
            }
        });
        // Summaries stay available through tooltips, never underneath the front.
        if let Some(summary) = self.ui.summary(control) {
            let tooltip = move |text: &str| {
                match control {
                ControlRef::Key(number) => format!(
                    "Button {number} · {text}\nDrag onto another key to swap saved settings and artwork."
                ),
                ControlRef::Dial(number) => format!("Encoder {number} · {text}"),
                ControlRef::Strip | ControlRef::Zone(_) => text.to_owned(),
            }
            };
            button.set_tooltip_text(Some(&tooltip(&summary.text())));
            let weak = button.downgrade();
            let handler = summary.connect_label_notify(move |label| {
                if let Some(button) = weak.upgrade() {
                    button.set_tooltip_text(Some(&tooltip(&label.text())));
                }
            });
            built.handlers.push((summary.downgrade(), handler));
        }
        built.controls.insert(position, (control, button));
    }
}

fn put(parent: &gtk::Fixed, child: &impl IsA<gtk::Widget>, rect: &Rect) {
    parent.put(child, f64::from(rect.x), f64::from(rect.y));
}

/// Place `child` at its chassis `rect` inside `parent`, which sits at `origin`.
fn put_in(parent: &gtk::Fixed, child: &impl IsA<gtk::Widget>, rect: &Rect, origin: &Rect) {
    let rect = rect.relative_to(origin);
    parent.put(child, f64::from(rect.x), f64::from(rect.y));
}

#[cfg(test)]
mod placeholder_tests {
    use super::*;
    use sdeck_plus::device_model::{NEO, PLUS, PLUS_XL};

    #[test]
    fn touch_strips_keep_their_caption_and_a_display_without_touch_is_neutral() {
        assert_eq!(placeholder(&PLUS), "Touch strip · waiting for input");
        assert_eq!(placeholder(&PLUS_XL), "Touch strip · waiting for input");
        assert_eq!(placeholder(&NEO), "Display");
    }
}
