# Adding a Stream Deck model

Developer guide to the device registry. Code: `src/device_model.rs`
(table), `src/protocol.rs` (family dispatch), `src/image_output.rs`
(images), `src/device_roots.rs` (data folders).

## How the registry works

`DeviceModel` is one static description of a product line: ids, product ids
(`pids`), protocol `family`, `support`, key grid, key image size/rotation/format,
dials, appended touch sensors (`touch_buttons`), the optional display under
the keys (`strip: Option<StripSpec>`), report sizes and the `doc_url` the facts
come from. `DeviceModel::all()` lists every entry; `DeviceModel::supported()`
lists the ones the app drives.

Generic code never reads raw fields where an accessor exists: use `keys()`,
`key_inputs()`, `has_strip()`, `has_touch()`, `zones()`, `strip_px()`,
`zone_px()`, `strip_rotation()`, `is_supported()`, `pid()`. A model without a
strip, dials or touch simply reports zero of them, and stores, the editor,
artwork targets and image output follow.

Dials are independent of touch sections. A strip with `touch: false` (the Neo
information window) has a size but no sections.

## Support status

- `Supported`: detected by discovery, driven, offered in "Preview as". Today
  exactly `plus` and `plus-xl` (pinned by a test).
- `Planned`: described from Elgato's documentation. Discovery never matches its
  product ids (`by_pid` / `by_pid_supported` skip it; `by_pid_any` finds it for
  information only). Nothing is created on disk for it unless a store is opened.

## Protocol families

`ProtocolFamily` selects a handler through `protocol::protocol_for`, the single
dispatch point. A handler implements `FamilyProtocol`: `decode` (input report to
`Event`s) and `encode_reports` (encoded image to output reports).

- `Expanded`: Classic, XL, Neo, +, + XL. Input `0x01 <command> <len LE16>`
  (0x00 keys, 0x03 encoders, 0x02 touch); key image `0x02 0x07` with an 8-byte
  header; window `0x0B` (whole) / `0x0C` (section, 16-byte header); feature
  reports `0x03`. Sensors a model appends after its keys arrive as extra
  `Event::Key` indexes (`keys()..key_inputs()`).
- `LegacyMini`: fixed 65-byte input without a command byte, 1024-byte output
  with a 16-byte header, BMP keys. Documented in the table, **not
  implemented**: its handler is `Unimplemented`, every call returns an error
  naming the model, and the monitor refuses to drive it
  (`DeviceModel::ensure_protocol`).

## Steps to add a model

1. Add a `static` entry to `src/device_model.rs` with the facts from the model's
   HID page and its `doc_url`, `support: Support::Planned`, and list it in `ALL` (update the array length, `&[&DeviceModel; N]`).
   Planned Expanded entries can use `..EXPANDED`.
2. If it speaks a new family, add a `ProtocolFamily` variant, implement
   `FamilyProtocol` for it and return it from `protocol_for`; set
   `is_implemented`. Image formats other than JPEG also need an encoder in
   `image_output::encode_jpeg`.
3. Add the model to `DOCUMENTED` in `tests/device_registry.rs` (the facts from
   the page, written independently of the table).
4. Verify with a simulator: `src/event_api/sim_registry.rs` already runs every
   Expanded entry through the simulated device (key/window upload size,
   rotation and chunk headers, input decoding, shutdown blanking). Add
   model-specific checks for anything new.
5. Verify on hardware if available. Then change `support` to `Supported`.
   Nothing below follows automatically; update each by hand:
   - the three pinned tests `exactly_the_plus_and_the_plus_xl_are_supported_today`
     (`tests/device_registry.rs`), `models_are_found_by_pid_and_id`
     (`tests/device_model.rs`) and `only_the_two_supported_product_ids_are_ever_discovered`;
   - the `DOCUMENTED` array length in `tests/device_registry.rs`;
   - one udev rule line per product id in `udev/70-sdeck.rules`
     (`tests/packaging.rs` fails when one is missing);
   - `install.sh`: the hidraw uevent regex and the printed rule;
   - the desktop file comment;
   - the model tables in the README, `docs/index.html` (Supported devices) and
     `docs/docs.html` (Devices), plus `CHANGELOG.md`;
   - the per-model preview image `docs/screenshots/preview-<id>.png`: the
     screenshot generator iterates `DeviceModel::supported()`, so regenerate it.

Invariant tests in `tests/device_registry.rs` and `src/event_api/sim_registry.rs`
run over `DeviceModel::all()`, so a new row is covered automatically: unique and
path-safe ids, unique product ids, `keys() == cols * rows`, numbering ranges,
`zones * zone width == strip width`, positive sizes, rotation correctness,
store/artwork/bundle round trips at the extreme controls and one past them,
discovery ignoring Planned ids, `doc_url` under
`https://docs.elgato.com/streamdeck/hid/`.

## Data root rule

`data_root(model)` is `<data>/sdeck-plus/devices/<id>/` for every model except
the Stream Deck +, which keeps `<data>/sdeck-plus/` (legacy root). The root of a
Planned model is never created unless a store is opened. Opening a model's root
with another model is refused; paths are resolved (`..`, symlinks) before the
comparison.

## Documented models

| Id | Name | Status | Family | PIDs | Keys | Key image | Display | Sensors |
|----|------|--------|--------|------|------|-----------|---------|---------|
| plus | Stream Deck + | Supported | Expanded | 0x0084 | 4x2 | 120 JPEG, no rotation | 800x100 touch strip, 4 sections; 4 dials | 0 |
| plus-xl | Stream Deck + XL | Supported | Expanded | 0x00C6 | 9x4 | 112 JPEG, 90 CCW | 1200x100 touch strip, 6 sections; 6 dials | 0 |
| classic | Stream Deck Classic | Planned | Expanded | 0x006D 0x0080 0x00A5 0x00B9 | 5x3 | 72 JPEG, 180 | none (the 480x272 LCD takes background images, 0x0D; not modelled) | 0 |
| xl | Stream Deck XL | Planned | Expanded | 0x006C 0x008F 0x00BA | 8x4 | 96 JPEG, 180 | none | 0 |
| neo | Stream Deck Neo | Planned | Expanded | 0x009A | 4x2 | 96 JPEG, 180 | 248x58 window, no touch (0x0B/0x0C) | 2, after the keys |
| mini | Stream Deck Mini | Planned | Legacy Mini | 0x0063 0x0090 0x00B3 0x00B8 | 3x2 | 80 BMP, 90 CW | none | 0 |

Sources (the `plus` and `plus-xl` pages follow the site's naming pattern and
were not opened):

- https://docs.elgato.com/streamdeck/hid/intro
- https://docs.elgato.com/streamdeck/hid/stream-deck-classic
- https://docs.elgato.com/streamdeck/hid/stream-deck-xl
- https://docs.elgato.com/streamdeck/hid/stream-deck-neo
- https://docs.elgato.com/streamdeck/hid/mini
- https://docs.elgato.com/streamdeck/hid/stream-deck-plus
- https://docs.elgato.com/streamdeck/hid/stream-deck-plus-xl

## Unverified points

- `key_flip_y` has no Elgato source; it is `false` everywhere.
- `partial_window_axis_swap`: Elgato says logical coordinates, a third-party
  PR disagrees; `false` until hardware shows otherwise.
- The Neo window rotation (`None`) and whether its upload really shares the
  strip path are assumptions. The Classic LCD background command (0x02 0x0D) is
  not implemented.
- Neo sensors: whether they carry images is unknown; they are modelled as extra
  buttons only.
- `input_report_len` (64) and the header sizes for Classic, XL and Neo are
  copied from the Expanded family, not read from their pages.
- Pedal, Studio, Mobile and the original 0x0060 have no HID documentation, so
  they have no entry.

Today the Stream Deck + is the only hardware-tested model; the + XL is
implemented from Elgato's HID documentation and validated with a simulator.

Never claim hardware testing without hardware. A simulator proves the wire
format against our reading of the documentation, nothing more; docs must say
"implemented from Elgato's HID documentation and validated with a simulator"
until a real unit confirms it.

## Pitfalls

- `by_id`, stored preferences and bundles can name Planned models. Consumers
  that need a connectable model must use `by_pid_supported` / `by_id_supported`.
- Neo sensors arrive as `Event::Key` indexes beyond `keys()`; store validation
  rejects them, so they cannot be bound until that is decided.
- Mini needs its own `FamilyProtocol` before it can be promoted.
- Never call `blank_device` for a family without a handler; the monitor guards
  this with `ensure_protocol`.
