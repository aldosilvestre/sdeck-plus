//! Pure artwork preparation and Stream Deck + image-report encoding.

use crate::local_artwork::ArtworkTarget;
use gtk::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
use std::path::Path;

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
    let scaled = cropped
        .scale_simple(target_w, target_h, InterpType::Bilinear)
        .ok_or_else(|| "could not resize source image".to_string())?;
    // GIF and some PNG sources carry alpha; JPEG has no alpha channel. Copy onto
    // an RGB canvas so transparent source pixels encode consistently as black.
    let rgb = Pixbuf::new(Colorspace::Rgb, false, 8, target_w, target_h)
        .ok_or_else(|| "could not allocate JPEG image".to_string())?;
    let pixels = unsafe { scaled.pixels() };
    let channels = scaled.n_channels() as usize;
    let stride = scaled.rowstride() as usize;
    for y in 0..target_h {
        for x in 0..target_w {
            let offset = y as usize * stride + x as usize * channels;
            let alpha = if scaled.has_alpha() {
                u16::from(pixels[offset + 3])
            } else {
                255
            };
            rgb.put_pixel(
                x as u32,
                y as u32,
                (u16::from(pixels[offset]) * alpha / 255) as u8,
                (u16::from(pixels[offset + 1]) * alpha / 255) as u8,
                (u16::from(pixels[offset + 2]) * alpha / 255) as u8,
                255,
            );
        }
    }
    let bytes = rgb
        .save_to_bufferv("jpeg", &[("quality", "92")])
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() {
        return Err("JPEG encoder returned an empty image".into());
    }
    Ok(bytes)
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
        // The returned slice belongs to the pixbuf and is read-only for this test.
        let data = unsafe { image.pixels() };
        let offset = y * stride + x * channels;
        [data[offset], data[offset + 1], data[offset + 2]]
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
