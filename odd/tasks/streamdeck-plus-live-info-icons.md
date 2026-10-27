# streamdeck-plus-live-info-icons

## Objective
Bring live-information images (keys and touch-strip sections) up to the binding
icon style, then close the follow-ups left after PR #1.

## Problem
Live-information images (src/info_render.rs) predate the binding icon work:
- touch sections draw on a transparent background (e.g. weather "22°C Clear"),
  unlike the translucent graphite tile now used by binding-icon sections;
- key captions are clipped at the bottom ("CPU", "Memory", "Temperature");
- glyphs (own `draw_glyph` set) differ in stroke/size from `src/binding_icons.rs`.
Follow-ups from PR #1: the base test failure and ugly wrapping of long auto captions.

## Scope / constraints
- Same in preview and device (info images are composed on the worker); never write
  derived images into stored artwork; keep user artwork as backdrop where present.
- Keep the live values, bars and refresh behavior; light/dark parity; no new crates.
- Branch `feat/live-info-icons` from main d2084ac. Delivery: single PR (user preference).

## Tasks
- [x] L1 Touch-section live info uses the same translucent tile/layout language as binding-icon sections (no bare transparent area); chevron room kept.
- [ ] L2 Key live info: captions fully visible (no clipping), icon/value/bar/caption spacing rebalanced.
- [ ] L3 Live info glyphs unified with the binding icon vocabulary (shared stroke/grid; reuse `binding_icons` where an equivalent exists).
- [ ] F1 Fix `tests::touchscreen_tests::touchscreen_page_indicators_follow_selection_artwork_and_live_info` (root cause, not weakening the test).
- [ ] F2 Long auto captions on keys/sections: show URL host only and ellipsize instead of breaking mid-word.
- [ ] D1 DESIGN.md, README.md, CHANGELOG.md, regenerated screenshots.

## Route
Delegated direct (one writer): 2+ non-trivial files. RDD unavailable (`gentle-ai` not installed).

## Checks
cargo fmt --all -- --check; cargo clippy --offline --all-targets --all-features -- -D warnings;
env -u DISPLAY -u WAYLAND_DISPLAY GDK_BACKEND=x11 cargo test --offline;
GDK_BACKEND=x11 cargo test --offline --bin sdeck-plus; cargo build --offline --examples; screenshot regeneration.

## Progress
Created 2026-10-26. Engram mirror pending (engram tools unavailable this session).

### L1 (delegated writer, route: delegated direct)
`info_render::compose_zone_image(value, background, previous, next)` draws the
binding-icon section tile (`binding_icons::draw_zone_tile`, now shared, alpha
0.80; 0.55 over the section's own artwork so it stays visible) and a testable
`zone_plan`: icon 52px on the left (44px when a chevron edge is reserved),
value / bar / detail stacked beside it, 30px kept at indicated outer edges;
chevrons still paint last (`compose_touchscreen_zone` passes zone 1 previous /
zone 4 next). Headlines shrink to fit (max 40%) then ellipsize; detail lines
that do not fit the tile are left out.
RED: `touch_sections_show_live_information_on_the_section_tile`,
`section_parts_stay_on_the_tile_and_out_of_the_chevron_room` (no `zone_plan`,
2-arg signature), then GREEN. `image_output` indicator test now samples the
artwork at y=2 (y=5 is under the tile edge; artwork still visible around it).
Checks: fmt ok; clippy ok; headless `cargo test --offline` ok (319+53+2);
display bin suite 52/53 (only the known F1 failure).
