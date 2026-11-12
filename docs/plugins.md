# External plugins (host core)

Plugins are separate programs that add actions without recompiling the app. This page is the
contract for the host in `src/plugins.rs`. **Process separation is not sandboxing**: a plugin runs
with your user's privileges, so enable only plugins you trust.

## Layout

```
<root>/                 # e.g. ~/.local/share/sdeck-plus/plugins (caller-provided)
├── plugins.json        # enabled state, written only by the host
└── <id>/
    ├── manifest.json
    └── run             # the executable named by the manifest
```

- Discovery only reads `manifest.json`; it never launches anything.
- Nothing is enabled by default or by discovery. Enable with `PluginRegistry::set_enabled`.
- `plugins.json`: `{"version":1,"enabled":["demo"]}`. If it is invalid it is kept untouched,
  nothing is enabled and `set_enabled` fails with `InvalidState`. Writes are atomic (0600).
- A malformed plugin is listed in `PluginRegistry::problems()`; other plugins keep working.
- Plugin binaries and credentials are never part of exported configuration. A binding
  (`PluginBinding { plugin_id, action_id, settings }`) is plain data and stays representable when
  the plugin is missing (`PluginError::Missing`) or disabled (`PluginError::Disabled`).

## Manifest (`manifest.json`, at most 64 KiB, unknown fields rejected)

```json
{
  "id": "demo", "version": "1.0.0", "protocol_version": 1, "name": "Demo",
  "executable": "run",
  "actions": [{
    "id": "send", "name": "Send message", "description": "optional",
    "controls": ["key", "dial_turn", "dial_press", "touch"],
    "settings": [
      {"type": "text", "id": "channel", "label": "Channel", "required": true, "max_length": 64},
      {"type": "integer", "id": "level", "label": "Level", "min": 0, "max": 255, "default": 10},
      {"type": "boolean", "id": "loud", "label": "Loud", "default": false}
    ]
  }]
}
```

- Ids (plugin, action, field): 1-48 chars of `[a-z0-9_-]`, starting alphanumeric. The plugin id
  must equal its directory name. At most 64 plugins, 32 actions, 16 fields per action.
- `executable` is a plain relative path. `..`, absolute paths, and symlinks that resolve outside
  the plugin directory are rejected at discovery and again at every invocation.
- Field `required` defaults to false; text `max_length` defaults to 256 (max 1024); integer
  `min`/`max` are mandatory; `default` must satisfy the field's own limits.
- Settings are validated (`PluginAction::validate_settings`): unknown keys, wrong types and
  out-of-bound values are rejected, defaults are filled in, and the payload must stay within
  4096 bytes (the stored-action bound).

### Requirements (optional)

A plugin may declare what it needs with a top-level `requirements` array (default: none; a
manifest without it behaves exactly as before; protocol_version stays 1). At most 16 entries:

```json
"requirements": [
  {"kind": "command", "name": "hyprctl", "description": "Hyprland control", "version": ">= 0.40"},
  {"kind": "service", "name": "bluetooth", "optional": true},
  {"kind": "file", "name": "~/.config/demo/token"}
]
```

| Field | Rule |
|---|---|
| `kind` | `command`, `service` or `file` (required) |
| `name` | 1-64 printable characters (required). For `command`: a bare name matching `^[A-Za-z0-9._+-]{1,64}$`, no slash |
| `description` | optional, at most 200 printable characters; shown as a tooltip |
| `version` | optional, at most 32 printable characters, informational text such as `>= 3.9`; never compared |
| `optional` | optional boolean, default false |

`command` is an executable the plugin runs, looked up on `PATH`; `service` is a system service,
daemon or app the plugin talks to; `file` is a file the user provides. Only `command` is checked.
Unknown kinds or fields, a duplicate (`kind`, `name`) pair and over-long or control-character
text are rejected with the manifest error, like the rest of the manifest.

- **Shown**: the plugin's sidebar item has a compact "Requires" list (kind, name, version,
  "(optional)", description as tooltip). Each required `command` not found on `PATH` gets an
  alert line such as `Missing: hyprctl`; a missing optional command shows `Not found: <name>`
  without an alert. The check runs again on every rescan or refresh. A plugin without
  requirements shows nothing extra.
- **Checked**: `sdeck_plus::plugins::missing_commands` (process `PATH`) and `missing_commands_in`
  (explicit `PATH`) only look: each absolute `PATH` entry is asked, through `std::fs::metadata`,
  for a regular file named `name` with an execute bit. Relative and empty entries are skipped. A
  symlink counts as `metadata` resolves it (a link to an executable is found, a dangling link
  is not). Nothing is executed or spawned.
- **Not done**: the app never installs packages, changes permissions, runs the commands or
  starts services, and a missing requirement never blocks enabling or assigning the plugin;
  the plugin reports its own failures when run.
- **Compatibility**: unknown manifest fields are rejected, so an app build that predates
  `requirements` rejects a manifest that has it (shown as a skipped plugin folder).

## Invocation protocol (version 1)

One child process per request, run by `PluginRegistry::invoke(&binding, &InvokeOptions)` only when
the plugin is installed, enabled and the binding validates. It is synchronous; call it from a
worker thread, not a GTK callback. It is never retried.

Stdin receives one JSON line, then EOF:

```json
{"protocol_version":1,"type":"invoke","request_id":"req-1","action":"send","settings":{"channel":"general","level":10}}
```

Stdout must be exactly one JSON line (no other output):

```json
{"protocol_version":1,"request_id":"req-1","ok":true,"message":"sent"}
{"protocol_version":1,"request_id":"req-1","ok":false,"error":"channel not found"}
```

`message`/`error` are optional, at most 512 characters. Stderr is for diagnostics only and is
returned truncated (4096 bytes). Exit status must be 0.

| Condition | `PluginError` |
|---|---|
| `ok:false` | `Reported(error)` |
| non-zero exit | `NonZeroExit { code, diagnostics }` |
| bad/extra stdout, wrong version | `MalformedResponse` |
| different `request_id` | `RequestIdMismatch` |
| stdout over 16 KiB | `OutputTooLarge` |
| total time exceeded | `Timeout` |

## Supervision

- `request_id`: 1-64 chars of `[A-Za-z0-9._-]`, chosen by the caller. `timeout` covers spawn,
  exchange and exit; default 5 s, at most 60 s.
- The child runs in its own process group with the plugin directory as working directory and a
  cleared environment except `PATH`, `HOME`, `LANG`, `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`,
  `XDG_DATA_HOME`, `WAYLAND_DISPLAY` and `DBUS_SESSION_BUS_ADDRESS`. No shell is involved.
- The host polls the pipes on the calling thread (no helper threads). On exit, timeout, or any
  error the whole process group is killed and the child reaped, so descendants holding the pipes
  open cannot hang the host or leak descriptors. A descendant that creates its own session can
  escape the group; this is a limit of process groups, not a sandbox.
- Plugin credentials and service state are the plugin's own concern; the host stores none.

## In the app

- **Root**: `<data dir>/sdeck-plus/plugins`, where the data dir is `$XDG_DATA_HOME` (absolute) or
  `$HOME/.local/share`, the same convention as the rest of the app's data. It is global: every
  device model sees the same plugins, and `plugins.json` there is the one enabled list. It is
  not part of exported bundles.
- **Sidebar**: each valid plugin is its own collapsible item in the Actions catalog, titled with
  its name plus "(disabled)" while off. The body shows the untrusted-code warning, an explicit
  "Enable" check box and the plugin's actions with the controls they support. Malformed plugin
  folders and an invalid `plugins.json` are shown above the items; "Update plugins" (the icon at the right end of the binding toolbar) rereads the
  folder. Nothing is enabled or run by browsing.
- **Form**: choosing an action opens the generic "Plugin action" form generated from the
  manifest fields (text, bounded integer, boolean). Only actions whose `controls` include the
  selected control (key, dial turn, dial press, touch strip) can be assigned; others are dimmed.
- **Storage**: a binding is the feature action `{"Plugin":{"plugin_id","action_id","settings"}}`
  inside `layers.conf`. Its shape is validated without the manifest (ids, settings that are
  text up to 1024 characters, integers or booleans, at most 16 settings) so it survives a
  missing or disabled plugin through load, save, export and import. The stored JSON is capped at
  4096 bytes on encode and decode. Unknown fields are rejected.
- **Running**: only a control press or "Test action" runs a plugin, through a dedicated
  `plugin-worker` thread (bounded queue of 8; a full queue refuses the request and says so).
  Each request carries an immutable copy of the binding and an id `sdeck-N`, uses the 5 second
  deadline, and is never retried. The worker rereads manifests and `plugins.json` per request.
  At shutdown queued requests are dropped and a running one is waited for at most 6 seconds.
  Results and errors arrive on the same status line as other actions.
- Windows with several device models each keep their own view of the folder; enabling in one
  becomes visible in another after "Update plugins", although runs always use the file on disk.

## Reference plugins

The demonstration plugins are not part of this repository; each one is its own project (one
repository per plugin) that declares what it needs in `manifest.json` and in its README:

- `sdeck-plugin-discord`: Hyprland shortcut actions and a bot message. See its `README.md` for the
  token file rules and limits.
- `sdeck-plugin-mergbw`: writes MeRGBW packets to a Bluetooth hexagon light through BlueZ
  (`busctl`). Its protocol comes from jmbwell/mergbw; see its `README.md` for what was and was not
  verified on a Hexagon Light TG609, and for the limits (5 s first press, connection stays open).

Both are local Git projects for now (no remote repository has been published). To use one, copy
its `manifest.json` and `run` into `<data dir>/sdeck-plus/plugins/<id>/`, update the plugins in
the app and enable it explicitly.

## Validating a plugin project

`examples/plugin_validate.rs` checks a plugin project directory (manifest.json and the
executable at the directory root; the directory name need not equal the id):

```bash
cargo run --offline --example plugin_validate -- <plugin-dir>
cargo run --offline --example plugin_validate -- <plugin-dir> --invoke <action> \
    [--settings '<json>'] [--timeout <seconds, default 5, max 60>] [--request-id <id>]
```

It copies the project's regular files (no symlinks; at most 256 files, 8 MiB per file, 32 MiB
in total; `.git` and `__pycache__` skipped) into a fresh temp root under the plugin id and runs
`PluginRegistry::open`, which executes nothing. It prints `PLUGIN_VALIDATE_OK:` with the id,
version, protocol, actions (controls, settings) and requirements (commands with a `PATH`
check), or one `PLUGIN_VALIDATE_ERROR:` line per problem. Exit status: 0 ok, 1 problems, 2 usage.
With `--invoke` it also enables the plugin in the temp root and performs ONE real invocation
through `PluginRegistry::invoke`, after a warning that this **runs the plugin with your
privileges**; it prints `PLUGIN_INVOKE_OK:` or `PLUGIN_INVOKE_ERROR:`. Without the flag nothing
is run. The temp root is always removed.

## Tests

`cargo test --offline --test plugin_host`, `--test plugin_bindings`, `--test plugin_requirements`
and `--test plugin_validate` run generated `/bin/sh`
fixtures only; they need no display, hardware or network. The worker and sidebar tests live in
the binary (`cargo test --bin sdeck-plus plugin`; GTK ones need a display).
