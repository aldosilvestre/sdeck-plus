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
//! verification (see `DEVICES.md`). Every entry is supported today; the
//! Classic, XL, Neo and Mini are validated with the simulator only.
//!
//! Beside the table, [`VIRTUAL_CLASSIC`] is a [`Support::Virtual`] model: a
//! Stream Deck Classic the application serves itself (a web page). It is found
//! by id and runs a session like hardware, but it has no product id, so USB
//! discovery, udev rules, previews and screenshots (which iterate
//! [`DeviceModel::supported`]) never see it.

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
    /// (0x07 key, 0x0B/0x0C window, 0x08 full screen for a keypad background)
    /// and 0x03 feature reports.
    Expanded,
    /// The original Mini: a fixed 65-byte input report (report id 0x01, then
    /// one state byte per key, no command byte), BMP key images in 1024-byte
    /// `02 01` output reports with a 16-byte header, and feature reports of
    /// its own. Implemented from Elgato's Mini page and validated with the
    /// simulator only.
    LegacyMini,
}

impl ProtocolFamily {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Expanded => "Expanded",
            Self::LegacyMini => "Legacy Mini",
        }
    }

    /// Whether this crate has a handler that can drive the family (every
    /// family does; a new one starts out without).
    pub const fn is_implemented(self) -> bool {
        matches!(self, Self::Expanded | Self::LegacyMini)
    }
}

/// Whether the application detects and drives a model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Support {
    /// Detected, driven and offered in the Current selector while connected.
    Supported,
    /// Described from Elgato's documentation only; discovery never matches it.
    Planned,
    /// Served by the application itself, never discovered over USB: no product
    /// id and no udev rule. Live only while the application attaches it.
    Virtual,
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

/// A picture that covers the whole LCD behind the keys (Elgato's "Full LCD
/// Image", sent with Update Full Screen Image `02 08`). Each key shows its
/// own picture over its part of the LCD; a key without one shows the part of
/// this picture under it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeypadBackground {
    /// Size (width, height) of the LCD picture in pixels.
    pub px: (u32, u32),
    /// Rotation applied before upload.
    pub rotation: Rotation,
    /// Top-left corner of key 1 on the LCD, in the upright picture.
    pub first_key: (u32, u32),
    /// Distance from one key to the next (the key plus the gap), across and down.
    pub pitch: (u32, u32),
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
    /// The picture behind the keys, if the application offers one for the
    /// model (see [`KeypadBackground`]).
    pub keypad_background: Option<KeypadBackground>,
    /// Whether partial-window image headers swap x/y when rotated.
    pub partial_window_axis_swap: bool,
    pub input_report_len: usize,
    /// Bytes before the payload of an input report: the report id, plus the
    /// command and length on Expanded models.
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
    keypad_background: None,
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
    keypad_background: None,
    partial_window_axis_swap: false,
    input_report_len: 64,
    input_header_len: 4,
    image_report_len: 1024,
    key_header_len: 8,
    zone_header_len: 16,
};

/// Shared by the keypad models (Classic, XL, Neo, Mini), which differ only in
/// their facts. A new entry built from it starts out Planned.
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
    keypad_background: None,
    partial_window_axis_swap: false,
    input_report_len: 64,
    input_header_len: 4,
    image_report_len: 1024,
    key_header_len: 8,
    zone_header_len: 16,
};

/// Facts from the Stream Deck Classic page (the MK.2 family; PID 0x0060, the
/// original with its own protocol, is not included; the page says every PID
/// of the family shares one command set). Its keys sit on one 480x272 LCD,
/// which is not a window, so no strip is modelled. The keypad background is
/// the page's "Full LCD Image": 480 x 272, "Rotate the image 180° before
/// upload", JPEG, sent with Update Full Screen Image (`02 08`). The page also
/// lists Update Background (`02 0D`), which only stores a picture at an index;
/// it documents no way to show a stored one on the Classic (Show Background
/// by Index, `03 13`, is on the XL page only), so it is not used. Key 1 sits
/// 11 px from the left and 5 px from the top, keys 72 px with 25 px between
/// them ("Unit Pixel Layout"). Validated with the simulator only.
pub static CLASSIC: DeviceModel = CLASSIC_FACTS;

/// The Classic's row, shared by [`CLASSIC`] and [`VIRTUAL_CLASSIC`].
const CLASSIC_FACTS: DeviceModel = DeviceModel {
    id: "classic",
    name: "Stream Deck Classic",
    support: Support::Supported,
    pids: &[0x006D, 0x0080, 0x00A5, 0x00B9],
    doc_url: "https://docs.elgato.com/streamdeck/hid/stream-deck-classic",
    key_cols: 5,
    key_rows: 3,
    key_px: 72,
    keypad_background: Some(KeypadBackground {
        px: (480, 272),
        rotation: Rotation::Rot180,
        first_key: (11, 5),
        pitch: (97, 97),
    }),
    ..EXPANDED
};

/// The Virtual Deck: a deck the application serves itself (shown in a web
/// page). Internally it is a Stream Deck Classic (the Classic's keys, keypad
/// background, images and reports) under an identity of its own, so it keeps
/// its own data root (`devices/virtual-classic/`) and session and runs
/// alongside a real Classic. It has no product id: USB discovery never finds
/// it and no udev rule names it. Users only ever see its name, "Virtual Deck"
/// (L50); the id `virtual-classic` and the data root stay, so no data moves.
pub static VIRTUAL_CLASSIC: DeviceModel = DeviceModel {
    id: "virtual-classic",
    name: "Virtual Deck",
    aliases: &[],
    support: Support::Virtual,
    pids: &[],
    ..CLASSIC_FACTS
};

static VIRTUAL: [&DeviceModel; 1] = [&VIRTUAL_CLASSIC];

/// Facts from the Stream Deck XL page. Validated with the simulator only.
/// Elgato documents a 1024 x 600 Full LCD Image for it too; the application
/// offers a keypad background on the Classic only.
pub static XL: DeviceModel = DeviceModel {
    id: "xl",
    name: "Stream Deck XL",
    support: Support::Supported,
    pids: &[0x006C, 0x008F, 0x00BA],
    doc_url: "https://docs.elgato.com/streamdeck/hid/stream-deck-xl",
    key_cols: 8,
    key_rows: 4,
    key_px: 96,
    ..EXPANDED
};

/// Facts from the Stream Deck Neo page: an information window with no touch,
/// and two capacitive sensors reported after the keys. The page tells to
/// "Rotate the image 180° before upload" for the keys and for the 248 x 58
/// window alike (Update Window Image, 0x0B). Validated with the simulator
/// only.
pub static NEO: DeviceModel = DeviceModel {
    id: "neo",
    name: "Stream Deck Neo",
    support: Support::Supported,
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
        rotation: Rotation::Rot180,
        zone_px: (248, 58),
    }),
    ..EXPANDED
};

/// Facts from the Stream Deck Mini page: the Legacy Mini protocol, 80x80 BMP
/// keys, 16-byte output header and a fixed 65-byte input report whose byte 0
/// is the report id, with no command byte. Validated with the simulator only.
///
/// The key index (one-based on the wire, `protocol::MINI_KEY_INDEX_BASE`) and
/// the orientation follow the open-source drivers that run on real Minis
/// (python-elgato-streamdeck, node-elgato-stream-deck): they send key + 1 and
/// transpose the key picture, which is `Ccw90` with `key_flip_y`. Elgato's
/// page says "rotated 90 degrees clockwise" without a flip and only shows a
/// 0-5 layout figure; UNVERIFIED on our side (no hardware). If a real Mini
/// shows its pictures mirrored, `Cw90` without a flip is the correction.
pub static MINI: DeviceModel = DeviceModel {
    id: "mini",
    name: "Stream Deck Mini",
    family: ProtocolFamily::LegacyMini,
    support: Support::Supported,
    pids: &[0x0063, 0x0090, 0x00B3, 0x00B8],
    doc_url: "https://docs.elgato.com/streamdeck/hid/mini",
    key_cols: 3,
    key_rows: 2,
    key_px: 80,
    key_rotation: Rotation::Ccw90,
    key_flip_y: true,
    image_format: ImageFormat::Bmp,
    input_report_len: 65,
    input_header_len: 1,
    key_header_len: 16,
    zone_header_len: 0,
    ..EXPANDED
};

static ALL: [&DeviceModel; 6] = [&PLUS, &PLUS_XL, &CLASSIC, &XL, &NEO, &MINI];

impl DeviceModel {
    /// Every model in the table, Supported or Planned. Virtual models are not
    /// part of it (see [`Self::virtual_models`]).
    pub fn all() -> &'static [&'static DeviceModel] {
        &ALL
    }

    /// The models the application detects and drives over USB.
    pub fn supported() -> impl Iterator<Item = &'static DeviceModel> {
        ALL.into_iter().filter(|model| model.is_supported())
    }

    /// The models the application serves itself instead of finding them.
    pub fn virtual_models() -> &'static [&'static DeviceModel] {
        &VIRTUAL
    }

    /// Every model a session can run: the supported hardware in table order,
    /// then the virtual models.
    pub fn connectable() -> impl Iterator<Item = &'static DeviceModel> {
        Self::supported().chain(VIRTUAL)
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

    /// Any model by id: a registry entry, Planned or not, or a virtual model.
    pub fn by_id(id: &str) -> Option<&'static DeviceModel> {
        ALL.into_iter().chain(VIRTUAL).find(|model| model.id == id)
    }

    /// A Supported entry by id. Preferences and anything that must name a
    /// USB model use this; [`Self::by_id`] also finds Planned and virtual ones.
    pub fn by_id_supported(id: &str) -> Option<&'static DeviceModel> {
        Self::by_id(id).filter(|model| model.is_supported())
    }

    pub fn is_supported(&self) -> bool {
        self.support == Support::Supported
    }

    /// Whether the application serves the model itself (see [`Support::Virtual`]).
    pub fn is_virtual(&self) -> bool {
        self.support == Support::Virtual
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

    /// Whether the display under the keys is an information window that
    /// senses no touch (the Neo's): it shows each page's picture and live
    /// information, and takes no input.
    pub const fn has_info_window(&self) -> bool {
        matches!(self.strip, Some(StripSpec { touch: false, .. }))
    }

    /// Whether each page can show a picture behind the keys (the Classic's).
    pub const fn has_keypad_background(&self) -> bool {
        self.keypad_background.is_some()
    }

    /// Where one-based `key` sits on the keypad background, as `(x, y, width,
    /// height)` in the upright picture; `None` without a background or for a
    /// key the model lacks.
    pub fn key_on_background(&self, key: u8) -> Option<(u32, u32, u32, u32)> {
        let background = self.keypad_background?;
        if !self.is_valid_key(key) {
            return None;
        }
        let index = u32::from(key - 1);
        let (column, row) = (
            index % u32::from(self.key_cols),
            index / u32::from(self.key_cols),
        );
        Some((
            background.first_key.0 + column * background.pitch.0,
            background.first_key.1 + row * background.pitch.1,
            self.key_px,
            self.key_px,
        ))
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

    /// One-based touch button numbers (empty without touch buttons).
    pub fn touch_button_numbers(&self) -> RangeInclusive<u8> {
        1..=self.touch_buttons
    }

    pub fn is_valid_touch_button(&self, number: u8) -> bool {
        self.touch_button_numbers().contains(&number)
    }

    /// The one-based touch button a zero-based HID key `index` reports: the
    /// sensors follow the keys in the input report. `None` for a key or an
    /// index past the sensors.
    pub fn touch_button_at(&self, index: usize) -> Option<u8> {
        let number = index.checked_sub(usize::from(self.keys()))? + 1;
        u8::try_from(number)
            .ok()
            .filter(|number| self.is_valid_touch_button(*number))
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

    /// The controls that respond while the model is connected, in plain words
    /// ("keys, dials and touch strip").
    pub fn live_controls(&self) -> String {
        Self::live_controls_of(&[self])
    }

    /// [`Self::live_controls`] for several connected models: every control
    /// any of them has, in a fixed order.
    pub fn live_controls_of(models: &[&DeviceModel]) -> String {
        let any = |test: fn(&DeviceModel) -> bool| models.iter().any(|model| test(model));
        let parts: Vec<&str> = [
            ("keys", true),
            ("dials", any(|model| model.dials > 0)),
            ("touch buttons", any(|model| model.touch_buttons > 0)),
            ("touch strip", any(|model| model.has_touch())),
            (
                "info window",
                any(|model| model.has_strip() && !model.has_touch()),
            ),
        ]
        .into_iter()
        .filter_map(|(name, present)| present.then_some(name))
        .collect();
        match parts.as_slice() {
            [only] => (*only).to_owned(),
            [first @ .., last] => format!("{} and {last}", first.join(", ")),
            [] => String::new(),
        }
    }

    /// Every product id of the model as hexadecimal, joined by `separator`.
    pub fn pid_list(&self, separator: &str) -> String {
        self.pids
            .iter()
            .map(|pid| format!("{pid:04X}"))
            .collect::<Vec<_>>()
            .join(separator)
    }
}
