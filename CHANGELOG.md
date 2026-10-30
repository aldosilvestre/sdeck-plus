# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). No release has been
tagged yet, so everything below is unreleased (package version `0.1.0`).

## [Unreleased]

### Changed

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
