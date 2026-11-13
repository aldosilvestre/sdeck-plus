---
name: sdeck-plus-plugin-author
description: "Trigger: create, extend, debug or integrate a sdeck-plus plugin, Stream Deck plugin for sdeck-plus, plugin manifest.json, plugin run protocol, plugin_validate. Guides an AI agent to write a protocol v1 plugin, test it, validate it and hand it to the user without enabling it."
license: MIT
metadata:
  author: aldosilvestre
  version: "1.0"
---

## Activation contract

Use this skill when the user asks to create, change, debug, validate or install a plugin for
**sdeck-plus** (the GTK app for the Elgato Stream Deck +). A plugin is an external program plus a
`manifest.json`: the app starts it once per key press or dial event, sends one JSON line on stdin
and reads one JSON line from stdout.

Do NOT use it to change the app itself (Rust code under `src/`), to write built-in actions, or for
other Stream Deck software. This file is self-contained; the longer references are
`docs/plugin-sdk.html` (author guide) and `docs/plugins.md` (host contract) in an sdeck-plus checkout.

## Hard rules

1. Never read or print secrets or credential files (token files, key files, other tools' configuration). Ask the user to create token files themselves.
2. NEVER enable a plugin on the user's behalf. Enable is the user's explicit act in the app (a check box in the plugin's sidebar item).
3. Never run an installed plugin, and never use `--invoke`, against anything real (a service, a device, the user's session) without the user's say-so. Prefer fakes in tests.
4. Never contact the network, push, publish or create a remote repository without explicit permission.
5. Never write outside the plugin project, except the one install step the user authorised (Step 8b).
6. The app never installs packages; you must not either. Declare dependencies in `requirements` and the README, and tell the user.
7. Process separation is not a sandbox: the plugin runs with the user's privileges. Write code a user can read in a minute and find nothing surprising.

## Step 0 - gather requirements

Ask the user ONLY what is missing; decide the rest and say which defaults you chose.

| Need | Default if the user has no preference |
|---|---|
| What each action does (one sentence per action) | none: ask |
| Controls per action: `key`, `dial_turn`, `dial_press`, `touch` | `key` (and `dial_press` for a one-shot) |
| Settings the user edits per binding (text / integer / boolean) | the fewest that make sense |
| External tools, services, credentials | Python 3 standard library only; no credentials |
| Target desktop / OS (the app is Linux; Hyprland, GNOME...) | the user's current desktop |
| Repository name and location | `sdeck-plugin-<id>`, a new local folder the user names |

Dial turns start a process per event: bind `dial_turn` only to cheap, quick actions.

## Step 1 - create the project

One repository per plugin, suggested name `sdeck-plugin-<id>`. Everything sits at the project root.
Either copy `examples/plugin-template` from an sdeck-plus checkout (Python 3, standard library only)
or generate the same layout:

```text
sdeck-plugin-<id>/
├── manifest.json     # Step 2
├── run               # executable, mode 0755 (chmod 755 run), shebang #!/usr/bin/env python3
├── README.md         # what it touches, actions table, a "## Dependencies" section
├── LICENSE
└── tests/test_run.py # Step 6
```

- No symlinks anywhere in the project (the installer rejects them). Helper files are fine and are installed with the project.
- `.git` and `__pycache__` are ignored by the installer; keep the repository small.
- Run tests with `python3 -B -I` so no `__pycache__` is written.

## Step 2 - manifest.json

Unknown fields are rejected at every level. At most 64 KiB. Example (identical to the template):

```json manifest.json
{
  "id": "hello",
  "version": "0.1.0",
  "protocol_version": 1,
  "name": "Hello",
  "executable": "run",
  "requirements": [
    {"kind": "command", "name": "python3", "version": ">= 3.8", "description": "Runs the plugin; standard library only, no pip packages"}
  ],
  "actions": [
    {
      "id": "say-hello",
      "name": "Say hello",
      "description": "Returns a greeting. It has no side effects.",
      "controls": ["key", "dial_press"],
      "settings": [
        {"type": "text", "id": "name", "label": "Name", "max_length": 32, "default": "world"},
        {"type": "boolean", "id": "shout", "label": "Shout", "default": false}
      ]
    }
  ]
}
```

| Field | Rule |
|---|---|
| `id` | 1-48 chars of `[a-z0-9_-]`, starting with a letter or digit. The installed folder name must equal it |
| `version` | 1-32 printable characters |
| `protocol_version` | the integer `1` |
| `name` | 1-64 printable characters |
| `executable` | plain relative path, at most 128 chars (`run`); no `..`, no absolute path, must stay inside the folder |
| `actions` | 1-32 entries, unique ids |
| action `id` / `name` / `description` | id as above; name 1-64 printable; description optional, at most 256 characters |
| action `controls` | non-empty list of `key`, `dial_turn`, `dial_press`, `touch` |
| action `settings` | at most 16 fields, unique ids; the validated payload stays within 4096 bytes |
| `requirements` | optional, at most 16, no duplicate (`kind`, `name`) |

Settings fields all take `id` (rules as above), `label` (1-64 printable) and optional `required` (default false):

| `type` | Extra keys |
|---|---|
| `text` | `max_length` 1-1024 (default 256), `default`; no control characters; a required text may not be blank |
| `integer` | `min` and `max` (both mandatory, `min <= max`), `default` inside the range |
| `boolean` | `default` |

A `default` must satisfy its own field's limits. Requirements:

| Key | Rule |
|---|---|
| `kind` | `command` (looked up on `PATH`), `service` (informational), `file` (informational) |
| `name` | 1-64 printable characters; for `command` a bare name of `[A-Za-z0-9._+-]` (no slash) |
| `description` | optional, at most 200 printable characters |
| `version` | optional, at most 32 printable characters, informational only |
| `optional` | optional boolean, default false |

Requirements are shown in the sidebar and a missing command is flagged; the app never installs or runs
anything for them and a missing one never blocks enabling. An app build that predates `requirements`
rejects a manifest that has it (the folder is listed as skipped).

## Step 3 - protocol v1

The app writes ONE line on stdin, then EOF:

```json
{"protocol_version":1,"type":"invoke","request_id":"sdeck-7","action":"say-hello","settings":{"name":"Ada"}}
```

The plugin writes exactly ONE line on stdout and exits with status 0:

```json
{"protocol_version":1,"request_id":"sdeck-7","ok":true,"message":"Hello, Ada!"}
{"protocol_version":1,"request_id":"sdeck-7","ok":false,"error":"channel not found"}
```

- `protocol_version` must be the integer 1 (not `true`, not `1.0`). `request_id` (1-64 chars of `[A-Za-z0-9._-]`) must be echoed unchanged.
- `message` / `error` are optional and at most 512 characters. No other fields, no other output, no banners.
- Failure is `ok:false` with exit 0. A non-zero exit, a traceback or a crash is reported by the app as a failure.
- stderr is for diagnostics only; the app keeps 4096 bytes of it. Never put secrets there.
- A hand-edited binding can hold settings of any JSON type: validate every setting (type, range, length, unknown keys) before acting.

| Outcome (`PluginError`) | Cause |
|---|---|
| `Reported(error)` | `ok:false` |
| `NonZeroExit` | exit status other than 0 |
| `MalformedResponse` | extra or missing output or fields, wrong `protocol_version`, text over 512 characters |
| `RequestIdMismatch` | a different `request_id` |
| `OutputTooLarge` | stdout over 16 KiB; the process is killed |
| `Timeout` | the 5 s deadline passed |
| `NoDisplay` | a query on an action that declares no `display` (nothing runs) |

Complete minimal plugin, standard library only (identical to `examples/plugin-template/run`). It is
importable (`main(argv, stdin, stdout, environ)`), always returns 0, validates before acting and never
prints a traceback. Replace `say_hello` and its settings check with your action:

```python run
#!/usr/bin/env python3
"""Starter plugin for sdeck-plus (protocol v1, standard library only).

Reads one JSON request line on stdin and writes exactly one JSON response line on stdout.
Diagnostics, if any, go to stderr. It only builds a greeting: no side effects.
"""

import json
import re
import sys

PROTOCOL_VERSION = 1
MAX_REQUEST_BYTES = 64 * 1024
MAX_NAME = 32
MAX_MESSAGE = 512  # the host rejects longer message/error texts
REQUEST_ID = re.compile(r"[A-Za-z0-9._-]{1,64}")


class PluginFailure(Exception):
    """A problem to report to the user as ok:false (never an exit code)."""


def settings_for_say_hello(settings):
    """Validates untrusted settings again (the host already did) and fills in the defaults."""
    if not isinstance(settings, dict):
        raise PluginFailure("settings must be an object")
    unknown = sorted(set(settings) - {"name", "shout"})
    if unknown:
        raise PluginFailure(f"unknown setting `{unknown[0]}`")
    name = settings.get("name", "world")
    if not isinstance(name, str) or not name.strip():
        raise PluginFailure("name must be non-empty text")
    if len(name) > MAX_NAME or any(not c.isprintable() for c in name):
        raise PluginFailure(f"name must be at most {MAX_NAME} printable characters")
    shout = settings.get("shout", False)
    if not isinstance(shout, bool):
        raise PluginFailure("shout must be true or false")
    return name, shout


def say_hello(settings):
    name, shout = settings_for_say_hello(settings)
    greeting = f"Hello, {name}!"
    return greeting.upper() if shout else greeting


ACTIONS = {"say-hello": say_hello}


def handle(request):
    if request.get("type") != "invoke":
        raise PluginFailure("unsupported request type")
    action = ACTIONS.get(request.get("action"))
    if action is None:
        raise PluginFailure("unknown action")
    return action(request.get("settings", {}))


def reply(stdout, request_id, ok, text):
    body = {"protocol_version": PROTOCOL_VERSION, "request_id": request_id, "ok": ok}
    body["message" if ok else "error"] = text[:MAX_MESSAGE]
    stdout.write(json.dumps(body) + "\n")
    stdout.flush()


def main(argv=None, stdin=None, stdout=None, environ=None):
    """Always returns 0: failures are answered with ok:false, never with a traceback."""
    stdin = sys.stdin if stdin is None else stdin
    stdout = sys.stdout if stdout is None else stdout
    request_id = "unknown"
    try:
        try:
            request = json.loads(stdin.readline(MAX_REQUEST_BYTES + 1))
        except ValueError:
            raise PluginFailure("request is not valid JSON") from None
        if not isinstance(request, dict):
            raise PluginFailure("request must be a JSON object")
        candidate = request.get("request_id")
        if isinstance(candidate, str) and REQUEST_ID.fullmatch(candidate):
            request_id = candidate
        if type(request.get("protocol_version")) is not int or request["protocol_version"] != 1:
            raise PluginFailure("unsupported protocol_version")
        reply(stdout, request_id, True, handle(request))
    except PluginFailure as failure:
        reply(stdout, request_id, False, str(failure))
    except Exception as error:  # last resort: no traceback, no error text
        print(f"internal error: {type(error).__name__}", file=sys.stderr)
        reply(stdout, request_id, False, "internal error in the plugin")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

An action with side effects takes its collaborators as parameters so tests can fake them:

```python
def my_action(settings, runner=subprocess.run, clock=time.monotonic):
    deadline = clock() + 4.5  # stop before the host's 5 s deadline
    result = runner(["tool", "--flag", value], capture_output=True, check=False,
                    timeout=max(0.1, deadline - clock()))
    if result.returncode != 0:
        raise PluginFailure("tool failed")  # never echo result.stderr blindly
    return "done"
```

### Showing a value (optional)

An action can show a value the plugin remembers (a brightness, a scene). Declare `"values": [{"id": "brightness", "label": "Brightness"}]` (at most 16 values, unique ids, label 1-64 printable characters) and give the action `"display": "brightness"` (it must name a declared value). The app then asks with a `query` request (a value id, no `action`):

```json
{"protocol_version":1,"type":"query","request_id":"sdeck-8","value":"brightness","settings":{}}
```

Answer with one line and exit 0:

```json
{"protocol_version":1,"request_id":"sdeck-8","ok":true,"value":{"headline":"70%","detail":"Brightness","level":70}}
```

`headline` is required (1-32 printable characters), `detail` optional (at most 64 printable characters), `level` an optional integer 0-100; unknown fields are rejected. Failure is `ok:false` with `error`, as for an invocation. Same limits as an invocation (5 s, 16 KiB). Answer from your own remembered state, quickly, and never block or contact the device. A query on an action without `display` fails with `NoDisplay` before anything runs. Check it with `plugin_validate <dir> --query <action>` (it RUNS the plugin).

## Step 4 - runtime constraints to design for

| Constraint | Consequence |
|---|---|
| Cleared environment: only `PATH`, `HOME`, `LANG`, `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `WAYLAND_DISPLAY`, `DBUS_SESSION_BUS_ADDRESS` | Derive anything else. Example: `HYPRLAND_INSTANCE_SIGNATURE` is not forwarded; find the instance under `$XDG_RUNTIME_DIR/hypr/`. Never ask the user to change the app's environment |
| Working directory is the plugin folder | Use relative paths only for your own files |
| Fixed 5 s deadline per request (a manifest cannot ask for more) | Stop at 4.5 s yourself and say so in `error`; a slow first press (for example a Bluetooth connect) may fail and the next work |
| stdout capped at 16 KiB | Keep answers to a sentence |
| Own process group, killed on exit and on timeout | Do not daemonise; a descendant in a new session can escape, never rely on it |
| One request at a time on one worker, a queue of eight; a full queue refuses the press | Keep actions quick; no state in memory (one process per request); keep state in files you own |
| No shell | Start subprocesses with argv lists; never `shell=True`; never build a command string from settings |

Bound every wait and every output. Never echo secrets. Do not retry an ambiguous side effect (a message
that may have been sent); retry once, bounded and only if time remains, for a definite device rejection
that is safe to repeat.

## Step 5 - credentials

Never put tokens, passwords or API keys in a setting, a default, the manifest, a binding (bindings are
plain text in `layers.conf` and are exported), a log or a `message` / `error`. Use a token file the USER creates:

```sh
mkdir -p ~/.config/sdeck-plus/plugins/<id>
install -m 600 /dev/null ~/.config/sdeck-plus/plugins/<id>/token
$EDITOR ~/.config/sdeck-plus/plugins/<id>/token   # paste the token only
```

In the plugin: path `$XDG_CONFIG_HOME/sdeck-plus/plugins/<id>/token` (fallback `$HOME/.config/...`), read at
run time; require a regular file, not a symlink, owned by the user, mode 0600 (no group or other bits), and
a small size bound. On failure name the path and the rule, never the contents. Document it in the README
and declare it as a `file` requirement.

Reference plugins (local Git projects only, no remote published; read them as examples):
`sdeck-plugin-discord` (Hyprland shortcuts and a bot message: derives the instance signature, token file,
no retry for the ambiguous send) and `sdeck-plugin-mergbw` (Bluetooth light through `busctl`: stops at 4.5 s,
one bounded retry for a safe-to-repeat device error, fake-runner tests).

## Step 6 - tests first

Write `tests/test_run.py` with standard-library `unittest` BEFORE the action code. Load `run` as a module
(`importlib.machinery.SourceFileLoader`) and call `main(argv, io.StringIO(request), out, environ)` with a
fake runner, opener and clock. No network, no hardware, no real services. Assert at least:

- success is exactly one JSON line ending in a newline, `protocol_version` 1 and the echoed `request_id`;
- failures are `ok:false` with an `error`, and `main` returns 0;
- a malformed `request_id` is not echoed; a wrong `protocol_version` or `type` and a non-object request are refused;
- every invalid setting (unknown key, wrong type, out of range, control characters, not an object) is rejected AND the fake runner was never called;
- an unexpected exception is answered without its text.

```sh
python3 -B -I -m unittest discover -s tests
```

## Step 7 - validate

From an sdeck-plus checkout (needs cargo; works offline). This executes nothing:

```sh
cargo run --offline --example plugin_validate -- <plugin-dir>
```

It prints `PLUGIN_VALIDATE_OK:` (id, version, actions, requirements with a `PATH` check) or one
`PLUGIN_VALIDATE_ERROR:` line per problem. Exit codes: 0 ok, 1 problems, 2 usage. It applies the same
checks as the app's Add plugin flow (the folder name need not equal the id here).

The next form RUNS the plugin once with the user's privileges, in a private temporary plugins folder where
it is enabled only there. Use it only with the user's OK and harmless settings:

```sh
cargo run --offline --example plugin_validate -- <plugin-dir> --invoke say-hello \
    --settings '{"name":"Ada"}' [--timeout 1-60] [--request-id <id>]
```

It prints `PLUGIN_INVOKE_OK:` or `PLUGIN_INVOKE_ERROR:`. Without a checkout, check by hand: the manifest is
valid JSON with only the keys above, `run` has mode 0755 and a working shebang, there are no symlinks, at
most 256 files, and the unit tests pass.

## Step 8 - integrate or hand off

Prefer, in this order:

**(a) The user adds it in the app.** The binding toolbar has an icon-only **Add plugin** button (+)
right before the **Update plugins** icon at its right end. The dialog has a **Folder | Repository**
selector (it follows what is typed and only changes the hints and the Browse button) and takes an
absolute folder path (or `~/...`; a relative path is refused) or an `https://` repository address,
then **Check plugin**. The app copies or
downloads it into a private temporary folder, validates it with the validator's code and shows a preview;
it runs nothing. **Install** places it in the plugins folder DISABLED. If the id is already installed the
button reads **Replace**, which swaps the files and disables the plugin until the user presses Enable
again. Problems are listed (the dialog shows the first 8 problems).

Repository rules: HTTPS only, public, no credentials, at most 512 characters, no query or fragment. The
system `git` makes a shallow clone (`--depth 1`) of the default branch's latest commit only. The plugin
must be at the repository ROOT (a sub-folder cannot be installed from the address). Limits: 256 files,
8 MiB per file, 32 MiB in total, 8 directory levels; a 60 s deadline and a 32 MiB ceiling for the
download (`.git` counts). Keep the default branch always passing the validator.

**(b) Manual copy, only if the user asked you to install it.** Copy `manifest.json`, `run` (keep mode 0755)
and any helper files to `<data dir>/sdeck-plus/plugins/<id>/`, where the data dir is `$XDG_DATA_HOME` (if
absolute) or `$HOME/.local/share`. The folder name must equal the id. Then tell the user to press the
**Update plugins** icon (right end of the binding toolbar) and tick **Enable** themselves.

**(c) Leave the project in place** and print the exact instructions for (a) or (b).

You must NEVER enable the plugin. Finish by stating what the user must do next: Add plugin (or Update
plugins), read `run`, tick Enable, assign the action to a control, press **Test action**.

## Common mistakes

| Mistake | Fix |
|---|---|
| `hyprctl` fails: `HYPRLAND_INSTANCE_SIGNATURE` is missing under the cleared environment | Derive it from `$XDG_RUNTIME_DIR/hypr/` |
| A slow Bluetooth connect exceeds the 5 s deadline | Bound work to 4.5 s, report it, do not block |
| Banner, debug print or second line on stdout | stdout is exactly one JSON line; diagnostics go to stderr |
| `request_id` changed or dropped | Echo the received id unchanged |
| `protocol_version` written as `true`, `1.0` or `"1"` | The integer `1` |
| `run` not executable | `chmod 755 run`; installs keep the execute bit as 0755 |
| Symlinks in the project | Real files only |
| `requirements` rejected by an older app build | Update the app, or omit the field |
| Repository holds the plugin in a sub-folder | `manifest.json` and `run` at the repository root |
| Secrets in settings, defaults or messages | Token file, mode 0600 |
| `shell=True` or an interpolated command string | argv list, validated values |
| Trusting settings because the form validated them | A hand-edited binding can hold any JSON type: validate again |
| Non-zero exit or traceback on failure | `ok:false` and exit 0 |
| Retrying a send that may have succeeded | No retry on ambiguous side effects |

## Final checklist

- [ ] Layout complete: `manifest.json`, executable `run` (0755), README with Dependencies, LICENSE, tests; no symlinks
- [ ] Manifest within every limit above; `requirements` declared and mirrored in the README
- [ ] Every setting validated before any action; no shell; no secrets in settings, logs or answers
- [ ] One JSON line out, `request_id` echoed, exit 0 even on failure, work bounded under 4.5 s
- [ ] Tests written first and passing with `python3 -B -I -m unittest discover -s tests`, no network or hardware
- [ ] `plugin_validate` printed `PLUGIN_VALIDATE_OK` (or the by-hand checks are listed as done)
- [ ] The plugin was not enabled, run for real or published by you

## Output contract

Report plainly and briefly:

1. Files created or changed (paths) and where the project lives.
2. Validator result (command and the `PLUGIN_VALIDATE_OK` / `ERROR` line) and any `--invoke` run the user allowed.
3. Tests run: command and counts (passed / failed / skipped).
4. What was NOT verified (real device, service, desktop, network, an actual install in the app).
5. Next steps for the user: the integration route chosen in Step 8 and the explicit Enable.

Never claim hardware or service behaviour you did not exercise.
