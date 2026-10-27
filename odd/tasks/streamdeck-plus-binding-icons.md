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
- [x] T2 Key icons: keys without artwork render icon + label (preview and device).
- [x] T3 Action type menu shows an icon per action type.
- [x] T4 Preview dials and touch zones show the action icon next to/with the caption.
- [x] T5 UI button icons reviewed and unified (inspector, page bar, header).
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

### T4a dials (delegated writer)
`binding_editor::dial_action` (same turn > ticks > press order as `dial_legend`,
which now reuses it). Each preview knob shows that action's icon (20px,
`dial-icon`, pale `#e1e6eb`, not targetable) centered on its face above the
unchanged caption; hidden when unassigned or None; preview-only.
RED: `binding_icon_tests::preview_dials_show_their_action_icon_with_the_caption`
(no `dial_icons`), then GREEN. Touch-zone half pending with T2 rendering.

### T2/T4b preview (delegated writer)
Lib: `binding_icons::compose_key_image` (120x120: icon 46px at y40 + 14px bold
2-line caption at y72 over black; with user text the icon takes the free band:
Bottom text -> icon above, Top text -> icon below; Center/vertical text -> None,
key unchanged) and `compose_zone_image` (200x100 transparent: icon 40px +
one-line 15px caption). `binding_editor::action_caption` reuses the summary text.
Window: `ArtworkUi::control_icon` derives the icon at render time for keys
(KeyPress) and touch sections (ZoneTap, then ZonePress) only without own
artwork and without live information; None/unassigned stay empty. Page
indicators still paint over the icon. Stores are never written.
RED: lib tests (no compose fns) and
`binding_icon_tests::keys_and_touch_sections_without_artwork_show_their_action_icon`
(key 2 picture hidden), then GREEN (artwork tree byte-identical after refresh).
Device output pending: needs `src/event_api.rs` + `src/image_output.rs`
(outside the authorized edit surface).

### T5 (delegated writer)
Every delete/remove icon action (Remove binding, Delete page, Delete folder,
Delete touchscreen page, Delete profile) now uses `user-trash-symbolic` (was
`edit-delete-symbolic`, which Adwaita draws as a circled cross, same as Clear);
Organize uses `preferences-system-symbolic` instead of the misleading
`folder-new-symbolic`. Save/Test/Folders/New page/Rename/OBS/Menu already
matched (16px symbolic, tooltip, accessible name) and are unchanged.
Not changed: Edit text's `insert-text-symbolic` lives in
`button_text_editor.rs`, outside the authorized edit surface.
RED: `binding_icon_tests::icon_actions_share_one_size_and_one_delete_icon`
(profile delete used edit-delete), then GREEN. Checks: fmt ok, clippy ok,
display bin suite 48/49 (only the known base failure).

### T2/T4 device (delegated writer, surfaces extended by the user)
`ArtworkCommand::ComposeIcon { target, icon, caption, text, strip_path,
previous, next }` (event_api.rs) and `image_output::prepare_icon_jpeg`: keys use
`compose_key_image` (falls back to the plain text picture), sections compose
icon + page indicators over the strip crop (black without a strip), all in
memory on the HID worker. `ArtworkUi::with_icons` (main.rs) substitutes these
commands exactly where the preview draws an icon (`control_icon` +
`binding_icons::key_has_room`); an indicated section's icon command carries
its indicators instead of a separate `ComposeTouchscreenZone`.
RED: `binding_icon_tests::device_snapshot_draws_binding_icons_only_where_the_preview_does`
failed to compile (no `ComposeIcon`/`prepare_icon_jpeg`), then GREEN.
Checks: fmt ok, clippy ok, lib 314/314, display bin suite 49/50 (only the known
base failure, same panic as base).
