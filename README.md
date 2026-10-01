# Stream Deck + Event Monitor

A read-only Rust console prototype for observing keypad and encoder input from the Elgato Stream Deck + (VID `0x0FD9`, PID `0x0084`). It does not send reports or change device configuration.

## Requirements and run

- Rust toolchain with Cargo (stable).
- The Stream Deck + connected over USB.
- On Linux, access to the device's HID node. If opening fails with a permission error, use your distribution's supported device-access mechanism (for example, a narrowly scoped udev rule for this VID/PID with `TAG+="uaccess"`, installed by an administrator). Reconnect the device after changing access rules. The program never changes host permissions.

From this directory:

```sh
cargo run
```

The interface lists key press/release, encoder button press/release, and signed encoder rotation ticks. Press `q` or `Esc` to exit. If the device is absent or cannot be opened, a readable diagnostic is shown. A read failure while monitoring is reported as a disconnect/read error.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The protocol decoder validates the report ID and declared little-endian payload length before decoding supported input commands. Runtime behavior with physical hardware requires the device to be connected and accessible.
