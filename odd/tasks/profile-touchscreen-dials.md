# Profile-owned touchscreen dials
Objective: dials follow each profile's selected touchscreen page, independent of keypad navigation.
Branch: feat/profile-touchscreen-dials; feature-branch-chain from f4327f8; runners: offline Cargo tests (display-backed GTK), fmt, clippy. Forecast: ~500 changed lines across two dependent work units.

## Specs

- S1: "esta funcionando. Pero me olvide de comentarte que tambien los dial se deben mantener a nivel perfil, similar al comportamiento de las secciones del touch"
- S2: "Junto con las páginas del touch (recomendado)" — "Cada página del touch guarda sus diales: giro, pulsación e información. Cambiar páginas o carpetas del keypad no los afecta."
- S3: "Copiar desde la página principal del perfil" — "Recomendado: cada página del touch empieza con esos diales; se conserva su touch actual y los registros originales."

## Tasks

- T1 [S1-S3] inline; done; versioned dial ownership and idempotent existing-page migration, preserving legacy rows/returns/info; commit: slice 1 (recorded below after commit). Risk: item 1 (stored config upgrade), item 3 (shared ownership).
- T2 [S1,S2] inline; pending; editor page context/summaries, effective live info, bundle regression and user docs; commit: pending. Risk: item 3 (shared editor/render routing).
- T3 [S1-S3] inline checks plus delegated independent verifier; pending; full suite, baseline differential, committed slice assessments and parent spot check; commit: pending. Risk: items 1 and 3.

## Log

L1 Original request: "esta funcionando. Pero me olvide de comentarte que tambien los dial se deben mantener a nivel perfil, similar al comportamiento de las secciones del touch"

L2 User selected S2 then S3. V1 touch-page storage lacks source keypad provenance; seed existing pages from Organization::profile_key, not current keypad selection. Retain edited touch bindings/artwork and legacy records. Fresh legacy migration compares complete touch+dial configurations. New empty touch pages remain empty, like existing touch behavior. No actual user config migration, install or publishing authorized.

L3 Route: one dependent behavior; inline implementation, not independent writers. Read-only map mvkdeoou-9-vgpj supplies ownership/editor/bundle seams. Independent verifier at feature end for named risks. Use a separate ownership predicate: is_touchscreen remains gesture-only for navigation-action validation. Preserve exact-owner bundle writes, exact-rotation precedence and paired publication/recovery.

L6 T1 full checks: job-39 exit 0, 306 library tests/fmt/strict lib clippy; job-40 exit 0, all 41 display-backed GTK tests. Updated existing indicator regression to explicitly assign CPU dial info on its new page rather than inheriting keypad data. Docs describe dial ownership and v1 seeding, including v2 incompatibility for old app versions. Slice 1 will contain T1 only; T2 editor context remains next. No real data changed.

L5 T1 RED job-37 exit 101 reproduced keypad-local dials and collapsed dial variants. Initial hand-written v1 fixture had an invalid trigger/codec; corrected fixture then exact RED exited 101 on missing profile-root seed. GREEN job-38 exit 0: all 11 touchscreen storage tests, including upgrade/edit/restart and existing interrupted-save recovery. Full lib check pending; no completion claimed.

L4 Delivery: reuse explicitly selected feature-branch-chain for this extension, local only. Slice 1: T1 upgrade/routing; slice 2: T2 integration and final source; T3 evidence closes feature. Each assessed against preceding boundary, never accumulated earlier branch. Engram mirror unavailable under local-Markdown-only policy; authoritative task document and authorized vault request record retained, no shared memory used.
