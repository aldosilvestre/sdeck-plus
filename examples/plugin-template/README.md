# sdeck-plugin-hello

A tiny starter plugin for [sdeck-plus](https://github.com/aldosilvestre/sdeck-plus) (protocol v1,
Python 3 standard library only). It has one action, `say-hello`, that returns a greeting. It has
no side effects: no network, no subprocess, no files. Copy this folder, rename it and replace the
action with your own. The full guide is the SDK page (`docs/plugin-sdk.html` in sdeck-plus).

| Action | Controls | Settings |
|---|---|---|
| `say-hello` | key, dial press | `name` (text, up to 32 characters, default `world`), `shout` (boolean, default off) |

## Dependencies

The app shows these in the plugin's sidebar item and flags a missing required command. It never
installs anything for you.

| Kind | Name | Needed for | Notes |
|---|---|---|---|
| command | `python3` | everything | 3.8 or newer; standard library only, no pip packages |

## Use it as a starting point

1. Copy this folder to `sdeck-plugin-<your-id>` and make it its own Git repository.
2. Edit `manifest.json`: change `id`, `name`, `version`, the actions and the `requirements`.
3. Edit `run` (keep it executable) and `tests/test_run.py`; update `LICENSE` with your name.
4. Update the Dependencies table above.

## Test

```sh
python3 -B -I -m unittest discover -s tests -v
```

From an sdeck-plus checkout, validate the project without running it, then ask the validator to
run it once (`--invoke` **runs the plugin** with your privileges):

```sh
cargo run --offline --example plugin_validate -- /path/to/sdeck-plugin-hello
cargo run --offline --example plugin_validate -- /path/to/sdeck-plugin-hello \
    --invoke say-hello --settings '{"name":"Ada"}'
```

## Install

Copy `manifest.json` and `run` to `<data dir>/sdeck-plus/plugins/hello/` (usually
`~/.local/share/sdeck-plus/plugins/hello/`; the folder name must equal the manifest `id`, and
`run` must stay executable). In the app press **Update plugins** in the binding toolbar, open
the **Hello** item under **Plugins** and tick **Enable**. Only `manifest.json` and `run` are
needed at runtime. Plugins run with your user's privileges: read `run` before enabling it.

## Compatibility

Plugin protocol 1. The `requirements` list needs an sdeck-plus build that knows that field
(older builds reject unknown manifest fields); without it, delete the array.
