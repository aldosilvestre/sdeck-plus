# Stream Deck + Event Monitor

A GTK4 desktop monitor for the Elgato Stream Deck + (VID `0x0FD9`, PID `0x0084`). It displays live input and can send explicitly assigned artwork to physical keys and the Touch Strip over USB HID. It does not alter profiles, key mappings, brightness, sleep, or other device configuration.

## Requirements

- Rust stable toolchain and Cargo.
- GTK 4.8 or newer development files and a C toolchain. On Arch Linux: `sudo pacman -S gtk4 base-devel`.
- Stream Deck + connected over USB for live device events.
- Permission to open the device's HID node. If access is denied, use the distribution's supported device-access mechanism (for example, an administrator-installed narrowly scoped udev rule for this VID/PID with `TAG+="uaccess"`), then reconnect the device. The application does not change host permissions.

## Build and run

```sh
cargo run
```

The UI displays connection state, eight keypad states, four encoder button/rotation states, and Touch Strip Tap, Press, and Flick coordinates in Elgato's 800 × 100 logical-pixel space. Touch gestures are reported after the interaction completes, not as separate contact start/release events. If the device is absent, inaccessible, or disconnected, the UI remains open and reports the state; it retries detection after connection failures.

## Graceful shutdown

Closing the application window stops device communication, discards pending artwork uploads, removes UI polling, and joins the HID worker to release the device. Application shutdown repeats this cleanup safely as a fallback. A synchronous HID call already admitted before cancellation may finish; shutdown waits for it, so a blocked driver call can delay closing. No new reports are admitted after cancellation.

Shutdown does not clear the displays or send shutdown artwork. The device may retain its last pixels; retained images do not mean the application is still transmitting. Unplug/reconnect retries and artwork replay remain active while the application is open. External action subprocesses are not cancelled or synchronously joined on close.

## Local artwork

Select **Artwork target (active layer)** in the right settings panel, then use the shared **Image…** or **×** (Clear artwork) controls. All thirteen targets are available: eight keys, the Touch Strip background, and four touch zones. Clicking a key or touch zone also selects its artwork target. Clicking an encoder selects only its action binding; use the artwork selector explicitly to choose an image region. PNG and GIF files are explicitly available in the picker; a separate filter includes all image formats supported by the installed GdkPixbuf loaders. GIF artwork is uploaded as a static still image; animation is not supported by the hardware JPEG upload format. Artwork previews preserve aspect ratio and are center-cropped to fill their surfaces. Images are center-cropped to cover the device target and sent as correctly sized JPEGs over USB HID. The eight keys use 120 × 120 images; the Touch Strip background uses 800 × 100 and each encoder overlay uses a 200 × 100 region. The background and zone images can be layered. A failed import leaves the current assignment unchanged. **×** (Clear artwork) removes the local assignment; clearing a key sends a black key image, while clearing a Touch Strip image rebuilds the strip from its saved background (or black) and remaining zone images. It does not issue a separate device clear command.

Images are copied into the application's XDG data directory, so the original files may be moved or deleted after assignment. The assignment manifest is stored beside the copied assets. The default path is `$XDG_DATA_HOME/streamdeck-plus-monitor` (normally `~/.local/share/streamdeck-plus-monitor`); when `XDG_DATA_HOME` is unset, the application uses `$HOME/.local/share/streamdeck-plus-monitor`. Artwork is independent per layer. Legacy global assignments migrate into the active layer (or `Base` if unavailable); other layers start empty. Selecting a layer refreshes all thirteen previews and replaces the device's complete image state: unassigned keys become black, and the strip is rebuilt from its background (or black) followed by assigned zones. The same state is replayed after reconnect, so empty layers clear previous images. A chooser saves to the artwork target and layer captured when it was opened, even if you change the selected control, artwork target, or layer before accepting; acceptance is rejected if that layer was removed, even if another layer was created with the same name. Storage open or migration failures show a repair message and disable artwork writes and layer removal rather than saving an empty fallback. Layer removal is aborted if artwork cleanup cannot be persisted. Upload failures are shown in the connection status; local assignments remain saved if a device write fails.

## Application layers and actions

Layers and action bindings are stored independently in `layers.conf` in the same app data directory; per-layer artwork assets and assignments remain in their separate artwork store. Create, select, or remove a named layer in **APPLICATION LAYERS**. The built-in `Base` layer cannot be removed; legacy files that named the protected layer `default` are migrated to `Base`.

To configure an action, choose the layer at the top, then select a physical control:

1. Click one of the eight buttons (two rows of four), the horizontal touch strip, or one of the four encoders beneath it. Clicking an artwork zone also selects the whole touch strip; zones are image regions, not separate gesture bindings.
2. Pick an event valid for that control: button press; encoder press/clockwise/counterclockwise; or touch tap, press-and-hold, generic swipe/flick, and directional swipes.
3. Pick an action and fill only the fields shown for it. **Move to layer** uses the destination layer selector, **Run command** uses shell text, **Open application** uses an executable plus one escaped argument per line, **Shortcut** is the sole keyboard action and accepts a full chord such as `CTRL + D` or `SUPER + RETURN`, or one key with optional modifier checkboxes, and **Emit legacy event** keeps compatibility with older in-process event bindings.
4. Click **Save binding** to create or update that control event, or **Remove binding** to clear it. Selecting an already configured control event loads its saved action for inspection and editing.

Hover a physical control to see assignments for the selected layer, including **Unassigned** events: encoders include **Press**, **CW** (clockwise), and **CCW** (counterclockwise); the touch strip includes tap, hold, flick, and directional swipes. Legacy exact-tick rotation overrides appear separately. Long action text is shortened; select the event to inspect its full saved fields. Summaries refresh after saving/removing bindings, selecting/creating/removing layers, and input-driven **Move to layer** actions. Selection highlighting is separate from live pressed-state highlighting, and live input does not replace assignment summaries. The shared **Image…** and **×** edit the explicitly selected artwork target, independently of the binding editor.

### Window layout

All controls fit without scrolling at the default 1000 × 700 logical-pixel client size: the device canvas stays on the left, action settings on the right, and layer controls above. The last-event panel has been removed; live physical states and connection/error indicators remain. Summaries and long layer names are ellipsized; tooltips show full summaries and the editor retains full saved action fields.

Every outer edge has 38 logical pixels of padding: a 10mm reference at 96dpi, not a physical measurement guarantee. Actual millimeters depend on monitor DPI and scaling. Inactive window, viewport, shell, and cards share one dark background. The physical preview intentionally contrasts: a nearly square silver chassis, two rows of four separated black square keys, a continuous black display bezel, and four round purple knobs on a recessed silver rail aligned to the key columns. The artwork strip retains its 8:1 ratio inside the wider bezel; transparent zones preserve strip-background layering. No sample artwork or audio values are rendered. Selection, live state, keyboard focus, and connection status remain distinct.

The serialized production GTK test checks the shared editor's thirteen assign/clear paths, explicit target selection, chooser ownership, reference geometry, and all five actions at 1000 × 700 with empty, loaded, and cleared artwork and long editor values, without moving scroll adjustments. It also checks 1840 × 980, 1280 × 720, and 960 × 640 horizontal fit separately. Reduced heights (including 960 × 640) may scroll vertically; 640 × 480 retains two-axis scroll reachability. Larger system fonts can also require scrolling. Window managers may override the requested startup size; no desktop resolution change is needed.

Launch arguments are passed as distinct process arguments; no shell evaluates launches. In the application-argument box, an empty box means zero arguments, `\\empty` means one empty argument, and multiple empty arguments are written on separate `\\empty` lines. Literal backslash, newline, carriage return, and tab are escaped as `\\\\`, `\\n`, `\\r`, and `\\t`, so leading/trailing spaces are preserved instead of trimmed. Command actions intentionally run `/bin/sh -c` with the configured shell string. External actions execute serially on a background worker, and failures appear in the UI without blocking GTK. Internal events remain on the in-process `MonitorEvent::AppEvent` channel and are safely consumed without a display panel. Action, persistence, and artwork-upload errors appear in the connection detail. Bindings do not inherit or fall through between layers.

### Physical keyboard actions (Linux)

**Shortcut** uses a Linux uinput virtual keyboard, not compositor shortcut dispatch, shell commands, desktop bindings, or text typing. With no modifiers, it sends one complete key down/up. With modifiers, it holds them while the main key goes down/up, then releases them in reverse order. For example, `CTRL + D` holds Control during D; `SUPER + RETURN` holds Windows/Meta during Enter. Modifiers are released after each action, not kept held between bindings. Failed sends attempt release cleanup and broken devices are discarded.

- Full chord text and checkbox modifiers are merged and deduplicated in CTRL, SHIFT, ALT, SUPER order. Checkboxes may all be empty. Names are case-insensitive; `Control` = `CTRL`, `Win`/`Windows`/`Meta` = `SUPER`, and `Return` = `Enter`. Uppercase letters do not implicitly add Shift.
- Saved legacy **Press key** actions keep working and appear in the editor as **Shortcut** with no modifiers. Inspection never rewrites saved records or key names; explicitly saving converts that binding to a canonical Shortcut. Unsupported legacy names remain visible unchanged, but validation rejects unsupported explicit saves.
- New bindings save canonical modifier/key fields and reject unknown names, empty segments, multiple main keys, or modifier-only chords. Supported keys include ASCII letters/digits, F1–F24, Enter, Tab, Escape, Space, navigation, named punctuation, keypad and media keys. Structurally valid legacy names remain loadable and unchanged; unsupported names report an actionable execution error, not a destructive migration.
- Names identify physical evdev positions. The active desktop keyboard layout determines resulting symbols; this is not Unicode text entry and does not promise indistinguishable hardware behavior.
- The serial layer worker owns one lazy, persistent keyboard. Startup and non-keyboard actions do not open uinput. First use requires effective access to `/dev/uinput` and kernel uinput support. Access/create/write failures are reported; there is no fallback or automatic privilege escalation, module loading, permission change, or desktop configuration change. Ask an administrator to assess narrowly scoped access if needed; a device node or udev tag alone does not prove effective permission.
- Creation has a bounded 200 ms discovery grace period, not proof that the compositor has recognized the keyboard. Real access, first-action delivery, compositor acceptance and layout behavior remain unverified on hardware. Automated tests use mocks only and never open or send to a real keyboard.

The versioned layer file is validated on load; malformed or unreadable layer data leaves startup configuration unavailable with writes disabled. The application shows a repair message and does not save an empty `Base` fallback over the original file; repair `layers.conf` in the app data directory and restart. Missing layer-switch targets are rejected, and removing a layer removes bindings targeting it. Legacy exact rotation bindings remain supported and are checked before directional rotation fallbacks, but the primary editor only creates the generic clockwise/counterclockwise rotation bindings.

## In-process event API

`src/event_api.rs` defines the typed `MonitorEvent` boundary: `Connection(ConnectionState)` for searching/connected/unavailable/disconnected state, `Input(protocol::Event)` for decoded HID input, and `ArtworkResult` for hardware upload results. `start_hid_monitor` returns an owned `HidController` with an artwork sender, explicit cancellation, and idempotent joined cleanup. It owns HID initialization, interruptible retry waits, timed input reads, and serialized image writes on one producer thread. Consumers ignore the `Heartbeat` liveness probe; losing the event consumer also ends the worker. GTK submits complete desired artwork snapshots through an in-process channel; only the newest pending snapshot is applied, replacing obsolete disconnected state. GTK consumes input/upload results with a short GLib timer, so device I/O does not block rendering or input handling. This phase has no TCP/HTTP/WebSocket listener or external API server. A future external transport can subscribe behind this event boundary without changing protocol decoding or UI event semantics.

## Development checks

```sh
cargo fmt --check
cargo test --offline
cargo clippy --offline --all-targets --all-features -- -D warnings
```

The single GTK widget test uses the production widget constructor without starting HID/action workers. Run `cargo test --offline -- --nocapture` in a display session to see the executed sizing/reachability markers; without a display it explicitly reports a skip. Measurements are layout checks, not a substitute for manual visual, chooser, and device verification.

The protocol decoder validates report ID and declared little-endian payload length before decoding supported input commands. Hardware event behavior requires a connected, accessible Stream Deck +.
