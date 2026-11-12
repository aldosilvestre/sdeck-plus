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

## Tests

`cargo test --offline --test plugin_host` runs generated `/bin/sh` fixtures only; it needs no
display, hardware or network.
