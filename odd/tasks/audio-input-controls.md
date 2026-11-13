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
- S6: Docs, help text and CHANGELOG describe all of the above; outputs behave exactly as before.

## Tasks
- T1 (S1,S2,S3) core actions: DesktopAction variants, wpctl/pw-dump, serde, validate, summary. inline. commit: pending
- T2 (S1,S2,S3) editor choices, icons, catalog. inline. commit: pending
- T3 (S4) live information sources. inline. commit: pending
- T4 (S5) dial preset. inline. commit: pending
- T6 (S7) OBS input volume up/down actions (obs-websocket). inline. commit: pending
- T5 (S6) docs: README, index, docs.html, CHANGELOG. inline. commit: pending

## Log
L1: hagamos una modificacion mas en el proyecto, necesito que al igual que el volumen de salida se pueda tener controles de dial para el volumen de entrada y que se pueda rotar entre los dispositivos de entrada de audio.
L2: scope chosen: volume +/- with step, mute, rotate input device, live info + dial preset.
L3: "continuemos con estos cambios por que seguramente tendremos que agregar esta informacion a la documentacion" (docs edits from the earlier docs task stay uncommitted and ship with T5).
L4: ah adicionalmente, tendremos que agregar al plugin de OBS la posibilidad de subir o bajar el volumen de las fuentes de entrada de audio.
