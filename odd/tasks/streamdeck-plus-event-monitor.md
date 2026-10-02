# Stream Deck + Event Monitor

## Objective
Create a Rust desktop application for Linux that detects the Elgato Stream Deck + and displays keypad presses, encoder-button presses, encoder rotation, and Touch Strip tap, press, and flick events in a polished GTK4 interface.

## Problem and rationale
The empty project needs a small, useful first milestone before full device configuration: observe HID input events and identify the physical control that generated each event.

## Scope
- Detect the Stream Deck + by VID `0x0FD9` and PID `0x0084`.
- Poll HID input reports and decode keypad state changes (`report 0x01`, command `0x00`), encoder button changes (`0x01`, command `0x03`, content type `0x00`), and encoder rotations (`0x01`, command `0x03`, content type `0x01`).
- Decode Touch Strip activity (`report 0x01`, command `0x02`): taps (`content type 0x01`), long presses (`0x02`), and flicks (`0x03`) with their documented coordinates. The device reports completed gestures rather than separate contact-start/release events.
- Present labeled controls and live event feedback in a clear, attractive application interface.
- Replace the terminal interface with a polished GTK4 desktop UI suitable for Wayland.
- Route HID events to the UI through a documented in-process event API. This is a single-application design; do not add a network listener or external API server unless requested later.
- Keep the first version read-only: no image uploads, key mappings, profiles, or device configuration writes.
- Document how to build, run, and troubleshoot permissions/device detection.

## Constraints
- Follow Elgato's published Stream Deck HID protocol; shared input fields include report ID, command, and little-endian payload length.
- Use Rust and preserve safe behavior on malformed or unsupported reports and device disconnects.
- Technical artifacts are in English.
- Testing mode: standard testing, explicitly selected by the user; test runner: `cargo test`.
- Delivery strategy: `ask-on-risk`; chain strategy: `stacked-to-main` (selected by the user for the >400-line UI/API change).

## Authorized scope
- Create the application and supporting documentation in this otherwise empty directory.
- No remote operations, external account access, or modification of host-wide device permissions, except the user-authorized public crates.io dependency download for GTK4 (no credentials; no other remote destinations).

## Acceptance criteria
- The program opens the matching Stream Deck + HID interface and reads its input reports.
- Each keypad press/release identifies the keypad index/position.
- Encoder button press/release identifies one of four encoders.
- Encoder rotation identifies the encoder and signed tick direction/count.
- Touch Strip tap, press/hold, and flick identify gesture type and documented start/end coordinates; coordinates follow Elgato's 800 × 100 logical-pixel space.
- A polished GTK4 view clearly distinguishes those event classes and handles unavailable device/disconnect gracefully.
- The UI receives HID events through the in-process event API without blocking the GTK main loop.
- Unit tests cover protocol decoding for the supported reports, including malformed touch reports; `cargo test` passes.
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
- [x] `SDP-2` — Add Stream Deck + Touch Strip tap, press, and flick report decoding, event display, and protocol tests.
  - Route: delegated direct.
  - Trigger evidence: implementation changes multiple non-trivial source/test files; protocol details are now established by Elgato's HID API documentation and device behavior by its Touch Strip guide.
  - Evidence: Elgato HID API documents input report `0x01`, command `0x02`, type `0x01` TAP and `0x02` PRESS with X/Y at offsets `+0x06/+0x08`, and type `0x03` FLICK with start/end X/Y at `+0x06` through `+0x0C`; payload lengths are `0x0A` and `0x0E`. Coordinates are in the 800 × 100 window space. Gestures are reported after interaction completion, not as separate contact states.
  - Verification evidence: `cargo fmt --check` passed; `cargo test` passed (5 tests; independently rerun by parent); `cargo clippy --all-targets --all-features -- -D warnings` passed. Hardware events were not verified because no device was connected. Commit: `9bb5e54` (`feat: decode Stream Deck Plus touch gestures`).
- [x] `SDP-3a` — Expose the typed in-process HID event API as a tested Rust library boundary.
  - Route: delegated direct.
  - Trigger evidence: implementation spans the protocol/event model and library boundary.
  - Accepted architecture: one application only; HID producer publishes typed events through an in-process channel/API consumed by GTK. No HTTP/WebSocket listener or remote access is needed now. A future external API can be added behind the same event boundary.
  - TDD: standard mode, test runner `cargo test`, as already recorded for this feature.
  - Delivery: first stacked-to-main slice; changes must be independently buildable and stay within 400 authored additions plus deletions.
  - Verification evidence: 114 authored lines; `cargo fmt --check` passed; `cargo test --offline` passed (6 library tests, 7 binary tests; parent spot-check passed); `cargo clippy --offline --all-targets --all-features -- -D warnings` passed. Commit: `cc9512d` (`feat: expose typed HID event API`).
- [ ] `SDP-3b` — Replace the terminal view with a polished GTK4 Wayland-compatible window showing connection state, keypad state, and a live event feed.
  - Dependency: `SDP-3a`.
  - Route: delegated direct.
  - Delivery: second stacked-to-main slice; include GTK dependencies, launch instructions, and verification with the UI behavior; stay within 400 authored lines.
  - Verification evidence: pending.
- [ ] `SDP-3c` — Add dedicated encoder and Touch Strip event panels to the GTK UI.
  - Dependency: `SDP-3b`.
  - Route: delegated direct.
  - Delivery: third stacked-to-main slice; include README and tests for the displayed controls; stay within 400 authored lines.
  - Verification evidence: pending.

## Progress and next step
Task `SDP-1` implementation is present and Rust checks pass, but remains unchecked pending resolution of the native review base-ref requirement and hardware verification. Touch Strip task `SDP-2` is implemented and checked. GTK/API work is authorized; the user selected `stacked-to-main` after the integrated implementation measured 573 authored lines. Slice `SDP-3a` is complete in commit `cc9512d` (114 lines); `SDP-3b` (GTK connection/keypad/feed) and `SDP-3c` (encoder/touch panels) remain. Native review assessment for each work-unit commit is required before the next slice; the earlier workspace preflight was blocked at intended-untracked selection for pre-existing `.atl/` files. Local delivery is on `feature/streamdeck-plus-event-monitor`; no PR was created or remote action performed.
