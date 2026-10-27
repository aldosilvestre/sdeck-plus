# sdeck-plus

![Platform: Linux](https://img.shields.io/badge/Platform-Linux-FCC624?logo=linux&logoColor=black)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> [!NOTE]
> This project is still under construction.

sdeck-plus is a native GTK4 control desk for the Elgato Stream Deck +
(Stream Deck Plus; VID `0x0FD9`, PID `0x0084`) on Linux, developed and tested on
Arch Linux with Hyprland. It configures the eight keys, four dials and touch
strip, shows live information on them and runs desktop, media, audio and OBS
Studio actions.

**Website and full documentation:** <https://aldosilvestre.github.io/sdeck-plus/>

![sdeck-plus main window](docs/screenshots/main-window.png)

## Why

Elgato's own software does not run on Linux. sdeck-plus gives Linux desktop
users a native, local application for every control of the Stream Deck +,
with no account, cloud service or system-wide changes required. It runs in the
tray, keeps working with its window closed and never installs packages or changes
permissions on its own.

## Features

- **Every control:** eight keys, four dials with press and clockwise/counterclockwise
  bindings, and touch-strip sections with tap, hold, flick and swipes.
- **Profiles, pages and folders:** profile tabs switched automatically by the
  focused application (Hyprland), plus touchscreen pages navigated by swiping.
- **Artwork, button text and action icons** on keys, the touch strip and its zones.
- **Live information:** volume, audio output, time, weather, CPU, memory,
  temperature, device brightness and OBS state, updated on the device.
- **Actions:** commands, applications, keyboard shortcuts, media and volume
  control, audio-output switching, sounds, brightness, timers and page returns.
- **OBS Studio:** switch scenes, toggle recording and streaming, mute inputs and
  show or hide sources over the local WebSocket server.
- **System tray** with optional start at login.
- **Light and dark GTK themes** following the desktop.
- **Configuration backup and restore** through a single export file.

## Installation

Requirements: the Rust stable toolchain, a C toolchain, `pkg-config`,
`glib-compile-resources`, GTK 4.8 or newer, libudev development files and a
StatusNotifierItem tray. On Arch Linux:

```sh
sudo pacman -S gtk4 base-devel
```

Install for the current user (no root; files go under `~/.local`):

```sh
git clone https://github.com/aldosilvestre/sdeck-plus.git
cd sdeck-plus
./install.sh
```

Remove it with `./install.sh --uninstall`; your configuration is kept. To try it
without installing, run `cargo run` from the checkout.

Your user needs permission to open the device's HID node. If access is denied,
an administrator can install a udev rule scoped to this VID/PID with
`TAG+="uaccess"` (the installer prints one when needed). Some actions need
optional tools such as `mpv`, `wpctl` or `wl-copy`, and keyboard actions need
`/dev/uinput` access. See [Install](https://aldosilvestre.github.io/sdeck-plus/#install)
for the full list and installer options.

## Documentation

- [Features](https://aldosilvestre.github.io/sdeck-plus/#features) and
  [Actions](https://aldosilvestre.github.io/sdeck-plus/#actions): what each part does.
- [Configuration](https://aldosilvestre.github.io/sdeck-plus/#configuration): controls,
  touchscreen pages, profiles, artwork, OBS setup, data files, tray and startup.
- [Development](https://aldosilvestre.github.io/sdeck-plus/#development): checks,
  documentation screenshots, real-device checks and code organization.

The site is published from [`docs/`](docs/index.html); changes are tracked in
[CHANGELOG.md](CHANGELOG.md).

## License

sdeck-plus is distributed under the [MIT license](LICENSE).
Copyright (c) 2026 Aldo Silvestre. The license also appears in **About**.
