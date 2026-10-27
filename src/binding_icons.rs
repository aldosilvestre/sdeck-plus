//! One icon vocabulary for bindings, shared by the physical device images and
//! the GTK window.
//!
//! Every glyph is a monochrome vector path on a 24-unit grid with a 2-unit
//! round stroke, drawn with cairo. Drawing needs no icon theme, display or
//! decoder, so the same picture reaches hardware JPEGs, headless tests and
//! window widgets, and the caller chooses the ink (white on keys, the theme's
//! foreground in the window).

use crate::button_text::{ButtonText, TextOrientation, VerticalAlignment};
use crate::local_artwork::ArtworkTarget;
use crate::{
    desktop_actions::{DesktopAction, MediaCommand},
    device_control::DeviceAction,
    features::{FeatureAction, TouchscreenAction},
    layers::Action,
    obs::ObsAction,
    organization::NavigationAction,
};
use gtk::{
    cairo::{self, Context},
    gdk_pixbuf::{Colorspace, Pixbuf},
    pango::{self, prelude::FontMapExt},
};
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
    /// Whole-strip gestures, drawn before the gesture's action icon.
    SwipeLeft,
    SwipeRight,
    Tap,
    Hold,
    /// Live information readings (keys and sections showing a value).
    Volume,
    Cloud,
    PartlyCloudy,
    Rain,
    Snow,
    Storm,
    Fog,
    Cpu,
    Memory,
    Temperature,
    /// Now playing: media is playing.
    Play,
    /// Now playing: no media is playing.
    MediaIdle,
    /// The "More actions" family as a whole.
    More,
    /// Fallback for anything without a dedicated picture.
    Generic,
}

impl BindingIcon {
    pub const ALL: [Self; 53] = [
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
        Self::SwipeLeft,
        Self::SwipeRight,
        Self::Tap,
        Self::Hold,
        Self::Volume,
        Self::Cloud,
        Self::PartlyCloudy,
        Self::Rain,
        Self::Snow,
        Self::Storm,
        Self::Fog,
        Self::Cpu,
        Self::Memory,
        Self::Temperature,
        Self::Play,
        Self::MediaIdle,
        Self::More,
        Self::Generic,
    ];

    /// The picture of a live information reading: the binding icon with the
    /// same meaning where one exists (sun = brightness, muted speaker = mute,
    /// headphones = audio output, OBS states), otherwise a vocabulary icon
    /// drawn for live information. A plain clock or text reading has none.
    pub fn for_glyph(glyph: crate::info::Glyph) -> Option<Self> {
        use crate::info::Glyph;
        Some(match glyph {
            Glyph::None => return None,
            Glyph::Speaker => Self::Volume,
            Glyph::SpeakerMuted => Self::Mute,
            Glyph::Headphones => Self::AudioOutput,
            Glyph::Sun => Self::Brightness,
            Glyph::Cloud => Self::Cloud,
            Glyph::PartlyCloudy => Self::PartlyCloudy,
            Glyph::Rain => Self::Rain,
            Glyph::Snow => Self::Snow,
            Glyph::Storm => Self::Storm,
            Glyph::Fog => Self::Fog,
            Glyph::Chip => Self::Cpu,
            Glyph::Memory => Self::Memory,
            Glyph::Thermometer => Self::Temperature,
            Glyph::Record => Self::Record,
            Glyph::Live => Self::Stream,
            Glyph::ObsIdle => Self::ObsScene,
            Glyph::Play => Self::Play,
            Glyph::Pause => Self::Pause,
            Glyph::MediaIdle => Self::MediaIdle,
        })
    }

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
            Self::SwipeLeft => "swipe-left",
            Self::SwipeRight => "swipe-right",
            Self::Tap => "tap",
            Self::Hold => "hold",
            Self::Volume => "volume",
            Self::Cloud => "cloud",
            Self::PartlyCloudy => "partly-cloudy",
            Self::Rain => "rain",
            Self::Snow => "snow",
            Self::Storm => "storm",
            Self::Fog => "fog",
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Temperature => "temperature",
            Self::Play => "play",
            Self::MediaIdle => "media-idle",
            Self::More => "more",
            Self::Generic => "generic",
        }
    }
}

/// Straight (not premultiplied) red, green, blue and alpha in 0..=1.
pub type Ink = (f64, f64, f64, f64);

/// Opacity of the duotone's soft secondary layer, relative to the ink.
pub const SOFT_ALPHA: f64 = 0.35;
/// Width of the crisp primary stroke on the 24-unit grid.
const STROKE: f64 = 1.85;

/// Families of binding icons; on hardware each family has its own accent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconCategory {
    /// Pages, folders, gestures and generic pictures: neutral white.
    Navigation,
    /// Playback and sounds: violet.
    Media,
    /// Volume and outputs: cyan.
    Audio,
    /// OBS scenes, recording and streaming: red.
    Obs,
    /// Commands, applications, shortcuts and the device: amber.
    System,
    /// Websites: blue.
    Web,
    /// Weather readings: cloud blue-grey.
    Weather,
    /// Computer load readings: green.
    Monitor,
}

impl IconCategory {
    /// The accent used for this family on keys and touch sections, chosen
    /// from the `@desk_*` palette (slightly brighter for the device LCD).
    pub fn accent(self) -> Ink {
        match self {
            Self::Navigation => INK,
            Self::Media => (0.73, 0.64, 0.96, 1.0),
            Self::Audio => (0.52, 0.87, 0.92, 1.0),
            Self::Obs => (0.98, 0.45, 0.53, 1.0),
            Self::System => (0.98, 0.80, 0.48, 1.0),
            Self::Web => (0.49, 0.70, 0.98, 1.0),
            Self::Weather => (0.72, 0.79, 0.92, 1.0),
            Self::Monitor => (0.49, 0.89, 0.70, 1.0),
        }
    }
}

impl BindingIcon {
    /// The family this picture belongs to.
    pub fn category(self) -> IconCategory {
        use IconCategory as C;
        match self {
            Self::NoAction
            | Self::GoToPage
            | Self::NextPage
            | Self::PreviousPage
            | Self::Folder
            | Self::ParentFolder
            | Self::SwipeLeft
            | Self::SwipeRight
            | Self::Tap
            | Self::Hold
            | Self::More
            | Self::Generic => C::Navigation,
            Self::PlaySound
            | Self::PlayPause
            | Self::Pause
            | Self::Stop
            | Self::NextTrack
            | Self::PreviousTrack
            | Self::Play
            | Self::MediaIdle => C::Media,
            Self::VolumeUp | Self::VolumeDown | Self::Mute | Self::AudioOutput | Self::Volume => {
                C::Audio
            }
            Self::ObsScene
            | Self::Record
            | Self::Stream
            | Self::MicrophoneMute
            | Self::Visibility => C::Obs,
            Self::Command
            | Self::Application
            | Self::Shortcut
            | Self::NamedEvent
            | Self::Scroll
            | Self::Text
            | Self::OpenFile
            | Self::CloseApplication
            | Self::Toggle
            | Self::Timer
            | Self::CancelTimer
            | Self::Brightness
            | Self::Sleep => C::System,
            Self::Website => C::Web,
            Self::Cloud
            | Self::PartlyCloudy
            | Self::Rain
            | Self::Snow
            | Self::Storm
            | Self::Fog => C::Weather,
            Self::Cpu | Self::Memory | Self::Temperature => C::Monitor,
        }
    }

    /// The ink this picture uses on keys and touch sections.
    pub fn accent(self) -> Ink {
        self.category().accent()
    }
}

/// Draw `icon` centered on `center`, `size` pixels square, in `ink`, as a
/// duotone: a soft fill ([`SOFT_ALPHA`] of the ink) under a crisp round
/// stroke. `ink`'s own alpha fades the whole picture evenly.
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
    // The group keeps overlapping layers from doubling a translucent ink.
    cairo.push_group();
    cairo.translate(center.0, center.1);
    cairo.scale(size / 24.0, size / 24.0);
    cairo.set_line_width(STROKE);
    cairo.set_line_cap(cairo::LineCap::Round);
    cairo.set_line_join(cairo::LineJoin::Round);
    cairo.new_path();
    let drawn = glyph(
        cairo,
        icon,
        Pen {
            red: ink.0,
            green: ink.1,
            blue: ink.2,
        },
    );
    let picture = cairo.pop_group();
    let result = drawn.and(picture).and_then(|picture| {
        cairo.set_source(&picture)?;
        cairo.paint_with_alpha(ink.3)
    });
    cairo.restore()?;
    result
}

/// Draw `icon` like [`paint`] over a soft dark drop shadow, so it reads over
/// any key color or artwork on the device without a hard outline.
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
    with_soft_shadow(cairo, size / 24.0, |cairo| {
        paint(cairo, icon, center, size, ink)
    })
}

/// Draw with `draw`, then put a soft dark shadow under it: a halo that is
/// densest at the shape's edge and fades over about 2.5 `unit`s, dropped a
/// little downwards. `unit` is the picture's pixels per grid unit.
///
/// # Errors
/// Returns cairo drawing failures.
pub(crate) fn with_soft_shadow(
    cairo: &Context,
    unit: f64,
    draw: impl FnOnce(&Context) -> Result<(), cairo::Error>,
) -> Result<(), cairo::Error> {
    cairo.push_group();
    let drawn = draw(cairo);
    let picture = cairo.pop_group()?;
    drawn?;
    let unit = unit.max(0.75);
    for (reach, alpha) in [(0.6, 0.28), (1.3, 0.15), (2.3, 0.08)] {
        for index in 0..12 {
            let angle = f64::from(index) * TAU / 12.0;
            cairo.save()?;
            cairo.translate(
                angle.cos() * reach * unit,
                (angle.sin() * reach + 0.6) * unit,
            );
            cairo.set_source_rgba(0.0, 0.0, 0.0, alpha);
            let stamped = cairo.mask(&picture);
            cairo.restore()?;
            stamped?;
        }
    }
    cairo.set_source(&picture)?;
    cairo.paint()
}

/// Stable name of an artwork target's marker, for example `target-zone-3`.
pub fn target_marker_id(target: ArtworkTarget) -> String {
    match target {
        ArtworkTarget::Keypad(key) => format!("target-key-{key}"),
        ArtworkTarget::TouchStripBackground => "target-strip".into(),
        ArtworkTarget::EncoderZone(zone) => format!("target-zone-{zone}"),
    }
}

/// Draw a mini map of the eight keys above the four-section strip, outlined,
/// with `target` filled, so a menu row shows where its control sits.
///
/// # Errors
/// Returns cairo drawing failures.
pub fn paint_target(
    cairo: &Context,
    target: ArtworkTarget,
    center: (f64, f64),
    size: f64,
    ink: Ink,
) -> Result<(), cairo::Error> {
    const CELL: f64 = 4.6;
    const GAP: f64 = 1.33;
    const LEFT: f64 = -11.5;
    cairo.save()?;
    cairo.translate(center.0, center.1);
    cairo.scale(size / 24.0, size / 24.0);
    cairo.set_line_width(1.0);
    let mut elements = Vec::new();
    for key in 1..=8u8 {
        let column = f64::from((key - 1) % 4);
        let row = f64::from((key - 1) / 4);
        elements.push((
            ArtworkTarget::Keypad(key),
            (
                LEFT + column * (CELL + GAP),
                -10.0 + row * (CELL + GAP),
                CELL,
                CELL,
            ),
        ));
    }
    for zone in 1..=4u8 {
        let column = f64::from(zone - 1);
        elements.push((
            ArtworkTarget::EncoderZone(zone),
            (LEFT + column * (CELL + GAP), 4.0, CELL, 6.0),
        ));
    }
    let strip = target == ArtworkTarget::TouchStripBackground;
    let result = (|| {
        for (shown, (x, y, width, height)) in elements {
            let zone = matches!(shown, ArtworkTarget::EncoderZone(_));
            rounded(cairo, x, y, width, height, 1.0);
            if shown == target || (strip && zone) {
                cairo.set_source_rgba(ink.0, ink.1, ink.2, ink.3);
                cairo.fill()?;
            } else {
                // Duotone like the icons: a soft cell under a light outline.
                cairo.set_source_rgba(ink.0, ink.1, ink.2, ink.3 * SOFT_ALPHA * 0.6);
                cairo.fill_preserve()?;
                cairo.set_source_rgba(ink.0, ink.1, ink.2, ink.3 * 0.5);
                cairo.stroke()?;
            }
        }
        Ok(())
    })();
    cairo.restore()?;
    result
}

/// Key and touch-section ink: the same near-white as live information.
const INK: Ink = (0.94, 0.95, 0.98, 1.0);
const KEY: i32 = 120;
const ZONE_WIDTH: i32 = 200;
const ZONE_HEIGHT: i32 = 100;

/// Whether a key with this user text has a free band for its binding icon:
/// no text, or horizontal text aligned to the top or bottom.
pub fn key_has_room(text: Option<&ButtonText>) -> bool {
    match text.filter(|text| !text.text.is_empty()) {
        None => true,
        Some(text) => {
            text.orientation == TextOrientation::Horizontal
                && text.vertical_alignment != VerticalAlignment::Center
        }
    }
}

/// Compose the 120x120 image of a key that has no artwork of its own: the
/// binding's icon over black, with `caption` below it, or with the user's
/// button text exactly where it was placed.
///
/// Returns `None` when the user's text leaves no free band for the icon
/// (vertically centered or vertical text); such keys keep today's picture.
/// Nothing is stored: callers derive this at render time.
///
/// # Errors
/// Returns text validation, cairo or allocation failures.
pub fn compose_key_image(
    icon: BindingIcon,
    caption: &str,
    text: Option<&ButtonText>,
) -> Result<Option<Pixbuf>, String> {
    if !key_has_room(text) {
        return Ok(None);
    }
    let text = text.filter(|text| !text.text.is_empty());
    // (icon center y, icon size, caption) for each free band.
    let (center_y, size, caption) = match text {
        None => (39.0, 50.0, Some(caption)),
        Some(text) if text.orientation == TextOrientation::Vertical => return Ok(None),
        Some(text) => match text.vertical_alignment {
            VerticalAlignment::Center => return Ok(None),
            VerticalAlignment::Bottom => (39.0, 50.0, None),
            VerticalAlignment::Top => (78.0, 42.0, None),
        },
    };
    let default = ButtonText::default();
    let base = crate::image_output::compose_button_image(None, text.unwrap_or(&default))?;
    let mut surface = rgb_surface(&base)?;
    {
        let cairo = Context::new(&surface).map_err(|error| error.to_string())?;
        paint_outlined(&cairo, icon, (60.0, center_y), size, icon.accent())
            .map_err(|error| error.to_string())?;
        if let Some(caption) = caption.filter(|caption| !caption.is_empty()) {
            let layout = caption_layout(caption, 14.0, KEY - 8, 2);
            draw_caption(&cairo, &layout, (4.0, 72.0)).map_err(|error| error.to_string())?;
        }
    }
    let data = surface.data().map_err(|error| error.to_string())?;
    let mut rgb = Vec::with_capacity((KEY * KEY * 3) as usize);
    for pixel in data.as_chunks::<4>().0 {
        rgb.extend_from_slice(&u32::from_ne_bytes(*pixel).to_be_bytes()[1..]);
    }
    Ok(Some(Pixbuf::from_bytes(
        &gtk::glib::Bytes::from_owned(rgb),
        Colorspace::Rgb,
        false,
        8,
        KEY,
        KEY,
        KEY * 3,
    )))
}

/// Compose a 200x100 touch-section foreground for a section without artwork:
/// a translucent graphite tile with rounded corners (the strip background
/// shows through it and around it), a large icon on the left and up to two
/// caption lines beside it. It is drawn over the strip like section artwork.
/// `previous`/`next` reserve the outer edge where a page indicator paints.
///
/// # Errors
/// Returns cairo or allocation failures.
pub fn compose_zone_image(
    icon: BindingIcon,
    caption: &str,
    previous: bool,
    next: bool,
) -> Result<Pixbuf, String> {
    // Page indicators occupy about 32px at the section's outer edge.
    let left = if previous { 30.0 } else { 0.0 };
    let right = if next { 30 } else { 0 };
    let canvas = vec![0; (ZONE_WIDTH * ZONE_HEIGHT * 4) as usize];
    let mut surface = cairo::ImageSurface::create_for_data(
        canvas,
        cairo::Format::ARgb32,
        ZONE_WIDTH,
        ZONE_HEIGHT,
        ZONE_WIDTH * 4,
    )
    .map_err(|error| error.to_string())?;
    {
        let cairo = Context::new(&surface).map_err(|error| error.to_string())?;
        draw_zone_tile(&cairo, ZONE_TILE_ALPHA).map_err(|error| error.to_string())?;
        let size = if previous || next { 50.0 } else { 58.0 };
        let center = (left + 10.0 + size / 2.0, 50.0);
        paint_outlined(&cairo, icon, center, size, icon.accent())
            .map_err(|error| error.to_string())?;
        if !caption.is_empty() {
            let x = left + size + 22.0;
            let width = ZONE_WIDTH - right - 8 - x as i32;
            let layout = caption_layout(caption, 17.0, width, 2);
            layout.set_alignment(pango::Alignment::Left);
            let (_, height) = layout.pixel_size();
            let top = (f64::from(ZONE_HEIGHT) - f64::from(height)) / 2.0;
            draw_caption(&cairo, &layout, (x, top)).map_err(|error| error.to_string())?;
        }
    }
    let data = surface.data().map_err(|error| error.to_string())?;
    let mut rgba = Vec::with_capacity(data.len());
    for pixel in data.as_chunks::<4>().0 {
        let [alpha, red, green, blue] = u32::from_ne_bytes(*pixel).to_be_bytes();
        // Cairo stores premultiplied channels; Pixbuf expects straight RGBA.
        for channel in [red, green, blue] {
            rgba.push(if alpha == 0 {
                0
            } else {
                (u16::from(channel) * 255 / u16::from(alpha)).min(255) as u8
            });
        }
        rgba.push(alpha);
    }
    Ok(Pixbuf::from_bytes(
        &gtk::glib::Bytes::from_owned(rgba),
        Colorspace::Rgb,
        true,
        8,
        ZONE_WIDTH,
        ZONE_HEIGHT,
        ZONE_WIDTH * 4,
    ))
}

/// Section tile opacity over the plain strip (at its top; it fades downwards).
pub(crate) const ZONE_TILE_ALPHA: f64 = 0.15;
/// Section tile opacity over the section's own artwork, so the artwork shows.
pub(crate) const ZONE_TILE_ARTWORK_ALPHA: f64 = 0.10;

/// The section tile: a graphite veil like the hardware rail, clearly
/// see-through (`alpha` at the top, fading to about half towards the bottom)
/// so strip or section artwork stays visible, with a subtle light edge.
/// Legibility comes from the soft shadow of icons and text, not from the
/// fill. Binding icons and live information share it.
pub(crate) fn draw_zone_tile(cairo: &Context, alpha: f64) -> Result<(), cairo::Error> {
    rounded(cairo, 3.5, 4.5, 193.0, 91.0, 14.0);
    let fill = cairo::LinearGradient::new(0.0, 4.0, 0.0, 96.0);
    fill.add_color_stop_rgba(0.0, 0.22, 0.25, 0.30, alpha);
    fill.add_color_stop_rgba(1.0, 0.12, 0.14, 0.17, alpha * 0.5);
    cairo.set_source(&fill)?;
    cairo.fill_preserve()?;
    let edge = cairo::LinearGradient::new(0.0, 4.0, 0.0, 96.0);
    edge.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.12);
    edge.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.04);
    cairo.set_source(&edge)?;
    cairo.set_line_width(1.0);
    cairo.stroke()
}

/// An opaque RGB pixbuf as an RGB24 cairo surface.
fn rgb_surface(image: &Pixbuf) -> Result<cairo::ImageSurface, String> {
    let bytes = image.read_pixel_bytes();
    let stride = usize::try_from(image.rowstride()).map_err(|error| error.to_string())?;
    let channels = image.n_channels() as usize;
    let (width, height) = (image.width(), image.height());
    let mut canvas = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let offset = y * stride + x * channels;
            let pixel = u32::from(bytes[offset]) << 16
                | u32::from(bytes[offset + 1]) << 8
                | u32::from(bytes[offset + 2]);
            canvas.extend_from_slice(&pixel.to_ne_bytes());
        }
    }
    cairo::ImageSurface::create_for_data(canvas, cairo::Format::Rgb24, width, height, width * 4)
        .map_err(|error| error.to_string())
}

fn caption_font(size: f64) -> pango::Layout {
    // PangoCairo's default font map is per-thread; no display or GTK init is used.
    let context = pangocairo::FontMap::default().create_context();
    let layout = pango::Layout::new(&context);
    let mut font = pango::FontDescription::new();
    font.set_family("Sans");
    font.set_weight(pango::Weight::Bold);
    font.set_absolute_size(size * f64::from(pango::SCALE));
    layout.set_font_description(Some(&font));
    layout
}

/// Split an automatic caption into at most `lines` lines between words: each
/// line takes as many whole words as fit `width`; the last line takes the
/// rest. A line that is still too wide is ellipsized when drawn, so a word is
/// never broken in the middle.
fn caption_lines(text: &str, size: f64, width: i32, lines: usize) -> Vec<String> {
    let measure = caption_font(size);
    let fits = |line: &str| {
        measure.set_text(line);
        measure.pixel_size().0 <= width
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result = Vec::new();
    let mut next = 0;
    while next < words.len() && result.len() < lines {
        let last = result.len() + 1 == lines;
        let mut line = words[next].to_owned();
        next += 1;
        while next < words.len() {
            let candidate = format!("{line} {}", words[next]);
            if !last && !fits(&candidate) {
                break;
            }
            line = candidate;
            next += 1;
        }
        result.push(line);
    }
    result
}

fn caption_layout(text: &str, size: f64, width: i32, lines: i32) -> pango::Layout {
    let layout = caption_font(size);
    let lines = caption_lines(text, size, width, usize::try_from(lines).unwrap_or(1));
    layout.set_text(&lines.join("\n"));
    layout.set_width(width * pango::SCALE);
    layout.set_alignment(pango::Alignment::Center);
    // One line per paragraph: too-wide lines end in an ellipsis.
    layout.set_height(-1);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout
}

/// Caption text in the key ink over the same soft shadow as the icons.
fn draw_caption(
    cairo: &Context,
    layout: &pango::Layout,
    (x, y): (f64, f64),
) -> Result<(), cairo::Error> {
    with_soft_shadow(cairo, 1.0, |cairo| {
        cairo.move_to(x, y);
        cairo.set_source_rgba(INK.0, INK.1, INK.2, INK.3);
        pangocairo::functions::show_layout(cairo, layout);
        Ok(())
    })
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

fn closed(cairo: &Context, points: &[(f64, f64)]) {
    polyline(cairo, points);
    cairo.close_path();
}

fn circle(cairo: &Context, x: f64, y: f64, radius: f64) {
    cairo.new_sub_path();
    cairo.arc(x, y, radius, 0.0, TAU);
}

/// The two layers of a duotone glyph in one ink: a soft fill and a crisp
/// primary stroke or fill. Each call consumes the current path.
#[derive(Clone, Copy)]
struct Pen {
    red: f64,
    green: f64,
    blue: f64,
}

impl Pen {
    fn primary(self, cairo: &Context) {
        cairo.set_source_rgb(self.red, self.green, self.blue);
    }

    /// Stroke the path in the primary ink.
    fn line(self, cairo: &Context) -> Result<(), cairo::Error> {
        self.primary(cairo);
        cairo.stroke()
    }

    /// Stroke the path in the primary ink with a heavier `width`.
    fn bold(self, cairo: &Context, width: f64) -> Result<(), cairo::Error> {
        cairo.save()?;
        cairo.set_line_width(width);
        let result = self.line(cairo);
        cairo.restore()?;
        result
    }

    /// Fill the path in the primary ink.
    fn solid(self, cairo: &Context) -> Result<(), cairo::Error> {
        self.primary(cairo);
        cairo.fill()
    }

    /// Fill the path with the soft secondary layer only.
    fn soft(self, cairo: &Context) -> Result<(), cairo::Error> {
        cairo.set_source_rgba(self.red, self.green, self.blue, SOFT_ALPHA);
        cairo.fill()
    }

    /// The duotone: a soft fill under a primary outline of the same path.
    fn duo(self, cairo: &Context) -> Result<(), cairo::Error> {
        cairo.set_source_rgba(self.red, self.green, self.blue, SOFT_ALPHA);
        cairo.fill_preserve()?;
        self.line(cairo)
    }
}

/// A speaker body pointing right, its mouth at x = -1.
fn speaker(cairo: &Context, pen: Pen) -> Result<(), cairo::Error> {
    closed(
        cairo,
        &[
            (-10.5, -3.5),
            (-6.5, -3.5),
            (-1.5, -8.5),
            (-1.5, 8.5),
            (-6.5, 3.5),
            (-10.5, 3.5),
        ],
    );
    pen.duo(cairo)
}

fn stopwatch(cairo: &Context, pen: Pen) -> Result<(), cairo::Error> {
    circle(cairo, 0.0, 2.0, 8.5);
    pen.duo(cairo)?;
    polyline(cairo, &[(-3.0, -10.5), (3.0, -10.5)]);
    polyline(cairo, &[(0.0, -10.5), (0.0, -6.5)]);
    polyline(cairo, &[(6.5, -6.0), (8.0, -7.5)]);
    pen.line(cairo)
}

/// A cloud outline about 20 units wide, its flat base at y = 6.
fn cloud(cairo: &Context) {
    cairo.new_sub_path();
    cairo.move_to(-6.5, 6.0);
    cairo.line_to(7.0, 6.0);
    cairo.arc_negative(7.0, 1.5, 4.5, PI / 2.0, -PI / 2.0);
    cairo.arc_negative(1.0, -2.0, 6.0, -0.165, -PI + 0.35);
    cairo.arc_negative(-6.5, 1.5, 4.5, -PI / 2.0, -3.0 * PI / 2.0);
    cairo.close_path();
}

/// A smaller cloud raised to leave room for weather below it.
fn raised_cloud(cairo: &Context, pen: Pen) -> Result<(), cairo::Error> {
    cairo.save()?;
    cairo.translate(0.0, -4.0);
    cairo.scale(0.85, 0.85);
    cloud(cairo);
    cairo.restore()?;
    pen.duo(cairo)
}

/// A folder with its tab on the left, front edge at y = -1.5.
fn folder(cairo: &Context, pen: Pen) -> Result<(), cairo::Error> {
    closed(
        cairo,
        &[
            (-10.5, -8.0),
            (-4.0, -8.0),
            (-1.5, -5.0),
            (10.5, -5.0),
            (10.5, 8.5),
            (-10.5, 8.5),
        ],
    );
    pen.duo(cairo)?;
    polyline(cairo, &[(-10.5, -1.5), (10.5, -1.5)]);
    pen.line(cairo)
}

/// A crescent moon: the disc of radius 9 minus an offset disc.
fn crescent(cairo: &Context) {
    let (outer, (cx, cy), inner) = (9.0_f64, (5.0_f64, -4.5_f64), 7.5_f64);
    let distance = cx.hypot(cy);
    let toward = cy.atan2(cx);
    let spread =
        ((outer * outer - inner * inner + distance * distance) / (2.0 * distance * outer)).acos();
    let on_outer = |angle: f64| (outer * angle.cos(), outer * angle.sin());
    let (first, second) = (on_outer(toward + spread), on_outer(toward - spread));
    cairo.new_sub_path();
    // Around the far side of the outer disc, then back along the inner one.
    cairo.arc(0.0, 0.0, outer, toward + spread, toward - spread + TAU);
    cairo.arc_negative(
        cx,
        cy,
        inner,
        (second.1 - cy).atan2(second.0 - cx),
        (first.1 - cy).atan2(first.0 - cx),
    );
    cairo.close_path();
}

fn glyph(cairo: &Context, icon: BindingIcon, pen: Pen) -> Result<(), cairo::Error> {
    match icon {
        BindingIcon::NoAction => {
            circle(cairo, 0.0, 0.0, 9.0);
            pen.duo(cairo)?;
            polyline(cairo, &[(-6.4, 6.4), (6.4, -6.4)]);
            pen.line(cairo)
        }
        BindingIcon::GoToPage => {
            rounded(cairo, -2.0, -10.0, 12.0, 20.0, 3.0);
            pen.duo(cairo)?;
            polyline(cairo, &[(-11.0, 0.0), (4.0, 0.0)]);
            polyline(cairo, &[(0.0, -4.0), (4.0, 0.0), (0.0, 4.0)]);
            pen.line(cairo)
        }
        BindingIcon::NextPage | BindingIcon::PreviousPage => {
            let side = if icon == BindingIcon::NextPage {
                1.0
            } else {
                -1.0
            };
            circle(cairo, 0.0, 0.0, 10.5);
            pen.soft(cairo)?;
            polyline(
                cairo,
                &[(-2.5 * side, -6.0), (3.5 * side, 0.0), (-2.5 * side, 6.0)],
            );
            pen.bold(cairo, 2.4)
        }
        BindingIcon::Folder => folder(cairo, pen),
        BindingIcon::ParentFolder => {
            folder(cairo, pen)?;
            polyline(cairo, &[(0.0, 6.0), (0.0, 1.5)]);
            polyline(cairo, &[(-3.0, 4.0), (0.0, 1.0), (3.0, 4.0)]);
            pen.line(cairo)
        }
        BindingIcon::Command => {
            rounded(cairo, -11.0, -9.0, 22.0, 18.0, 3.5);
            pen.duo(cairo)?;
            polyline(cairo, &[(-6.0, -3.5), (-2.0, 0.0), (-6.0, 3.5)]);
            polyline(cairo, &[(1.0, 4.0), (6.0, 4.0)]);
            pen.line(cairo)
        }
        BindingIcon::Application => {
            for (x, y) in [(-10.5, -10.5), (1.5, -10.5), (-10.5, 1.5)] {
                rounded(cairo, x, y, 9.0, 9.0, 2.5);
            }
            pen.duo(cairo)?;
            circle(cairo, 6.0, 6.0, 4.5);
            pen.duo(cairo)
        }
        BindingIcon::Shortcut => {
            rounded(cairo, -11.0, -8.0, 22.0, 16.0, 3.5);
            pen.duo(cairo)?;
            for (x, y) in [(-6.5, -3.0), (-2.2, -3.0), (2.2, -3.0), (6.5, -3.0)] {
                circle(cairo, x, y, 1.3);
            }
            pen.solid(cairo)?;
            polyline(cairo, &[(-5.0, 3.0), (5.0, 3.0)]);
            pen.line(cairo)
        }
        BindingIcon::NamedEvent => {
            closed(
                cairo,
                &[
                    (2.5, -11.0),
                    (-7.5, 2.0),
                    (-0.5, 2.0),
                    (-2.5, 11.0),
                    (7.5, -2.0),
                    (0.5, -2.0),
                ],
            );
            pen.duo(cairo)
        }
        BindingIcon::PlaySound => {
            circle(cairo, -6.5, 6.5, 3.2);
            circle(cairo, 6.0, 4.5, 3.2);
            pen.duo(cairo)?;
            polyline(cairo, &[(-3.3, 6.5), (-3.3, -7.0)]);
            polyline(cairo, &[(9.2, 4.5), (9.2, -9.0)]);
            pen.line(cairo)?;
            polyline(cairo, &[(-3.3, -7.0), (9.2, -9.0)]);
            pen.bold(cairo, 3.2)
        }
        BindingIcon::PlayPause => {
            closed(cairo, &[(-10.5, -8.0), (-1.5, 0.0), (-10.5, 8.0)]);
            pen.duo(cairo)?;
            polyline(cairo, &[(4.0, -7.5), (4.0, 7.5)]);
            polyline(cairo, &[(9.5, -7.5), (9.5, 7.5)]);
            pen.bold(cairo, 2.6)
        }
        BindingIcon::Pause => {
            rounded(cairo, -8.0, -9.0, 5.5, 18.0, 2.0);
            rounded(cairo, 2.5, -9.0, 5.5, 18.0, 2.0);
            pen.duo(cairo)
        }
        BindingIcon::Stop => {
            rounded(cairo, -8.5, -8.5, 17.0, 17.0, 3.5);
            pen.duo(cairo)
        }
        BindingIcon::NextTrack | BindingIcon::PreviousTrack => {
            let side = if icon == BindingIcon::NextTrack {
                1.0
            } else {
                -1.0
            };
            closed(
                cairo,
                &[(-9.0 * side, -8.0), (3.5 * side, 0.0), (-9.0 * side, 8.0)],
            );
            pen.duo(cairo)?;
            polyline(cairo, &[(8.5 * side, -8.0), (8.5 * side, 8.0)]);
            pen.bold(cairo, 2.6)
        }
        BindingIcon::VolumeUp => {
            speaker(cairo, pen)?;
            polyline(cairo, &[(3.5, 0.0), (10.5, 0.0)]);
            polyline(cairo, &[(7.0, -3.5), (7.0, 3.5)]);
            pen.line(cairo)
        }
        BindingIcon::VolumeDown => {
            speaker(cairo, pen)?;
            polyline(cairo, &[(3.5, 0.0), (10.5, 0.0)]);
            pen.line(cairo)
        }
        BindingIcon::Mute => {
            speaker(cairo, pen)?;
            polyline(cairo, &[(4.0, -3.5), (11.0, 3.5)]);
            polyline(cairo, &[(11.0, -3.5), (4.0, 3.5)]);
            pen.line(cairo)
        }
        BindingIcon::AudioOutput => {
            cairo.arc(0.0, 2.0, 9.0, PI, TAU);
            pen.line(cairo)?;
            rounded(cairo, -11.0, 0.5, 6.0, 10.0, 2.5);
            rounded(cairo, 5.0, 0.5, 6.0, 10.0, 2.5);
            pen.duo(cairo)
        }
        BindingIcon::Scroll => {
            rounded(cairo, -6.5, -10.5, 13.0, 21.0, 6.5);
            pen.duo(cairo)?;
            polyline(cairo, &[(0.0, -6.5), (0.0, -3.0)]);
            pen.bold(cairo, 2.4)
        }
        BindingIcon::Text => {
            rounded(cairo, -10.5, -10.5, 21.0, 21.0, 5.0);
            pen.soft(cairo)?;
            polyline(
                cairo,
                &[(-6.0, -4.5), (-6.0, -6.0), (6.0, -6.0), (6.0, -4.5)],
            );
            polyline(cairo, &[(0.0, -6.0), (0.0, 6.5)]);
            polyline(cairo, &[(-2.5, 6.5), (2.5, 6.5)]);
            pen.bold(cairo, 2.2)
        }
        BindingIcon::OpenFile => {
            closed(
                cairo,
                &[
                    (-8.0, -10.5),
                    (2.5, -10.5),
                    (8.5, -4.5),
                    (8.5, 10.5),
                    (-8.0, 10.5),
                ],
            );
            pen.duo(cairo)?;
            polyline(cairo, &[(2.5, -10.5), (2.5, -4.5), (8.5, -4.5)]);
            polyline(cairo, &[(-4.0, 1.0), (4.5, 1.0)]);
            polyline(cairo, &[(-4.0, 5.5), (2.0, 5.5)]);
            pen.line(cairo)
        }
        BindingIcon::Website => {
            circle(cairo, 0.0, 0.0, 10.5);
            pen.duo(cairo)?;
            cairo.save()?;
            cairo.scale(0.45, 1.0);
            circle(cairo, 0.0, 0.0, 10.5);
            cairo.restore()?;
            polyline(cairo, &[(-10.5, 0.0), (10.5, 0.0)]);
            pen.line(cairo)
        }
        BindingIcon::CloseApplication => {
            rounded(cairo, -10.0, -10.0, 20.0, 20.0, 4.0);
            pen.duo(cairo)?;
            polyline(cairo, &[(-4.0, -4.0), (4.0, 4.0)]);
            polyline(cairo, &[(4.0, -4.0), (-4.0, 4.0)]);
            pen.bold(cairo, 2.2)
        }
        BindingIcon::Toggle => {
            rounded(cairo, -11.0, -6.5, 22.0, 13.0, 6.5);
            pen.duo(cairo)?;
            circle(cairo, 4.5, 0.0, 3.6);
            pen.solid(cairo)
        }
        BindingIcon::Timer => {
            stopwatch(cairo, pen)?;
            polyline(cairo, &[(0.0, 2.0), (0.0, -2.5)]);
            polyline(cairo, &[(0.0, 2.0), (3.5, 4.0)]);
            pen.line(cairo)
        }
        BindingIcon::CancelTimer => {
            stopwatch(cairo, pen)?;
            polyline(cairo, &[(-3.0, -1.0), (3.0, 5.0)]);
            polyline(cairo, &[(3.0, -1.0), (-3.0, 5.0)]);
            pen.line(cairo)
        }
        BindingIcon::Brightness => {
            circle(cairo, 0.0, 0.0, 4.5);
            pen.duo(cairo)?;
            for index in 0..8 {
                let angle = f64::from(index) * TAU / 8.0;
                polyline(
                    cairo,
                    &[
                        (angle.cos() * 7.8, angle.sin() * 7.8),
                        (angle.cos() * 10.5, angle.sin() * 10.5),
                    ],
                );
            }
            pen.line(cairo)
        }
        BindingIcon::Sleep => {
            cairo.translate(-1.0, 1.0);
            crescent(cairo);
            pen.duo(cairo)?;
            circle(cairo, 7.5, -8.0, 1.2);
            pen.solid(cairo)
        }
        BindingIcon::ObsScene => {
            rounded(cairo, -11.0, -8.5, 22.0, 17.0, 3.5);
            pen.duo(cairo)?;
            closed(cairo, &[(-3.0, -4.0), (4.5, 0.0), (-3.0, 4.0)]);
            pen.duo(cairo)
        }
        BindingIcon::Record => {
            circle(cairo, 0.0, 0.0, 10.0);
            pen.duo(cairo)?;
            circle(cairo, 0.0, 0.0, 5.0);
            pen.solid(cairo)
        }
        BindingIcon::Stream => {
            circle(cairo, 0.0, 0.0, 4.5);
            pen.soft(cairo)?;
            circle(cairo, 0.0, 0.0, 2.3);
            pen.solid(cairo)?;
            for radius in [6.5, 10.5] {
                cairo.new_sub_path();
                cairo.arc(0.0, 0.0, radius, -0.75, 0.75);
                cairo.new_sub_path();
                cairo.arc(0.0, 0.0, radius, PI - 0.75, PI + 0.75);
            }
            pen.line(cairo)
        }
        BindingIcon::MicrophoneMute => {
            rounded(cairo, -3.75, -10.5, 7.5, 13.5, 3.75);
            pen.duo(cairo)?;
            cairo.new_sub_path();
            cairo.arc(0.0, -0.5, 7.5, 0.0, PI);
            polyline(cairo, &[(0.0, 7.0), (0.0, 10.5)]);
            polyline(cairo, &[(-9.5, -9.5), (9.5, 9.5)]);
            pen.line(cairo)
        }
        BindingIcon::Visibility => {
            cairo.move_to(-11.0, 0.0);
            cairo.curve_to(-6.0, -8.5, 6.0, -8.5, 11.0, 0.0);
            cairo.curve_to(6.0, 8.5, -6.0, 8.5, -11.0, 0.0);
            cairo.close_path();
            pen.duo(cairo)?;
            circle(cairo, 0.0, 0.0, 3.3);
            pen.solid(cairo)
        }
        BindingIcon::SwipeLeft | BindingIcon::SwipeRight => {
            // A fingertip with a motion trail ahead of it.
            let side = if icon == BindingIcon::SwipeLeft {
                -1.0
            } else {
                1.0
            };
            circle(cairo, -6.5 * side, 0.0, 4.0);
            pen.duo(cairo)?;
            polyline(cairo, &[(-side, 0.0), (10.0 * side, 0.0)]);
            polyline(
                cairo,
                &[(6.0 * side, -4.5), (10.5 * side, 0.0), (6.0 * side, 4.5)],
            );
            pen.line(cairo)
        }
        BindingIcon::Tap => {
            // A fingertip with a short burst above it.
            circle(cairo, 0.0, 4.0, 5.0);
            pen.duo(cairo)?;
            for angle in [-0.75, -0.5, -0.25] {
                let (x, y) = ((angle * PI).cos(), (angle * PI).sin());
                polyline(
                    cairo,
                    &[(x * 8.0, 4.0 + y * 8.0), (x * 11.5, 4.0 + y * 11.5)],
                );
            }
            pen.line(cairo)
        }
        BindingIcon::Hold => {
            // A fingertip inside a three-quarter progress ring.
            circle(cairo, 0.0, 0.0, 4.5);
            pen.duo(cairo)?;
            circle(cairo, 0.0, 0.0, 9.5);
            pen.soft(cairo)?;
            cairo.arc(0.0, 0.0, 9.5, -PI / 2.0, PI);
            pen.bold(cairo, 2.2)
        }
        BindingIcon::Volume => {
            speaker(cairo, pen)?;
            for (radius, reach) in [(5.0, 0.8), (9.5, 0.85)] {
                cairo.new_sub_path();
                cairo.arc(0.5, 0.0, radius, -reach, reach);
            }
            pen.line(cairo)
        }
        BindingIcon::Cloud => {
            cairo.translate(0.0, 1.0);
            cloud(cairo);
            pen.duo(cairo)
        }
        BindingIcon::PartlyCloudy => {
            // The sun behind the cloud: the cloud's area is cleared first.
            cairo.push_group();
            circle(cairo, -4.5, -4.5, 3.5);
            pen.duo(cairo)?;
            for index in 0..6 {
                let angle = PI * 0.55 + f64::from(index) * PI * 0.29;
                polyline(
                    cairo,
                    &[
                        (-4.5 + angle.cos() * 6.0, -4.5 + angle.sin() * 6.0),
                        (-4.5 + angle.cos() * 8.0, -4.5 + angle.sin() * 8.0),
                    ],
                );
            }
            pen.line(cairo)?;
            cairo.save()?;
            cairo.translate(2.0, 3.0);
            cairo.scale(0.8, 0.8);
            cloud(cairo);
            cairo.restore()?;
            cairo.save()?;
            cairo.set_operator(cairo::Operator::Clear);
            cairo.fill_preserve()?;
            cairo.restore()?;
            pen.duo(cairo)?;
            cairo.pop_group_to_source()?;
            cairo.paint()
        }
        BindingIcon::Rain => {
            raised_cloud(cairo, pen)?;
            for x in [-4.0, 0.0, 4.0] {
                polyline(cairo, &[(x + 0.5, 6.5), (x - 1.5, 10.5)]);
            }
            pen.line(cairo)
        }
        BindingIcon::Snow => {
            raised_cloud(cairo, pen)?;
            // Small six-pointed flakes.
            for (x, y) in [(-5.5, 8.0), (0.0, 10.5), (5.5, 8.0)] {
                for angle in [0.0, PI / 3.0, 2.0 * PI / 3.0] {
                    let (dx, dy) = (angle.cos() * 1.7, angle.sin() * 1.7);
                    polyline(cairo, &[(x - dx, y - dy), (x + dx, y + dy)]);
                }
            }
            pen.bold(cairo, 1.3)
        }
        BindingIcon::Storm => {
            raised_cloud(cairo, pen)?;
            closed(
                cairo,
                &[
                    (1.5, 3.0),
                    (-3.0, 8.0),
                    (0.0, 8.0),
                    (-1.5, 11.5),
                    (4.0, 6.0),
                    (1.0, 6.0),
                    (3.0, 3.0),
                ],
            );
            pen.solid(cairo)
        }
        BindingIcon::Fog => {
            raised_cloud(cairo, pen)?;
            polyline(cairo, &[(-8.0, 7.0), (8.0, 7.0)]);
            polyline(cairo, &[(-5.5, 10.5), (5.5, 10.5)]);
            pen.line(cairo)
        }
        BindingIcon::Cpu => {
            rounded(cairo, -7.0, -7.0, 14.0, 14.0, 2.5);
            pen.duo(cairo)?;
            for p in [-3.5, 0.0, 3.5] {
                polyline(cairo, &[(p, -7.0), (p, -10.5)]);
                polyline(cairo, &[(p, 7.0), (p, 10.5)]);
                polyline(cairo, &[(-7.0, p), (-10.5, p)]);
                polyline(cairo, &[(7.0, p), (10.5, p)]);
            }
            pen.line(cairo)?;
            rounded(cairo, -2.75, -2.75, 5.5, 5.5, 1.0);
            pen.solid(cairo)
        }
        BindingIcon::Memory => {
            rounded(cairo, -11.0, -6.5, 22.0, 11.0, 2.0);
            pen.duo(cairo)?;
            for x in [-7.0, -3.5, 0.0, 3.5, 7.0] {
                polyline(cairo, &[(x, 4.5), (x, 8.0)]);
            }
            pen.line(cairo)?;
            for x in [-7.5, -1.5, 4.5] {
                rounded(cairo, x, -3.0, 3.0, 4.0, 0.6);
            }
            pen.solid(cairo)
        }
        BindingIcon::Temperature => {
            // A tube opening into a bulb, with the mercury inside.
            cairo.move_to(-2.5, 2.4);
            cairo.line_to(-2.5, -8.0);
            cairo.arc(0.0, -8.0, 2.5, PI, TAU);
            cairo.line_to(2.5, 2.4);
            cairo.arc(0.0, 6.0, 4.38, -0.964, PI + 0.964);
            cairo.close_path();
            pen.duo(cairo)?;
            polyline(cairo, &[(0.0, 6.0), (0.0, -4.0)]);
            pen.line(cairo)?;
            circle(cairo, 0.0, 6.0, 2.2);
            pen.solid(cairo)
        }
        BindingIcon::Play => {
            // Round corners, like the pause bars and the stop square.
            cairo.save()?;
            cairo.set_line_join(cairo::LineJoin::Round);
            closed(cairo, &[(-6.0, -9.5), (9.5, 0.0), (-6.0, 9.5)]);
            let result = pen.duo(cairo);
            cairo.restore()?;
            result
        }
        BindingIcon::MediaIdle => {
            // An eighth note, struck through like "None".
            circle(cairo, -4.5, 6.0, 3.6);
            pen.duo(cairo)?;
            polyline(cairo, &[(-0.9, 6.0), (-0.9, -9.5)]);
            cairo.curve_to(1.0, -6.0, 6.5, -5.5, 6.0, -0.5);
            pen.line(cairo)?;
            polyline(cairo, &[(-9.5, -9.5), (9.5, 9.5)]);
            pen.line(cairo)
        }
        BindingIcon::More => {
            rounded(cairo, -11.0, -5.5, 22.0, 11.0, 5.5);
            pen.soft(cairo)?;
            for x in [-6.0, 0.0, 6.0] {
                circle(cairo, x, 0.0, 2.2);
            }
            pen.solid(cairo)
        }
        BindingIcon::Generic => {
            // A four-pointed sparkle with curved sides.
            let mut from = (0.0, -11.0);
            cairo.move_to(from.0, from.1);
            for to in [(11.0, 0.0), (0.0, 11.0), (-11.0, 0.0), (0.0, -11.0)] {
                // Both handles near the quadrant's inner corner.
                let (x, y) = ((from.0 + to.0) * 0.28, (from.1 + to.1) * 0.28);
                cairo.curve_to(x, y, x, y, to.0, to.1);
                from = to;
            }
            cairo.close_path();
            pen.duo(cairo)
        }
    }
}
