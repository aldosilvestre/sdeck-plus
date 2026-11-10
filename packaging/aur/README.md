# AUR packaging (prepared, not published)

Two packages are prepared here. Nothing has been uploaded: no AUR account, tag or release exists yet.

| Directory | Package | Source |
|-----------|---------|--------|
| `sdeck-plus/` | stable | pinned `v$pkgver` release tarball from GitHub |
| `sdeck-plus-git/` | development | `git+https://github.com/aldosilvestre/sdeck-plus.git`, `pkgver()` from `git describe --tags --long` (falls back to `0.r<count>.<shortsha>` while no tag exists) |

`sdeck-plus-git` provides and conflicts with `sdeck-plus`. Both install the binary, the desktop file
(`dev.streamdeck.EventMonitor.desktop`), the icon, the MIT license and a udev rule. They never use `install.sh`
and never install per-user files.

## Why the package ships a udev rule

`packaging/70-sdeck-plus.rules` grants the logged-in user access to the Stream Deck + (`0fd9:0084`) and the Stream Deck + XL (`0fd9:00c6`) through `uaccess`.
The app itself never changes permissions, so the package installs the rule to `/usr/lib/udev/rules.d/`.

## Why `options=('!lto')`

makepkg builds C code with `-flto`. The bundled hidapi object then cannot be linked by `rust-lld`
(`undefined symbol: hid_*`), so LTO is disabled for both packages.

## Before publishing (human steps)

1. Both PKGBUILDs already name the maintainer on their first line; change it there if the maintainer changes.
2. Release `v0.1.0` is published and the stable PKGBUILD already carries its checksum. For a later
   release, set `pkgver`, reset `pkgrel=1` and run `updpkgsums` in `sdeck-plus/`.
3. Check the pinned source: `makepkg --verifysource` in `sdeck-plus/` (downloads the tag's tarball and
   compares it with `sha256sums`).
4. In each package directory, regenerate the metadata: `makepkg --printsrcinfo > .SRCINFO`.
5. For each package, clone its empty AUR repository, copy `PKGBUILD` and `.SRCINFO` into it, commit and push:

   ```bash
   git -c init.defaultBranch=master clone ssh://aur@aur.archlinux.org/sdeck-plus.git
   git -c init.defaultBranch=master clone ssh://aur@aur.archlinux.org/sdeck-plus-git.git
   ```

Bump `pkgver` (matching `Cargo.toml`) and repeat steps 3 and 4 for every later release.

## Checks

`cargo test --offline --test packaging` verifies the repository files (paths, versions, desktop file, udev rule,
`.SRCINFO` consistency) without makepkg or network. `.SRCINFO` must always come from `makepkg --printsrcinfo`.
