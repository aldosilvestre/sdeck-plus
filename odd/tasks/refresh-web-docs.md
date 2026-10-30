# Refresh the website and its screenshots
Regenerate docs/screenshots/*.png from the native generator and update docs/index.html and docs/docs.html for the new editor.
Generated PNGs must come from the generator (never hand-edited); width/height/crop attributes must match the new images.

## Specs
- S1: "actualicemos las capturas de la pagina web y la pagina web"

## Tasks
- [x] T1 (S1) route: inline — rework the generator (`src/tests/documentation_screenshots.rs`) for the new UI: dial-based dropdown captures, new captures (Dials/Binding tabs, key menu, dial event choice, missing file, hover help, shortcut table). Commit: none (see Log).
- [x] T2 (S1) route: inline — regenerate into docs/screenshots and review every image. Commit: none.
- [x] T3 (S1) route: inline — update docs/index.html (features, captions, image sizes/crops). Commit: none.
- [x] T4 (S1) route: inline — update docs/docs.html (steps, captions, new sections). Commit: none.
- [x] T5 (S1) route: inline — verify: image attributes match files, no broken links, full test suite. Commit: none.

## Log
- L1: "perfecto, ahora actualicemos las capturas de la pagina web y la pagina web"
- L2 (agent note): no commits, same reason as symlinked-assets: the working tree holds earlier uncommitted work the user has not asked to commit.
- L3: Generator passed (33 native captures); all reviewed in target/doc-sheet.jpg. Popover autohide must not change after opening (it recreates the native popup).
- L4: T3/T4 complete: landing copy and reference docs match the new layout; seven new captures used, PNG dimensions refreshed and stale crops removed. T5 verification underway.
- L5: User continuation: "continua". All tasks complete: 33 native captures, local links/anchors and 37 image dimensions verified. Chromium desktop/mobile (1440/390): both pages, no broken images or horizontal overflow; landing lightbox opens with decoded image. Rust: 371 lib, 97 GTK, 2 integration; strict Clippy, fmt, diff check passed. Native review unavailable: gentle-ai CLI not installed. No commit/publish.
