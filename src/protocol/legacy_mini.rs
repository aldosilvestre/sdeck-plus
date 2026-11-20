//! Input and image reports of the Legacy Mini protocol, from Elgato's Mini
//! page (https://docs.elgato.com/streamdeck/hid/mini). Its feature reports
//! live with the family handler in `protocol.rs`.

use super::{Event, MINI_KEY_INDEX_BASE};
use crate::{
    device_model::DeviceModel,
    image_output::{self, OutputReport},
    local_artwork::ArtworkTarget,
};

/// Report id of the Key Press State Change input report.
const KEY_STATES: u8 = 0x01;

/// Report id 0x02 and command 0x01: Upload Data to Image Memory Bank.
const UPLOAD: [u8; 2] = [0x02, 0x01];

/// Decode one input report: report id 0x01, then one byte per key, 0x00
/// released or 0x01 pressed. Anything else yields no events.
pub(super) fn decode(model: &DeviceModel, report: &[u8]) -> Vec<Event> {
    // The header is the report id alone.
    let header = model.input_header_len;
    let keys = usize::from(model.key_inputs());
    if header == 0 || report.len() < header + keys || report[0] != KEY_STATES {
        return Vec::new();
    }
    report[header..header + keys]
        .iter()
        .enumerate()
        .filter_map(|(index, &state)| {
            (state <= 1).then_some(Event::Key {
                index,
                pressed: state == 1,
            })
        })
        .collect()
}

/// The reports that upload `image` (an encoded BMP) to a key: fixed-size
/// output reports whose 16-byte header is `02 01`, the chunk index, 0x00, the
/// Show Image flag, the key index and ten zero bytes, then the chunk, zero
/// padded. The header carries no length: every chunk but the last is full.
///
/// The Show Image flag is set on the last chunk only, so the key changes
/// once its whole picture is in the memory bank. UNVERIFIED on hardware: the
/// page says to send the key image "with Show Image flag set to 0x01" without
/// naming the chunk; the open-source drivers set it on the last one.
///
/// # Errors
/// A message for a control the model lacks (the Mini has keys only), an
/// empty image, or one needing more chunks than the one-byte index counts.
pub(super) fn encode_reports(
    model: &DeviceModel,
    target: ArtworkTarget,
    image: &[u8],
) -> Result<Vec<OutputReport>, String> {
    image_output::dimensions_for(model, target)?;
    let ArtworkTarget::Keypad(number) = target else {
        return Err("unsupported artwork target".into());
    };
    if image.is_empty() {
        return Err("image payload must not be empty".into());
    }
    let header = model.key_header_len;
    let chunks = image.chunks(model.image_report_len - header);
    let count = chunks.len();
    if count > usize::from(u8::MAX) + 1 {
        return Err("image requires too many chunks".into());
    }
    let key_index = number - 1 + MINI_KEY_INDEX_BASE;
    chunks
        .enumerate()
        .map(|(index, chunk)| {
            let mut report = vec![0; model.image_report_len];
            report[..2].copy_from_slice(&UPLOAD);
            report[2] = u8::try_from(index).map_err(|error| error.to_string())?;
            report[4] = u8::from(index + 1 == count);
            report[5] = key_index;
            report[header..header + chunk.len()].copy_from_slice(chunk);
            Ok(OutputReport(report))
        })
        .collect()
}
