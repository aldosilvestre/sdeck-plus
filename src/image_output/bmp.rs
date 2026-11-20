//! The uncompressed BMP files a Legacy Mini key takes.
//!
//! Elgato's Mini page (https://docs.elgato.com/streamdeck/hid/mini) only says
//! "Export as BMP format". The layout written here is the common uncompressed
//! one, UNVERIFIED on hardware: a 14-byte file header, a 40-byte
//! BITMAPINFOHEADER, 24 bits per pixel in blue, green, red order, and rows
//! bottom-up (positive height), each padded to a multiple of four bytes.

use gtk::gdk_pixbuf::{Colorspace, Pixbuf};

/// File header plus BITMAPINFOHEADER: where the pixels start.
const HEADER_LEN: usize = 14 + INFO_HEADER_LEN;
const INFO_HEADER_LEN: usize = 40;
/// 72 DPI. UNVERIFIED: the page names no resolution, and a display ignores it.
const PIXELS_PER_METRE: i32 = 2835;

/// `image` (8-bit RGB; an alpha channel is ignored) as a 24-bit bottom-up BMP.
///
/// # Errors
/// A message when the picture is not 8-bit RGB or too large for a BMP.
pub(super) fn encode(image: &Pixbuf) -> Result<Vec<u8>, String> {
    if image.colorspace() != Colorspace::Rgb || image.bits_per_sample() != 8 {
        return Err("BMP encoder takes 8-bit RGB pictures".into());
    }
    let width = usize::try_from(image.width()).map_err(|error| error.to_string())?;
    let height = usize::try_from(image.height()).map_err(|error| error.to_string())?;
    let channels = usize::try_from(image.n_channels()).map_err(|error| error.to_string())?;
    let stride = usize::try_from(image.rowstride()).map_err(|error| error.to_string())?;
    let row = (width * 3).next_multiple_of(4);
    let pixels_len = row * height;
    let too_large = |_| "picture too large for a BMP".to_string();
    let file_len = u32::try_from(HEADER_LEN + pixels_len).map_err(too_large)?;
    let mut bmp = Vec::with_capacity(HEADER_LEN + pixels_len);
    // BITMAPFILEHEADER: magic, file size, two reserved words, pixel offset.
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_len.to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    // BITMAPINFOHEADER: size, width, height (positive: bottom-up), one plane,
    // 24 bits, BI_RGB (no compression), pixel bytes, resolution, no palette.
    bmp.extend_from_slice(&(INFO_HEADER_LEN as u32).to_le_bytes());
    bmp.extend_from_slice(&image.width().to_le_bytes());
    bmp.extend_from_slice(&image.height().to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&u32::try_from(pixels_len).map_err(too_large)?.to_le_bytes());
    bmp.extend_from_slice(&PIXELS_PER_METRE.to_le_bytes());
    bmp.extend_from_slice(&PIXELS_PER_METRE.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    let pixels = image.read_pixel_bytes();
    for y in (0..height).rev() {
        let start = y * stride;
        for x in 0..width {
            let at = start + x * channels;
            bmp.extend_from_slice(&[pixels[at + 2], pixels[at + 1], pixels[at]]);
        }
        bmp.resize(bmp.len() + row - width * 3, 0);
    }
    Ok(bmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_bottom_up_bgr_and_padded_to_four_bytes() {
        // 3x2: a 9-byte row needs 3 bytes of padding.
        let image = Pixbuf::new(Colorspace::Rgb, true, 8, 3, 2).unwrap();
        for y in 0..2u8 {
            for x in 0..3u8 {
                image.put_pixel(x.into(), y.into(), 10 * x + y, 100 + x, 200 + y, 7);
            }
        }
        let bmp = encode(&image).unwrap();
        assert_eq!(bmp.len(), 54 + 2 * 12);
        assert_eq!(bmp[..2], *b"BM");
        assert_eq!(bmp[2..6], 78u32.to_le_bytes());
        assert_eq!(bmp[18..26], [3, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(bmp[34..38], 24u32.to_le_bytes());
        // File row 0 is picture row 1; alpha is not written.
        assert_eq!(
            bmp[54..66],
            [201, 100, 1, 201, 101, 11, 201, 102, 21, 0, 0, 0]
        );
        assert_eq!(
            bmp[66..78],
            [200, 100, 0, 200, 101, 10, 200, 102, 20, 0, 0, 0]
        );
    }

    #[test]
    fn a_round_trip_through_gdk_pixbuf_keeps_every_pixel() {
        let image = Pixbuf::new(Colorspace::Rgb, false, 8, 5, 3).unwrap();
        for y in 0..3u8 {
            for x in 0..5u8 {
                image.put_pixel(x.into(), y.into(), 40 * x, 80 * y, 255 - x, 255);
            }
        }
        let decoded = Pixbuf::from_read(std::io::Cursor::new(encode(&image).unwrap())).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (5, 3));
        for y in 0..3usize {
            for x in 0..5usize {
                let at = |picture: &Pixbuf| {
                    let offset =
                        y * picture.rowstride() as usize + x * picture.n_channels() as usize;
                    picture.read_pixel_bytes()[offset..offset + 3].to_vec()
                };
                assert_eq!(at(&decoded), at(&image), "pixel {x},{y}");
            }
        }
    }
}
