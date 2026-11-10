#!/usr/bin/env bash
set -euo pipefail

mode=install
pinned_tag=''
case ${1:-} in
    --help|-h)
        printf '%s\n' \
            'Usage: ./install.sh [--uninstall | --tag vX.Y.Z]' \
            'Build and install sdeck-plus for the current Linux user.' \
            'Without a checkout, downloads the latest published vX.Y.Z release (requires curl and tar).' \
            'Release sources and build caches are temporary and removed on exit.' \
            '--tag vX.Y.Z (or --tag=vX.Y.Z) installs exactly that release; not supported in a checkout.' \
            '--uninstall removes the installed files; configuration and profiles are kept.' \
            'PREFIX sets the executable prefix (default: $HOME/.local).' \
            'XDG_DATA_HOME sets desktop data (default: $HOME/.local/share).' \
            'Requires Rust, a C toolchain, GTK >= 4.8, libudev and GLib build tools.'
        exit 0
        ;;
    --tag|--tag=*)
        tag_argument=$1
        if [[ $1 == --tag ]]; then
            pinned_tag=${2:-}
            shift
            if (( $# )); then shift; fi
        else
            pinned_tag=${1#--tag=}
            shift
        fi
        if [[ ! $pinned_tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            printf 'Unknown argument: %s. Use --help for usage.\n' "$tag_argument" >&2
            exit 2
        fi
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

# A script on stdin has no BASH_SOURCE[0], including under set -u.
project_dir=''
if [[ -n ${BASH_SOURCE[0]:-} ]]; then
    project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fi
release_mode=true
if [[ -n $project_dir && -f $project_dir/Cargo.toml &&
      -f $project_dir/LICENSE && -d $project_dir/assets ]]; then
    release_mode=false
fi
if [[ -n $pinned_tag ]] && ! $release_mode; then
    printf 'Unknown argument: --tag (only supported without a checkout). Use --help for usage.\n' >&2
    exit 2
fi
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

# One EXIT handler owns both the atomic desktop temporary and release scratch.
# Never register a second EXIT trap that could discard either cleanup action.
release_temporary=''
desktop_temporary=''
cleanup() {
    if [[ -n $desktop_temporary ]]; then
        rm -f -- "$desktop_temporary"
    fi
    if [[ -n $release_temporary ]]; then
        rm -rf -- "$release_temporary"
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if $release_mode; then
    for tool in curl tar mktemp; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            printf 'Required release tool not found: %s.\n' "$tool" >&2
            exit 1
        fi
    done
    release_temporary=$(mktemp -d)
    # These two overrides are offline test hooks, not user-facing options.
    release_api=https://api.github.com/repos/aldosilvestre/sdeck-plus/releases/latest
    archive_base=https://github.com/aldosilvestre/sdeck-plus/archive/refs/tags
    if [[ ${SDECK_PLUS_INSTALL_TEST:-} == 1 ]]; then
        release_api=${SDECK_PLUS_RELEASE_API:-$release_api}
        archive_base=${SDECK_PLUS_ARCHIVE_BASE:-$archive_base}
    fi
    tag=$pinned_tag
    if [[ -z $tag ]]; then
        if ! release_json=$(curl -fsSL --proto '=https' --proto-redir '=https' \
            --connect-timeout 15 --max-time 60 "$release_api"); then
            printf 'Could not fetch the latest published release. Check curl, network access and GitHub Releases.\n' >&2
            exit 1
        fi
        tag_pattern='"tag_name"[[:space:]]*:[[:space:]]*"([^"]*)"'
        if [[ $release_json =~ $tag_pattern ]]; then
            tag=${BASH_REMATCH[1]}
        fi
    fi
    if [[ ! $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        printf 'Latest release must have a tag of the form vX.Y.Z. A published release is required.\n' >&2
        exit 1
    fi
    archive="$release_temporary/release.tar.gz"
    if ! curl -fsSL --proto '=https' --proto-redir '=https' \
        --connect-timeout 15 --max-time 300 --max-filesize 67108864 \
        --output "$archive" "${archive_base%/}/$tag.tar.gz"; then
        printf 'Unsafe release archive: download failed or exceeds the 64 MiB limit for %s.\n' "$tag" >&2
        exit 1
    fi
    # Also enforce the limit locally, including when a server omits its size.
    archive_size=$(wc -c < "$archive")
    if (( archive_size > 67108864 )); then
        printf 'Unsafe release archive: download exceeds 64 MiB.\n' >&2
        exit 1
    fi
    release_root="sdeck-plus-${tag#v}"
    # Inspect before extraction. Reject links/devices as well as traversal, and
    # use escaped names so a newline cannot masquerade as a second safe entry.
    if ! tar --list --gzip --quoting-style=escape --file "$archive" > "$release_temporary/members" ||
       ! LC_ALL=C tar --list --verbose --numeric-owner --gzip --quoting-style=escape \
           --file "$archive" > "$release_temporary/types"; then
        printf 'Unsafe release archive: cannot list contents.\n' >&2
        exit 1
    fi
    while IFS= read -r member; do
        if [[ ( $member != "$release_root" && $member != "$release_root/"* ) ||
              $member == *\\* || $member =~ (^|/)\.\.?(/|$) ]]; then
            printf 'Unsafe release archive: unexpected path.\n' >&2
            exit 1
        fi
    done < "$release_temporary/members"
    expanded_size=0
    entry_count=0
    # GNU tar's numeric-owner listing puts the decimal size in field three.
    # Bound each number before arithmetic to prevent overflow on hostile headers.
    while read -r permissions owner size remainder; do
        if [[ $permissions != -* && $permissions != d* ]]; then
            printf 'Unsafe release archive: only regular files and directories are allowed.\n' >&2
            exit 1
        fi
        entry_count=$((entry_count + 1))
        if (( entry_count > 5000 )); then
            printf 'Unsafe release archive: more than 5000 entries.\n' >&2
            exit 1
        fi
        if [[ ! $size =~ ^[0-9]{1,9}$ ]] ||
           (( 10#$size > 134217728 - expanded_size )); then
            printf 'Unsafe release archive: expanded size exceeds 128 MiB or is invalid.\n' >&2
            exit 1
        fi
        expanded_size=$((expanded_size + 10#$size))
    done < "$release_temporary/types"
    mkdir -- "$release_temporary/source"
    tar --extract --gzip --file "$archive" --directory "$release_temporary/source" \
        --no-same-owner --no-same-permissions --keep-old-files --delay-directory-restore
    project_dir="$release_temporary/source/$release_root"
    if [[ ! -d $project_dir || ! -f $project_dir/Cargo.toml ||
          ! -f $project_dir/LICENSE || ! -d $project_dir/assets ]]; then
        printf 'Unsafe release archive: missing release source tree.\n' >&2
        exit 1
    fi
    # The archive must be the tagged release: its package version is the tag's.
    package_version=''
    in_package=false
    while IFS= read -r line || [[ -n $line ]]; do
        if [[ $line =~ ^[[:space:]]*\[([^]]*)\][[:space:]]*$ ]]; then
            [[ ${BASH_REMATCH[1]} == package ]] && in_package=true || in_package=false
        elif $in_package &&
             [[ $line =~ ^[[:space:]]*version[[:space:]]*=[[:space:]]*\"([^\"]*)\" ]]; then
            package_version=${BASH_REMATCH[1]}
            break
        fi
    done < "$project_dir/Cargo.toml"
    if [[ $package_version != "${tag#v}" ]]; then
        printf 'Unsafe release archive: Cargo.toml version does not match %s.\n' "$tag" >&2
        exit 1
    fi
    # Keep rustup's existing toolchain/RUSTUP_HOME; isolate Cargo downloads and
    # build output without touching any caller-owned cache or target directory.
    export CARGO_HOME="$release_temporary/cargo-home"
    export CARGO_TARGET_DIR="$release_temporary/target"
    mkdir -- "$CARGO_HOME"
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
