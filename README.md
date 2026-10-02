# Stream Deck + Event Monitor

A read-only GTK4 desktop monitor for the Elgato Stream Deck + (VID `0x0FD9`, PID `0x0084`). The Linux-native interface runs through GTK's selected display backend, including Wayland, and never writes to or reconfigures the device.

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

## In-process event API

`src/event_api.rs` defines the typed `MonitorEvent` boundary: `Connection(ConnectionState)` for searching/connected/unavailable/disconnected state and `Input(protocol::Event)` for decoded HID input. `start_hid_monitor` owns HID initialization and blocking reads on a dedicated producer thread. The GTK main loop consumes these messages from a channel using a short GLib timer, so HID reads never block rendering or input handling. This phase has no TCP/HTTP/WebSocket listener or external API server. A future external transport can subscribe behind this event boundary without changing protocol decoding or UI event semantics.

## Development checks

```sh
cargo fmt --check
cargo test --offline
cargo clippy --offline --all-targets --all-features -- -D warnings
```

The protocol decoder validates report ID and declared little-endian payload length before decoding supported input commands. Hardware event behavior requires a connected, accessible Stream Deck +.
