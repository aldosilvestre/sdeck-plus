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
- Add persistent, named application layers with an active-layer selector; keep layer state local to the app and do not write Stream Deck profiles/configuration.
- Bind keypad presses, encoder-button presses, encoder rotations, and completed Touch Strip tap/press/flick gestures to typed actions: switch layer, launch an application or executable with explicit argument-list semantics (never implicit shell evaluation), or emit an internal app event.
- Defer global shortcut injection until a Linux desktop/compositor backend is selected; do not install or download external shortcut tooling.

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
- Layers and action bindings persist independently from artwork. Invalid/missing configuration falls back safely without discarding a prior valid file.
- Matching input actions execute outside the HID owner thread and GTK event loop; command actions invoke an executable directly with configured arguments, never a shell.
- Layer switch bindings select a named target layer; unknown targets do not crash the app, and the active layer always resolves to a configured layer.
- Internal event actions are delivered through the in-process event API and visible in the application event activity; no network listener is added.
- The layer UI permits creating, naming, selecting, and removing layers and configuring bindings for supported input types.
- Input capabilities remain type-safe: keypad keys trigger key-press events only; encoder buttons trigger button-press events; encoder rotation is signed and scoped to an encoder; the Touch Strip supports tap, press, and flick gestures. Physical encoders have no artwork target; `EncoderZone` means the corresponding Touch Strip display segment, not the encoder knob.

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
- [x] `SDP-6` — Add persistent named application layers and typed input-action bindings.
  - Dependency: existing typed HID event API, GTK interface, and app-local persistence conventions.
  - Route: delegated direct.
  - Trigger evidence: configuration model/persistence, event matching/dispatch, GTK editing controls, tests, and README are multiple non-trivial files; one writer owns the changes.
  - Authorized behavior: create/name/select/remove app-local layers; bind keypad press, encoder-button press, encoder rotation, and completed Touch Strip tap/press/flick to layer switching, launching an executable plus argument list without a shell, or emitting an internal app event. Global shortcut injection is deferred until a Linux backend is selected.
  - Constraints: no network API, no device profile/configuration writes, no dependency download without explicit authorization, and do not create commits (prior user instruction remains in force).
  - TDD: standard mode; runner `cargo test --offline`.
   - Applicable verification: `cargo fmt --check`, `cargo test --offline`, `cargo clippy --offline --all-targets --all-features -- -D warnings`, and `git diff --check`.
   - Implementation evidence: `src/layers.rs` adds a versioned, bounded, hex-encoded layer store separate from artwork, validated layer names/triggers/actions, a protected `default` layer, safe invalid-file fallback, selector/switch/removal behavior, cleanup of deleted-layer targets, and exact signed rotation matching. GTK provides layer create/select/remove and binding controls. The input matcher ignores key/button releases and matches completed touch gesture classes. `MonitorEvent::AppEvent` is delivered through the existing in-process channel and displayed in Recent Events. Launches use `Command::new(...).args(...)` and are spawned away from HID and GTK callback execution; no shell or dependency was added. README documents configuration selectors and launch semantics.
    - Correction: layer mutations now update the in-memory store immediately, and immutable snapshots are queued FIFO to a dedicated layer worker for persistence. The same worker launches configured executables directly with argv; persistence and launch failures return through `MonitorEvent::LayerError` and render in the connection detail and recent-event feed. Incoming event matching and valid layer switches still update the active in-memory state synchronously; switch snapshots enter the same ordered queue. Corrupt configuration continues to fall back through the existing parser without overwriting the source until a subsequent user mutation is persisted. No dependency was added.
    - Verification evidence: `cargo fmt --check` passed; `cargo test --offline` passed (20 library tests and 2 binary tests); `cargo clippy --offline --all-targets --all-features -- -D warnings` passed; `git diff --check` passed. Tests cover persistence/selection/switch/removal, invalid config and selectors/actions, and argv construction. GUI/hardware runtime behavior was not exercised.
     - Remaining validation: graphical responsiveness and runtime rendering were not exercised in this headless run; no Stream Deck hardware behavior is claimed. The synchronous startup config load remains unchanged; interactive layer writes and child-process launching are isolated from GTK and HID threads.
- [ ] `SDP-7` — Fix layer-creation crash and make `Base` the initial layer.
  - Dependency: `SDP-6`.
  - Route: delegated direct.
  - Trigger evidence: user reported the application closes after adding a layer and specified that layer 0 must be named `Base`; during physical key testing the app window disappeared while a Base-to-Studio switch binding was configured.
  - Authorized scope: fix GTK layer selection/create/remove, button-binding, event-action dispatch, and synchronous layer-selector notification interactions; set initial/default layer name to `Base` while safely migrating existing `default` configurations and bindings; test keypad, encoder-button/rotation in both directions, and Touch Strip tap/press/flick event matching; verify live layer creation/selection and key-to-layer binding; preserve the no-artwork-target rule for physical encoder knobs.
  - TDD: standard mode; runner `cargo test --offline`.
  - Applicable verification: `cargo fmt --check`, `cargo test --offline`, `cargo clippy --offline --all-targets --all-features -- -D warnings`, `git diff --check`, and runtime test of creating a layer, selecting it, configuring bindings, and switching from `Base` via an input action.
   - Implementation evidence: changed the protected initial layer to `Base`; legacy `default` layer IDs are canonicalized during load, including active-layer IDs, binding owners, and switch targets. Canonicalization prevents a duplicate legacy default layer or orphaned bindings, while persistence writes only canonical IDs. `Base` is explicitly placed first in selector names. GTK selection, binding, and removal callbacks extract the selected name within a limited immutable-borrow scope before taking the mutable borrow; callback helpers are regression-tested. Removal queues the post-removal `LayerStore` snapshot through the existing FIFO persistence worker and then returns the picker to Base.
    - Runtime evidence: under Wayland, keyboard navigation and text entry in the actual app created `Studio`; the window remained open and its layer selector changed to `Studio`. The selector was changed back to `Base`. The trigger `k:1` and action `switch:Studio` were entered into the binding controls, Save binding was activated, and the isolated test store's `layers.conf` was read back showing the persisted Base/key-1-to-Studio binding. The app used a temporary `XDG_DATA_HOME`, so no existing user configuration was altered. This verifies actual GTK layer creation, selection, and binding persistence.
     - Event-matrix evidence: `src/layers.rs::event_capabilities_match_only_their_typed_trigger` binds seven distinct safe internal Emit actions in Base and verifies `Trigger::from_event` plus `LayerStore::matching_action`: keypad press matches KeyPress while release maps to no trigger/action; encoder-button press matches EncoderPress while release maps to none; positive and negative encoder rotations preserve signed tick counts and remain distinct from encoder-button events; TouchTap, TouchPress, and TouchFlick each match only their own trigger/action. No production behavior needed changing for these mappings.
       - Event-action switch borrow fix: `MonitorEvent::Input` calls `matching_action(&layers, &event)` so action matching returns owned data before the mutable switch. This was necessary but insufficient: `gtk_single_selection_set_selected` synchronously emits selected-notify, and the nested `layers.borrow().names()...position(...)` argument kept an immutable borrow alive while `select_layer` attempted a mutable borrow.
        - Confirmed crash root cause: `coredumpctl info 2589945` reports SIGABRT. The main-thread stack includes `gtk_single_selection_set_selected` from the `MonitorEvent::Input` SwitchLayer branch and enters the build_ui `DropDown::connect_selected_notify` callback. That callback calls `select_layer`, which mutably borrows the same `RefCell<LayerStore>` while the selected-index expression's temporary immutable borrow is alive.
        - Correction/regression: compute the target selector index with `layer_index` in a completed scope, then call `set_selected` after the borrow drops. Regression runs the real matching/switch path, computes the selector index, and invokes `select_layer` as the synchronous notification callback; it asserts Studio selection completes without a borrow panic.
       - Verification evidence: `cargo fmt --check` passed; `cargo test --offline` passed (22 library tests, 7 binary tests); `cargo clippy --offline --all-targets --all-features -- -D warnings` passed; `git diff --check` passed. The selector-notify regression passed. After the fix, physical keypad 1 changed the live app from Base to Studio and the process remained open; the isolated config persisted Studio as active. User then reported encoder 1 clockwise rotation registered `+1`; the app remained open in Base in the available screenshot. Counterclockwise rotation, encoder-button press, and Touch Strip tap/press/flick still need physical validation. No profile changes or device writes were made; keep SDP-7 open.
    - Artwork scope: `EncoderZone` images are Touch Strip display regions, never images of the physical encoder knobs; physical encoders have no artwork target.

## Progress and next step
Task `SDP-1` implementation is present and Rust checks pass, but remains unchecked pending resolution of the native review base-ref requirement and hardware verification. Touch Strip task `SDP-2` is implemented and checked; its coordinate offsets were corrected from the user's real report (tap example X=75, Y=35), with regression tests passing. GTK/API work is authorized; the user selected `stacked-to-main` after the integrated implementation measured 573 authored lines. Slice `SDP-3a` is complete in commit `cc9512d` (114 lines). GTK connection/keypad/feed and encoder/touch panels are present in commit `f92f693`; GTK launch/hardware smoke testing remains unverified. Artwork tasks `SDP-4a` and `SDP-4b` implement 13 saved local targets; `SDP-5a` encodes device-sized JPEG image output reports and `SDP-5b` sends them on the HID owner thread, replays assignments after connection, and rebuilds Touch Strip layers after background/zone changes. PNG and GIF sources are accepted, converted to static JPEG frames, and GIF animation is not displayed on hardware. Functional verification passes: 20 library tests and 2 binary tests plus formatting, Clippy, and diff checks. SDP-6 layer writes and process launches now use a dedicated worker; no commit, PR, or remote operation was performed.
SDP-6's interactive persistence and process-launch blocker is resolved. GTK and HID event paths retain immediate matching/state updates and enqueue worker commands; runtime UI/hardware behavior remains unverified.
SDP-7's selection/binding/removal borrow fixes and legacy `default` to `Base` migration are implemented and covered by tests. `coredumpctl info 2589945` confirms SIGABRT with the GTK selection setter and synchronous selected-notify callback on the SwitchLayer path; the nested index lookup held an immutable RefCell borrow across that callback. `layer_index` now ends the borrow before `set_selected`, with a regression that models synchronous callback selection to Studio. Previous matching-action scoping remains necessary but did not fix this callback reentrancy hazard. After the fix, physical keypad 1 changed Base to Studio and the app remained open. User-reported encoder 1 clockwise rotation registered `+1`; counterclockwise rotation, encoder-button press, and Touch Strip tap/press/flick are still pending. `cargo fmt --check`, `cargo test --offline` (22 library, 7 binary), `cargo clippy --offline --all-targets --all-features -- -D warnings`, and `git diff --check` pass. SDP-7 remains open; preserve the running test app and isolated XDG data for continuation.
