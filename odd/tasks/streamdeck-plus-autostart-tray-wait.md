# Autostart tray readiness

Bounded login-startup fix. Risk: item 4 (asynchronous GTK/tray startup lifecycle).

## Specs
S1. “necesito que el autostart cuando tenga el autostart activado, que espere que se cargue el tray.”
S2. “Debe contemplar los casos en donde no exista un tray.”

## Tasks
T1 [done] | S1–S2 | Implement bounded, nonblocking tray readiness and regression tests/documentation | route: worker (context backstop) | commit: work-unit commit pending
T2 [done] | S1–S2 | Independently verify isolated startup, readiness, no-tray fallback and shutdown | route: verify (risk item 4) | commit: work-unit commit pending
T3 | S1–S2 | Complete native assessment/review if available, record evidence and close | route: parent | commit: pending

## Log
L1. User request (verbatim): vamos a hacer una correccion en sdeck, necesito que el autostart cuando tenga el autostart activado, que espere que se cargue el tray. Debe contemplar los casos en donde no exista un tray.
L2. Existing login entry uses --background. Preserve it and manual launch/preferences. Use a finite 10-second grace period, keep GTK responsive, present configuration on unavailable tray; never overwrite preferences. Additional activation must remain usable. Request record: Obsidian memory/requests/sdeck-plus/autostart-wait-for-tray-2026-10-30.md. No callable project-scoped Engram tool: local Markdown/vault continuity only.
L3. Baseline working tree clean on main. gentle-ai review mode status failed (exit 127: command not found); facade assessment later confirmed RDD on.
L4. Human approved direct Tokio dependency via explicit choice approve-direct-tokio; only root dependency declaration changed, no versions upgraded.
L5. Worker RED: 2 failed / 1 passed (exit 101); GREEN: 9/9 focused tests including actual GTK production lifecycle, late watcher/host, unavailable/rejected/stalled registration, pending shutdown and preserved preferences. Headless suite: 397 library + 124 binary + 2 integration passed; graphical bodies skip headless. Clippy all targets passed. Scoped test rustfmt passed after one wrapping correction.
L6. Global fmt check exit 1: unchanged baseline src/binding_editor.rs:238 and src/feature_persistence_tests.rs:52,128. Parent independently confirmed both baseline files fail rustfmt; preserved unrelated files. This remains a known pre-existing gate failure, not a successful global check.
L7. Independent verifier: S1 and S2 met, no production blocker; isolated suite/focused/clippy passed, production lifecycle probe passed and was retained in regression test. Initial diagnostic-only synthetic-window probe timed out at 180 seconds (unpresented window); corrected presented-window probe passed. Remote activation transport not directly exercised; existing-window activation path exercised. No real HID or session login test.
L8. Parent focused spot check exit 0: 9 passed, including displayed GTK lifecycle (12.26 seconds). Final native review/commit evidence pending. One cohesive startup work unit; tests and docs retained together despite exceeding approximately 400 authored lines.
