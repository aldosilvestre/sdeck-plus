//! Blocking, loopback-only OBS Studio WebSocket v5 integration.
//!
//! Each operation opens an authenticated connection with a three-second total
//! network deadline. Call from a worker, never from the UI thread.

mod client;
mod config;
mod transport;

pub use client::Client;
pub use config::ObsConfig;

use crate::desktop_actions::VolumeDirection;
use serde::{Deserialize, Serialize};
use std::io;

/// A persistable OBS action. Names are passed verbatim after validation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ObsAction {
    Scene(String),
    ToggleRecording,
    ToggleStreaming,
    ToggleMute(String),
    SourceVisibility {
        scene: String,
        source: String,
        visible: bool,
    },
    StartRecording,
    StopRecording,
    PauseRecording,
    ResumeRecording,
    StartStreaming,
    StopStreaming,
    /// Raise or lower the volume of an OBS audio input by `db` decibels (1 to 20).
    InputVolume {
        input: String,
        direction: VolumeDirection,
        db: u8,
    },
}

/// Largest and smallest step of [`ObsAction::InputVolume`], in decibels.
pub const MAX_INPUT_VOLUME_STEP_DB: u8 = 20;
/// Step offered by default in the editor, in decibels.
pub const DEFAULT_INPUT_VOLUME_STEP_DB: u8 = 2;

impl ObsAction {
    /// Rejects empty, oversized, or control-character-containing names.
    ///
    /// # Errors
    /// Returns `InvalidInput` for an invalid scene or input name.
    pub fn validate(&self) -> io::Result<()> {
        match self {
            Self::Scene(name) | Self::ToggleMute(name) => validate_name(name),
            Self::InputVolume { input, db, .. } => {
                if !(1..=MAX_INPUT_VOLUME_STEP_DB).contains(db) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "OBS volume step must be between 1 and {MAX_INPUT_VOLUME_STEP_DB} dB, not {db}"
                        ),
                    ));
                }
                validate_name(input)
            }
            Self::ToggleRecording
            | Self::ToggleStreaming
            | Self::StartRecording
            | Self::StopRecording
            | Self::PauseRecording
            | Self::ResumeRecording
            | Self::StartStreaming
            | Self::StopStreaming => Ok(()),
            Self::SourceVisibility { scene, source, .. } => {
                validate_name(scene)?;
                validate_name(source)
            }
        }
    }

    /// A short description suitable for a binding label.
    pub fn summary(&self) -> String {
        match self {
            Self::Scene(scene) => format!("OBS scene: {scene}"),
            Self::ToggleRecording => "OBS toggle recording".into(),
            Self::ToggleStreaming => "OBS toggle streaming".into(),
            Self::StartRecording => "OBS start recording".into(),
            Self::StopRecording => "OBS stop recording".into(),
            Self::PauseRecording => "OBS pause recording".into(),
            Self::ResumeRecording => "OBS resume recording".into(),
            Self::StartStreaming => "OBS start streaming".into(),
            Self::StopStreaming => "OBS stop streaming".into(),
            Self::ToggleMute(input) => format!("OBS toggle mute: {input}"),
            Self::InputVolume {
                input,
                direction,
                db,
            } => format!(
                "OBS input volume {} {db} dB: {input}",
                match direction {
                    VolumeDirection::Up => "up",
                    VolumeDirection::Down => "down",
                }
            ),
            Self::SourceVisibility {
                scene,
                source,
                visible,
            } => {
                let verb = if *visible { "show" } else { "hide" };
                format!("OBS {verb}: {scene} / {source}")
            }
        }
    }
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.trim().is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid OBS name",
        ));
    }
    Ok(())
}

/// Read-only connection probe results; all fields are queried from OBS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObsStatus {
    pub scenes: Vec<String>,
    pub current_scene: String,
    pub recording: bool,
    pub streaming: bool,
}

#[cfg(test)]
mod tests;
