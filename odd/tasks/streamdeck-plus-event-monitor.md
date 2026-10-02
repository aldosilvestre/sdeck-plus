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
- Keep device writes narrowly scoped: upload only explicitly assigned or cleared button/window artwork through the documented HID image commands. Do not alter key mappings, profiles, brightness, sleep settings, or other device configuration.
- Document how to build, run, and troubleshoot permissions/device detection.
- Allow local artwork assignment for each of the eight keypad buttons, the overall Touch Strip, and each of its four encoder zones (13 image targets total); the user confirmed persistent assignments and approved the overall Touch Strip image as a background with per-encoder images overlaid in their zones.
- Keep original artwork assignments and assets local to the app; send only rendered image bytes to the selected Stream Deck + over USB HID. Do not add remote/network services.
- Copy selected images into an app-owned local asset directory and persist stable target-to-asset assignments so moving/removing the original file does not break an assignment.
- Accept PNG and GIF source images in the artwork picker and converter; uploads are still encoded as static JPEG frames because the hardware image reports carry JPEGs, not animated files.

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
- Users can assign, replace, and clear local artwork for all 13 targets; the Touch Strip background and four encoder-zone images render in the agreed layered layout.
- Assignments remain after restarting the app and do not depend on the original selected files remaining at their old paths.
- Invalid images, failed imports, or missing stored assets do not crash the app or destroy a previous valid assignment; users receive a useful fallback/error state.
- PNG and GIF image files are accepted by import and converted to the target's required JPEG dimensions; animated GIF input is rendered as a static frame, not played as an animation on the device.
- When the Stream Deck + is connected, assigned key images appear on the matching physical keys and Touch Strip background/encoder images appear on the device; assignments are replayed after reconnect.
- Uploads use Elgato's documented 120 × 120 JPEG keypad and 800 × 100 / 200 × 100 JPEG touchscreen formats and chunked output-report commands. Clearing explicitly sends a black image for that target; startup leaves targets with no saved assignment untouched.
- HID read and image write operations are serialized through one device-owning thread; write failures are reported clearly and do not erase the persisted local assignment.

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
  - Evidence: Elgato HID API documents input report `0x01`, command `0x02`, type `0x01` TAP and `0x02` PRESS, and type `0x03` FLICK. A user-captured tap frame established coordinate bytes at absolute frame offsets `+0x06/+0x08`, which are payload offsets 2 and 4 after the four-byte frame header. The same payload-relative positions are used for flick start/end coordinates (2, 4, 6, 8). Coordinates are in the 800 × 100 window space. Gestures are reported after interaction completion, not as separate contact states.
  - Verification evidence: `cargo fmt --check`, `cargo test --offline` (6 library tests, 7 binary tests), and `cargo clippy --offline --all-targets --all-features -- -D warnings` passed after the coordinate-offset correction; tests include the user's observed tap frame (X=75, Y=35). Hardware behavior was verified for the captured tap report. Commit: `9bb5e54` (`feat: decode Stream Deck Plus touch gestures`); later coordinate correction remains uncommitted per user instruction.
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
  - Verification evidence: `cargo fmt --check`, `cargo test --offline` (6 library tests and 7 binary tests), and `cargo clippy --offline --all-targets --all-features -- -D warnings` passed during continuation. UI launch/hardware smoke testing remains unverified.
- [ ] `SDP-3c` — Add dedicated encoder and Touch Strip event panels to the GTK UI.
  - Dependency: `SDP-3b`.
  - Route: delegated direct.
  - Delivery: third stacked-to-main slice; include README and tests for the displayed controls; stay within 400 authored lines.
  - Verification evidence: `cargo fmt --check`, `cargo test --offline` (6 library tests and 7 binary tests), and `cargo clippy --offline --all-targets --all-features -- -D warnings` passed during continuation. UI launch/hardware smoke testing remains unverified.
- [x] `SDP-4a` — Add versioned local artwork assignment persistence and safe app-owned image asset import for 13 stable targets.
  - Dependency: existing GTK UI and the user-confirmed artwork scope.
  - Route: delegated direct.
  - Trigger evidence: persistence, validated asset copying, GTK UI integration, and user documentation span multiple non-trivial files; one bounded writer owns implementation.
  - Requirements: target IDs cover keypad buttons 1–8, Touch Strip background, and encoder zones 1–4; assignments survive restart; replacing/clearing and missing/corrupt assets are handled safely; no network/device writes or unauthorized dependency downloads.
  - TDD: standard mode; runner `cargo test --offline`.
  - Verification evidence: `ArtworkTarget` defines 13 stable IDs; `ArtworkStore` uses an XDG app data directory, versioned assignment file, app-owned image copies, image validation before replacement, and safe clear. Tests cover unique target IDs, restart round-trip after deleting the source, replacement/clear, failed invalid replacement preserving the existing assignment, malformed config/missing assets, and unusual source extensions. `cargo fmt --check`, `cargo test --offline` (11 library tests and 7 binary tests), and `cargo clippy --offline --all-targets --all-features -- -D warnings` passed. No new dependencies remain in Cargo manifests/lockfile.
- [x] `SDP-4b` — Add GTK image selection/replacement/clear interactions and render keypad, Touch Strip background, and four encoder-zone artwork without obscuring live event feedback.
  - Dependency: `SDP-4a`.
  - Route: delegated direct.
  - Trigger evidence: multiple UI components and README require coordinated non-trivial changes; one bounded writer owns implementation.
  - Verification evidence: `cargo fmt --check`, parent spot-check `cargo test --offline` (11 library tests, 7 binary tests), and `cargo clippy --offline --all-targets --all-features -- -D warnings` passed. README documents image selection, clearing, and XDG asset location. GTK display/file-picker smoke testing remains unverified because no graphical session was exercised. No commit per the user's explicit instruction.
- [x] `SDP-5a` — Prepare keypad, full Touch Strip, and encoder-zone artwork as device-sized JPEGs and encode documented chunked HID output reports.
  - Dependency: persisted artwork assets and official Elgato Stream Deck + HID output-report reference.
  - Route: delegated direct.
  - Trigger evidence: image scaling/encoding, multiple report packet formats, and pure protocol tests are non-trivial; one bounded writer owns the output codec.
  - Requirements: accept PNG/GIF source images and convert them to static device-sized JPEGs (animated GIFs are not animated on hardware); key image 120 × 120 JPEG command `0x07`; full Touch Strip 800 × 100 JPEG command `0x0B`; four 200 × 100 JPEG overlays via partial-window command `0x0C`; 1024-byte zero-padded output reports, correct per-command chunk headers and final-chunk flag. No unverified device commands and no dependency downloads.
  - TDD: standard mode; runner `cargo test --offline`.
  - Verification evidence: `src/image_output.rs` center-crops to the correct aspect ratio, resizes/encodes in-memory JPEGs, and builds zero-padded 1024-byte HID reports. Tests verify PNG/GIF input conversion to decodable device-sized JPEGs, crop, black clear images, key/full-window/partial-zone headers, chunk sizes/index/final flags/padding, 0-based physical key indices, all zone offsets, and invalid targets. `cargo fmt --check`, parent spot-check `cargo test --offline` (17 library tests and 2 binary tests), `cargo clippy --offline --all-targets --all-features -- -D warnings`, and `git diff --check` passed. No device writes or dependency additions occurred.
- [x] `SDP-5b` — Serialize authorized artwork output through the HID owner thread; upload changes, replay saved assignments on connect/reconnect, report failures, and document hardware sync behavior.
  - Dependency: `SDP-5a`.
  - Route: delegated direct.
  - Trigger evidence: command/event channel, GTK integration, reconnect synchronization, and README changes touch multiple non-trivial files; one bounded writer owns implementation.
  - Requirements: preserve existing input processing; write only selected image report IDs/commands; after reconnect upload saved assignments; no assignment for a never-configured target means leave hardware untouched. Clearing a key sends a black key image; clearing any Touch Strip target rebuilds from its saved background (or black) and reapplies remaining zone images so clearing a zone reveals the strip background when available.
  - Verification evidence: `HidDevice::write` now runs only on the existing HID owner thread, with sequential fixed-size reports and short/error write detection. GTK commands persist locally before queuing; saved key and Touch Strip assignments are replayed after each connection. Updating/clearing the Touch Strip background rebuilds the full composition in order (background or black, then saved zone overlays); clearing a zone restores the background and reapplies remaining zones. Upload results appear in the event feed and failures are also shown in connection detail. Fake-write tests verify order/error behavior; composition tests verify background precedes zones and clearing restores the remaining layers. PNG and GIF files pass conversion tests; animated GIFs are uploaded as static JPEGs. `cargo fmt --check`, parent spot-check `cargo test --offline` (17 library tests and 2 binary tests), `cargo clippy --offline --all-targets --all-features -- -D warnings`, and `git diff --check` passed. `timeout 15s cargo run --offline` launched while `lsusb` showed the Stream Deck + attached; this environment did not provide visual confirmation of the hardware display. No commit per the user's instruction.

## Progress and next step
Task `SDP-1` implementation is present and Rust checks pass, but remains unchecked pending resolution of the native review base-ref requirement and hardware verification. Touch Strip task `SDP-2` is implemented and checked; its coordinate offsets were corrected from the user's real report (tap example X=75, Y=35), with regression tests passing. GTK/API work is authorized; the user selected `stacked-to-main` after the integrated implementation measured 573 authored lines. Slice `SDP-3a` is complete in commit `cc9512d` (114 lines). GTK connection/keypad/feed and encoder/touch panels are present in commit `f92f693`; GTK launch/hardware smoke testing remains unverified. Artwork tasks `SDP-4a` and `SDP-4b` implement 13 saved local targets; `SDP-5a` encodes device-sized JPEG image output reports and `SDP-5b` sends them on the HID owner thread, replays assignments after connection, and rebuilds Touch Strip layers after background/zone changes. PNG and GIF sources are accepted, converted to static JPEG frames, and GIF animation is not displayed on hardware. Functional verification passes: 17 library tests and 2 binary tests plus formatting, Clippy, and diff checks. The app was launched for 15 seconds with the physical device visible in `lsusb`, but visual screen contents were not confirmed. The user previously instructed to continue without committing; no new commit has been created. RDD remains globally on. A prior selectorless STATUS snapshot before hardware-output changes required intended-untracked selection for `src/local_artwork.rs`; later changes invalidate that snapshot, and no refreshed status, START, or approval is claimed. Local delivery is on `feature/streamdeck-plus-event-monitor`; no PR or remote action was performed.
