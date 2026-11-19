//! Shared native configuration controls and anchored secondary panels.
use gtk::{glib, prelude::*, Box as GtkBox, Button, Image, Label, Orientation, Popover};
use std::{cell::RefCell, rc::Rc, time::Duration};

/// What a toast's action button runs; taken out when used, so it runs once.
type ToastHandler = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// How long a plain success message stays before it closes itself.
const MESSAGE_LIFETIME: Duration = Duration::from_secs(5);
/// How long a message with an action (such as Undo) stays, and its action with it.
const ACTION_LIFETIME: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(crate) struct Toast {
    pub(crate) root: gtk::Revealer,
    pub(crate) label: Label,
    /// The tone glyph before the message: a check for success, an alert for an error.
    pub(crate) tone: Image,
    /// Shown only for errors, which stay until dismissed; a success hides itself.
    pub(crate) dismiss: Button,
    /// An optional action beside the message, such as Undo; hidden unless a caller asks for it.
    pub(crate) action: Button,
    handler: ToastHandler,
    timeout: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Toast {
    pub(crate) fn new() -> Self {
        // Messages show at the top center of the window, over its body.
        let root = gtk::Revealer::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .accessible_role(gtk::AccessibleRole::Alert)
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .build();
        let content = GtkBox::new(Orientation::Horizontal, 12);
        content.add_css_class("toast");
        let tone = Image::from_icon_name("object-select-symbolic");
        tone.add_css_class("toast-tone");
        tone.set_accessible_role(gtk::AccessibleRole::Presentation);
        content.append(&tone);
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
        {
            let (handler, pending, weak) = (handler.clone(), timeout.clone(), root.downgrade());
            dismiss.connect_clicked(move |_| {
                if let Some(source) = pending.borrow_mut().take() {
                    source.remove();
                }
                drop(handler.take());
                if let Some(root) = weak.upgrade() {
                    root.set_reveal_child(false);
                }
            });
        }
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
        let (pending, expiring) = (timeout.clone(), handler.clone());
        root.connect_unrealize(move |root| {
            if let Some(source) = pending.borrow_mut().take() {
                source.remove();
            }
            // Without its timer an action would outlive its time: it goes, and its message.
            if expiring.take().is_some() {
                root.set_reveal_child(false);
            }
        });
        Self {
            root,
            label,
            tone,
            dismiss,
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

    /// A message with one action beside it (for example Undo). It stays ten seconds, twice as
    /// long as a plain message, since reading it and deciding takes longer. When it closes, or
    /// another message replaces it, the action is dropped: it can no longer run, and what it
    /// holds is released.
    #[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
    pub(crate) fn show_with_action(&self, message: &str, caption: &str, run: Rc<dyn Fn()>) {
        self.present(message, false, Some(ACTION_LIFETIME));
        self.action.set_label(caption);
        self.action
            .update_property(&[gtk::accessible::Property::Label(caption)]);
        self.action.set_visible(true);
        *self.handler.borrow_mut() = Some(run);
    }

    /// A message: a success closes itself after five seconds, an error stays until dismissed.
    pub(crate) fn show(&self, message: &str, failed: bool) {
        self.present(message, failed, (!failed).then_some(MESSAGE_LIFETIME));
    }

    /// Replace whatever the toast shows with `message`, closing it after `lifetime` if any.
    fn present(&self, message: &str, failed: bool, lifetime: Option<Duration>) {
        if let Some(source) = self.timeout.borrow_mut().take() {
            source.remove();
        }
        // Any earlier action no longer belongs to this message.
        self.action.set_visible(false);
        drop(self.handler.take());
        self.label.set_text(message);
        // The message stays in ink; only the tone glyph carries the colour.
        for class in ["feedback-error", "feedback-success"] {
            self.tone.remove_css_class(class);
        }
        self.tone.add_css_class(if failed {
            "feedback-error"
        } else {
            "feedback-success"
        });
        self.tone.set_icon_name(Some(if failed {
            "dialog-error-symbolic"
        } else {
            "object-select-symbolic"
        }));
        self.dismiss.set_visible(failed);
        self.root.set_reveal_child(true);
        if let Some(lifetime) = lifetime {
            let weak = self.root.downgrade();
            let (pending, handler) = (self.timeout.clone(), self.handler.clone());
            *self.timeout.borrow_mut() = Some(glib::timeout_add_local_once(lifetime, move || {
                pending.borrow_mut().take();
                // An expired action can no longer run; what it holds is released.
                drop(handler.take());
                if let Some(root) = weak.upgrade() {
                    root.set_reveal_child(false);
                }
            }));
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
    let shown = std::time::Instant::now();
    toast.show("Successful retry", false);
    loop_.run();
    toast.root.disconnect(signal);
    if !timed_out.get() {
        deadline.remove();
    }
    assert!(!timed_out.get(), "success toast did not auto-dismiss");
    assert!(
        shown.elapsed() >= Duration::from_secs(5),
        "a plain success stays five seconds"
    );
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

/// Runs the main loop until `toast` closes by itself, at most `limit`: how long after `shown` it
/// closed, or `None` when it was still open. Awaits the real reveal signal, never a fixed sleep.
#[cfg(test)]
pub(crate) fn time_until_closed(
    toast: &Toast,
    shown: std::time::Instant,
    limit: Duration,
) -> Option<Duration> {
    let loop_ = glib::MainLoop::new(None, false);
    let closed = Rc::new(std::cell::Cell::new(None));
    let (seen, done) = (closed.clone(), loop_.clone());
    let signal = toast.root.connect_reveal_child_notify(move |root| {
        if !root.reveals_child() && seen.get().is_none() {
            seen.set(Some(shown.elapsed()));
            done.quit();
        }
    });
    let timed_out = Rc::new(std::cell::Cell::new(false));
    let (flag, done) = (timed_out.clone(), loop_.clone());
    let deadline = glib::timeout_add_local_once(limit, move || {
        flag.set(true);
        done.quit();
    });
    if toast.root.reveals_child() {
        loop_.run();
    }
    toast.root.disconnect(signal);
    if !timed_out.get() {
        deadline.remove();
    }
    closed.get()
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

/// A small muted sub-heading of the profile rail ("PROFILES", "TOOLS").
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn rail_heading(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("rail-heading");
    label.set_xalign(0.0);
    label
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

/// The first descendant of `widget` carrying the CSS class `class`.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
fn descendant_with_class(widget: &gtk::Widget, class: &str) -> Option<gtk::Widget> {
    let mut child = widget.first_child();
    while let Some(current) = child {
        if current.has_css_class(class) {
            return Some(current);
        }
        if let Some(found) = descendant_with_class(&current, class) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

/// The artboard dialog frame for a `gtk::Dialog`: radius-12 surface, 14px padding, 10px gap and
/// ONE right-aligned action row of natural-width buttons with an 8px gap. Add the buttons in
/// reading order: the dismissal first, then the primary or danger action.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn style_dialog(dialog: &gtk::Dialog) {
    dialog.add_css_class("popup-dialog");
    let content = dialog.content_area();
    content.set_spacing(10);
    if let Some(area) = descendant_with_class(dialog.upcast_ref(), "dialog-action-area")
        .and_then(|area| area.downcast::<GtkBox>().ok())
    {
        area.set_homogeneous(false);
        area.set_spacing(8);
        area.set_halign(gtk::Align::End);
        area.set_hexpand(true);
    }
}

/// Adds the dismissing button of a dialog: its caption followed by the `Esc` hint.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn dismiss_button(
    dialog: &gtk::Dialog,
    caption: &str,
    response: gtk::ResponseType,
) -> Button {
    let button = dialog.add_button(caption, response);
    add_key_hint(button.upcast_ref(), "Esc");
    button
        .downcast::<Button>()
        .expect("dialog action buttons are buttons")
}

/// A confirm dialog reads like a dialog: title and explanation start at the left edge.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn style_confirm_dialog(dialog: &gtk::MessageDialog) {
    style_dialog(dialog.upcast_ref());
    dialog.add_css_class("confirm-dialog");
    let mut child = dialog.message_area().first_child();
    while let Some(widget) = child {
        if let Some(label) = widget.downcast_ref::<Label>() {
            label.set_justify(gtk::Justification::Left);
            label.set_xalign(0.0);
            label.set_halign(gtk::Align::Fill);
            label.set_hexpand(true);
        }
        child = widget.next_sibling();
    }
    // The theme insets the message block by 30px; the dialog's own 14px padding replaces that.
    let mut block = dialog.message_area().parent();
    while let Some(widget) = block {
        if widget.has_css_class("dialog-vbox") {
            break;
        }
        widget.set_margin_start(0);
        widget.set_margin_end(0);
        block = widget.parent();
    }
    dialog.set_resizable(false);
    dialog.set_default_size(360, -1);
}

/// The destructive confirmation of the compact-command design: `Delete “name”?`, a body that
/// says what goes with it, **Cancel** (with its `Esc` hint, the default) and a red **Delete**.
/// The caller connects the response; only `ResponseType::Accept` means delete.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn delete_confirmation(
    parent: &impl IsA<gtk::Window>,
    name: &str,
    detail: &str,
) -> gtk::MessageDialog {
    let dialog = gtk::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .destroy_with_parent(true)
        .message_type(gtk::MessageType::Question)
        .text(format!("Delete \u{201c}{name}\u{201d}?"))
        .secondary_text(detail)
        .build();
    style_confirm_dialog(&dialog);
    add_key_hint(
        &dialog.add_button("Cancel", gtk::ResponseType::Cancel),
        "Esc",
    );
    dialog
        .add_button("Delete", gtk::ResponseType::Accept)
        .add_css_class("destructive-action");
    dialog.set_default_response(gtk::ResponseType::Cancel);
    dialog
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
    if let Some(content) = row.child().and_downcast::<GtkBox>() {
        content.set_halign(gtk::Align::Start);
        content.set_spacing(10);
    }
    row
}

/// The compact Organize-style command menu: just rows, no header or forms.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn command_menu(rows: &[Button]) -> Popover {
    command_menu_sections(&[rows])
}

/// A command menu whose sections are divided by a hairline, as in the artboard.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn command_menu_sections(sections: &[&[Button]]) -> Popover {
    let panel = Popover::new();
    panel.set_halign(gtk::Align::End);
    panel.set_has_arrow(false);
    panel.add_css_class("organize-popover");
    panel.add_css_class("command-menu");
    track_motion(&panel);
    // The popover's own 6px padding frames the rows; rows touch, as in the artboard.
    let list = GtkBox::new(Orientation::Vertical, 0);
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            list.append(&gtk::Separator::new(Orientation::Horizontal));
        }
        for row in *section {
            list.append(row);
        }
    }
    panel.set_child(Some(&list));
    panel
}

/// A transient modal dialog holding one profile form. Escape and the footer's dismissal
/// close (hide) it; the form widgets are reused, never rebuilt, so state is not duplicated.
/// The footer is the artboard's single right-aligned action row: the dismissal with its `Esc`
/// hint, then `primary` (the form's create or save button) when there is one.
#[allow(dead_code)] // unused by the ui_showcase example, which mounts this file
pub(crate) fn profile_dialog(
    parent: &impl IsA<gtk::Window>,
    title: &str,
    subtitle: &str,
    body: &impl IsA<gtk::Widget>,
    dismiss: &str,
    primary: Option<&Button>,
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
    dialog.add_css_class("popup-dialog");
    let content = GtkBox::new(Orientation::Vertical, 10);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    content.set_margin_start(14);
    content.set_margin_end(14);
    let heading = Label::new(Some(title));
    heading.add_css_class("dialog-title");
    heading.set_xalign(0.0);
    content.append(&heading);
    let note = helper(subtitle);
    note.add_css_class("organize-subtitle");
    content.append(&note);
    content.append(body);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.add_css_class("dialog-actions");
    actions.set_halign(gtk::Align::End);
    let close = Button::with_label(dismiss);
    add_key_hint(close.upcast_ref(), "Esc");
    let weak = dialog.downgrade();
    close.connect_clicked(move |_| {
        if let Some(dialog) = weak.upgrade() {
            dialog.close();
        }
    });
    actions.append(&close);
    if let Some(primary) = primary {
        actions.append(primary);
    }
    content.append(&actions);
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
    panel.set_has_arrow(false);
    panel.add_css_class("help-popover");
    track_motion(&panel);
    let body = GtkBox::new(Orientation::Vertical, 6);
    body.set_margin_top(12);
    body.set_margin_bottom(12);
    body.set_margin_start(12);
    body.set_margin_end(12);
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

const FEEDBACK_CLASSES: [&str; 3] = ["feedback-error", "feedback-success", "feedback-warning"];

pub(crate) fn clear_feedback(label: &Label) {
    for class in FEEDBACK_CLASSES {
        label.remove_css_class(class);
    }
    if let Some(banner) = banner_of(label) {
        for class in FEEDBACK_CLASSES {
            banner.remove_css_class(class);
        }
        if let Some(icon) = banner.first_child().and_downcast::<Image>() {
            icon.set_visible(false);
        }
    }
}

pub(crate) fn feedback(label: &Label, failed: bool) {
    clear_feedback(label);
    let class = if failed {
        "feedback-error"
    } else {
        "feedback-success"
    };
    label.add_css_class(class);
    if let Some(banner) = banner_of(label).or_else(|| wrap_in_banner(label)) {
        banner.add_css_class(class);
        if let Some(icon) = banner.first_child().and_downcast::<Image>() {
            icon.set_icon_name(Some(if failed {
                "dialog-error-symbolic"
            } else {
                "object-select-symbolic"
            }));
            icon.set_visible(true);
        }
    }
}

/// The banner row a feedback label already sits in, if any.
pub(crate) fn banner_of(label: &Label) -> Option<GtkBox> {
    label
        .parent()
        .and_downcast::<GtkBox>()
        .filter(|parent| parent.has_css_class("banner"))
}

/// Put a feedback label into a banner row (tone glyph, then the message in ink) in the same
/// place of its box, the first time it shows feedback. The row follows the label's visibility.
/// Labels outside a box (none today) keep the tinted-label look.
fn wrap_in_banner(label: &Label) -> Option<GtkBox> {
    let parent = label.parent().and_downcast::<GtkBox>()?;
    let before = label.prev_sibling();
    let banner = GtkBox::new(Orientation::Horizontal, 8);
    banner.add_css_class("banner");
    banner.set_hexpand(label.hexpands());
    banner.set_halign(label.halign());
    banner.set_valign(label.valign());
    let icon = Image::from_icon_name("object-select-symbolic");
    icon.add_css_class("banner-tone");
    icon.set_valign(gtk::Align::Start);
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    banner.append(&icon);
    parent.remove(label);
    banner.append(label);
    parent.insert_child_after(&banner, before.as_ref());
    label
        .bind_property("visible", &banner, "visible")
        .sync_create()
        .build();
    Some(banner)
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

/// Give a titled popover an icon beside its title and a one-line subtitle under it.
/// Only the `ui_showcase` example uses it; the form popovers pass their subtitle to
/// [`form_popover`].
#[allow(dead_code)]
pub(crate) fn refine_popover_header(panel: &Popover, icon: &str, subtitle: &str) {
    let Some(content) = panel.child().and_downcast::<GtkBox>() else {
        return;
    };
    let Some(header) = content.first_child().and_downcast::<GtkBox>() else {
        return;
    };
    let Some(title) = header.first_child() else {
        return;
    };
    header.remove(&title);
    let line = GtkBox::new(Orientation::Horizontal, 8);
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(20);
    image.add_css_class("popover-icon");
    line.append(&image);
    line.append(&title);
    header.prepend(&line);
    let note = helper(subtitle);
    note.add_css_class("popover-subtitle");
    header.append(&note);
}

/// A popover with a bold title line inside its body, like the help popover. It has no title bar
/// and no Close button: Escape or a click outside dismisses it (GTK's default autohide).
#[allow(dead_code)] // the ui_showcase example mounts this file and only uses `popover`
pub(crate) fn form_popover(
    title: &str,
    subtitle: Option<&str>,
    body: &impl IsA<gtk::Widget>,
) -> Popover {
    let panel = Popover::new();
    panel.set_halign(gtk::Align::End);
    panel.set_has_arrow(false);
    panel.add_css_class("form-popover");
    panel.update_property(&[gtk::accessible::Property::Label(title)]);
    track_motion(&panel);
    // The popover keeps its 6px padding; the form content sits 12px inside it.
    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let header = GtkBox::new(Orientation::Vertical, 2);
    let heading = Label::new(Some(title));
    heading.add_css_class("section-label");
    heading.add_css_class("popover-title");
    heading.set_xalign(0.0);
    header.append(&heading);
    if let Some(subtitle) = subtitle {
        let note = helper(subtitle);
        note.add_css_class("popover-subtitle");
        header.append(&note);
    }
    content.append(&header);
    content.append(body);
    panel.set_child(Some(&content));
    panel
}

#[allow(dead_code)] // only the ui_showcase example uses the subtitle-less form
pub(crate) fn popover(title: &str, body: &impl IsA<gtk::Widget>) -> Popover {
    form_popover(title, None, body)
}
