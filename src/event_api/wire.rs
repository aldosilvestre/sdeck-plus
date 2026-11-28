//! Expanded-family wire layouts seen from the device side, shared by the
//! simulator and the virtual deck: the input reports a device sends, and the
//! header of each image chunk the host writes.

use crate::device_model::DeviceModel;

/// An input report: id 1, command, little-endian payload length, payload,
/// zero padded to the model's input report size.
pub(super) fn input_report(model: &DeviceModel, command: u8, payload: &[u8]) -> Vec<u8> {
    let mut report = vec![1, command];
    report.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    report.extend_from_slice(payload);
    report.resize(report.len().max(model.input_report_len), 0);
    report
}

/// The header of one Expanded image report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Chunk {
    /// 0x07 key image, 0x08 full screen (a Classic's keypad background),
    /// 0x0b full window, 0x0c partial window.
    pub(super) command: u8,
    /// Zero-based key of a key image, otherwise the reserved byte.
    pub(super) slot: u8,
    /// `(x, y, width, height)` of a 0x0c partial window.
    pub(super) window: Option<(u16, u16, u16, u16)>,
    /// The final chunk of the picture.
    pub(super) last: bool,
    pub(super) index: u16,
    /// Bytes of picture data after the header (not checked against the report).
    pub(super) length: usize,
    /// Bytes before the picture data.
    pub(super) header: usize,
}

/// The chunk header of an Expanded `02 07`, `02 08`, `02 0B` or `02 0C`
/// report; `None` for any other report, or one shorter than its header. Update
/// Full Screen Image (`02 08`) has the key image's layout with its key index
/// byte reserved.
pub(super) fn expanded_chunk(model: &DeviceModel, report: &[u8]) -> Option<Chunk> {
    if report.first() != Some(&0x02) {
        return None;
    }
    let word = |at: usize| u16::from_le_bytes([report[at], report[at + 1]]);
    match *report.get(1)? {
        command @ (0x07 | 0x08 | 0x0b) => {
            let header = model.key_header_len;
            (report.len() >= header.max(8)).then(|| Chunk {
                command,
                slot: report[2],
                window: None,
                last: report[3] == 1,
                index: word(6),
                length: usize::from(word(4)),
                header,
            })
        }
        0x0c => {
            let header = model.zone_header_len;
            (report.len() >= header.max(15)).then(|| Chunk {
                command: 0x0c,
                slot: 0,
                window: Some((word(2), word(4), word(6), word(8))),
                last: report[10] == 1,
                index: word(11),
                length: usize::from(word(13)),
                header,
            })
        }
        _ => None,
    }
}
