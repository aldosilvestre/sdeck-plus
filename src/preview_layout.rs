//! Pure geometry of the hardware preview for any device model.
//!
//! Every coordinate is in preview pixels at scale 1, measured from the top-left
//! corner of the chassis. Nothing here touches GTK, so the same numbers drive
//! the widgets, the tests and the documentation.
//!
//! The Stream Deck + keeps the geometry it has always shipped (taken from the
//! reference image); every other model is laid out from one generic rule set:
//! 44 px keys on a 52 px pitch, touch sections 76 px wide, dials centred under
//! their sections, and a chassis just wide enough for the widest row. The XL+
//! uses the same rules with 40 px keys on a 46 px pitch and a tighter bottom
//! margin so its four rows fit the default window.
//!
//! A Stream Deck Neo's information window is drawn as a plain, non-interactive
//! display (a [`StripLayout`] without zones and with `touch == false`); its two
//! capacitive sensors are not drawn at all because they have no preview control.

use crate::device_model::DeviceModel;

/// The previews are always shown at their natural size.
pub const PREVIEW_SCALE: f64 = 1.0;
/// Smallest side of any interactive control, in pixels at [`PREVIEW_SCALE`].
pub const MIN_HIT_PX: i32 = 24;
/// Side of the action icon drawn on a dial's knob.
pub const DIAL_ICON_PX: i32 = 20;

const BRAND_Y: i32 = 6;
const BRAND_H: i32 = 24;
const KEYS_Y: i32 = 38;
const KEY_TO_BEZEL: i32 = 12;
const BEZEL_H: i32 = 77;
const BEZEL_INSET: i32 = 23;
const HEADER_Y: i32 = 3;
const HEADER_H: i32 = 16;
const STRIP_Y: i32 = 25;
const STRIP_H: i32 = 41;
const BEZEL_TO_RAIL: i32 = 19;
const RAIL_H: i32 = 108;
const DIAL_PX: i32 = 46;
const DIAL_Y: i32 = 34;
const LEGEND_Y: i32 = 84;
const LEGEND_H: i32 = 16;
const BOTTOM_MARGIN: i32 = 33;
const KEYS_ONLY_BOTTOM_MARGIN: i32 = 24;
const XL_BOTTOM_MARGIN: i32 = 24;
const MIN_WIDTH: i32 = 240;

/// An axis-aligned rectangle in preview pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// Whether the two rectangles share any area (touching edges do not).
    pub const fn overlaps(&self, other: &Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// Whether `other` lies entirely within `self`.
    pub const fn contains(&self, other: &Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    /// The same rectangle expressed relative to `origin`'s top-left corner.
    pub const fn relative_to(&self, origin: &Self) -> Self {
        Self::new(self.x - origin.x, self.y - origin.y, self.w, self.h)
    }
}

/// A control of the preview, numbered from one like the device's own inputs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ControlRef {
    Key(u8),
    /// The whole touch strip.
    Strip,
    Dial(u8),
    /// One touch section of the strip.
    Zone(u8),
}

impl ControlRef {
    /// The accessible name of a control; the strip has none of its own.
    pub fn accessible_name(&self) -> Option<String> {
        match self {
            Self::Key(number) => Some(format!("Button {number}")),
            Self::Dial(number) => Some(format!("Encoder {number}")),
            Self::Zone(number) => Some(format!("Touch zone {number}")),
            Self::Strip => None,
        }
    }
}

/// The display under the keys, with its optional touch sections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StripLayout {
    /// The frame that holds the header line and the strip.
    pub bezel: Rect,
    /// The status line above the strip (live text and gesture badges).
    pub header: Rect,
    pub rect: Rect,
    /// Whether the display senses touch; a plain window is decoration only.
    pub touch: bool,
    /// One rectangle per touch section, left to right.
    pub zones: Vec<Rect>,
}

/// The dials and their captions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialLayout {
    /// The recessed rail the dials sit in.
    pub rail: Rect,
    pub dials: Vec<Rect>,
    /// The action icon drawn on each knob.
    pub icons: Vec<Rect>,
    /// The assigned-action caption under each knob.
    pub legends: Vec<Rect>,
}

/// Where everything of one model sits on its chassis.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewLayout {
    pub model_id: &'static str,
    /// Chassis width and height.
    pub size: (i32, i32),
    pub scale: f64,
    /// The model's name in capitals, as printed on the device.
    pub brand: String,
    pub brand_rect: Rect,
    /// Keys in row-major order: element `n - 1` is key `n`.
    pub keys: Vec<Rect>,
    pub strip: Option<StripLayout>,
    pub dial_rail: Option<DialLayout>,
}

/// How one family of models is drawn.
struct Metrics {
    key: i32,
    pitch_x: i32,
    pitch_y: i32,
    zone_w: i32,
    /// Distance between neighbouring dials; `None` centres each under its section.
    dial_pitch: Option<i32>,
    /// Rail-relative x of the first dial when `dial_pitch` is set.
    dial_x0: i32,
    legend_w: i32,
    /// Space under the last row when the model has a strip or dials.
    bottom_margin: i32,
    /// A shipped chassis width, left key margin and bezel margin.
    fixed: Option<(i32, i32, i32)>,
}

const PLUS_METRICS: Metrics = Metrics {
    key: 60,
    pitch_x: 91,
    pitch_y: 67,
    zone_w: 82,
    dial_pitch: Some(91),
    dial_x0: 28,
    legend_w: 86,
    bottom_margin: BOTTOM_MARGIN,
    fixed: Some((426, 48, 27)),
};

const GENERIC_METRICS: Metrics = Metrics {
    key: 44,
    pitch_x: 52,
    pitch_y: 52,
    zone_w: 76,
    dial_pitch: None,
    dial_x0: 0,
    legend_w: 72,
    bottom_margin: BOTTOM_MARGIN,
    fixed: None,
};

/// The XL+ has four rows of keys, so its keys are a little smaller and its
/// bottom margin tighter: the whole card then fits the default 1000x700 window
/// without the page scrolling.
const XL_METRICS: Metrics = Metrics {
    key: 40,
    pitch_x: 46,
    pitch_y: 46,
    bottom_margin: XL_BOTTOM_MARGIN,
    ..GENERIC_METRICS
};

impl PreviewLayout {
    /// The layout of `model`. Deterministic: equal models give equal layouts.
    pub fn for_model(model: &DeviceModel) -> Self {
        // The Stream Deck + keeps its shipped geometry; see the module notes.
        let metrics = match model.id {
            id if id == crate::device_model::PLUS.id => &PLUS_METRICS,
            id if id == crate::device_model::PLUS_XL.id => &XL_METRICS,
            _ => &GENERIC_METRICS,
        };
        build(model, metrics)
    }

    /// Every control in the order the preview offers them: keys row-major, then
    /// the strip, its sections and the dials, as the widgets are created.
    pub fn controls(&self) -> Vec<ControlRef> {
        let mut controls: Vec<ControlRef> = (1..=self.keys.len())
            .map(|number| ControlRef::Key(number as u8))
            .collect();
        if let Some(strip) = self.strip.as_ref().filter(|strip| strip.touch) {
            controls.push(ControlRef::Strip);
            controls.extend((1..=strip.zones.len()).map(|number| ControlRef::Zone(number as u8)));
        }
        if let Some(rail) = &self.dial_rail {
            controls.extend((1..=rail.dials.len()).map(|number| ControlRef::Dial(number as u8)));
        }
        controls
    }

    /// Whether `control` exists on this model.
    pub fn has(&self, control: ControlRef) -> bool {
        self.rect_of(control).is_some()
    }

    /// The chassis rectangle of `control`, if the model has it.
    pub fn rect_of(&self, control: ControlRef) -> Option<Rect> {
        let index = |number: u8| usize::from(number).checked_sub(1);
        match control {
            ControlRef::Key(number) => self.keys.get(index(number)?).copied(),
            ControlRef::Strip => self
                .strip
                .as_ref()
                .filter(|strip| strip.touch)
                .map(|strip| strip.rect),
            ControlRef::Zone(number) => self
                .strip
                .as_ref()
                .and_then(|strip| strip.zones.get(index(number)?).copied()),
            ControlRef::Dial(number) => self
                .dial_rail
                .as_ref()
                .and_then(|rail| rail.dials.get(index(number)?).copied()),
        }
    }

    pub fn chassis(&self) -> Rect {
        Rect::new(0, 0, self.size.0, self.size.1)
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    pub fn dial_count(&self) -> usize {
        self.dial_rail.as_ref().map_or(0, |rail| rail.dials.len())
    }

    pub fn zone_count(&self) -> usize {
        self.strip.as_ref().map_or(0, |strip| strip.zones.len())
    }
}

fn build(model: &DeviceModel, metrics: &Metrics) -> PreviewLayout {
    let cols = i32::from(model.key_cols);
    let rows = i32::from(model.key_rows);
    let zones = i32::from(model.zones());
    let dials = i32::from(model.dials);
    let touch = model.has_touch();

    let keys_w = if cols > 0 {
        (cols - 1) * metrics.pitch_x + metrics.key
    } else {
        0
    };
    // A touch strip is as wide as its sections; a plain window spans the keys.
    let strip_w = if touch && zones > 0 {
        zones * metrics.zone_w
    } else {
        keys_w
    };
    let has_strip = model.has_strip();
    let bezel_w = strip_w + 2 * BEZEL_INSET;
    let dials_w = dials * metrics.zone_w;
    let rail_w = if has_strip {
        bezel_w
    } else {
        dials_w + 2 * BEZEL_INSET
    };

    let (width, key_x0, side_x) = match metrics.fixed {
        Some(fixed) => fixed,
        None => {
            let mut width = MIN_WIDTH.max(keys_w + 96);
            if has_strip {
                width = width.max(bezel_w + 54);
            }
            if dials > 0 {
                width = width.max(rail_w + 54);
            }
            // Keep every margin whole so the layout is symmetric.
            let parity = keys_w.max(bezel_w).max(rail_w) % 2;
            if width % 2 != parity {
                width += 1;
            }
            (
                width,
                (width - keys_w) / 2,
                (width - rail_w.max(bezel_w)) / 2,
            )
        }
    };

    let keys: Vec<Rect> = (0..rows * cols)
        .map(|index| {
            Rect::new(
                key_x0 + (index % cols) * metrics.pitch_x,
                KEYS_Y + (index / cols) * metrics.pitch_y,
                metrics.key,
                metrics.key,
            )
        })
        .collect();
    let mut bottom = match keys.last() {
        Some(last) => last.bottom(),
        None => BRAND_Y + BRAND_H,
    };

    let strip = has_strip.then(|| {
        let bezel = Rect::new(side_x, bottom + KEY_TO_BEZEL, bezel_w, BEZEL_H);
        bottom = bezel.bottom();
        let rect = Rect::new(bezel.x + BEZEL_INSET, bezel.y + STRIP_Y, strip_w, STRIP_H);
        StripLayout {
            bezel,
            header: Rect::new(rect.x, bezel.y + HEADER_Y, strip_w, HEADER_H),
            rect,
            touch,
            zones: (0..zones)
                .map(|zone| {
                    Rect::new(
                        rect.x + zone * metrics.zone_w,
                        rect.y,
                        metrics.zone_w,
                        STRIP_H,
                    )
                })
                .collect(),
        }
    });

    let dial_rail = (dials > 0).then(|| {
        let gap = if has_strip {
            BEZEL_TO_RAIL
        } else {
            KEY_TO_BEZEL
        };
        let rail = Rect::new(side_x, bottom + gap, rail_w, RAIL_H);
        bottom = rail.bottom();
        // Centres: under each touch section when the counts match, else spread
        // evenly across the rail.
        let aligned = strip
            .as_ref()
            .filter(|strip| strip.touch && strip.zones.len() as i32 == dials);
        let centre = |dial: i32| match (&metrics.dial_pitch, aligned) {
            (Some(pitch), _) => rail.x + metrics.dial_x0 + dial * pitch + DIAL_PX / 2,
            (None, Some(strip)) => {
                let zone = strip.zones[dial as usize];
                zone.x + zone.w / 2
            }
            (None, None) => rail.x + (2 * dial + 1) * rail.w / (2 * dials),
        };
        let dial_rects: Vec<Rect> = (0..dials)
            .map(|dial| {
                Rect::new(
                    centre(dial) - DIAL_PX / 2,
                    rail.y + DIAL_Y,
                    DIAL_PX,
                    DIAL_PX,
                )
            })
            .collect();
        DialLayout {
            rail,
            icons: dial_rects
                .iter()
                .map(|dial| {
                    Rect::new(
                        dial.x + (DIAL_PX - DIAL_ICON_PX) / 2,
                        dial.y + (DIAL_PX - DIAL_ICON_PX) / 2,
                        DIAL_ICON_PX,
                        DIAL_ICON_PX,
                    )
                })
                .collect(),
            legends: dial_rects
                .iter()
                .map(|dial| {
                    Rect::new(
                        dial.x + DIAL_PX / 2 - metrics.legend_w / 2,
                        rail.y + LEGEND_Y,
                        metrics.legend_w,
                        LEGEND_H,
                    )
                })
                .collect(),
            dials: dial_rects,
        }
    });

    let height = bottom
        + if has_strip || dials > 0 {
            metrics.bottom_margin
        } else {
            KEYS_ONLY_BOTTOM_MARGIN
        };
    PreviewLayout {
        model_id: model.id,
        size: (width, height),
        scale: PREVIEW_SCALE,
        brand: model.name.to_uppercase(),
        brand_rect: Rect::new(0, BRAND_Y, width, BRAND_H),
        keys,
        strip,
        dial_rail,
    }
}
