//! Pure artwork preparation and Stream Deck + image-report encoding.

use crate::button_text::{ButtonText, HorizontalAlignment, TextOrientation, VerticalAlignment};
use crate::local_artwork::ArtworkTarget;
use gtk::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
use gtk::pango::prelude::*;
use std::{path::Path, time::Duration};

pub const REPORT_SIZE: usize = 1024;
const KEY_OR_FULL_CHUNK: usize = 1016;
const PARTIAL_CHUNK: usize = 1008;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputReport(pub Vec<u8>);

/// Return the required native pixel dimensions for a supported artwork target.
pub fn dimensions(target: ArtworkTarget) -> Result<ImageDimensions, String> {
    match target {
        ArtworkTarget::Keypad(n) if (1..=8).contains(&n) => Ok(ImageDimensions {
            width: 120,
            height: 120,
        }),
        ArtworkTarget::TouchStripBackground => Ok(ImageDimensions {
            width: 800,
            height: 100,
        }),
        ArtworkTarget::EncoderZone(n) if (1..=4).contains(&n) => Ok(ImageDimensions {
            width: 200,
            height: 100,
        }),
        _ => Err("unsupported artwork target".into()),
    }
}

/// Decode an image, center-crop to cover, resize, and encode as an in-memory JPEG.
pub fn prepare_jpeg(target: ArtworkTarget, source: impl AsRef<Path>) -> Result<Vec<u8>, String> {
    let size = dimensions(target)?;
    encode_jpeg(&prepare_background(size, source.as_ref())?)
}

pub(crate) fn prepare_background(size: ImageDimensions, source: &Path) -> Result<Pixbuf, String> {
    let scaled = resize_background(size, source)?;
    let target_w = scaled.width();
    let target_h = scaled.height();
    // JPEG has no alpha channel; flatten only for device output.
    let rgb = Pixbuf::new(Colorspace::Rgb, false, 8, target_w, target_h)
        .ok_or_else(|| "could not allocate JPEG image".to_string())?;
    rgb.fill(0x000000ff);
    scaled.composite(
        &rgb,
        0,
        0,
        target_w,
        target_h,
        0.0,
        0.0,
        1.0,
        1.0,
        InterpType::Nearest,
        255,
    );
    Ok(rgb)
}

pub(crate) fn resize_background(size: ImageDimensions, source: &Path) -> Result<Pixbuf, String> {
    let image = Pixbuf::from_file(source).map_err(|error| error.to_string())?;
    let width = image.width();
    let height = image.height();
    if width <= 0 || height <= 0 {
        return Err("source image has invalid dimensions".into());
    }
    let target_w = i32::try_from(size.width).map_err(|error| error.to_string())?;
    let target_h = i32::try_from(size.height).map_err(|error| error.to_string())?;

    // Compare aspect ratios by cross multiplication, avoiding floating-point crop drift.
    let (crop_x, crop_y, crop_w, crop_h) =
        if i64::from(width) * i64::from(target_h) > i64::from(height) * i64::from(target_w) {
            let crop_w = (i64::from(height) * i64::from(target_w) / i64::from(target_h)) as i32;
            ((width - crop_w) / 2, 0, crop_w, height)
        } else {
            let crop_h = (i64::from(width) * i64::from(target_h) / i64::from(target_w)) as i32;
            (0, (height - crop_h) / 2, width, crop_h)
        };
    let cropped = image.new_subpixbuf(crop_x, crop_y, crop_w, crop_h);
    cropped
        .scale_simple(target_w, target_h, InterpType::Bilinear)
        .ok_or_else(|| "could not resize source image".to_string())
}

fn encode_jpeg(image: &Pixbuf) -> Result<Vec<u8>, String> {
    // Transparency belongs to the preview/overlay. The device takes JPEG, so
    // flatten only after compositing the overlay onto its existing artwork.
    let flattened;
    let image = if image.has_alpha() {
        flattened = Pixbuf::new(Colorspace::Rgb, false, 8, image.width(), image.height())
            .ok_or_else(|| "could not allocate JPEG image".to_string())?;
        flattened.fill(0x000000ff);
        image.composite(
            &flattened,
            0,
            0,
            image.width(),
            image.height(),
            0.0,
            0.0,
            1.0,
            1.0,
            InterpType::Nearest,
            255,
        );
        &flattened
    } else {
        image
    };
    let bytes = image
        .save_to_bufferv("jpeg", &[("quality", "92")])
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() {
        return Err("JPEG encoder returned an empty image".into());
    }
    Ok(bytes)
}

/// Compose one physical 120x120 keypad image, without initializing GTK.
/// Text is clipped to the button, with a thin black outline.
pub fn compose_button_image(source: Option<&Path>, text: &ButtonText) -> Result<Pixbuf, String> {
    text.validate().map_err(|error| error.to_string())?;
    let background = match source {
        Some(source) => prepare_background(
            ImageDimensions {
                width: 120,
                height: 120,
            },
            source,
        )?,
        None => {
            let image = Pixbuf::new(Colorspace::Rgb, false, 8, 120, 120)
                .ok_or_else(|| "could not allocate button image".to_string())?;
            image.fill(0x000000ff);
            image
        }
    };
    if text.text.is_empty() {
        return Ok(background);
    }
    // RGB24 is native-endian 0x00RRGGBB. Work through safe owned bytes rather
    // than GDK Cairo helpers, which require GTK's initialized main thread.
    let bytes = background.read_pixel_bytes();
    let stride = usize::try_from(background.rowstride()).map_err(|error| error.to_string())?;
    let mut canvas = Vec::with_capacity(120 * 120 * 4);
    for y in 0..120 {
        for x in 0..120 {
            let offset = y * stride + x * 3;
            let pixel = u32::from(bytes[offset]) << 16
                | u32::from(bytes[offset + 1]) << 8
                | u32::from(bytes[offset + 2]);
            canvas.extend_from_slice(&pixel.to_ne_bytes());
        }
    }
    let mut surface =
        gtk::cairo::ImageSurface::create_for_data(canvas, gtk::cairo::Format::Rgb24, 120, 120, 480)
            .map_err(|error| error.to_string())?;
    let cairo = gtk::cairo::Context::new(&surface).map_err(|error| error.to_string())?;
    // PangoCairo's default font map is per-thread; no display or GTK init is used.
    let context = pangocairo::FontMap::default().create_context();
    let layout = gtk::pango::Layout::new(&context);
    let mut font = gtk::pango::FontDescription::new();
    font.set_family(&text.font_family);
    font.set_absolute_size(f64::from(text.font_size) * f64::from(gtk::pango::SCALE));
    layout.set_font_description(Some(&font));
    layout.set_text(&text.text);
    let (width, _) = layout.pixel_size();
    layout.set_width(width * gtk::pango::SCALE);
    layout.set_alignment(match text.horizontal_alignment {
        HorizontalAlignment::Left => gtk::pango::Alignment::Left,
        HorizontalAlignment::Center => gtk::pango::Alignment::Center,
        HorizontalAlignment::Right => gtk::pango::Alignment::Right,
    });
    let (ink, _) = layout.pixel_extents();
    let width = f64::from(ink.width());
    let height = f64::from(ink.height());
    let (output_width, output_height) = match text.orientation {
        TextOrientation::Horizontal => (width, height),
        TextOrientation::Vertical => (height, width),
    };
    let x = match text.horizontal_alignment {
        HorizontalAlignment::Left => 2.0,
        HorizontalAlignment::Center => (120.0 - output_width) / 2.0,
        HorizontalAlignment::Right => 118.0 - output_width,
    };
    let y = match text.vertical_alignment {
        VerticalAlignment::Top => 2.0,
        VerticalAlignment::Center => (120.0 - output_height) / 2.0,
        VerticalAlignment::Bottom => 118.0 - output_height,
    };
    cairo.translate(x, y);
    match text.orientation {
        TextOrientation::Horizontal => {}
        TextOrientation::Vertical => {
            cairo.translate(height, 0.0);
            cairo.rotate(std::f64::consts::FRAC_PI_2);
        }
    }
    cairo.move_to(-f64::from(ink.x()), -f64::from(ink.y()));
    pangocairo::functions::layout_path(&cairo, &layout);
    cairo.set_source_rgb(0.0, 0.0, 0.0);
    cairo.set_line_width(3.0);
    cairo.set_line_join(gtk::cairo::LineJoin::Round);
    cairo.stroke_preserve().map_err(|error| error.to_string())?;
    cairo.set_source_rgb(1.0, 1.0, 1.0);
    cairo.fill().map_err(|error| error.to_string())?;
    drop(cairo);
    let data = surface.data().map_err(|error| error.to_string())?;
    let mut rgb = Vec::with_capacity(120 * 120 * 3);
    for pixel in data.as_chunks::<4>().0 {
        let value = u32::from_ne_bytes(*pixel);
        let channels = value.to_be_bytes();
        rgb.extend_from_slice(&channels[1..]);
    }
    Ok(Pixbuf::from_bytes(
        &gtk::glib::Bytes::from_owned(rgb),
        Colorspace::Rgb,
        false,
        8,
        120,
        120,
        360,
    ))
}

/// Encode the same composed pixels used by GTK preview as a device-ready JPEG.
pub fn prepare_button_jpeg(source: Option<&Path>, text: &ButtonText) -> Result<Vec<u8>, String> {
    encode_jpeg(&compose_button_image(source, text)?)
}

/// Compose a transparent touch-zone foreground, painting page indicators last.
/// Only the outer edges of zones 1 and 4 can show previous/next availability.
pub fn compose_touchscreen_zone(
    zone: u8,
    background: Option<&Path>,
    value: Option<&crate::info::InfoValue>,
    previous: bool,
    next: bool,
) -> Result<Pixbuf, String> {
    compose_touchscreen_zone_at(zone, background, value, previous, next, Duration::ZERO)
}

/// [`compose_touchscreen_zone`] at `elapsed` into a scrolling title.
pub fn compose_touchscreen_zone_at(
    zone: u8,
    background: Option<&Path>,
    value: Option<&crate::info::InfoValue>,
    previous: bool,
    next: bool,
    elapsed: Duration,
) -> Result<Pixbuf, String> {
    let size = dimensions(ArtworkTarget::EncoderZone(zone))?;
    let image = if let Some(value) = value {
        crate::info_render::compose_zone_image_at(
            value,
            background,
            zone == 1 && previous,
            zone == 4 && next,
            elapsed,
        )?
    } else if let Some(background) = background {
        resize_background(size, background)?
    } else {
        let image = Pixbuf::new(Colorspace::Rgb, true, 8, 200, 100)
            .ok_or("could not allocate touchscreen foreground")?;
        image.fill(0);
        image
    };
    let previous = zone == 1 && previous;
    let next = zone == 4 && next;
    if !previous && !next {
        return Ok(image);
    }
    let result = Pixbuf::new(Colorspace::Rgb, true, 8, 200, 100)
        .ok_or("could not allocate touchscreen indicators")?;
    result.fill(0);
    image.composite(
        &result,
        0,
        0,
        200,
        100,
        0.0,
        0.0,
        1.0,
        1.0,
        InterpType::Nearest,
        255,
    );
    let overlay = Pixbuf::new(Colorspace::Rgb, true, 8, 200, 100)
        .ok_or("could not allocate touchscreen indicator overlay")?;
    overlay.fill(0);
    for (show, center, left) in [(previous, 16i32, true), (next, 184, false)] {
        if !show {
            continue;
        }
        for dy in -22i32..=22 {
            for dx in -14i32..=14 {
                if (dx.abs() - 10).max(0).pow(2) + (dy.abs() - 18).max(0).pow(2) <= 16 {
                    overlay.put_pixel((center + dx) as u32, (50 + dy) as u32, 12, 17, 23, 210);
                }
            }
        }
        for dy in -14i32..=14 {
            let x = if left {
                8 + dy.abs() * 3 / 4
            } else {
                192 - dy.abs() * 3 / 4
            };
            for dx in -1..=1 {
                overlay.put_pixel((x + dx) as u32, (50 + dy) as u32, 255, 255, 255, 255);
            }
        }
    }
    overlay.composite(
        &result,
        0,
        0,
        200,
        100,
        0.0,
        0.0,
        1.0,
        1.0,
        InterpType::Nearest,
        255,
    );
    Ok(result)
}

/// Encode the same indicated zone foreground as the preview over its strip crop.
pub fn prepare_touchscreen_zone_jpeg(
    zone: u8,
    background: Option<&Path>,
    strip_background: Option<&Path>,
    value: Option<&crate::info::InfoValue>,
    previous: bool,
    next: bool,
) -> Result<Vec<u8>, String> {
    prepare_touchscreen_zone_jpeg_at(
        zone,
        background,
        strip_background,
        value,
        (previous, next),
        Duration::ZERO,
    )
}

/// [`prepare_touchscreen_zone_jpeg`] at `elapsed` into a scrolling title.
pub fn prepare_touchscreen_zone_jpeg_at(
    zone: u8,
    background: Option<&Path>,
    strip_background: Option<&Path>,
    value: Option<&crate::info::InfoValue>,
    (previous, next): (bool, bool),
    elapsed: Duration,
) -> Result<Vec<u8>, String> {
    let overlay = compose_touchscreen_zone_at(zone, background, value, previous, next, elapsed)?;
    if let Some(strip) = strip_background {
        let strip = prepare_background(dimensions(ArtworkTarget::TouchStripBackground)?, strip)?;
        let section = strip.new_subpixbuf(i32::from(zone - 1) * 200, 0, 200, 100);
        overlay.composite(
            &section,
            0,
            0,
            200,
            100,
            0.0,
            0.0,
            1.0,
            1.0,
            InterpType::Nearest,
            255,
        );
        return encode_jpeg(&section);
    }
    encode_jpeg(&overlay)
}

/// Encode a binding icon picture for a key or touch section that has no
/// artwork of its own. Keys keep the user's button text where it was placed;
/// touch sections draw over their strip crop with page indicators painted last.
/// Everything is composed in memory on the caller's (worker) thread.
pub fn prepare_icon_jpeg(
    target: ArtworkTarget,
    icon: crate::binding_icons::BindingIcon,
    caption: &str,
    text: Option<&ButtonText>,
    strip_background: Option<&Path>,
    previous: bool,
    next: bool,
) -> Result<Vec<u8>, String> {
    match target {
        ArtworkTarget::Keypad(_) => {
            match crate::binding_icons::compose_key_image(icon, caption, text)? {
                Some(image) => encode_jpeg(&image),
                // No free band beside the user's text: the plain text picture.
                None => prepare_button_jpeg(None, text.unwrap_or(&ButtonText::default())),
            }
        }
        ArtworkTarget::EncoderZone(zone) => {
            let overlay = crate::binding_icons::compose_zone_image(icon, caption, previous, next)?;
            let indicators = compose_touchscreen_zone(zone, None, None, previous, next)?;
            indicators.composite(
                &overlay,
                0,
                0,
                200,
                100,
                0.0,
                0.0,
                1.0,
                1.0,
                InterpType::Nearest,
                255,
            );
            let section = match strip_background {
                Some(strip) => {
                    let strip = prepare_background(
                        dimensions(ArtworkTarget::TouchStripBackground)?,
                        strip,
                    )?;
                    strip.new_subpixbuf(i32::from(zone - 1) * 200, 0, 200, 100)
                }
                None => {
                    let black = Pixbuf::new(Colorspace::Rgb, false, 8, 200, 100)
                        .ok_or("could not allocate touchscreen section")?;
                    black.fill(0x000000ff);
                    black
                }
            };
            overlay.composite(
                &section,
                0,
                0,
                200,
                100,
                0.0,
                0.0,
                1.0,
                1.0,
                InterpType::Nearest,
                255,
            );
            encode_jpeg(&section)
        }
        ArtworkTarget::TouchStripBackground => {
            Err("binding icons cannot replace the touch strip background".into())
        }
    }
}

/// Encode a live information picture as a device-ready JPEG for a key or touch
/// section, drawn over `background` (the target's own artwork) when there is one.
pub fn prepare_info_jpeg(
    target: ArtworkTarget,
    value: &crate::info::InfoValue,
    background: Option<&Path>,
    strip_background: Option<&Path>,
) -> Result<Vec<u8>, String> {
    prepare_info_jpeg_at(target, value, background, strip_background, Duration::ZERO)
}

/// [`prepare_info_jpeg`] at `elapsed` into a scrolling title.
pub fn prepare_info_jpeg_at(
    target: ArtworkTarget,
    value: &crate::info::InfoValue,
    background: Option<&Path>,
    strip_background: Option<&Path>,
    elapsed: Duration,
) -> Result<Vec<u8>, String> {
    let _ = dimensions(target)?;
    match target {
        ArtworkTarget::Keypad(_) => encode_jpeg(&crate::info_render::compose_info_image_at(
            value, background, elapsed,
        )?),
        ArtworkTarget::EncoderZone(zone) => {
            let overlay = crate::info_render::compose_zone_image_at(
                value, background, false, false, elapsed,
            )?;
            if let Some(strip) = strip_background {
                let strip =
                    prepare_background(dimensions(ArtworkTarget::TouchStripBackground)?, strip)?;
                let section = strip.new_subpixbuf(i32::from(zone - 1) * 200, 0, 200, 100);
                overlay.composite(
                    &section,
                    0,
                    0,
                    200,
                    100,
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                    InterpType::Nearest,
                    255,
                );
                return encode_jpeg(&section);
            }
            encode_jpeg(&overlay)
        }
        ArtworkTarget::TouchStripBackground => {
            Err("live information cannot replace the touch strip background".into())
        }
    }
}

/// Prepare a black JPEG for the target, suitable for the same image command as artwork.
pub fn prepare_clear_jpeg(target: ArtworkTarget) -> Result<Vec<u8>, String> {
    let size = dimensions(target)?;
    let width = i32::try_from(size.width).map_err(|error| error.to_string())?;
    let height = i32::try_from(size.height).map_err(|error| error.to_string())?;
    let image = Pixbuf::new(Colorspace::Rgb, false, 8, width, height)
        .ok_or_else(|| "could not allocate clear image".to_string())?;
    image.fill(0x000000ff);
    let bytes = image
        .save_to_bufferv("jpeg", &[("quality", "92")])
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() {
        return Err("JPEG encoder returned an empty image".into());
    }
    Ok(bytes)
}

/// Encode JPEG bytes into zero-padded, fixed-size output reports for the target.
pub fn encode_reports(target: ArtworkTarget, jpeg: &[u8]) -> Result<Vec<OutputReport>, String> {
    let _ = dimensions(target)?;
    if jpeg.is_empty() {
        return Err("JPEG payload must not be empty".into());
    }
    let (command, chunk_capacity, header_size) = match target {
        ArtworkTarget::Keypad(_) => (0x07, KEY_OR_FULL_CHUNK, 8),
        ArtworkTarget::TouchStripBackground => (0x0b, KEY_OR_FULL_CHUNK, 8),
        ArtworkTarget::EncoderZone(_) => (0x0c, PARTIAL_CHUNK, 16),
    };
    let count = jpeg
        .len()
        .checked_add(chunk_capacity - 1)
        .ok_or_else(|| "JPEG payload length overflow".to_string())?
        / chunk_capacity;
    if count > usize::from(u16::MAX) + 1 {
        return Err("image requires too many chunks".into());
    }
    let key_index = match target {
        ArtworkTarget::Keypad(n) => Some(n - 1),
        _ => None,
    };
    let zone_x = match target {
        ArtworkTarget::EncoderZone(n) => Some(u16::from(n - 1) * 200),
        _ => None,
    };
    let mut reports = Vec::with_capacity(count);
    for (index, chunk) in jpeg.chunks(chunk_capacity).enumerate() {
        let index = u16::try_from(index).map_err(|error| error.to_string())?;
        let length = u16::try_from(chunk.len()).map_err(|error| error.to_string())?;
        let mut report = vec![0; REPORT_SIZE];
        report[0] = 0x02;
        report[1] = command;
        if header_size == 16 {
            report[2..4].copy_from_slice(&zone_x.unwrap().to_le_bytes());
            report[4..6].copy_from_slice(&0u16.to_le_bytes());
            report[6..8].copy_from_slice(&200u16.to_le_bytes());
            report[8..10].copy_from_slice(&100u16.to_le_bytes());
            report[10] = u8::from(index as usize + 1 == count);
            report[11..13].copy_from_slice(&index.to_le_bytes());
            report[13..15].copy_from_slice(&length.to_le_bytes());
            report[15] = 0;
        } else {
            report[2] = key_index.unwrap_or(0);
            report[3] = u8::from(index as usize + 1 == count);
            report[4..6].copy_from_slice(&length.to_le_bytes());
            report[6..8].copy_from_slice(&index.to_le_bytes());
        }
        report[header_size..header_size + chunk.len()].copy_from_slice(chunk);
        reports.push(OutputReport(report));
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture(width: i32, height: i32) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "image-output-{}-{}.png",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let image = Pixbuf::new(Colorspace::Rgb, false, 8, width, height).unwrap();
        // Left half red and right half blue makes horizontal crop placement observable.
        for y in 0..height {
            for x in 0..width {
                let (red, green, blue) = if x < width / 2 {
                    (255, 0, 0)
                } else {
                    (0, 0, 255)
                };
                image.put_pixel(x as u32, y as u32, red, green, blue, 255);
            }
        }
        image.savev(&path, "png", &[]).unwrap();
        path
    }

    fn pixel(image: &Pixbuf, x: usize, y: usize) -> [u8; 3] {
        let stride = image.rowstride() as usize;
        let channels = image.n_channels() as usize;
        let data = image.read_pixel_bytes();
        let offset = y * stride + x * channels;
        [data[offset], data[offset + 1], data[offset + 2]]
    }

    #[test]
    fn touchscreen_indicators_appear_only_at_available_outer_edges() {
        for (previous, next) in [(false, false), (false, true), (true, true), (true, false)] {
            for zone in 1..=4 {
                let image = compose_touchscreen_zone(zone, None, None, previous, next).unwrap();
                assert_eq!((image.width(), image.height()), (200, 100));
                let bytes = image.read_pixel_bytes();
                let alpha = |x: usize, y: usize| {
                    bytes[y * image.rowstride() as usize + x * image.n_channels() as usize + 3]
                };
                assert_eq!(alpha(8, 50), if zone == 1 && previous { 255 } else { 0 });
                assert_eq!(alpha(192, 50), if zone == 4 && next { 255 } else { 0 });
                assert_eq!(alpha(100, 50), 0, "indicators must not cover the center");
            }
        }
    }

    #[test]
    fn touchscreen_indicators_survive_artwork_and_live_value_redraws() {
        let own = fixture(200, 100);
        let strip = fixture(800, 100);
        for reading in ["Volume: 0.20", "Volume: 0.80"] {
            let value = crate::info::parse_volume(reading).unwrap();
            for (zone, x) in [(1, 8), (4, 192)] {
                let image =
                    compose_touchscreen_zone(zone, Some(&own), Some(&value), true, true).unwrap();
                assert_eq!(pixel(&image, x, 50), [255, 255, 255]);
                // The artwork stays visible around the section tile.
                assert_eq!(pixel(&image, 100, 2), [0, 0, 255]);
                let jpeg = prepare_touchscreen_zone_jpeg(
                    zone,
                    Some(&own),
                    Some(&strip),
                    Some(&value),
                    true,
                    true,
                )
                .unwrap();
                let device = Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
                assert!(pixel(&device, x, 50).iter().all(|channel| *channel > 215));
                for (actual, expected) in pixel(&device, 100, 2)
                    .into_iter()
                    .zip(pixel(&image, 100, 2))
                {
                    assert!(actual.abs_diff(expected) < 15);
                }
            }
        }
        fs::remove_file(own).unwrap();
        fs::remove_file(strip).unwrap();
    }

    #[test]
    fn touchscreen_composition_without_indicators_preserves_existing_info_output() {
        let strip = fixture(800, 100);
        let value = crate::info::parse_volume("Volume: 0.45").unwrap();
        for zone in 1..=4 {
            let old =
                prepare_info_jpeg(ArtworkTarget::EncoderZone(zone), &value, None, Some(&strip))
                    .unwrap();
            let new =
                prepare_touchscreen_zone_jpeg(zone, None, Some(&strip), Some(&value), false, false)
                    .unwrap();
            assert_eq!(new, old);
        }
        fs::remove_file(strip).unwrap();
    }

    #[test]
    fn live_sections_keep_the_underlying_strip_in_device_jpegs() {
        let strip = fixture(800, 100);
        let own = fixture(200, 100);
        let value = crate::info::parse_volume("Volume: 0.45").unwrap();
        for zone in 1..=4 {
            let jpeg =
                prepare_info_jpeg(ArtworkTarget::EncoderZone(zone), &value, None, Some(&strip))
                    .unwrap();
            let image = Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
            assert_eq!((image.width(), image.height()), (200, 100));
            let corner = pixel(&image, 4, 4);
            if zone <= 2 {
                assert!(corner[0] > 240 && corner[2] < 15, "{zone}: {corner:?}");
            } else {
                assert!(corner[2] > 240 && corner[0] < 15, "{zone}: {corner:?}");
            }
        }
        // An assigned section image still takes precedence over the strip.
        let jpeg = prepare_info_jpeg(
            ArtworkTarget::EncoderZone(4),
            &value,
            Some(&own),
            Some(&strip),
        )
        .unwrap();
        let image = Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
        assert!(pixel(&image, 4, 4)[0] > 240);
        fs::remove_file(strip).unwrap();
        fs::remove_file(own).unwrap();
    }

    fn white_bounds(image: &Pixbuf) -> (usize, usize, usize, usize, usize) {
        let mut bounds = (120, 120, 0, 0, 0);
        for y in 0..120 {
            for x in 0..120 {
                if pixel(image, x, y).iter().all(|channel| *channel > 220) {
                    bounds.0 = bounds.0.min(x);
                    bounds.1 = bounds.1.min(y);
                    bounds.2 = bounds.2.max(x);
                    bounds.3 = bounds.3.max(y);
                    bounds.4 += 1;
                }
            }
        }
        assert!(bounds.4 > 0, "text must produce white pixels");
        bounds
    }

    #[test]
    fn button_text_is_headless_multiline_and_composited_into_jpeg() {
        std::thread::spawn(|| {
            let path = fixture(400, 100);
            let text = ButtonText {
                text: "Hello\nDeck".into(),
                ..ButtonText::default()
            };
            let plain = compose_button_image(Some(&path), &ButtonText::default()).unwrap();
            assert_eq!((plain.width(), plain.height()), (120, 120));
            assert_eq!(pixel(&plain, 0, 60), [255, 0, 0]);
            assert_eq!(pixel(&plain, 119, 60), [0, 0, 255]);
            let composed = compose_button_image(Some(&path), &text).unwrap();
            white_bounds(&composed);
            assert_eq!(pixel(&composed, 0, 60), [255, 0, 0]);
            assert_eq!(pixel(&composed, 119, 60), [0, 0, 255]);
            let decoded = Pixbuf::from_read(std::io::Cursor::new(
                prepare_button_jpeg(Some(&path), &text).unwrap(),
            ))
            .unwrap();
            assert_eq!((decoded.width(), decoded.height()), (120, 120));
            white_bounds(&decoded);
            assert!(pixel(&decoded, 0, 60)[0] > 220);
            assert!(pixel(&decoded, 119, 60)[2] > 220);
            fs::remove_file(path).unwrap();
        })
        .join()
        .unwrap();
    }

    #[test]
    fn physical_alignment_rotation_size_and_family_change_pixels() {
        let mut text = ButtonText {
            text: "Wi\nii".into(),
            font_size: 20,
            ..ButtonText::default()
        };
        for horizontal in [
            HorizontalAlignment::Left,
            HorizontalAlignment::Center,
            HorizontalAlignment::Right,
        ] {
            for vertical in [
                VerticalAlignment::Top,
                VerticalAlignment::Center,
                VerticalAlignment::Bottom,
            ] {
                text.horizontal_alignment = horizontal;
                text.vertical_alignment = vertical;
                let (left, top, right, bottom, _) =
                    white_bounds(&compose_button_image(None, &text).unwrap());
                match horizontal {
                    HorizontalAlignment::Left => assert!(left <= 4),
                    HorizontalAlignment::Center => assert!(left.abs_diff(119 - right) <= 3),
                    HorizontalAlignment::Right => assert!(right >= 115),
                }
                match vertical {
                    VerticalAlignment::Top => assert!(top <= 4),
                    VerticalAlignment::Center => assert!(top.abs_diff(119 - bottom) <= 3),
                    VerticalAlignment::Bottom => assert!(bottom >= 115),
                }
            }
        }
        text.horizontal_alignment = HorizontalAlignment::Center;
        text.vertical_alignment = VerticalAlignment::Center;
        text.text = "Wi".into();
        let horizontal = compose_button_image(None, &text).unwrap();
        let h = white_bounds(&horizontal);
        text.orientation = TextOrientation::Vertical;
        let v = white_bounds(&compose_button_image(None, &text).unwrap());
        assert!((h.2 - h.0).abs_diff(v.3 - v.1) <= 2);
        assert!((h.3 - h.1).abs_diff(v.2 - v.0) <= 2);
        // Clockwise rotation of the entire shaped layout, not stacked glyphs.
        let vertical = compose_button_image(None, &text).unwrap();
        let mut difference = 0usize;
        for y in 0..120 {
            for x in 0..120 {
                difference += usize::from(
                    pixel(&horizontal, x, y)[0].abs_diff(pixel(&vertical, 119 - y, x)[0]) > 80,
                );
            }
        }
        assert!(difference < 160, "rotation mismatch: {difference} pixels");
        text.orientation = TextOrientation::Horizontal;
        text.font_size = 40;
        let large = white_bounds(&compose_button_image(None, &text).unwrap());
        assert!(large.4 > h.4 * 2);
        assert!(large.2 - large.0 > h.2 - h.0);
        text.font_family = "Monospace".into();
        let mono = compose_button_image(None, &text)
            .unwrap()
            .read_pixel_bytes();
        text.font_family = "Serif".into();
        let serif = compose_button_image(None, &text)
            .unwrap()
            .read_pixel_bytes();
        assert_ne!(mono.as_ref(), serif.as_ref());
    }

    #[test]
    fn outline_alpha_and_invalid_render_configuration() {
        let path = fixture(120, 120);
        let white = Pixbuf::new(Colorspace::Rgb, true, 8, 120, 120).unwrap();
        white.fill(0xffffffff);
        white.savev(&path, "png", &[]).unwrap();
        let text = ButtonText {
            text: "Outline".into(),
            ..ButtonText::default()
        };
        let composed = compose_button_image(Some(&path), &text).unwrap();
        assert_eq!(pixel(&composed, 0, 0), [255, 255, 255]);
        let black = (0..120)
            .flat_map(|y| (0..120).map(move |x| (x, y)))
            .filter(|(x, y)| pixel(&composed, *x, *y).iter().all(|value| *value < 20))
            .count();
        assert!(black > 100, "black outline should remain visible on white");
        white.fill(0xff000000);
        white.savev(&path, "png", &[]).unwrap();
        let transparent = compose_button_image(Some(&path), &ButtonText::default()).unwrap();
        assert_eq!(pixel(&transparent, 60, 60), [0, 0, 0]);
        assert!(compose_button_image(
            None,
            &ButtonText {
                font_size: 0,
                ..text.clone()
            }
        )
        .is_err());
        assert!(prepare_button_jpeg(
            None,
            &ButtonText {
                font_family: "".into(),
                ..text
            }
        )
        .is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn preparation_center_crops_and_outputs_native_size() {
        let path = fixture(400, 100);
        let jpeg = prepare_jpeg(ArtworkTarget::EncoderZone(1), &path).unwrap();
        let decoded = Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (200, 100));
        let left = pixel(&decoded, 0, 50);
        let right = pixel(&decoded, 199, 50);
        assert!(left[0] > left[2], "left edge should be red: {left:?}");
        assert!(right[2] > right[0], "right edge should be blue: {right:?}");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn png_jpeg_and_gif_sources_convert_to_target_sized_decodable_jpegs() {
        let root = std::env::temp_dir().join(format!(
            "image-format-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let png_path = root.join("source.png");
        Pixbuf::new(Colorspace::Rgb, false, 8, 32, 20)
            .unwrap()
            .savev(&png_path, "png", &[])
            .unwrap();
        let gif_path = root.join("source.gif");
        // A real 1x1 GIF89a fixture; no external fixture or package is needed.
        fs::write(
            &gif_path,
            [
                0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00,
                0x00, 0x00, 0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c,
                0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00,
                0x3b,
            ],
        )
        .unwrap();

        let jpg_path = root.join("source.jpg");
        let jpeg_path = root.join("source.jpeg");
        for path in [&jpg_path, &jpeg_path] {
            Pixbuf::from_file(&png_path)
                .unwrap()
                .savev(path, "jpeg", &[])
                .unwrap();
        }
        for path in [&png_path, &jpg_path, &jpeg_path, &gif_path] {
            let jpeg = prepare_jpeg(ArtworkTarget::Keypad(1), path).unwrap();
            let decoded = Pixbuf::from_read(std::io::Cursor::new(jpeg)).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (120, 120));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn key_and_full_window_reports_have_correct_headers_and_padding() {
        let one = encode_reports(ArtworkTarget::Keypad(8), &[0x5a; 3]).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(&one[0].0[..8], &[2, 7, 7, 1, 3, 0, 0, 0]);
        assert_eq!(&one[0].0[8..11], &[0x5a; 3]);
        assert!(one[0].0[11..].iter().all(|byte| *byte == 0));

        let single = encode_reports(ArtworkTarget::TouchStripBackground, &[1]).unwrap();
        assert_eq!(&single[0].0[..8], &[2, 0x0b, 0, 1, 1, 0, 0, 0]);
        let empty = encode_reports(ArtworkTarget::TouchStripBackground, &[]).unwrap_err();
        assert!(empty.contains("empty"));

        let multi = encode_reports(ArtworkTarget::Keypad(1), &vec![0x31; 1017]).unwrap();
        assert_eq!(multi.len(), 2);
        assert_eq!(&multi[0].0[..8], &[2, 7, 0, 0, 248, 3, 0, 0]);
        assert_eq!(&multi[1].0[..8], &[2, 7, 0, 1, 1, 0, 1, 0]);
        assert!(multi[1].0[9..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn partial_reports_cover_all_zone_offsets_and_chunk_layout() {
        for (zone, x) in [(1, 0u16), (2, 200), (3, 400), (4, 600)] {
            let reports = encode_reports(ArtworkTarget::EncoderZone(zone), &[0x22; 1009]).unwrap();
            assert_eq!(reports.len(), 2);
            let first = &reports[0].0;
            assert_eq!(first[0..2], [2, 0x0c]);
            assert_eq!(u16::from_le_bytes([first[2], first[3]]), x);
            assert_eq!(u16::from_le_bytes([first[4], first[5]]), 0);
            assert_eq!(u16::from_le_bytes([first[6], first[7]]), 200);
            assert_eq!(u16::from_le_bytes([first[8], first[9]]), 100);
            assert_eq!(&first[10..16], &[0, 0, 0, 240, 3, 0]);
            let last = &reports[1].0;
            assert_eq!(&last[10..15], &[1, 1, 0, 1, 0]);
            assert_eq!(last[16], 0x22);
            assert!(last[17..].iter().all(|byte| *byte == 0));
        }
    }

    #[test]
    fn invalid_targets_and_black_clear_jpeg_are_handled() {
        assert!(dimensions(ArtworkTarget::Keypad(0)).is_err());
        assert!(dimensions(ArtworkTarget::Keypad(9)).is_err());
        assert!(dimensions(ArtworkTarget::EncoderZone(5)).is_err());
        assert!(encode_reports(ArtworkTarget::EncoderZone(5), &[1]).is_err());
        let bytes = prepare_clear_jpeg(ArtworkTarget::Keypad(1)).unwrap();
        let black = Pixbuf::from_read(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!((black.width(), black.height()), (120, 120));
        let pixel = pixel(&black, 60, 60);
        assert!(
            pixel[0] < 8 && pixel[1] < 8 && pixel[2] < 8,
            "expected black pixel, got {pixel:?}"
        );
    }
}
