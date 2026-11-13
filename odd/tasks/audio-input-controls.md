# Audio input (microphone) controls
Mirror the output-volume and output-device actions for audio inputs, for keys and dials.
Branch: feat/audio-input-controls. Scope chosen by the user: step volume, mute, rotate/fixed input device, live info, dial preset, docs.

## Specs
- S1: "necesito que al igual que el volumen de salida se pueda tener controles de dial para el volumen de entrada" -> Input volume up/down with a 1-100% step (default 5%), on `@DEFAULT_AUDIO_SOURCE@`, raising capped at 100%.
- S2: Toggle input mute (microphone) on `@DEFAULT_AUDIO_SOURCE@`.
- S3: "que se pueda rotar entre los dispositivos de entrada de audio" -> Change audio input: Rotate (next connected `Audio/Source`, wrap) or a fixed device by stable `node.name`.
- S4: Live information: input volume (crossed out when muted) and input device name.
- S5: A "Microphone" preset in the Dials tab (input volume turns, mute on press).
- S7: "tendremos que agregar al plugin de OBS la posibilidad de subir o bajar el volumen de las fuentes de entrada de audio" -> OBS actions to raise/lower the volume of a named OBS audio input (OBS is built in, not an external plugin).
- S8: "La tercer iniciativa me gusta mas, pero me gustaria conservar como se ve la preview actualmente." -> chosen artboard 03-profile-rail (others are alternatives); the hardware preview keeps its current look.
- S6: Docs, help text and CHANGELOG describe all of the above; outputs behave exactly as before.

## Tasks
- T1 (S1,S2,S3) core actions: DesktopAction variants, wpctl/pw-dump, serde, validate, summary. inline. commit: c656412 (bin did not compile alone; fixed by T2 commit)
- T2 (S1,S2,S3) editor choices, icons, catalog. inline. commit: pending
- T3 (S4) live information sources. inline. commit: see git log
- T4 (S5) dial preset. inline. commit: see git log
- T6 (S7) OBS input volume up/down actions (obs-websocket). inline (workers unavailable: provider auth). commit: see git log
- T7 (S8) main-window redesign: implement design/main-window/03-profile-rail.html in GTK keeping the current hardware preview look. rail done (default window 1080x700, compact rail <1200px). Remaining from the artboard: category chips and Media/Output/Input grouping in the catalog; title/status inside the rail (kept in the top header). commit: see git log
- T5 (S6) docs: README, index, docs.html, CHANGELOG. inline. commit: see git log (screenshots regenerated at the end)

## Log
L1: hagamos una modificacion mas en el proyecto, necesito que al igual que el volumen de salida se pueda tener controles de dial para el volumen de entrada y que se pueda rotar entre los dispositivos de entrada de audio.
L2: scope chosen: volume +/- with step, mute, rotate input device, live info + dial preset.
L3: "continuemos con estos cambios por que seguramente tendremos que agregar esta informacion a la documentacion" (docs edits from the earlier docs task stay uncommitted and ship with T5).
L4: ah adicionalmente, tendremos que agregar al plugin de OBS la posibilidad de subir o bajar el volumen de las fuentes de entrada de audio.
L5: vi tambien que las capturas en la documentacion no 100% fieles a la app. Tambien usemos el gentle design para mejorar la interfaz de la aplicacion. (decisions: screenshots regenerated all at the end; redesign = design directions first; redesign before the remaining tasks)
L6: design/main-window/ created with artboards 01-compact-shell, 02-inspector-first, 03-profile-rail (none chosen yet). Pending: T6 OBS input volume, T3 live info, T4 dial preset, T5 docs, final screenshots. Pre-existing GTK failures on base: plus_preview_is_identical_to_the_shipped_one, 2 update_check gtk tests.
L7: La tercer iniciativa me gusta mas, pero me gustaria conservar como se ve la preview actualmente. (artboard 03 updated to the current preview look; chosen via implement handoff; implementation not started)
L8: decisions: rail collapses to icons at 1000px (expands at >=1200px); redesign implemented inline now (delegation unavailable: provider auth).
L9: Recuerda tambien rediseñar los modales, popover, hover, alertas, toast, etc. (overlay/feedback components of the redesign; to be designed as artboards first)
L10: T7 state (uncommitted): vertical profile rail implemented (Notebook tab_pos Left, tools at its foot, one-letter/icon-only below 1200px) in src/feature_runtime.rs, src/main.rs, src/ui.rs, src/ui.css. At 1000px the Stream Deck + XL preview (570px) no longer fits beside the catalog (preview_model_tests::the_preview_contract_holds_at_the_default_window_for_both_models fails) and the Plus preview renders 482px wide instead of 520 (plus_preview_is_identical_to_the_shipped_one asserts 520x428; it already failed on base by hash). Decision needed: default window width vs accept the XL wrap. Popup-heavy GTK tests (binding_icon_tests, binding_editor_hides...) are flaky under the full run.
L11: width decision: default window raised to 1080x700 (XL fits beside the catalog again). GTK suite back to the 3 baseline failures.
L12: T6 implemented inline (workers kept failing with auth; the gentle sync rewrites model profiles back to claude-bridge). OBS input volume: step 1-20 dB (default 2), clamp -100..+26 dB.
L13: T3 (microphone live info: input-volume, input-device, appended to InfoSource::ALL) and T4 (Microphone dial preset) done inline.
L14: T5 docs text done (OBS actions list, return-to-previous profile, plugin values, microphone, OBS input volume, profile rail, 1080px window). Screenshots, catalog grouping and overlay redesign (modals, popovers, hover, alerts, toasts) pending.
L15: design/overlays/ created with 01-quiet-surfaces, 02-tonal-feedback, 03-compact-command (modals, popovers, hover/tooltip, alerts, toast, help). Waiting for the user to choose.
L16: la tercer propuesta es la mejor (overlays: 03-compact-command chosen). Implemented in src/ui.css (popover/tooltip radius+shadow, selected rows, pill toast, banner binding status). A global min-height on popover rows clipped the Organize popover, so row sizing stays unchanged.
L17: docs screenshots regenerated with the official generator (35 PNGs, window 1000x950 captures; Microphone preset uses a microphone icon); img width/height attributes aligned to the PNG sizes.
L18: y por que no lo hiciste, debes hacer todo. (finish every pending part of the chosen artboards: catalog chips + grouping, title/status in the rail, confirm dialogs, toast Undo, banners for all feedback, menu rows + hover/focus)
L19: catalog chips+groups (118448a) and the connection badge in the rail done; title stays in the header (it spans the full width and the rail is 56px when compact).
L20: tambien recuerda que la barra izquierda de la nueva propuesta debe ser consistente, sino con solo las letras cuando la pantalla es chica no queda estetico y es poco entendible (decision: drop the compact initials-only rail; one consistent rail with names at every width; default window widened so the + XL still fits)
