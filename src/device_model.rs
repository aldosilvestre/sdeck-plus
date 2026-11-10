//! Static description of each supported Stream Deck + model.
//!
//! Every device-shaped number (control counts, image sizes, report sizes)
//! lives in one [`DeviceModel`] so generic code takes a model instead of
//! hard-coding the Stream Deck +. Disputed values are per-model fields so they
//! can be corrected on real hardware without a redesign.

use std::ops::RangeInclusive;

/// Rotation applied to an image before JPEG encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rotation {
    None,
    Ccw90,
}

#[derive(Debug, Eq, PartialEq)]
pub struct DeviceModel {
    /// Stable identifier used in file names and preferences.
    pub id: &'static str,
    pub name: &'static str,
    pub pid: u16,
    pub key_cols: u8,
    pub key_rows: u8,
    pub dials: u8,
    pub zones: u8,
    /// Side of a square key image in pixels.
    pub key_px: u32,
    pub key_rotation: Rotation,
    pub key_flip_y: bool,
    /// Touch strip size (width, height) in pixels.
    pub strip_px: (u32, u32),
    /// One touch section (width, height) in pixels.
    pub zone_px: (u32, u32),
    pub strip_rotation: Rotation,
    /// Whether partial-window image headers swap x/y when rotated.
    pub partial_window_axis_swap: bool,
    pub input_report_len: usize,
    pub image_report_len: usize,
    pub key_header_len: usize,
    pub zone_header_len: usize,
}

pub static PLUS: DeviceModel = DeviceModel {
    id: "plus",
    name: "Stream Deck +",
    pid: 0x0084,
    key_cols: 4,
    key_rows: 2,
    dials: 4,
    zones: 4,
    key_px: 120,
    key_rotation: Rotation::None,
    key_flip_y: false,
    strip_px: (800, 100),
    zone_px: (200, 100),
    strip_rotation: Rotation::None,
    partial_window_axis_swap: false,
    input_report_len: 64,
    image_report_len: 1024,
    key_header_len: 8,
    zone_header_len: 16,
};

pub static PLUS_XL: DeviceModel = DeviceModel {
    id: "plus-xl",
    name: "Stream Deck + XL",
    pid: 0x00C6,
    key_cols: 9,
    key_rows: 4,
    dials: 6,
    zones: 6,
    key_px: 112,
    key_rotation: Rotation::Ccw90,
    key_flip_y: false,
    strip_px: (1200, 100),
    zone_px: (200, 100),
    strip_rotation: Rotation::Ccw90,
    partial_window_axis_swap: false,
    input_report_len: 64,
    image_report_len: 1024,
    key_header_len: 8,
    zone_header_len: 16,
};

static ALL: [&DeviceModel; 2] = [&PLUS, &PLUS_XL];

impl DeviceModel {
    /// Models in detection order.
    pub fn all() -> &'static [&'static DeviceModel] {
        &ALL
    }

    /// The model assumed when nothing else is known.
    pub fn default_model() -> &'static DeviceModel {
        &PLUS
    }

    pub fn by_pid(pid: u16) -> Option<&'static DeviceModel> {
        ALL.into_iter().find(|model| model.pid == pid)
    }

    pub fn by_id(id: &str) -> Option<&'static DeviceModel> {
        ALL.into_iter().find(|model| model.id == id)
    }

    pub const fn keys(&self) -> u8 {
        self.key_cols * self.key_rows
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
        1..=self.zones
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
        (number as u32 - 1) * self.zone_px.0
    }

    /// One-based touch section containing strip coordinate `x`, if any.
    pub fn zone_index_from_x(&self, x: u32) -> Option<u8> {
        let zone = x / self.zone_px.0 + 1;
        (x < self.strip_px.0 && zone <= u32::from(self.zones)).then_some(zone as u8)
    }

    /// Whether `zone` is the first or last section (page indicator edges).
    pub fn is_first_zone(&self, zone: u8) -> bool {
        zone == 1
    }

    pub fn is_last_zone(&self, zone: u8) -> bool {
        zone == self.zones
    }
}
