#!/usr/bin/env bash
set -euo pipefail

mode=install
case ${1:-} in
    --help|-h)
        printf '%s\n' \
            'Usage: ./install.sh [--uninstall]' \
            'Build and install sdeck-plus for the current Linux user.' \
            '--uninstall removes the installed files; configuration and profiles are kept.' \
            'PREFIX sets the executable prefix (default: $HOME/.local).' \
            'XDG_DATA_HOME sets desktop data (default: $HOME/.local/share).' \
            'Requires Rust, a C toolchain, GTK >= 4.8, libudev and GLib build tools.'
        exit 0
        ;;
    --uninstall)
        mode=uninstall
        shift
        ;;
esac
if (( $# != 0 )); then
    printf 'Unknown argument: %s. Use --help for usage.\n' "$1" >&2
    exit 2
fi
if [[ $(uname -s) != Linux ]]; then
    printf 'sdeck-plus requires Linux.\n' >&2
    exit 1
fi
if (( EUID == 0 )); then
    printf 'Run install.sh as your regular user, without sudo: it installs per-user files.\n' >&2
    exit 1
fi

: "${HOME:?HOME must be set}"
prefix=${PREFIX:-"$HOME/.local"}
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
if [[ $data_home != /* ]]; then
    data_home="$HOME/.local/share"
fi
config_home=${XDG_CONFIG_HOME:-"$HOME/.config"}
if [[ $config_home != /* ]]; then
    config_home="$HOME/.config"
fi
if [[ $prefix != /* ]]; then
    printf 'Installation paths must be absolute.\n' >&2
    exit 1
fi

project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
binary_path="$prefix/bin/sdeck-plus"
desktop_file="$data_home/applications/dev.streamdeck.EventMonitor.desktop"
icon_theme_dir="$data_home/icons/hicolor"
icon_file="$icon_theme_dir/scalable/apps/sdeck-plus.svg"
license_file="$data_home/doc/sdeck-plus/LICENSE"
autostart_file="$config_home/autostart/sdeck-plus.desktop"

note() {
    printf 'Note: %s\n' "$*"
}

refresh_desktop_caches() {
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q "$data_home/applications" ||
            note 'could not refresh the desktop database; the launcher may appear after the next login.'
    fi
    # Only an existing cache can go stale; creating one for a user theme is unnecessary.
    if [[ -f $icon_theme_dir/icon-theme.cache ]] &&
        command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f "$icon_theme_dir" ||
            note 'could not refresh the icon cache; the icon may appear after the next login.'
    fi
}

note_running_instance() {
    if command -v pgrep >/dev/null 2>&1 && pgrep -u "$EUID" -x sdeck-plus >/dev/null 2>&1; then
        note "$1"
    fi
}

if [[ $mode == uninstall ]]; then
    rm -f -- "$binary_path" "$desktop_file" "$icon_file" "$license_file"
    rmdir -- "${license_file%/*}" 2>/dev/null || true
    refresh_desktop_caches
    printf 'Removed sdeck-plus from %s and %s.\n' "$prefix/bin" "$data_home"
    printf 'Configuration, profiles and artwork were kept.\n'
    if [[ -e $autostart_file ]]; then
        note "a Start at login entry remains at $autostart_file; delete it to stop session startup."
    fi
    note_running_instance 'sdeck-plus is still running; choose Quit in its tray menu to stop it.'
    exit 0
fi

# Desktop string escaping is decoded before Exec argument escaping.
exec_path='"'
for (( index = 0; index < ${#binary_path}; index++ )); do
    character=${binary_path:index:1}
    case "$character" in
        \\) exec_path+='\\\\' ;;
        '"'|'`'|'$') exec_path+="\\\\$character" ;;
        '%') exec_path+='%%' ;;
        $'\n') exec_path+='\n' ;;
        $'\r') exec_path+='\r' ;;
        $'\t') exec_path+='\t' ;;
        [[:cntrl:]])
            printf 'Unsupported control character in installation path.\n' >&2
            exit 1
            ;;
        *) exec_path+="$character" ;;
    esac
done
exec_path+='"'

for tool in cargo rustc cc pkg-config glib-compile-resources install; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'Required build tool not found: %s. See README.md requirements.\n' "$tool" >&2
        exit 1
    fi
done
if ! pkg-config --atleast-version=4.8 gtk4; then
    printf 'GTK development files >= 4.8 are required. See README.md.\n' >&2
    exit 1
fi
if ! pkg-config --print-errors --exists libudev; then
    printf 'libudev development files are required. See README.md.\n' >&2
    exit 1
fi

host_target=$(rustc -vV | while IFS= read -r line; do
    if [[ $line == 'host: '* ]]; then
        printf '%s' "${line#host: }"
    fi
done)
cd -- "$project_dir"
build_dir=${CARGO_TARGET_DIR:-"$project_dir/target"}
printf 'Building sdeck-plus for %s...\n' "$host_target"
cargo build --release --locked --bin sdeck-plus \
    --target "$host_target" --target-dir "$build_dir"

install -Dm755 "$build_dir/$host_target/release/sdeck-plus" "$binary_path"
install -Dm644 "$project_dir/assets/icons/scalable/apps/sdeck-plus.svg" "$icon_file"
install -Dm644 "$project_dir/LICENSE" "$license_file"
install -d "$data_home/applications"
desktop_temporary="$desktop_file.tmp.$$"
trap 'rm -f -- "$desktop_temporary"' EXIT
{
    printf '%s\n' \
        '[Desktop Entry]' \
        'Type=Application' \
        'Name=sdeck-plus' \
        'Comment=Configure an Elgato Stream Deck + on Linux'
    # GIO validates the first executable before expanding filename field codes.
    printf 'Exec=/usr/bin/env %s\n' "$exec_path"
    printf '%s\n' \
        'Icon=sdeck-plus' \
        'Terminal=false' \
        'Categories=Settings;HardwareSettings;' \
        'StartupWMClass=dev.streamdeck.EventMonitor'
} > "$desktop_temporary"
chmod 644 "$desktop_temporary"
mv -f -- "$desktop_temporary" "$desktop_file"
refresh_desktop_caches

printf 'Installed executable: %s\nDesktop launcher: %s\n' "$binary_path" "$desktop_file"
printf 'Launch from your application menu or run: %q\n' "$binary_path"

case ":${PATH:-}:" in
    *":$prefix/bin:"*) ;;
    *) note "$prefix/bin is not in PATH; add it to run 'sdeck-plus' by name." ;;
esac
note_running_instance 'a running sdeck-plus still uses the previous executable; quit it from the tray and start it again.'
if [[ -f $autostart_file ]] &&
    ! grep -Fxq -- "Exec=$exec_path --background" "$autostart_file"; then
    note 'Start at login points at another executable; disable and re-enable it from the installed application.'
fi

missing_tools=()
for entry in \
    'mpv:sound playback' \
    'wpctl:volume and mute' \
    'wl-copy:text pasting' \
    'xdg-open:opening files and websites'; do
    if ! command -v "${entry%%:*}" >/dev/null 2>&1; then
        missing_tools+=("${entry%%:*} (${entry#*:})")
    fi
done
if (( ${#missing_tools[@]} > 0 )); then
    note 'optional tools not found; their actions will report an error until installed:'
    printf '  - %s\n' "${missing_tools[@]}"
fi

for uevent in /sys/class/hidraw/hidraw*/device/uevent; do
    if [[ -r $uevent ]] && grep -qi '^HID_ID=[0-9A-F]*:00000FD9:00000084$' "$uevent"; then
        node=${uevent#/sys/class/hidraw/}
        node="/dev/${node%/device/uevent}"
        if [[ ! -r $node || ! -w $node ]]; then
            note "your user cannot open the Stream Deck + at $node. An administrator can grant access with:"
            printf '%s\n' \
                "  echo 'SUBSYSTEM==\"hidraw\", ATTRS{idVendor}==\"0fd9\", ATTRS{idProduct}==\"0084\", TAG+=\"uaccess\"' | sudo tee /etc/udev/rules.d/70-sdeck-plus.rules" \
                '  sudo udevadm control --reload-rules' \
                '  Then reconnect the device.'
        fi
        break
    fi
done
