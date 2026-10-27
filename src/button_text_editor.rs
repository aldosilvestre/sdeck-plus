//! Native keypad text controls sharing the hardware image compositor.

use super::{labeled, ui, ArtworkUi};
use gtk::{glib, prelude::*, Box as GtkBox, Label, Orientation, Picture};
use sdeck_plus::{
    button_text::{ButtonText, HorizontalAlignment, TextOrientation, VerticalAlignment},
    local_artwork::ArtworkTarget,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(super) struct ButtonTextEditor {
    pub(super) root: GtkBox,
    pub(super) open: gtk::MenuButton,
    pub(super) panel: gtk::Popover,
    pub(super) input: gtk::TextView,
    pub(super) font: gtk::FontButton,
    pub(super) size: gtk::SpinButton,
    pub(super) orientation: [gtk::ToggleButton; 2],
    /// Text position on the key: row-major top-left .. bottom-right.
    pub(super) position: [gtk::ToggleButton; 9],
    pub(super) save: gtk::Button,
    pub(super) remove: gtk::Button,
    pub(super) preview: Picture,
    /// Warns when the text overflows the key at the chosen size/orientation.
    pub(super) overflow: Label,
    /// Names the edited key and page, e.g. "Key 3 on “Everyday / Page 1”".
    pub(super) context_line: Label,
    /// Shown while the form differs from the key's saved text.
    pub(super) unsaved: Label,
    /// Ctrl+Enter saves from anywhere in the popup; plain Enter stays a new line.
    pub(super) save_shortcut: gtk::Shortcut,
    status: Label,
    loading: Cell<bool>,
    control_available: Cell<bool>,
    context: RefCell<Option<(String, ArtworkTarget)>>,
    /// The key's saved text, `None` when it has none.
    saved: RefCell<Option<ButtonText>>,
    /// The page indicator whose tooltip carries the current view's name.
    page_label: RefCell<Option<glib::WeakRef<Label>>>,
    artwork: ArtworkUi,
}

impl ButtonTextEditor {
    pub(super) fn new(artwork: &ArtworkUi) -> Rc<Self> {
        let content = GtkBox::new(Orientation::Vertical, 8);
        let context_line = Label::new(None);
        context_line.add_css_class("muted");
        context_line.add_css_class("button-text-context");
        context_line.set_xalign(0.0);
        context_line.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        context_line.set_max_width_chars(40);
        content.append(&context_line);
        let body = GtkBox::new(Orientation::Horizontal, 16);
        let form = GtkBox::new(Orientation::Vertical, 8);
        form.set_hexpand(true);
        let input = gtk::TextView::new();
        input.set_wrap_mode(gtk::WrapMode::WordChar);
        input.set_left_margin(8);
        input.set_right_margin(8);
        input.set_top_margin(8);
        input.set_bottom_margin(8);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&input)
            .min_content_height(48)
            .max_content_height(48)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        form.append(&labeled("Text (new lines supported)", &scroller));
        let font = gtk::FontButton::new();
        font.set_level(gtk::FontChooserLevel::FAMILY);
        font.set_use_font(false);
        font.set_use_size(false);
        font.set_hexpand(true);
        font.set_title("Choose text font");
        let size = gtk::SpinButton::with_range(6.0, 120.0, 1.0);
        size.set_numeric(true);
        let (orientation_row, orientation) = orientation_group();
        let (position_grid, position) = position_grid();
        // One grid keeps Position aligned under Size and Orientation under Font.
        let fields = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .build();
        fields.attach(&labeled("Font", &font), 0, 0, 1, 1);
        fields.attach(&labeled("Size (px)", &size), 1, 0, 1, 1);
        let orientation_field = labeled("Orientation", &orientation_row);
        orientation_field.set_valign(gtk::Align::Start);
        orientation_row.set_halign(gtk::Align::Start);
        fields.attach(&orientation_field, 0, 1, 1, 1);
        fields.attach(&labeled("Position", &position_grid), 1, 1, 1, 1);
        form.append(&fields);
        body.append(&form);
        let preview_column = GtkBox::new(Orientation::Vertical, 8);
        preview_column.set_valign(gtk::Align::Start);
        let preview = Picture::new();
        preview.set_size_request(120, 120);
        preview.set_halign(gtk::Align::Center);
        preview.set_can_shrink(false);
        preview.update_property(&[gtk::accessible::Property::Label("Button text preview")]);
        preview_column.append(&labeled("Preview", &preview));
        let overflow = Label::new(Some("Text does not fit; try a smaller size"));
        overflow.add_css_class("feedback-warning");
        overflow.add_css_class("button-text-overflow");
        overflow.set_xalign(0.0);
        overflow.set_wrap(true);
        overflow.set_max_width_chars(18);
        overflow.set_visible(false);
        preview_column.append(&overflow);
        // Under the preview, so showing it never widens the popup.
        let unsaved = Label::new(Some("Unsaved changes"));
        unsaved.add_css_class("muted");
        unsaved.add_css_class("button-text-unsaved");
        unsaved.set_xalign(0.0);
        unsaved.set_visible(false);
        preview_column.append(&unsaved);
        let status = Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_max_width_chars(18);
        status.set_lines(3);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        preview_column.append(&status);
        body.append(&preview_column);
        content.append(&body);
        let buttons = GtkBox::new(Orientation::Horizontal, 12);
        let save = ui::button("document-save-symbolic", "Save text");
        save.add_css_class("suggested-action");
        let remove = ui::button("edit-clear-symbolic", "Remove text");
        remove.add_css_class("destructive-action");
        remove.set_has_frame(false);
        buttons.append(&save);
        buttons.append(&remove);
        content.append(&buttons);
        let panel = ui::popover("Button text", &content);
        // The native font dialog must not dismiss and reset this editing form.
        panel.set_autohide(false);
        let save_shortcut = gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>Return|<Control>KP_Enter"),
            None::<gtk::ShortcutAction>,
        );
        let shortcuts = gtk::ShortcutController::new();
        // Capture runs before the text view would insert a new line.
        shortcuts.set_propagation_phase(gtk::PropagationPhase::Capture);
        shortcuts.add_shortcut(save_shortcut.clone());
        panel.add_controller(shortcuts);
        let open = ui::menu_button("format-text-plaintext-symbolic", "Add text", &panel);
        open.set_halign(gtk::Align::Start);
        let root = GtkBox::new(Orientation::Vertical, 8);
        root.append(&open);
        let editor = Rc::new(Self {
            root,
            open,
            panel,
            input,
            font,
            size,
            orientation,
            position,
            save,
            remove,
            preview,
            overflow,
            context_line,
            unsaved,
            save_shortcut,
            status,
            loading: Cell::new(false),
            control_available: Cell::new(true),
            context: RefCell::new(None),
            saved: RefCell::new(None),
            page_label: RefCell::new(None),
            artwork: artwork.clone(),
        });
        let weak = Rc::downgrade(&editor);
        editor
            .save_shortcut
            .set_action(Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(editor) = weak.upgrade() {
                    if editor.can_save() {
                        editor.persist(false);
                    }
                }
                glib::Propagation::Stop
            })));
        let weak = Rc::downgrade(&editor);
        editor.input.buffer().connect_changed(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.changed();
            }
        });
        let weak = Rc::downgrade(&editor);
        editor.size.connect_value_changed(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.changed();
            }
        });
        let weak = Rc::downgrade(&editor);
        editor.font.connect_font_desc_notify(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.changed();
            }
        });
        for button in editor.orientation.iter().chain(editor.position.iter()) {
            let weak = Rc::downgrade(&editor);
            button.connect_toggled(move |button| {
                if button.is_active() {
                    if let Some(editor) = weak.upgrade() {
                        editor.changed();
                    }
                }
            });
        }
        let weak = Rc::downgrade(&editor);
        editor.save.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.persist(false);
            }
        });
        let weak = Rc::downgrade(&editor);
        editor.remove.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.persist(true);
            }
        });
        let weak = Rc::downgrade(&editor);
        editor.open.set_create_popup_func(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.load_context(true);
            }
        });
        let weak = Rc::downgrade(&editor);
        editor.panel.connect_show(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.input.grab_focus();
            }
        });
        let weak = Rc::downgrade(&editor);
        editor.panel.connect_closed(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.load_context(true);
            }
        });
        editor.load_context(false);
        editor
    }

    pub(super) fn load_context(&self, force: bool) {
        let layer = self.artwork.layers.borrow().active().to_owned();
        let target = self.artwork.selected_target.get();
        let context = (layer.clone(), target);
        if !force && self.context.borrow().as_ref() == Some(&context) {
            return;
        }
        if !force {
            self.panel.popdown();
        }
        // Discard a draft preview when moving to another key, without sending it.
        if let Some((previous_layer, previous_target)) = self.context.borrow().as_ref() {
            if previous_layer == &layer {
                self.artwork.restore_button_preview(previous_target);
            }
        }
        *self.context.borrow_mut() = Some(context);
        self.root.set_visible(
            self.control_available.get() && matches!(target, ArtworkTarget::Keypad(_)),
        );
        if !matches!(target, ArtworkTarget::Keypad(_)) {
            return;
        }
        let saved = self
            .artwork
            .store
            .borrow()
            .as_ref()
            .and_then(|store| store.button_text_in_layer(&layer, target).cloned())
            .filter(|value| !value.text.is_empty());
        let value = saved.clone().unwrap_or_default();
        *self.saved.borrow_mut() = saved;
        self.refresh_context_line();
        let caption = if value.text.is_empty() {
            "Add text"
        } else {
            "Edit text"
        };
        self.open.set_child(Some(&ui::icon_caption(
            "format-text-plaintext-symbolic",
            caption,
        )));
        self.open.set_tooltip_text(Some(caption));
        self.open
            .update_property(&[gtk::accessible::Property::Label(caption)]);
        self.loading.set(true);
        self.input.buffer().set_text(&value.text);
        let mut font = gtk::pango::FontDescription::new();
        font.set_family(&value.font_family);
        self.font.set_font_desc(&font);
        self.size.set_value(f64::from(value.font_size));
        self.orientation[match value.orientation {
            TextOrientation::Horizontal => 0,
            TextOrientation::Vertical => 1,
        }]
        .set_active(true);
        self.position[position_index(value.horizontal_alignment, value.vertical_alignment)]
            .set_active(true);
        self.loading.set(false);
        let available = self.artwork.store.borrow().is_some();
        self.root.set_sensitive(available);
        self.status.set_text("");
        if let Err(error) = self.render_preview(&value) {
            self.feedback(&error, true);
        }
        self.update_state();
    }

    /// Follow the page indicator's tooltip, which names the current view.
    pub(super) fn follow_page_label(self: &Rc<Self>, indicator: &Label) {
        *self.page_label.borrow_mut() = Some(indicator.downgrade());
        let weak = Rc::downgrade(self);
        indicator.connect_tooltip_text_notify(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.refresh_context_line();
            }
        });
        self.refresh_context_line();
    }

    fn refresh_context_line(&self) {
        let ArtworkTarget::Keypad(key) = self.artwork.selected_target.get() else {
            return;
        };
        let page = self
            .page_label
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
            .and_then(|label| label.tooltip_text())
            .map(|name| name.to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.artwork.layers.borrow().active().to_owned());
        let line = format!("Key {key} on “{page}”");
        self.context_line.set_text(&line);
        self.context_line.set_tooltip_text(Some(&line));
    }

    /// The form value when it is valid, nonblank and differs from the saved text.
    fn savable(&self) -> Result<ButtonText, &'static str> {
        let value = self.value().map_err(|_| "Complete the form to save")?;
        if value.text.trim().is_empty() || value.validate().is_err() {
            return Err("Type some text to save");
        }
        if Some(&value) == self.saved.borrow().as_ref() {
            return Err("No changes to save");
        }
        Ok(value)
    }

    fn can_save(&self) -> bool {
        self.savable().is_ok()
    }

    fn is_dirty(&self) -> bool {
        let baseline = self.saved.borrow().clone().unwrap_or_default();
        self.value().map_or(true, |value| value != baseline)
    }

    /// Save, Remove and the unsaved hint follow the form against the saved text.
    fn update_state(&self) {
        let savable = self.savable();
        self.save.set_sensitive(savable.is_ok());
        self.save.set_tooltip_text(Some(match savable {
            Ok(_) => "Save text (Ctrl+Enter)",
            Err(reason) => reason,
        }));
        self.remove.set_visible(self.saved.borrow().is_some());
        self.unsaved.set_visible(self.is_dirty());
    }

    pub(super) fn set_control_available(&self, available: bool) {
        self.control_available.set(available);
        self.root.set_visible(
            available && matches!(self.artwork.selected_target.get(), ArtworkTarget::Keypad(_)),
        );
        if !available {
            self.panel.popdown();
        }
    }

    pub(super) fn value(&self) -> Result<ButtonText, String> {
        let buffer = self.input.buffer();
        let (horizontal_alignment, vertical_alignment) = self
            .position
            .iter()
            .position(|button| button.is_active())
            .map(position_alignment)
            .ok_or("Choose a text position")?;
        Ok(ButtonText {
            text: buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), true)
                .to_string(),
            font_family: self
                .font
                .font_desc()
                .and_then(|font| font.family())
                .ok_or("Choose a font")?
                .to_string(),
            font_size: u16::try_from(self.size.value_as_int()).map_err(|e| e.to_string())?,
            orientation: match self
                .orientation
                .iter()
                .position(|button| button.is_active())
            {
                Some(0) => TextOrientation::Horizontal,
                Some(1) => TextOrientation::Vertical,
                _ => return Err("Choose a text orientation".into()),
            },
            horizontal_alignment,
            vertical_alignment,
        })
    }

    fn feedback(&self, message: &str, failed: bool) {
        self.status.set_text(message);
        self.status.set_tooltip_text(Some(message));
        ui::feedback(&self.status, failed);
    }

    fn changed(&self) {
        if self.loading.get() {
            return;
        }
        match self.value().and_then(|value| self.render_preview(&value)) {
            Ok(()) => {
                self.status.set_text("");
                self.status.set_tooltip_text(None);
                ui::clear_feedback(&self.status);
            }
            Err(error) => self.feedback(&error, true),
        }
        self.update_state();
    }

    pub(super) fn refresh_preview(&self) {
        if let Err(error) = self.value().and_then(|value| self.render_preview(&value)) {
            self.feedback(&error, true);
        }
    }

    fn render_preview(&self, value: &ButtonText) -> Result<(), String> {
        let target = self.artwork.selected_target.get();
        if !matches!(target, ArtworkTarget::Keypad(_)) {
            return Ok(());
        }
        let layer = self.artwork.layers.borrow().active().to_owned();
        let (image, shown) = self.artwork.key_text_image(&layer, target, value)?;
        let texture = gtk::gdk::Texture::for_pixbuf(&image);
        self.preview.set_paintable(Some(&texture));
        self.overflow.set_visible(!text_fits(value));
        // A key that shows live information keeps its picture in the window.
        if self.artwork.shows_info(&layer, target) {
            return Ok(());
        }
        for (picture_target, picture) in self.artwork.pictures.borrow().iter() {
            if *picture_target == target {
                picture.set_paintable(Some(&texture));
                picture.set_visible(shown);
            }
        }
        Ok(())
    }

    fn persist(&self, remove: bool) {
        // Signals may still reach an insensitive or hidden button.
        if (remove && self.saved.borrow().is_none()) || (!remove && !self.can_save()) {
            return;
        }
        let result = (|| {
            let mut value = self.value()?;
            if remove {
                value.text.clear();
            }
            let layer = self.artwork.layers.borrow().active().to_owned();
            let target = self.artwork.selected_target.get();
            self.artwork
                .store
                .borrow_mut()
                .as_mut()
                .ok_or("Artwork storage is unavailable")?
                .set_button_text_in_layer(&layer, target, value)
                .map_err(|error| error.to_string())
        })();
        match result {
            Ok(()) => {
                self.artwork.refresh(true);
                self.load_context(true);
                self.feedback(if remove { "Text removed" } else { "Text saved" }, false);
                self.panel.popdown();
            }
            Err(error) => self.feedback(&format!("Could not save text: {error}"), true),
        }
    }
}

/// Text placement inside the key, as the renderer applies it: 2px margins and
/// the ink extents of the Pango layout, swapped for vertical text.
const KEY_TEXT_AREA: i32 = 116;

/// Whether `value` fits inside the key at its size and orientation, measured
/// with Pango exactly as `compose_button_image` lays it out.
pub(super) fn text_fits(value: &ButtonText) -> bool {
    if value.text.is_empty() {
        return true;
    }
    let context = pangocairo::FontMap::default().create_context();
    let layout = gtk::pango::Layout::new(&context);
    let mut font = gtk::pango::FontDescription::new();
    font.set_family(&value.font_family);
    font.set_absolute_size(f64::from(value.font_size) * f64::from(gtk::pango::SCALE));
    layout.set_font_description(Some(&font));
    layout.set_text(&value.text);
    let (ink, _) = layout.pixel_extents();
    let (width, height) = match value.orientation {
        TextOrientation::Horizontal => (ink.width(), ink.height()),
        TextOrientation::Vertical => (ink.height(), ink.width()),
    };
    width <= KEY_TEXT_AREA && height <= KEY_TEXT_AREA
}

const POSITION_NAMES: [&str; 9] = [
    "Top left",
    "Top center",
    "Top right",
    "Middle left",
    "Center",
    "Middle right",
    "Bottom left",
    "Bottom center",
    "Bottom right",
];

/// Grid index (row-major, top-left first) of a text alignment pair.
pub(super) fn position_index(
    horizontal: HorizontalAlignment,
    vertical: VerticalAlignment,
) -> usize {
    let column = match horizontal {
        HorizontalAlignment::Left => 0,
        HorizontalAlignment::Center => 1,
        HorizontalAlignment::Right => 2,
    };
    let row = match vertical {
        VerticalAlignment::Top => 0,
        VerticalAlignment::Center => 1,
        VerticalAlignment::Bottom => 2,
    };
    row * 3 + column
}

/// The alignment pair a grid index stands for.
pub(super) fn position_alignment(index: usize) -> (HorizontalAlignment, VerticalAlignment) {
    let horizontal = [
        HorizontalAlignment::Left,
        HorizontalAlignment::Center,
        HorizontalAlignment::Right,
    ][index % 3];
    let vertical = [
        VerticalAlignment::Top,
        VerticalAlignment::Center,
        VerticalAlignment::Bottom,
    ][(index / 3).min(2)];
    (horizontal, vertical)
}

/// A 3x3 grid of grouped toggles, each drawing a key outline with a dot where
/// the text sits. Arrow keys move focus between them.
fn position_grid() -> (gtk::Grid, [gtk::ToggleButton; 9]) {
    let grid = gtk::Grid::builder()
        .row_spacing(2)
        .column_spacing(2)
        .halign(gtk::Align::Start)
        .build();
    grid.add_css_class("button-text-position");
    grid.update_property(&[gtk::accessible::Property::Label("Text position")]);
    let buttons: [gtk::ToggleButton; 9] = std::array::from_fn(|index| {
        let button = gtk::ToggleButton::new();
        let glyph = gtk::DrawingArea::builder()
            .content_width(14)
            .content_height(14)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        let (column, row) = ((index % 3) as f64, (index / 3) as f64);
        glyph.set_draw_func(move |glyph, context, _, _| {
            let color = glyph.style_context().color();
            let (red, green, blue, alpha) = (
                f64::from(color.red()),
                f64::from(color.green()),
                f64::from(color.blue()),
                f64::from(color.alpha()),
            );
            // A faint key outline, then the solid text spot.
            context.set_source_rgba(red, green, blue, alpha * 0.45);
            context.set_line_width(1.0);
            context.rectangle(0.5, 0.5, 13.0, 13.0);
            let outline = context.stroke();
            context.set_source_rgba(red, green, blue, alpha);
            context.rectangle(2.0 + column * 3.0, 2.0 + row * 3.0, 4.0, 4.0);
            if let Err(error) = outline.and_then(|()| context.fill()) {
                eprintln!("Could not draw text position icon: {error}");
            }
        });
        button.set_child(Some(&glyph));
        button.set_tooltip_text(Some(POSITION_NAMES[index]));
        button.update_property(&[gtk::accessible::Property::Label(POSITION_NAMES[index])]);
        button
    });
    for (index, button) in buttons.iter().enumerate() {
        if index > 0 {
            button.set_group(Some(&buttons[0]));
        }
        grid.attach(button, (index % 3) as i32, (index / 3) as i32, 1, 1);
    }
    buttons[4].set_active(true);
    (grid, buttons)
}

/// A linked pair of icon toggles: upright and clockwise-rotated "Ab".
fn orientation_group() -> (GtkBox, [gtk::ToggleButton; 2]) {
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.add_css_class("linked");
    row.add_css_class("button-text-choices");
    row.update_property(&[gtk::accessible::Property::Label("Text orientation")]);
    let names = [
        ("Horizontal", "Horizontal text"),
        ("Vertical", "Vertical text, rotated 90 degrees clockwise"),
    ];
    let buttons: [gtk::ToggleButton; 2] = std::array::from_fn(|index| {
        let button = gtk::ToggleButton::new();
        let glyph = gtk::DrawingArea::builder()
            .content_width(18)
            .content_height(18)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        let vertical = index == 1;
        glyph.set_draw_func(move |glyph, context, width, height| {
            let layout = glyph.create_pango_layout(Some("Ab"));
            let mut font = gtk::pango::FontDescription::new();
            font.set_weight(gtk::pango::Weight::Bold);
            font.set_absolute_size(11.0 * f64::from(gtk::pango::SCALE));
            layout.set_font_description(Some(&font));
            let (ink, _) = layout.pixel_extents();
            let color = glyph.style_context().color();
            context.set_source_rgba(
                f64::from(color.red()),
                f64::from(color.green()),
                f64::from(color.blue()),
                f64::from(color.alpha()),
            );
            context.translate(f64::from(width) / 2.0, f64::from(height) / 2.0);
            if vertical {
                context.rotate(std::f64::consts::FRAC_PI_2);
            }
            context.move_to(
                -f64::from(ink.x()) - f64::from(ink.width()) / 2.0,
                -f64::from(ink.y()) - f64::from(ink.height()) / 2.0,
            );
            pangocairo::functions::show_layout(context, &layout);
        });
        button.set_child(Some(&glyph));
        button.set_tooltip_text(Some(names[index].1));
        button.update_property(&[gtk::accessible::Property::Label(names[index].0)]);
        button
    });
    buttons[1].set_group(Some(&buttons[0]));
    for button in &buttons {
        row.append(button);
    }
    buttons[0].set_active(true);
    (row, buttons)
}
