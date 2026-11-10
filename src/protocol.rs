use crate::{
    device_model::{DeviceModel, ProtocolFamily, PLUS},
    image_output::{self, OutputReport},
    local_artwork::ArtworkTarget,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Key {
        index: usize,
        pressed: bool,
    },
    EncoderButton {
        index: usize,
        pressed: bool,
    },
    Rotation {
        index: usize,
        ticks: i8,
    },
    TouchTap {
        x: u16,
        y: u16,
    },
    TouchPress {
        x: u16,
        y: u16,
    },
    TouchFlick {
        start_x: u16,
        start_y: u16,
        end_x: u16,
        end_y: u16,
    },
}

/// What one protocol family implements. This is the single dispatch point for
/// a new family: add a [`ProtocolFamily`] variant, implement this trait for it
/// and return it from [`protocol_for`]; no other call site changes.
pub trait FamilyProtocol: Sync {
    /// Decode one input report of `model`.
    ///
    /// # Errors
    /// A message when the family cannot be decoded.
    fn decode(&self, model: &DeviceModel, report: &[u8]) -> Result<Vec<Event>, String>;

    /// The output reports that upload `jpeg` (an encoded image) to `target`.
    ///
    /// # Errors
    /// A message when the target does not exist or the family cannot be driven.
    fn encode_reports(
        &self,
        model: &DeviceModel,
        target: ArtworkTarget,
        jpeg: &[u8],
    ) -> Result<Vec<OutputReport>, String>;
}

/// The handler of `family`.
pub fn protocol_for(family: ProtocolFamily) -> &'static dyn FamilyProtocol {
    match family {
        ProtocolFamily::Expanded => &Expanded,
        ProtocolFamily::LegacyMini => &Unimplemented,
    }
}

/// Classic, XL, Neo, + and + XL.
struct Expanded;

/// A documented family with no handler yet: every call fails with a message
/// and nothing is ever sent to a device.
struct Unimplemented;

impl FamilyProtocol for Unimplemented {
    fn decode(&self, model: &DeviceModel, _report: &[u8]) -> Result<Vec<Event>, String> {
        model.ensure_protocol().map(|()| Vec::new())
    }

    fn encode_reports(
        &self,
        model: &DeviceModel,
        _target: ArtworkTarget,
        _jpeg: &[u8],
    ) -> Result<Vec<OutputReport>, String> {
        model.ensure_protocol().map(|()| Vec::new())
    }
}

impl FamilyProtocol for Expanded {
    fn decode(&self, model: &DeviceModel, report: &[u8]) -> Result<Vec<Event>, String> {
        Ok(decode_expanded(model, report))
    }

    fn encode_reports(
        &self,
        model: &DeviceModel,
        target: ArtworkTarget,
        jpeg: &[u8],
    ) -> Result<Vec<OutputReport>, String> {
        image_output::encode_expanded_reports(model, target, jpeg)
    }
}

/// Decode one Stream Deck + input report.
pub fn decode_report(report: &[u8]) -> Vec<Event> {
    decode_report_for(&PLUS, report)
}

/// Decode one input report of `model`, whose key and dial counts size the
/// payloads. A report the family cannot decode yields no events.
pub fn decode_report_for(model: &DeviceModel, report: &[u8]) -> Vec<Event> {
    try_decode_report_for(model, report).unwrap_or_default()
}

/// [`decode_report_for`] that says why a family cannot be decoded.
///
/// # Errors
/// A message when the family of `model` has no handler.
pub fn try_decode_report_for(model: &DeviceModel, report: &[u8]) -> Result<Vec<Event>, String> {
    protocol_for(model.family).decode(model, report)
}

fn decode_expanded(model: &DeviceModel, report: &[u8]) -> Vec<Event> {
    // The sensors a model appends after its keys read as extra buttons.
    let keys = usize::from(model.key_inputs());
    let dials = usize::from(model.dials);
    let header = model.input_header_len;
    if header < 4 || report.len() < header || report[0] != 0x01 {
        return Vec::new();
    }
    let length = u16::from_le_bytes([report[2], report[3]]) as usize;
    if report.len() < header + length {
        return Vec::new();
    }
    let payload = &report[header..header + length];
    match report[1] {
        0x00 => payload
            .iter()
            .take(keys)
            .enumerate()
            .filter_map(|(index, &state)| {
                (state <= 1).then_some(Event::Key {
                    index,
                    pressed: state == 1,
                })
            })
            .collect(),
        0x03 if payload.len() > dials => match payload[0] {
            0x00 => payload[1..=dials]
                .iter()
                .enumerate()
                .filter_map(|(index, &state)| {
                    (state <= 1).then_some(Event::EncoderButton {
                        index,
                        pressed: state == 1,
                    })
                })
                .collect(),
            0x01 => payload[1..=dials]
                .iter()
                .enumerate()
                .filter_map(|(index, &ticks)| {
                    let ticks = ticks as i8;
                    (ticks != 0).then_some(Event::Rotation { index, ticks })
                })
                .collect(),
            _ => Vec::new(),
        },
        0x02 if model.has_touch() && payload.len() >= 2 => match payload[0] {
            0x01 if payload.len() >= 6 => vec![Event::TouchTap {
                x: u16::from_le_bytes([payload[2], payload[3]]),
                y: u16::from_le_bytes([payload[4], payload[5]]),
            }],
            0x02 if payload.len() >= 6 => vec![Event::TouchPress {
                x: u16::from_le_bytes([payload[2], payload[3]]),
                y: u16::from_le_bytes([payload[4], payload[5]]),
            }],
            0x03 if payload.len() >= 10 => vec![Event::TouchFlick {
                start_x: u16::from_le_bytes([payload[2], payload[3]]),
                start_y: u16::from_le_bytes([payload[4], payload[5]]),
                end_x: u16::from_le_bytes([payload[6], payload[7]]),
                end_y: u16::from_le_bytes([payload[8], payload[9]]),
            }],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(command: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![1, command, payload.len() as u8, 0];
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn decodes_key_states() {
        assert_eq!(
            decode_report(&report(0, &[1, 0, 1, 0, 0, 0, 0, 0])),
            vec![
                Event::Key {
                    index: 0,
                    pressed: true
                },
                Event::Key {
                    index: 1,
                    pressed: false
                },
                Event::Key {
                    index: 2,
                    pressed: true
                },
                Event::Key {
                    index: 3,
                    pressed: false
                },
                Event::Key {
                    index: 4,
                    pressed: false
                },
                Event::Key {
                    index: 5,
                    pressed: false
                },
                Event::Key {
                    index: 6,
                    pressed: false
                },
                Event::Key {
                    index: 7,
                    pressed: false
                },
            ]
        );
    }

    #[test]
    fn decodes_encoder_buttons_and_signed_rotations() {
        assert_eq!(
            decode_report(&report(3, &[0, 1, 0, 1, 0])),
            vec![
                Event::EncoderButton {
                    index: 0,
                    pressed: true
                },
                Event::EncoderButton {
                    index: 1,
                    pressed: false
                },
                Event::EncoderButton {
                    index: 2,
                    pressed: true
                },
                Event::EncoderButton {
                    index: 3,
                    pressed: false
                },
            ]
        );
        assert_eq!(
            decode_report(&report(3, &[1, 2, 0xfe, 0, 1])),
            vec![
                Event::Rotation { index: 0, ticks: 2 },
                Event::Rotation {
                    index: 1,
                    ticks: -2
                },
                Event::Rotation { index: 3, ticks: 1 },
            ]
        );
    }

    #[test]
    fn ignores_malformed_and_unsupported_reports() {
        assert!(decode_report(&[1, 0, 8, 0, 1]).is_empty());
        assert!(decode_report(&report(3, &[2, 0, 0, 0, 0])).is_empty());
        assert!(decode_report(&report(9, &[1])).is_empty());
    }

    #[test]
    fn decodes_touch_strip_gestures_with_little_endian_coordinates() {
        assert_eq!(
            decode_report(&report(
                2,
                &[1, 1, 0x4b, 0, 0x23, 0, 0, 0, 0, 0, 0, 0, 0, 0]
            )),
            vec![Event::TouchTap { x: 75, y: 35 }]
        );
        assert_eq!(
            decode_report(&report(2, &[2, 0, 1, 0, 2, 0])),
            vec![Event::TouchPress { x: 1, y: 2 }]
        );
        assert_eq!(
            decode_report(&report(2, &[3, 0, 1, 0, 2, 0, 0x20, 3, 100, 0])),
            vec![Event::TouchFlick {
                start_x: 1,
                start_y: 2,
                end_x: 800,
                end_y: 100
            }]
        );
    }

    #[test]
    fn rejects_truncated_touch_reports() {
        assert!(decode_report(&report(2, &[1, 0, 1, 0, 2])).is_empty());
        assert!(decode_report(&report(2, &[3, 0, 1, 0, 2, 0, 3, 0, 4])).is_empty());
        assert!(decode_report(&report(2, &[4; 14])).is_empty());
    }
}
