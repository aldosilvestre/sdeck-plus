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
- [x] T2 Layout: single 3x3 "Position" grid replacing the two alignment rows; orientation as compact icon toggles; preview drawn on the key's real background (artwork or binding icon tile) with an overflow warning when the text does not fit; fit tests still pass. GTK tests.
- [x] T3 DESIGN.md, README.md, CHANGELOG.md, regenerated button-text-editor screenshot.

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

### T2
Route: delegated direct (writer). src/button_text_editor.rs: the two alignment
rows are replaced by one 3x3 "Position" grid (`position: [ToggleButton; 9]`,
row-major, grouped, 14px key-outline glyph with a dot, tooltips/accessible
names Top left .. Bottom right; `position_index`/`position_alignment` map
HorizontalAlignment x VerticalAlignment). Orientation is a linked pair of
icon toggles drawing upright/rotated "Ab" (accessible names Horizontal /
Vertical). Both sit in one row. The preview uses the new
`ArtworkUi::key_text_image` (main.rs): the key's artwork (artwork owner), else
its binding icon tile via `ControlIcon::compose` when the text leaves room,
else the neutral black key, which is the same choice `set_button_picture`
makes; nothing is stored. `text_fits` measures Pango ink extents exactly like
`compose_button_image` (116px inside 2px margins; the key is square so
rotation does not change fit) and shows "Text does not fit; try a smaller
size" under the preview; Save stays allowed. The "Unsaved changes" hint moved
under the preview: in the button row it widened the popup while it was being
mapped and GTK/X11 closed it (found by the `STREAMDECK_TEXT_QA_DIR` fit path).
CSS in src/ui.css uses existing tokens (light parity through ui-light.css
token overrides; no light-specific rule needed).
RED: the preview-background test failed against the old preview composition
(key 2 drew a black key instead of its icon tile); grid/overflow tests failed
to compile before `position`/`overflow`/`text_fits` existed. GREEN: `cargo
test --bin sdeck-plus button_text` 7 passed, also with `STREAMDECK_TEXT_QA_DIR`
(640x480/1000x700/1280x800 fit, light+dark).
Checks: fmt ok; clippy -D warnings ok; headless 333 lib + 63 bin + 2 ok; display
bin suite 63/63; git diff --check ok. Commit: see git log
(`feat(button-text): position grid, icon orientation and real key preview`).
Engram mirror pending.

### T3
Route: delegated direct (writer). Layout polish commit first
(`style(button-text): align the text popup fields in one grid`): Font/Size and
Orientation/Position share one gtk::Grid so Position sits under Size; the
context line gets 4px before the form. Then DESIGN.md (button text contract:
context line, orientation pair + Position grid, real key background, overflow
warning, Save/Remove/unsaved/Ctrl+Enter rules), README.md (overview blurb and
Button text section), CHANGELOG.md (Changed). The documentation generator now
opens the popup on an edited draft ("Play\nNow", Bottom center) so Save is
enabled and the unsaved hint shows. Regenerated docs/screenshots; only
button-text-editor.png (462x490) changed visually (AE ~23k); the other five
regenerated PNGs differed by ~90-600 pixels of noise (temp-path pid in
import-configuration) and were restored.
Test-first exception: passive docs/screenshots; checked by reading the PNG and
the light/dark QA captures. Checks: fmt ok; clippy -D warnings ok; headless
333 lib + 63 bin + 2 ok; display bin suite 63/63; `STREAMDECK_TEXT_QA_DIR`
fit path ok; git diff --check ok. Commit: see git log (`docs: describe the
button text popup ...`). Engram mirror pending.

## Next step
Human review; push/PR remain the user's decision.
