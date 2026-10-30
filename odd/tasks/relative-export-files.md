# Relative file references in configuration bundles
Export metadata only, preserving filenames in type-specific relative folders.
Risk: item 1 (versioned bundle/import changes and transactional preservation of local files); item 2 (path privacy and untrusted imports).

## Specs
S1. "cuando se exporta la configuracion no deberia exportar los archivos. Deberia exportar solo el link relativo al archivo, incluso esa era mi idea cuando quise guardar link simbolicos, para no exponer la direccion de los verdaderos archivos cuando se pase a otra maquina la configuracion exportada."
S2. "pero con eso estaria exponiendo como esta estructurado mi sistema de carpetas a otro"
S3. "lo que deberias hacer es tener una carpeta para imagenes y una para sonidos y todo lo que necesite apuntar a otro archivo. por ejemplo si tengo un archivo el /home/aldo/.../musica/cancionX.mp3 configurado actualmente, a la hora de exportar deberia ser a ./sonidos/cancionX.mp3"
S4. "Y de esa forma para todos los archivos."
S5. "pero tienes que aplicar este cambio tambien para las imagenes de fondo, no quiero que guardes nada en base64."

## Tasks
- [x] T1 — S1-S5 — inline: public export/import regression tests; observe RED. Commit: 4e460ba.
- [x] T2 — S1-S5 — inline: metadata-only versioned bundles, typed file references, local resolution/preservation and documentation. Commit: 4e460ba.
- [x] T4 — S5 — inline: refresh full suite, Clippy, format and diff checks for plain-text exports. Commit: 4e460ba.
- [-] T3 — S1-S5 — independent verification unavailable; user explicitly accepted delivery without this proof. No independent verdict.
- [x] T5 — S1-S5 — inline: record acceptance, commit scoped implementation and close. Commit: 4e460ba.

## Log
L1. User: "cuando se exporta la configuracion no deberia exportar los archivos. Deberia exportar solo el link relativo al archivo, incluso esa era mi idea cuando quise guardar link simbolicos, para no exponer la direccion de los verdaderos archivos cuando se pase a otra maquina la configuracion exportada."
L2. User: "pero con eso estaria exponiendo como esta estructurado mi sistema de carpetas a otro"
L3. User: "lo que deberias hacer es tener una carpeta para imagenes y una para sonidos y todo lo que necesite apuntar a otro archivo. por ejemplo si tengo un archivo el /home/aldo/.../musica/cancionX.mp3 configurado actualmente, a la hora de exportar deberia ser a ./sonidos/cancionX.mp3 \n\nY de esa forma para todos los archivos."
L4. Implementation scope: sound -> sonidos, image -> imagenes, other structured file/folder/executable targets -> archivos. Preserve URLs, bare executable names, command source and arguments as user-entered opaque text; do not promise general secret redaction. Missing local referenced files allowed; unsafe names and ambiguous same-basename targets rejected rather than merged. Preserve legacy v1/v2 import. Imported paths resolve under local app data root, never export location or process CWD. No migration of original local files on export.
L5. RED observed: metadata reference export fails with unsafe asset filename; duplicate sound basenames fail with missing-file I/O rather than collision diagnostic. Logs: target/relative-export-red.log and target/relative-export-collision-red.log.
L6. T2 implemented: bundle v3 metadata only; typed references and conservative collision rejection; missing-image references and local directory preservation with rollback. Legacy v1/v2 imports retained. All 22 focused bundle tests passed. Strict Clippy initially rejected a range pattern; corrected to 1..=3. Full checks running. ASSESS returned high via item 1 escalation, independent verifier required; native assessment unavailable due undeclared untracked task document.
L7. Full self-verification passed: 375 library, 97 GTK, 2 integration; strict Clippy, fmt and diff checks passed. Native generator passed all 33 screenshots; copied only updated import confirmation and refreshed its HTML dimensions.
L8. BLOCKED: independent gentle-ai-verify failed before agent_settled with Node WriteStream error -122, syscall write. No verdict or independent proof produced; no retry. T3 awaits user decision about unavailable independent verification. No commit, installation or publication.
L9. User: "pero tienes que aplicar este cambio tambien para las imagenes de fondo, no quiero que guardes nada en base64." Reopen T1/T2 for S5: background artwork references and plain-text metadata export, preserving import of v1-v3. Independent verification remains unavailable; no automatic retry.
L10. S5 RED: public background export test covers keys, whole-strip and zone images; fails because exported artwork manifest is base64 rather than raw streamdeck-artwork-v3 text. Exit 101; target/plain-export-red.log.
L11. S5 implemented: export v4 stores only UTF-8 config text in JSON (no base64 calls on export). Import v1-v3 decoding retained. All 24 focused bundle tests pass, including key/strip/zone background references and legacy-v3 import. Background roundtrip test corrected to initialize touchscreen ownership through the public API, as production does. Docs updated; full S5 checks next.
L12. S5 full checks passed (job-45, exit 0): 377 library + 97 GTK + 2 integration; strict Clippy, fmt and diff checks. T4 complete. T3 remains BLOCKED on unavailable independent verifier; no retry, commit, installation or publication.
L13. User: "si acepto". Explicitly accepts delivery without independent verification; T3 dropped, not passed. No native/independent approval claimed. T5 finalizing local work-unit commit; no installation/push/publication. Bundle version, typed paths and transactional importer form one coherent risk-bearing unit with tests/docs, kept together.
L14. Implementation, regression tests, docs and native confirmation capture committed as 4e460ba (fix(config): export readable relative file references without payloads). Feature accepted with T3 explicitly omitted, not verified. No install/push/publication. This tracking record is retained separately from the implementation commit.
