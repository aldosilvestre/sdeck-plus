//! Feedback follows design/overlays/03-compact-command: a tone glyph carries the colour and
//! the message stays in ink, in toasts and in inline banners.
use super::*;

fn gtk_ready() -> bool {
    if init_gtk_for_test() {
        return true;
    }
    assert!(
        std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none(),
        "GTK initialization failed despite an advertised display"
    );
    false
}

#[test]
fn a_toast_shows_a_tone_glyph_and_its_dismiss_only_for_errors() {
    run_gtk_test(toast_body);
}

fn toast_body() {
    if !gtk_ready() {
        return;
    }
    let toast = ui::Toast::new();
    toast.show_with_action("Binding removed.", "Undo", Rc::new(|| {}));
    assert!(toast.tone.has_css_class("feedback-success"));
    assert_eq!(
        toast.tone.icon_name().as_deref(),
        Some("emblem-ok-symbolic")
    );
    assert!(toast.action.is_visible(), "Undo sits beside the message");
    assert!(!toast.dismiss.is_visible(), "a success needs no Dismiss");
    assert!(
        !toast.label.has_css_class("feedback-success"),
        "the message stays in ink"
    );
    assert_eq!(toast.label.text(), "Binding removed.");

    toast.show("Could not save the binding.", true);
    assert!(toast.tone.has_css_class("feedback-error"));
    assert!(!toast.tone.has_css_class("feedback-success"));
    assert_eq!(
        toast.tone.icon_name().as_deref(),
        Some("dialog-error-symbolic")
    );
    assert!(toast.dismiss.is_visible(), "an error stays until dismissed");
    assert!(!toast.action.is_visible());
    ui::assert_toast_lifecycle(&toast);
}

#[test]
fn feedback_puts_its_label_in_a_banner_with_a_tone_glyph() {
    run_gtk_test(banner_body);
}

fn banner_body() {
    if !gtk_ready() {
        return;
    }
    let column = gtk::Box::new(Orientation::Vertical, 4);
    let first = Label::new(Some("above"));
    let status = Label::new(None);
    let last = Label::new(Some("below"));
    column.append(&first);
    column.append(&status);
    column.append(&last);

    status.set_text("Saved.");
    ui::feedback(&status, false);
    let banner = ui::banner_of(&status).expect("the label sits in a banner");
    assert_eq!(banner.parent().as_ref(), Some(column.upcast_ref()));
    assert_eq!(
        banner.prev_sibling().as_ref(),
        Some(first.upcast_ref()),
        "the banner takes the label's place"
    );
    assert!(banner.has_css_class("feedback-success"));
    let glyph = banner.first_child().and_downcast::<gtk::Image>().unwrap();
    assert!(glyph.is_visible());
    assert_eq!(glyph.icon_name().as_deref(), Some("emblem-ok-symbolic"));
    assert_eq!(status.text(), "Saved.", "the message text is unchanged");

    ui::feedback(&status, true);
    assert_eq!(
        ui::banner_of(&status),
        Some(banner.clone()),
        "wrapped only once"
    );
    assert!(banner.has_css_class("feedback-error"));
    assert!(!banner.has_css_class("feedback-success"));
    assert_eq!(glyph.icon_name().as_deref(), Some("dialog-error-symbolic"));
    assert!(status.has_css_class("feedback-error"));

    ui::clear_feedback(&status);
    assert!(!banner.has_css_class("feedback-error"));
    assert!(!glyph.is_visible());

    status.set_visible(false);
    assert!(
        !banner.is_visible(),
        "the banner follows its label's visibility"
    );
    status.set_visible(true);
    assert!(banner.is_visible());
}
