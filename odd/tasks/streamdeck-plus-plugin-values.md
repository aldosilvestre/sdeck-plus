# Plugins that show a value (brightness, scene, sound mode) on keys and the touch strip
Objective: Let a plugin action bring its own display, so binding it to a key, dial or touch-strip zone shows a value the plugin reports (for MeRGBW: brightness level, selected scene, sound mode and sensitivity), plus the MeRGBW action for the light's sound-reactive mode.
Branch: feat/plugin-values (stacked on feat/plugin-install, PR #6); delivery: local branch, no push without request; checks: cargo test --offline (serial GTK, filtered), fmt, clippy -D warnings, plugin_validate on the plugin repos. MeRGBW work lives in the separate repo /home/aldo/Documents/git/sdeck-plugin-mergbw.

## Specs
S1. "es la posibilidad que tengan la opcion de venir con binding, por ejemplo en el mergbw que nos diga nivel de brillo o la escena seleccionada." Checks: a plugin action can declare a display value; binding that action to a control shows the value the plugin reports; MeRGBW reports brightness level and the selected scene.
S2. "Tambien falto el accion para setear si quiero que funcione como un boometro." with the user's explanation: "En las luces hay un efecto que toma el sonido ambiente y prende las luces segun como suena." Checks: a MeRGBW action switches the light to its sound-reactive mode (NOT a level bar).
S3. Where it shows: "Ambas" (keys and the touch strip). Checks: the value is drawn on a key and on a touch-strip zone.
S4. Source of the shown value: "Si me sirve" (the plugin's own remembered target; the light cannot be read). Checks: the value comes from the plugin, never from reading the light.
S5. Decision: "La acción trae su display (Recomendado)" (the alternative was extra entries in the live-information dropdown). Checks: no new choice in the live-information dropdown is required; display-only actions exist for controls that only show.

## Tasks
- T1 | S2 | route: inline (plugin repo only) | MeRGBW `set-music-mode` (mode 1-6, optional sensitivity), dial `music-next/previous`, `sensitivity-up/down`; 114 unit tests, mutation checks | done | commit: sdeck-plugin-mergbw fa75b62
- T2 | S1,S5 | route: TBD | manifest `values` + per-action `display`, protocol `query` request/response, host support, docs
- T3 | S1,S3,S4 | route: TBD | run queries on the plugin worker and feed the live-information rendering for keys, dials and touch zones
- T4 | S1,S5 | route: TBD | catalog/editor hints, SDK guide, docs/SKILL.md, DESIGN.md
- T5 | S1,S4,S5 | route: inline (plugin repo) | MeRGBW `query` of brightness, scene, sound mode and sensitivity from its state; display-only actions
- T6 | S1-S5 | route: parent | final checks, CHANGELOG, vault, PR note

## Log
L1. User request (2026-11-12/13), verbatim: "otro cambio tambien que deberiamos hacer con respecto a los plugins, es la posibilidad que tengan la opcion de venir con binding, por ejemplo en el mergbw que nos diga nivel de brillo o la escena seleccionada. Tambien falto el accion para setear si quiero que funcione como un boometro."
L2. Answers to the scoping questions: "1. Ambas / 2. En las luces hay un efecto que toma el sonido ambiente y prende las luces segun como suena. / 3. Si me sirve"
L3. Decision question: "La acción trae su display (Recomendado)".
L4. Exploration facts: live info lives in src/info.rs (InfoSource is a Copy enum of 10 sources, `displays: BTreeMap<Trigger, InfoSource>` per layer in layers.rs, rendering src/info_render.rs, MonitorEvent::Info). A new data-carrying InfoSource variant is invasive (loses Copy; layers.conf/touchscreen.json ids), which is why the display is attached to the plugin binding instead. The sound-reactive packets come from jmbwell/mergbw: cmd 0x07 mode 1-6 (spectrum1/2/3, flowing, rolling, rhythm), cmd 0x08 sensitivity 0-100.
L5. T1 done in the MeRGBW repo (fa75b62): set-music-mode, music-next/previous, sensitivity-up/down; state file gained music/sensitivity/mode (old files keep their values); scene and music are mutually exclusive (last chosen wins). Not run on the real light; checksums of the new packets derived by hand.
