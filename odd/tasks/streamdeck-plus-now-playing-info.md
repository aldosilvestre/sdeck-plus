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

## Next step
Review the feature branch (RDD unavailable); push/PR are the user's decision.
