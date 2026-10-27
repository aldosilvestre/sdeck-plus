# streamdeck-plus-binding-icons

## Objective
Give bindings recognizable icons: on keys without user artwork (preview and
physical device), in the Action type menu, on preview dials/touch zones, and
consistently across inspector/toolbar buttons.

## Problem
Keys without artwork show only small text; the Action dropdown is text-only;
dials/touch zones show no action cue; button icons are not reviewed as a set.

## Scope / constraints
- Derived icons are computed at render time; never written into stored artwork,
  never replace user artwork or button text, never change hardware geometry.
- Same icon vocabulary in preview and physical output; light/dark parity.
- No new crate dependencies unless unavoidable (gtk/cairo/pango available).
- Base: branch `feat/binding-icons` from `feat/ui-refresh` (152ba0d+).
- Delivery strategy: ask-on-risk.

## Tasks
- [x] T1 Icon vocabulary: one action -> icon mapping (Action, DesktopAction, FeatureAction incl. OBS) with a fallback.
- [ ] T2 Key icons: keys without artwork render icon + label (preview and device).
- [x] T3 Action type menu shows an icon per action type.
- [ ] T4 Preview dials and touch zones show the action icon next to/with the caption.
- [ ] T5 UI button icons reviewed and unified (inspector, page bar, header).
- [ ] T6 Docs: DESIGN.md, README, CHANGELOG, regenerated screenshots.

## Route
Delegated direct (one writer): 2+ non-trivial files. RDD unavailable (`gentle-ai` not installed).

## Checks
cargo fmt --all -- --check; cargo clippy --offline --all-targets --all-features -- -D warnings;
env -u DISPLAY -u WAYLAND_DISPLAY GDK_BACKEND=x11 cargo test --offline;
GDK_BACKEND=x11 cargo test --offline --bin sdeck-plus; screenshot regeneration.
Known base failure: tests::touchscreen_tests::touchscreen_page_indicators_follow_selection_artwork_and_live_info.

## Progress
Created 2026-10-26. Engram mirror pending (engram tools unavailable this session).

### T1 (delegated writer)
Approach: cairo vector glyphs in a new lib module `src/binding_icons.rs`
(`BindingIcon`, `for_action`, `for_action_type`, `id`, `paint`, `paint_outlined`):
24-unit grid, 2-unit round strokes, monochrome ink chosen by the caller. No new
crates, no icon theme or SVG decoder, deterministic and headless, so the same
picture serves the device JPEGs and GTK widgets. 37 icons incl. `generic`
fallback and `none` (menu only, never drawn on controls).
RED: `binding_icons::tests` failed to compile (no `BindingIcon`/`paint`); GREEN 4/4.
Contact sheet inspected at 48px and 16px.

### T3 (delegated writer)
`src/ui/binding_icon.rs` (bin-only, mounted like clamp/scaled_bin because
examples include `ui.rs` directly): `binding_icon` DrawingArea (16px, theme
foreground via the style context, presentation role, `binding-icon` class,
widget name = icon id) and `icon_dropdown` factories. The Action dropdown shows
the icon in the closed button and in each menu row; menu rows replicate GTK's
`object-select-symbolic` check mark on the selected row. Labels unchanged.
RED: `binding_icon_tests::action_menu_rows_show_an_icon_name_and_check_mark`
(no icon widgets), then GREEN. Checks: fmt ok, clippy ok, display bin suite
45/46 (only the known base failure).
