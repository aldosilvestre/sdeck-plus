//! Native keypad text controls sharing the hardware image compositor.

use super::{labeled, ui, ArtworkUi};
use gtk::{prelude::*, Box as GtkBox, Label, Orientation, Picture};
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
    pub(super) horizontal: [gtk::ToggleButton; 3],
    pub(super) vertical: [gtk::ToggleButton; 3],
    pub(super) save: gtk::Button,
    pub(super) remove: gtk::Button,
    pub(super) preview: Picture,
    status: Label,
    loading: Cell<bool>,
    control_available: Cell<bool>,
    context: RefCell<Option<(String, ArtworkTarget)>>,
    artwork: ArtworkUi,
}

impl ButtonTextEditor {
    pub(super) fn new(artwork: &ArtworkUi) -> Rc<Self> {
        let content = GtkBox::new(Orientation::Vertical, 8);
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
        let type_row = GtkBox::new(Orientation::Horizontal, 8);
        type_row.append(&labeled("Font", &font));
        type_row.append(&labeled("Size (px)", &size));
        form.append(&type_row);
        let (orientation_row, orientation) = toggle_group(["Horizontal", "Vertical"]);
        orientation[1].set_tooltip_text(Some("Rotate text 90 degrees clockwise"));
        form.append(&labeled("Orientation", &orientation_row));
        let (horizontal_row, horizontal) =
            alignment_group(["Align left", "Center horizontally", "Align right"], false);
        let (vertical_row, vertical) =
            alignment_group(["Align top", "Center vertically", "Align bottom"], true);
        let alignment_row = GtkBox::new(Orientation::Horizontal, 8);
        alignment_row.set_homogeneous(true);
        alignment_row.append(&labeled("Horizontal alignment", &horizontal_row));
        alignment_row.append(&labeled("Vertical alignment", &vertical_row));
        form.append(&alignment_row);
        body.append(&form);
        let preview_column = GtkBox::new(Orientation::Vertical, 8);
        preview_column.set_valign(gtk::Align::Start);
        let preview = Picture::new();
        preview.set_size_request(120, 120);
        preview.set_halign(gtk::Align::Center);
        preview.set_can_shrink(false);
        preview.update_property(&[gtk::accessible::Property::Label("Button text preview")]);
        preview_column.append(&labeled("Preview", &preview));
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
        let open = ui::menu_button("insert-text-symbolic", "Add text", &panel);
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
            horizontal,
            vertical,
            save,
            remove,
            preview,
            status,
            loading: Cell::new(false),
            control_available: Cell::new(true),
            context: RefCell::new(None),
            artwork: artwork.clone(),
        });
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
        for button in editor
            .orientation
            .iter()
            .chain(editor.horizontal.iter())
            .chain(editor.vertical.iter())
        {
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
        let value = self
            .artwork
            .store
            .borrow()
            .as_ref()
            .and_then(|store| store.button_text_in_layer(&layer, target).cloned())
            .unwrap_or_default();
        let caption = if value.text.is_empty() {
            "Add text"
        } else {
            "Edit text"
        };
        self.open
            .set_child(Some(&ui::icon_caption("insert-text-symbolic", caption)));
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
        self.horizontal[match value.horizontal_alignment {
            HorizontalAlignment::Left => 0,
            HorizontalAlignment::Center => 1,
            HorizontalAlignment::Right => 2,
        }]
        .set_active(true);
        self.vertical[match value.vertical_alignment {
            VerticalAlignment::Top => 0,
            VerticalAlignment::Center => 1,
            VerticalAlignment::Bottom => 2,
        }]
        .set_active(true);
        self.loading.set(false);
        let available = self.artwork.store.borrow().is_some();
        self.root.set_sensitive(available);
        self.status.set_text("");
        if let Err(error) = self.render_preview(&value) {
            self.feedback(&error, true);
        }
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
            horizontal_alignment: match self.horizontal.iter().position(|button| button.is_active())
            {
                Some(0) => HorizontalAlignment::Left,
                Some(1) => HorizontalAlignment::Center,
                Some(2) => HorizontalAlignment::Right,
                _ => return Err("Choose a horizontal alignment".into()),
            },
            vertical_alignment: match self.vertical.iter().position(|button| button.is_active()) {
                Some(0) => VerticalAlignment::Top,
                Some(1) => VerticalAlignment::Center,
                Some(2) => VerticalAlignment::Bottom,
                _ => return Err("Choose a vertical alignment".into()),
            },
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
                self.status.set_text("Unsaved changes");
                self.status.set_tooltip_text(None);
                ui::clear_feedback(&self.status);
            }
            Err(error) => self.feedback(&error, true),
        }
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
        let path = self
            .artwork
            .store
            .borrow()
            .as_ref()
            .and_then(|store| store.assigned_path_in_layer(&layer, target));
        let image = sdeck_plus::image_output::compose_button_image(path.as_deref(), value)?;
        let texture = gtk::gdk::Texture::for_pixbuf(&image);
        self.preview.set_paintable(Some(&texture));
        // A key that shows live information keeps its picture in the window.
        if self.artwork.shows_info(&layer, target) {
            return Ok(());
        }
        for (picture_target, picture) in self.artwork.pictures.borrow().iter() {
            if *picture_target == target {
                picture.set_paintable(Some(&texture));
                picture.set_visible(path.is_some() || !value.text.is_empty());
            }
        }
        Ok(())
    }

    fn persist(&self, remove: bool) {
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

fn alignment_group(captions: [&str; 3], vertical: bool) -> (GtkBox, [gtk::ToggleButton; 3]) {
    let (row, buttons) = toggle_group(captions);
    for (index, (button, caption)) in buttons.iter().zip(captions).enumerate() {
        let icon = gtk::DrawingArea::builder()
            .content_width(16)
            .content_height(16)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        icon.set_draw_func(move |icon, context, _, _| {
            let color = icon.style_context().color();
            context.set_source_rgba(
                f64::from(color.red()),
                f64::from(color.green()),
                f64::from(color.blue()),
                f64::from(color.alpha()),
            );
            if vertical {
                context.rectangle(2.0, 1.0, 12.0, 1.0);
                context.rectangle(2.0, 14.0, 12.0, 1.0);
                let top = [3.0, 5.0, 7.0][index];
                for (width, offset) in [10.0, 6.0, 10.0].into_iter().zip([0.0, 2.0, 4.0]) {
                    context.rectangle(3.0, top + offset, width, 1.0);
                }
            } else {
                for (width, top) in [12.0, 8.0, 12.0, 6.0]
                    .into_iter()
                    .zip([3.0, 6.0, 9.0, 12.0])
                {
                    let left = [2.0, (16.0 - width) / 2.0, 14.0 - width][index];
                    context.rectangle(left, top, width, 1.0);
                }
            }
            if let Err(error) = context.fill() {
                eprintln!("Could not draw text alignment icon: {error}");
            }
        });
        button.set_child(Some(&icon));
        button.set_tooltip_text(Some(caption));
        button.update_property(&[gtk::accessible::Property::Label(caption)]);
    }
    (row, buttons)
}

fn toggle_group<const N: usize>(captions: [&str; N]) -> (GtkBox, [gtk::ToggleButton; N]) {
    const { assert!(N > 0) };
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.add_css_class("linked");
    row.add_css_class("button-text-choices");
    row.set_homogeneous(true);
    let buttons = captions.map(gtk::ToggleButton::with_label);
    for button in buttons.iter().skip(1) {
        button.set_group(Some(&buttons[0]));
    }
    for button in &buttons {
        row.append(button);
    }
    buttons[0].set_active(true);
    (row, buttons)
}
