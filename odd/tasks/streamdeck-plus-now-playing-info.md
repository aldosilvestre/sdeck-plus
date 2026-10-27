# streamdeck-plus-now-playing-info

## Objective
Add a "Now playing" live information source for keys and touch-strip sections:
what media is playing, whether it is playing or paused, and a clear caption and
icon when nothing is playing.

## Problem
Live information covers volume, audio output, brightness, clock, weather, CPU,
memory, temperature and OBS (`InfoSource` in src/info.rs, drawn by
src/info_render.rs). There is no media status, although the app already talks
MPRIS over the D-Bus session bus for media actions (src/desktop_actions/mpris.rs).

## Behavior
- Reads MPRIS players on the session bus (`org.mpris.MediaPlayer2.*`):
  PlaybackStatus and Metadata (xesam:title, xesam:artist).
- Player choice: a Playing player wins; otherwise the most recently seen
  Paused one; otherwise none.
- Playing: play glyph, title as headline, artist as detail.
  Paused: pause glyph, title/artist, muted (inactive) accent.
  No player or Stopped: idle media glyph and "Nothing playing".
  Bus unavailable: the normal unavailable value.
- Same rendering on preview and device, keys and sections; long titles follow
  the existing fit/ellipsize rules. No blocking work in GTK callbacks.

## Scope / constraints
- No new crates unless the existing MPRIS adapter cannot be reused (report).
- Never execute media actions from the info source; read-only.
- Light/dark parity; info.json compatibility (unknown ids keep working as now).
- Branch `feat/now-playing-info` from main e1e51df.

## Tasks
- [x] N1 Core: `InfoSource::NowPlaying` (id, label, glyphs), MPRIS reader with a testable bus abstraction, player selection and value formatting; lib tests (playing, paused, stopped, none, multiple players, missing metadata, bus error).
- [x] N2 Worker + UI: polled/refreshed on the info worker, selectable in the live information settings, rendered with new binding-vocabulary glyphs (play, pause, idle media); GTK/render tests.
- [x] N3 DESIGN.md, docs/index.html (live information), README features bullet, CHANGELOG.md, regenerated screenshots; manual check with `dbus-run-session` + mpv if available.

- [x] N4 Marquee for long Now playing titles (keys and sections, device and preview); tests; docs.
- [x] N5 Application icon on Now playing (DesktopEntry -> theme icon, cached), badge, tests, docs.

## Route
Delegated direct (one writer): 2+ non-trivial files. RDD unavailable (`gentle-ai` not installed).

## Checks
cargo fmt --all -- --check; cargo clippy --offline --all-targets --all-features -- -D warnings;
env -u DISPLAY -u WAYLAND_DISPLAY GDK_BACKEND=x11 cargo test --offline;
GDK_BACKEND=x11 cargo test --offline --bin sdeck-plus; cargo build --offline --examples; screenshot regeneration.

## Progress
Created 2026-10-27. Engram mirror pending (engram tools unavailable this session).

### N1
- Route: delegated direct (one writer; info.rs + mpris.rs + tests are 2+ non-trivial files).
- Read path: `desktop_actions::read_players()` in src/desktop_actions/mpris.rs reuses the
  gio `DBusConnection` + private `Bus` trait: `ListNames`, then per player
  `Properties.GetAll(org.mpris.MediaPlayer2.Player)` (PlaybackStatus, Metadata
  xesam:title / xesam:artist as `as` or `s`), and `Get(org.mpris.MediaPlayer2, Identity)`
  only when the title is missing. `NO_AUTO_START`, 1 s timeout per call, read-only.
  Discovery failures are errors (unavailable value); a single broken player is left out.
- Selection (`info::NowPlaying`): Playing wins (first by bus name); otherwise the Paused
  player last seen Playing (never-playing ties: first by bus name); Stopped = nothing.
- Values: Playing = `Glyph::Play`, title (-> Identity -> "Unknown title") + artist
  (wrapped 14x2); Paused = `Glyph::Pause`, inactive; nothing = `Glyph::MediaIdle`, empty
  headline, detail "Nothing playing", inactive. Source id `now-playing`, 1 s poll.
- Interim icon mapping until N2: Play -> play-pause, Pause -> pause, MediaIdle -> play-sound.
- RED: `cargo test --offline --lib now_playing` failed to compile (49 errors: missing
  `NowPlaying`, `PlayerState`, `read_players_on`, glyphs). GREEN: lib 342/342.
- Checks: fmt ok, clippy -D warnings ok, headless suite ok, display bin 63/63.

### N2
- Route: delegated direct (same writer; binding_icons.rs, info_render.rs, GTK test).
- Worker: `InfoReader` reads `NowPlaying` on the existing info worker thread every 1 s
  (wired in N1); never on the GTK thread. Settings: the source list is built from
  `InfoSource::ALL`, so "Now playing" is offered last (existing indices unchanged).
- Icons: new `BindingIcon::Play` (`play`, rounded triangle) and `BindingIcon::MediaIdle`
  (`media-idle`, eighth note struck through), media family (violet), added to `ALL`
  (53). `Glyph::Pause` reuses the existing `pause` bars. `for_glyph` maps all three.
- Render: Play inks the media violet; Pause and the idle icon are grey (the idle caption
  stays white, like OBS idle). Contact sheet inspected; the play triangle got round
  joins to match pause/stop.
- Tests: `info_render::now_playing_states_render_on_keys_and_sections` (accent per state,
  fit on key and section, long title ellipsized, full "Nothing playing" caption);
  `tests::live_info_tests::now_playing_is_selectable_and_draws_its_three_states_on_keys_and_sections`
  (GTK: offered by label, saved on a key and a touch section, each state redraws both).
- RED/GREEN: icons were added before their mapping test (no separate RED); the render
  sample first failed the existing 18px headline floor for "Blue in Green" (long titles
  shrink to 60% = 14px per the existing fit rule), so samples use short titles and long
  titles are covered by the dedicated ellipsis assertion. GREEN: lib 343/343, display bin 64/64.
- Checks: fmt ok, clippy -D warnings ok, headless suite ok.

### N3
- Route: delegated direct (same writer; docs + screenshot generator).
- Docs: DESIGN.md (live information icon list, the three states and selection, palette),
  docs/index.html (Live information feature + Configuration card), README Features bullet,
  CHANGELOG Unreleased/Added.
- Screenshots: the generator binds touch section 4 to Now playing ("So What" / "Miles
  Davis", play glyph). Changed visually and committed: live-information.png,
  live-information-settings.png. Restored as noise: application-profile-tab.png and
  live-input-feedback.png (dial knob shading, ~90 px), import-configuration.png (temp path).
- Manual check (bounded, `dbus-run-session` + mpv-mpris on a generated silent WAV, via a
  temporary uncommitted test calling `read_players()`): Playing -> play/"Silent Test";
  after MPRIS Pause -> pause, inactive; after mpv exits -> media-idle "Nothing playing".
  mpv registers `org.mpris.MediaPlayer2.mpv.instance-<id>`.
- Checks: fmt ok; clippy -D warnings ok; headless suite ok (lib 343, bin 64 no-display,
  integration 2); display bin 64/64; examples build; git diff --check clean.

### N4
- Route: delegated direct (same writer; info_render, image_output, event_api, main.rs, tests).
- Rule (`info_render::headline_layout`): Now playing titles shrink to at most 75% of the
  headline size (key 24 -> 18px, section 26 -> 19px). A playing title that still overflows
  scrolls at that floor, clipped to its text column; paused titles ellipsize at the floor.
  Other sources keep the old shrink-to-60%-then-ellipsize rule.
- `marquee_offset(text, column, elapsed)`: 1.5 s still, 30 px/s, 28 px gap, seamless loop;
  `marquee_extent(value, section)` measures once; `*_at(.., elapsed)` variants in
  info_render/image_output; phase zero = the static first frame (old entry points).
- Device: `event_api::marquee::Marquees` on the HID owner thread follows every sent command
  (strip and snapshots reset sections/all), sends due frames between input reads (100 ms
  minimum per title, whole-pixel offset must change, at most 2 frames per pass), reports
  only failures, and is per connection. Measured cost: ~11-13 ms render+encode and 5 (key) /
  6 (section) 1024-byte reports per frame, hence the 2-frames-per-pass cap.
- Preview: `InfoState` (derefs to the readings map) holds `PreviewMarquee`; one 100 ms
  `glib::timeout_add_local` runs only while a title scrolls, redraws only moved titles, and
  stops when none scrolls or their picture has no root.
- Tests: marquee_offset, fit decision, frames at two phases differ only inside the text
  rect, ticker start/stop/rate/strip/snapshot, HID sender (dedup, failure stops), GTK
  preview (moves on tick, pause stops the real timer, detached preview stops it).
- RED/GREEN: implementation written before its tests (no RED observed); GREEN lib 349,
  display bin 64/64. Manual: mpv titled "Bohemian Rhapsody (Remastered 2011)" -> extent
  (343, 112), frames at 0 s and 3 s differ; paused/idle static.
- Screenshots: regenerated; only noise differences (short sample title), all restored.

### N5
- Route: delegated direct (same writer; mpris.rs, new src/info/app_icon.rs, info.rs, info_render.rs).
- Read: per player, `GetAll(org.mpris.MediaPlayer2)` gives Identity and DesktopEntry
  (replaces the conditional `Get Identity`); 2 bounded calls per player, NO_AUTO_START, 1 s.
- Resolve (`info::resolve_app_icon`): `<data>/applications/<entry>.desktop` (as is or lower
  case) -> `gio::DesktopAppInfo::from_filename` -> GIcon: FileIcon path, or ThemedIcon names
  (absolute path as is) looked up in the current theme (GNOME `icon-theme` setting via a
  safe schema lookup, else GTK settings.ini), its `Inherits`, then hicolor (scalable SVG >
  PNG by size up to 256 > larger PNG), then `pixmaps`. Data dirs: XDG_DATA_HOME, flatpak
  exports (user + system), XDG_DATA_DIRS. Decoding: gdk-pixbuf (SVG at 128px, bitmaps
  scaled down to 128, never up) into an `Arc` of cairo ARGB pixels; `AppIcon` compares
  by path. `AppIcons` caches per entry, misses included; lookups happen on the info worker.
- Selection: chosen player's DesktopEntry, then Identity (spaces -> `-`) as an entry hint;
  nothing playing never carries an icon. `InfoValue::app_icon` is `None` for other sources.
- Render: icon fills the glyph slot (full color; 60% while paused) with a 0.21-slot-radius
  dark badge at its bottom-right holding `play` (media violet) or `pause` (grey).
- Tests: root GetAll read (mock bus), resolution fixture (theme SVG/PNG ranking, current
  theme over hicolor, `@2` dirs, case, absolute path, pixmaps, misses, path escape), cache
  (decoded once, misses cached), selection with icon, render (pixels in slot, dimmed
  paused, violet/grey badge, fallback to glyph, idle ignores icons).
- RED/GREEN: implementation before tests (no RED observed); two tests first failed and were
  fixed (identity hint was lower-cased before the case-preserving lookup; PNGs were upscaled).
  GREEN lib 354, display bin 64/64.
- Manual (dbus-run-session + mpv-mpris): DesktopEntry `mpv` resolved to
  /usr/share/icons/hicolor/scalable/apps/mpv.svg (themes Yaru-blue, Yaru, hicolor), ~40-55 ms
  first lookup; paused keeps the icon; after exit no icon.
- Screenshots: unchanged (the generator has no deterministic application icon to show).

## Next step
Review the feature branch (RDD unavailable); push/PR are the user's decision.
