# Impeccable audit fixes (UI and design)

Apply the findings of the impeccable design audit of the GTK app (score 15/20), saved at
`.impeccable/audit/2026-11-14T18-34-08Z__project.md`. Branch `feat/popups-buttons-design`.
GTK tests run only through an off-screen wrapper (`/tmp/sdeck-xvfb.sh`), never on the user's display.

## Specs

- S1: User request: "mejora la ui y el diseño." Scope is the audit's findings below (S2-S10); nothing else is redesigned.
- S2 (audit P1): "Tone text on its own tint fails 4.5:1 in the light theme". Light: warning 3.85, success 4.04, error 4.54 over `desk_surface`; 3.56, 3.70, 4.20 over `desk_bg`. Dark: error on a surface-backed tint 4.43. Locations: `src/ui.css` `.status*`, `.rail-badge`, `label.feedback-*`; tokens in `src/ui-light.css`. Done when warning, success and error text on their own tint reach at least 4.5:1 over `desk_surface` and `desk_bg` in both themes.
- S3 (audit P2): "Sub-24px target on the field help button" (`src/ui.css:217`, about 24x20). Done when it is at least 24x24.
- S4 (audit P2): "Off-token literals": dark primary hover/active, the selection ring `#7c3aed` and its gap `#f0f0f2`, the live-input colours. Done when those come from `desk_*` tokens defined in both theme files; hardware-preview literals stay as they are.
- S5 (audit P2): "Destructive command-row hover in dark theme" (`src/ui.css:569-572`): error on `desk_hover` is 4.35:1. Done when it reaches 4.5:1, using `alpha(@desk_error, 0.12)` as the other destructive rows do.
- S6 (audit P3-1): "DESIGN.md contradicts itself": section 7 says "no … accent borders" while the callouts and the profile note use a left edge. Done when DESIGN.md states what the code does.
- S7 (audit P3-2): "Light shadow for form/help popovers (unverified)": `src/ui.css:560,566` use `alpha(black, 0.45)`; `src/ui-light.css:19` only overrides `popover > contents, tooltip`. Verify in a light render; fix only if the dark shadow shows in the light theme.
- S8 (audit P3-3): "Small-type cluster": three `0.9em` sizes beside the 11/12/13px steps. Done when the `0.9em` uses take an existing px step and `design/system.md` lists the type scale.
- S9 (audit P3-5): "Scrollbar against dropdown edge": in `touch-strip.png` the Category/Action dropdowns reach the overlay scrollbar. Done when they stay clear of it.
- S10 (audit, unverified): the screenshots show no outer shell margin while DESIGN.md says "Shell margins: 16px". Verify in a render; fix only if the running app lacks the margin.

Left for the user (not implemented): audit P2 "Binding status shown in the catalog card, not the editor" (a layout decision), and audit P3-4 "Drop target and focus share cyan" (DESIGN.md documents the cyan target outline as intended).

## Tasks

- [x] T1 (S2, S5) contrast of tone text and destructive hover; worker A (CSS); verified by tests/ui_css.rs; commit: 61f8965
- [x] T2 (S3) 24px field help button; worker A; verified by hover_help tests; commit: 61f8965
- [x] T3 (S4) tokenise the off-token colours; worker A; verified by tests/ui_css.rs; commit: 61f8965
- [x] T4 (S7, S8) light popover shadow (fixed: the class rules outranked the light override) and the `0.9em` sizes; parent; tests/ui_css.rs; commit: 61f8965
- [x] T5 (S6, S9, S10) DESIGN.md wording, dropdown against the scrollbar (form margin-end 12 -> 16, scrollbar is 15px), shell margin (present on the root); parent; src/tests/audit_layout_tests.rs; commit: 61f8965
- [x] T6 (S1-S10) CHANGELOG, screenshots, full checks; parent; fmt + clippy clean, screenshots regenerated, suite green except pre-existing failures (L7); commit: 61f8965

## Log

- L1: User request: "mejora la ui y el diseño."
- L2: Commits are deferred: the worktree already holds earlier uncommitted, user-requested changes in the same files (catalog search only, PROFILES heading, 884px default height, folder link wording, page-rename regression tests), and the user has not yet answered whether to commit them.
- L3: User: "utiliza agentes en paralelo" (and the request again: "mejora la ui y el diseño."). Plan changed to two parallel workers with disjoint files (A: CSS, tokens and their tests; B: Rust layout and DESIGN.md), then one finishing worker (T6).
- L4: The first single worker (mwgr13ra-7-wx5j) was cancelled mid-run and left partial, unverified work in the tree: light `desk_error/success/warning` darkened, dark `desk_error` #fb778a, new tokens `desk_violet_hover/pressed`, `desk_ring`, `desk_ring_gap`, `desk_live_press/rotate/touch`, 24px field help button, `0.9em` -> 13px, destructive command-row hover, `tests/ui_css.rs` (3 tests, pass when run by the parent) and a test in `src/tests/hover_help_tests.rs` (not yet run). Worker A reviews and completes it.
- L5: The off-screen wrapper `/tmp/sdeck-xvfb.sh` now takes a free display from :90-:119 per call, so parallel workers can run GTK tests at once.
- L6: Resumed in a new session ("continua con la implementacion de la ultima sesion"). Found the S9 test vacuous (it picked action index 7 = None, and required a visible overlay scrollbar); fixed it to select More actions (index 5) and measure the scrollbar's natural width, which showed the real defect (dropdown right 394 > 391); fixed with form margin-end 16. S7 confirmed by specificity (`.form-popover > contents` outranks `popover > contents`) with a RED test, then fixed in src/ui-light.css.
- L7: Full off-screen suite: 7 GTK failures, none caused by this work. Fail identically at HEAD dd9ffbd: `plus_preview_is_identical_to_the_shipped_one` (golden hash), both `plugin_add_ui_tests` size/visibility tests (20 s timeout), both `update_check` tests (expect v0.1.0, Cargo.toml is 0.2.0 since af37ae3). Pass alone (order-dependent in the full run): `binding_editor_hides_labeled_rows_for_inactive_controls_and_actions`, `plugin_ui_tests::the_toolbar_add_button_opens_one_dialog_and_the_hint_points_at_it`.
- L8: User: "git haz un solo commit, y sube la rama y crea el PR." One commit 61f8965 (audit fixes + earlier requested polish), branch pushed, PR https://github.com/aldosilvestre/sdeck-plus/pull/9 (6 commits, ~3.3k lines).
