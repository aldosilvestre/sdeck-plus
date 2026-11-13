//! Shared native configuration controls and anchored secondary panels.
use gtk::{glib, prelude::*, Box as GtkBox, Button, Image, Label, Orientation, Popover};
use std::{cell::RefCell, rc::Rc, time::Duration};

/// What a toast's action button runs; taken out when used, so it runs once.
type ToastHandler = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

#[derive(Clone)]
pub(crate) struct Toast {
    pub(crate) root: gtk::Revealer,
    pub(crate) label: Label,
    /// An optional action beside the message, such as Undo; hidden unless a caller asks for it.
    pub(crate) action: Button,
    handler: ToastHandler,
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
        let action = Button::new();
        action.add_css_class("toast-action");
        action.set_has_frame(false);
        action.set_visible(false);
        content.append(&action);
        let handler: ToastHandler = Rc::default();
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
        {
            let (handler, pending, weak) = (handler.clone(), timeout.clone(), root.downgrade());
            action.connect_clicked(move |_| {
                // The action runs once: it is taken out before it is called.
                let run = handler.borrow_mut().take();
                if let Some(source) = pending.borrow_mut().take() {
                    source.remove();
                }
                if let Some(root) = weak.upgrade() {
                    root.set_reveal_child(false);
                }
                if let Some(run) = run {
                    run();
                }
            });
        }
        let pending = timeout.clone();
        root.connect_unrealize(move |_| {
            if let Some(source) = pending.borrow_mut().take() {
                source.remove();
            }
        });
        Self {
            root,
            label,
            action,
            handler,
            timeout,
        }
    }

    pub(crate) fn wrap(&self, child: &impl IsA<gtk::Widget>) -> gtk::Overlay {
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(child));
        overlay.add_overlay(&self.root);
        overlay
    }

    /// A message with one action beside it (for example Undo). It stays a little longer than a
    /// plain message, since reading it and deciding takes longer.
    #[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
    pub(crate) fn show_with_action(&self, message: &str, caption: &str, run: Rc<dyn Fn()>) {
        self.show(message, false);
        self.action.set_label(caption);
        self.action
            .update_property(&[gtk::accessible::Property::Label(caption)]);
        self.action.set_visible(true);
        *self.handler.borrow_mut() = Some(run);
    }

    pub(crate) fn show(&self, message: &str, failed: bool) {
        if let Some(source) = self.timeout.borrow_mut().take() {
            source.remove();
        }
        // Any earlier action no longer belongs to this message.
        self.action.set_visible(false);
        *self.handler.borrow_mut() = None;
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
    let content = GtkBox::new(Orientation::Horizontal, 6);
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(16);
    content.append(&image);
    content.append(&Label::new(Some(caption)));
    content
}

/// A label that follows `source`: its text, tooltip and style classes. Used to show the
/// connection badge in the profile rail while the header's label stays the one written to.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn mirror_badge(source: &Label) -> Label {
    let badge = Label::new(None);
    badge.set_halign(gtk::Align::Start);
    badge.set_valign(gtk::Align::Start);
    badge.set_ellipsize(gtk::pango::EllipsizeMode::End);
    badge.set_margin_bottom(6);
    let copy = {
        let badge = badge.downgrade();
        move |source: &Label| {
            let Some(badge) = badge.upgrade() else { return };
            badge.set_text(&source.text());
            badge.set_tooltip_text(source.tooltip_text().as_deref());
            let mut classes: Vec<String> = source
                .css_classes()
                .iter()
                .map(ToString::to_string)
                .collect();
            classes.push("rail-badge".into());
            let classes: Vec<&str> = classes.iter().map(String::as_str).collect();
            badge.set_css_classes(&classes);
        }
    };
    copy(source);
    let update = copy.clone();
    source.connect_label_notify(move |source| update(source));
    let update = copy.clone();
    source.connect_css_classes_notify(move |source| update(source));
    source.connect_tooltip_text_notify(move |source| copy(source));
    badge
}

/// A confirm dialog reads like a dialog: title and explanation start at the left edge.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn style_confirm_dialog(dialog: &gtk::MessageDialog) {
    dialog.add_css_class("confirm-dialog");
    let mut child = dialog.message_area().first_child();
    while let Some(widget) = child {
        if let Some(label) = widget.downcast_ref::<Label>() {
            label.set_justify(gtk::Justification::Left);
            label.set_xalign(0.0);
        }
        child = widget.next_sibling();
    }
    dialog.set_resizable(false);
    dialog.set_default_size(360, -1);
}

/// Shows a keyboard hint (for example `Esc`) after a button's caption. The caption stays the
/// button's accessible name.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn add_key_hint(button: &gtk::Widget, hint: &str) {
    let Some(button) = button.downcast_ref::<Button>() else {
        return;
    };
    let Some(caption) = button.label() else {
        return;
    };
    let content = GtkBox::new(Orientation::Horizontal, 8);
    content.append(&Label::new(Some(&caption)));
    let key = Label::new(Some(hint));
    key.add_css_class("key-hint");
    content.append(&key);
    button.set_child(Some(&content));
    button.update_property(&[gtk::accessible::Property::Label(&caption)]);
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
    button.add_css_class("icon-only");
    if let Some(image) = button.child().and_downcast::<Image>() {
        image.set_pixel_size(16);
    }
    button.update_property(&[gtk::accessible::Property::Label(caption)]);
    button
}

pub(crate) fn menu_button(icon: &str, caption: &str, panel: &Popover) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .child(&icon_caption(icon, caption))
        .always_show_arrow(false)
        .popover(panel)
        .build();
    button.add_css_class("icon-action");
    button.set_tooltip_text(Some(caption));
    button.update_property(&[gtk::accessible::Property::Label(caption)]);
    button
}

/// A frameless, left-aligned row of the compact command menu.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn command_row(icon: &str, caption: &str) -> Button {
    let row = button(icon, caption);
    row.add_css_class("command-row");
    row.set_has_frame(false);
    if let Some(content) = row.child() {
        content.set_halign(gtk::Align::Start);
    }
    row
}

/// The compact Organize-style command menu: just rows, no header or forms.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn command_menu(rows: &[Button]) -> Popover {
    let panel = Popover::new();
    panel.set_halign(gtk::Align::End);
    panel.add_css_class("organize-popover");
    panel.add_css_class("command-menu");
    track_motion(&panel);
    let list = GtkBox::new(Orientation::Vertical, 2);
    list.set_margin_top(6);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);
    for row in rows {
        list.append(row);
    }
    panel.set_child(Some(&list));
    panel
}

/// A transient modal dialog holding one profile form. Escape and the footer button
/// close (hide) it; the form widgets are reused, never rebuilt, so state is not duplicated.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn profile_dialog(
    parent: &impl IsA<gtk::Window>,
    title: &str,
    subtitle: &str,
    body: &impl IsA<gtk::Widget>,
    dismiss: &str,
) -> gtk::Window {
    let dialog = gtk::Window::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .destroy_with_parent(true)
        .hide_on_close(true)
        .resizable(false)
        .default_width(400)
        .build();
    dialog.add_css_class("profile-dialog");
    let content = GtkBox::new(Orientation::Vertical, 16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    let heading = Label::new(Some(title));
    heading.add_css_class("section-label");
    heading.set_xalign(0.0);
    content.append(&heading);
    let note = helper(subtitle);
    note.add_css_class("organize-subtitle");
    content.append(&note);
    content.append(body);
    let close = Button::with_label(dismiss);
    add_key_hint(close.upcast_ref(), "Esc");
    close.set_halign(gtk::Align::End);
    let weak = dialog.downgrade();
    close.connect_clicked(move |_| {
        if let Some(dialog) = weak.upgrade() {
            dialog.close();
        }
    });
    content.append(&close);
    dialog.set_child(Some(&content));
    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(|widget, _| {
            if let Some(window) = widget.downcast_ref::<gtk::Window>() {
                window.close();
            }
            glib::Propagation::Stop
        })),
    ));
    dialog.add_controller(escape);
    dialog
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
    // A light panel that appears on hover or keyboard focus: no click needed,
    // and no native tooltip repeating it.
    let panel = Popover::new();
    panel.set_autohide(false);
    panel.set_can_target(false);
    panel.set_position(gtk::PositionType::Bottom);
    panel.add_css_class("help-popover");
    track_motion(&panel);
    let body = GtkBox::new(Orientation::Vertical, 6);
    body.set_margin_top(12);
    body.set_margin_bottom(12);
    body.set_margin_start(14);
    body.set_margin_end(14);
    let heading = Label::new(Some(title));
    heading.add_css_class("section-label");
    heading.set_xalign(0.0);
    body.append(&heading);
    body.append(&helper(explanation));
    panel.set_child(Some(&body));
    let help = gtk::MenuButton::builder()
        .icon_name("help-about-symbolic")
        .popover(&panel)
        .build();
    help.add_css_class("field-help");
    // Just the glyph: no button frame around the information icon.
    help.set_has_frame(false);
    help.update_property(&[gtk::accessible::Property::Label(title)]);
    let motion = gtk::EventControllerMotion::new();
    let weak = panel.downgrade();
    motion.connect_enter(move |_, _, _| {
        if let Some(panel) = weak.upgrade() {
            panel.popup();
        }
    });
    let weak = panel.downgrade();
    motion.connect_leave(move |_| {
        if let Some(panel) = weak.upgrade() {
            panel.popdown();
        }
    });
    help.add_controller(motion);
    let focus = gtk::EventControllerFocus::new();
    let weak = panel.downgrade();
    // Only keyboard navigation (visible focus) opens it; a form that merely
    // starts with this button focused must not pop the help open by itself.
    focus.connect_enter(move |controller| {
        let keyboard = controller
            .widget()
            .and_then(|widget| widget.root())
            .and_downcast::<gtk::Window>()
            .is_some_and(|window| window.gets_focus_visible());
        if let (Some(panel), true) = (weak.upgrade(), keyboard) {
            panel.popup();
        }
    });
    let weak = panel.downgrade();
    focus.connect_leave(move |_| {
        if let Some(panel) = weak.upgrade() {
            panel.popdown();
        }
    });
    help.add_controller(focus);
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
    label.remove_css_class("feedback-warning");
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

/// Give a titled popover an icon, a one-line subtitle and an icon-only Close.
/// Organize no longer uses it; the `ui_showcase` example still does.
#[allow(dead_code)]
pub(crate) fn refine_popover_header(panel: &Popover, icon: &str, subtitle: &str) {
    let Some(content) = panel.child().and_downcast::<GtkBox>() else {
        return;
    };
    let Some(header) = content.first_child().and_downcast::<GtkBox>() else {
        return;
    };
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(20);
    image.add_css_class("popover-icon");
    header.prepend(&image);
    if let Some(close) = header.last_child().and_downcast::<Button>() {
        let glyph = Image::from_icon_name("window-close-symbolic");
        glyph.set_pixel_size(16);
        close.set_child(Some(&glyph));
        close.set_tooltip_text(Some("Close"));
        close.add_css_class("popover-close");
        close.update_property(&[gtk::accessible::Property::Label("Close")]);
    }
    let note = helper(subtitle);
    note.add_css_class("organize-subtitle");
    content.insert_child_after(&note, Some(&header));
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
