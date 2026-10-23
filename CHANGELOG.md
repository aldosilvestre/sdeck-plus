# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). No release has been
tagged yet, so everything below is unreleased (package version `0.1.0`).

## [Unreleased]

### Added

- **Touchscreen page indicators.** Left/right edge chevrons show available previous/
  next touch pages on the device and preview, stay visible over artwork/live info,
  and disappear at the corresponding bounds. Existing swipe bindings are unchanged.

- **Profile-owned touchscreen pages.** Touch gestures, section information and
  strip/section images stay independent of keypad pages and folders. Select the
  touch strip to add or select its pages; touchscreen-only next/previous actions
  can be assigned to left/right swipes. Distinct legacy configurations become
  separate touch pages without discarding assignments, and export/import includes
  all touch pages and their local sounds. Dial bindings remain keypad-page-owned.
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
