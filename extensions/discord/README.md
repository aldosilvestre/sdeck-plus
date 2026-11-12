# Discord demo extension

A demonstration plugin for sdeck-plus (protocol v1, Python 3 standard library only). It has
three actions:

| Action | Controls | What it does |
|---|---|---|
| `toggle-mute` | key, dial press | Sends a shortcut (default `CTRL SHIFT` + `M`) to the Discord window. |
| `toggle-deafen` | key, dial press | Same, default `CTRL SHIFT` + `D`. |
| `send-message` | key | Posts a message to a channel as your own Discord bot. |

## Limits

- The shortcut actions need **Hyprland** (`hyprctl dispatch sendshortcut`, no shell, no focus
  change). The host does not pass Hyprland's session variable on, so the plugin finds the running
  instance in `$XDG_RUNTIME_DIR/hypr/` and uses the most recently modified one when there are
  several (it fails with "no running Hyprland instance found" if there is none). The window selector defaults to `class:^(discord|vesktop|legcord)$`.
- They only report that the shortcut was **sent**. Nothing confirms that Discord received it or
  that your microphone or deafen state changed. Set the same shortcut in Discord's keybinds.
- `send-message` needs a bot you create and authorize yourself in the Discord developer portal,
  invited to the server with permission to post in the channel. There is no retry.
- Nothing here has been verified against a live Discord or a real Hyprland session; the tests use
  fakes only.

## Install

1. Copy this folder to `<data dir>/sdeck-plus/plugins/discord/` (usually
   `~/.local/share/sdeck-plus/plugins/discord/`). `run` must stay executable.
2. Open the app, rescan plugins and enable **Discord**. Process separation is **not**
   sandboxing: the plugin runs with your privileges, so read `run` before enabling it.

## Bot token

The token is never a binding setting and is never stored by the app. Create the file yourself:

```sh
mkdir -p ~/.config/sdeck-plus/plugins/discord
install -m 600 /dev/null ~/.config/sdeck-plus/plugins/discord/token
$EDITOR ~/.config/sdeck-plus/plugins/discord/token   # paste the token only
```

The path is `$XDG_CONFIG_HOME/sdeck-plus/plugins/discord/token`, falling back to
`$HOME/.config/...`. It must be a regular file (not a symlink) owned by you, with no group or
other permissions, at most 512 bytes. Otherwise the action fails and names the path and the rule,
never the contents. Exports and bundles never contain the token.

## Tests

```sh
python3 -B -I -m unittest discover -s extensions/discord/tests -v
```
