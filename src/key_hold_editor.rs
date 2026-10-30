//! Optional keypad hold access without exposing the hidden physical-key selector.
use crate::{
    binding_editor::{trigger_from_selection, ControlKind, EditorError, EventKind},
    layers::Trigger,
};
use gtk::prelude::*;

pub const HOLD_LABEL: &str = "Hold 2 seconds";
pub const HOLD_HELP: &str = "Edit the optional secondary action. With a hold action assigned, releasing before 2 seconds runs only the short action; holding for 2 seconds runs only this action.";

/// Reuse the action form; toggling chooses which independent binding it edits.
/// Merely opening this control never creates a hold binding.
pub fn hold_toggle() -> gtk::CheckButton {
    let toggle = gtk::CheckButton::with_label(HOLD_LABEL);
    toggle.set_tooltip_text(Some(HOLD_HELP));
    toggle
}

/// Keep physical keys separate from the touch strip's existing hold gesture.
///
/// # Errors
/// Rejects physical key numbers outside 1 through 8 using the existing editor
/// validation, with the same error text as primary key selection.
pub fn key_trigger(number: u8, hold: bool) -> Result<Trigger, EditorError> {
    let primary = trigger_from_selection(ControlKind::Button, number, EventKind::ButtonPress)?;
    Ok(if hold {
        Trigger::KeyHold(number)
    } else {
        primary
    })
}
