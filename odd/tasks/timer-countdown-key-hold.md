# Timer countdown and exclusive key hold
Objective: countdown on the initiating key and an optional exclusive two-second hold action.
Branch: `feature/timer-countdown-key-hold`; delivery: feature-branch-chain (forecast 800–1200 authored lines); runner: offline Cargo plus display-backed GTK tests.

## Specs
- S1: "podemos hacer que una vez que inicie el timer, el boton muestre la cantidad de segundos, minutos y horas en caso de ser, que faltan para que suene el timer."
- S2: "Tambien podemos agregar una segunda funcionalidad a los botones de tap and hold, que pueda ejecutar una segunda accion en caso de mantener el boton presionado por 2 segundos. Por ejemplo para cancelar el timer." User correction: "lo del tiempo faltante en el boton esta bien, pero lo de cancelar el timer cuando se mantiene el boton 2 segundos no esta funcionando."
- S3: User selected "Acciones excluyentes (recomendado)": "Al soltar antes de 2 segundos, ejecuta la acción corta. Al alcanzar 2 segundos, ejecuta solo la segunda acción; soltar después no ejecuta nada más. Así podés iniciar el timer con un toque y cancelarlo manteniendo presionado."

## Tasks
- T1 [S1] delegated countdown: done; commit: pending; tests: runtime RED exit 101, GREEN 7 display-backed tests exit 0.
- T2 [S2-S3] delegated hold storage/recognition/editor: done; core commit: 6d31596; editor integration commit: pending; core 16 GREEN, editor/runtime 4 GREEN, genuine catalog-path cancellation RED/GREEN observed.
- T3 [S1-S3] parent integration and verification: in_progress; commit: pending; display binary 112 passed; headless 395 library/112 binary/2 integration passed; docs finished; formatting/commit closure pending.

## Log
### L1 — Original request
podemos hacer que una vez que inicie el timer, el boton muestre la cantidad de segundos, minutos y horas en caso de ser, que faltan para que suene el timer.

Tambien podemos agregar una segunda funcionalidad a los botones de tap and hold, que pueda ejecutar una segunda accion en caso de mantener el boton presionado por 2 segundos. Por ejemplo para cancelar el timer.

### L2 — Decision and evidence
User selected exclusive tap/hold. Keys without a secondary action retain immediate execution. Proposed countdown display: `45s`, `02:15`, `01:02:15`. Device reports full key-state snapshots; deduplication is needed for stable dispatch. Preserve all preexisting dirty changes (timer notification/chime plus unrelated icon/render optimizations). No publishing authorized. No shared Engram used: project requires local Markdown records in authorized vault. Full feature mirror is stored alongside request note.

### L3 — Routing and delivery
Two independent substantial behaviors require coordinated implementation. Countdown may own main.rs artwork hooks; hold writer owns core/storage and separate modules; parent integrates main.rs input/editor seams. Forecast exceeds advisory 400-line review slice. User selected feature-branch-chain before implementation. No publishing authorized. Existing dirty baseline must not enter feature commits accidentally; prior timer fixes overlap and require deliberate selective staging or a separately authorized baseline commit.

### L4 — Worker evidence
T1 implemented ephemeral owner/key countdown with existing ComposeInfo and snapshot rendering. Seven display-backed countdown tests passed; notification/chime (2), page-return (1), live-info (2) also passed. Actual hardware not checked. T2 core added KeyHold/kh:n optional bindings, public pure recognizer and optional editor toggle helper; 16 key-hold, 13 persistence, 21 layer, 9 editor and 24 bundle tests passed. Binary integration is still outstanding. Native assessment returned unassessable (untracked scope) and a writer self-verification plan; no review started. Full suite is deferred until integration. No shared memory or commits performed.

### L5 — Integration partial / needs user decision
Integration worker wired physical input through pure recognizer and existing 40ms loop, keypad-only Hold 2 seconds toggle, captured identities/returns, countdown origin, generation cancellation and import/owner lifecycle hooks. Runtime RED exit 101 observed (`hold-configured key must defer tap`), then 8 countdown and 3 hold runtime tests passed. Editor regression remains failing at src/tests/key_hold_ui_tests.rs:80: expected saved/reloaded KeyHold(1) = Some(Feature(CancelTimer)), observed None. Three focused attempts exited 101; worker stopped at its bounded correction limit. Full display suite exit 101: 111 passed, 1 failed (same editor test). Headless suite exit 0: 395 library, 112 binary (GTK bodies skip), 2 integration. cargo fmt --all -- --check exit 1 with new/peer formatting differences. Remove/undo assertions after failure unexecuted. Hold docs still include integration notes; HTML/changelog hold additions unfinished. Dedicated disconnect/import/shutdown/batch tests absent; no real hardware proof or historical differential check. Native assessment again unassessable due untracked scope, no native review started. No commits or install. Needs decision: authorize a focused editor save/reload diagnosis/fix and completion of docs/formatting/remaining proof, or narrow delivery to countdown while explicitly deferring hold (do not revert anything without user's choice).

### L6 — User failure report / resume
lo del tiempo faltante en el boton esta bien, pero lo de cancelar el timer cuando se mantiene el boton 2 segundos no esta funcionando.

Resume authorized focused correction of cancellation without altering accepted countdown. Local task file and vault mirror reconciled before work. Reproduce editor save/reload failure and complete real input hold cancellation path.

### L7 — Focused correction verified
Parent reproduction exit 101 preserved. Real bug reproduced via catalog action assignment: reselecting same key rebuilt its number picker and reset hold_toggle, saving CancelTimer as primary KeyPress instead of KeyHold. Minimal same-key early return now preserves mode; selecting a different control still resets it. Prior test also opened wrong fixture parent directory; corrected to snapshot.root(). Added genuine catalog/save/persist/reload/input-channel regression: tap starts countdown, hold cancels, artwork restored, release never restarts. Worker focused key_hold 4 passed; countdown 8 passed; full display 112 passed; headless 395 library/112 binary/2 integration passed. Docs now describe actual finished feature. No actual HID proof; previously overwritten user assignments require manual reconfiguration. No stored user data changed.

### L8 — Work-unit boundaries and closure checks
Slice 1 committed as 6d31596 (core binding storage/recognizer/editor helper, 780 authored lines). Slice 2 is the application countdown/notification/chime and editor/input integration with public-path tests/docs (1651 authored lines staged; includes prior authorized timer fixes, excludes human icon/background/marquee optimizations). One honest splitting pass kept coherent tested units; both exceed advisory 400-line budget, mostly due to functional test coverage. Do not shrink tests or code-golf for size; future PRs remain unpublished and follow feature-branch-chain choice. Initial forecast was low; actual feature total is approximately 2431 authored lines plus tracking. Native assessment on core committed range was unavailable due intended-untracked declaration, with writer self-verification plan. Scope formatting check initially exited 1, then exact new files were formatted and scope check plus git diff --check passed (exit 0). Broad project formatter remains not claimed clean; unrelated files preserved. Final display/headless suites are running after scoped formatting.
