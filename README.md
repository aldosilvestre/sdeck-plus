# Stream Deck + Event Monitor

A GTK4 desktop monitor for the Elgato Stream Deck + (VID `0x0FD9`, PID `0x0084`). It displays live input and can send explicitly assigned artwork to physical keys and the Touch Strip over USB HID. It does not alter profiles, key mappings, brightness, sleep, or other device configuration.

## Requirements

- Rust stable toolchain and Cargo.
- GTK4 development files and a C toolchain. On Arch Linux: `sudo pacman -S gtk4 base-devel`.
- Stream Deck + connected over USB for live device events.
- Permission to open the device's HID node. If access is denied, use the distribution's supported device-access mechanism (for example, an administrator-installed narrowly scoped udev rule for this VID/PID with `TAG+="uaccess"`), then reconnect the device. The application does not change host permissions.

## Build and run

```sh
cargo run
```

The UI displays connection state, eight keypad states, four encoder button/rotation states, recent events, and Touch Strip Tap, Press, and Flick coordinates in Elgato's 800 × 100 logical-pixel space. Touch gestures are reported after the interaction completes, not as separate contact start/release events. If the device is absent, inaccessible, or disconnected, the UI remains open and reports the state; it retries detection after connection failures.

## Local artwork

Use **Choose image…** beside any keypad, the Touch Strip background, or one of the four encoder zones to assign or replace artwork. PNG and GIF files are explicitly available in the picker; a separate filter includes all image formats supported by the installed GdkPixbuf loaders. GIF artwork is uploaded as a static still image; animation is not supported by the hardware JPEG upload format. Images are center-cropped to cover the device target and sent as correctly sized JPEGs over USB HID. The eight keys use 120 × 120 images; the Touch Strip background uses 800 × 100 and each encoder overlay uses a 200 × 100 region. The background and zone images can be layered. A failed import leaves the current assignment unchanged. **Clear** removes the local assignment; clearing a key sends a black key image, while clearing a Touch Strip image rebuilds the strip from its saved background (or black) and remaining zone images. It does not issue a separate device clear command.

Images are copied into the application's XDG data directory, so the original files may be moved or deleted after assignment. The assignment manifest is stored beside the copied assets. The default path is `$XDG_DATA_HOME/streamdeck-plus-monitor` (normally `~/.local/share/streamdeck-plus-monitor`); when `XDG_DATA_HOME` is unset, the application uses `$HOME/.local/share/streamdeck-plus-monitor`. Existing missing or invalid assignments are ignored with a diagnostic rather than preventing startup. Persisted assignments are uploaded on startup when the device connects and replayed after reconnect; targets with no assignment are left untouched. Upload failures are shown in the connection status; local assignments remain saved if a device write fails.

## In-process event API

`src/event_api.rs` defines the typed `MonitorEvent` boundary: `Connection(ConnectionState)` for searching/connected/unavailable/disconnected state, `Input(protocol::Event)` for decoded HID input, and `ArtworkResult` for hardware upload results. `start_hid_monitor` owns HID initialization, timed input reads, and serialized image writes on one producer thread. GTK submits artwork commands through an in-process channel and consumes input/upload results with a short GLib timer, so device I/O does not block rendering or input handling. This phase has no TCP/HTTP/WebSocket listener or external API server. A future external transport can subscribe behind this event boundary without changing protocol decoding or UI event semantics.

## Development checks

```sh
cargo fmt --check
cargo test --offline
cargo clippy --offline --all-targets --all-features -- -D warnings
```

The protocol decoder validates report ID and declared little-endian payload length before decoding supported input commands. Hardware event behavior requires a connected, accessible Stream Deck +.
