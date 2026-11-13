# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- **Plugin display values (host side):** a plugin manifest may declare `values` (id and label, at
  most 16) and an action may name the one it shows with `display`. A new `query` request asks the
  plugin for it and returns a headline (1-32 characters), an optional detail (at most 64) and an
  optional 0-100 level, under the same limits as an invocation (`PluginRegistry::query`,
  `QueryOutcome`, `PluginError::NoDisplay`). The Add plugin preview data and `plugin_validate`
  list the values, and `plugin_validate --query <action>` runs one query. Saved bindings and
  stores are unchanged; showing the value on keys and the touch strip comes separately.
- **Agent skill file:** `docs/SKILL.md` lets an AI agent write, test, validate and hand off an
  sdeck-plus plugin (it never enables the plugin); `tests/plugin_sdk.rs` checks it against the code.
- **External plugins:** separate programs add actions without recompiling the app. Each valid
  plugin is its own item under a **Plugins** heading of the Actions catalog, with an explicit
  **Enable** check box (nothing is enabled by default), a generated settings form, and an
  **Update plugins** icon in the binding toolbar that rereads the plugin folder. A supervised
  host starts the plugin once per press with a cleared environment, bounded output and a fixed
  5 s deadline, in its own process group that is killed afterwards, on a dedicated worker, and
  never retries. Process separation is **not** a sandbox: plugins run with your privileges (the
  notices now say plainly that no administrator rights are involved).
- Optional manifest `requirements` (commands, services, files): shown in the plugin's sidebar
  item; only commands are looked up on `PATH` and the app never installs or runs anything.
- `plugin_validate` example (`cargo run --offline --example plugin_validate -- <dir>`) checks a
  plugin project without running it; its explicit `--invoke` mode runs the plugin once.
- **Plugin SDK:** a guide for plugin authors (`docs/plugin-sdk.html`, linked from the
  documentation and the site map), the host contract in `docs/plugins.md`, and a copyable starter
  project in `examples/plugin-template`. A test keeps the guide's limits equal to the code's.
  Limits: the Discord plugin has never been verified against a live Discord or Hyprland session,
  and on the MeRGBW light only power-off was observed to work (TG609).
- **Add plugin dialog:** an **Add plugin** button (+) in the binding toolbar, next to
  **Update plugins**, adds a plugin from an absolute folder path or the `https://` address of a repository.
  Before anything is installed the candidate is checked with the validator's rules (manifest,
  executable confinement, limits, no symlinks; missing commands only warn) and previewed with its
  id, version, source, downloaded commit, files, actions and requirements. Install never enables
  the plugin; replacing an installed id needs an explicit **Replace** and disables it until it is
  enabled again. Installs run one at a time and closing the dialog cancels.
- **Shared install library** (`sdeck_plus::plugins::{parse_source, stage, install}`): repositories
  are fetched only over HTTPS with the system `git` (shallow clone of the default branch, no
  submodules, hooks, LFS or user git configuration, 60 s deadline, 32 MiB ceiling, `.git`
  removed); `plugin_validate` now uses the same staging, so both report the same problems.
  Limits: only public HTTPS repositories with the plugin at the repository root; no SSH, branch
  or tag selection, removal from the app or automatic updates. A real HTTPS clone is not exercised
  by the automated tests (a fake git and a local repository only), and keyboard and screen-reader
  behaviour of the dialog was not verified with real input.

### Changed

- The **Plugins** heading of the sidebar is always visible and, with no plugins, shows a hint
  about the toolbar's Add plugin button and the plugins folder.
- The **Add plugin** dialog has a new look: a header row with a Close button, a **Folder |
  Repository** selector that adapts the hints and Browse, and a plugin card (monogram, version, id
  and source chips, files, size and commit, collapsible actions and requirements) with the
  run-with-your-privileges warning and the replace notice pinned above the footer. Errors appear
  in a callout. Behaviour is unchanged.
- The demonstration plugins now live in their own local Git projects, `sdeck-plugin-discord` and
  `sdeck-plugin-mergbw` (no remote repository published yet), each declaring its dependencies.
  Their manifests need a build that knows `requirements`; older builds reject unknown fields.

### Removed

- The `extensions/` folder and the two demo-plugin integration tests (`discord_extension`,
  `mergbw_extension`) are gone from this repository; their coverage moved to the plugin projects'
  own tests and the validator.

## [0.2.0] - 2026-11-10

Support for the Stream Deck + XL and for several Stream Decks at once (package version `0.2.0`).

### Added

- **Stream Deck + XL support** (VID `0x0FD9`, PID `0x00C6`): 36 keys (9x4), 6 dials and a
  1200x100 touch strip with 6 sections. The app detects the connected models automatically.
  The XL is implemented from Elgato's HID documentation and validated with a simulated device;
  **real-hardware validation is pending** because no Stream Deck + XL was available.
- **Several Stream Decks at once**: every connected device runs its own configuration live
  (pictures and input actions). A **Current** selector in the header appears only while two or
  more different models are connected and chooses which model the window configures; with one
  model or none there is no selector. Two devices of the same model share that model's profiles
  and mirror each other. Each model keeps its own profiles, pages, bindings and artwork; the
  Stream Deck + keeps its data directory unchanged and the XL uses
  `<data>/sdeck-plus/devices/plus-xl/`. Only `tray.json`, `updates.json` and the new
  `device.json` (last model shown) are shared. Simultaneous operation was validated only with
  simulated devices; it has not been tested with two real Stream Decks.
- Configuration bundle **format 5** records the model. Importing a bundle made for the other
  model is refused with a clear message.
- A model registry (`src/device_model.rs`, guide in `DEVICES.md`) with the Stream Deck Classic,
  XL, Neo and Mini described from Elgato's HID documentation as *planned*: discovery never
  matches them and they are not supported yet.
- Per-model preview images in the documentation (`preview-plus.png`, `preview-plus-xl.png` and
  `main-window-xl.png`) and a two-device window (`main-window-two-devices.png`), generated by the
  documentation screenshot generator.
- **Volume up** and **Volume down** take a step of 1 to 100 percent (default 5%, as before),
  chosen in the action editor and shown in the binding summary when it is not 5. Dial turns
  still apply one step per event. Bindings with the default step are saved exactly as before;
  a `layers.conf` saved with a custom step cannot be read by older versions (default steps
  remain readable).

### Fixed

- **Remove binding** clears the whole control (a key's press and hold, a dial's press, turns
  and exact ticks, a touch area's gestures) with its page returns and live information, so a
  dial no longer looks configured after Remove; one **Undo remove** restores all of it.
  Applying a dial preset also drops the page return and live information of the events it
  replaces.
  Undo refuses, changing nothing, when the control was edited after Remove.

### Compatibility and downgrade

- Bundles exported by this version are format 5; older versions cannot import them. Older bundles
  (versions 1 to 4) are Stream Deck + bundles and still import into the + configuration.
- The Stream Deck + data directory is unchanged. The XL keeps its own folder under `devices/`;
  importing into the + configuration preserves it.
- The udev rule gains a `00c6` line for the Stream Deck + XL and now lives at `udev/70-sdeck.rules`
  (renamed from `packaging/70-sdeck-plus.rules`; the name no longer mentions a model). The installer prints one
  command that installs that file from the repository, and the AUR packages install it as
  `/usr/lib/udev/rules.d/70-sdeck.rules`. A rule installed by hand as `70-sdeck-plus.rules` keeps working; remove
  it when adding the new file to avoid a duplicate.
- Real-hardware validation of the Stream Deck + XL and of running several devices at once is
  pending; only the Stream Deck + alone has been tested on hardware.

### Changed

- The connection text names the connected models ("Stream Deck + XL connected", "Stream Deck +
  and Stream Deck + XL connected", "Stream Deck + or Stream Deck + XL not connected").
- README, the site, `DESIGN.md`, package descriptions and the desktop file describe both models and
  the XL's validation status.
- The stable AUR PKGBUILD pins the SHA-256 of the published release tarball instead of `SKIP`.

## [0.1.0] - 2026-11-09

First release (package version `0.1.0`).

### Fixed

- Release installer rejects an archive whose `Cargo.toml` package version differs from
  the requested tag (`Unsafe release archive`, nothing built); release tags must match
  the package version. The check ignores a version hidden behind a multi-line string
  (such manifests are rejected) and accepts a `[package] # comment` heading.
- The in-app updater forwards a custom `RUSTUP_HOME` to the installer's cargo, and a
  known newer release stays visible (indicator and Update button) while a later check
  runs or fails.

### Added

- **AUR packaging, prepared but not published.** `packaging/aur/sdeck-plus` (pinned
  release tarball, checksum `SKIP` until the `v0.1.0` tag exists) and
  `packaging/aur/sdeck-plus-git` (VCS source) each carry a PKGBUILD and `.SRCINFO`,
  plus a shared desktop file and a `uaccess` udev rule for the Stream Deck +.
  Nothing was uploaded; `packaging/aur/README.md` lists the release-time steps.

### Changed

- **Updates moved to the menu.** The update controls now live in a modal
  **Menu > Updates** dialog (between Help and Import) instead of the Help dialog,
  and the header "Update available" indicator opens it. Behavior is unchanged;
  the dialog is not scrolled, and long installer output is shortened with an
  ellipsis (full text in the tooltip). Help returns to its setup steps only.

- **Explicit updates from Help.** Default per-user installations now offer Update
  for a detected stable release. The app downloads that tag's installer over
  bounded HTTPS and runs it as a file on an owned worker with a clean environment
  and a pinned `--tag` argument. Checks and duplicate updates are blocked while
  running; failures report the real exit code and bounded output. Quit terminates
  and joins the process group, allowing ten seconds for cleanup. Success offers
  Restart now; the replacement starts only after normal shutdown releases HID
  and the application bus name. Package-manager, checkout and unknown copies
  show guidance instead. Nothing installs or restarts automatically.
  The standalone installer accepts `--tag vX.Y.Z` or `--tag=vX.Y.Z` in release
  mode only; its default latest-release and checkout behavior remain unchanged.

- **Release update indicator.** Help now includes Updates with a manual check,
  status and a saved automatic-check setting (on by default). A newer stable
  GitHub release shows a text indicator beside the connection badge. Checks
  run on an owned worker with bounded HTTPS responses; automatic checks are
  suppressed for six hours after completion and can be disabled. Invalid
  preferences are preserved. Checking never runs an installer and no release data is saved.

- **Install without a checkout.** Run
  `curl -fsSL https://raw.githubusercontent.com/aldosilvestre/sdeck-plus/main/install.sh | bash`
  to download and build the latest published GitHub release (`vX.Y.Z`; a published
  release is required). Downloaded sources, build output and an isolated Cargo
  cache are removed on success, failure or interruption. Only the per-user
  binary, launcher, icon and license remain; existing caches and checkouts are
  preserved. Checkout installation still works. Uninstall with
  `curl -fsSL https://raw.githubusercontent.com/aldosilvestre/sdeck-plus/main/install.sh | bash -s -- --uninstall`;
  configuration and profiles are kept.

- **Artwork target picker lists only touch targets.** Appearance now offers the
  touch-strip background and the four touch zones; key images use the key's
  right-click popover. The documentation text and its screenshot match.

- Documentation site: regenerated all native screenshots for the current interface
  and added captures of the timer countdown on a key and the Hold 2 seconds editor.

- **Optional exclusive two-second key hold.** Configure a key's short action,
  then select **Hold 2 seconds** and assign a separate catalog action, such as
  **Cancel timer**. A short release runs only the short action; reaching two
  seconds runs only the hold action, with no action on release afterwards.
  Catalog assignment to the same key preserves the selected short/hold mode
  instead of overwriting the short action. Removing the hold binding restores
  immediate key-down behavior. Primary artwork and live information remain independent.

- **Countdown on the initiating key.** A timer started with a physical keypad key
  shows its remaining time on that key and the GTK preview (`mm:ss` below one
  hour, `hh:mm` from one hour onwards). It stays on its original page and survives
  artwork refreshes and device reconnects. Completion or cancellation restores normal
  artwork, button text or live information. Test action and non-key controls still
  start timers without claiming a key; completion notifications and chimes are unchanged.

- **Reference-only configuration exports.** Version 4 exports contain no image,
  sound or other file contents, including key, touch-strip and zone backgrounds.
  Configuration entries are readable UTF-8 text, never base64. File targets keep their basenames under
  `./imagenes/`, `./sonidos/` or `./archivos/`, without original parent paths or
  symlink destinations. Import resolves these folders under the local app data
  directory, preserves local files/links and allows missing references. Legacy
  v1/v2/v3 imports remain supported. Duplicate target basenames cancel export;
  command text, application arguments, URLs and OBS credentials remain verbatim.

- **Website redesign.** The GitHub Pages site is now a landing page
  (`docs/index.html`) and a documentation page (`docs/docs.html`) sharing
  `docs/assets/site.css` and `docs/assets/site.js`, with light and dark themes
  (system default plus a remembered toggle) and asymmetric layouts. The landing
  page has an interactive Stream Deck + preview (keys, touch strip with two
  touch pages and dials that describe their actions and animate demonstration
  readings), a capabilities grid, alternating feature rows, a screenshot gallery
  with a keyboard-operable lightbox, tabbed action categories and copy buttons
  on code. Install, Configuration and Development moved to the documentation
  page, which adds a sticky table of contents with scroll-spy and a filter over
  its sections. Old `#configuration` and `#development` links forward to the new
  page, README links point there, the sitemap lists both pages and a 404 page
  was added. Animations respect reduced-motion preferences and every page works
  without JavaScript.
- **README simplified.** The README now covers only what sdeck-plus is, why
  it exists, its key features, installation and one main-window screenshot.
  Configuration, behavior details and development material (checks,
  screenshot regeneration, real-device checks, event API, code organization)
  moved to new Configuration and Development sections of the website, whose
  Install section now lists every requirement and installer option.
- **Button text popup.** A line under the title names the edited key and page
  ("Key 3 on “Everyday / Page 1”"). One 3x3 Position grid replaces the two
  alignment rows, and orientation is a linked pair of icon toggles. The
  preview draws the text on the key's real background: its artwork, else its
  action icon tile, else the neutral key. Save text is enabled only for valid
  changes, Remove text appears only for saved text, an "Unsaved changes" hint
  tracks the draft, Ctrl+Enter saves, and a warning appears when the text does
  not fit the key (saving is still allowed).
- **Page folders popover.** Folders are listed as rows with Open, Link to
  control, Rename and Delete icon actions (tooltips and accessible names
  include the folder name) instead of a dropdown with captioned buttons. The
  create row validates as you type (duplicate and invalid names are explained,
  blank names disable the add button), Enter creates, and creating keeps the
  popover open on the owning page. A page without folders shows an empty
  state.

- **Modern binding icons.** Every icon is redrawn as a duotone (a soft fill
  under a crisp round stroke) with clearer shapes for weak glyphs (toggle
  switch, beamed notes, folder with a front edge, hold progress ring, tap
  burst, snowflakes, sparkle fallback). Keys, touch sections and preview dials
  color icons by family (media violet, audio cyan, OBS red, system amber, web
  blue, navigation white) and replace the hard dark outline of icons and
  captions with a soft drop shadow. Menus keep the theme color.
- **See-through touch section tiles.** The section tile drops to 15% (10%
  over section artwork) and fades towards the bottom, with a subtler edge.
- The app always starts on the Base profile's first page instead of restoring the
  page that was active when it last closed.
- **Main window refresh.** The window fills its width (up to 1240px) and the
  hardware preview scales uniformly into spare room (up to 1.5x) without changing
  hardware coordinates, artwork or the 1000x700 no-scroll layout. The page bar
  groups previous/next/parent before the page name and keeps folders and page
  management after it, with lighter rename/delete icons; Organize and OBS are
  frameless. The header states the connection in plain language with a colored
  badge, moving the USB IDs to its tooltip. Preview dials show their assigned
  action and the touch strip placeholder is easier to read. In the editor,
  shortcut modifiers are one row of Ctrl/Alt/Shift/Super toggles, an Options
  heading groups the return and live-information choices, the binding status sits
  under the action buttons (warning-toned while unsaved) and Appearance has a
  clear heading.

### Added

- **Now playing live information.** Keys, dials and touch sections can show
  the media playing in any MPRIS player: a violet play icon with the title
  (or the player's name) and artist, the same greyed out with a pause icon
  while paused, and a struck-through note with "Nothing playing" when no
  player plays. A playing player wins, then the paused one that played last;
  players are only read, never controlled.
- **Now playing shows the application's icon.** When the playing
  application's icon is installed (found through its desktop entry and the
  icon theme), keys and touch sections show it in full color, dimmed while
  paused, with a small play or pause badge.
- **Scrolling Now playing titles.** A playing title too long for its key or
  touch section scrolls right to left at a readable size (1.5 s pause, then
  30 px/s, looping) instead of shrinking, on the device and in the preview;
  paused titles stay still and ellipsized.
- **Rename page folders** in place from **Page folders** (Enter saves, Escape
  cancels). Layer keys, the folder's pages and nested folders are kept, and the
  owning page's links to the folder follow the new name.

- **Live information matches binding icons.** Live touch sections sit on the
  same translucent section tile as binding icons, with the icon on the left and
  value, bar and caption beside it, keeping room for page chevrons. Live
  readings draw their icons from the binding icon vocabulary (new volume,
  weather, CPU, memory and temperature icons), and key captions such as CPU,
  Memory and Temperature are no longer clipped.
- Automatic captions for website actions show only the URL host, and long
  captions wrap between words with an ellipsis instead of breaking mid-word.
- **Binding icons.** One icon vocabulary covers every action, including media,
  audio, mouse, text, page and touchscreen navigation and OBS. The Action menu
  shows an icon per type, preview dials show their action's icon on the knob,
  and keys and touch sections without artwork show the icon with a caption in
  the preview and on the device. Artwork, live information and button text
  placement are unchanged, and nothing is written to the artwork stores.
- Every delete and remove button now uses the same trash icon; Organize and
  button text have clearer icons.
- **Touch strip icons.** Touch sections without artwork show a larger icon
  beside a two-line caption on a translucent tile instead of a black box,
  leaving room for page arrows. The preview strip shows badges for swipe
  left/right, tap and hold, including the default swipe paging. More actions
  categories, actions and the Action button show specific icons, and the
  Artwork target menu marks each key or section and its assigned action.

- **Delete touchscreen pages.** A trash button beside the Touch page controls
  deletes the selected touch page and its bindings and artwork after confirmation.
  A profile always keeps at least one touch page.
- **Swipes change touch page by default.** Unbound Swipe left shows the next touch
  page and Swipe right the previous one. Any binding on the gesture overrides it.
- **Page-owned dials.** Dial press/turn actions and live information now follow
  each profile's touchscreen pages, not keypad pages or folders. Existing touch
  pages retain their edits and start with a copy of their profile's main-page
  dials; legacy records are kept. Touchscreen storage upgrades to version 2.

- **Touchscreen page indicators.** Left/right edge chevrons show available previous/
  next touch pages on the device and preview, stay visible over artwork/live info,
  and disappear at the corresponding bounds. Existing swipe bindings are unchanged.

- **Profile-owned touchscreen pages.** Touch gestures, section information and
  strip/section images stay independent of keypad pages and folders. Select the
  touch strip to add or select its pages; touchscreen-only next/previous actions
  can be assigned to left/right swipes. Distinct legacy configurations become
  separate touch pages without discarding assignments, and export/import includes
  all touch pages and their local sounds.
  Paired saves stage keypad/touch data before publication and roll back the layer
  replacement when touchscreen publication fails, keeping failed saves restartable.
  A durable recovery journal reconciles saves interrupted between file replacements
  before either store loads, preserving completed pairs and restoring pending pairs.

- **Start in tray.** A tray checkbox, on by default, makes a normal launch
  begin hidden in the tray. Launching again while running opens the window,
  and an unavailable tray still shows it. `tray.json` gains `start_in_tray`;
  files without it are read as on.
- **Live information.** Keys, dial zones and per-section touch bindings can
  show volume, audio output, brightness, time/date, weather, CPU, memory,
  temperature and OBS state. Transparent icons, bars and text share the
  production compositor and preserve the assigned artwork.
- **Configuration import/export.** The top-right Menu includes Help, Import
  configuration and Export configuration. Version-2 exports embed saved
  settings and local sounds, but image entries contain filenames only, without
  base64 image data. Import reuses local assets, validates before replacing,
  retains a backup and still accepts version-1 bundles.
- **Multimedia controls.** Play and stop owned soundboard audio, select a
  fixed PipeWire output by stable name, or rotate through the connected
  outputs on each press. Choosing or saving an output does not switch audio.
- **Delayed page returns.** A non-None binding can return to an exact page
  after 1–86400 seconds. Reusing it restarts the wait; navigation, cancellation
  and destination removal clear pending returns.

- **Swap keys by drag-and-drop.** Drag one keypad button onto another in the
  native preview to exchange their saved action, background and text on the
  current page. Empty keys can be used as destinations. The exchange is saved
  automatically without executing either action.

- **Light theme.** When the host GTK theme is not dark, `src/ui-light.css`
  overrides the `@desk_*` palette tokens, and the switch follows theme changes
  live. The silver hardware preview is identical in both modes.
- **Live input feedback on the device preview.** A held key or dial press gets
  a green outline, a dial turn flashes amber for 300 ms, and a touch tap, hold
  or flick lights the touched strip zone in cyan (the 800 px strip maps to four
  200 px zones). The highlight is drawn on the control above assigned artwork,
  so it stays visible on keys and zones with images.
- **Stronger edit indicator.** The control being configured has a 5 px violet
  ring separated from the hardware by a 2 px light gap.
- **Live profile-name validation.** Create profile is disabled for empty or
  duplicate names and the reason is shown under the field.
- **Native shortcut reference.** The Shortcut action links to a read-only
  dialog listing the canonical physical key names and the accepted aliases,
  taken from the same catalogue the parser uses.
- **Page folder count.** The Page folders menu shows the number of folders
  owned by the current page, including zero.
- **Documentation.** New README screenshots for live input feedback and the
  Organize panel (Edit profile, New profile, light theme), plus README and
  DESIGN.md updates for the points above.
- **Tests.** Native GTK test `control_feedback_tests` drives production input
  events with artwork assigned and checks the highlight classes, their
  clearing, the zone mapping and that the rendered pixels change.

### Changed

- **None by default.** Untouched controls on new profiles and pages start
  with None instead of suggesting a page switch. None can display information
  without executing an action or starting a return timer.
- **Text alignment icons.** Six Word-like line icons replace alignment
  captions while preserving grouped selection, tooltips, accessible names,
  keyboard focus and the shared live preview.
- **Current documentation and captures.** README and the documentation site
  describe the new controls, version-2 image references and configuration
  recovery. The reproducible native generator now captures 26 current
  application views, including both themes, information, audio routing,
  page returns and configuration transfer.

- **Organize popup.** Edit profile and New profile are separate tasks in a
  native stack, so only one occupies space and the popup no longer scrolls at
  640x480 or 1000x700. Long explanations moved from permanent paragraphs into
  question-mark help buttons with tooltips and click/keyboard popovers.
- **Task tabs.** The active tab is filled violet with a heavier weight and the
  inactive one is muted, so the clickable tab is no longer the brighter one.
- **Save changes.** It is enabled only when the application rule differs from
  the saved one, and never for Base.
- **Error messages.** "duplicate layer or layer limit reached" is replaced by
  "A profile named X already exists." and "Profile limit reached. Delete a
  profile to create another." The creation field is cleared after a successful
  create.
- **Page and folder management.** Organize now covers profiles only. Pages are
  created, renamed and deleted from the page bar, and folders belong to a page.
  Organization metadata is written as version 2 with stable parent-page
  references; version 1 folders migrate to their former parent's first
  surviving page.
- **Selection styling.** The old neutral selection wash is replaced by the
  violet ring described above.

### Fixed

- Starting a timer while one is active no longer restarts or replaces it. Short
  taps and other Start timer actions leave its deadline, remaining time and key
  picture unchanged; cancel it or wait for completion before starting another.
- Timer key countdowns now use compact `hh:mm` from one hour onwards and `mm:ss`
  below one hour, preventing long durations such as 7200 seconds from being clipped.
  A `hh:mm` or `mm:ss` label above the digits identifies the current units on
  both the physical key and its app preview, at approximately 60% of the digits'
  fitted font size and without an extra Remaining caption.
- An `hh:mm` countdown now redraws its key and preview once per minute, when the
  shown time changes, instead of rendering and uploading the same picture every
  second.
- Countdown timers now send a native system notification with their duration on
  completion, including while the configuration window is hidden. Cancellation
  and ignored starts do not notify or play sound. Completion also plays a theme alarm
  via optional `canberra-gtk-play`, without interrupting Play audio file actions.
  If the helper is missing or fails, the GTK bell is attempted (possibly silent);
  the completion status and notification remain available.
- A stale error colour could remain on the "Manual selection" status after a
  failed profile creation.
- Opening a page name dialog no longer enabled manual profile selection before
  the name was accepted.

### Fixed

- **Record-level configuration recovery.** Invalid and conflicting records
  in readable layer files are isolated and logged to `button-errors.log`
  while valid assignments remain usable. Unreadable files and unsupported
  headers still preserve the original file with startup writes disabled.
- **Visible runtime feedback.** Progress and error messages no longer hide
  themselves behind the feedback container's inherited visibility.
- **Compact binding forms.** Return and live-information options fit the
  existing 1000x700 layout with the base action fields. Form and Appearance
  spacing is reduced without shrinking text or controls.
- **GTK test initialization.** Headless checks avoid initializing GTK without
  a display; an advertised display must initialize successfully. Native checks
  use the current None and touch-section behavior.

### Verification

- Graphical binary tests run one case per process so GTK retains a single
  owning thread. Popup checks wait for allocated, rendered content.
- Physical HID input and live OBS were not exercised; verification used
  temporary data and command channels.
