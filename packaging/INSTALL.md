# Installing the prebuilt sdeck-plus binary

This archive is a plain tarball, not a pacman package: `pacman -U` cannot install it.
On Arch Linux use the `sdeck-plus-X.Y.Z-1-x86_64.pkg.tar.zst` file from the same release
(`sudo pacman -U sdeck-plus-X.Y.Z-1-x86_64.pkg.tar.zst`) or the installer.

## Requirements

- x86_64 Linux with glibc 2.39 or newer and GTK 4.8 or newer.
- On older systems build from source with `install.sh` (see the documentation).

## Check the download

```bash
sha256sum -c sdeck-plus-X.Y.Z-x86_64-linux.tar.gz.sha256
```

## Start at login after moving the binary

If **Start at login** was on for a copy that is no longer there (for example
`/usr/bin/sdeck-plus` from a removed pacman package), start the new copy once:
it repoints the login entry at itself. An entry whose executable still exists
is left alone, so remove the old copy first.

## Install for your user (no root)

From the folder this file is in:

```bash
install -Dm755 sdeck-plus ~/.local/bin/sdeck-plus
install -Dm644 dev.streamdeck.EventMonitor.desktop ~/.local/share/applications/dev.streamdeck.EventMonitor.desktop
install -Dm644 sdeck-plus.svg ~/.local/share/icons/hicolor/scalable/apps/sdeck-plus.svg
install -Dm644 LICENSE ~/.local/share/doc/sdeck-plus/LICENSE
```

`~/.local/bin` must be on your `PATH`. Start the app from your launcher or with `sdeck-plus`.

## Device access (once, needs root)

The udev rule lets the logged-in user open every supported model: the Stream Deck +, + XL, Classic, XL, Mini
and Neo (one line per product id):

```bash
sudo install -Dm644 70-sdeck.rules /etc/udev/rules.d/70-sdeck.rules
sudo udevadm control --reload
sudo udevadm trigger
```

Unplug and plug the Stream Deck again if it is already connected.

## Uninstall

```bash
rm -f ~/.local/bin/sdeck-plus \
  ~/.local/share/applications/dev.streamdeck.EventMonitor.desktop \
  ~/.local/share/icons/hicolor/scalable/apps/sdeck-plus.svg \
  ~/.local/share/doc/sdeck-plus/LICENSE
sudo rm -f /etc/udev/rules.d/70-sdeck.rules
```

Your configuration and profiles under `~/.local/share/sdeck-plus` are kept.
