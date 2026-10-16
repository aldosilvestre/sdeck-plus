//! Draws a live information value (icon, bar and text) as a 120x120 key image.

use crate::{
    image_output::{resize_background, ImageDimensions},
    info::{Glyph, InfoValue},
};
use gtk::{
    cairo::{self, Context},
    gdk_pixbuf::{Colorspace, Pixbuf},
    pango::{self, prelude::FontMapExt},
};
use std::{
    f64::consts::{PI, TAU},
    path::Path,
};

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
    compose(value, SIZE, SIZE, background)
}

/// Compose one 200x100 touch-strip section the same way.
///
/// # Errors
/// Returns background decoding, cairo or surface allocation failures.
pub fn compose_zone_image(value: &InfoValue, background: Option<&Path>) -> Result<Pixbuf, String> {
    compose(value, ZONE_WIDTH, ZONE_HEIGHT, background)
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
        if width > height {
            draw_zone(&cairo, value)
        } else {
            draw(&cairo, value)
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

/// Long headlines such as `REC LIVE` shrink instead of being ellipsized.
fn fit(size: f64, headline: &str, limit: usize) -> f64 {
    if headline.chars().count() > limit {
        size * 0.65
    } else {
        size
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

/// The icon, outlined in dark so it reads over any artwork.
fn draw_icon(
    cairo: &Context,
    value: &InfoValue,
    (x, y): (f64, f64),
    scale: f64,
    color: Color,
) -> Result<(), cairo::Error> {
    cairo.push_group();
    cairo.save()?;
    cairo.translate(x, y);
    cairo.scale(scale, scale);
    draw_glyph(cairo, value.glyph, color, value.inactive)?;
    cairo.restore()?;
    let icon = cairo.pop_group()?;
    for index in 0..12 {
        let angle = f64::from(index) * TAU / 12.0;
        cairo.save()?;
        cairo.translate(angle.cos() * 2.2, angle.sin() * 2.2);
        cairo.set_source_rgba(0.0, 0.0, 0.0, 0.85);
        cairo.mask(&icon)?;
        cairo.restore()?;
    }
    cairo.set_source(&icon)?;
    cairo.paint()
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
    let color = accent(value);
    let has_bar = value.level.is_some();
    let has_glyph = value.glyph != Glyph::None;
    if has_glyph {
        draw_icon(
            cairo,
            value,
            (60.0, if has_bar { 34.0 } else { 32.0 }),
            1.0,
            color,
        )?;
    }
    if let Some(level) = value.level {
        draw_bar(cairo, level, color, (16.0, 70.0, 88.0, 12.0))?;
    }

    let headline_size = match (has_bar, has_glyph) {
        (true, _) => 20.0,
        (false, true) => 24.0,
        (false, false) => 38.0,
    };
    let detail_size = if has_glyph { 13.0 } else { 18.0 };
    let mut layouts = Vec::new();
    if !value.headline.is_empty() {
        layouts.push(make_layout(
            &value.headline,
            fit(headline_size, &value.headline, 6),
            true,
            112,
            pango::Alignment::Center,
        ));
    }
    layouts.extend(
        value
            .detail
            .lines()
            .map(|line| make_layout(line, detail_size, false, 112, pango::Alignment::Center)),
    );
    let heights: Vec<f64> = layouts
        .iter()
        .map(|layout| f64::from(layout.pixel_size().1))
        .collect();
    let total: f64 = heights.iter().sum();
    let mut y = match (has_bar, has_glyph) {
        (true, _) => 88.0,
        (false, true) => 64.0,
        (false, false) => (f64::from(SIZE) - total) / 2.0,
    };
    let fill = if value.inactive { GREY } else { WHITE };
    for (layout, height) in layouts.iter().zip(heights) {
        draw_text(cairo, layout, (4.0, y), fill)?;
        y += height;
    }
    Ok(())
}

/// Wide layout: icon on the left; value, bar and detail stacked on the right.
fn draw_zone(cairo: &Context, value: &InfoValue) -> Result<(), cairo::Error> {
    let color = accent(value);
    let has_glyph = value.glyph != Glyph::None;
    if has_glyph {
        draw_icon(cairo, value, (46.0, 50.0), 1.15, color)?;
    }
    let (left, width, alignment) = if has_glyph {
        (92.0, 100, pango::Alignment::Left)
    } else {
        (0.0, ZONE_WIDTH, pango::Alignment::Center)
    };
    let headline_size = if has_glyph { 28.0 } else { 46.0 };
    let detail_size = if has_glyph { 14.0 } else { 20.0 };
    let headline = (!value.headline.is_empty()).then(|| {
        let size = fit(
            headline_size,
            &value.headline,
            if has_glyph { 5 } else { 8 },
        );
        make_layout(&value.headline, size, true, width, alignment)
    });
    let details: Vec<_> = value
        .detail
        .lines()
        .map(|line| make_layout(line, detail_size, false, width, alignment))
        .collect();
    let height_of = |layout: &pango::Layout| f64::from(layout.pixel_size().1);
    let bar_space = if value.level.is_some() { 22.0 } else { 0.0 };
    let total = headline.iter().map(height_of).sum::<f64>()
        + bar_space
        + details.iter().map(height_of).sum::<f64>();
    let mut y = (f64::from(ZONE_HEIGHT) - total) / 2.0;
    let fill = if value.inactive { GREY } else { WHITE };
    if let Some(layout) = &headline {
        draw_text(cairo, layout, (left, y), fill)?;
        y += height_of(layout);
    }
    if let Some(level) = value.level {
        draw_bar(cairo, level, color, (left + 4.0, y + 5.0, 92.0, 12.0))?;
        y += bar_space;
    }
    for layout in &details {
        draw_text(cairo, layout, (left, y), fill)?;
        y += height_of(layout);
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

fn cloud(cairo: &Context) -> Result<(), cairo::Error> {
    cairo.new_path();
    cairo.arc(-11.0, 4.0, 9.0, 0.0, TAU);
    cairo.new_sub_path();
    cairo.arc(0.0, -5.0, 13.0, 0.0, TAU);
    cairo.new_sub_path();
    cairo.arc(13.0, 3.0, 10.0, 0.0, TAU);
    cairo.rectangle(-11.0, 4.0, 24.0, 9.0);
    cairo.fill()
}

fn rays(cairo: &Context, inner: f64, outer: f64, count: u32) -> Result<(), cairo::Error> {
    for index in 0..count {
        let angle = f64::from(index) * TAU / f64::from(count);
        cairo.move_to(angle.cos() * inner, angle.sin() * inner);
        cairo.line_to(angle.cos() * outer, angle.sin() * outer);
    }
    cairo.stroke()
}

fn draw_glyph(
    cairo: &Context,
    glyph: Glyph,
    color: Color,
    inactive: bool,
) -> Result<(), cairo::Error> {
    cairo.set_line_cap(cairo::LineCap::Round);
    cairo.set_line_join(cairo::LineJoin::Round);
    set(cairo, color);
    match glyph {
        Glyph::None => {}
        Glyph::Speaker | Glyph::SpeakerMuted => {
            cairo.translate(-6.0, 0.0);
            cairo.move_to(-22.0, -9.0);
            cairo.line_to(-10.0, -9.0);
            cairo.line_to(4.0, -21.0);
            cairo.line_to(4.0, 21.0);
            cairo.line_to(-10.0, 9.0);
            cairo.line_to(-22.0, 9.0);
            cairo.close_path();
            cairo.fill()?;
            cairo.set_line_width(4.0);
            if glyph == Glyph::SpeakerMuted {
                set(cairo, RED);
                cairo.move_to(12.0, -10.0);
                cairo.line_to(28.0, 10.0);
                cairo.move_to(28.0, -10.0);
                cairo.line_to(12.0, 10.0);
                cairo.stroke()?;
            } else {
                for radius in [11.0, 19.0] {
                    cairo.new_path();
                    cairo.arc(4.0, 0.0, radius, -0.75, 0.75);
                    cairo.stroke()?;
                }
            }
        }
        Glyph::Headphones => {
            cairo.set_line_width(5.0);
            cairo.new_path();
            cairo.arc(0.0, 2.0, 19.0, PI, TAU);
            cairo.stroke()?;
            rounded(cairo, -26.0, 2.0, 11.0, 20.0, 4.0);
            cairo.fill()?;
            rounded(cairo, 15.0, 2.0, 11.0, 20.0, 4.0);
            cairo.fill()?;
        }
        Glyph::Sun => {
            cairo.new_path();
            cairo.arc(0.0, 0.0, 11.0, 0.0, TAU);
            cairo.fill()?;
            cairo.set_line_width(3.5);
            rays(cairo, 16.0, 23.0, 8)?;
        }
        Glyph::Cloud => {
            cairo.translate(-1.0, 2.0);
            cloud(cairo)?;
        }
        Glyph::PartlyCloudy => {
            cairo.save()?;
            cairo.translate(-9.0, -9.0);
            cairo.new_path();
            cairo.arc(0.0, 0.0, 9.0, 0.0, TAU);
            cairo.fill()?;
            cairo.set_line_width(3.0);
            rays(cairo, 13.0, 18.0, 8)?;
            cairo.restore()?;
            cairo.translate(4.0, 8.0);
            cairo.scale(0.8, 0.8);
            set(cairo, if inactive { GREY } else { CLOUD });
            cloud(cairo)?;
        }
        Glyph::Rain | Glyph::Snow | Glyph::Storm | Glyph::Fog => {
            cairo.save()?;
            cairo.translate(0.0, -8.0);
            set(cairo, if inactive { GREY } else { CLOUD });
            cloud(cairo)?;
            cairo.restore()?;
            cairo.set_line_width(3.5);
            match glyph {
                Glyph::Rain => {
                    set(cairo, color);
                    for x in [-10.0, 0.0, 10.0] {
                        cairo.move_to(x, 10.0);
                        cairo.line_to(x - 3.0, 21.0);
                    }
                    cairo.stroke()?;
                }
                Glyph::Snow => {
                    set(cairo, color);
                    for (x, y) in [(-9.0, 13.0), (0.0, 20.0), (9.0, 13.0)] {
                        cairo.new_path();
                        cairo.arc(x, y, 2.8, 0.0, TAU);
                        cairo.fill()?;
                    }
                }
                Glyph::Storm => {
                    set(cairo, color);
                    cairo.move_to(3.0, 5.0);
                    cairo.line_to(-6.0, 16.0);
                    cairo.line_to(0.0, 16.0);
                    cairo.line_to(-4.0, 26.0);
                    cairo.line_to(8.0, 12.0);
                    cairo.line_to(2.0, 12.0);
                    cairo.line_to(6.0, 5.0);
                    cairo.close_path();
                    cairo.fill()?;
                }
                _ => {
                    set(cairo, GREY);
                    for y in [12.0, 20.0] {
                        cairo.move_to(-16.0, y);
                        cairo.line_to(16.0, y);
                    }
                    cairo.stroke()?;
                }
            }
        }
        Glyph::Chip => {
            cairo.set_line_width(4.0);
            rounded(cairo, -14.0, -14.0, 28.0, 28.0, 5.0);
            cairo.stroke()?;
            rounded(cairo, -6.0, -6.0, 12.0, 12.0, 2.0);
            cairo.fill()?;
            cairo.set_line_width(3.0);
            for p in [-8.0, 0.0, 8.0] {
                cairo.move_to(p, -14.0);
                cairo.line_to(p, -22.0);
                cairo.move_to(p, 14.0);
                cairo.line_to(p, 22.0);
                cairo.move_to(-14.0, p);
                cairo.line_to(-22.0, p);
                cairo.move_to(14.0, p);
                cairo.line_to(22.0, p);
            }
            cairo.stroke()?;
        }
        Glyph::Memory => {
            cairo.set_line_width(4.0);
            rounded(cairo, -24.0, -12.0, 48.0, 22.0, 4.0);
            cairo.stroke()?;
            for x in [-18.0, -8.0, 2.0, 12.0] {
                cairo.rectangle(x, -6.0, 7.0, 10.0);
            }
            cairo.fill()?;
            cairo.set_line_width(3.0);
            for x in [-16.0, -8.0, 0.0, 8.0, 16.0] {
                cairo.move_to(x, 10.0);
                cairo.line_to(x, 17.0);
            }
            cairo.stroke()?;
        }
        Glyph::Thermometer => {
            cairo.set_line_width(4.0);
            rounded(cairo, -5.0, -24.0, 10.0, 34.0, 5.0);
            cairo.stroke()?;
            cairo.new_path();
            cairo.arc(0.0, 16.0, 10.0, 0.0, TAU);
            cairo.fill()?;
            cairo.rectangle(-2.0, -8.0, 4.0, 24.0);
            cairo.fill()?;
        }
        Glyph::Record => {
            cairo.new_path();
            cairo.arc(0.0, 0.0, 17.0, 0.0, TAU);
            cairo.fill()?;
        }
        Glyph::Live => {
            cairo.new_path();
            cairo.arc(0.0, 0.0, 7.0, 0.0, TAU);
            cairo.fill()?;
            cairo.set_line_width(4.0);
            for radius in [15.0, 23.0] {
                cairo.new_path();
                cairo.arc(0.0, 0.0, radius, -0.7, 0.7);
                cairo.stroke()?;
                cairo.new_path();
                cairo.arc(0.0, 0.0, radius, PI - 0.7, PI + 0.7);
                cairo.stroke()?;
            }
        }
        Glyph::ObsIdle => {
            cairo.set_line_width(4.0);
            cairo.new_path();
            cairo.arc(0.0, 0.0, 16.0, 0.0, TAU);
            cairo.stroke()?;
        }
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

    #[test]
    fn bar_fill_follows_the_level_over_a_translucent_track() {
        let half = compose_info_image(&volume("Volume: 0.50"), None).unwrap();
        assert!(near(pixel(&half, 30, 76), CYAN), "filled part");
        assert!(near(pixel(&half, 90, 76), (0.0, 0.0, 0.0)), "empty part");
        let empty = compose_info_image(&volume("Volume: 0.00"), None).unwrap();
        assert!(near(pixel(&empty, 30, 76), (0.0, 0.0, 0.0)));
        let full = compose_info_image(&volume("Volume: 1.00"), None).unwrap();
        assert!(near(pixel(&full, 90, 76), CYAN));
    }

    #[test]
    fn muted_speaker_gets_a_red_cross_and_a_grey_bar() {
        let muted = compose_info_image(&volume("Volume: 0.50 [MUTED]"), None).unwrap();
        let [red, green, _] = pixel(&muted, 74, 34);
        assert!(
            red > 200 && green < 150,
            "cross centre is red: {red},{green}"
        );
        assert!(near(pixel(&muted, 30, 76), GREY));
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
        let track = pixel(&over_artwork, 90, 76);
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

    #[test]
    fn every_glyph_and_source_renders_a_key_and_a_section() {
        for source in InfoSource::ALL {
            for value in [source.placeholder(), source.unavailable()] {
                let key = compose_info_image(&value, None).unwrap();
                assert_eq!((key.width(), key.height()), (120, 120));
                let section = compose_zone_image(&value, None).unwrap();
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
