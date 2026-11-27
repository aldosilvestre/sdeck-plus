//! The QR code of the page address: the `qrcode` crate makes the modules and
//! the app draws them itself, whole pixels per module, dark on a white
//! rounded square, as in the artboard.
use gtk::{cairo, prelude::*};
use std::{cell::RefCell, rc::Rc};

/// White modules around the code, as in the artboard (`viewBox -2 -2 29 29`).
const QUIET_ZONE: usize = 2;
/// The artboard's 8px corner of the white square.
const RADIUS: f64 = 8.0;
/// `desk_bg` of the dark theme: the artboard's module colour.
const DARK: (f64, f64, f64) = (
    0x15 as f64 / 255.0,
    0x19 as f64 / 255.0,
    0x1f as f64 / 255.0,
);

/// The modules of a QR code, row-major; `true` is dark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Matrix {
    width: usize,
    dark: Vec<bool>,
}

impl Matrix {
    /// The code of `text` at the default (medium) error correction.
    pub(crate) fn encode(text: &str) -> Result<Self, String> {
        let code = qrcode::QrCode::new(text.as_bytes())
            .map_err(|error| format!("Could not make a QR code of the address: {error}"))?;
        Ok(Self {
            width: code.width(),
            dark: code
                .to_colors()
                .into_iter()
                .map(|color| color == qrcode::Color::Dark)
                .collect(),
        })
    }

    /// Modules per side.
    pub(crate) fn width(&self) -> usize {
        self.width
    }

    pub(crate) fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.width && self.dark[y * self.width + x]
    }
}

/// A square picture of one QR code, named "QR code of <text>" for assistive
/// technology.
pub(crate) struct QrView {
    pub(crate) area: gtk::DrawingArea,
    matrix: Rc<RefCell<Option<Matrix>>>,
    text: RefCell<String>,
}

impl QrView {
    pub(crate) fn new(size: i32) -> Self {
        let area = gtk::DrawingArea::builder()
            .content_width(size)
            .content_height(size)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .accessible_role(gtk::AccessibleRole::Img)
            .build();
        area.add_css_class("virtual-deck-qr");
        let matrix: Rc<RefCell<Option<Matrix>>> = Rc::default();
        let drawn = matrix.clone();
        area.set_draw_func(move |_, context, width, height| {
            draw(context, width.min(height), drawn.borrow().as_ref());
        });
        Self {
            area,
            matrix,
            text: RefCell::default(),
        }
    }

    /// Show the code of `text`; an address too long for a QR code shows the
    /// white square alone and says why.
    pub(crate) fn show(&self, text: &str) -> Result<(), String> {
        let encoded = Matrix::encode(text);
        let result = encoded.as_ref().map(|_| ()).map_err(Clone::clone);
        *self.matrix.borrow_mut() = encoded.ok();
        *self.text.borrow_mut() = text.to_string();
        self.area
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "QR code of {text}"
            ))]);
        self.area
            .set_tooltip_text(Some(&format!("QR code of {text}")));
        self.area.queue_draw();
        result
    }

    /// What the code says.
    #[cfg(test)]
    pub(crate) fn text(&self) -> String {
        self.text.borrow().clone()
    }

    /// The modules drawn.
    #[cfg(test)]
    pub(crate) fn matrix(&self) -> Option<Matrix> {
        self.matrix.borrow().clone()
    }
}

/// White rounded square of `size` pixels, then the dark modules, centred, each
/// a whole number of pixels so the code stays crisp.
fn draw(context: &cairo::Context, size: i32, matrix: Option<&Matrix>) {
    let side = f64::from(size.max(0));
    context.new_sub_path();
    context.arc(side - RADIUS, RADIUS, RADIUS, -90f64.to_radians(), 0.0);
    context.arc(
        side - RADIUS,
        side - RADIUS,
        RADIUS,
        0.0,
        90f64.to_radians(),
    );
    context.arc(
        RADIUS,
        side - RADIUS,
        RADIUS,
        90f64.to_radians(),
        180f64.to_radians(),
    );
    context.arc(
        RADIUS,
        RADIUS,
        RADIUS,
        180f64.to_radians(),
        270f64.to_radians(),
    );
    context.close_path();
    context.set_source_rgb(1.0, 1.0, 1.0);
    let _ = context.fill();
    let Some(matrix) = matrix else {
        return;
    };
    let pixels = usize::try_from(size).unwrap_or(0);
    let module = (pixels / (matrix.width() + 2 * QUIET_ZONE)).max(1);
    let offset = pixels.saturating_sub(module * matrix.width()) / 2;
    context.set_antialias(cairo::Antialias::None);
    context.set_source_rgb(DARK.0, DARK.1, DARK.2);
    for y in 0..matrix.width() {
        for x in 0..matrix.width() {
            if matrix.is_dark(x, y) {
                context.rectangle(
                    (offset + x * module) as f64,
                    (offset + y * module) as f64,
                    module as f64,
                    module as f64,
                );
            }
        }
    }
    let _ = context.fill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_encodes_with_finder_patterns_in_three_corners() {
        let matrix = Matrix::encode("https://192.168.100.200:65535/").unwrap();
        let width = matrix.width();
        assert!(
            width >= 21 && (width - 21).is_multiple_of(4),
            "width {width}"
        );
        // Each finder pattern: a dark 7x7 ring around a light ring and a dark 3x3 core.
        for (left, top) in [(0, 0), (width - 7, 0), (0, width - 7)] {
            for (x, y) in [(0, 0), (6, 0), (0, 6), (6, 6), (3, 3), (2, 4)] {
                assert!(matrix.is_dark(left + x, top + y), "dark at {x},{y}");
            }
            for (x, y) in [(1, 1), (5, 1), (1, 5), (5, 5)] {
                assert!(!matrix.is_dark(left + x, top + y), "light at {x},{y}");
            }
        }
        assert!(!matrix.is_dark(width, 0), "outside the code is light");
    }

    #[test]
    fn different_addresses_make_different_codes() {
        assert_ne!(
            Matrix::encode("https://127.0.0.1:8765/").unwrap(),
            Matrix::encode("https://127.0.0.1:8766/").unwrap()
        );
    }
}
