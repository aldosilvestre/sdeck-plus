# Multi action: one key runs several actions in order
A new "More actions" kind, Multi action, runs an ordered list of steps (actions and waits) from one key.
Branch: feat/multi-action (from main 3d4e1aa; that commit holds the user's earlier pending work, 1247 lines, not part of this feature).

## Specs
- S1: "una nueva accion, esta sera un multi accion, es decir un accion que puede hacer multiples acciones." -> new FeatureAction variant Multi { steps } stored inside the existing `feature-v1` row (no layers.conf version change; older builds keep the row unchanged as an unknown kind).
- S2: "Orden estricto, esperando que cada paso termine" -> step N+1 starts only after step N finished (Command: process exited; Launch: process spawned; Shortcut/KeyPress/Desktop/OBS/Device/Plugin/Emit: their dispatch completed), even when steps run on different threads.
- S3: "Sí, paso Esperar (ms/s)" -> a Wait step pauses the sequence without blocking the GTK thread or any worker; the editor offers milliseconds or seconds; range 10 ms to 60 s (assumption A1).
- S4: "Cortar la secuencia y avisar en el toast" -> when a step fails, the remaining steps do not run and the error toast names the step: "Multi action stopped at step <n>: <error>" (assumption A2 for the wording).
- S5: "Teclas, sin anidar, navegación solo al final" -> Multi is valid only on key triggers (press, hold); a step can never be a Multi; Go to profile/page, Navigation and Touchscreen steps are allowed only as the last step; saving an invalid list is refused with a sentence and stores nothing.
- S6: every place that follows a binding's sounds, files, launch paths, pages and folders also follows the steps of a Multi (asset links, bundle export/import, layer/folder removal and rename cleanup, missing-target checks).
- S7: docs (docs/docs.html, docs/index.html, DESIGN.md, CHANGELOG.md, src/AGENTS.md) describe Multi action.

## Assumptions (defaults chosen by the parent; the user may change them)
- A1: Wait range 10 ms to 60 s; at most 16 steps; at least 1 action step; the whole stored action keeps the existing 4096-byte cap (error "Multi action is too large to store" otherwise).
- A2: toast wording of S4.
- A3: Timer, Cancel timer and None are not offered as steps.
- A4: pressing the key again while its sequence runs is ignored; the binding's optional page return is armed after the last step finishes successfully.

## Tasks
- T1 (S1,S5,A1,A3) model, persistence and validation of FeatureAction::Multi + steps. done; inline; commit: see L5.
- T2 (S6) nested traversal: asset links, config bundle, ownership, layer cleanup, validation. pending.
- T3 (S2,S3,S4,A4) runtime sequencer with per-step completion. pending.
- T4 (S3,S5) editor: Multi action step list in More actions. pending.
- T5 (S7) docs, changelog, screenshots. pending.

## Log
L1: "podemos analizar la posibilidad de agregar una nueva accion, esta sera un multi accion, es decir un accion que puede hacer multiples acciones."
L2: explorer mwfdyfk-1-o796 map: Action not serde; feature-v1 = hex(JSON) <= 4096 bytes; unknown_action_kind keeps unknown variants; GTK-thread actions (SwitchLayer/Navigation/Touchscreen/Emit/Timer) vs serial layer worker (Desktop/OBS/Launch/Command/KeyPress/Shortcut) vs plugin/device workers, no per-step completion signal; top-level-only traversals in asset_links, config_bundle, ownership, layers cleanup, plugin_values, key_hold; core action fields live in main.rs.
L3: "podemos commitear los cambios que quedaron sin commitear, y luego procedemos hacer este nuevo feature en un branch nuevo con los ultimos cambios" -> the pending work was already committed as 3d4e1aa "Fix: background + toast" (fmt 0, clippy 0, headless cargo test 0 run by the parent before); branch feat/multi-action created from it.
L4: user decisions: "Orden estricto, esperando que cada paso termine"; "Sí, paso Esperar (ms/s)"; "Cortar la secuencia y avisar en el toast"; "Teclas, sin anidar, navegación solo al final".
L5: T1: FeatureAction::Multi { steps: Vec<MultiStep> }, MultiStep::Action (stored as the layers.conf action string via layers::encode_action/decode_action, now pub(crate)) | Wait { milliseconds }; validate_multi (1-16 steps, >=1 action, wait 10..=60000 ms, no nesting/timers/None/touchscreen, navigation last, step errors "Step n: ..."), "Multi action is too large to store" over 4096 bytes (also on decode), "multi actions require a key press or key hold" in validate_binding_action; VARIANTS += Multi; icon falls back to More; editor populate returns early; runtime reports "Multi actions cannot run yet." until T3. Tests src/multi_action_tests.rs: RED (compile: no Multi/MultiStep) then GREEN 4/4. fmt 0, clippy 0, headless cargo test 1251 passed 0 failed. Nested SwitchLayer target existence and keyboard normalization of nested steps left for T2.
