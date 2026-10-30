# Symlinked assets and red-X placeholder
Sounds and "open file" targets are referenced through app-owned symlinks instead of the real path; a missing target shows a red X. Images stay copied (L2).
Constraint kept from the project: exported bundles stay symlink-free (links are resolved to file contents).

## Specs
- S1: "en vez de guardar el path real, se crearan link simbolicos a los archivos que se estan utilizando, por ejemplo para imagenes, sonidos, etc."
- S2: "Y si no encuentra el archivo debera setear como imagen una generica, por ejemplo una X roja de fondo para que se vea que no pudo configurarse el boton o seccion del touch."
- S3: "Solo sonidos y archivos; imágenes siguen copiadas" (the user's choice for S1's scope)

## Tasks
- [x] T1 (S1, S3) route: inline — `asset_links` module in the lib: create/reuse a symlink in `<data root>/links/`, resolve it back, report a missing target. Commit: none (see Log). Evidence: 12 lib tests in src/asset_links/tests.rs.
- [x] T2 (S1) route: inline — Save binding stores the link instead of the real path; the editor shows the real target again. Commit: none. Evidence: src/tests/asset_link_save_tests.rs (3 GTK tests).
- [x] T3 (S1) route: inline — bundle export resolves app links to file contents so bundles stay symlink-free. Commit: none. Evidence: export test in src/config_bundle/tests.rs.
- [x] T4 (S2) route: inline — red-X `BindingIcon::MissingFile` shown on a key or touch section whose sound/file target is missing. Commit: none. Evidence: binding_icons test + GTK test sounds_and_files_whose_original_is_gone_show_a_red_cross_on_their_control; screenshots reviewed.
- [x] T5 (S1, S2) route: inline — DESIGN.md / docs, full verification. Commit: none. Evidence: two consecutive full runs, strict clippy: 368 lib, 96 GTK, 2 integration tests.

## Log
- L1: "y como siguiente paso haremos que al exportar la configuración o mejor dicho al guardar la configuracion del dispositivo, en vez de guardar el path real, se crearan link simbolicos a los archivos que se estan utilizando, por ejemplo para imagenes, sonidos, etc. Y si no encuentra el archivo debera setear como imagen una generica, por ejemplo una X roja de fondo para que se vea que no pudo configurarse el boton o seccion del touch."
- L2: "Solo sonidos y archivos; imágenes siguen copiadas" (answer to: how to apply the symlinks; the alternative was symlinking images too)
- L3 (agent note, not a user requirement): no commits are made for this feature because the working tree already holds earlier, uncommitted, unrelated work that the user has not asked to commit; asking before committing.
