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

/// Size of every feature report; the host pads it with zeros (Elgato's general
/// reference and Mini page both fix it to 32 bytes).
pub const FEATURE_REPORT_LEN: usize = 32;

/// One feature report, as sent with HID SEND FEATURE REPORT.
pub type FeatureReport = [u8; FEATURE_REPORT_LEN];

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

    /// The feature report that sets the display backlight to `percent`,
    /// clamped to the documented 0..=100.
    fn brightness_report(&self, percent: u8) -> FeatureReport;

    /// The feature report that turns every display dark at once: brightness 0.
    /// The family's sleep command only sets an idle timer
    /// ([`Self::sleep_timer_report`]), so it cannot blank a device.
    fn blank_report(&self) -> FeatureReport {
        self.brightness_report(0)
    }

    /// The feature report that forcibly shows the boot logo.
    fn show_logo_report(&self) -> FeatureReport;

    /// The feature report that sets the idle time, in seconds, before the
    /// device enters its own sleep mode; 0 disables it. The wire value is a
    /// little-endian INT32, so longer times saturate at `i32::MAX`.
    fn sleep_timer_report(&self, seconds: u32) -> FeatureReport;
}

/// The handler of `family`.
pub fn protocol_for(family: ProtocolFamily) -> &'static dyn FamilyProtocol {
    match family {
        ProtocolFamily::Expanded => &Expanded,
        ProtocolFamily::LegacyMini => &LegacyMini,
    }
}

/// `head` zero padded to [`FEATURE_REPORT_LEN`].
fn feature_report(head: &[u8]) -> FeatureReport {
    let mut report = [0; FEATURE_REPORT_LEN];
    report[..head.len()].copy_from_slice(head);
    report
}

/// A Set Sleep Mode Duration report: `id`, `command`, then the seconds.
fn sleep_duration_report([id, command]: [u8; 2], seconds: u32) -> FeatureReport {
    let [a, b, c, d] = i32::try_from(seconds).unwrap_or(i32::MAX).to_le_bytes();
    feature_report(&[id, command, a, b, c, d])
}

/// Classic, XL, Neo, + and + XL.
struct Expanded;

/// The original Mini. Its feature reports are implemented; its input and image
/// protocol are not yet, so decoding and image output fail with a message and
/// nothing is sent (discovery refuses the family until then).
struct LegacyMini;

impl FamilyProtocol for LegacyMini {
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

    // https://docs.elgato.com/streamdeck/hid/mini (Setter Feature Reports).
    fn brightness_report(&self, percent: u8) -> FeatureReport {
        feature_report(&[0x05, 0x55, 0xAA, 0xD1, 0x01, percent.min(100)])
    }

    fn show_logo_report(&self) -> FeatureReport {
        feature_report(&[0x0B, 0x63, 0x00])
    }

    fn sleep_timer_report(&self, seconds: u32) -> FeatureReport {
        sleep_duration_report([0x0B, 0xA2], seconds)
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

    // https://docs.elgato.com/streamdeck/hid/general (Setter Feature Reports).
    fn brightness_report(&self, percent: u8) -> FeatureReport {
        feature_report(&[0x03, 0x08, percent.min(100)])
    }

    fn show_logo_report(&self) -> FeatureReport {
        feature_report(&[0x03, 0x02])
    }

    fn sleep_timer_report(&self, seconds: u32) -> FeatureReport {
        sleep_duration_report([0x03, 0x0D], seconds)
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

    /// `head` zero padded to the 32 bytes of a feature report.
    fn padded(head: &[u8]) -> [u8; 32] {
        let mut report = [0; 32];
        report[..head.len()].copy_from_slice(head);
        report
    }

    #[test]
    fn plus_and_plus_xl_feature_reports_keep_their_golden_bytes() {
        use crate::device_model::PLUS_XL;
        for model in [&PLUS, &PLUS_XL] {
            let family = protocol_for(model.family);
            // The exact bytes sent before families: report 03, command 08.
            assert_eq!(
                family.brightness_report(73),
                [
                    0x03, 0x08, 73, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0,
                ],
                "{}",
                model.id
            );
            assert_eq!(
                family.blank_report(),
                [
                    0x03, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0,
                ]
            );
            assert_eq!(family.brightness_report(100), padded(&[0x03, 0x08, 100]));
            assert_eq!(family.brightness_report(0), family.blank_report());
            // Elgato's general reference: Show Logo 03 02, Set Sleep Mode
            // Duration 03 0D with a little-endian INT32 of seconds.
            assert_eq!(family.show_logo_report(), padded(&[0x03, 0x02]));
            assert_eq!(
                family.sleep_timer_report(0),
                padded(&[0x03, 0x0D, 0, 0, 0, 0])
            );
            assert_eq!(
                family.sleep_timer_report(300),
                padded(&[0x03, 0x0D, 0x2C, 0x01, 0, 0])
            );
        }
    }

    #[test]
    fn legacy_mini_feature_reports_follow_elgatos_mini_page() {
        let family = protocol_for(crate::device_model::MINI.family);
        assert_eq!(
            family.brightness_report(73),
            [
                0x05, 0x55, 0xAA, 0xD1, 0x01, 73, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]
        );
        assert_eq!(
            family.brightness_report(100),
            padded(&[0x05, 0x55, 0xAA, 0xD1, 0x01, 100])
        );
        // Blanking is brightness 0: 0B A2 only sets the idle timer.
        assert_eq!(
            family.blank_report(),
            padded(&[0x05, 0x55, 0xAA, 0xD1, 0x01, 0])
        );
        assert_eq!(family.show_logo_report(), padded(&[0x0B, 0x63, 0x00]));
        assert_eq!(
            family.sleep_timer_report(0),
            padded(&[0x0B, 0xA2, 0, 0, 0, 0])
        );
        assert_eq!(
            family.sleep_timer_report(300),
            padded(&[0x0B, 0xA2, 0x2C, 0x01, 0, 0])
        );
    }

    #[test]
    fn feature_reports_stay_in_the_documented_ranges() {
        assert_eq!(FEATURE_REPORT_LEN, 32);
        let expanded = protocol_for(ProtocolFamily::Expanded);
        let mini = protocol_for(ProtocolFamily::LegacyMini);
        for percent in [101, u8::MAX] {
            assert_eq!(expanded.brightness_report(percent)[2], 100);
            assert_eq!(mini.brightness_report(percent)[5], 100);
        }
        // The duration is an INT32: longer times saturate at i32::MAX.
        for seconds in [i32::MAX as u32, i32::MAX as u32 + 1, u32::MAX] {
            assert_eq!(
                expanded.sleep_timer_report(seconds),
                padded(&[0x03, 0x0D, 0xFF, 0xFF, 0xFF, 0x7F])
            );
            assert_eq!(
                mini.sleep_timer_report(seconds),
                padded(&[0x0B, 0xA2, 0xFF, 0xFF, 0xFF, 0x7F])
            );
        }
    }

    #[test]
    fn rejects_truncated_touch_reports() {
        assert!(decode_report(&report(2, &[1, 0, 1, 0, 2])).is_empty());
        assert!(decode_report(&report(2, &[3, 0, 1, 0, 2, 0, 3, 0, 4])).is_empty());
        assert!(decode_report(&report(2, &[4; 14])).is_empty());
    }
}
