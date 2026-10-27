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
- [x] F2 Long auto captions on keys/sections: show URL host only and ellipsize instead of breaking mid-word.
- [x] D1 DESIGN.md, README.md, CHANGELOG.md, regenerated screenshots.
- [x] L5 Touch-section tiles clearly see-through (~0.15 fill over the strip, ~0.10 over section artwork, soft vertical fade, subtle edge); legibility from the icon/text soft shadow, chevrons still last.
- [x] L6 Better icons everywhere: duotone (soft accent fill ~35% under a ~1.85-unit round stroke), soft drop shadow instead of the hard outline on hardware, per-category accents on hardware images, redrawn weak glyphs; contact sheet inspected.

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
Commit 5cad887.

### F2 (delegated writer, route: delegated direct)
`binding_editor::action_caption` shows only the host for Website actions
(`url_host`: http/https, drops user info, port, path, query, fragment; IPv6
brackets kept; anything else keeps the summary). `action_summary` (inspector,
tooltips) is unchanged; user button text is never touched (captions are only
derived for keys/sections without artwork). `binding_icons::caption_lines`
wraps automatic captions between whole words (as many as fit per line, the
rest on the last line) and Pango ellipsizes a line that is still too wide,
instead of `WordChar` breaking mid-word. Keys and sections, preview and device
share `compose_key_image`/`compose_zone_image`, so all four paths change.
RED: `website_captions_show_only_the_url_host`,
`long_captions_wrap_between_words_and_ellipsize_instead_of_breaking_words`
(missing `url_host`/`caption_lines`), then GREEN.
Checks: clippy ok; headless ok (323+53+2); display bin suite 53/53.
Commit f398bf5.

### D1 (delegated writer, route: delegated direct)
DESIGN.md (caption wrapping/host rule, live information icons and key/section
layout), README.md (live information on keys and sections), CHANGELOG.md.
Screenshot sizes unchanged (1000x950), so docs/index.html needs no change.
Screenshots regenerated with the documented command (12 changed); inspected
live-information, touch-strip, main-window, main-window-light. No meaningful
RED (documentation).
Final checks: fmt ok; clippy ok; headless `cargo test --offline` ok
(323+53+2); display bin suite 53/53; examples build ok; `git diff --check` ok.
Commit 6656dcc; docs (DESIGN/README/CHANGELOG, 15 regenerated screenshots) in
the following `docs:` commit. Engram mirror still pending (not available to
the writer).

## Next step
Human review of `feat/live-info-icons`; push/PR remain the user's decision.
Optional real-hardware check: `cargo run --offline --example verify_streamdeck -- --device-controls`.

### L4 (inline, user feedback: "placa más transparente")
Section tile opacity lowered and shared as `binding_icons::ZONE_TILE_ALPHA`
(0.35, was 0.80) and `ZONE_TILE_ARTWORK_ALPHA` (0.25 over section artwork, was
0.55) for binding-icon and live-information sections; icons keep their dark
outline for legibility. Pure visual tweak, no meaningful RED; existing tile
test (translucent, not transparent, not black) still holds.
Checks: fmt ok; clippy ok; lib 323 ok; display bin suite 53/53; screenshots
regenerated. Note: the installed ~/.local/bin/sdeck-plus predates all UI work
(17:58); reinstall with ./install.sh to see changes outside `cargo run`.

### L5 (delegated writer, route: delegated direct; user feedback "more transparent")
`ZONE_TILE_ALPHA` 0.35 -> 0.15 and `ZONE_TILE_ARTWORK_ALPHA` 0.25 -> 0.10 at
the tile top, fading to half towards the bottom; the light edge fades from
0.12 to 0.04; corner radius 14. Shared by binding-icon and live-information
sections; chevrons still paint last (unchanged order).
RED: `section_tiles_are_clearly_see_through` (alpha 89 at y=12, expected
<=46), then GREEN. The two existing tile tests keep "translucent, not black"
with bounds moved to the new intended values: alpha 8..60 at the tile bottom
(was 60..255).
Checks: fmt ok; clippy ok; lib 324 ok; display bin suite 53/53.
Commit 6aa1067.

### L6 (delegated writer, route: delegated direct; user feedback "better icons")
`binding_icons::paint` draws every glyph as a duotone through a `Pen`: soft
fill at `SOFT_ALPHA` (0.35) of the ink under a crisp 1.85-unit round stroke
(2.2-2.6 for chevrons, transport bars, the text T), grouped so a translucent
ink fades evenly. `paint_outlined` (name kept for API stability) now draws a
soft drop shadow (`with_soft_shadow`: 3 rings x 12 stamps at 28/15/8% black,
0.6-2.3 units out, dropped 0.6 units) instead of the hard 80% outline;
captions (`draw_caption`) and live-information text (`info_render::draw_text`)
use the same shadow. `BindingIcon::category()`/`accent()` and `IconCategory`
color device icons by family: navigation white, media violet, audio cyan, OBS
red, system amber, web blue, weather blue-grey, load green; live information
keeps its own accents. Preview dials (`.dial-icon`) use the family accent;
menus, gesture badges and the Action menu keep the theme foreground (light /
dark parity); the artwork-target mini map gains a soft cell fill.
Redrawn: toggle (switch), play sound (beamed notes), folder (front edge),
parent folder (arrow inside folder), open file (text lines), application (3
tiles + circle), tap (burst), hold (progress ring), snow (flakes), sleep (true
crescent path + star), next/previous page (chevron on soft disc), more (soft
pill), generic (sparkle, was identical to hold).
Contact sheet: `STREAMDECK_ICON_SHEET=<png> cargo test --offline --lib
icon_contact_sheet` (lib test, opt-in, writes nothing when unset; also writes
`-partN.png` crops). Inspected twice; iteration 2 fixed hold/generic
confusion, wifi-like tap, dot-like snow and weak caption contrast on light
artwork (stronger shadow).
RED: `every_icon_is_a_duotone_of_a_soft_fill_under_a_crisp_stroke` (old
renderer: "go-to-page: soft layer 10") and
`hardware_icons_sit_on_a_soft_drop_shadow_not_a_hard_outline` (old: "246 soft,
619 dark") observed failing against the pre-L6 renderer after implementation
(tests written alongside, not strictly first); `hardware_accents_follow_the_icon_family`
was RED by construction (missing API). Pure shape changes have no RED.
Test adjustments: key/section pixel helpers (`bright`, `bright_in`,
`bright_columns`, GTK `bright_pixels`) count lit pixels (any channel > 180)
instead of near-white, since icons are now accent-colored.
Size: ~750 changed lines in `binding_icons.rs`, above the 400-line heuristic
because every glyph is redrawn; not split.
Checks: fmt ok; clippy ok; headless `cargo test --offline` ok (328+53+2);
display bin suite 53/53; examples build ok; `git diff --check` ok.
Commit 6656dcc; docs (DESIGN/README/CHANGELOG, 15 regenerated screenshots) in
the following `docs:` commit. Engram mirror still pending (not available to
the writer).
