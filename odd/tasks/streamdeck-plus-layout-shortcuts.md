# Stream Deck layout and shortcut-only editor

## Intent
Move Recent Events into the right column immediately below control configuration. Refine the existing dark Stream Deck-inspired UI with consistent spacing, margins, and hierarchy, preserving artwork geometry and responsive scrolling. Remove Press key from the editor while preserving saved legacy bindings; edit them as modifier-free Shortcuts.

## Constraints
Preserve the pre-existing dirty worktree and unrelated task documents. Do not change HID, keyboard execution, persisted formats, or physical Press triggers. No push, PR, or merge. Commits pending explicit user authorization (existing changes are intertwined). English repository artifacts. Engram tools unavailable: full-document mirror `odd/streamdeck-plus-layout-shortcuts/tasks` pending.

## Work units
- [x] T1: Place events beneath settings in a right sidebar, unify shell/card/device spacing and styling, and verify production widget geometry. Route: delegated worker; unfamiliar UI and non-trivial constructor/style/tests. Allowed: src/main.rs, README.md. Status: implementation and functional checks complete; commit pending authorization.
- [x] T2: Remove Press key editor option; load legacy actions as modifier-free Shortcut without rewriting until explicit save. Add regression tests and update docs. Route: delegated worker; multiple non-trivial files. Allowed: src/main.rs, src/binding_editor.rs, src/layers.rs (tests only), README.md. Status: implementation and writer checks complete; independent verification passed; commit pending authorization.

- [x] T3: Independent read-only final verification required by unavailable native assessment. Route: delegated verifier; native plan treats unassessable candidate as high. Status: passed. Independent run confirmed fmt, executed focused GTK, full suite (56 library +14 binary), clippy, build and diff check. No scoped regressions found. GTK markers confirmed sidebar hierarchy/spacing, legacy inspection and explicit saving, five-action geometry and 640x480 scrolling, and populated event history.

- [x] T4: Refine surface padding/margins and replace event history with only the latest recorded event. Preserve initial waiting placeholder and existing producers. Route: delegated worker; UI/update/test/docs changes. Allowed: src/main.rs, README.md. Status: implementation and writer checks passed; commit pending authorization. Follow-up scope forecast: 80–160 authored diff lines, separate reviewable unit; earlier changes remain untouched.

## Acceptance and checks
Events remain wired and follow settings in the right column. Dark visual hierarchy and consistent spacing retain square keys, 8:1 strip, all artwork targets, selected/live states, and small-window scrolling. Editor lists Shortcut but not Press key; physical Press event and legacy decoding/execution remain supported. Unsupported legacy names remain non-destructively loadable.
Use meaningful test-first assertions for layout hierarchy and action behavior, observed RED/GREEN where runnable. CSS fidelity needs visual inspection; do not claim screenshot validation without evidence. Run cargo fmt --check, cargo test --offline, cargo clippy --offline --all-targets --all-features -- -D warnings, cargo build --offline, and git diff --check. GTK coverage must report executed vs skipped. Native review switch is on; assess final candidate and inspect before review; consent remains human-owned.

## Delivery and evidence
Forecast: approximately 200–350 authored diff lines over current worktree, ask-on-risk strategy. Pre-existing accumulated diff is not this feature's scope. Review workload re-evaluated from incremental changes. Work-unit commit identities: pending explicit authorization. Initial branch: feature/streamdeck-plus-event-monitor. T1 incremental changes: src/main.rs +133/-11. Observed GTK RED (events outside settings sidebar), then GREEN with executed GTK test (not skipped). All 55 library and 14 binary tests, fmt, clippy, build, and diff check passed. Unsupported cargo fmt --bin attempt changed no files; targeted formatting edits resolved it. No manual visual or hardware checks performed.

## T2 evidence and final verification
Incremental T2: +179/-40 (219 changed lines) across main.rs, binding_editor.rs, layers.rs (tests only), README.md. Combined incremental authored workload: 363 lines. Observed library and executed GTK RED assertions, then GREEN. Final writer checks: fmt, 7 editor tests, 18 layer tests, executed GTK integration, full suite (56 library +14 binary), clippy, build, diff check all passed. Intermediate test compile error and incorrect event-count assertion were corrected before final checks. Manual visual/hardware checks remain pending.
Native assessment unavailable (`package-local-binary-missing`): risk unassessable; independent verifier required. Inspect blocked (`native-status-package-binary-missing`); no lineage or mutation created. Review remains unavailable; do not install or change global tooling automatically. Engram mirror and commits remain pending as stated above.

## Follow-up acceptance
The event card contains exactly one current event (or initial waiting placeholder), replacing rather than appending on every recording call; full text remains accessible. No history count UI. Keep existing notification producers, sidebar placement and keyboard compatibility. Match shared padding/gaps to dark surface separation; preserve artwork geometry and small-window reachability. Extend executed GTK regression coverage with first/repeated/identical/long event replacements and consistent padding assertions. Historical 20-event tests are superseded by this authorized requirement.

## T4 verification
Writer observed executed GTK RED on second event (only latest expected), then GREEN. Latest-only initial/first/repeated/identical/long-event assertions, padding after allocation, geometry and 640x480 fallback passed. Shell margin 16px, CSS card padding 14px, internal gaps 12px. Removed history/count/nested scroller; full-text tooltip retained. README updated. Final fmt, focused executed GTK, full 56 library +14 binary tests, clippy, build, diff check passed. Intermediate format/padding-assertion failures corrected. Native assess and inspect still unavailable (package-local-binary-missing), no lineage/mutation; independent verifier required.

- [x] T5: Independent read-only verification of latest-event and spacing follow-up. Route: delegated verifier per unassessable native risk plan. Status: passed. Fmt, focused GTK twice (executed), full 56 library +14 binary tests, clippy, build and diff check passed; no scoped regressions. Incremental baseline count unavailable, not independently verified.

## Next step
Automated latest-event/padding checks complete. Manual visual/hardware acceptance, native review tooling, commits and unavailable Engram mirror remain pending. No staging or commits authorized. New device-shutdown request is under separate read-only exploration; no shutdown source edits yet.
