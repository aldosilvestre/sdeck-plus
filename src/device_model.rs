//! Registry of the Elgato Stream Deck models.
//!
//! Every device-shaped number (control counts, image sizes, report sizes)
//! lives in one [`DeviceModel`] so generic code takes a model instead of
//! hard-coding the Stream Deck +. Disputed values are per-model fields so they
//! can be corrected on real hardware without a redesign.
//!
//! The table describes every model Elgato documents, but only the entries whose
//! [`Support`] is [`Support::Supported`] are ever detected or driven. A
//! [`Support::Planned`] entry is data only: promoting it is a table edit plus
//! verification (see `DEVICES.md`).

use std::ops::RangeInclusive;

/// Rotation applied to an image before it is encoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rotation {
    None,
    Cw90,
    Ccw90,
    Rot180,
}

/// The wire protocol a model speaks. One handler per family implements
/// decoding and image output (see `protocol::protocol_for`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolFamily {
    /// Classic, XL, Neo, + and + XL: 0x01 input commands, 0x02 image reports
    /// (0x07 key, 0x0B/0x0C window) and 0x03 feature reports.
    Expanded,
    /// The original Mini: fixed 65-byte input without a command byte and
    /// 1024-byte output reports with a 16-byte header. Documented, not implemented.
    LegacyMini,
}

impl ProtocolFamily {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Expanded => "Expanded",
            Self::LegacyMini => "Legacy Mini",
        }
    }

    /// Whether this crate has a handler that can drive the family.
    pub const fn is_implemented(self) -> bool {
        matches!(self, Self::Expanded)
    }
}

/// Whether the application detects and drives a model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Support {
    /// Detected, driven and offered in "Preview as".
    Supported,
    /// Described from Elgato's documentation only; discovery never matches it.
    Planned,
}

/// Encoding of the pictures a model's displays take.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageFormat {
    Jpeg,
    Bmp,
}

impl ImageFormat {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Bmp => "BMP",
        }
    }
}

/// The wide display under the keys: a touch strip, or a plain window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StripSpec {
    /// Size (width, height) in pixels.
    pub px: (u32, u32),
    /// Touch sections along the strip (0 when it is not touch sensitive).
    pub zones: u8,
    pub touch: bool,
    pub rotation: Rotation,
    /// One touch section (width, height) in pixels.
    pub zone_px: (u32, u32),
}

#[derive(Debug, Eq, PartialEq)]
pub struct DeviceModel {
    /// Stable identifier used in file names and preferences.
    pub id: &'static str,
    pub name: &'static str,
    /// Informational model numbers and alternative names.
    pub aliases: &'static [&'static str],
    pub family: ProtocolFamily,
    pub support: Support,
    /// USB product ids of every hardware revision, first one primary.
    pub pids: &'static [u16],
    /// Elgato HID documentation page the entry is taken from.
    pub doc_url: &'static str,
    pub key_cols: u8,
    pub key_rows: u8,
    /// Dials are independent of touch sections.
    pub dials: u8,
    /// Capacitive sensors reported after the keys, as extra buttons.
    pub touch_buttons: u8,
    /// Side of a square key image in pixels.
    pub key_px: u32,
    pub key_rotation: Rotation,
    pub key_flip_y: bool,
    pub image_format: ImageFormat,
    /// The display under the keys, if the model has one.
    pub strip: Option<StripSpec>,
    /// Whether partial-window image headers swap x/y when rotated.
    pub partial_window_axis_swap: bool,
    pub input_report_len: usize,
    /// Bytes before the payload of an input report (command, length).
    pub input_header_len: usize,
    pub image_report_len: usize,
    pub key_header_len: usize,
    pub zone_header_len: usize,
}

const HID_DOCS: &str = "https://docs.elgato.com/streamdeck/hid/";

pub static PLUS: DeviceModel = DeviceModel {
    id: "plus",
    name: "Stream Deck +",
    aliases: &[],
    family: ProtocolFamily::Expanded,
    support: Support::Supported,
    pids: &[0x0084],
    doc_url: "https://docs.elgato.com/streamdeck/hid/stream-deck-plus",
    key_cols: 4,
    key_rows: 2,
    dials: 4,
    touch_buttons: 0,
    key_px: 120,
    key_rotation: Rotation::None,
    key_flip_y: false,
    image_format: ImageFormat::Jpeg,
    strip: Some(StripSpec {
        px: (800, 100),
        zones: 4,
        touch: true,
        rotation: Rotation::None,
        zone_px: (200, 100),
    }),
    partial_window_axis_swap: false,
    input_report_len: 64,
    input_header_len: 4,
    image_report_len: 1024,
    key_header_len: 8,
    zone_header_len: 16,
};

pub static PLUS_XL: DeviceModel = DeviceModel {
    id: "plus-xl",
    name: "Stream Deck + XL",
    aliases: &[],
    family: ProtocolFamily::Expanded,
    support: Support::Supported,
    pids: &[0x00C6],
    doc_url: "https://docs.elgato.com/streamdeck/hid/stream-deck-plus-xl",
    key_cols: 9,
    key_rows: 4,
    dials: 6,
    touch_buttons: 0,
    key_px: 112,
    key_rotation: Rotation::Ccw90,
    key_flip_y: false,
    image_format: ImageFormat::Jpeg,
    strip: Some(StripSpec {
        px: (1200, 100),
        zones: 6,
        touch: true,
        rotation: Rotation::Ccw90,
        zone_px: (200, 100),
    }),
    partial_window_axis_swap: false,
    input_report_len: 64,
    input_header_len: 4,
    image_report_len: 1024,
    key_header_len: 8,
    zone_header_len: 16,
};

/// Shared by the planned Expanded models, which differ only in their facts.
const EXPANDED: DeviceModel = DeviceModel {
    id: "",
    name: "",
    aliases: &[],
    family: ProtocolFamily::Expanded,
    support: Support::Planned,
    pids: &[],
    doc_url: HID_DOCS,
    key_cols: 0,
    key_rows: 0,
    dials: 0,
    touch_buttons: 0,
    key_px: 0,
    key_rotation: Rotation::Rot180,
    key_flip_y: false,
    image_format: ImageFormat::Jpeg,
    strip: None,
    partial_window_axis_swap: false,
    input_report_len: 64,
    input_header_len: 4,
    image_report_len: 1024,
    key_header_len: 8,
    zone_header_len: 16,
};

/// Planned. Facts from the Stream Deck Classic page. Its 480x272 LCD takes
/// background images (command 0x02 0x0D), not a window, so no strip is modelled.
pub static CLASSIC: DeviceModel = DeviceModel {
    id: "classic",
    name: "Stream Deck Classic",
    pids: &[0x006D, 0x0080, 0x00A5, 0x00B9],
    doc_url: "https://docs.elgato.com/streamdeck/hid/stream-deck-classic",
    key_cols: 5,
    key_rows: 3,
    key_px: 72,
    ..EXPANDED
};

/// Planned. Facts from the Stream Deck XL page.
pub static XL: DeviceModel = DeviceModel {
    id: "xl",
    name: "Stream Deck XL",
    pids: &[0x006C, 0x008F, 0x00BA],
    doc_url: "https://docs.elgato.com/streamdeck/hid/stream-deck-xl",
    key_cols: 8,
    key_rows: 4,
    key_px: 96,
    ..EXPANDED
};

/// Planned. Facts from the Stream Deck Neo page: an information window with no
/// touch, and two capacitive sensors reported after the keys.
pub static NEO: DeviceModel = DeviceModel {
    id: "neo",
    name: "Stream Deck Neo",
    pids: &[0x009A],
    doc_url: "https://docs.elgato.com/streamdeck/hid/stream-deck-neo",
    key_cols: 4,
    key_rows: 2,
    key_px: 96,
    touch_buttons: 2,
    strip: Some(StripSpec {
        px: (248, 58),
        zones: 0,
        touch: false,
        rotation: Rotation::None,
        zone_px: (248, 58),
    }),
    ..EXPANDED
};

/// Planned. Facts from the Stream Deck Mini page: the Legacy Mini protocol,
/// 80x80 BMP keys rotated 90 degrees clockwise, 16-byte output header and a
/// fixed 65-byte input report with no command byte.
pub static MINI: DeviceModel = DeviceModel {
    id: "mini",
    name: "Stream Deck Mini",
    family: ProtocolFamily::LegacyMini,
    pids: &[0x0063, 0x0090, 0x00B3, 0x00B8],
    doc_url: "https://docs.elgato.com/streamdeck/hid/mini",
    key_cols: 3,
    key_rows: 2,
    key_px: 80,
    key_rotation: Rotation::Cw90,
    image_format: ImageFormat::Bmp,
    input_report_len: 65,
    input_header_len: 0,
    key_header_len: 16,
    zone_header_len: 0,
    ..EXPANDED
};

static ALL: [&DeviceModel; 6] = [&PLUS, &PLUS_XL, &CLASSIC, &XL, &NEO, &MINI];

impl DeviceModel {
    /// Every model in the table, Supported or Planned.
    pub fn all() -> &'static [&'static DeviceModel] {
        &ALL
    }

    /// The models the application detects and drives.
    pub fn supported() -> impl Iterator<Item = &'static DeviceModel> {
        ALL.into_iter().filter(|model| model.is_supported())
    }

    /// The model assumed when nothing else is known.
    pub fn default_model() -> &'static DeviceModel {
        &PLUS
    }

    /// The model discovery assigns to `pid`: Supported models only, so a
    /// Planned model's product id is ignored.
    pub fn by_pid_supported(pid: u16) -> Option<&'static DeviceModel> {
        Self::supported().find(|model| model.pids.contains(&pid))
    }

    /// Same as [`Self::by_pid_supported`]; the name discovery callers use.
    pub fn by_pid(pid: u16) -> Option<&'static DeviceModel> {
        Self::by_pid_supported(pid)
    }

    /// The registry entry owning `pid`, Planned or not. Informational only.
    pub fn by_pid_any(pid: u16) -> Option<&'static DeviceModel> {
        ALL.into_iter().find(|model| model.pids.contains(&pid))
    }

    /// Any registry entry by id, Planned or not.
    pub fn by_id(id: &str) -> Option<&'static DeviceModel> {
        ALL.into_iter().find(|model| model.id == id)
    }

    /// A Supported entry by id. Preferences and anything that must name a
    /// connectable model use this; [`Self::by_id`] also finds Planned ones.
    pub fn by_id_supported(id: &str) -> Option<&'static DeviceModel> {
        Self::by_id(id).filter(|model| model.is_supported())
    }

    pub fn is_supported(&self) -> bool {
        self.support == Support::Supported
    }

    /// The primary product id.
    pub fn pid(&self) -> u16 {
        self.pids.first().copied().unwrap_or_default()
    }

    /// Whether this crate can talk to the model's protocol family.
    ///
    /// # Errors
    /// A message naming the model and family when no handler exists.
    pub fn ensure_protocol(&self) -> Result<(), String> {
        if self.family.is_implemented() {
            Ok(())
        } else {
            Err(format!(
                "{} uses the {} protocol, which is not implemented",
                self.name,
                self.family.name()
            ))
        }
    }

    pub const fn keys(&self) -> u8 {
        self.key_cols * self.key_rows
    }

    /// Keys plus the sensors the input report appends after them.
    pub const fn key_inputs(&self) -> u8 {
        self.keys() + self.touch_buttons
    }

    pub const fn has_strip(&self) -> bool {
        self.strip.is_some()
    }

    /// Whether the strip senses touch (a window such as the Neo's does not).
    pub const fn has_touch(&self) -> bool {
        matches!(self.strip, Some(StripSpec { touch: true, .. }))
    }

    /// Touch sections along the strip; 0 without one.
    pub const fn zones(&self) -> u8 {
        match self.strip {
            Some(strip) => strip.zones,
            None => 0,
        }
    }

    /// Strip size in pixels, `(0, 0)` without a strip.
    pub const fn strip_px(&self) -> (u32, u32) {
        match self.strip {
            Some(strip) => strip.px,
            None => (0, 0),
        }
    }

    /// One touch section in pixels, `(0, 0)` without a strip.
    pub const fn zone_px(&self) -> (u32, u32) {
        match self.strip {
            Some(strip) => strip.zone_px,
            None => (0, 0),
        }
    }

    pub const fn strip_rotation(&self) -> Rotation {
        match self.strip {
            Some(strip) => strip.rotation,
            None => Rotation::None,
        }
    }

    /// One-based key numbers.
    pub fn key_numbers(&self) -> RangeInclusive<u8> {
        1..=self.keys()
    }

    /// One-based dial numbers.
    pub fn dial_numbers(&self) -> RangeInclusive<u8> {
        1..=self.dials
    }

    /// One-based touch section numbers.
    pub fn zone_numbers(&self) -> RangeInclusive<u8> {
        1..=self.zones()
    }

    pub fn is_valid_key(&self, number: u8) -> bool {
        self.key_numbers().contains(&number)
    }

    pub fn is_valid_dial(&self, number: u8) -> bool {
        self.dial_numbers().contains(&number)
    }

    pub fn is_valid_zone(&self, number: u8) -> bool {
        self.zone_numbers().contains(&number)
    }

    /// Left edge of one-based touch section `number` on the strip.
    pub const fn zone_x(&self, number: u8) -> u32 {
        (number as u32 - 1) * self.zone_px().0
    }

    /// One-based touch section containing strip coordinate `x`, if any.
    pub fn zone_index_from_x(&self, x: u32) -> Option<u8> {
        let width = self.zone_px().0;
        if !self.has_touch() || width == 0 {
            return None;
        }
        let zone = x / width + 1;
        (x < self.strip_px().0 && zone <= u32::from(self.zones())).then_some(zone as u8)
    }

    /// Whether `zone` is the first or last section (page indicator edges).
    pub fn is_first_zone(&self, zone: u8) -> bool {
        zone == 1
    }

    pub fn is_last_zone(&self, zone: u8) -> bool {
        zone == self.zones()
    }
}
