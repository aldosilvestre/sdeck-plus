//! Reusable keypad text configuration shared by preview and device rendering.

use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HorizontalAlignment {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerticalAlignment {
    Top,
    #[default]
    Center,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonText {
    pub text: String,
    pub font_family: String,
    pub font_size: u16,
    pub orientation: TextOrientation,
    pub horizontal_alignment: HorizontalAlignment,
    pub vertical_alignment: VerticalAlignment,
}

impl Default for ButtonText {
    fn default() -> Self {
        Self {
            text: String::new(),
            font_family: "Sans".into(),
            font_size: 24,
            orientation: TextOrientation::Horizontal,
            horizontal_alignment: HorizontalAlignment::Center,
            vertical_alignment: VerticalAlignment::Center,
        }
    }
}

impl ButtonText {
    /// Validate configuration at storage and rendering boundaries.
    pub fn validate(&self) -> io::Result<()> {
        if !(6..=120).contains(&self.font_size) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "button font size must be 6..=120 pixels",
            ));
        }
        if self.font_family.trim().is_empty() || self.font_family.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "button font family must be nonempty and contain no NUL",
            ));
        }
        if self.text.len() > 2048 || self.text.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "button text must be at most 2048 bytes and contain no NUL",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_validation_boundaries() {
        let mut text = ButtonText::default();
        assert_eq!(text.font_family, "Sans");
        assert_eq!(text.font_size, 24);
        assert_eq!(text.orientation, TextOrientation::Horizontal);
        assert_eq!(text.horizontal_alignment, HorizontalAlignment::Center);
        assert_eq!(text.vertical_alignment, VerticalAlignment::Center);
        for size in [6, 120] {
            text.font_size = size;
            text.validate().unwrap();
        }
        for size in [0, 5, 121, u16::MAX] {
            text.font_size = size;
            assert!(text.validate().is_err());
        }
        text.font_size = 24;
        text.font_family = " \t".into();
        assert!(text.validate().is_err());
        text.font_family = "Sans".into();
        text.text = "é".repeat(1024);
        text.validate().unwrap();
        text.text.push('x');
        assert!(text.validate().is_err());
    }
}
