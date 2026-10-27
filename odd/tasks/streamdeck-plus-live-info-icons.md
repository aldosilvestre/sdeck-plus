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
- [x] L2 Key live info: captions fully visible (no clipping), icon/value/bar/caption spacing rebalanced.
- [x] L3 Live info glyphs unified with the binding icon vocabulary (shared stroke/grid; reuse `binding_icons` where an equivalent exists).
- [x] F1 Fix `tests::touchscreen_tests::touchscreen_page_indicators_follow_selection_artwork_and_live_info` (root cause, not weakening the test).
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
display bin suite 52/53 (only the known F1 failure). Commit 8093ae8.

### L2 (delegated writer, route: delegated direct)
`key_plan`: icon (32-44px, shrinks first), value (24px bold, fitted), bar
(8px) and detail (14px, lines that do not fit are left out) stacked and
centered inside 4..116px; clock (no icon) uses 36/16px. Shared
`text_column`/`draw_parts` with the section plan; `fit()` removed.
RED: `key_parts_are_fully_visible_and_readable_at_preview_size` (no
`key_plan`), then GREEN. Older pixel tests now read bar/icon positions from
the plan instead of fixed coordinates. Contact sheet inspected: CPU, Memory,
Temperature, Volume captions fully visible.
Checks: fmt ok; clippy ok; headless ok (320+53+2); display bin suite 52/53
(only the known F1 failure). Commit 9085f1a.

### L3 (delegated writer, route: delegated direct)
`BindingIcon::for_glyph` maps every live glyph into the binding vocabulary:
reused Brightness (sun), Mute (muted speaker), AudioOutput (headphones),
Record, Stream (live), ObsScene (OBS idle); new vocabulary icons Volume,
Cloud, PartlyCloudy, Rain, Snow, Storm, Fog, Cpu, Memory, Temperature (24-unit
grid, 2-unit round stroke, in `ALL`, so the unique-id/visible-ink test covers
them). `info_render` paints them with `paint_outlined` in the info accent
color (grey when inactive; a muted speaker keeps a red cross via a clipped
second pass) and its own `draw_glyph` set is removed. Clock stays text-only
(no glyph by design: the time is the picture).
RED: `live_information_glyphs_come_from_the_binding_vocabulary` (no variants /
`for_glyph`), then GREEN. Contact sheet inspected at key and section size.
Checks: fmt ok; clippy ok; headless ok (321+53+2); display bin suite 52/53
(only the known F1 failure).
Commit 21bb6e5.

### F1 (delegated writer, route: delegated direct)
Root cause: the test found "New touchscreen page" with
`touchscreen_controls().last_child()`. That was correct when the test was
written (f4327f8: Touch page label, picker, previous, next, new); commit
7a1a324 added the Delete touchscreen page button after New, as DESIGN.md
documents ("add and delete buttons"). The test therefore clicked Delete (on the
only page: disabled / confirmation), no pages were created, and the snapshot
had no indicators. Product behavior matches DESIGN.md, so only the stale
locator changed: the button is found by its tooltip/accessible name. No
assertion was removed or relaxed.
RED: failed at touchscreen_tests.rs:177 (`[]` vs `[(4, false, true)]`); GREEN.
Checks: clippy ok; display bin suite 53/53.
