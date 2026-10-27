//! One icon vocabulary for bindings, shared by the physical device images and
//! the GTK window.
//!
//! Every glyph is a monochrome vector path on a 24-unit grid with a 2-unit
//! round stroke, drawn with cairo. Drawing needs no icon theme, display or
//! decoder, so the same picture reaches hardware JPEGs, headless tests and
//! window widgets, and the caller chooses the ink (white on keys, the theme's
//! foreground in the window).

use crate::{
    desktop_actions::{DesktopAction, MediaCommand},
    device_control::DeviceAction,
    features::{FeatureAction, TouchscreenAction},
    layers::Action,
    obs::ObsAction,
    organization::NavigationAction,
};
use gtk::cairo::{self, Context};
use std::f64::consts::{PI, TAU};

#[cfg(test)]
mod tests;

/// What a binding does, as a recognizable picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BindingIcon {
    /// Explicitly does nothing. Shown in menus only, never on controls.
    NoAction,
    GoToPage,
    NextPage,
    PreviousPage,
    Folder,
    ParentFolder,
    Command,
    Application,
    Shortcut,
    NamedEvent,
    PlaySound,
    PlayPause,
    Pause,
    Stop,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
    Mute,
    AudioOutput,
    Scroll,
    Text,
    OpenFile,
    Website,
    CloseApplication,
    Toggle,
    Timer,
    CancelTimer,
    Brightness,
    Sleep,
    ObsScene,
    Record,
    Stream,
    MicrophoneMute,
    Visibility,
    /// The "More actions" family as a whole.
    More,
    /// Fallback for anything without a dedicated picture.
    Generic,
}

impl BindingIcon {
    pub const ALL: [Self; 37] = [
        Self::NoAction,
        Self::GoToPage,
        Self::NextPage,
        Self::PreviousPage,
        Self::Folder,
        Self::ParentFolder,
        Self::Command,
        Self::Application,
        Self::Shortcut,
        Self::NamedEvent,
        Self::PlaySound,
        Self::PlayPause,
        Self::Pause,
        Self::Stop,
        Self::NextTrack,
        Self::PreviousTrack,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Mute,
        Self::AudioOutput,
        Self::Scroll,
        Self::Text,
        Self::OpenFile,
        Self::Website,
        Self::CloseApplication,
        Self::Toggle,
        Self::Timer,
        Self::CancelTimer,
        Self::Brightness,
        Self::Sleep,
        Self::ObsScene,
        Self::Record,
        Self::Stream,
        Self::MicrophoneMute,
        Self::Visibility,
        Self::More,
        Self::Generic,
    ];

    /// The picture for a saved binding.
    pub fn for_action(action: &Action) -> Self {
        match action {
            Action::None => Self::NoAction,
            Action::SwitchLayer(_) => Self::GoToPage,
            Action::Command(_) => Self::Command,
            Action::Launch { .. } => Self::Application,
            Action::KeyPress { .. } | Action::Shortcut { .. } => Self::Shortcut,
            Action::Emit(_) => Self::NamedEvent,
            Action::Feature(action) => Self::for_feature(action),
        }
    }

    fn for_feature(action: &FeatureAction) -> Self {
        match action {
            FeatureAction::Desktop(action) => match action {
                DesktopAction::ShortcutToggle { .. } => Self::Toggle,
                DesktopAction::Text { .. } => Self::Text,
                DesktopAction::Open { .. } => Self::OpenFile,
                DesktopAction::Website { .. } => Self::Website,
                DesktopAction::CloseApplication { .. } => Self::CloseApplication,
                DesktopAction::Media(command) => match command {
                    MediaCommand::PlayPause => Self::PlayPause,
                    MediaCommand::Stop => Self::Stop,
                    MediaCommand::Next => Self::NextTrack,
                    MediaCommand::Previous => Self::PreviousTrack,
                    MediaCommand::VolumeUp => Self::VolumeUp,
                    MediaCommand::VolumeDown => Self::VolumeDown,
                    MediaCommand::Mute => Self::Mute,
                },
                DesktopAction::PlayAudio { .. } => Self::PlaySound,
                DesktopAction::StopAudio => Self::Stop,
                DesktopAction::SetAudioOutput { .. } | DesktopAction::CycleAudioOutput => {
                    Self::AudioOutput
                }
                DesktopAction::MouseScroll { .. } => Self::Scroll,
            },
            FeatureAction::Navigation(action) => match action {
                NavigationAction::NextPage => Self::NextPage,
                NavigationAction::PreviousPage => Self::PreviousPage,
                NavigationAction::GoToPage(_) => Self::GoToPage,
                NavigationAction::OpenFolder(_) => Self::Folder,
                NavigationAction::ParentFolder => Self::ParentFolder,
            },
            FeatureAction::Touchscreen(TouchscreenAction::NextPage) => Self::NextPage,
            FeatureAction::Touchscreen(TouchscreenAction::PreviousPage) => Self::PreviousPage,
            FeatureAction::Device(DeviceAction::Sleep) => Self::Sleep,
            FeatureAction::Device(_) => Self::Brightness,
            FeatureAction::Obs(action) => match action {
                ObsAction::Scene(_) => Self::ObsScene,
                ObsAction::ToggleRecording
                | ObsAction::StartRecording
                | ObsAction::ResumeRecording => Self::Record,
                ObsAction::PauseRecording => Self::Pause,
                ObsAction::StopRecording | ObsAction::StopStreaming => Self::Stop,
                ObsAction::ToggleStreaming | ObsAction::StartStreaming => Self::Stream,
                ObsAction::ToggleMute(_) => Self::MicrophoneMute,
                ObsAction::SourceVisibility { .. } => Self::Visibility,
            },
            FeatureAction::Timer { .. } => Self::Timer,
            FeatureAction::CancelTimer => Self::CancelTimer,
        }
    }

    /// The picture for one row of the editor's Action menu, in its order:
    /// Go to profile or page, Run command, Open application, Shortcut, Send
    /// named event, More actions, Play sound, None.
    pub fn for_action_type(index: u32) -> Self {
        match index {
            0 => Self::GoToPage,
            1 => Self::Command,
            2 => Self::Application,
            3 => Self::Shortcut,
            4 => Self::NamedEvent,
            5 => Self::More,
            6 => Self::PlaySound,
            7 => Self::NoAction,
            _ => Self::Generic,
        }
    }

    /// Whether keys, dials and touch sections show this icon. "None" keeps a
    /// control visually empty.
    pub fn shown_on_controls(self) -> bool {
        self != Self::NoAction
    }

    /// Stable, human-readable identifier, also used as the widget name.
    pub fn id(self) -> &'static str {
        match self {
            Self::NoAction => "none",
            Self::GoToPage => "go-to-page",
            Self::NextPage => "next-page",
            Self::PreviousPage => "previous-page",
            Self::Folder => "folder",
            Self::ParentFolder => "parent-folder",
            Self::Command => "command",
            Self::Application => "application",
            Self::Shortcut => "shortcut",
            Self::NamedEvent => "named-event",
            Self::PlaySound => "play-sound",
            Self::PlayPause => "play-pause",
            Self::Pause => "pause",
            Self::Stop => "stop",
            Self::NextTrack => "next-track",
            Self::PreviousTrack => "previous-track",
            Self::VolumeUp => "volume-up",
            Self::VolumeDown => "volume-down",
            Self::Mute => "mute",
            Self::AudioOutput => "audio-output",
            Self::Scroll => "scroll",
            Self::Text => "text",
            Self::OpenFile => "open-file",
            Self::Website => "website",
            Self::CloseApplication => "close-application",
            Self::Toggle => "toggle",
            Self::Timer => "timer",
            Self::CancelTimer => "cancel-timer",
            Self::Brightness => "brightness",
            Self::Sleep => "sleep",
            Self::ObsScene => "obs-scene",
            Self::Record => "record",
            Self::Stream => "stream",
            Self::MicrophoneMute => "microphone-mute",
            Self::Visibility => "visibility",
            Self::More => "more",
            Self::Generic => "generic",
        }
    }
}

/// Straight (not premultiplied) red, green, blue and alpha in 0..=1.
pub type Ink = (f64, f64, f64, f64);

/// Draw `icon` centered on `center`, `size` pixels square, in `ink`.
///
/// # Errors
/// Returns cairo drawing failures.
pub fn paint(
    cairo: &Context,
    icon: BindingIcon,
    center: (f64, f64),
    size: f64,
    ink: Ink,
) -> Result<(), cairo::Error> {
    cairo.save()?;
    cairo.translate(center.0, center.1);
    cairo.scale(size / 24.0, size / 24.0);
    cairo.set_source_rgba(ink.0, ink.1, ink.2, ink.3);
    cairo.set_line_width(2.0);
    cairo.set_line_cap(cairo::LineCap::Round);
    cairo.set_line_join(cairo::LineJoin::Round);
    cairo.new_path();
    let result = glyph(cairo, icon);
    cairo.restore()?;
    result
}

/// Draw `icon` like [`paint`], surrounded by a soft dark outline so it reads
/// over any key color or artwork.
///
/// # Errors
/// Returns cairo drawing failures.
pub fn paint_outlined(
    cairo: &Context,
    icon: BindingIcon,
    center: (f64, f64),
    size: f64,
    ink: Ink,
) -> Result<(), cairo::Error> {
    cairo.push_group();
    paint(cairo, icon, center, size, ink)?;
    let picture = cairo.pop_group()?;
    let reach = (size / 24.0).max(1.0) * 1.6;
    for index in 0..12 {
        let angle = f64::from(index) * TAU / 12.0;
        cairo.save()?;
        cairo.translate(angle.cos() * reach, angle.sin() * reach);
        cairo.set_source_rgba(0.0, 0.0, 0.0, 0.8);
        cairo.mask(&picture)?;
        cairo.restore()?;
    }
    cairo.set_source(&picture)?;
    cairo.paint()
}

fn rounded(cairo: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    cairo.new_sub_path();
    cairo.arc(x + width - radius, y + radius, radius, -PI / 2.0, 0.0);
    cairo.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        PI / 2.0,
    );
    cairo.arc(x + radius, y + height - radius, radius, PI / 2.0, PI);
    cairo.arc(x + radius, y + radius, radius, PI, 3.0 * PI / 2.0);
    cairo.close_path();
}

fn polyline(cairo: &Context, points: &[(f64, f64)]) {
    if let Some((&(x, y), rest)) = points.split_first() {
        cairo.move_to(x, y);
        for &(x, y) in rest {
            cairo.line_to(x, y);
        }
    }
}

fn polygon(cairo: &Context, points: &[(f64, f64)]) -> Result<(), cairo::Error> {
    polyline(cairo, points);
    cairo.close_path();
    cairo.fill()
}

fn dot(cairo: &Context, x: f64, y: f64, radius: f64) -> Result<(), cairo::Error> {
    cairo.new_sub_path();
    cairo.arc(x, y, radius, 0.0, TAU);
    cairo.fill()
}

fn speaker(cairo: &Context) -> Result<(), cairo::Error> {
    polygon(
        cairo,
        &[
            (-11.0, -4.0),
            (-6.0, -4.0),
            (0.0, -9.5),
            (0.0, 9.5),
            (-6.0, 4.0),
            (-11.0, 4.0),
        ],
    )
}

fn stopwatch(cairo: &Context) -> Result<(), cairo::Error> {
    cairo.new_sub_path();
    cairo.arc(0.0, 2.0, 8.5, 0.0, TAU);
    polyline(cairo, &[(-3.0, -10.0), (3.0, -10.0)]);
    polyline(cairo, &[(0.0, -10.0), (0.0, -6.5)]);
    cairo.stroke()
}

fn glyph(cairo: &Context, icon: BindingIcon) -> Result<(), cairo::Error> {
    match icon {
        BindingIcon::NoAction => {
            cairo.arc(0.0, 0.0, 9.0, 0.0, TAU);
            polyline(cairo, &[(-6.4, 6.4), (6.4, -6.4)]);
            cairo.stroke()
        }
        BindingIcon::GoToPage => {
            rounded(cairo, -2.0, -10.0, 12.0, 20.0, 2.5);
            polyline(cairo, &[(-11.0, 0.0), (4.0, 0.0)]);
            polyline(cairo, &[(0.0, -4.0), (4.0, 0.0), (0.0, 4.0)]);
            cairo.stroke()
        }
        BindingIcon::NextPage => {
            cairo.set_line_width(2.6);
            polyline(cairo, &[(-4.0, -8.0), (4.0, 0.0), (-4.0, 8.0)]);
            cairo.stroke()
        }
        BindingIcon::PreviousPage => {
            cairo.set_line_width(2.6);
            polyline(cairo, &[(4.0, -8.0), (-4.0, 0.0), (4.0, 8.0)]);
            cairo.stroke()
        }
        BindingIcon::Folder => {
            polyline(
                cairo,
                &[
                    (-10.0, -8.0),
                    (-3.5, -8.0),
                    (-1.0, -5.0),
                    (10.0, -5.0),
                    (10.0, 8.0),
                    (-10.0, 8.0),
                ],
            );
            cairo.close_path();
            cairo.stroke()
        }
        BindingIcon::ParentFolder => {
            polyline(cairo, &[(0.0, 10.0), (0.0, -8.0)]);
            polyline(cairo, &[(-7.0, -1.0), (0.0, -8.0), (7.0, -1.0)]);
            cairo.stroke()
        }
        BindingIcon::Command => {
            rounded(cairo, -11.0, -9.0, 22.0, 18.0, 3.0);
            polyline(cairo, &[(-6.0, -3.5), (-2.0, 0.0), (-6.0, 3.5)]);
            polyline(cairo, &[(1.0, 4.0), (6.0, 4.0)]);
            cairo.stroke()
        }
        BindingIcon::Application => {
            for (x, y) in [(-10.0, -10.0), (1.5, -10.0), (-10.0, 1.5), (1.5, 1.5)] {
                rounded(cairo, x, y, 8.5, 8.5, 2.0);
            }
            cairo.stroke()
        }
        BindingIcon::Shortcut => {
            rounded(cairo, -11.0, -7.5, 22.0, 15.0, 2.5);
            cairo.stroke()?;
            for x in [-6.5, -2.2, 2.2, 6.5] {
                cairo.rectangle(x - 1.2, -3.7, 2.4, 2.4);
            }
            cairo.fill()?;
            polyline(cairo, &[(-5.0, 3.0), (5.0, 3.0)]);
            cairo.stroke()
        }
        BindingIcon::NamedEvent => polygon(
            cairo,
            &[
                (2.5, -11.0),
                (-7.0, 1.5),
                (-1.0, 1.5),
                (-3.0, 11.0),
                (7.0, -2.0),
                (1.0, -2.0),
            ],
        ),
        BindingIcon::PlaySound => {
            dot(cairo, -4.0, 6.5, 3.6)?;
            polyline(
                cairo,
                &[(-0.6, 6.5), (-0.6, -10.0), (7.0, -6.5), (7.0, -2.5)],
            );
            cairo.stroke()
        }
        BindingIcon::PlayPause => {
            polygon(cairo, &[(-10.0, -8.0), (-1.5, 0.0), (-10.0, 8.0)])?;
            cairo.set_line_width(2.6);
            polyline(cairo, &[(3.5, -8.0), (3.5, 8.0)]);
            polyline(cairo, &[(9.0, -8.0), (9.0, 8.0)]);
            cairo.stroke()
        }
        BindingIcon::Pause => {
            rounded(cairo, -7.0, -8.0, 4.5, 16.0, 1.2);
            rounded(cairo, 2.5, -8.0, 4.5, 16.0, 1.2);
            cairo.fill()
        }
        BindingIcon::Stop => {
            rounded(cairo, -7.5, -7.5, 15.0, 15.0, 2.0);
            cairo.fill()
        }
        BindingIcon::NextTrack => {
            polygon(cairo, &[(-8.0, -8.0), (4.0, 0.0), (-8.0, 8.0)])?;
            cairo.set_line_width(2.6);
            polyline(cairo, &[(7.5, -8.0), (7.5, 8.0)]);
            cairo.stroke()
        }
        BindingIcon::PreviousTrack => {
            polygon(cairo, &[(8.0, -8.0), (-4.0, 0.0), (8.0, 8.0)])?;
            cairo.set_line_width(2.6);
            polyline(cairo, &[(-7.5, -8.0), (-7.5, 8.0)]);
            cairo.stroke()
        }
        BindingIcon::VolumeUp => {
            speaker(cairo)?;
            polyline(cairo, &[(3.5, 0.0), (11.0, 0.0)]);
            polyline(cairo, &[(7.25, -3.75), (7.25, 3.75)]);
            cairo.stroke()
        }
        BindingIcon::VolumeDown => {
            speaker(cairo)?;
            polyline(cairo, &[(3.5, 0.0), (11.0, 0.0)]);
            cairo.stroke()
        }
        BindingIcon::Mute => {
            speaker(cairo)?;
            polyline(cairo, &[(4.0, -3.5), (11.0, 3.5)]);
            polyline(cairo, &[(11.0, -3.5), (4.0, 3.5)]);
            cairo.stroke()
        }
        BindingIcon::AudioOutput => {
            cairo.arc(0.0, 2.0, 9.0, PI, TAU);
            cairo.stroke()?;
            rounded(cairo, -11.0, 1.0, 5.5, 9.5, 1.5);
            rounded(cairo, 5.5, 1.0, 5.5, 9.5, 1.5);
            cairo.fill()
        }
        BindingIcon::Scroll => {
            rounded(cairo, -6.5, -10.0, 13.0, 20.0, 6.5);
            polyline(cairo, &[(0.0, -6.0), (0.0, -2.0)]);
            cairo.stroke()
        }
        BindingIcon::Text => {
            cairo.set_line_width(2.6);
            polyline(
                cairo,
                &[(-8.0, -6.5), (-8.0, -9.0), (8.0, -9.0), (8.0, -6.5)],
            );
            polyline(cairo, &[(0.0, -9.0), (0.0, 9.0)]);
            polyline(cairo, &[(-3.5, 9.0), (3.5, 9.0)]);
            cairo.stroke()
        }
        BindingIcon::OpenFile => {
            polyline(
                cairo,
                &[
                    (-7.5, -10.5),
                    (3.0, -10.5),
                    (8.5, -5.0),
                    (8.5, 10.5),
                    (-7.5, 10.5),
                ],
            );
            cairo.close_path();
            polyline(cairo, &[(3.0, -10.5), (3.0, -5.0), (8.5, -5.0)]);
            cairo.stroke()
        }
        BindingIcon::Website => {
            cairo.arc(0.0, 0.0, 10.0, 0.0, TAU);
            cairo.stroke()?;
            cairo.save()?;
            cairo.scale(0.45, 1.0);
            cairo.arc(0.0, 0.0, 10.0, 0.0, TAU);
            cairo.restore()?;
            polyline(cairo, &[(-10.0, 0.0), (10.0, 0.0)]);
            polyline(cairo, &[(-8.5, -5.0), (8.5, -5.0)]);
            polyline(cairo, &[(-8.5, 5.0), (8.5, 5.0)]);
            cairo.stroke()
        }
        BindingIcon::CloseApplication => {
            rounded(cairo, -10.0, -10.0, 20.0, 20.0, 3.0);
            polyline(cairo, &[(-4.0, -4.0), (4.0, 4.0)]);
            polyline(cairo, &[(4.0, -4.0), (-4.0, 4.0)]);
            cairo.stroke()
        }
        BindingIcon::Toggle => {
            polyline(cairo, &[(-9.0, -4.5), (8.0, -4.5)]);
            polyline(cairo, &[(4.0, -8.5), (8.0, -4.5), (4.0, -0.5)]);
            polyline(cairo, &[(9.0, 4.5), (-8.0, 4.5)]);
            polyline(cairo, &[(-4.0, 0.5), (-8.0, 4.5), (-4.0, 8.5)]);
            cairo.stroke()
        }
        BindingIcon::Timer => {
            stopwatch(cairo)?;
            polyline(cairo, &[(0.0, 2.0), (0.0, -2.5)]);
            polyline(cairo, &[(0.0, 2.0), (3.5, 4.0)]);
            cairo.stroke()
        }
        BindingIcon::CancelTimer => {
            stopwatch(cairo)?;
            polyline(cairo, &[(-3.5, -1.5), (3.5, 5.5)]);
            polyline(cairo, &[(3.5, -1.5), (-3.5, 5.5)]);
            cairo.stroke()
        }
        BindingIcon::Brightness => {
            dot(cairo, 0.0, 0.0, 4.5)?;
            for index in 0..8 {
                let angle = f64::from(index) * TAU / 8.0;
                polyline(
                    cairo,
                    &[
                        (angle.cos() * 7.5, angle.sin() * 7.5),
                        (angle.cos() * 10.5, angle.sin() * 10.5),
                    ],
                );
            }
            cairo.stroke()
        }
        BindingIcon::Sleep => {
            // A crescent: the full disc minus an offset disc.
            cairo.push_group();
            dot(cairo, 0.0, 0.0, 9.5)?;
            cairo.set_operator(cairo::Operator::Clear);
            dot(cairo, 5.5, -4.5, 8.0)?;
            cairo.set_operator(cairo::Operator::Over);
            cairo.pop_group_to_source()?;
            cairo.paint()
        }
        BindingIcon::ObsScene => {
            rounded(cairo, -11.0, -8.5, 22.0, 17.0, 2.5);
            cairo.stroke()?;
            polygon(cairo, &[(-3.0, -4.5), (4.5, 0.0), (-3.0, 4.5)])
        }
        BindingIcon::Record => {
            cairo.arc(0.0, 0.0, 10.0, 0.0, TAU);
            cairo.stroke()?;
            dot(cairo, 0.0, 0.0, 5.5)
        }
        BindingIcon::Stream => {
            dot(cairo, 0.0, 0.0, 2.6)?;
            for radius in [6.0, 10.5] {
                cairo.new_sub_path();
                cairo.arc(0.0, 0.0, radius, -0.75, 0.75);
                cairo.new_sub_path();
                cairo.arc(0.0, 0.0, radius, PI - 0.75, PI + 0.75);
            }
            cairo.stroke()
        }
        BindingIcon::MicrophoneMute => {
            rounded(cairo, -4.0, -10.5, 8.0, 13.5, 4.0);
            cairo.new_sub_path();
            cairo.arc(0.0, -0.5, 7.5, 0.0, PI);
            polyline(cairo, &[(0.0, 7.0), (0.0, 10.5)]);
            polyline(cairo, &[(-9.5, -9.5), (9.5, 9.5)]);
            cairo.stroke()
        }
        BindingIcon::Visibility => {
            cairo.move_to(-11.0, 0.0);
            cairo.curve_to(-6.0, -8.0, 6.0, -8.0, 11.0, 0.0);
            cairo.curve_to(6.0, 8.0, -6.0, 8.0, -11.0, 0.0);
            cairo.close_path();
            cairo.stroke()?;
            dot(cairo, 0.0, 0.0, 3.2)
        }
        BindingIcon::More => {
            for x in [-7.5, 0.0, 7.5] {
                dot(cairo, x, 0.0, 2.6)?;
            }
            Ok(())
        }
        BindingIcon::Generic => {
            cairo.arc(0.0, 0.0, 9.5, 0.0, TAU);
            cairo.stroke()?;
            dot(cairo, 0.0, 0.0, 3.5)
        }
    }
}
