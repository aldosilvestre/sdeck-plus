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

pub fn decode_report(report: &[u8]) -> Vec<Event> {
    if report.len() < 4 || report[0] != 0x01 {
        return Vec::new();
    }
    let length = u16::from_le_bytes([report[2], report[3]]) as usize;
    if report.len() < 4 + length {
        return Vec::new();
    }
    let payload = &report[4..4 + length];
    match report[1] {
        0x00 => payload
            .iter()
            .take(8)
            .enumerate()
            .filter_map(|(index, &state)| {
                (state <= 1).then_some(Event::Key {
                    index,
                    pressed: state == 1,
                })
            })
            .collect(),
        0x03 if payload.len() >= 5 => match payload[0] {
            0x00 => payload[1..5]
                .iter()
                .enumerate()
                .filter_map(|(index, &state)| {
                    (state <= 1).then_some(Event::EncoderButton {
                        index,
                        pressed: state == 1,
                    })
                })
                .collect(),
            0x01 => payload[1..5]
                .iter()
                .enumerate()
                .filter_map(|(index, &ticks)| {
                    let ticks = ticks as i8;
                    (ticks != 0).then_some(Event::Rotation { index, ticks })
                })
                .collect(),
            _ => Vec::new(),
        },
        0x02 if payload.len() >= 2 => match payload[0] {
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
