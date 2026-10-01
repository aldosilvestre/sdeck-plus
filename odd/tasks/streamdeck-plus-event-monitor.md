# Stream Deck + Event Monitor

## Objective
Create a Rust console prototype that detects the Elgato Stream Deck + and clearly displays keypad presses, encoder-button presses, and encoder rotation events.

## Problem and rationale
The empty project needs a small, useful first milestone before full device configuration: observe HID input events and identify the physical control that generated each event.

## Scope
- Detect the Stream Deck + by VID `0x0FD9` and PID `0x0084`.
- Poll HID input reports and decode keypad state changes (`report 0x01`, command `0x00`), encoder button changes (`0x01`, command `0x03`, content type `0x00`), and encoder rotations (`0x01`, command `0x03`, content type `0x01`).
- Present labeled controls and live event feedback in a clear, attractive terminal interface.
- Keep the first version read-only: no image uploads, key mappings, profiles, or device configuration writes.
- Document how to build, run, and troubleshoot permissions/device detection.

## Constraints
- Follow Elgato's published Stream Deck HID protocol; shared input fields include report ID, command, and little-endian payload length.
- Use Rust and preserve safe behavior on malformed or unsupported reports and device disconnects.
- Technical artifacts are in English.
- Testing mode: standard testing, explicitly selected by the user; test runner: `cargo test`.
- Delivery strategy: `ask-on-risk` (default); chain strategy: not applicable to this single work unit.

## Authorized scope
- Create the application and supporting documentation in this otherwise empty directory.
- No remote operations, external account access, or modification of host-wide device permissions.

## Acceptance criteria
- The program opens the matching Stream Deck + HID interface and reads its input reports.
- Each keypad press/release identifies the keypad index/position.
- Encoder button press/release identifies one of four encoders.
- Encoder rotation identifies the encoder and signed tick direction/count.
- A polished terminal view clearly distinguishes those event classes and handles unavailable device/disconnect gracefully.
- Unit tests cover protocol decoding for the supported reports; `cargo test` passes.
- README documents prerequisites and run instructions.

## Applicable checks
- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`
- Build/run smoke check where the environment permits; hardware-dependent behavior must be reported honestly if no device is connected.

## Tasks
- [ ] `SDP-1` — Implement the Rust Stream Deck + HID event monitor, polished terminal interface, protocol decoder tests, and README.
  - Route: delegated direct.
  - Trigger evidence: the implementation requires multiple non-trivial source/documentation files and protocol/library research; one bounded writer owns the write.
  - Progress: implementation and README added in `Cargo.toml`, `src/`, and `README.md`. Initial compile exposed an uninferred event-list type in `src/main.rs`; fixed with an explicit `Vec<(String, Color)>` annotation.
  - Verification evidence: `cargo fmt --check` passed; `cargo test` passed (3 unit tests); `cargo clippy --all-targets --all-features -- -D warnings` passed. `timeout 5s cargo run` exited successfully and displayed the expected missing-device message; no Stream Deck + was connected, so hardware event behavior remains unverified.
  - Review status: RDD is enabled. Native assessment requires a base ref for this single-root-commit repository; selectorless STATUS returned `empty_candidate_base_ref_required`. No native review approval is claimed.

## Progress and next step
Task `SDP-1` implementation is present and Rust checks pass, but remains unchecked pending resolution of the native review base-ref requirement and hardware verification. Local delivery is on `feature/streamdeck-plus-event-monitor`; no remote operations were performed.
