# Never delete bindings this build does not understand; redo focus-return on the plugin branch
Objective: A build that cannot read a binding (for example a plugin binding opened by a build without plugins) must keep it untouched instead of deleting it, and the "return to the previous profile" feature must live on the plugin-capable branch so testing it cannot damage the user's configuration.
Branches: fix/keep-unknown-bindings (from main, worktree ../sdeck-plus-fix) to be merged into feat/plugin-values; feat/focus-return-on-plugins (from feat/plugin-values, cherry-picks of 7c1f009, 192d404, 0f2ae2c); local only, no push.

## Specs
S1. "deja la configuracion como esta" Checks: the user's real data dir (~/.local/share/sdeck-plus) is never modified by this work.
S2. "solo corrige todo aquello que pueda generar fallos" Checks: (a) loading layers.conf with a binding row whose action/feature kind is unknown to the build keeps that row and writes it back byte-identical on every save (it is not executed and not shown as a normal binding); it is dropped only when the user binds that same control or deletes its layer; (b) the same holds for touch-page bindings in touchscreen.json and for configuration bundle export/import; (c) the conflict log says the row was kept, not removed; (d) the focus-return feature works on top of the plugin branch with all plugin features intact.
S3. "y verifica que funcione la comunicacion con los plugins" Checks: plugin_validate on every installed plugin, real-host invoke/query of safe actions, plus the plugin test suites after the changes.

## Tasks
- T1 | S3 | route: parent | installed discord 0.2.2, intellij 0.1.0, mergbw 0.5.0 valid and identical to their repos; real-host: discord/intellij invoke reach the plugin (non-matching window), intellij query -> NoDisplay, mergbw queries OK, show-mode display-only OK, mergbw power-on wrote to the real light | done
- T2 | S1,S2 | route: gentle-ai-worker in ../sdeck-plus-fix | keep unknown bindings
- T3 | S1,S2 | route: gentle-ai-worker in the main worktree | redo focus-return on feat/plugin-values
- T4 | S1-S3 | route: parent | merge, full checks, re-run T1 checks

## Log
L1. "si, restaura. Probamos y volvemos a hacer la implementacion de lo que te pedi, pero esta vez correctamente." / "listo, ahora si esta funcionando"
L2. "deja la configuracion como esta, solo corrige todo aquello que pueda generar fallos, y verifica que funcione la comunicacion con los plugins"
L3. Root cause of the data loss: src/layers.rs parse() turns any row that fails apply_row into a ConfigConflict, logs it (button-errors.log, `removed=<row>` up to 300 chars) and drops it; the store is later saved without it. A build of feat/focus-return-previous-profile (from main, no plugin host) did this to 17 plugin bindings on 2026-11-13 23:58.
