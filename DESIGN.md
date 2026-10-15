# sdeck-plus native design system

## 1. Atmosphere and identity

sdeck-plus is a native hardware configuration desk, not a web dashboard. Ink-toned
surfaces, luminous violet actions and cyan interaction cues frame the physical Stream Deck preview: eight
keys, a touch strip and four circular encoders. Preserve its coordinates,
artwork and existing functions.

Configuration follows a short path: choose a profile, select a control, choose
an action, complete its fields, then save. Secondary organization and artwork
tools use progressive disclosure instead of competing with action editing.

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

Application chrome uses the branded dark palette, and a matching light palette (`src/ui-light.css`) when the host GTK theme is not dark (the dark preference is off and the theme name does not contain "dark"). The light file only redefines the `@desk_*` tokens and two shadows, so both modes share every rule and follow theme changes live. Explicit field, popup, menu and button surfaces prevent
mixed light/dark controls. Native dialogs, fonts and accessibility remain GTK-owned.

| Role | Token |
| --- | --- |
| Shell / base surface | `@desk_bg`, `#10121d` |
| Primary ink | `@desk_ink`, `#f1f3fb` |
| Supporting ink | `@desk_muted`, `#afb9d0` |
| Elevated editor / toolbar | `@desk_surface`, `#1b2033` |
| Input surface | `@desk_field`, `#141827` |
| Hover / pressed surface | `@desk_hover`, `#2b3350` |
| Neutral divider / edge | `@desk_edge`, `#39435c` |
| Interactive boundary / scrollbar thumb | `@desk_control_edge`, `#68789d` |
| Scrollbar hover / drag | `@desk_muted` / `@desk_cyan` |
| Primary action / selected tab | `@desk_violet`, `#a78bfa`, with `@desk_bg` ink |
| Primary action hover / press | `#bba5ff` / `#9575ed` |
| Keyboard focus / interaction cue | `@desk_cyan`, `#67e8f9` |
| Error ink | `@desk_error`, `#fb7185` |
| Success ink | `@desk_success`, `#7de2b3` |
| Warning ink | `@desk_warning`, `#f9cc7a` |

Light palette values: base `#f4f5fb`, surface `#ffffff`, field `#eef0f8`, hover
`#e2e6f3`, edge `#cfd5e6`, interactive boundary `#8a96b5`, ink `#1b2033`,
supporting ink `#55607a`, primary action `#6d4fd6` (hover `#5f43c8`, press
`#4f35b0`), focus cue `#0e7490`, error `#c0243f`, success `#1b7f55`, warning
`#9a6a00`. The silver hardware preview is identical in both modes.

Status badges pair semantic tones with visible text and low-alpha surfaces.
Color alone never communicates state. Hardware retains its existing silver
chassis (`#f0f0f2` to `#c5c5ca`), black key/strip material (`#08090b`), inset key
edge (`#36363a`), rail (`#b5b5bd` to `#e3e3e6`) and magenta encoders
(`#c329b3` to `#710c72`). The control being edited has a 5px violet ring (`#7c3aed`) separated from the
hardware by a 2px light gap, readable on the silver chassis and over artwork.
Live input is an inset outline drawn on the control itself, above any artwork:
green `#22c55e` while a key or dial is held, amber `#f59e0b` for 300ms on a
dial turn, and a cyan `#67e8f9` wash over each touched strip zone for 300ms (the
800px strip maps to four 200px zones; a flick lights every zone it crosses).
Keyboard focus keeps its cyan outline. Task tabs fill the active one with violet
at 60% and weight 600; inactive tabs are transparent with supporting ink.

Interactive boundaries use their own neutral token rather than the quieter
decorative card edge: 4.00:1 against the input surface and 3.66:1 against the
editor surface. Scrollbar thumbs use the same token and an explicit solid fill,
so a light desktop theme cannot supply a dark-on-dark gradient. Hover and drag
increase thumb contrast without changing native scrolling geometry.

## 3. Typography

Use the system GTK font. App title: 20px/700. Inspector title: 18px/700.
Section labels: 14px/600. Body, controls and field labels: 14px. Supporting
metadata: 13px. Existing physical-device branding and the tiny touch display
retain their material typography; neither carries configuration instructions.
Long summaries ellipsize with full tooltips; editable fields preserve the value.

## 4. Spacing and layout

Default window: 1000x700. Shell margins: 24px. Tokens: 4px label stacks, 8px
compact controls, 12px action/tool gaps, 16px editor padding/form groups and
24px primary region gaps. The compact page-navigation card retains 12px padding.
Control radius: 8px; editor/toolbar radius: 14px. Device canvas padding remains
6px so hardware/artwork geometry is unchanged. Icons are 16px, or 20px for
section identity. Popover padding and header/body separation are 16px; text measure is at most 38
characters.

Profile tabs have a 12px gap before page navigation; navigation and the
device/editor region have 24px separation. The device and editor also have
24px horizontal separation, retained as a vertical gap when they stack.
The editor uses 16px inner padding and 16px between form groups.
Event/action selectors and Save/Test/Remove controls have 12px inline gaps.
Binding feedback stays with its action buttons at a 12px vertical gap,
while unrelated editor sections retain their 16px separation.
Appearance tools and artwork disclosure use 12px gaps without extra empty cards.

Executable and arguments are one related adaptive group: two columns with a
16px gap when each can receive at least 180px, otherwise stacked with 12px
separation. Shortcut input and modifiers use the same adaptive group; modifiers
form a two-by-two grid with 12px columns and 8px rows instead of a cramped line.
Commands and legacy events occupy a full-width field. Hidden fields also hide
their layout cells, and the group disappears for actions that do not use it,
so unrelated actions never leave an empty layout slot.
Field labels remain associated with their inputs.
This redistribution makes room for breathing space without adding primary
scrolling at the default 1000x700 size.

Secondary panels have 16px between logical sections. Setup help groups each
heading with its explanation at 8px, rather than giving headings and unrelated
steps the same gap. Text editing keeps 16px between form and preview and 12px
between Save/Remove actions; its 120px preview remains unchanged.

The fixed-size device chassis is centered on both axes within the preview card.

The profile tabs, profile organization menu and OBS menu form a compact toolbar.
The organization popover contains only profile creation, application association
and profile deletion. The page bar groups compact previous/next/parent icons,
page-owned folders, then New page followed by pencil and red-trash icons.
The folder menu has no outer padding, preserving the page bar's 34px controls.
Icon-only actions retain accessible names, tooltips and visible keyboard focus.
New page and rename open contextual native name dialogs; cancellation never
creates a page. Destructive page, folder and profile operations confirm their
specific scope. Folder creation, selection, linking and deletion belong to the
current page, not the whole profile. Linking persists the selected control's
navigation binding without executing it. The physical preview and a minimum-300px inspector sit side by side
when roomy and stack at narrow desktop widths. The outer `ScrolledWindow`
owns primary vertical overflow. A text editor may scroll its own text; native
popovers contain secondary content. No primary field forces horizontal scroll.

## 5. Components and states

- **Card:** neutral tonal separation, readable heading and inline feedback;
  subtle inset top light, no colored accent border.
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
toolbar/editor surfaces. Profile tabs use a violet wash instead of an underline;
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
