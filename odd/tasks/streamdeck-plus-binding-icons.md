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
- [x] T6 Docs: DESIGN.md, README, CHANGELOG, regenerated screenshots.
- [x] T7 Touch zone icons: larger, horizontal icon+caption layout using the zone width, background integrated with the strip (no black box); preview and device.
- [x] T8 Touchpad gestures: strip shows icons/indicators for assigned swipe/tap/hold gestures (e.g. swipe -> Next page), including the default swipe paging.
- [x] T9 More actions: specific icons per FeatureAction (Organization, OBS, audio...) in the Category/Action dropdowns, the closed Action button and on keys/zones/dials instead of the generic glyph.
- [x] T10 Touch zone editor (Artwork and touch zones): icons identifying each zone and its action.
- [ ] T11 Docs + regenerated screenshots for T7-T10.

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

### T5 follow-up (delegated writer)
Add/Edit text now uses `format-text-plaintext-symbolic` (a plain "A") instead
of `insert-text-symbolic`, in `button_text_editor.rs` (surface granted).
RED: the extended `icon_actions_share_one_size_and_one_delete_icon` saw
`insert-text-symbolic`, then GREEN. clippy ok.

### T6 (delegated writer)
Screenshot fixture: keys 5-7 have no artwork (OBS toggle recording, website,
media next) and touch sections 2-3 none (OBS scene, mute), so captures show
derived icons. After inspecting main-window(-light).png and action-types.png,
one iteration enlarged key icons 46->50px and section icons 40->52px with an
18px caption (commit style(icons)). DESIGN.md (binding icon vocabulary + rules,
icon action consistency), README.md (Binding icons), CHANGELOG.md, and
docs/index.html (artwork card copy, refreshed image dimensions) updated.
No meaningful RED (documentation). Final checks: fmt ok; clippy ok; headless
`cargo test --offline` ok (314 + 50 + 2); display bin suite 49/50 (only the
known base failure); examples build ok; `git diff --check` ok; screenshots
regenerated with the documented command.

## Next step
Human review of `feat/binding-icons`; push/PR remain the user's decision.
Optional real-hardware check: `cargo run --offline --example verify_streamdeck -- --device-controls`.

### T7 (delegated writer)
`compose_zone_image(icon, caption, previous, next)`: translucent graphite tile
(rounded 12px, 80% alpha gradient like the rail, 16% white edge) so strip
artwork shows through and around it; 58px icon on the left with up to two
17px bold caption lines beside it, vertically centered. Sections with an
edge page indicator reserve 30px at that edge (icon 50px) so the chevron keeps
its room; chevrons still paint last. Same composer for preview and the device
worker (`prepare_icon_jpeg` over the strip crop, or black).
RED: `touch_sections_lay_the_icon_beside_its_caption_on_a_strip_tile` (no tile)
and `edge_page_indicators_keep_their_room_beside_the_section_icon` (signature),
then GREEN. Checks: clippy ok, display bin suite only the known failure.

### T8 (delegated writer)
Placement: the preview strip's header line (beside "Touch strip · waiting for
input", above the sections) holds compact badges in the order swipe left,
swipe right, tap, hold: gesture glyph (new `swipe-left`, `swipe-right`, `tap`,
`hold` icons) + action icon, 11px, pale pill; unbound swipes show their
default touch paging dimmed (`default` class); explicit None and unassigned
tap/hold show nothing. Tooltip/accessible name e.g. "Swipe left (default):
Next touchscreen page". Device: unchanged on purpose; its edge chevrons
already show swipe paging and badges would cover section content.
`binding_editor::strip_gestures`/`StripGesture`/`GestureBinding` (lib).
Three test sibling walks now step out of the new header row.
RED: lib `strip_gestures_list_swipes_tap_and_hold_with_default_paging` and GTK
`strip_header_shows_gesture_badges_without_covering_the_sections` (no
items/field), then GREEN. Checks: clippy ok, display bin suite 50/51 (known).

### T9 (delegated writer)
`Choice::binding_icon` derives each More action's icon from a representative
`FeatureAction` through `BindingIcon::for_action`, so menus, guidance and
saved controls can never disagree. Category rows: System `application`,
Multimedia `play-pause`, Organization `folder`, Device `brightness`, OBS
`obs-scene`. The Category and Action dropdowns use `icon_dropdown` (now
closure-based, `icon_dropdown_with` for a separate closed-button icon); the
guidance icon is a 20px vocabulary icon; the closed main Action button shows
the selected More action's icon (synced through a weak
`FeatureEditor::connect_icon_changed`), menu rows keep `more`. Keys, sections
and dials already used `for_action` (no generic glyph for known actions).
RED: `more_actions_show_specific_icons_in_menus_guidance_and_the_action_button`
(no category icons), then GREEN. Checks: clippy ok, display bin suite only the
known failure.

### T10 (delegated writer)
`binding_icons::paint_target`/`target_marker_id` (lib): a mini map of the eight
keys over the four strip sections, outlined at half ink, with the target
filled (the whole strip row for the background). `binding_icon::target_dropdown`
gives the Artwork target picker that 26x22 marker before every name (closed
button and menu) and, in menu rows, the target's assigned action icon after
the name with its summary as tooltip (keys: press; sections: tap, then hold;
strip background: none). The action icon is read again each time a row maps,
so saved bindings appear when the menu reopens. Check mark kept.
RED: `artwork_target_menu_marks_each_control_and_its_action` (no rows) and lib
`target_markers_fill_exactly_their_control_on_a_mini_map` (missing fns), then
GREEN; marker enlarged once after inspecting the capture. Checks: clippy ok,
display bin suite 52/53 (only the known failure).
