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
- T3 (S4) live information sources. inline. commit: pending
- T4 (S5) dial preset. inline. commit: pending
- T6 (S7) OBS input volume up/down actions (obs-websocket). inline. commit: pending
- T7 (S8) main-window redesign: implement design/main-window/03-profile-rail.html in GTK keeping the current hardware preview look. NOT STARTED, blocked on width decision. commit: pending
- T5 (S6) docs: README, index, docs.html, CHANGELOG. inline. commit: pending

## Log
L1: hagamos una modificacion mas en el proyecto, necesito que al igual que el volumen de salida se pueda tener controles de dial para el volumen de entrada y que se pueda rotar entre los dispositivos de entrada de audio.
L2: scope chosen: volume +/- with step, mute, rotate input device, live info + dial preset.
L3: "continuemos con estos cambios por que seguramente tendremos que agregar esta informacion a la documentacion" (docs edits from the earlier docs task stay uncommitted and ship with T5).
L4: ah adicionalmente, tendremos que agregar al plugin de OBS la posibilidad de subir o bajar el volumen de las fuentes de entrada de audio.
L5: vi tambien que las capturas en la documentacion no 100% fieles a la app. Tambien usemos el gentle design para mejorar la interfaz de la aplicacion. (decisions: screenshots regenerated all at the end; redesign = design directions first; redesign before the remaining tasks)
L6: design/main-window/ created with artboards 01-compact-shell, 02-inspector-first, 03-profile-rail (none chosen yet). Pending: T6 OBS input volume, T3 live info, T4 dial preset, T5 docs, final screenshots. Pre-existing GTK failures on base: plus_preview_is_identical_to_the_shipped_one, 2 update_check gtk tests.
L7: La tercer iniciativa me gusta mas, pero me gustaria conservar como se ve la preview actualmente. (artboard 03 updated to the current preview look; chosen via implement handoff; implementation not started)
