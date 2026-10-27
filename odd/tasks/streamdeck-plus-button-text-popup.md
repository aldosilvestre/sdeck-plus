# streamdeck-plus-button-text-popup

## Objective
Improve the "Button text" popover used to add and edit text on keypad keys.

## Problem
The popover (src/button_text_editor.rs, docs/screenshots/button-text-editor.png)
works but: it does not say which key/page it edits; the preview draws the text on
a fixed blue square instead of the key's real background; horizontal and
vertical alignment take two rows of three toggles; Save is always enabled (even
with no changes or empty text) and Remove shows even when the key has no saved
text; there is no warning when the text does not fit the key at the chosen size;
there is no keyboard shortcut to save.

## Scope / constraints
- No ButtonText model/storage change (no new fields such as color).
- Keep the DESIGN.md contract: anchored popup, two-column body, full 120px
  preview, no popup scrolling at 640x480 or larger (existing fit tests), native
  font chooser, 6-120px spinner; saving never executes the binding.
- Light/dark parity, accessible names, no blocking work in callbacks, no new crates.
- Branch `feat/live-info-icons` (single PR, user preference).

## Tasks
- [x] T1 Context and state: header context "Key N on “<page>”"; Save insensitive when unchanged or invalid; Remove only when the key has saved text; Ctrl+Enter saves; unsaved-changes hint; GTK tests.
- [ ] T2 Layout: single 3x3 "Position" grid replacing the two alignment rows; orientation as compact icon toggles; preview drawn on the key's real background (artwork or binding icon tile) with an overflow warning when the text does not fit; fit tests still pass. GTK tests.
- [ ] T3 DESIGN.md, README.md, CHANGELOG.md, regenerated button-text-editor screenshot.

## Route
Delegated direct (one writer): 2+ non-trivial files. RDD unavailable (`gentle-ai` not installed).

## Checks
cargo fmt --all -- --check; cargo clippy --offline --all-targets --all-features -- -D warnings;
env -u DISPLAY -u WAYLAND_DISPLAY GDK_BACKEND=x11 cargo test --offline;
GDK_BACKEND=x11 cargo test --offline --bin sdeck-plus; screenshot regeneration.

## Progress
Created 2026-10-27. Engram mirror pending (engram tools unavailable this session).

### T1
Route: delegated direct (writer). src/button_text_editor.rs: muted context line
under the header ("Key N on “<view>”"; the view is the page indicator's
tooltip, i.e. the organization label `profile / folders / page`, falling back
to the active page key), followed live through `follow_page_label` wired in
main.rs `build_widgets`. Save is sensitive only for a valid, nonblank value that
differs from the saved text (its tooltip says why otherwise); Remove is shown
only when the key has saved text; a muted "Unsaved changes" hint in the button
row tracks the form against the saved value (default form when none). A
capture-phase ShortcutController on the popover maps
`<Control>Return|<Control>KP_Enter` to Save; plain Enter still inserts a line.
`persist` refuses insensitive Save / hidden Remove even when the signal is
emitted. `assert_text_popup_fits` now checks Save/Remove for clipping only
(Save may be insensitive, Remove hidden) and also checks the context line.
RED: new src/tests/button_text_popup_tests.rs (3 GTK tests) failed to compile
(no `context_line`, `unsaved`, `save_shortcut`). GREEN: `cargo test --bin
sdeck-plus button_text` 4 passed; QA fit path (`STREAMDECK_TEXT_QA_DIR`) passed.
Checks: fmt ok; clippy -D warnings ok; headless 333 lib + 60 bin + 2 ok; display
bin suite 60/60; git diff --check ok. Commit: see git log
(`feat(button-text): show context and saved state in the text popup`).
Engram mirror pending.

## Next step
T2.
