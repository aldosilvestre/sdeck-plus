# sdeck-plus

![Platform: Linux](https://img.shields.io/badge/Platform-Linux-FCC624?logo=linux&logoColor=black)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> [!NOTE]
> This project is still under construction.

sdeck-plus is a GTK4 control desk for the Elgato Stream Deck + (VID `0x0FD9`, PID `0x0084`) on Arch Linux. Configure actions and live information for eight keys, four encoders and touch-strip gestures, organize profiles into folders and pages, style button text, route audio and connect to OBS Studio.

## Screenshots

The 26 captures in [`docs/screenshots`](docs/screenshots) show the current
native GTK application with isolated demonstration profiles and artwork.
Input feedback and live readings are deterministic demonstrations; capturing these
images does not operate a device, change audio output or connect to OBS.

Regenerate the complete set from a graphical session with:

```sh
GDK_BACKEND=x11 STREAMDECK_DOC_SCREENSHOTS_DIR="$PWD/docs/screenshots" \
  cargo test --offline --bin sdeck-plus \
  tests::documentation_screenshots::regenerate_documentation_screenshots_when_explicitly_requested \
  -- --exact --nocapture --test-threads=1
```

**Main window.** Connection status, profile tabs, page navigation, the
hardware preview with the assigned artwork, and the binding editor for the
selected control.

![Main window with Key 1 selected](docs/screenshots/main-window.png)

The same native window in the light palette:

![Main window in the light theme](docs/screenshots/main-window-light.png)

**Swap keys.** Drag a key onto another key in the preview to exchange their
saved action, background image and text styling on the current page. An empty
key exchanges its empty configuration too, so this also moves a configured key
to a free position. Dropping on the same key or cancelling changes nothing.
The exchange is saved automatically; it never runs either action. Other
profiles, pages, dials and touch zones are unchanged. Unsaved editor drafts
are not part of the exchange.

**Selected control and live input.** The control being edited has a thick violet
ring. When you press a key or dial on the device it gets a green outline and
brightens while held; a dial turn flashes amber, and a touch tap, hold or flick
lights the touched strip zone in cyan. The highlight is drawn on the control
itself, over any assigned artwork, so it stays visible on keys and zones with
images.

![Key press, dial turn and touch zone highlighted over assigned artwork](docs/screenshots/live-input-feedback.png)

**Action types.** The **Action** menu offers layer/page switching, shell
commands, launching applications, keyboard shortcuts, legacy events, the
categorized **More actions**, **Play sound** and **None**.
**None** keeps a saved binding but does nothing when pressed. It can still
display live information and does not schedule a page return.
Untouched buttons start with **None**, including buttons on new profiles and pages.

![An untouched key with None selected](docs/screenshots/default-none.png)
![Action menu with every action type](docs/screenshots/action-types.png)

**Configuration menu.** The top-right **Menu** contains **Help**,
**Import configuration...** and **Export configuration...**.
Help opens the setup guide. Export writes a single `.sdeck-plus.json` file
containing all saved profiles, folders, pages, bindings, artwork filenames, button text,
live-information settings, page returns, local sound files and OBS settings.
Images are referenced only by filename; their binary data is not embedded in base64.
The current format is version 2: an image entry contains only
`{"path":"assets/filename.png"}`, without a `data` field. Configuration and local
sound-file payloads remain embedded.
Before importing, the named images must already exist in the destination's `assets`
directory (normally `~/.local/share/sdeck-plus/assets`). Missing or invalid images
cancel the import without replacing the configuration. Older exports with embedded
images are still accepted.
The export includes saved OBS credentials and is written with owner-only permissions.
Application executables, shell commands and external document paths remain references;
launcher autostart and tray preferences remain local to the computer.

Import validates the file before replacing the current configuration and asks
for confirmation. The previous configuration is retained in a sibling backup
directory whose path is shown after import. Profiles, previews and the device
are refreshed immediately. Cancelling or selecting an invalid file keeps the
existing configuration. Importing never runs any saved action.

![Menu with Help, Import configuration and Export configuration](docs/screenshots/configuration-menu.png)
![Import confirmation with Cancel as the default response](docs/screenshots/import-configuration.png)

**More actions categories.** **More actions** groups the extended actions
into System, Multimedia, Organization, Device and OBS.

**Change audio output.** Under **More actions > Multimedia**, choose
**Change audio output**, select an output device, then save the binding.
**Refresh** discovers newly connected outputs using `pw-dump`. Pressing the
control switches the default output with `wpctl`; choosing or saving never
changes audio. Bindings store the stable device name, so numeric ID changes
after reconnecting do not select a different device. A disconnected saved
device stays selected and reports an error if used while unavailable.
Choose **Rotate** in the same device selector to advance to the next connected
output on each press, returning to the first after the last. Rotation starts
from the system's current output and checks the available devices each time.

![Change audio output with Rotate selected](docs/screenshots/more-actions-audio-output.png)

![More actions category menu](docs/screenshots/more-actions-categories.png)

**Return to a page afterwards.** A non-None binding can return to a selected
page after a delay: tick **Return to a page afterwards** under the action, then pick the
destination page (listed with its profile and folder, such as `Base` or
`Studio / Page 2`) and the delay in seconds (1–86400). Pressing the key again
restarts the wait; **Device > Cancel timer** cancels it. The destination is
exactly the page you picked. Saving a binding does not start the wait.

![Binding with a ten-second return to the selected page](docs/screenshots/page-return.png)

**Live information on keys and dials.** Under the action of a key or a dial,
tick **Show live information** and pick what to show. The icon, bar and text are drawn directly over the
key's own image, with transparent empty space and no background of
their own, and update by themselves: a
speaker with a volume bar (crossed out and greyed while muted), the audio output
device, Stream Deck brightness with a bar, time and date, weather with its icon,
CPU, memory and temperature with a bar that turns amber and red under load, or
the OBS state (recording, streaming and scene). Only the key or zone that shows
a value is redrawn, and the binding's action still runs as usual. Each touch section can show
it too, for its Tap or Hold binding. A dial's section shows the information of
its press binding first, then its turns, then a tap or hold on that section.
Flick, swipes and whole-strip gestures cannot show information. Weather uses Open-Meteo through `curl`: enter a city or
`latitude,longitude`; the lookup needs internet access and shows `--` when it
fails. Volume and audio device use `wpctl`. The previews in the window show the
same picture as the device. On the touch strip, transparent areas reveal the
existing strip background instead of adding a black rectangle to the section.

![Demonstration clock, CPU, memory, temperature, volume and touch-section readings](docs/screenshots/live-information.png)
![Live-information selector and weather location field](docs/screenshots/live-information-settings.png)

**More actions binding.** A key bound to **OBS > Toggle recording**: each
action shows only its own fields and a short description of what it does.

![Key bound to an OBS More actions entry](docs/screenshots/more-actions-obs.png)

**Encoder events.** Each dial has independent **Press**, **Clockwise** and
**Counterclockwise** bindings.

![Event menu for Dial 1](docs/screenshots/encoder-events.png)

**Touch strip gestures.** Selecting the strip configures its gestures (tap,
hold, flick and directional swipes) and targets the strip background artwork.
The strip has four sections, one above each dial. Choose **Touch**, then the
control number **1** to **4**, and give each section its own **Tap** and
**Hold** binding, all at the same time. **Any** keeps the older whole-strip
bindings, which still work for taps that no section binding claims. Flick and
swipes always apply to the whole strip.

Touchscreen bindings, section information and strip/section images belong to
**the profile**, not its keypad pages or folders. Changing a keypad page or
opening a folder does not change the selected touchscreen page. Dial bindings
and their live-information settings still belong to the keypad page.

Existing configurations migrate automatically: each distinct complete touch
configuration in a profile becomes an independent touchscreen page. Equal
configurations share one page; existing actions, None bindings, delayed returns,
section information and image references are preserved. The active keypad
page's touch configuration is selected initially. Legacy records are retained,
and migration is not repeated on restart. Invalid touchscreen configuration
blocks saving rather than replacing the original file. Keypad and touch files
are both staged before publication. If the final touchscreen replacement fails,
the previous layer file is restored; a rollback error retains its layer backup
and reports the recovery path instead of silently claiming success.

**Touchscreen pages and swipe navigation.** Select the touch strip or a section
to show **Touch page** above its editor. Use the page selector and arrows to
browse independently, or **New touchscreen page** (+) to add and select a blank
page. Each profile remembers its selected touch page, including after restart.
There are up to 64 touch pages per profile, separate from the keypad layer limit.

On each touch page, select **Swipe left**, then **More actions > Organization >
Next touchscreen page**, and save. Assign **Previous touchscreen page** to
**Swipe right** in the same way. These actions are offered only for touchscreen
controls; the existing Next/Previous page actions still navigate keypad pages.
Neither touchscreen navigation action wraps at the first or last page. Existing
swipe assignments are kept, not replaced automatically. Saving a navigation
binding does not run it. A configured delayed return still targets its chosen
keypad page; it does not select a touch page.

![Touch strip selected in the editor](docs/screenshots/touch-strip.png)

**Button text.** **Add text** opens the text editor: font, size, orientation
and alignment, with a live preview of the key composition. Word-like icons select
left, center or right and top, center or bottom. Hover for a tooltip; the controls
also retain distinct accessible names and keyboard focus.

![Button text editor with a live preview](docs/screenshots/button-text-editor.png)

**Artwork and touch zones.** Choose any of the thirteen artwork targets for
the active layer and assign or clear its image.

![Artwork target selector](docs/screenshots/artwork-touch-zones.png)

**Profiles.** **Organize** creates profiles, associates a profile with an
application and deletes the selected profile. Pages and folders are managed
from the page bar instead.

![Organize panel, Edit profile task](docs/screenshots/organize-profiles-pages.png)
![Organize panel, New profile task](docs/screenshots/organize-new-profile.png)

**Create profile** stays disabled until the name is non-empty and unused, and a
duplicate name is explained while you type. **Save changes** is enabled only
when the application rule differs from the saved one; Base cannot be changed or
deleted. The same panel in the light palette, used when the system GTK theme is
not dark:

![Organize panel in the light theme](docs/screenshots/organize-light-theme.png)

**Application profiles.** Each profile is a tab with its own pages, bindings
and artwork. Here the **OBS** profile, associated with the `obs` application,
shows its own keys.

![OBS application profile tab](docs/screenshots/application-profile-tab.png)

**OBS Studio.** Local WebSocket endpoint and masked password, with a
connection test that only queries OBS state.

![OBS settings panel](docs/screenshots/obs-settings.png)

**Setup help.** Choose **Menu > Help** for the three-step guide: select a control,
configure an action, then save or test it.

![Setup help dialog](docs/screenshots/setup-help.png)

## Requirements

- Rust stable toolchain and Cargo.
- GTK 4.8 or newer development files and a C toolchain. On Arch Linux: `sudo pacman -S gtk4 base-devel`.
- A desktop tray supporting StatusNotifierItem (for example, the Omarchy tray).
- Stream Deck + connected over USB for live device events.
- Permission to open the device's HID node. If access is denied, use the distribution's supported device-access mechanism (for example, an administrator-installed narrowly scoped udev rule for this VID/PID with `TAG+="uaccess"`), then reconnect the device. The application does not change host permissions.
- Action-specific tools: `xdg-utils` for opening files/websites, `wireplumber` (`wpctl`) for PipeWire volume and output selection, `pipewire` (`pw-dump`) for output discovery, `curl` for weather, `wl-clipboard` for Wayland text pasting, `procps-ng` for closing a named process, and `mpv` for soundboard playback. Keyboard and mouse-wheel actions require effective `/dev/uinput` access; media transport requires an MPRIS-capable player. Missing tools or access are reported; the app never installs packages or changes permissions.

## Build and run

Run these commands from the `sdeck-plus` checkout:

```sh
cargo run
```

The UI displays connection state, eight keypad states, four encoder button/rotation states, and Touch Strip Tap, Press, and Flick coordinates in Elgato's 800 × 100 logical-pixel space. Touch gestures are reported after the interaction completes, not as separate contact start/release events. If the device is absent, inaccessible, or disconnected, the UI remains open and reports the state; it retries detection after connection failures.

## Install for the current user

After installing the build requirements above, run:

```sh
./install.sh
```

The installer builds the native release binary using `Cargo.lock` and installs:

- Executable: `~/.local/bin/sdeck-plus`.
- Application-menu launcher: `~/.local/share/applications/dev.streamdeck.EventMonitor.desktop`.
- Icon: `~/.local/share/icons/hicolor/scalable/apps/sdeck-plus.svg`.
- MIT license: `~/.local/share/doc/sdeck-plus/LICENSE`.

`PREFIX` overrides the executable prefix, and an absolute `XDG_DATA_HOME`
overrides the desktop-data directory. `CARGO_TARGET_DIR` controls build output.
Run `./install.sh --help` for usage. The installer can be invoked from any
working directory; it refuses to run as root and does not install system
packages, change device permissions, enable autostart or launch the application.
Re-running it updates the installed files.

After installing, it reports anything that needs your attention:

- The executable directory is missing from `PATH`.
- A running sdeck-plus still uses the previous executable.
- **Start at login** points at another executable; disable it and re-enable it
  from the installed application to update the saved path.
- Optional action tools (`mpv`, `wpctl`, `wl-copy`, `xdg-open`) are missing.
- A connected Stream Deck + cannot be opened by your user; it prints the udev
  rule an administrator can install, without running it.

To remove the installed files:

```sh
./install.sh --uninstall
```

Uninstalling keeps configuration, profiles, artwork and any **Start at login**
entry, and does not stop a running instance. Use the same `PREFIX` and
`XDG_DATA_HOME` values that were used to install.

## System tray and session startup

Closing the configuration window hides it. Device communication, bindings,
artwork replay, timers, and app-owned playback remain active until **Quit**.
Left-click or right-click the tray icon to open the same menu:

- **Open configuration** reopens the existing configuration window.
- **Start at login** enables or disables per-user session autostart; its checkmark reflects the saved state.
- **Start in tray** (on by default) makes a normal launch begin hidden in the tray instead of opening the configuration window; launching again while the app runs still opens it. The choice is saved in `tray.json` beside the close preference.
- **About** opens the native sdeck-plus dialog with its version, description, author, copyright, and complete MIT license.
- **Quit** stops the application and removes its tray icon.

Autostart uses `$XDG_CONFIG_HOME/autostart/sdeck-plus.desktop`,
or `$HOME/.config/autostart/sdeck-plus.desktop` when XDG config
home is unset or relative. It starts the current executable with `--background`,
without opening configuration. Keep that executable at the same path, or
disable and re-enable autostart after moving it. For a quiet manual startup:

```sh
cargo run -- --background
```

Repeated launches reuse the running application and configuration window.
If connecting to the session bus fails, configuration remains accessible and
reports the error; closing it exits instead of hiding an unreachable application.
When autostart precedes the desktop tray, the notifier waits for the host.

### Existing configuration

The application, Cargo package and executable are named **sdeck-plus**.
The application ID remains stable so repeated launches reuse the same instance.
The XDG data directory is now `sdeck-plus` and the autostart entry is
`sdeck-plus.desktop`. When upgrading from the previous name, quit the old
application and move its data directory to the new path before launching
to retain profiles, artwork and OBS settings. Disable the previous autostart
entry and re-enable **Start at login** with the new executable.
Existing files are not migrated automatically.

## Graceful shutdown

**Quit** discards pending artwork uploads, removes UI polling, turns off display brightness, clears all eight keys and the full touch strip to black, and joins the HID worker to release the device. Application shutdown repeats this cleanup safely as a fallback. A synchronous HID call already admitted before cancellation may finish; shutdown waits for it, so a blocked driver call can delay exiting. Only the final display cleanup reports are sent after cancellation; failures are logged without preventing device release.

Saved artwork and profiles are preserved. On the next launch, brightness is restored to 100% and saved artwork is replayed. Unplug/reconnect retries and artwork replay remain active while the application is running, even with configuration hidden; hiding configuration does not turn off or clear the device. Quitting cancels the timer and explicitly stops the action worker, which releases app-owned soundboard playback. Independently opened applications remain open.

## Local artwork

### Button text

Select a key and click **Add text** in its configuration panel. Once text is
saved, **Edit text** reopens the same popup with the saved settings.
Choose an installed font, a size from 6 to 120 pixels, horizontal or vertical
orientation, and separate horizontal and vertical alignment. Vertical rotates
the whole text 90 degrees clockwise; new lines are supported. Text is white
with a black outline and is drawn over the background image, or over black
when no image is assigned. Oversized text is clipped to the key.

The enlarged preview and the key preview show the same composition sent to
the device. **Save text** applies and persists the settings for that key in the
active profile/page and closes the popup. Closing without saving discards
draft changes. **Remove text** keeps the
background image; clearing the background keeps the text. Text settings are
restored when reopening the application or switching profiles/pages.

### Background images

Select **Artwork target (active layer)** in the right settings panel, then use the shared **Image…** or **×** (Clear artwork) controls. All thirteen targets are available: eight keys, the Touch Strip background, and four touch zones. Clicking a key or touch zone also selects its artwork target. Clicking an encoder selects only its action binding; use the artwork selector explicitly to choose an image region. PNG and GIF files are explicitly available in the picker; a separate filter includes all image formats supported by the installed GdkPixbuf loaders. GIF artwork is uploaded as a static still image; animation is not supported by the hardware JPEG upload format. Artwork previews preserve aspect ratio and are center-cropped to fill their surfaces. Images are center-cropped to cover the device target and sent as correctly sized JPEGs over USB HID. The eight keys use 120 × 120 images; the Touch Strip background uses 800 × 100 and each encoder overlay uses a 200 × 100 region. The background and zone images can be layered. A failed import leaves the current assignment unchanged. **×** (Clear artwork) removes the local assignment; clearing a key sends a black key image, while clearing a Touch Strip image rebuilds the strip from its saved background (or black) and remaining zone images. It does not issue a separate device clear command.

Images are copied into the application's XDG data directory, so the original files may be moved or deleted after assignment. The assignment manifest is stored beside the copied assets. The default path is `$XDG_DATA_HOME/sdeck-plus` (normally `~/.local/share/sdeck-plus`); when `XDG_DATA_HOME` is unset, the application uses `$HOME/.local/share/sdeck-plus`. Artwork is independent per layer. Legacy global assignments migrate into the active layer (or `Base` if unavailable); other layers start empty. Selecting a layer refreshes all thirteen previews and replaces the device's complete image state: unassigned keys become black, and the strip is rebuilt from its background (or black) followed by assigned zones. The same state is replayed after reconnect, so empty layers clear previous images. A chooser saves to the artwork target and layer captured when it was opened, even if you change the selected control, artwork target, or layer before accepting; acceptance is rejected if that layer was removed, even if another layer was created with the same name. Storage open or migration failures show a repair message and disable artwork writes and layer removal rather than saving an empty fallback. Layer removal is aborted if artwork cleanup cannot be persisted. Upload failures are shown in the connection status; local assignments remain saved if a device write fails.

## Application layers and actions

Keypad/dial layers and legacy action bindings are stored in `layers.conf` in the same app data directory. Profile-owned touch pages and their independent selection are stored in `touchscreen.json`; artwork remains in its separate assignment store. Select a profile tab and use **Organize** to create, associate or delete a profile. The built-in `Base` profile and its first page cannot be removed; legacy files that named the protected layer `default` are migrated to `Base`.

### Pages and their folders

**New page** asks for a name before creating anything. Cancel leaves the
configuration unchanged. The adjacent pencil renames the current page;
the red trash button deletes it after confirmation, including its owned
folders, bindings and artwork. Renaming preserves its identity and contents.
The compact previous, next and parent icons have accessible labels and tooltips.

The folder icon in the page bar opens **Page folders**. Create a folder,
select one of this page's folders, then **Open folder**, **Link to control**
or **Delete folder**. Linking saves navigation for the selected physical
control and event without opening the folder. Deleting removes only this
page's folder and its local links; other pages can keep independent folders
with the same name. **Parent** returns to the exact owning page.

Organization metadata writes version 2 with stable page-owner keys. Existing
version-1 folders attach to their former parent view's first surviving page
(page 1 when present). Names, backing keys, bindings and artwork are retained.
Invalid metadata remains untouched and disables organization writes.

![New page name prompt](docs/screenshots/new-page-name.png)
![Rename current page](docs/screenshots/rename-page.png)
![Folders owned by the current page](docs/screenshots/page-folders.png)

To configure an action, choose the layer at the top, then select a physical control:

1. Click one of the eight buttons (two rows of four), the horizontal touch strip, or one of the four encoders beneath it. Clicking an artwork zone also selects the whole touch strip; zones are image regions, not separate gesture bindings.
2. Pick an event valid for that control: button press; encoder press/clockwise/counterclockwise; or touch tap, press-and-hold, generic swipe/flick, and directional swipes.
3. Pick an action and fill only its visible fields. The existing layer, command, application, shortcut and legacy-event choices remain available. **More actions** opens typed category-specific fields described below. Shortcuts accept a full chord such as `CTRL + D` or one key with optional modifier checkboxes.
4. Click **Save binding** to create or update that control event, or **Remove binding** to clear it. A floating toast confirms success only after the binding is written to disk; validation or saving problems show an error toast. Success disappears after five seconds; errors remain until dismissed or replaced by another notification. Saving never executes the action. Selecting an already configured control event loads its saved action for inspection and editing.

Hover a physical control to see assignments for the selected keypad page or profile touchscreen page, including **Unassigned** events: encoders include **Press**, **CW** (clockwise), and **CCW** (counterclockwise); the touch strip includes tap, hold, flick, and directional swipes. Legacy exact-tick rotation overrides appear separately. Long action text is shortened; select the event to inspect its full saved fields. Summaries refresh after saving/removing bindings, selecting/creating/removing layers, and input-driven **Move to layer** actions. Selection highlighting is separate from live pressed-state highlighting, and live input does not replace assignment summaries. The shared **Image…** and **×** edit the explicitly selected artwork target, independently of the binding editor.

### Extended actions on Arch Linux

Choose **More actions**, a category, and an action. Saving or inspecting an
assignment never executes it. **Test action** explicitly runs the selected
action and can affect the desktop, device or OBS; test only the action you
intend to perform.

For a sound button, choose **Play sound** directly in **Action**, then use
**Choose file...** or enter an audio file path and click **Save binding**.
WAV, MP3, FLAC, OGG, M4A/AAC, Opus and other formats decoded by mpv are supported.
Choosing a file or saving never starts playback; press the assigned control or
use **Test action** to play it. Playback replaces the app's previous sound.
**More actions > Multimedia > Stop audio file** stops only app-owned playback.

| Category | Actions |
| --- | --- |
| System | Alternate two shortcuts, paste Unicode text with optional Enter, open files/folders or HTTP(S) websites, close an exact process basename, scroll the mouse wheel |
| Multimedia | Play/pause, stop, next/previous track, PipeWire volume up/down and mute, change audio output, play a local audio file, stop app-owned soundboard playback |
| Organization | Next/previous/specific keypad page, enter a named child folder, return to the parent; touchscreen controls also offer next/previous touchscreen page |
| Device | Absolute or relative brightness, sleep and wake; timer actions show countdown and completion |
| OBS | Select a scene, toggle recording/streaming, toggle input mute, show/hide a named source in a scene |

Text uses the Wayland clipboard and a physical Ctrl+V chord, not synthetic
Unicode keycodes. It replaces clipboard contents and requires the focused
application to support Ctrl+V. Closing an application targets its exact Linux
process basename (1-15 ASCII bytes), not a window title or a shell pattern.
Volume uses the default PipeWire output in 5% steps, capped at 100%.
Media transport uses the session's MPRIS player interface. Soundboard playback
belongs to this app and stopping it does not stop unrelated media players.

The four dials support independent press, clockwise and counterclockwise
assignments. For example, assign volume up/down to rotation and mute to press;
use relative brightness actions for another dial. Mouse wheel directions are
vertical or horizontal. The touch strip retains tap, hold and directional
swipe assignments. Device brightness is the Stream Deck's display brightness,
not monitor backlight. Sleep sets brightness to zero and wake restores the
remembered level; before an explicit absolute level, relative/wake actions use
a 100% baseline because the device protocol has no brightness readback.

Profiles appear as native tabs, with **Base** always available. Create another
profile in **Organize → New profile** using **Profile name** and **Create profile** (disabled until the name is new). Pages
belong to their profile; folders belong to individual pages and do not create additional profile tabs.
Switching tabs restores that profile's last selected page for the current session.

To make a profile application-specific, open **Organize → Edit profile**, choose it under
**Profile to edit**, enter a window class or executable name under
**Auto-switch application (optional)**, and click **Save changes** (enabled once the field differs from the saved rule). Question-mark
icons provide extended help on hover or click. Both save buttons confirm
success only after writing the configuration, and report disk errors. For example,
`kitty`, `firefox`, or `obs` can match executable names; a precise window class
can distinguish windows belonging to the same executable. Leave the field
empty to disable automatic selection. All configured profiles remain visible and
manually selectable, including when their associated application is not focused.
Base cannot require an application. Unsaved application input survives focus changes.

Application detection uses Hyprland's IPC focus events, not running-process
polling. A matching external application selects its profile;
unmatched or unavailable focus returns to Base. When configuration itself
has focus, the last external application context remains active. Closing that
external window clears its retained context. A manual tab choice is retained
until the next changed external application context. Selecting a profile
never launches its application. Associations survive restarting the app.

Use **New page** beside the page indicator or **New folder** in **Page folders**. Every view has
its own existing layer-backed bindings and artwork. Navigation stays within
the current profile and folder; the indicator shows the current page.
To rename the current page, click the pencil next to **New page**, edit
**Page name**, and click **Save name** (or press Enter). Names must be nonblank,
contain no control characters, and fit within 64 UTF-8 bytes. Surrounding
spaces are trimmed. The saved name appears in the page indicator and survives
restarts; page numbers, navigation, bindings and artwork remain unchanged.
`organization.json` stores this hierarchy beside `layers.conf`. Existing flat
layers load as independent profiles. Invalid metadata is retained and
organization writes are disabled until repaired. Legacy `brightness-v1` and
`scroll-v1` action records now load as the corresponding typed actions.

### OBS Studio

In OBS, enable its built-in WebSocket server under **Tools > WebSocket Server
Settings**. In this app, click **OBS** in the profile bar, enter the local endpoint
(normally `ws://127.0.0.1:4455`) and password, then save and test the connection.
The connection test queries actual scenes and recording/streaming state; it
does not start either output. Scene/source/input names must match OBS.

OBS operations run on the worker with bounded network waits and surface
authentication, connection and request failures in the UI. Only local
loopback endpoints are accepted. Credentials are stored in owner-only
`obs.json`, not action bindings, and are masked in the editor and diagnostics.
The application does not enable OBS's server or alter its configuration.

### Window layout

At the default 1000 × 700 logical-pixel client size, the device canvas stays on the left, action settings on the right, and layer controls above. Longer action forms and smaller windows can scroll vertically. The documentation captures use 1000 × 950 to show complete forms. Live physical states and connection/error indicators remain. Summaries and long layer names are ellipsized; tooltips show full summaries and the editor retains full saved action fields.

The window uses 16 logical pixels of outer padding, charcoal surfaces in dark
mode and clean pale-gray surfaces in light mode, following the desktop GTK theme.
The complete configuration desk—header, profile tabs, page bar and editor—is
centered at its natural content width on wide displays, keeping toolbar actions
near their task instead of stretching empty bars across the window. At 640 pixels
it uses the available width and stacks the preview and editor without horizontal
scrolling. Hardware proportions stay fixed; forms keep their natural height.
Violet save actions and cyan keyboard focus stand apart from the quieter chrome.
Profile tabs use a violet tonal selection with a fine accent edge, and the **Appearance** group
separates button text and artwork from action editing. **Save binding** only
persists the binding; **Test action** runs it immediately.

Profile tabs and page navigation have a 12-pixel gap; the main preview and
editor regions have 16-pixel separation. The editor uses 16-pixel padding
and 6-pixel gaps between related form groups; Appearance uses 4-pixel internal
gaps. Executable/arguments and shortcut/modifiers share
adaptive columns, stacking when their available width is too small. Modifier
checkboxes use a two-by-two grid, and hidden fields do not leave empty cells.

Organization, OBS and help panels keep their title and **Close** button visible
above scrollable content. The text popup retains its full-size preview and
native font chooser. Brief hover and press transitions follow GTK's animation
setting and stop when animations are disabled.

The physical preview uses a graphite chassis, pale branding, eight black keys,
a continuous touch-strip bezel and four dimensional neutral-gray knobs in both themes.
Its fixed physical coordinates and artwork geometry are unchanged. The artwork strip keeps its 8:1 ratio;
transparent zones preserve strip-background layering. No sample artwork or audio
values are injected by the running application; documentation fixtures supply
their own demonstration data. Selection, live state, keyboard focus and connection status
remain distinct.

The serialized production GTK test checks the shared editor's thirteen assign/clear paths, explicit target selection, chooser ownership, reference geometry, and all five actions at 1000 × 700 with empty, loaded, and cleared artwork and long editor values, without moving scroll adjustments. It also checks 1840 × 980, 1280 × 720, and 960 × 640 horizontal fit separately. Reduced heights (including 960 × 640) may scroll vertically; 640 × 480 retains two-axis scroll reachability. Larger system fonts can also require scrolling. Window managers may override the requested startup size; no desktop resolution change is needed.

Launch arguments are passed as distinct process arguments; no shell evaluates launches. In the application-argument box, an empty box means zero arguments, `\\empty` means one empty argument, and multiple empty arguments are written on separate `\\empty` lines. Literal backslash, newline, carriage return, and tab are escaped as `\\\\`, `\\n`, `\\r`, and `\\t`, so leading/trailing spaces are preserved instead of trimmed. Command actions intentionally run `/bin/sh -c` with the configured shell string. External actions execute serially on a background worker, and failures appear in the UI without blocking GTK. Internal events remain on the in-process `MonitorEvent::AppEvent` channel and are safely consumed without a display panel. Action, persistence, and artwork-upload errors appear in the connection detail. Bindings do not inherit or fall through between layers.

### Physical keyboard actions (Linux)

In the **Shortcut** editor, select **Key names and aliases** to open the native
reference. It lists the parser's accepted aliases and canonical physical keys,
with selectable text for copying. Closing the reference preserves the current
shortcut draft and never saves or executes it.

**Shortcut** uses a Linux uinput virtual keyboard, not compositor shortcut dispatch, shell commands, desktop bindings, or text typing. With no modifiers, it sends one complete key down/up. With modifiers, it holds them while the main key goes down/up, then releases them in reverse order. For example, `CTRL + D` holds Control during D; `SUPER + RETURN` holds Windows/Meta during Enter. Modifiers are released after each action, not kept held between bindings. Failed sends attempt release cleanup and broken devices are discarded.

- Full chord text and checkbox modifiers are merged and deduplicated in CTRL, SHIFT, ALT, SUPER order. Checkboxes may all be empty. Names are case-insensitive; `Control` = `CTRL`, `Win`/`Windows`/`Meta` = `SUPER`, and `Return` = `Enter`. Uppercase letters do not implicitly add Shift.
- Saved legacy **Press key** actions keep working and appear in the editor as **Shortcut** with no modifiers. Inspection never rewrites saved records or key names; explicitly saving converts that binding to a canonical Shortcut. Unsupported legacy names remain visible unchanged, but validation rejects unsupported explicit saves.
- New bindings save canonical modifier/key fields and reject unknown names, empty segments, multiple main keys, or modifier-only chords. Supported keys include ASCII letters/digits, F1–F24, Enter, Tab, Escape, Space, navigation, named punctuation, keypad and media keys. Structurally valid legacy names remain loadable and unchanged; unsupported names report an actionable execution error, not a destructive migration.
- Names identify physical evdev positions. The active desktop keyboard layout determines resulting symbols; this is not Unicode text entry and does not promise indistinguishable hardware behavior.
- Multimedia shortcuts accept `XF86AudioLowerVolume`, `XF86AudioRaiseVolume`, `XF86AudioMute`, `XF86AudioMicMute` (`mute_mic`), `XF86AudioPlay` (`play`), `XF86AudioPause` (`media_pause`), `play_pause`, `XF86AudioNext` (`next_track`), `XF86AudioPrev` (`previous`), `XF86AudioStop`, `XF86AudioRecord`, `XF86AudioRewind`, `XF86AudioForward` and `XF86Eject`. Display brightness accepts `XF86MonBrightnessUp`/`XF86MonBrightnessDown`; keyboard lighting accepts `XF86KbdBrightnessUp`/`XF86KbdBrightnessDown`/`XF86KbdLightOnOff`. These names are case-insensitive and work with optional modifiers. Existing `PAUSE`, `NEXT` and `STOP` retain their keyboard/navigation meanings; use the media names for transport controls. These are physical key events: desktop bindings determine their effects, so separate Play and Play/Pause key codes may still resolve to the same desktop action.
- The serial layer worker owns one lazy, persistent keyboard. Startup and non-keyboard actions do not open uinput. First use requires effective access to `/dev/uinput` and kernel uinput support. Access/create/write failures are reported; there is no fallback or automatic privilege escalation, module loading, permission change, or desktop configuration change. Ask an administrator to assess narrowly scoped access if needed; a device node or udev tag alone does not prove effective permission.
- Creation has a bounded 200 ms discovery grace period, not proof that the compositor has recognized the keyboard. Real access, first-action delivery, compositor acceptance and layout behavior remain unverified on hardware. Automated tests use mocks only and never open or send to a real keyboard.

The versioned layer file is validated on load. An unreadable file or unsupported
header leaves startup configuration unavailable with writes disabled; the app
does not save an empty `Base` fallback over it. Repair the file and restart.
Within a readable version-1 file, invalid, conflicting and duplicate records are
removed while valid assignments are retained. Their original lines and reasons
are appended to `button-errors.log` in the app data directory, and the cleaned
file is saved. Missing layer-switch and page-return targets are rejected; removing
a layer also removes references to it. Legacy exact rotation bindings remain
supported and take priority over directional rotation, while the GUI edits
clockwise/counterclockwise bindings.

## In-process event API

`src/event_api.rs` defines the typed `MonitorEvent` boundary: `Connection(ConnectionState)` for searching/connected/unavailable/disconnected state, `Input(protocol::Event)` for decoded HID input, and `ArtworkResult` for hardware upload results. `start_hid_monitor` returns an owned `HidController` with an artwork sender, explicit cancellation, and idempotent joined cleanup. It owns HID initialization, interruptible retry waits, timed input reads, and serialized image writes on one producer thread. Consumers ignore the `Heartbeat` liveness probe; losing the event consumer also ends the worker. GTK submits complete desired artwork snapshots through an in-process channel; only the newest pending snapshot is applied, replacing obsolete disconnected state. GTK consumes input/upload results with a short GLib timer, so device I/O does not block rendering or input handling. This phase has no TCP/HTTP/WebSocket listener or external API server. A future external transport can subscribe behind this event boundary without changing protocol decoding or UI event semantics.

## Development checks

```sh
cargo fmt --all -- --check
env -u DISPLAY -u WAYLAND_DISPLAY GDK_BACKEND=x11 cargo test --offline
cargo clippy --offline --all-targets --all-features -- -D warnings
```

The headless run covers the Rust suites but skips display-dependent GTK bodies.
Run native GTK cases separately in a graphical session so each process owns its
GTK main thread. For example, this case checks every text-alignment combination,
the shared preview, save/reload and draft cancellation:

```sh
GDK_BACKEND=x11 cargo test --offline --bin sdeck-plus \
  tests::button_text_alignment_icons_update_preview_and_save \
  -- --exact --nocapture --test-threads=1
```

The capture command above drives the production widgets without HID/action
workers. These checks do not replace real chooser, accessibility or device
verification.

The protocol decoder validates report ID and declared little-endian payload length before decoding supported input commands. Hardware event behavior requires a connected, accessible Stream Deck +.

### Opt-in real-device checks

Close other instances before these checks so one production HID worker owns
the device. The first command sends real feature reports: brightness 50%,
relative adjustment, sleep and wake. It leaves the remembered brightness at
50%. The second waits up to 60 seconds for a physical key press, dial rotation
and touch-strip gesture.

```sh
cargo run --offline --example verify_streamdeck -- --device-controls
cargo run --offline --example verify_streamdeck -- --input
```

To check a running local OBS instance without starting recording or streaming:

```sh
cargo run --offline --example verify_obs -- "$HOME/.local/share/sdeck-plus"
```

The OBS check authenticates, queries scenes/output state, then reselects its
already-current scene. Configure `obs.json` through the app first.

The other opt-in checks paste exact Unicode into their own GTK window and
exercise mpv on an isolated D-Bus session. The multimedia fixture must be a
local silent audio file; it requires the `mpv-mpris` plugin.

```sh
cargo run --offline --example verify_desktop
dbus-run-session -- cargo run --offline --example verify_multimedia -- /path/to/silent.wav
```

## Code organization

| File | Responsibility |
| --- | --- |
| `src/main.rs` | Application startup, configuration window, worker lifecycle and GTK-owner fixtures |
| `src/feature_runtime.rs` | Serialized action/configuration work, profiles, page returns and runtime feedback |
| `src/config_bundle.rs` | Versioned configuration export, local image references, validation and backup/restore |
| `src/info.rs`, `src/info_render.rs` | Live readings and the shared key/touch-section compositor |
| `src/desktop_actions/audio_output.rs` | Fixed and rotating PipeWire output selection |
| `src/tray.rs` | StatusNotifier menu, GTK command dispatch and native About dialog |
| `src/autostart.rs` | Per-user XDG session startup entry |
| `src/ui.rs` | Shared native controls, feedback and secondary panels |
| `DESIGN.md` | Native layout, interaction and accessibility contract |

Keep device work off the GTK thread. Closing configuration must not stop
bindings, polling or timers; application shutdown owns cleanup. GTK widget
checks must keep a single owning test thread, even when test helpers move
to separate files. Run the development checks above after changing either
production code or its tests.

## License

sdeck-plus is distributed under the [MIT license](LICENSE).
Copyright (c) 2026 Aldo Silvestre. The license also appears in **About**.
