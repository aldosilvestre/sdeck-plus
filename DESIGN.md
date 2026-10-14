# sdeck-plus native design system

## 1. Atmosphere and identity

sdeck-plus is a native hardware configuration desk, not a web dashboard. Quiet neutral
surfaces and symbolic icons frame the physical Stream Deck preview: eight
keys, a touch strip and four circular encoders. Preserve its coordinates,
artwork and existing functions.

Configuration follows a short path: choose a profile, select a control, choose
an action, complete its fields, then save. Secondary organization and artwork
tools use progressive disclosure instead of competing with action editing.

References consulted:

- Frontend `redesign-skill.md`: audit-first hierarchy and legibility.
- `design/apple.md`: restrained configurator chrome and readable utility text;
  no Apple branding, proprietary fonts or marketing imagery.
- [StyleGallery sidebar](https://github.com/changeroa/StyleGallery/blob/main/patterns/split-sidebar/sidebar.md):
  two task regions that stack when available width is insufficient.
- [beui popover source](https://beui.dev/r/popover/raw): anchored disclosure,
  Escape dismissal and focus restoration. GTK supplies the implementation;
  no web component or motion library is copied.

## 2. Color

Application chrome inherits the GTK theme so light and dark variants remain
coherent with native fields.

| Role | Token |
| --- | --- |
| Shell / base surface | `@theme_bg_color` |
| Primary ink | `@theme_fg_color` |
| Supporting ink | `alpha(@theme_fg_color, 0.72)` |
| Elevated editor / toolbar | `mix(@theme_bg_color, @theme_fg_color, 0.035)` |
| Neutral divider / edge | `alpha(@theme_fg_color, 0.14)` |
| Primary action | Theme selection accent mixed with 20% black, white ink; 16% black on hover and 28% on press |
| Error ink | `mix(@theme_fg_color, #e25a70, 0.45)` |
| Success ink | `mix(@theme_fg_color, #38a979, 0.45)` |
| Warning ink | `mix(@theme_fg_color, #c79438, 0.45)` |

Status badges pair semantic tones with visible text and low-alpha surfaces.
Color alone never communicates state. Hardware retains its existing silver
chassis (`#f0f0f2` to `#c5c5ca`), black key/strip material (`#08090b`), inset key
edge (`#36363a`), rail (`#b5b5bd` to `#e3e3e6`) and magenta encoders
(`#c329b3` to `#710c72`). Physical selection uses a neutral wash or selection
mark; only keyboard focus may use a colored edge.

## 3. Typography

Use the system GTK font. App title: 20px/700. Inspector title: 18px/700.
Section labels: 14px/600. Body, controls and field labels: 14px. Supporting
metadata: 13px. Existing physical-device branding and the tiny touch display
retain their material typography; neither carries configuration instructions.
Long summaries ellipsize with full tooltips; editable fields preserve the value.

## 4. Spacing and layout

Default window: 1000x700. Shell margins: 24px. Tokens: 4px label stacks, 8px
inline controls, 12px form groups/card padding, 16px primary region gaps.
Control radius: 8px; editor/toolbar radius: 14px. Device canvas padding remains
6px so hardware/artwork geometry is unchanged. Icons are 16px, or 20px for
section identity. Popover padding is 12px and text measure is at most 38
characters.

The fixed-size device chassis is centered on both axes within the preview card.

The profile picker, organization menu and OBS menu form a compact toolbar.
Page/folder navigation stays visible; creation tools live in the organization
popover. The physical preview and a minimum-300px inspector sit side by side
when roomy and stack at narrow desktop widths. The outer `ScrolledWindow`
owns primary vertical overflow. A text editor may scroll its own text; native
popovers contain secondary content. No primary field forces horizontal scroll.

## 5. Components and states

- **Card:** neutral tonal separation, readable heading and inline feedback;
  no colored accent border.
- **Icon action:** one GTK symbolic icon family, visible outcome label,
  accessible name and tooltip. Save is the editor's only primary action.
  Destructive actions use the native destructive style.
- **Labeled field:** associated, visible label; expected-format placeholder;
  contextual explanation. Conditional fields hide their labels with the input.
- **Binding editor:** preserve existing action indices 0 through 4 and
  `More actions` at index 5. `Play sound` at index 6 exposes the existing audio
  file field and native chooser directly, without secondary category/action
  selectors. Category/action selection never executes.
  `Test action` explicitly executes; save persists configuration only.
- **Action guidance:** explain the selected action and expected values beside
  its fields. Native file/folder/audio pickers populate absolute paths. A
  stale picker result must not write into another editor context.
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
- **Popover:** organization, setup help and OBS settings stay anchored to the
  trigger, with a visible close action and native Escape/outside dismissal.
  Never open one automatically at startup.
- **Artwork disclosure:** independent key/strip/zone artwork editing remains
  reachable in a clearly named expandable section of the selected editor.
- **Button text:** the configuration inspector contains one keypad-only
  Add text / Edit text button. Both states open the same anchored popup,
  prefilled with the selected key's saved settings. The editor fields and
  enlarged preview appear only in the popup. A compact two-column body keeps
  the entire 120px live preview next to the form, with no popup scrolling at
  640x480 or larger. Only a long multiline text input may scroll internally.
  Use a native font chooser button, a 6–120px size spinner and linked, grouped
  toggle buttons for horizontal/vertical orientation and separate
  left/center/right and top/center/bottom alignment. Vertical text
  rotates the layout 90 degrees clockwise. White glyphs with a thin black
  outline sit above the existing background image. An enlarged 120px preview
  and the physical key preview share the device compositor. Save text is
  explicit; removing text preserves the background, and clearing the background
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

Retain native GTK interaction, theme motion and the system animation setting.
Popovers acknowledge their trigger immediately and never gate input. Do not
add decorative motion. Every action retains hover, press, focus and disabled
states. Countdown uses an owned GLib source removed on cancellation or
application shutdown; hiding configuration leaves it running.
Navigation refreshes selector, bindings, summaries and artwork together.

## 7. Depth and surface

Use restrained tonal separation and neutral edges for toolbar/editor surfaces.
The dimensional chassis, artwork cover/crop and circular encoder material
remain intact. No new fonts, heavy shadows, ambient backgrounds or accent
borders.

## 8. Accessibility and verification

Native keyboard-operable controls, visible associated labels and focus rings
are required. Icon-only controls have names/tooltips. Popovers provide a
visible way out. Error explanations say what to correct near the relevant
editor; passwords never appear in summaries or diagnostic text.

Target tasks: a first-time user assigns a dial action; a keyboard-only user
chooses and saves an action; a user locates profile and OBS settings without
backend knowledge. Each retains a clear next step and a way back.

The desktop-first physical preview keeps fixed hardware coordinates. Test
640x480, 1000x700 and 1280x800 allocations, empty/loaded forms, long labels,
every action family and all changed popovers in light/dark GTK variants.
The GTK owner test is the state/geometry harness; fresh native captures and
independent reviews establish visual quality. Web-only Lighthouse/React
tooling does not apply. Manual hardware input is reported only if observed.
