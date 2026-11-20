# Adding a Stream Deck model

Developer guide to the device registry. Code: `src/device_model.rs`
(table), `src/protocol.rs` (family dispatch), `src/image_output.rs`
(images), `src/device_roots.rs` (data folders). The user-facing description
of each model (controls, sizes, product ids, preview, validation status) is
the "Supported devices" section of `docs/docs.html`.

## How the registry works

`DeviceModel` is one static description of a product line: ids, product ids
(`pids`), protocol `family`, `support`, key grid, key image size/rotation/format,
dials, appended touch sensors (`touch_buttons`), the optional display under
the keys (`strip: Option<StripSpec>`), the optional picture behind the keys
(`keypad_background: Option<KeypadBackground>`), report sizes and the
`doc_url` the facts come from. `DeviceModel::all()` lists every entry;
`DeviceModel::supported()` lists the ones the app drives.

Generic code never reads raw fields where an accessor exists: use `keys()`,
`key_inputs()`, `has_strip()`, `has_touch()`, `has_info_window()`,
`has_keypad_background()`, `zones()`, `strip_px()`, `zone_px()`,
`strip_rotation()`, `touch_button_numbers()`, `is_supported()`, `pid()`. A
model without a strip, dials or touch simply reports zero of them, and stores,
the editor, artwork targets and image output follow.

Dials are independent of touch sections. A strip with `touch: false` (the Neo
information window) has a size but no sections.

## Support status

- `Supported`: detected by discovery, driven and offered in the "Current" selector
  while connected. Today every entry: `plus`, `plus-xl`, `classic`, `xl`, `neo`
  and `mini`, in that order (pinned by tests, see "Steps to add a model").
- `Planned`: described from Elgato's documentation only. Discovery never matches its
  product ids (`by_pid` / `by_pid_supported` skip it; `by_pid_any` finds it for
  information only). Nothing is created on disk for it unless a store is opened.
  No entry is Planned today; a new model starts here.

## Several devices at once

The HID hub drives every connected supported device, one worker per device
(`DeviceKey { path, serial, model }`), and the window keeps one complete session
per **model**. Consequences for a new model:

- Profiles, artwork, brightness and sleep belong to the model, so two units of
  one model mirror each other; nothing is per unit.
- Artwork and controls are addressed to a model and fanned out to every device of
  it; a device only ever receives its own model's pictures.
- The header "Current" selector lists the connected supported models and appears
  only with two or more. It needs no per-model code: entries come from
  `DeviceModel::supported()` filtered by what is connected, in registry order.
- Only the Stream Deck + has been tested on real hardware, and only on its own.
  The other five models and running several devices at once are validated with
  simulated devices (`event_api::test_support`, `src/tests/mixed_model_tests.rs`).

To try it without hardware, run the manual launcher, which plugs simulated devices
into the real hub and opens the real window over scratch directories:

```bash
SDECK_SIMULATE_MODEL=plus,plus-xl GDK_BACKEND=x11 cargo test --offline --bin sdeck-plus \
  tests::simulated_device_tests::open_the_window_with_simulated_devices \
  -- --ignored --nocapture --test-threads=1
```

`SDECK_SIMULATE_MODEL` is a comma list of supported model ids (`plus`,
`plus-xl`, `classic`, `xl`, `mini`, `neo`; default `plus-xl`).

## Protocol families

`ProtocolFamily` selects a handler through `protocol::protocol_for`, the single
dispatch point. A handler implements `FamilyProtocol`: `decode` (input report to
`Event`s), `encode_reports` (encoded image to output reports) and the feature
reports below.

- `Expanded`: Classic, XL, Neo, +, + XL. Input `0x01 <command> <len LE16>`
  (0x00 keys, 0x03 encoders, 0x02 touch); key image `0x02 0x07` with an 8-byte
  header; window `0x02 0x0B` (whole window: the Neo's info window) / `0x02 0x0C`
  (section, 16-byte header: the touch strips); full screen `0x02 0x08` (the
  Classic's keypad background: 8-byte header, 1016 picture bytes per 1024-byte
  report); feature reports `0x03`. Sensors a model appends after its keys
  arrive as extra `Event::Key` indexes (`keys()..key_inputs()`) and map to
  `Trigger::TouchButton`.
- `LegacyMini` (`src/protocol/legacy_mini.rs`): fixed 65-byte input, report id
  `0x01` at byte 0 and then one state byte per key (`input_header_len` 1, no
  command byte). Key pictures are 80x80 24-bit BGR bottom-up BMPs
  (`src/image_output/bmp.rs`, 19254 bytes) sent in 1024-byte `0x02 0x01`
  reports with a 16-byte header `02 01 <part> 00 <last> <key index>` plus ten
  zeros. The key index is `key - 1 + MINI_KEY_INDEX_BASE` (1, see below).
  `image_output::encode_image` picks JPEG or BMP from the model.

### Feature reports per family

Every feature report is 32 bytes (`FEATURE_REPORT_LEN`), zero padded.

| Report | Expanded | Legacy Mini |
|--------|----------|-------------|
| `brightness_report(v)` (0-100) | `03 08 <v>` | `05 55 AA D1 01 <v>` |
| `blank_report()` | brightness 0 | brightness 0 |
| `show_logo_report()` | `03 02` | `0B 63 00` |
| `sleep_timer_report(s)` | `03 0D <INT32 LE>` | `0B A2 <INT32 LE>` |

The sleep command only sets an idle timer, so Sleep and shutdown blank with
brightness 0; `sleep_timer_report` is not used at runtime, and
`show_logo_report` is not wired (the app blanks on exit). `Controls` carries
its model and `blank_device` goes through `protocol_for(model.family)`; the
+ and + XL send the same bytes as before the split (golden tests).

## Model-specific controls and stores

- **Touch buttons** (Neo, `touch_buttons: 2`): `Trigger::TouchButton(n)`,
  stored in layers.conf as `tb:<n>` (one-based), validated per model. Press
  only: a touch runs once when it starts (`TouchButtonPresses`); no hold, live
  information, multi action or picture. Editor `ControlKind::TouchButton`,
  preview `ControlRef::TouchButton`.
- **Info window** (Neo, `has_info_window()`): per keypad page. The picture is
  the existing artwork target `touch-strip-background` under the keypad page;
  the live information is the trigger `window`, stored as
  `binding <page> window none` plus `info <page> window <source>` (any action
  other than `none` is refused; only on window models). No button text on the
  window: text rows are keys only.
- **Keypad background** (Classic, `has_keypad_background()`): per keypad page,
  the target `touch-strip-background` stored in `keypad-background.conf`
  beside artwork.conf (version line `streamdeck-keypad-background-v1`, then
  `<hex page>\ttouch-strip-background\t<asset>`), never in artwork.conf, whose
  older readers refuse that row for a Classic ("unknown artwork target"). The
  file exists only while a page has a background and travels in bundles.
- **Unknown control kinds:** a layers.conf row whose control kind this build
  does not know (name-shaped) is kept verbatim, logged once as `kept=`, never
  run, written back and dropped only with its layer; a touchscreen.json with an
  unknown control kind stays untouched with saving disabled. Released 1.0.0
  removes unknown kinds, but never meets `tb` or `window` rows: it does not
  drive a Neo, refuses another model's root and refuses a Neo bundle.

## Steps to add a model

1. Add a `static` entry to `src/device_model.rs` with the facts from the model's
   HID page and its `doc_url`, `support: Support::Planned`, and list it in `ALL` (update the array length, `&[&DeviceModel; N]`).
   Planned Expanded entries can use `..EXPANDED`.
2. If it speaks a new family, add a `ProtocolFamily` variant, implement
   `FamilyProtocol` for it (decode, encode_reports and the four feature
   reports) and return it from `protocol_for`; set `is_implemented`. Image
   formats other than JPEG and BMP also need an encoder behind
   `image_output::encode_image`.
3. Add the model to `DOCUMENTED` in `tests/device_registry.rs` (the facts from
   the page, written independently of the table).
4. Verify with a simulator: `src/event_api/sim_registry.rs` already runs every
   entry through the simulated device (key/window upload size,
   rotation and chunk headers, input decoding, shutdown blanking). Add
   model-specific checks for anything new.
5. Verify on hardware if available. Then change `support` to `Supported`.
   Nothing below follows automatically; update each by hand:
   - the pinned tests `every_registry_model_is_supported`
     (`tests/device_registry.rs`), `models_are_found_by_pid_and_id`
     (`tests/device_model.rs`) and `only_the_supported_product_ids_are_ever_discovered`
     (`src/event_api/sim_registry.rs`);
   - the `DOCUMENTED` array length in `tests/device_registry.rs`;
   - one udev rule line per product id in `udev/70-sdeck.rules`
     (`tests/packaging.rs` fails when one is missing);
   - `install.sh`: the hidraw uevent regex and the printed rule;
   - the desktop file comment;
   - the model tables in the README, `docs/index.html` (Supported devices) and
     `docs/docs.html` (Models at a glance), a section of its own in
     `docs/docs.html` (`#device-<id>`: controls, sizes, product ids, preview,
     validation status and known limits) linked from the landing page's device
     cards, plus `CHANGELOG.md`;
   - the per-model preview image `docs/screenshots/preview-<id>.png`: the
     screenshot generator iterates `DeviceModel::supported()`, so regenerate it
     and reference it with its width and height.

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
| plus | Stream Deck + | Supported, hardware-tested | Expanded | 0x0084 | 4x2 | 120 JPEG, no rotation | 800x100 touch strip, 4 sections; 4 dials | 0 |
| plus-xl | Stream Deck + XL | Supported, simulator only | Expanded | 0x00C6 | 9x4 | 112 JPEG, 90 CCW | 1200x100 touch strip, 6 sections; 6 dials | 0 |
| classic | Stream Deck Classic | Supported, simulator only | Expanded | 0x006D 0x0080 0x00A5 0x00B9 | 5x3 | 72 JPEG, 180 | keypad background 480x272 JPEG, 180, `02 08` | 0 |
| xl | Stream Deck XL | Supported, simulator only | Expanded | 0x006C 0x008F 0x00BA | 8x4 | 96 JPEG, 180 | none | 0 |
| neo | Stream Deck Neo | Supported, simulator only | Expanded | 0x009A | 4x2 | 96 JPEG, 180 | 248x58 info window, no touch, JPEG 180 (`02 0B`) | 2, after the keys |
| mini | Stream Deck Mini | Supported, simulator only | Legacy Mini | 0x0063 0x0090 0x00B3 0x00B8 | 3x2 | 80 BMP, transposed (`Ccw90` + `key_flip_y`) | none | 0 |

Sources (the `plus` and `plus-xl` pages follow the site's naming pattern and
were not opened):

- https://docs.elgato.com/streamdeck/hid/intro
- https://docs.elgato.com/streamdeck/hid/stream-deck-classic
- https://docs.elgato.com/streamdeck/hid/stream-deck-xl
- https://docs.elgato.com/streamdeck/hid/stream-deck-neo
- https://docs.elgato.com/streamdeck/hid/mini
- https://docs.elgato.com/streamdeck/hid/stream-deck-plus
- https://docs.elgato.com/streamdeck/hid/stream-deck-plus-xl

The Mini's key index base and orientation also follow the open-source drivers
python-elgato-streamdeck (`StreamDeckMini.set_key_image`) and
node-elgato-stream-deck (`StreamdeckGen1ImageHeaderGenerator`).

## Unverified points

- `partial_window_axis_swap`: Elgato says logical coordinates, a third-party
  PR disagrees; `false` until hardware shows otherwise.
- Mini: `MINI_KEY_INDEX_BASE = 1` and the transposed picture (`Ccw90` with
  `key_flip_y`) follow the open-source drivers; Elgato's page only shows a
  0-5 layout figure and says "rotated 90 degrees clockwise". If a real Mini
  shows every picture one key off, set the base to 0; if mirrored, use `Cw90`
  without a flip. Also unverified: the Show Image flag on the last part only,
  BMP 24-bit at 2835 ppm, part numbering from 0, 32-byte feature reports (the
  python driver sends 17), the 0-100 brightness range, and whether the Mini
  keeps up with scrolling titles (about 190 KB/s per key as BMP).
- Neo: the window and keys are JPEG turned 180 degrees as Elgato's Neo page
  says; the window goes whole with `0x0B`. Whether the sensors carry images is
  unknown; they are modelled as buttons without pictures.
- Classic: Update Background (`0x02 0x0D`) only stores a picture and the page
  documents no way to show it on the Classic (Show Background by Index,
  `0x03 0x13`, is on the XL page only), so the background is sent with Update
  Full Screen Image `0x02 0x08`. Unconfirmed: how `02 08` and later key images
  combine, the orientation of the layout figure, firmware support on every
  revision, timing.
- `input_report_len` (64) and the header sizes for Classic, XL and Neo are
  copied from the Expanded family, not read from their pages.
- Pedal, Studio, Mobile and the original 0x0060 have no HID documentation, so
  they have no entry.

Today the Stream Deck + is the only hardware-tested model, and only on its own;
the + XL, Classic, XL, Mini and Neo and several devices at once are
implemented from Elgato's HID documentation and validated with a simulator.

Never claim hardware testing without hardware. A simulator proves the wire
format against our reading of the documentation, nothing more; docs must say
"implemented from Elgato's HID documentation and validated with a simulator"
until a real unit confirms it.

## Known gaps

- Classic: shutdown does not clear the background (brightness 0 hides it); the
  target-picker mini map draws nothing for Background; a page deleted by an
  older build leaves its background row, and a new page with the same name
  inherits it; an older build refuses to import a Classic bundle that carries a
  background (one without imports unchanged).
- Neo: no button text on the info window.

## Pitfalls

- `by_id`, stored preferences and bundles can name Planned models. Consumers
  that need a connectable model must use `by_pid_supported` / `by_id_supported`.
- Neo sensors arrive as `Event::Key` indexes beyond `keys()`; the layers level
  maps them to `Trigger::TouchButton`, and only models with touch buttons
  accept `tb` rows.
- The Mini's pictures are BMP, not JPEG: go through `encode_image`, never a
  JPEG-only encoder.
- Never call `blank_device` for a family without a handler; the monitor guards
  this with `ensure_protocol`.
