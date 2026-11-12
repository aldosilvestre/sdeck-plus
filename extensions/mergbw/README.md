# MeRGBW Hexagon Light demo extension

A demonstration plugin for sdeck-plus (protocol v1, Python 3 standard library only) that writes
MeRGBW-style packets to a Bluetooth LED hexagon light.

> **The protocol is taken from the reference implementation and is UNVERIFIED on a Hexagon Light
> TG609.** Nothing proves the TG609 speaks it, and the developer ran **no hardware test**. The
> tests use a fake `busctl` only. Success messages say packets were *written*; the light's state
> is never confirmed (the reference never reads notifications, and neither does this plugin).

## Credit

The packet format comes from the documented protocol of
[jmbwell/mergbw](https://github.com/jmbwell/mergbw) (HEXAGON profile), MIT licensed, Copyright (c)
2025 John Burwell. No code or scene table was copied; the packet builder is written from the
documented facts.

## Actions

Every action has the required text setting `address` (`AA:BB:CC:DD:EE:FF`) and works on a key or a
dial press.

| Action | Extra settings | What is written |
|---|---|---|
| `power-on` / `power-off` | none | One power packet. |
| `set-color` | `hue` 0-359 (0), `saturation` 0-100 (100), `brightness` 0-100 (optional, no default) | Power on, then colour. A brightness packet follows only when you set `brightness`, so choosing a colour leaves the current brightness alone. |
| `set-brightness` | `brightness` 0-100 (100) | One brightness packet; power state untouched. |
| `set-scene` | `scene` 1-117 (2) | Power on, then the scene packet and a second packet with the reference's default value `0x3200`. The reference is uncertain what that second packet means, so it is not a setting. |

Percentages are sent as value x 10.

## Requirements

- BlueZ with the system `busctl` (systemd). No Bluetooth library is used.
- The light must already be **known to BlueZ**, for example paired with `bluetoothctl`. The plugin
  never scans, pairs, trusts, removes or disconnects anything.
- The official MeRGBW app must be closed: the light accepts one connection at a time.

## Limits

- The light drops its Bluetooth link when idle, so the first press after a pause reconnects and
  may take longer. Readiness is always decided from a fresh BlueZ read (connected **and**
  services resolved) before the first write, never from a snapshot taken before connecting.
- A write the light rejects with ATT error `0x0e` (seen right after a reconnect) is retried
  **once** per press, after another fresh readiness check, and only if at least 1 s of the 4.5 s
  budget is left; the success message then says "after 1 retry". Timeouts, other errors and
  connect failures are never retried.
- Connecting and resolving services can exceed the host's 5 s request limit on the **first press**;
  the plugin stops at 4.5 s, and a later press then works.
- The plugin does not disconnect afterwards, so the light stays connected to this computer and the
  official app cannot connect until you run `bluetoothctl disconnect <address>`.
- Only the GATT characteristic `0000fff3-...` is written, with BlueZ choosing the write type. The
  notify characteristic is never subscribed.
- No credentials are involved.

## Install

Copy this folder to `<data dir>/sdeck-plus/plugins/mergbw/` (usually
`~/.local/share/sdeck-plus/plugins/mergbw/`), keep `run` executable, rescan plugins and enable
**MeRGBW Hexagon Light**. Process separation is **not** sandboxing: the plugin runs with your
privileges, so read `run` before enabling it.

## Tests

```sh
python3 -B -I -m unittest discover -s extensions/mergbw/tests -v
```
