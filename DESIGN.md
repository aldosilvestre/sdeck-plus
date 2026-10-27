# sdeck-plus native design system

## 1. Atmosphere and identity

sdeck-plus is a native hardware configuration desk, not a web dashboard. Ink-toned
surfaces, restrained violet actions and cyan interaction cues frame the graphite Stream Deck preview: eight
keys, a touch strip and four circular encoders. Preserve its coordinates,
artwork and existing functions.

Configuration follows a short path: choose a profile, select a control, choose
an action, complete its fields, then save. Secondary organization and artwork
tools use progressive disclosure instead of competing with action editing.

Touchscreen configuration is profile-owned and has its own page selection,
independent of keypad pages and folders. Only the touch inspector shows the
compact **Touch page** selector, boundary-aware previous/next arrows and add
and delete buttons (delete is disabled for a profile's last touch page and asks
for confirmation); it does not change the fixed hardware preview geometry. Each control
retains an accessible name and tooltip. Organization actions distinguish keypad
navigation from touchscreen navigation. Unbound swipe left/right change touch
page by default (next/previous); any explicit binding replaces that default, and
existing swipe assignments are never silently replaced. The app always starts on
the Base profile's first page. Dials follow the same selected touchscreen page, including
press/turn actions, delayed returns and live information. Keypad navigation must
not change them. On a v1 upgrade, copy each profile's main-page dials into its
existing touch pages without replacing touch edits, artwork or legacy records.
Compact white chevrons on dark translucent badges show previous/next availability
at the touchscreen's left/right outer edges. Hide unavailable directions and both
for a single page. Paint them last over section artwork/live information in both
physical output and preview, without resizing controls or changing stored artwork.

References consulted:

- Agency Agents `UI Designer` (`~/.claude/agents/design-ui-designer.md`):
  consistent component states, accessible color combinations and design QA.
- Agency Agents `UX Architect` (`~/.claude/agents/design-ux-architect.md`):
  shared semantic tokens, component boundaries and task-oriented hierarchy.
- Agency Agents `UX Researcher` (`~/.claude/agents/design-ux-researcher.md`):
  explicit usability questions and evidence-based task review. This project
  review is a technical walkthrough, not a participant study.
- Frontend `redesign-skill.md`: audit-first hierarchy and legibility.
- `design/apple.md`: restrained configurator chrome and readable utility text;
  no Apple branding, proprietary fonts or marketing imagery.
- [StyleGallery sidebar](https://github.com/changeroa/StyleGallery/blob/main/patterns/split-sidebar/sidebar.md):
  two task regions that stack when available width is insufficient.
- [beui popover source](https://beui.dev/r/popover/raw): anchored disclosure,
  Escape dismissal and focus restoration. GTK supplies the implementation;
  no web component or motion library is copied.

## 2. Color

Application chrome uses the branded dark palette, and a matching light palette (`src/ui-light.css`) when the host GTK theme is not dark (the dark preference is off and the theme name does not contain "dark"). The light file only redefines the `@desk_*` tokens, primary-action states and two shadows, so both modes share every rule and follow theme changes live. Explicit field, popup, menu and button surfaces prevent
mixed light/dark controls. Native dialogs, fonts and accessibility remain GTK-owned.

| Role | Token |
| --- | --- |
| Shell / base surface | `@desk_bg`, `#15191f` |
| Primary ink | `@desk_ink`, `#f1f4f7` |
| Supporting ink | `@desk_muted`, `#b0bac6` |
| Elevated editor / toolbar | `@desk_surface`, `#20262e` |
| Input surface | `@desk_field`, `#191e25` |
| Hover / pressed surface | `@desk_hover`, `#303943` |
| Neutral divider / edge | `@desk_edge`, `#353e49` |
| Interactive boundary / scrollbar thumb | `@desk_control_edge`, `#76828f` |
| Scrollbar hover / drag | `@desk_muted` / `@desk_cyan` |
| Primary action | `@desk_violet`, `#b6a1ef`, with `@desk_bg` ink |
| Selected tab | `@desk_selection`, `#4e426d`, with `@desk_ink` ink |
| Primary action hover / press | `#c7b6f5` / `#a58cde` |
| Keyboard focus / interaction cue | `@desk_cyan`, `#85dce8` |
| Error ink | `@desk_error`, `#fb7185` |
| Success ink | `@desk_success`, `#7de2b3` |
| Warning ink | `@desk_warning`, `#f9cc7a` |

Light palette values: base `#f3f5f7`, surface `#ffffff`, field `#f7f8fa`, hover
`#e6eaef`, edge `#d7dde4`, interactive boundary `#7a8491`, ink `#242c35`,
supporting ink `#536170`, primary action `#6746bb` (hover `#5939a9`, press
`#4c2f95`), selected tab `#e8e1f6`, focus cue `#0e7183`, error `#c0243f`,
success `#1b7f55`, warning `#9a6a00`. The graphite hardware is identical in both modes.

Status badges pair semantic tones with visible text, a low-alpha fill and a
55%-alpha edge of the same token: warning for Not connected/Disconnected, error
for Unavailable and success for Connected. Color alone never communicates state. Hardware uses a graphite chassis
(`#414951` to `#272d34`), pale branding (`#e1e6eb`), black key/strip material
(`#08090b`), inset key edge (`#515b65`), recessed rail (`#22282e` to `#343c44`)
and neutral encoders (`#68737e` to `#343c44`). Subtle inset lighting gives the
chassis and dials depth without changing their geometry. The control being
edited retains a 5px violet ring (`#7c3aed`) separated from the hardware by a
2px light gap, readable on the graphite chassis and over artwork.
Live input is an inset outline drawn on the control itself, above any artwork:
green `#22c55e` while a key or dial is held, amber `#f59e0b` for 300ms on a
dial turn, and a cyan `#67e8f9` wash over each touched strip zone for 300ms (the
800px strip maps to four 200px zones; a flick lights every zone it crosses).
Keyboard focus keeps its cyan outline. Task tabs use the selected-tab surface
and weight 600; profile tabs add a fine violet inset edge. Inactive tabs are
transparent with supporting ink. Inspector section headings (Options,
Appearance) use primary ink at 13px/700.

Interactive boundaries use their own neutral token rather than the quieter
decorative card edge: at least 3:1 against input and editor surfaces in both
palettes, with at least 4.5:1 for supporting text, selected-tab text and save labels. Scrollbar thumbs use the same token and an explicit solid fill,
so a light desktop theme cannot supply a dark-on-dark gradient. Hover and drag
increase thumb contrast without changing native scrolling geometry.

## 3. Typography

Use the system GTK font. App title: 20px/700. Inspector title: 18px/700.
Section labels: 14px/600. Body, controls and field labels: 14px. Supporting
metadata: 13px. Binding status: 13px/600 primary ink. Existing physical-device
branding keeps its material typography. The preview-only touch placeholder
("Touch strip · waiting for input") is 11px/600 mint on black, and each preview
dial's caption is 10px/600 pale graphite ink; neither carries configuration
instructions or reaches the physical device.
Long summaries ellipsize with full tooltips; editable fields preserve the value.

## 4. Spacing and layout

Default window: 1000x700. Shell margins: 16px. Tokens: 4px label stacks, 8px
compact controls, 12px action/tool gaps, 16px editor padding and
16px primary region gaps. The compact page-navigation card retains 12px padding.
Control radius: 8px; editor/toolbar radius: 14px. Device canvas padding remains
6px so hardware/artwork geometry is unchanged. Icons are 16px, or 20px for
section identity. Popover padding and header/body separation are 16px; text measure is at most 38
characters.

Profile tabs have a 12px gap before page navigation; navigation and the
device/editor region have 16px separation. The device and editor also have
16px layout separation, retained as a vertical gap when they stack.
The editor uses 16px inner padding and 6px between related form groups, keeping
the added return and information options within the default 1000x700 layout.
Event/action selectors and Save/Test/Remove controls have 12px inline gaps.
A small "Options" heading groups the page-return and live-information checks.
Binding feedback sits directly under its action buttons at an 8px vertical gap
in primary ink, while the Appearance separator, a 4px extra top margin and an
ink "Appearance" heading keep action and artwork sections distinct.
That status line reads "Unsaved changes." in the warning tone as soon as any
editor field differs from the loaded or saved binding; loading, saving or
removing resets it.
Selecting another preview control while edits are unsaved asks first with the
compact confirmation style (Keep editing is the default, Discard changes is
destructive). Remove leaves an Undo remove button under that status line until
another control is loaded; it re-saves the binding, its page return and its
live information.
The related controls inside Appearance use 4px gaps so expanded artwork and
the taller shortcut/argument fields still fit the default client height.
Appearance tools and artwork disclosure use 12px gaps without extra empty cards.

Executable and arguments are one related adaptive group: two columns with a
16px gap when each can receive at least 180px, otherwise stacked with 12px
separation. Shortcut input and modifiers use the same adaptive group; modifiers
are one linked row of named toggle buttons ordered Ctrl, Alt, Shift, Super
(4px/10px padding, checked state uses the selected-tab surface with a violet
inset edge, tooltip "Hold X with the key"). A muted line under the row previews the chord
once it parses (for example "Presses Ctrl + Alt + P") and stays hidden otherwise.
Commands and named events occupy a full-width field. Hidden fields also hide
their layout cells, and the group disappears for actions that do not use it,
so unrelated actions never leave an empty layout slot.
Field labels remain associated with their inputs.
This redistribution makes room for breathing space without adding primary
scrolling at the default 1000x700 size.

Secondary panels have 16px between logical sections. Setup help groups each
heading with its explanation at 8px, rather than giving headings and unrelated
steps the same gap. Text editing keeps 16px between form and preview and 12px
between Save/Remove actions; its 120px preview remains unchanged.

The fixed-geometry device chassis sits in its preview card (440x428 at 1x,
including padding and border). The card is presented through `ScaledBin`
(`src/ui/scaled_bin.rs`): one uniform transform between 1.0x and 1.5x that
fits the card into its region's width and height, top-aligned and horizontally
centered. Hardware coordinates, artwork targets and the card's minimum size
never change, so scaling only uses spare room and never adds scrolling; the
card never stretches non-uniformly to match a tall form. GTK applies the same
transform to drawing and pointer picking, so selection, drag-and-drop and
focus keep working. The inspector uses its content height, without an
arbitrary minimum height.
The complete configuration shell (header, profile toolbar, page navigation and
body) fills the available width up to 1240px (1272px including margins), then
centers (`Clamp`, `src/ui/clamp.rs`; GTK 4.8 has no native clamp). These bars
share the shell's leading/trailing edges. The native FlowBox decides wrapping
from natural widths, so the preview's natural width stays unscaled; spare
width reaches both regions through horizontal expansion and spare height
reaches the preview row through vertical expansion. At 1000x700 the preview
is about 1.1x; at 1280x800 and wider it reaches at least 1.25x.
At narrow widths the shell shrinks to the available width and the same native
container stacks the regions; only vertical scrolling is needed at 640px.
These two size-only containers are the only custom layout; there is no resize
polling.

The profile tabs, profile organization menu and OBS menu form a compact toolbar;
Organize and OBS are frameless (hover shows the surface) so they stay secondary
to the tabs. The organization popover contains only profile creation, application
association and profile deletion. The page bar separates navigation from
management: a linked previous/next/parent segmented group leads the
breadcrumb with a 12px gap, and page-owned folders and New page trail it,
followed by frameless square pencil and red-trash icons (still named, with
tooltips; deletion still confirms).
The folder menu has no outer padding, preserving the page bar's 34px controls.
Icon-only actions use square 34px hit areas and centered 16px symbolic icons;
captioned actions use a 6px icon/text gap and the same control height. MenuButton
styling belongs to its internal button, avoiding doubled padding and background
seams. Icon-only actions retain accessible names, tooltips and visible keyboard focus.
New page and rename open contextual native name dialogs; cancellation never
creates a page. Destructive page, folder and profile operations confirm their
specific scope. Folder creation, renaming, linking and deletion belong to the
current page, not the whole profile. Linking persists the selected control's
navigation binding without executing it.
The **Page folders** popover keeps the shared header (title, Close) and reads
top to bottom: a context line "Folders on “page”" with a one-line helper about
linking; a linked create row (entry with the "New folder name" placeholder and
a suggested-action add icon; Enter creates) with live validation beneath it
(blank disables the button silently; duplicate or invalid names disable it and
show the reason in error color); then a boxed list of the page's folders,
sorted by name, scrolling past about 240px. Each 40px row shows a violet
folder icon, the bold name (ellipsized at the end) and frameless trailing
icon actions Open, Link to control, Rename and red Delete, each with a tooltip
and accessible name that include the folder name ("Rename folder Studio").
Activating a row opens the folder. Creating keeps the popover open, clears
the entry and lists the new folder without leaving the page. Rename swaps the
name for a prefilled, selected entry: Enter saves, Escape or moving focus
away cancels, and validation errors stay inline without ending the edit.
Delete still confirms and is unavailable while artwork writes are disabled.
A page without folders shows a dim folder icon and "No folders on this page
yet". Rows use only `@desk_*` tokens, so light and dark themes match, and
keyboard focus draws the cyan focus ring. The physical preview and a minimum-300px inspector sit side by side
when roomy and stack at narrow desktop widths. The outer `ScrolledWindow`
owns primary vertical overflow. A text editor may scroll its own text; native
popovers contain secondary content. No primary field forces horizontal scroll.

## 5. Components and states

- **Card:** neutral tonal separation, readable heading and inline feedback;
  subtle inset top light, no colored accent border.
- **Icon action:** one GTK symbolic icon family at 16px, visible outcome
  label, accessible name and tooltip. Save is the editor's only primary action.
  Destructive actions use the native destructive style, and every delete or
  remove (binding, page, folder, touch page, profile) uses the same trash icon;
  clearing text or artwork keeps the clear icon. Organize uses the manage
  (tools) icon and button text uses the plain text icon.
- **Binding icons:** one vocabulary (`src/binding_icons.rs`) maps every
  action to a duotone vector glyph on a 24-unit grid: a soft fill (35% of
  the ink) under a crisp 1.85-unit round stroke (2.2-2.6 for chevrons, bars
  and the text T), round caps and joins: go to page, next/previous page, folder, parent folder, command,
  application, shortcut, named event, play sound, media transport, volume,
  mute, audio output, scroll, text, open file, website, close application,
  shortcut toggle, timer, cancel timer, brightness, sleep, OBS scene, record,
  stream, microphone mute and visibility, plus `more` for More actions, `none`
  (menus only) and a `generic` fallback (a four-pointed sparkle). The caller
  supplies the ink, so the same picture serves the window (theme foreground
  for both layers, 16px, decorative with the label carrying the name) and the
  device. On keys, touch sections and preview dials the ink is the icon
  family's accent from the `@desk_*` palette: navigation, folders and
  gestures neutral `#f0f2fa`, media violet `#baa3f5`, audio cyan `#85dee9`,
  OBS red `#fa7387`, system and apps amber `#facc7a`, web blue `#7db3fa`,
  weather `#b8c9eb`, load readings green `#7de3b3`. Instead of a hard outline,
  device icons and captions sit on a soft dark drop shadow (three rings of
  12 stamps at 28/15/8% black, 0.6-2.3 units out, dropped 0.6 units), so they
  read over light artwork without a box. The Action menu shows the icon before each row name and keeps the
  check mark on the selected row. Each preview dial draws its caption's icon
  (20px, in its family accent like the device) on the knob face; it never takes clicks and never
  reaches the device. A key with no artwork of its own and no live information
  shows the icon (50px) with the action summary in up to two 14px bold lines
  below it, on black. A touch section with no artwork shows a see-through
  graphite veil (rounded 14px, 15% alpha at the top fading to half at the
  bottom, light edge fading 12% to 4%) over the strip, so strip artwork shows
  through and around it and legibility comes from the shadowed icon and text, with a 58px icon on the left and
  up to two 17px bold caption lines beside it; a section that carries a page
  indicator reserves 30px at that outer edge and uses a 50px icon. User button text keeps its
  placement: top text puts the icon below it, bottom text above it, and
  centered or vertical text leaves the key as before. Icons are derived when
  drawing, never stored, and never replace artwork; None and unassigned
  controls stay empty; page indicators still paint last. Preview and hardware
  use identical pixels. Automatic captions wrap only between whole words and
  end in an ellipsis when a word does not fit; a website shows only its URL
  host (`example.com`). User button text is never shortened or rewrapped.
- **Live information:** readings use the same vocabulary (`for_glyph`): sun =
  `brightness`, muted speaker = `mute` (grey speaker, red cross), headphones =
  `audio-output`, recording = `record`, live = `stream`, OBS idle =
  `obs-scene`, plus `volume`, `cloud`, `partly-cloudy`, `rain`, `snow`,
  `storm`, `fog`, `cpu`, `memory` and `temperature`, duotone and shadowed
  like key icons and inked in the reading's accent color (cyan audio, amber sun/storm, violet
  load, green temperature turning amber/red, grey when inactive). Time and
  date has no icon. A key stacks icon (32-44px, shrinks first), a 24px bold
  value, an 8px bar and 14px detail lines, centered inside a 4px margin;
  detail lines that do not fit are left out, so nothing is cut at the edge. A
  touch section draws the binding section tile (15% alpha; 10% over the
  section's own artwork) with a 52px icon on the left and a 26px value, bar
  and up to two 13px detail lines beside it; an indicated edge keeps 30px and
  a 44px icon. Long values shrink to fit, then ellipsize.
- **Strip gestures:** the preview strip's header line (beside the touch
  status, above the sections) carries compact badges for swipe left, swipe
  right, tap and hold, in that order: the gesture glyph (`swipe-left`,
  `swipe-right`, `tap`, `hold`) then the action icon, 11px on a pale pill.
  Unbound swipes show their default touch paging dimmed; explicit None and
  unassigned tap or hold show nothing. Each badge's tooltip and accessible
  name read like "Swipe left (default): Next touchscreen page". The badges are
  preview-only: on the device, the edge chevrons already show swipe paging and
  badges would cover section content.
- **More actions:** each feature action's icon comes from the same mapping as
  saved controls (derived from a representative action). Category rows use
  System `application`, Multimedia `play-pause`, Organization `folder`, Device
  `brightness` and OBS `obs-scene`; Action rows, the guidance line (20px) and
  the closed Action button show the chosen action's icon, while the Action
  menu row keeps `more`.
- **Artwork targets:** every row of the Artwork target menu starts with a
  26x22 mini map of the eight keys over the four strip sections, soft-filled
  and outlined at half ink with its target filled (the whole strip row for the background),
  and menu rows end with the target's assigned action icon (key press; section
  tap, then hold), refreshed whenever the menu opens.
- **Labeled field:** associated, visible label; expected-format placeholder;
  contextual explanation. Conditional fields hide their labels with the input.
- **Binding editor:** preserve existing action indices 0 through 4 and
  `More actions` at index 5. `Play sound` at index 6 exposes the existing audio
  file field and native chooser directly, without secondary category/action
  selectors. Category/action selection never executes.
  `Test action` explicitly executes; save persists configuration only.
- **Page return option:** below every action, a "Return to a page afterwards"
  check button reveals a labeled destination selector (profiles, folders and
  pages by name) and a seconds field. It is off by
  default, saved with the binding, and applies once after the action runs.
- **Live information option:** below the page return, a "Show live information
  on the key" check button reveals a labeled source selector, plus a location
  field only for Weather. Keys, dials and a tap or hold on one touch section; the
  picture is drawn by `info_render` directly over the key's own artwork (120x120)
  or the touch section's (200x100, icon left, value/bar/detail right), never on
  a solid background of its own. Icons and text carry the soft drop shadow
  for legibility and the bar a translucent track. Icon colors
  follow the palette: cyan audio, amber light, violet load that turns amber at
  70 and red at 85, grey when inactive, red for the muted cross and recording.
  Readings come from a worker thread, never the GTK thread.
- **Action guidance:** explain the selected action and expected values beside
  its fields. Native file/folder/audio pickers populate absolute paths. A
  stale picker result must not write into another editor context.
- **Shortcut reference:** a cyan keyboard-focusable Key names and aliases link
  lives in Shortcut guidance without adding a form row. It opens a read-only
  native dialog with selectable aliases, canonical names and physical key names
  from the parser's shared catalogue. The list owns vertical scrolling; Close
  remains outside the list. Existing 16px padding and 8px related-item gaps apply.
- **Organization:** visible profile/folder/page context and navigation;
  contextual creation tools. Invalid metadata disables writes and preserves
  the original file.
- **Profile tabs:** all configured profiles remain visible and manually
  selectable regardless of application focus. Focus detection changes the active
  profile, never the configured tab list. Unsaved application input survives focus
  refreshes. Manual tab selection and saving retain the editing profile across
  unrelated application focus; a matching application can select its profile
  automatically. Save new profile and Save profile use the existing native save
  primitive; completion feedback follows the actual disk result.
- **Profile manager:** Organize separates Edit profile and New profile in a
  native StackSwitcher. Only the selected task occupies vertical space.
  Editing groups the profile selector, optional auto-switch application and
  Save changes/Delete profile actions with 12px gaps. New profile has a name
  and Create profile action. Extended explanations use named question-mark
  controls with hover tooltips and click/keyboard help popovers, not paragraphs
  in the form. Existing 16px panel padding and 4px field-label gaps apply.
  Create profile is disabled for empty or duplicate names and says why under the
  field; Save changes is enabled only when the application rule differs from the
  saved one, and never for Base.
- **Popover:** organization, setup help and OBS settings stay anchored to the
  trigger, with a visible close action and native Escape/outside dismissal.
  Never open one automatically at startup.
- **Artwork disclosure:** independent key/strip/zone artwork editing remains
  reachable in a clearly named expandable section of the selected editor.
  An Appearance group separates text and artwork from the action form.
  Save binding is filled violet; Test action remains neutral and explicitly
  labeled, with guidance that testing executes and saving only persists.
- **Button text:** the configuration inspector contains one keypad-only
  Add text / Edit text button. Both states open the same anchored popup,
  prefilled with the selected key's saved settings. A muted context line under
  the title names what is edited, "Key N on “<view>”", where the view is the
  page indicator's label (profile / folders / page). The editor fields and
  enlarged preview appear only in the popup. A compact two-column body keeps
  the entire 120px live preview next to the form, with no popup scrolling at
  640x480 or larger. Only a long multiline text input may scroll internally.
  Use a native font chooser button, a 6–120px size spinner, and one row with
  a linked pair of orientation icon toggles (upright and clockwise-rotated
  "Ab", named Horizontal and Vertical) beside a 3x3 Position grid of grouped
  toggles (one active; Top left .. Bottom right, each a key outline with a
  dot where the text sits). Grid cells map row to vertical and column to
  horizontal alignment, inherit the control's theme color, are reachable with
  Tab and arrow keys, and keep tooltips and distinct accessible names.
  Vertical text
  rotates the layout 90 degrees clockwise. White glyphs with a thin black
  outline sit above the key's background. The enlarged 120px preview and the
  physical key preview show the key as the device does: its artwork, else its
  binding icon tile when the text leaves the icon a band, else the neutral
  black key; the derived picture is never stored. When the Pango ink extents
  exceed the 116px text area, a warning-toned "Text does not fit; try a
  smaller size" sits under the preview; it is informational. Save text is
  explicit and enabled only for a valid, nonblank value that differs from the
  saved one (Ctrl+Enter saves; plain Enter inserts a line); its tooltip says
  why it is disabled. A muted "Unsaved changes" hint under the preview tracks
  the draft; it never sits in the button row, so it cannot widen the popup.
  Remove text is shown only for a key with saved text. Removing text
  preserves the background, and clearing the background
  preserves text. Closing without saving discards the draft preview. Changing
  key or profile closes the popup and reloads that context. Editor chrome
  continues to use the GTK system font and existing spacing/field primitives.
  This editing popover is non-modal: opening the native font dialog must not
  dismiss it or discard its draft. Close, the trigger, Save/Remove and a
  key/profile change provide explicit exits.
- **Profile removal confirmation:** name the exact profile and loss of
  bindings/artwork, provide Keep/Remove choices, and never default to Remove.
- **Feedback:** countdown, completion, connection and errors use inline text
  with semantic style. Busy workers do not block GTK.
- **OBS:** visible endpoint/password labels, masked password, Save settings
  and Test connection. Testing queries state without starting any output.
  Action choices include existing toggles plus explicit start/stop recording,
  pause/resume recording, and start/stop streaming. Choosing or saving an
  action never executes it.
- **Tray close preference:** Keep running when window closes is checked by
  default. Checked closes hide configuration and retain device monitoring;
  unchecked closes quit the application and its workers. Persist the choice,
  and keep the previous behavior if saving fails. Explicit Quit always exits.

Shared native helpers live in `src/ui.rs`: `button(icon, caption)`,
`decorate_button(button, icon, caption)`, `popover(title, body)`, and
`section_heading(icon, text)`. Colors and sizes trace to this contract.

## 6. Motion and interaction

Keypad drag-and-drop uses GTK's native MOVE cursor and an artwork drag preview.
The source dims to 55% opacity; a valid destination has an inset dashed cyan
outline above its artwork. Dropping swaps both saved actions and appearance
(image and text) within the current page; dropping on itself or cancelling
does nothing. The configuration surface is temporarily disabled while the
serial worker saves the exchange. Success or failure is reported inline.

Retain native GTK interaction and the system animation setting.
Button hover/press washes transition in 140ms with ease-out; geometry, live
hardware input and keyboard focus update immediately. A `motion-enabled` class
tracks `gtk-enable-animations`, including changes while the app is open;
without it transitions have zero duration. No looping decorative effects.
Popovers acknowledge their trigger immediately and never gate input. Do not
add decorative motion. Every action retains hover, press, focus and disabled
states. Countdown uses an owned GLib source removed on cancellation or
application shutdown; hiding configuration leaves it running.
Navigation refreshes selector, bindings, summaries and artwork together.

## 7. Depth and surface

Use ink-toned layering, neutral edges and a faint inset top light for
toolbar/editor surfaces. The profile toolbar has no competing outer outline.
Profile tabs use a violet tonal fill and fine inset edge instead of an underline;
the active tab and keyboard focus remain different states.
The dimensional chassis, artwork cover/crop and circular encoder material
remain intact. No new fonts, heavy shadows, ambient backgrounds or accent borders.
Secondary panels align their trailing edge with their trigger. Their header
and Close action remain outside a bounded vertical scroller, so tall forms do
not hide the exit. The scroller's natural content height is capped at 288px,
leaving room for the header and GTK popup material within a 480px-tall client.
Button text retains its full-size preview.

## 8. Accessibility and verification

Native keyboard-operable controls, visible associated labels and focus rings
are required. Icon-only controls have names/tooltips. Popovers provide a
visible way out. Error explanations say what to correct near the relevant
editor; passwords never appear in summaries or diagnostic text.

Agency Agents accessibility guidance is adapted to this native desktop surface:
retain keyboard access, distinguish interactive boundaries at 3:1 or better,
and retain the existing compact desktop layout and fixed hardware hit regions.
The UI Designer document's 44px touch-target recommendation is not a claim
that this desktop application is optimized for touch-only use. Its web theme
toggle examples do not apply to the approved branded-dark GTK appearance.
No full WCAG conformance or participant-research result is inferred from
color measurements and automated GTK tests alone.

Target tasks: a first-time user assigns a dial action; a keyboard-only user
chooses and saves an action; a user locates profile and OBS settings without
backend knowledge. Each retains a clear next step and a way back.

The desktop-first physical preview keeps fixed hardware coordinates. Test
640x480, 1000x700 and 1280x800 allocations, empty/loaded forms, long labels,
every action family and all changed popovers in light/dark GTK variants.
The GTK owner test is the state/geometry harness; fresh native captures and
independent reviews establish visual quality. Web-only Lighthouse/React
tooling does not apply. Manual hardware input is reported only if observed.
