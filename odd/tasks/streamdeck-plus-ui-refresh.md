# streamdeck-plus-ui-refresh

## Objective
Improve the main window's hierarchy, use of space and legibility without
changing the palette tokens, hardware geometry or existing functions.

## Problem
The main window (docs/screenshots/main-window.png) leaves about a third of the
window empty, gives nine toolbar buttons equal weight, shows technical
VID/PID text as the main status, renders tiny key labels, a flat touch strip
and generic dials, and lays out the binding inspector unevenly (ragged
modifier grid, loose option checkboxes, low-visibility status line).

## Scope / constraints
- Keep `@desk_*` tokens, light/dark parity, hardware coordinates and artwork.
- Respect DESIGN.md and odd/tasks/streamdeck-plus-unified-no-scroll-layout.md.
- No blocking work in GTK callbacks; accessible names and tooltips preserved.
- Branch: `feat/ui-refresh`. Base snapshot of prior WIP: 7afaa76.
- Delivery strategy: ask-on-risk. Forecast: ~500-700 authored changed lines.

## Tasks
- [x] T1 Layout uses available width/height: preview + inspector scale, no dead band.
- [ ] T2 Toolbar hierarchy: navigation vs management groups; destructive/secondary actions lighter.
- [ ] T3 Header status: plain-language connection state, visible badge, VID/PID moved to tooltip.
- [ ] T4 Device preview legibility: larger key labels, clearer touch strip, dials show assigned action.
- [ ] T5 Inspector: aligned modifier toggles, grouped options, visible status next to actions, Appearance placement.
- [ ] T6 Docs: DESIGN.md, README if affected, CHANGELOG, regenerated screenshots.

## Route
Delegated direct (one writer): 2+ non-trivial files (src/ui.css, src/main.rs,
src/binding_editor.rs, src/feature_runtime/*). RDD unavailable: `gentle-ai` not installed.

## Checks
cargo fmt --all -- --check; cargo clippy --offline --all-targets --all-features -- -D warnings;
env -u DISPLAY -u WAYLAND_DISPLAY GDK_BACKEND=x11 cargo test --offline;
GDK_BACKEND=x11 cargo test --offline --bin sdeck-plus (display); screenshot regeneration.

## Progress
Created 2026-10-26. Engram mirror pending (engram tools unavailable this session).

### Known base failure
`tests::touchscreen_tests::touchscreen_page_indicators_follow_selection_artwork_and_live_info`
fails deterministically on base 7afaa76 (touchscreen.rs:177, indicator commands
empty) before any UI-refresh change; it is outside this feature's scope.

### T1 (delegated writer)
Shell fills the window up to 1240px (new `Clamp`, src/ui/clamp.rs) instead of
centering at its ~860px natural width; preview and inspector share spare width.
The preview card is presented through `ScaledBin` (src/ui/scaled_bin.rs), a
uniform 1.0-1.5x transform: hardware coordinates, artwork targets and minimum
sizes are unchanged, so the 1000x700 no-scroll contract still holds (minimum
height-for-width 658). Geometry tests now assert proportional geometry, full-width
shell, uniform scale and >=1.25x at 1280+. RED not observed first (layout asserts
were updated with the implementation). Checks: fmt ok, clippy ok, headless suite
ok, display bin suite 40/41 (only the known base failure).
