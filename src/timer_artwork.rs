//! Ephemeral countdown pictures, composed through the live-information pipeline.
use crate::{ArtworkCommand, ArtworkTarget, ArtworkUi};
use sdeck_plus::info::{Glyph, InfoValue};

pub(crate) struct CountdownArtwork {
    layer: String,
    key: u8,
    seconds: u64,
}

fn format_remaining(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    }
}

impl ArtworkUi {
    fn countdown_picture(&self, layer: &str) -> Option<(ArtworkTarget, InfoValue)> {
        let info = self.info.borrow();
        let countdown = info
            .countdown
            .as_ref()
            .filter(|value| value.layer == layer)?;
        Some((
            ArtworkTarget::Keypad(countdown.key),
            InfoValue::text(Glyph::None, &format_remaining(countdown.seconds)),
        ))
    }

    pub(crate) fn start_key_countdown(&self, layer: String, key: u8, seconds: u32) {
        self.info.borrow_mut().countdown = Some(CountdownArtwork {
            layer,
            key,
            seconds: u64::from(seconds),
        });
        self.draw_countdown();
    }

    pub(crate) fn update_countdown(&self, seconds: u64) {
        let mut info = self.info.borrow_mut();
        let Some(countdown) = info.countdown.as_mut() else {
            return;
        };
        if countdown.seconds == seconds {
            return;
        }
        countdown.seconds = seconds;
        drop(info);
        self.draw_countdown();
    }

    pub(crate) fn clear_countdown(&self) {
        let previous = self.info.borrow_mut().countdown.take();
        if previous.is_some_and(|value| value.layer == self.layers.borrow().active()) {
            self.refresh(true);
        }
    }

    fn draw_countdown(&self) {
        let layer = self.layers.borrow().active().to_owned();
        if let Some((target, value)) = self.countdown_picture(&layer) {
            self.paint_info_preview(target, &value);
            let _ = self
                .output
                .send(self.info_value_command(&layer, target, value));
        }
    }

    pub(crate) fn countdown_shows(&self, layer: &str, target: ArtworkTarget) -> bool {
        self.countdown_picture(layer)
            .is_some_and(|(shown, _)| shown == target)
    }

    pub(crate) fn with_countdown(
        &self,
        layer: &str,
        mut commands: Vec<ArtworkCommand>,
    ) -> Vec<ArtworkCommand> {
        if let Some((target, value)) = self.countdown_picture(layer) {
            commands.retain(|command| match command {
                ArtworkCommand::Assign { target: shown, .. }
                | ArtworkCommand::Clear { target: shown }
                | ArtworkCommand::ComposeInfo { target: shown, .. }
                | ArtworkCommand::ComposeIcon { target: shown, .. } => *shown != target,
                ArtworkCommand::ComposeButton { key, .. } => ArtworkTarget::Keypad(*key) != target,
                _ => true,
            });
            commands.push(self.info_value_command(layer, target, value));
        }
        commands
    }

    pub(crate) fn paint_countdown_preview(&self, layer: &str) {
        if let Some((target, value)) = self.countdown_picture(layer) {
            self.paint_info_preview(target, &value);
        }
    }
}
