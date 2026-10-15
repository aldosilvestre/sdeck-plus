//! Shared native configuration controls and anchored secondary panels.
use gtk::{glib, prelude::*, Box as GtkBox, Button, Image, Label, Orientation, Popover};
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Clone)]
pub(crate) struct Toast {
    pub(crate) root: gtk::Revealer,
    pub(crate) label: Label,
    timeout: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Toast {
    pub(crate) fn new() -> Self {
        let root = gtk::Revealer::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_start(24)
            .margin_end(24)
            .margin_bottom(24)
            .accessible_role(gtk::AccessibleRole::Alert)
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .build();
        let content = GtkBox::new(Orientation::Horizontal, 12);
        content.add_css_class("toast");
        let label = Label::new(None);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_max_width_chars(40);
        label.set_xalign(0.0);
        content.append(&label);
        let dismiss = button("window-close-symbolic", "Dismiss");
        dismiss.update_property(&[gtk::accessible::Property::Label("Dismiss notification")]);
        dismiss.set_has_frame(false);
        content.append(&dismiss);
        root.set_child(Some(&content));
        let timeout = Rc::new(RefCell::new(None::<glib::SourceId>));
        let pending = timeout.clone();
        let weak = root.downgrade();
        dismiss.connect_clicked(move |_| {
            if let Some(source) = pending.borrow_mut().take() {
                source.remove();
            }
            if let Some(root) = weak.upgrade() {
                root.set_reveal_child(false);
            }
        });
        let pending = timeout.clone();
        root.connect_unrealize(move |_| {
            if let Some(source) = pending.borrow_mut().take() {
                source.remove();
            }
        });
        Self {
            root,
            label,
            timeout,
        }
    }

    pub(crate) fn wrap(&self, child: &impl IsA<gtk::Widget>) -> gtk::Overlay {
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(child));
        overlay.add_overlay(&self.root);
        overlay
    }

    pub(crate) fn show(&self, message: &str, failed: bool) {
        if let Some(source) = self.timeout.borrow_mut().take() {
            source.remove();
        }
        self.label.set_text(message);
        feedback(&self.label, failed);
        self.root.set_reveal_child(true);
        if !failed {
            let weak = self.root.downgrade();
            let pending = self.timeout.clone();
            *self.timeout.borrow_mut() = Some(glib::timeout_add_local_once(
                Duration::from_secs(5),
                move || {
                    pending.borrow_mut().take();
                    if let Some(root) = weak.upgrade() {
                        root.set_reveal_child(false);
                    }
                },
            ));
        }
    }
}

#[cfg(test)]
pub(crate) fn assert_toast_lifecycle(toast: &Toast) {
    toast.show("First save result", false);
    let old = glib::MainContext::default()
        .find_source_by_id(toast.timeout.borrow().as_ref().unwrap())
        .unwrap();
    toast.show("Replacement error", true);
    assert!(
        old.is_destroyed(),
        "replacement must cancel the old timeout"
    );
    assert!(toast.timeout.borrow().is_none());
    let loop_ = glib::MainLoop::new(None, false);
    let done = loop_.clone();
    let signal = toast.root.connect_reveal_child_notify(move |root| {
        if !root.reveals_child() {
            done.quit();
        }
    });
    let timed_out = Rc::new(std::cell::Cell::new(false));
    let flag = timed_out.clone();
    let done = loop_.clone();
    let deadline = glib::timeout_add_local_once(Duration::from_secs(10), move || {
        flag.set(true);
        done.quit();
    });
    // Auto-dismiss timing is the behavior under test; await its exact signal.
    toast.show("Successful retry", false);
    loop_.run();
    toast.root.disconnect(signal);
    if !timed_out.get() {
        deadline.remove();
    }
    assert!(!timed_out.get(), "success toast did not auto-dismiss");
    assert!(!toast.root.reveals_child());
    assert!(toast.timeout.borrow().is_none());
    toast.show("Dismissible error", true);
    toast
        .root
        .child()
        .unwrap()
        .last_child()
        .unwrap()
        .downcast::<Button>()
        .unwrap()
        .emit_clicked();
    assert!(!toast.root.reveals_child());
}

pub(crate) fn icon_caption(icon: &str, caption: &str) -> GtkBox {
    let content = GtkBox::new(Orientation::Horizontal, 8);
    content.set_halign(gtk::Align::Center);
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(16);
    content.append(&image);
    content.append(&Label::new(Some(caption)));
    content
}

pub(crate) fn decorate_button(button: &Button, icon: &str, caption: &str) {
    button.set_child(Some(&icon_caption(icon, caption)));
    button.add_css_class("icon-action");
    button.set_tooltip_text(Some(caption));
    button.update_property(&[gtk::accessible::Property::Label(caption)]);
}

pub(crate) fn button(icon: &str, caption: &str) -> Button {
    let button = Button::new();
    decorate_button(&button, icon, caption);
    button
}

pub(crate) fn icon_button(icon: &str, caption: &str) -> Button {
    let button = Button::builder()
        .icon_name(icon)
        .tooltip_text(caption)
        .build();
    button.add_css_class("icon-action");
    button.update_property(&[gtk::accessible::Property::Label(caption)]);
    button
}

pub(crate) fn menu_button(icon: &str, caption: &str, panel: &Popover) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .child(&icon_caption(icon, caption))
        .popover(panel)
        .build();
    button.add_css_class("icon-action");
    button.set_tooltip_text(Some(caption));
    button.update_property(&[gtk::accessible::Property::Label(caption)]);
    button
}

pub(crate) fn section_heading(icon: &str, text: &str) -> GtkBox {
    let heading = GtkBox::new(Orientation::Horizontal, 8);
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(20);
    let label = Label::new(Some(text));
    label.add_css_class("section-label");
    label.set_xalign(0.0);
    heading.append(&image);
    heading.append(&label);
    heading
}

pub(crate) fn helper(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_max_width_chars(38);
    label.add_css_class("helper");
    label
}

pub(crate) fn field_help(row: &GtkBox, title: &str, explanation: &str) {
    let Some(label) = row
        .first_child()
        .and_then(|child| child.downcast::<Label>().ok())
    else {
        return;
    };
    row.remove(&label);
    label.set_hexpand(true);
    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.append(&label);
    let panel = popover(title, &helper(explanation));
    let help = gtk::MenuButton::builder()
        .icon_name("help-about-symbolic")
        .tooltip_text(explanation)
        .popover(&panel)
        .build();
    help.add_css_class("field-help");
    help.update_property(&[gtk::accessible::Property::Label(title)]);
    header.append(&help);
    row.prepend(&header);
}

pub(crate) fn track_motion(widget: &impl IsA<gtk::Widget>) {
    let settings = widget.settings();
    if settings.is_gtk_enable_animations() {
        widget.add_css_class("motion-enabled");
    }
    let weak = widget.as_ref().downgrade();
    settings.connect_gtk_enable_animations_notify(move |settings| {
        if let Some(widget) = weak.upgrade() {
            if settings.is_gtk_enable_animations() {
                widget.add_css_class("motion-enabled");
            } else {
                widget.remove_css_class("motion-enabled");
            }
        }
    });
}

pub(crate) fn clear_feedback(label: &Label) {
    label.remove_css_class("feedback-error");
    label.remove_css_class("feedback-success");
}

pub(crate) fn feedback(label: &Label, failed: bool) {
    clear_feedback(label);
    label.add_css_class(if failed {
        "feedback-error"
    } else {
        "feedback-success"
    });
}

pub(crate) fn panel_body(body: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_width(true)
        .propagate_natural_height(true)
        .min_content_width(300)
        .max_content_width(560)
        .max_content_height(288)
        .child(body)
        .build()
}

pub(crate) fn popover(title: &str, body: &impl IsA<gtk::Widget>) -> Popover {
    let panel = Popover::new();
    panel.set_halign(gtk::Align::End);
    track_motion(&panel);
    let content = GtkBox::new(Orientation::Vertical, 16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some(title));
    title.add_css_class("section-label");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);
    let close = button("window-close-symbolic", "Close");
    close.set_has_frame(false);
    let weak = panel.downgrade();
    close.connect_clicked(move |_| {
        if let Some(panel) = weak.upgrade() {
            panel.popdown();
        }
    });
    header.append(&close);
    content.append(&header);
    content.append(body);
    panel.set_child(Some(&content));
    panel
}
