use gtk::{gdk, gdk_pixbuf, gio};

const SVG_RESOURCE: &str = "/dev/streamdeck/EventMonitor/icons/scalable/apps/sdeck-plus.svg";

pub fn register() -> Result<(), Box<dyn std::error::Error>> {
    gio::resources_register_include!("icons.gresource")?;
    let display = gdk::Display::default()
        .ok_or_else(|| std::io::Error::other("GTK display is unavailable"))?;
    gtk::IconTheme::for_display(&display).add_resource_path("/dev/streamdeck/EventMonitor/icons");
    gtk::Window::set_default_icon_name(crate::APP_NAME);
    Ok(())
}

pub fn tray_pixmap() -> Result<ksni::Icon, Box<dyn std::error::Error>> {
    let pixbuf = gdk_pixbuf::Pixbuf::from_resource_at_scale(SVG_RESOURCE, 64, 64, true)?
        .add_alpha(false, 0, 0, 0)?;
    let pixels = pixbuf.read_pixel_bytes();
    Ok(ksni::Icon {
        width: pixbuf.width(),
        height: pixbuf.height(),
        data: argb_pixels(
            pixels.as_ref(),
            usize::try_from(pixbuf.width())?,
            usize::try_from(pixbuf.rowstride())?,
        ),
    })
}

fn argb_pixels(pixels: &[u8], width: usize, rowstride: usize) -> Vec<u8> {
    pixels
        .chunks(rowstride)
        .flat_map(|row| {
            row.as_chunks::<4>()
                .0
                .iter()
                .take(width)
                .flat_map(|rgba| [rgba[3], rgba[0], rgba[1], rgba[2]])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_pixels_keep_alpha_and_colors_without_row_padding() {
        let rgba = [255, 0, 0, 128, 42, 42, 42, 42, 0, 0, 255, 255];
        let argb = argb_pixels(&rgba, 1, 8);
        assert_eq!(argb, [128, 255, 0, 0, 255, 0, 0, 255]);
    }
}
