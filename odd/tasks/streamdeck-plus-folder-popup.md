# streamdeck-plus-folder-popup

## Objective
Improve the "Page folders" popover used to create and modify page folders.

## Problem
The popover (src/feature_runtime/page_controls.rs, docs/screenshots/page-folders.png)
is a form: a labeled entry plus a full-width "New folder" button, a separator, a
dropdown to pick a folder and three captioned buttons acting on the dropdown
selection. Folders cannot be renamed; the selection is hidden in a dropdown;
there is no empty state and no inline validation (errors only reach the status
line after clicking).

## Scope / constraints
- Folders still belong to the selected page; deleting still confirms; linking
  still saves navigation on the selected control without executing it.
- Rename keeps layer keys; it updates the folder's views, its descendants'
  folder paths and the owner page's OpenFolder bindings. Errors leave both stores
  unchanged; paired saves go through the existing organization change path.
- Light/dark parity, keyboard + accessible names, no new crates, no blocking work
  in GTK callbacks.
- Branch `feat/live-info-icons` (single PR, user preference).

## Tasks
- [x] P1 `Organization::rename_folder(layers, old, new)` with lib tests (rename, descendants, links follow, invalid/duplicate/unknown rejected, unchanged on error).
- [x] P2 Popover redesign: inline create row (entry + add button, Enter creates, live validation message, button disabled when blank/invalid/duplicate); folder list (boxed rows: icon, name, Open/Link/Rename/Delete flat icon buttons with tooltips and accessible names, row activation opens); inline rename (Enter saves, Escape cancels); empty state. GTK tests.
- [x] P3 DESIGN.md, README.md, CHANGELOG.md, regenerated screenshots (page-folders).

## Route
Delegated direct (one writer): 2+ non-trivial files. RDD unavailable (`gentle-ai` not installed).

## Checks
cargo fmt --all -- --check; cargo clippy --offline --all-targets --all-features -- -D warnings;
env -u DISPLAY -u WAYLAND_DISPLAY GDK_BACKEND=x11 cargo test --offline;
GDK_BACKEND=x11 cargo test --offline --bin sdeck-plus; screenshot regeneration.

## Progress
Created 2026-10-27. Engram mirror pending (engram tools unavailable this session).

### P1
Route: delegated direct (writer). `Organization::rename_folder` in
src/organization/ownership.rs; tests in src/organization/folder_rename_tests.rs.
Same-name rename is a documented no-op `Ok`. Only the owner page's own layer
bindings are relinked (mirrors `remove_folder`).
RED: `cargo test --offline --lib organization::folder_rename` failed to compile
(no method `rename_folder`). GREEN: `cargo test --offline --lib organization::`
50 passed. Checks: cargo fmt, clippy -D warnings clean. Commit: see git log
(`feat(organization): rename page folders`). Engram mirror pending.

### P2
Route: delegated direct (writer). New src/feature_runtime/folder_panel.rs
(create row, boxed ListBox rows, inline rename, empty state) routed through
`FeatureRuntime::folder_command` in folders.rs (`change_organization` for
create/rename, `confirm_removal` for delete, `navigate`, `link_folder`). The
DropDown (`folder_picker`, `folder_model`, `selected_folder`) is gone. Creating
now keeps the owning page selected and the popover open (was: entered the
folder and popped down); the main.rs feature test opens the new folder through
its row instead. CSS in src/ui.css uses existing tokens only (light parity via
ui-light.css token overrides). binding_icon_tests now lists one folder so the
per-row "Delete folder" icon action is still asserted.
RED: new tests in src/tests/folder_panel_tests.rs run against the stashed old
popover: 3 failed (missing rows / row controls / empty state). GREEN: 3 GTK
tests + 1 name-check unit test pass; full `GDK_BACKEND=x11 cargo test --offline
--bin sdeck-plus` 57 passed. fmt/clippy clean. Commit: see git log
(`feat(folders): redesign the page folders popover`). Engram mirror pending.

### P3
Route: delegated direct (writer). DESIGN.md (Page folders popover contract),
README.md (create, open, link, rename, delete), CHANGELOG.md (Changed + Added).
The documentation fixture now gives the Base page three folders (Studio tools,
Audio mix, Lighting); regenerated docs/screenshots (page-folders.png 458x437;
other changed PNGs differ only in the "Folders 3" page-bar count).
Test-first exception: passive docs/screenshots; checked by reading the PNG.
Checks: cargo fmt --check ok; clippy -D warnings ok; headless
`cargo test --offline` 333 lib + 57 bin + 2 ok; display bin suite 57 passed;
git diff --check clean. Commit: see git log (`docs: describe the page folders
popover ...`). Engram mirror pending.

## Next step
Human review; push/PR remain the user's decision.
