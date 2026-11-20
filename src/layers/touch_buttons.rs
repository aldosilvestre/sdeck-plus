//! Touches of the capacitive buttons a model reports after its keys.
use super::Trigger;
use crate::device_model::DeviceModel;

/// Recognizes touches of the capacitive buttons a model reports after its
/// keys (the Stream Deck Neo's two). A touch runs its action once, when it
/// starts: every input report repeats the state of a held touch, and a release
/// runs nothing. Touch buttons have no hold action. Owned by the input path,
/// like the key hold recognizer; it creates no thread or timer.
#[derive(Clone, Debug)]
pub struct TouchButtonPresses {
    model: &'static DeviceModel,
    down: Vec<bool>,
}

impl TouchButtonPresses {
    /// One state per touch button of `model` (none for a model without them).
    pub fn new(model: &'static DeviceModel) -> Self {
        Self {
            model,
            down: vec![false; usize::from(model.touch_buttons)],
        }
    }

    /// Feed every key report (zero-based HID index), releases included.
    /// Returns the touch button's trigger when this report starts a touch;
    /// `None` for keys, releases, a held touch reported again and indexes past
    /// the sensors.
    pub fn update(&mut self, index: usize, pressed: bool) -> Option<Trigger> {
        let number = self.model.touch_button_at(index)?;
        let down = self.down.get_mut(usize::from(number) - 1)?;
        let started = pressed && !*down;
        *down = pressed;
        started.then_some(Trigger::TouchButton(number))
    }
}
