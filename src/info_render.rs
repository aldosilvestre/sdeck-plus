//! Draws a live information value (icon, bar and text) as a 120x120 key image.

use crate::{
    binding_icons::{self, BindingIcon},
    image_output::{resize_background, ImageDimensions},
    info::{Glyph, InfoValue},
};
use gtk::{
    cairo::{self, Context},
    gdk_pixbuf::{Colorspace, Pixbuf},
    pango::{self, prelude::FontMapExt},
};
use std::{f64::consts::PI, path::Path};

type Color = (f64, f64, f64);

const SIZE: i32 = 120;
const ZONE_WIDTH: i32 = 200;
const ZONE_HEIGHT: i32 = 100;
const WHITE: Color = (0.94, 0.95, 0.98);
const CYAN: Color = (0.40, 0.91, 0.98);
const AMBER: Color = (0.98, 0.80, 0.48);
const GREEN: Color = (0.49, 0.89, 0.70);
const RED: Color = (0.98, 0.44, 0.52);
const VIOLET: Color = (0.65, 0.55, 0.98);
const BLUE: Color = (0.40, 0.65, 0.98);
const CLOUD: Color = (0.69, 0.75, 0.88);
const GREY: Color = (0.45, 0.50, 0.62);

/// Compose one physical key image: the live picture drawn directly over the
/// key's own artwork (transparent without one), with no background of its own.
///
/// # Errors
/// Returns background decoding, cairo or surface allocation failures.
pub fn compose_info_image(value: &InfoValue, background: Option<&Path>) -> Result<Pixbuf, String> {
    compose(value, SIZE, SIZE, background, None)
}

/// Compose one 200x100 touch-strip section: the section's own artwork (or
/// transparency, so the strip shows) under the translucent section tile that
/// binding icons use, with the icon on the left and the value, bar and detail
/// beside it. `previous`/`next` keep the outer edge free for the page
/// indicator that paints over the section afterwards.
///
/// # Errors
/// Returns background decoding, cairo or surface allocation failures.
pub fn compose_zone_image(
    value: &InfoValue,
    background: Option<&Path>,
    previous: bool,
    next: bool,
) -> Result<Pixbuf, String> {
    compose(
        value,
        ZONE_WIDTH,
        ZONE_HEIGHT,
        background,
        Some((previous, next)),
    )
}

/// Native-endian premultiplied ARGB pixels: the artwork, or transparent.
fn backdrop(width: i32, height: i32, source: Option<&Path>) -> Result<Vec<u8>, String> {
    let columns = usize::try_from(width).map_err(|error| error.to_string())?;
    let rows = usize::try_from(height).map_err(|error| error.to_string())?;
    let Some(source) = source else {
        return Ok(vec![0; columns * rows * 4]);
    };
    let image = resize_background(
        ImageDimensions {
            width: u32::try_from(width).map_err(|error| error.to_string())?,
            height: u32::try_from(height).map_err(|error| error.to_string())?,
        },
        source,
    )?;
    let bytes = image.read_pixel_bytes();
    let stride = usize::try_from(image.rowstride()).map_err(|error| error.to_string())?;
    let channels = image.n_channels() as usize;
    let mut canvas = Vec::with_capacity(columns * rows * 4);
    for y in 0..rows {
        for x in 0..columns {
            let offset = y * stride + x * channels;
            let alpha = if image.has_alpha() {
                bytes[offset + 3]
            } else {
                255
            };
            let premultiply = |channel| u32::from(bytes[offset + channel]) * u32::from(alpha) / 255;
            let pixel = u32::from(alpha) << 24
                | premultiply(0) << 16
                | premultiply(1) << 8
                | premultiply(2);
            canvas.extend_from_slice(&pixel.to_ne_bytes());
        }
    }
    Ok(canvas)
}

fn compose(
    value: &InfoValue,
    width: i32,
    height: i32,
    background: Option<&Path>,
    section: Option<(bool, bool)>,
) -> Result<Pixbuf, String> {
    let canvas = backdrop(width, height, background)?;
    let mut surface = cairo::ImageSurface::create_for_data(
        canvas,
        cairo::Format::ARgb32,
        width,
        height,
        width * 4,
    )
    .map_err(|error| error.to_string())?;
    {
        let cairo = Context::new(&surface).map_err(|error| error.to_string())?;
        match section {
            Some((previous, next)) => {
                // Over artwork the tile is lighter so the artwork stays visible.
                let alpha = if background.is_some() {
                    crate::binding_icons::ZONE_TILE_ARTWORK_ALPHA
                } else {
                    crate::binding_icons::ZONE_TILE_ALPHA
                };
                crate::binding_icons::draw_zone_tile(&cairo, alpha)
                    .and_then(|()| draw_parts(&cairo, value, &zone_plan(value, previous, next)))
            }
            None => draw(&cairo, value),
        }
        .map_err(|error| error.to_string())?;
    }
    let data = surface.data().map_err(|error| error.to_string())?;
    let mut rgba = Vec::new();
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
        width,
        height,
        width * 4,
    ))
}

fn set(cairo: &Context, color: Color) {
    cairo.set_source_rgb(color.0, color.1, color.2);
}

fn accent(value: &InfoValue) -> Color {
    if value.inactive {
        return GREY;
    }
    let level = value.level.unwrap_or(0);
    let load = |calm: Color| match level {
        85.. => RED,
        70..=84 => AMBER,
        _ => calm,
    };
    match value.glyph {
        Glyph::None => WHITE,
        Glyph::Speaker | Glyph::SpeakerMuted | Glyph::Headphones => CYAN,
        Glyph::Sun | Glyph::PartlyCloudy | Glyph::Storm => AMBER,
        Glyph::Cloud | Glyph::Fog => CLOUD,
        Glyph::Rain => BLUE,
        Glyph::Snow => WHITE,
        Glyph::Chip | Glyph::Memory => load(VIOLET),
        Glyph::Thermometer => load(GREEN),
        Glyph::Record | Glyph::Live => RED,
        Glyph::ObsIdle => GREY,
    }
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

fn make_layout(
    text: &str,
    size: f64,
    bold: bool,
    width: i32,
    alignment: pango::Alignment,
) -> pango::Layout {
    // PangoCairo's default font map is per-thread; no display or GTK init is used.
    let context = pangocairo::FontMap::default().create_context();
    let layout = pango::Layout::new(&context);
    let mut font = pango::FontDescription::new();
    font.set_family("Sans");
    if bold {
        font.set_weight(pango::Weight::Bold);
    }
    font.set_absolute_size(size * f64::from(pango::SCALE));
    layout.set_font_description(Some(&font));
    layout.set_text(text);
    layout.set_width(width * pango::SCALE);
    layout.set_alignment(alignment);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout
}

/// The reading's icon from the binding icon vocabulary, in its accent color
/// with the vocabulary's dark outline so it reads over any artwork. A muted
/// speaker keeps a grey speaker with a red cross.
fn draw_icon(
    cairo: &Context,
    value: &InfoValue,
    rect: Rect,
    color: Color,
) -> Result<(), cairo::Error> {
    let Some(icon) = BindingIcon::for_glyph(value.glyph) else {
        return Ok(());
    };
    let center = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let ink = |color: Color| (color.0, color.1, color.2, 1.0);
    binding_icons::paint_outlined(cairo, icon, center, rect.width, ink(color))?;
    if value.glyph == Glyph::SpeakerMuted {
        // The cross sits right of the speaker (x > 2 on the 24-unit grid).
        cairo.save()?;
        cairo.rectangle(
            center.0 + rect.width * 2.0 / 24.0,
            rect.y,
            rect.width,
            rect.height,
        );
        cairo.clip();
        let result = binding_icons::paint(cairo, icon, center, rect.width, ink(RED));
        cairo.restore()?;
        result?;
    }
    Ok(())
}

/// Text with the same thin dark outline the key text editor uses.
fn draw_text(
    cairo: &Context,
    layout: &pango::Layout,
    (x, y): (f64, f64),
    fill: Color,
) -> Result<(), cairo::Error> {
    cairo.move_to(x, y);
    pangocairo::functions::layout_path(cairo, layout);
    cairo.set_source_rgba(0.0, 0.0, 0.0, 0.9);
    cairo.set_line_width(3.5);
    cairo.set_line_join(cairo::LineJoin::Round);
    cairo.stroke_preserve()?;
    set(cairo, fill);
    cairo.fill()
}

fn draw(cairo: &Context, value: &InfoValue) -> Result<(), cairo::Error> {
    draw_parts(cairo, value, &key_plan(value))
}

/// The section tile shared with binding icons (`binding_icons::draw_zone_tile`).
const SECTION_TILE: Rect = Rect {
    x: 3.5,
    y: 4.5,
    width: 193.0,
    height: 91.0,
};
/// Room kept at a section's outer edge for a page indicator chevron.
const CHEVRON_ROOM: f64 = 30.0;
/// Vertical space between stacked parts.
const GAP: f64 = 3.0;

/// A part's box in image pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.width
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

/// One line of text and the box it is drawn in.
struct Text {
    layout: pango::Layout,
    rect: Rect,
}

/// Where every part of a live information picture goes.
struct Plan {
    icon: Option<Rect>,
    headline: Option<Text>,
    bar: Option<Rect>,
    details: Vec<Text>,
}

impl Plan {
    fn texts(&self) -> impl Iterator<Item = &Text> {
        self.headline.iter().chain(&self.details)
    }

    /// Move the text and bar down by `dy`.
    fn shift(&mut self, dy: f64) {
        for text in self.headline.iter_mut().chain(&mut self.details) {
            text.rect.y += dy;
        }
        if let Some(bar) = &mut self.bar {
            bar.y += dy;
        }
    }

    #[cfg(test)]
    fn rects(&self) -> Vec<Rect> {
        self.icon
            .iter()
            .chain(&self.bar)
            .copied()
            .chain(self.texts().map(|text| text.rect))
            .collect()
    }
}

/// A one-line layout no wider than `width`: a long headline first shrinks
/// (down to 60%), then ellipsizes; it never wraps mid-word.
fn fitted_layout(
    text: &str,
    size: f64,
    bold: bool,
    width: f64,
    alignment: pango::Alignment,
) -> pango::Layout {
    let natural = make_layout(text, size, bold, -1, alignment);
    let natural_width = f64::from(natural.pixel_size().0);
    let size = if natural_width > width {
        (size * width / natural_width).max(size * 0.6).floor()
    } else {
        size
    };
    make_layout(text, size, bold, width.floor() as i32, alignment)
}

/// Stack the value, bar and detail lines in one column, vertically centered
/// in `top..bottom`; detail lines that do not fit are left out.
fn text_column(
    value: &InfoValue,
    (x, width): (f64, f64),
    (top, bottom): (f64, f64),
    (headline_size, detail_size, bar_height): (f64, f64, f64),
    alignment: pango::Alignment,
) -> (Option<Text>, Option<Rect>, Vec<Text>) {
    let height_of = |layout: &pango::Layout| f64::from(layout.pixel_size().1);
    let headline = (!value.headline.is_empty())
        .then(|| fitted_layout(&value.headline, headline_size, true, width, alignment));
    let mut used = headline.as_ref().map_or(0.0, height_of);
    if value.level.is_some() {
        used += if headline.is_some() { GAP } else { 0.0 } + bar_height;
    }
    let mut details = Vec::new();
    for line in value.detail.lines().filter(|line| !line.is_empty()) {
        let layout = fitted_layout(line, detail_size, false, width, alignment);
        let extra = height_of(&layout) + if used > 0.0 { GAP } else { 0.0 };
        if used + extra > bottom - top {
            break;
        }
        used += extra;
        details.push(layout);
    }
    let mut y = top + ((bottom - top - used) / 2.0).max(0.0);
    let mut place = |height: f64| {
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        y += height + GAP;
        rect
    };
    let headline = headline.map(|layout| Text {
        rect: place(height_of(&layout)),
        layout,
    });
    let bar = value.level.map(|_| {
        let rect = place(bar_height);
        let inset = if alignment == pango::Alignment::Center {
            // A centered bar keeps a margin on both sides.
            8.0
        } else {
            1.0
        };
        Rect {
            x: rect.x + inset,
            width: (rect.width - 2.0 * inset).min(110.0),
            ..rect
        }
    });
    let details = details
        .into_iter()
        .map(|layout| Text {
            rect: place(height_of(&layout)),
            layout,
        })
        .collect();
    (headline, bar, details)
}

/// Wide layout on the section tile: icon on the left; value, bar and detail
/// stacked beside it. Without an icon the text is centered on the tile.
fn zone_plan(value: &InfoValue, previous: bool, next: bool) -> Plan {
    let left = SECTION_TILE.x + if previous { CHEVRON_ROOM } else { 0.0 } + 6.0;
    let right = SECTION_TILE.right() - if next { CHEVRON_ROOM } else { 0.0 } - 6.0;
    let vertical = (SECTION_TILE.y + 5.0, SECTION_TILE.bottom() - 5.0);
    if value.glyph == Glyph::None {
        let (headline, bar, details) = text_column(
            value,
            (left, right - left),
            vertical,
            (40.0, 16.0, 10.0),
            pango::Alignment::Center,
        );
        return Plan {
            icon: None,
            headline,
            bar,
            details,
        };
    }
    let size = if previous || next { 44.0 } else { 52.0 };
    let icon = Rect {
        x: left + 2.0,
        y: f64::from(ZONE_HEIGHT) / 2.0 - size / 2.0,
        width: size,
        height: size,
    };
    let x = icon.right() + 10.0;
    let (headline, bar, details) = text_column(
        value,
        (x, right - x),
        vertical,
        (26.0, 13.0, 8.0),
        pango::Alignment::Left,
    );
    Plan {
        icon: Some(icon),
        headline,
        bar,
        details,
    }
}

/// Square key layout: icon, value, bar and detail stacked and centered, all
/// inside the key. The icon gives up size first; detail lines that still do
/// not fit are left out, so nothing is cut at the bottom edge.
fn key_plan(value: &InfoValue) -> Plan {
    const TOP: f64 = 4.0;
    const BOTTOM: f64 = SIZE as f64 - 4.0;
    const MIN_ICON: f64 = 32.0;
    const MAX_ICON: f64 = 44.0;
    let column = (4.0, f64::from(SIZE) - 8.0);
    if value.glyph == Glyph::None {
        let (headline, bar, details) = text_column(
            value,
            column,
            (TOP, BOTTOM),
            (36.0, 16.0, 10.0),
            pango::Alignment::Center,
        );
        return Plan {
            icon: None,
            headline,
            bar,
            details,
        };
    }
    let (headline, bar, details) = text_column(
        value,
        column,
        (TOP + MIN_ICON + GAP, BOTTOM),
        (24.0, 14.0, 8.0),
        pango::Alignment::Center,
    );
    let mut plan = Plan {
        icon: None,
        headline,
        bar,
        details,
    };
    let first = plan
        .texts()
        .map(|text| text.rect.y)
        .chain(plan.bar.map(|bar| bar.y));
    let last = plan
        .texts()
        .map(|text| text.rect.bottom())
        .chain(plan.bar.map(|bar| bar.bottom()));
    let (first, last) = (first.fold(f64::MAX, f64::min), last.fold(0.0, f64::max));
    let used = if last > first {
        last - first + GAP
    } else {
        0.0
    };
    let size = (BOTTOM - TOP - used).clamp(MIN_ICON, MAX_ICON);
    let top = TOP + (BOTTOM - TOP - size - used) / 2.0;
    plan.icon = Some(Rect {
        x: f64::from(SIZE) / 2.0 - size / 2.0,
        y: top,
        width: size,
        height: size,
    });
    if used > 0.0 {
        plan.shift(top + size + GAP - first);
    }
    plan
}

fn draw_parts(cairo: &Context, value: &InfoValue, plan: &Plan) -> Result<(), cairo::Error> {
    let color = accent(value);
    if let Some(icon) = plan.icon {
        draw_icon(cairo, value, icon, color)?;
    }
    let fill = if value.inactive { GREY } else { WHITE };
    if let Some(bar) = plan.bar {
        draw_bar(
            cairo,
            value.level.unwrap_or(0),
            color,
            (bar.x, bar.y, bar.width, bar.height),
        )?;
    }
    for text in plan.texts() {
        draw_text(cairo, &text.layout, (text.rect.x, text.rect.y), fill)?;
    }
    Ok(())
}

/// A translucent dark track keeps the proportion readable over any artwork.
fn draw_bar(
    cairo: &Context,
    level: u8,
    color: Color,
    (x, y, width, height): (f64, f64, f64, f64),
) -> Result<(), cairo::Error> {
    rounded(cairo, x, y, width, height, height / 2.0);
    cairo.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    cairo.fill_preserve()?;
    cairo.set_source_rgba(0.0, 0.0, 0.0, 0.9);
    cairo.set_line_width(2.0);
    cairo.stroke()?;
    if level > 0 {
        cairo.save()?;
        rounded(cairo, x, y, width, height, height / 2.0);
        cairo.clip();
        set(cairo, color);
        cairo.rectangle(x, y, width * f64::from(level.min(100)) / 100.0, height);
        cairo.fill()?;
        cairo.restore()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::{parse_volume, InfoSource};

    fn pixel(image: &Pixbuf, x: usize, y: usize) -> [u8; 3] {
        let bytes = image.read_pixel_bytes();
        let stride = usize::try_from(image.rowstride()).unwrap();
        let at = y * stride + x * image.n_channels() as usize;
        [bytes[at], bytes[at + 1], bytes[at + 2]]
    }

    fn near(actual: [u8; 3], expected: Color) -> bool {
        actual
            .into_iter()
            .zip([expected.0, expected.1, expected.2])
            .all(|(actual, expected)| (f64::from(actual) - expected * 255.0).abs() <= 6.0)
    }

    fn volume(text: &str) -> InfoValue {
        parse_volume(text).unwrap()
    }

    /// A point a quarter and three quarters along the key's bar.
    fn bar_points(value: &InfoValue) -> ((usize, usize), (usize, usize)) {
        let bar = key_plan(value).bar.unwrap();
        let y = (bar.y + bar.height / 2.0) as usize;
        let at = |share: f64| (bar.x + bar.width * share) as usize;
        ((at(0.25), y), (at(0.75), y))
    }

    #[test]
    fn bar_fill_follows_the_level_over_a_translucent_track() {
        let value = volume("Volume: 0.50");
        let ((low, y), (high, _)) = bar_points(&value);
        let half = compose_info_image(&value, None).unwrap();
        assert!(near(pixel(&half, low, y), CYAN), "filled part");
        assert!(near(pixel(&half, high, y), (0.0, 0.0, 0.0)), "empty part");
        let empty = compose_info_image(&volume("Volume: 0.00"), None).unwrap();
        assert!(near(pixel(&empty, low, y), (0.0, 0.0, 0.0)));
        let full = compose_info_image(&volume("Volume: 1.00"), None).unwrap();
        assert!(near(pixel(&full, high, y), CYAN));
    }

    #[test]
    fn muted_speaker_gets_a_red_cross_and_a_grey_bar() {
        let value = volume("Volume: 0.50 [MUTED]");
        let muted = compose_info_image(&value, None).unwrap();
        let icon = key_plan(&value).icon.unwrap();
        let cross = (
            (icon.x + icon.width * (0.5 + 7.5 / 24.0)) as usize,
            (icon.y + icon.height / 2.0) as usize,
        );
        let [red, green, _] = pixel(&muted, cross.0, cross.1);
        assert!(
            red > 200 && green < 150,
            "cross centre is red: {red},{green}"
        );
        let ((low, y), _) = bar_points(&value);
        assert!(near(pixel(&muted, low, y), GREY));
        let on = compose_info_image(&volume("Volume: 0.50"), None).unwrap();
        assert_ne!(muted.read_pixel_bytes(), on.read_pixel_bytes());
    }

    #[test]
    fn the_picture_has_no_background_of_its_own() {
        let root = std::env::temp_dir().join(format!("sdeck-render-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let artwork = root.join("artwork.png");
        let blue = Pixbuf::new(Colorspace::Rgb, false, 8, 120, 120).unwrap();
        blue.fill(0x1e5ac8ff);
        blue.savev(&artwork, "png", &[]).unwrap();
        let value = volume("Volume: 0.50");

        let over_artwork = compose_info_image(&value, Some(&artwork)).unwrap();
        let blank = compose_info_image(&value, None).unwrap();

        let background = (30.0 / 255.0, 90.0 / 255.0, 200.0 / 255.0);
        assert!(
            near(pixel(&over_artwork, 2, 2), background),
            "artwork shows through"
        );
        assert!(blank.has_alpha(), "the preview must retain transparency");
        let bytes = blank.read_pixel_bytes();
        let corner = 2 * blank.rowstride() as usize + 2 * 4 + 3;
        assert_eq!(bytes[corner], 0, "no opaque rectangle behind the binding");
        // The empty part of the bar is translucent: it darkens, not replaces, the artwork.
        let (_, (high, y)) = bar_points(&value);
        let track = pixel(&over_artwork, high, y);
        assert!(track[2] > 60 && track[2] < 120, "{track:?}");
        let translucent = Pixbuf::new(Colorspace::Rgb, true, 8, 120, 120).unwrap();
        translucent.fill(0x1e5ac880);
        translucent.savev(&artwork, "png", &[]).unwrap();
        let image = compose_info_image(&value, Some(&artwork)).unwrap();
        assert!(near(pixel(&image, 2, 2), background));
        assert_eq!(
            image.read_pixel_bytes()[corner],
            128,
            "PNG alpha must survive"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn rgba(image: &Pixbuf, x: usize, y: usize) -> [u8; 4] {
        let bytes = image.read_pixel_bytes();
        let at = y * usize::try_from(image.rowstride()).unwrap() + x * 4;
        [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
    }

    fn samples() -> Vec<InfoValue> {
        let detailed = |glyph, level, headline: &str, detail: &str| InfoValue {
            glyph,
            level,
            detail: detail.into(),
            ..InfoValue::text(glyph, headline)
        };
        vec![
            detailed(Glyph::Sun, None, "22\u{b0}C", "Clear"),
            detailed(Glyph::Chip, Some(38), "38%", "CPU"),
            detailed(Glyph::Memory, Some(41), "6.4 GiB", "Memory"),
            detailed(Glyph::Thermometer, Some(47), "47\u{b0}C", "Temperature"),
            detailed(Glyph::Speaker, Some(42), "42%", "Volume"),
            detailed(
                Glyph::Speaker,
                None,
                "Speakers",
                "Family 17h/19h\nHD Audio\nController\u{2026}",
            ),
            detailed(Glyph::None, None, "10:24", "Fri 16 Oct"),
            detailed(Glyph::Record, None, "REC LIVE", "Main scene"),
            detailed(Glyph::Rain, None, "--", "set a location"),
        ]
    }

    #[test]
    fn touch_sections_show_live_information_on_the_section_tile() {
        let weather = &samples()[0];
        let image = compose_zone_image(weather, None, false, false).unwrap();
        // The same translucent graphite tile as binding-icon sections: not
        // transparent and not black, with transparent rounded corners.
        let tile = rgba(&image, 100, 92);
        assert!(tile[3] > 60 && tile[3] < 255, "translucent tile: {tile:?}");
        assert!(tile[0] > 20, "graphite, not black: {tile:?}");
        assert_eq!(rgba(&image, 0, 0)[3], 0);
        assert_eq!(rgba(&image, 199, 99)[3], 0);
    }

    #[test]
    fn section_parts_stay_on_the_tile_and_out_of_the_chevron_room() {
        for value in samples() {
            for (previous, next) in [(false, false), (true, false), (false, true), (true, true)] {
                let plan = zone_plan(&value, previous, next);
                let left = SECTION_TILE.x + if previous { CHEVRON_ROOM } else { 0.0 };
                let right = SECTION_TILE.right() - if next { CHEVRON_ROOM } else { 0.0 };
                assert_eq!(plan.headline.is_some(), !value.headline.is_empty());
                assert_eq!(plan.bar.is_some(), value.level.is_some());
                for rect in plan.rects() {
                    assert!(
                        rect.x >= left
                            && rect.right() <= right
                            && rect.y >= SECTION_TILE.y
                            && rect.bottom() <= SECTION_TILE.bottom(),
                        "{value:?} ({previous}, {next}): {rect:?} leaves the tile"
                    );
                }
                for text in plan.texts() {
                    assert!(
                        f64::from(text.layout.pixel_size().0) <= text.rect.width + 0.5,
                        "{value:?}: text wider than its column"
                    );
                }
                if let (Some(icon), Some(headline)) = (plan.icon, &plan.headline) {
                    assert!(icon.right() < headline.rect.x, "icon left of the value");
                }
            }
        }
        let indicated = compose_zone_image(&samples()[1], None, true, true).unwrap();
        for y in 30..70 {
            for x in (0..32).chain(168..200) {
                let [red, green, _, alpha] = rgba(&indicated, x, y);
                assert!(
                    !(alpha > 200 && red > 200 && green > 200),
                    "chevron room stays clear at {x},{y}"
                );
            }
        }
    }

    #[test]
    fn key_parts_are_fully_visible_and_readable_at_preview_size() {
        let size_of = |text: &Text| {
            f64::from(text.layout.font_description().unwrap().size()) / f64::from(pango::SCALE)
        };
        for value in samples() {
            let plan = key_plan(&value);
            assert_eq!(plan.headline.is_some(), !value.headline.is_empty());
            assert_eq!(plan.bar.is_some(), value.level.is_some());
            assert_eq!(plan.icon.is_some(), value.glyph != Glyph::None);
            if value.detail.lines().count() == 1 {
                // "CPU", "Memory", "Temperature": the caption is never cut off.
                assert_eq!(plan.details.len(), 1, "{value:?} keeps its caption");
            }
            for rect in plan.rects() {
                assert!(
                    rect.x >= 2.0
                        && rect.right() <= 118.0
                        && rect.y >= 2.0
                        && rect.bottom() <= 118.0,
                    "{value:?}: {rect:?} is clipped by the key"
                );
            }
            let mut previous_bottom = 0.0;
            for rect in plan.icon.iter().chain(plan.texts().map(|text| &text.rect)) {
                assert!(rect.y >= previous_bottom, "{value:?}: parts overlap");
                previous_bottom = rect.bottom();
            }
            for text in plan.texts() {
                assert!(f64::from(text.layout.pixel_size().0) <= text.rect.width + 0.5);
            }
            // About half size in the window preview: still legible there.
            if let Some(headline) = &plan.headline {
                assert!(size_of(headline) >= 18.0, "{value:?}: headline too small");
            }
            for detail in &plan.details {
                assert!(size_of(detail) >= 13.0, "{value:?}: caption too small");
            }
            if let Some(icon) = plan.icon {
                assert!(icon.width >= 30.0, "{value:?}: icon too small");
            }
        }
    }

    #[test]
    fn every_glyph_and_source_renders_a_key_and_a_section() {
        for source in InfoSource::ALL {
            for value in [source.placeholder(), source.unavailable()] {
                let key = compose_info_image(&value, None).unwrap();
                assert_eq!((key.width(), key.height()), (120, 120));
                let section = compose_zone_image(&value, None, false, false).unwrap();
                assert_eq!((section.width(), section.height()), (200, 100));
                for image in [&key, &section] {
                    assert!(image.has_alpha());
                    let bytes = image.read_pixel_bytes();
                    assert_eq!(bytes[3], 0, "{source:?} must not paint the corner");
                }
            }
        }
        let empty = compose_info_image(&InfoValue::text(Glyph::None, ""), None).unwrap();
        for glyph in [
            Glyph::Headphones,
            Glyph::Sun,
            Glyph::PartlyCloudy,
            Glyph::Rain,
            Glyph::Snow,
            Glyph::Storm,
            Glyph::Fog,
            Glyph::Record,
            Glyph::Live,
        ] {
            let image = compose_info_image(&InfoValue::text(glyph, ""), None).unwrap();
            assert_ne!(
                image.read_pixel_bytes(),
                empty.read_pixel_bytes(),
                "{glyph:?} draws something"
            );
        }
    }
}
