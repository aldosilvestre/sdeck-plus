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
    /// The "More actions" family as a whole.
    More,
    /// Fallback for anything without a dedicated picture.
    Generic,
}

impl BindingIcon {
    pub const ALL: [Self; 41] = [
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
            Self::SwipeLeft => "swipe-left",
            Self::SwipeRight => "swipe-right",
            Self::Tap => "tap",
            Self::Hold => "hold",
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
        paint_outlined(&cairo, icon, (60.0, center_y), size, INK)
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
        draw_zone_tile(&cairo, 0.80).map_err(|error| error.to_string())?;
        let size = if previous || next { 50.0 } else { 58.0 };
        paint_outlined(&cairo, icon, (left + 10.0 + size / 2.0, 50.0), size, INK)
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

/// The section tile: graphite like the hardware rail, translucent (`alpha`)
/// so strip or section artwork stays visible, with a fine light edge. Binding
/// icons and live information share it.
pub(crate) fn draw_zone_tile(cairo: &Context, alpha: f64) -> Result<(), cairo::Error> {
    rounded(cairo, 3.5, 4.5, 193.0, 91.0, 12.0);
    let fill = cairo::LinearGradient::new(0.0, 4.0, 0.0, 96.0);
    fill.add_color_stop_rgba(0.0, 0.20, 0.23, 0.27, alpha);
    fill.add_color_stop_rgba(1.0, 0.11, 0.13, 0.16, alpha);
    cairo.set_source(&fill)?;
    cairo.fill_preserve()?;
    cairo.set_source_rgba(1.0, 1.0, 1.0, 0.16);
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

fn caption_layout(text: &str, size: f64, width: i32, lines: i32) -> pango::Layout {
    // PangoCairo's default font map is per-thread; no display or GTK init is used.
    let context = pangocairo::FontMap::default().create_context();
    let layout = pango::Layout::new(&context);
    let mut font = pango::FontDescription::new();
    font.set_family("Sans");
    font.set_weight(pango::Weight::Bold);
    font.set_absolute_size(size * f64::from(pango::SCALE));
    layout.set_font_description(Some(&font));
    layout.set_text(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    layout.set_width(width * pango::SCALE);
    layout.set_alignment(pango::Alignment::Center);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_height(-lines);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout
}

/// Caption text with the thin dark outline used by key text and live information.
fn draw_caption(
    cairo: &Context,
    layout: &pango::Layout,
    (x, y): (f64, f64),
) -> Result<(), cairo::Error> {
    cairo.move_to(x, y);
    pangocairo::functions::layout_path(cairo, layout);
    cairo.set_source_rgba(0.0, 0.0, 0.0, 0.9);
    cairo.set_line_width(3.0);
    cairo.set_line_join(cairo::LineJoin::Round);
    cairo.stroke_preserve()?;
    cairo.set_source_rgba(INK.0, INK.1, INK.2, INK.3);
    cairo.fill()
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
        BindingIcon::SwipeLeft | BindingIcon::SwipeRight => {
            // A fingertip with a motion trail behind it.
            let side = if icon == BindingIcon::SwipeLeft {
                -1.0
            } else {
                1.0
            };
            dot(cairo, -6.0 * side, 0.0, 4.0)?;
            polyline(cairo, &[(-side, 0.0), (10.0 * side, 0.0)]);
            polyline(
                cairo,
                &[(6.0 * side, -4.5), (10.5 * side, 0.0), (6.0 * side, 4.5)],
            );
            cairo.stroke()
        }
        BindingIcon::Tap => {
            dot(cairo, 0.0, 2.0, 4.0)?;
            cairo.new_sub_path();
            cairo.arc(0.0, 2.0, 9.0, PI * 1.15, PI * 1.85);
            cairo.stroke()
        }
        BindingIcon::Hold => {
            dot(cairo, 0.0, 0.0, 4.0)?;
            cairo.new_sub_path();
            cairo.arc(0.0, 0.0, 9.5, 0.0, TAU);
            cairo.stroke()
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
