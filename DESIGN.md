# sdeck-plus native design system

## 1. Atmosphere and identity

sdeck-plus is a native hardware configuration desk, not a web dashboard. Ink-toned
surfaces, restrained violet actions and cyan interaction cues frame the graphite Stream Deck preview of the shown model: eight keys, a touch
strip and four circular encoders on the Stream Deck +; 36 keys, a touch strip and
six encoders on the Stream Deck + XL; 15 keys and a background bar on the
Classic; 32 keys on the XL; six keys on the Mini; eight keys, an info window and
two touch buttons on the Neo. Preserve its coordinates, artwork and existing
functions.

Configuration follows a short path: choose a profile, drag an action from the
right-hand searchable catalog onto a keypad button, then complete its fields
below the device preview. Parameter-free actions save on drop; parameterized
actions remain drafts until Save binding, keeping the previous action active.
Saving never executes the action. Clicking a catalog action assigns it to the
selected key as a keyboard-accessible alternative. Unsaved edits block another
catalog assignment; save them or discard them through control selection first.
Existing key-to-key MOVE dragging still swaps saved settings and artwork;
catalog COPY dragging has a distinct payload type and cyan target outline.
The right-hand panel is one card with three segmented tabs: **Actions**, **Dials**
and **Binding** (checked tab uses the selected-tab surface; a model without dials
has no Dials tab, see "Models without dials or a touch strip"). Dials lists presets
(Volume, Brightness, Tracks, Scroll, Pages) as two-line rows; dropping one on a
dial, or clicking it with a dial selected, binds every event it lists in one
validated, atomic save without executing anything and without touching the
dial's other events, and refuses while edits are unsaved. Binding holds the
live-information option for the selected control and explains when it is not
available. The Actions tab groups actions into collapsible categories (Basic open by default,
then System, Multimedia, Organization, Device and OBS), each header showing an
icon, its name and an action count. Rows are frameless 36px targets with the
action icon in a bordered tile and a drag grip that brightens on hover; dragging
shows the row itself. Searching opens every category with matches, hides the
rest and shows "No actions match your search." when nothing matches; clearing
restores the categories as the user left them. Every key and every dial of
the shown model takes a drop (36 keys and 6 dials on the + XL, 8 and 4 on the +,
15 keys on the Classic, 32 on the XL, 6 on the Mini and 8 on the Neo, which have
no dials),
and each model's session keeps its own search text and open categories.
Dropping a catalog action on a dial opens a small popover beside it offering
Turn clockwise, Turn counterclockwise and Press; only the chosen event is
assigned, with the same save-or-draft rule as keys, and clicking an action with a
dial selected uses the event chosen in the inspector. The touch strip retains
its existing action selector. **Plugins** (see `docs/plugins.md`) add one collapsible
item per installed plugin below the built-in categories, never a shared "Plugins"
category: the header shows the plugin name (with "(disabled)" while it is off) and
the body leads with the untrusted-code warning (process separation is not
sandboxing) and an explicit "Enable <name>" check box, then the plugin's actions
with the controls they support as the row subtitle. A plugin that declares
`requirements` also shows a compact **Requires** list under the check box (kind, name,
version, "(optional)", description as tooltip) with an alert-role error line such as
"Missing: hyprctl" for each required command not found on `PATH`; it is rechecked on
every rescan, never blocks enabling or assigning, and runs nothing. A thin divider and a small muted
**Plugins** heading row (heading role label, not clickable) mark where the plugin entries
begin. The row is always shown, so adding a plugin is discoverable before any plugin
exists, and shows only the label: the add button lives in the binding toolbar (see below).
Under the row come malformed-plugin and invalid enabled-state messages (labels with the alert role,
error colour) or the empty-state hint ("No plugins yet. Use the Add plugin button in the toolbar, or copy a plugin
folder into <folder> and use Update plugins."; "No plugins yet. Use the Add plugin button in the toolbar to add one." when no folder is known). Adding and rescanning are not in the sidebar: see the
binding toolbar below.
Actions of a disabled plugin, or that do not fit the selected control (key, dial
turn, dial press, touch strip), are dimmed with an explanatory tooltip. Assigning one
always opens the generic **Plugin action** form (plugin and action drop-downs, then
a text entry, a bounded spin button or a check box per manifest field, labelled for
assistive technology) and stays a draft until Save binding; plugins cannot add their
own widgets. A saved binding whose plugin is missing or changed shows a notice and is
kept verbatim until another action is chosen. **Plugin values.** An enabled plugin
action whose manifest declares a `display` shows the value the plugin reports (headline,
optional detail, optional level bar) on the key, dial zone or touch section it is bound to, in
the window preview and on the device, drawn like live information; there is no extra control
in the editor and nothing is stored. Until the first answer the control shows "..."; when the
plugin fails or times out it shows a greyed "--". An explicit live-information choice on the
same control wins, and a running timer countdown keeps its key. A disabled or missing plugin,
or an action without a display, leaves the control as it was (its usual icon). Values are
asked for when the layer, page or bindings change, when the device connects, when the plugin
list changes and after any action of the same plugin finishes; they are not polled.
Secondary organization and artwork tools use progressive disclosure.

**Add plugin dialog.** A modal window transient for the main window, one instance at a
time (pressing the button again presents it), 660 x 760 by default and resizable (it still
fits at 520 px wide). It opens with the entry focused, Enter in the entry runs the check and
Escape closes it. Top to bottom: an in-content header row (the window's title bar: an "Add
plugin" heading and a Close icon button), a short explanation ("Add a plugin from a folder on
this computer or from an HTTPS repository address. Nothing runs until you enable it.", shown
until a preview exists), the **Plugin source** block, a status row, a scrolling area, the
pinned callouts and the footer.

The source block has a **Folder | Repository** selector (linked toggle buttons, Folder by
default, operable with Tab and Space), one entry, **Browse…** and **Check plugin**. The selector
only changes the entry's placeholder and the helper line under it, and hides Browse for
Repository; it never changes what is checked (the text decides). Typing or pasting an
`https://` address selects Repository, and typing an absolute or `~` path selects Folder, so
the selector never contradicts the text. Check plugin has the primary look while the entry has
text and there is no preview; once a preview exists Install or Replace is the primary action.
While a check or install runs the status row shows a spinner, the text and Cancel.

The scrolling area holds the error callout, the "No plugin checked yet" empty state (idle
only) and the preview card. The error callout (tinted `desk_error` fill, `desk_error` left
edge, ink text, an icon on every row) is a list of alert-role sentences: a headline, up to 8
problems and "And N more problems.". The preview card has a monogram tile (the first letter of
the name on a `desk_selection` tile with a `desk_violet` edge), the name as a title, chips for
`v<version>`, `id <id>` and the source (Folder or Repository, with an icon), the source line
(folder path or repository address, wrapping, full text as tooltip), a four-cell stats row
(Files, Size, Commit, Actions; the commit is the first 10 characters of the downloaded commit,
and a folder shows a muted "n/a") and two native expanders. **Actions** (open by default, except when a required command is
missing, so the Requirements rows fit) lists each action with chips for its controls (Key, Dial turn, Dial press, Touch strip) and a
muted settings line. **Requirements** shows "X of Y found" (counting commands, the only kind
that is looked up) and is collapsed when every required command is found and opened
automatically when one is missing, and the card is then scrolled (on the frame clock, after layout)
so the first Missing row is fully visible; each row has a status chip: Found, "Missing: <command>"
(a required command not on `PATH`, an alert that never blocks installing), "Optional, not
found" or "Not checked" (services and files are not looked up).

The pinned callouts sit between the scrolling area and the footer and never scroll away: the
privileges warning (amber `desk_warning` edge and icon; ink text) always states that the plugin
runs with the user's privileges once enabled (never with administrator rights) and that its code should be read first, and, when
the id is installed, a replace notice (`desk_cyan` edge) gives the installed version and says
that replacing disables the plugin until it is enabled again. The footer has Close and Install
(or Replace); while Install cannot be pressed, a helper line there says "Check a plugin to
enable Install." Focus is the 2px `desk_cyan` outline on every control; colours come from the
`desk_*` tokens only, so the dark and light palettes both apply.

States: **idle** (Install disabled), **checking** (entry, selector, Browse and Check disabled; spinner,
Cancel), **valid preview** (Install enabled for exactly the text in the entry; editing the
entry or Browse drops the preview and the staged copy and returns to idle; switching the
selector does not), **invalid** (one or
more readable sentences, Install disabled; nothing is installed or left staged), **replace**
(valid preview of an id that is already installed; the button reads Replace) and
**installing** (everything disabled; spinner). An AlreadyInstalled answer from a race moves the
dialog to replace with an explanation instead of failing silently. On success the dialog closes,
the sidebar is rescanned so the plugin appears as its own disabled item, and the toast
says "Installed <name> <version>. It is disabled: enable it in the Plugins section." (a
replacement also says it was disabled and must be enabled again).

Safety rules: only `https://` repository addresses are accepted and are fetched with the
system git (hardened by the library; a missing git is explained and a folder suggested);
checking and installing run on a worker thread, never the GTK thread; a candidate is validated
with the registry rules before anything is installed; installed plugins are never enabled;
Replace is used only after an explicit press on a valid preview and disables the plugin;
installs are serialised through one hub-wide slot (a second request is refused); closing the
dialog never blocks: it cancels a running check and returns, and the main-loop poll joins the
finished worker later (a cancelled check leaves no staging directory; an install already
started completes, then the sidebar is rescanned and the toast reports it). Errors are sentences, never debug output.
Everything shown from a plugin or from git (names, labels, requirement text, git's message) is
displayed literally (no markup) with control, zero-width and bidirectional-format characters
removed; a long plugin name, version or id chip is ellipsized with the full text as its tooltip. Releasing a
staged copy (closing, Escape, editing, a failed replace, the main window closing) never blocks
the GTK thread: the directory is deleted on a short-lived thread, and an install that finishes
or fails after the dialog closed is reported in the toast.

Known limits: a collapsed section's rows are not in the widget tree until it opens; with many requirements at 520 px wide the last optional rows can sit under the pinned callouts until scrolled; the repository error offers no "choose a folder instead" shortcut (the message says to); Replace overwrites whatever version is installed at that moment, which may differ
from the one the preview showed if another install ran in between; the staged copy is validated and copied at check time, so
later edits to the source folder are not installed (the previewed bytes are).


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
  two task regions (here they stay side by side and scroll instead of stacking).
- [beui popover source](https://beui.dev/r/popover/raw): anchored disclosure,
  Escape dismissal and focus restoration. GTK supplies the implementation;
  no web component or motion library is copied.

## 2. Color

Application chrome uses the branded dark palette, and a matching light palette (`src/ui-light.css`) when the host GTK theme is not dark (the dark preference is off and the theme name does not contain "dark"). The light file only redefines the `@desk_*` tokens, primary-action states and the popover, tooltip and toast shadows, so both modes share every rule and follow theme changes live. Explicit field, popup, menu and button surfaces prevent
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
| Error ink | `@desk_error`, `#fb778a` |
| Success ink | `@desk_success`, `#7de2b3` |
| Warning ink | `@desk_warning`, `#f9cc7a` |

Light palette values: base `#f3f5f7`, surface `#ffffff`, field `#f7f8fa`, hover
`#e6eaef`, edge `#d7dde4`, interactive boundary `#7a8491`, ink `#242c35`,
supporting ink `#536170`, primary action `#6746bb` (hover `#5939a9`, press
`#4c2f95`), selected tab `#e8e1f6`, focus cue `#0e7183`, error `#b4223b`,
success `#176d49`, warning `#815900`. Tone text on its own 16% tint and over `desk_surface` or
`desk_bg` reaches 4.5:1 in both themes (guarded by `tests/ui_css.rs`). The primary hover and press
fills, the selection ring and its gap and the live-input colours are `desk_*` tokens too
(`desk_violet_hover`, `desk_violet_pressed`, `desk_ring`, `desk_ring_gap`, `desk_live_press`,
`desk_live_rotate`, `desk_live_touch`), defined in both theme files; the hardware preview keeps
its own literals. The graphite hardware is identical in both modes.

Status badges pair semantic tones with visible text, a low-alpha fill and a
55%-alpha edge of the same token: warning for Not connected/Disconnected, error
for Unavailable and success for Connected; the virtual deck's "Virtual deck on · N
viewers" badge is violet (`.status-virtual`). Color alone never communicates state. Hardware uses a graphite chassis
(`#414951` to `#272d34`), pale branding (`#e1e6eb`), black key/strip material
(`#08090b`), inset key edge (`#515b65`), recessed rail (`#22282e` to `#343c44`)
and neutral encoders (`#68737e` to `#343c44`). Subtle inset lighting gives the
chassis and dials depth without changing their geometry. The control being
edited retains a 5px violet ring (`#7c3aed`) separated from the hardware by a
2px light gap, readable on the graphite chassis and over artwork.
Live input is an inset outline drawn on the control itself, above any artwork:
green `#22c55e` while a key, touch button or dial is held, amber `#f59e0b` for 300ms on a
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
("Touch strip · waiting for input", or "Info window" in an empty Neo window)
is 11px/600 mint on black, and each preview
dial's caption is 10px/600 pale graphite ink; neither carries configuration
instructions or reaches the physical device.
Long summaries ellipsize with full tooltips; editable fields preserve the value.

## Overlays and feedback

Popovers, help popovers and tooltips share a 12px radius and the `0 8px 24px` shadow; tooltips are
8px-radius labels (12px text). Every popover has a 1px edge, 6px padding and no arrow, and no
title bar or Close button: Escape or a click outside dismisses it (GTK's autohide; the Button text
popover keeps autohide off so its native font dialog cannot dismiss it, and closes on Escape or
its trigger). Rows of command menus, dropdown lists and the key context menu are 38px high
(`min-height` 22px + 8px padding, because GTK's min-height is the content box), padding 8px 10px,
radius 8, 10px between icon and label, a hover fill; selected rows use the selection fill with a
1px violet inset edge. Separators are 1px with a 6px vertical margin. A trailing `kbd` hint
appears only on rows with a real shortcut (none today). Form popovers (OBS settings, Button text,
Page folders) start with a bold title and a muted subtitle inside the body, like the help
popover, and pad their content 12px inside the popover's own 6px. The toast sits at the top
center of the window, 24px below its top edge, over the content. It is a pill (`desk_hover` fill, 999px radius): a tone glyph (green check for a
success, red alert for an error), the message in ink, then its optional action. A success hides
itself after a few seconds and has no Dismiss button; an error stays and shows an icon-only Dismiss.
Every feedback label sits in a banner row the first time it shows feedback: 8px radius, 6px 10px
padding, `alpha(tone, 0.16)` fill, a leading tone glyph in the tone colour and the message in ink,
always with words. The row takes the label's place and follows its visibility.
Source artboard: `design/overlays/03-compact-command.html`.

The connection badge heads the profile rail as a small pill (11px text). The header keeps the title
and the detail line.

Dialogs (confirmations, the artwork error, page name, New/Edit profile, the multi action step
dialog, setup help, Updates and the shortcut reference) share one frame: surface fill, 1px edge, radius 12, 14px padding, 10px
gap, a 16px/700 title, a muted body, and ONE right-aligned action row of natural-width buttons
with an 8px gap: the dismissal first with its `Esc` key hint (monospace 11px chip: Cancel, Keep
editing or Close), then the primary (suggested) or danger action. No action is full width and none
sits in a second row. Destructive confirmations default to Cancel.

A toast may carry one action (violet, 600 weight) beside its message, as "Undo" after removing a
binding; using it runs it once and closes the toast, and any later message drops it. A toast with
an action stays 10 seconds (a plain success 5); when it closes, its action is dropped and can no
longer run.

## 4. Spacing and layout

Default window: 1120x884. Shell margins: 16px (`SHELL_MARGIN`, on the shell box; the documentation screenshots of the whole window paint them too). Tokens: 4px label stacks, 8px
compact controls, 12px action/tool gaps, 16px editor padding and
16px primary region gaps. The compact page-navigation card retains 12px padding.
Control radius: 8px; editor/toolbar radius: 14px. Device canvas padding remains
6px so hardware/artwork geometry is unchanged. Icons are 16px, or 20px for
section identity. Popover padding is 6px (12px more around form content), dialog padding 14px; text measure is at most 38
characters.

Under Hyprland the window floats: each time the main window maps (first show and
every show from the tray) an owned worker thread finds the app's own client in
`j/clients` (pid, class `dev.streamdeck.EventMonitor`, title) and, only if it is
tiled, sends one `[[BATCH]]` over the command socket: `setfloating`,
`resizewindowpixel exact 1120 884` (clamped to the monitor's usable area) and
`movewindowpixel` to center it, all with `address:0x...`. Floating windows,
manual resizes, other compositors and any IPC error are left untouched. Code:
`src/window_float.rs`.

Profile tabs have a 12px gap before page navigation; navigation and the
device/editor region have 16px separation. The device and editor also have
16px layout separation, retained as a vertical gap when they stack.
The editor uses 16px inner padding and 6px between related form groups, keeping
the added return and information options within the default 1120x884 layout.
Event/action selectors (shown for dials, the touch strip and touch buttons; a key has one
event and takes its action from the catalog; a touch button's one event is Press and
its action comes from the Action selector) and Save/Test/Remove controls have
12px inline gaps. The "Do nothing…" hint is also hidden for an unassigned key, and a key's extended
action hides its Category and Action dropdowns (the catalog chooses them) while
keeping the action's icon, description and fields.
A small "Options" heading groups the page-return and live-information checks.
Binding results and errors show only in the toast, never as a message line in
the side panel. The Appearance separator, a 4px extra top margin and an
ink "Appearance" heading keep action and artwork sections distinct.
The editor tracks whether any field differs from the loaded or saved binding
(loading, saving or removing resets it) without showing a message for it.
Selecting another preview control while edits are unsaved asks first with the
compact confirmation style (Keep editing is the default, Discard changes is
destructive). Remove clears the whole selected control (a key's press and hold,
a dial's press, turns and exact ticks, a touch area's gestures) with their page
returns and live information, whichever event is selected. Only its toast
confirms it ("Control configuration removed.", with an Undo action for 10
seconds), and there is no lasting Undo button.
Selecting another control keeps that toast; another Remove or any later message
replaces it. One Undo re-saves everything that was removed, into the profile or
page that owned it. Undo never overwrites later edits: if the control was changed after Remove, it refuses with an error message and offers nothing to retry.
The related controls inside Appearance use 4px gaps to keep expanded artwork
and the taller shortcut/argument fields compact within the scrolling workspace.
Appearance tools and artwork disclosure use 12px gaps without extra empty cards.

Executable and arguments are one related adaptive group: two columns with a
16px gap when each can receive at least 180px, otherwise stacked with 12px
separation. Shortcut input and modifiers use the same adaptive group; modifiers
are one linked row of named toggle buttons ordered Ctrl, Alt, Shift, Super
(4px/8px padding, checked state uses the selected-tab surface with a violet
inset edge, tooltip "Hold X with the key"). A muted line under the row previews the chord
once it parses (for example "Presses Ctrl + Alt + P") and stays hidden otherwise.
Commands and named events occupy a full-width field. Hidden fields also hide
their layout cells, and the group disappears for actions that do not use it,
so unrelated actions never leave an empty layout slot.
Field labels remain associated with their inputs.
The inspector-below-preview workspace uses vertical scrolling at the default
1120x884 size, while retaining the form's compact field groups.

Secondary panels have 16px between logical sections. Setup help groups each
heading with its explanation at 8px, rather than giving headings and unrelated
steps the same gap. Text editing keeps 16px between form and preview and 12px
between Save/Remove actions; its preview is the key at the shown model's size
(its key image size: 120px for the +, 112px for the + XL, 72px for the Classic,
96px for the XL and the Neo, 80px for the Mini).

The fixed-geometry device chassis sits in its preview card: the chassis plus
a 14px frame (padding and border), 440x428 at 1x for the Stream Deck +. The card is presented through `ScaledBin`
(`src/ui/scaled_bin.rs`) at a fixed 1.0x (`PREVIEW_MAX_SCALE`), top-aligned and
horizontally centered: the preview keeps one size whatever the window width,
the selected action or the form beside it, so it never jumps. Hardware
coordinates, artwork targets and the card's size never change while a model is
shown, and the card never stretches to match a tall form. GTK applies the same
transform to drawing and pointer picking, so selection, drag-and-drop and
focus keep working. The inspector uses its content height, without an
arbitrary minimum height.

**Model-driven preview.** The chassis is drawn from the shown model's layout
(`PreviewLayout::for_model`, `src/preview_layout.rs`, pure geometry shared by
the widgets and the tests); `PreviewView` (`src/ui/preview.rs`) builds it once, inside
the session of that model; showing another model shows another session (see
"Device model"), so a chassis is never rebuilt in place. Geometry,
in preview pixels at 1x:

| Model | Chassis | Keys | Touch strip | Dials |
|-------|---------|------|-------------|-------|
| Stream Deck + | 426x414 | 8 (4x2), 60px on a 91x67 pitch | 328x41, 4 sections of 82px | 4 knobs of 46px, 91px pitch |
| Stream Deck + XL | 556x456 | 36 (9x4), 40px on a 46px pitch | 456x41, 6 sections of 76px | 6 knobs of 46px, each centered under its section |
| Stream Deck Classic | 348x250 | 15 (5x3), 44px on a 52px pitch | none (a 252x28 Background bar under the keys) | none |
| Stream Deck XL | 504x262 | 32 (8x4), 44px on a 52px pitch | none | none |
| Stream Deck Mini | 244x158 | 6 (3x2), 44px on a 52px pitch | none | none |
| Stream Deck Neo | 296x208 | 8 (4x2), 44px on a 52px pitch | none (a 124x29 info window without touch, between 2 touch buttons of 28px) | none |

The Stream Deck + keeps the geometry it always had, pixel for pixel. Every
other model of the registry is laid out by the same generic rules (44px keys on
a 52px pitch, 76px sections, knobs under their sections, a chassis just wide
enough for its widest row, margins kept whole so it is symmetric); models
without a strip or dials omit them. The + XL uses those rules with 40px keys on
a 46px pitch and a 24px bottom margin, so its four rows still fit the window
(see below). A Neo's information window is drawn under the keys, where a strip
would be, as a selectable display of key material (`.window-surface`, named
"Info window") at half its pixel size, so it keeps the device's 248:58 shape
(124x29), with no bezel or header line around it: it is its own frame and the
chassis shows around it. It takes no touch: it shows the page's picture and
live information (see "Information window" below), and while it shows no
picture it is captioned "Info window" (11px/600 mint, centred in it). Its two
capacitive touch buttons are
round 28px pads of key material with the key's inset edge (`.touch-button`), one
on each side of the window on the chassis, 10px from it and vertically centred
on it (the odd half pixel downwards, so Tab reaches the window first); pads and
window span the key row (the Neo chassis is 296x208). They are selectable
controls that take no picture (Elgato documents none). A model with touch
buttons and no display would show them as one centred row under its keys.
The Stream Deck Classic adds a slim **Background** bar under its keys, as wide
as them and 12px below the last row (252x28, key material with the key's inset
edge, `.background-surface`): the page's background picture (42x24, cover) and
the caption "Background" (11px/600). It is a selectable control named
"Background" (see "Keypad background" below); the Classic chassis is 348x250.

**Models without dials or a touch strip.** The Stream Deck Classic (15 keys,
5x3) and XL (32 keys, 8x4) have keys only, and the Neo keys, two touch buttons
and an information window without touch. Their session shows the sections of
the controls the model has and nothing else: no section is left empty,
disabled or broken in place of one the model lacks, and the + and + XL keep
every section unchanged.
- The preview draws only the model's controls (keys, the Classic's background
  bar, the Neo's window and touch buttons); Tab visits the keys row-major, then
  the Classic's background bar or the Neo's window, then the touch buttons.
- The side panel shows **Actions** and **Binding**; the Dials tab and its
  presets are not there. The catalog serves every key as on the + (drop, click
  with a key selected, parameter-free actions save on drop); its rows read
  "Drag <action> onto a key, or click to assign to the selected control.", and
  clicking one while a touch button is selected says "Select a key before
  assigning an action." (a touch button takes its action from the Action
  selector).
- The touchscreen page controls, the strip's Appearance section and the swipe,
  tap and hold badges never appear (the Neo's empty window shows only its
  plain "Info window" caption, and the Artwork target picker lists nothing on
  an XL). On the Neo, selecting the window shows its own Appearance
  section (the picker lists only "Info window", in a "Window picture" expander
  that says each keypad page has its own picture and live information); on the
  Classic, selecting the background bar shows its own (the picker lists only
  "Background", in a "Background picture" expander that says each keypad page
  has its own background, shown behind every key without a picture of its
  own). The
  editor refuses a dial or the strip by name ("the Stream Deck Classic has no
  dials", "the Stream Deck Classic has no touch strip") and stays on the
  control it was on.
- Texts name only what the model has: New profile reads "New profile: add a
  profile with its own keys and pages." ("keys, pages and touch buttons" on the
  Neo), the live-information tooltip has no dial or section sentence, the page
  folders link "to selected key" ("key or touch button" on the Neo), and the
  window's setup help, which follows the shown model, reads "Click a key on the
  preview." ("Click a key or the background on the preview." on the Classic,
  "Click a key, the info window or a touch button on the preview." on the Neo)
  without the Touch strip pages step.
- Nothing is sent for a display the model lacks: an XL receives key pictures
  only, and so does a Classic until one of its pages has a background (see
  "Keypad background"), so no picture can fail and overwrite the status line.
  Connected with a + (or any other model), each keeps its own session as
  described under "Device model".

**Preview images.** The documentation shows one image per supported model,
rendered from the real preview widget with no device: `preview-<model id>.png`
is the device card alone, the size of its chassis (426x414 for the +, 556x456
for the + XL, 348x250 for the Classic, 504x262 for the XL, 244x158 for the Mini,
296x208 for the Neo) and
`main-window-xl.png` is the window content configuring the + XL at the default
1120x884 size (1152x916 with the 16px shell margin; no selector: one model).
`main-window-two-devices.png` (a 1000x700 window, 1032x732 with the margin,
scrolled to the header) shows the "Current" selector with a simulated + and + XL connected through
the real hub. They are written by the documentation screenshot generator, which
iterates `DeviceModel::supported()` for the per-model images, so a model that
becomes supported gets its image without editing the generator. The + XL images
and the two-device image come from simulated devices only; they say nothing about
hardware behavior.

The preview is never scaled for a model: the card is the chassis plus 14px
(570x470 for the + XL) at exactly 1.0x, never scrolls inside itself, and keeps
every control at least 24px on both sides (the smallest is the + XL key at
40px). At the default 1120x884 window the + XL card still sits beside the
action catalog: the 234px rail leaves 886px, so the catalog gives up part of its natural
352px (down toward its 250px minimum) and the settings column stays usable. A narrower window never wraps the catalog
below the workspace, for either model: the catalog shrinks toward its 250px
minimum and then the content column scrolls horizontally (the rail and header
stay put), so an action can always be dragged from the catalog onto a key or
dial that is on screen beside it.
The + XL chassis is 42px taller than the + (456px against 414px); its card
(570x470) ends inside the 884px viewport of the default window, so the page
needs no vertical scroll to reach the dials (a test pins this for both
models). A shorter window scrolls the page, never the preview. Dial captions are 72px wide on the + XL (86px
on the +) so neighbours never touch.

The complete configuration shell (header, profile rail, page navigation and
body) fills the available width up to 1240px (1272px including margins), then
centers (`Clamp`, `src/ui/clamp.rs`; GTK 4.8 has no native clamp). These bars
share the shell's leading/trailing edges. The body is a plain two-column
horizontal box (class `configuration-grid`, 16px gap) that never wraps: the
workspace keeps its natural width, so the preview stays unscaled, and spare
width goes to the action catalog through horizontal expansion. The preview does not consume spare vertical space so
its inspector follows immediately underneath. The inspector sits in its own
vertical scroller (`settings-scroll`) capped at about one window of height
(window height minus 240px, at least 320px): a tall form scrolls inside that
region instead of stretching the page, and the outer viewport only scrolls when
the window is shorter than the preview itself. The action catalog has its own
bounded scrolling list. Its real minimum is the 250px size request: the Actions /
Dials / Binding tabs shrink (the theme's button minimum width is cleared and the
captions may ellipsize) and the live-information check caption wraps.
At narrow widths the shell shrinks to the available width; the two columns
stay side by side down to their combined minimum (workspace + 16px + the 250px
catalog; measured content minimum 706px for the + and 836px for the + XL, which
with the 234px the rail takes (rail plus its gap) is a window of 940px and 1070px),
and below that the content column scrolls horizontally, as it
does at 640px, instead of stacking.
These two size-only containers are the only custom layout; there is no resize
polling.

Information buttons (the circled "i" beside a field label, `ui::field_help`) reveal their
help on hover, or on keyboard focus when navigating with the keyboard, in a light popover
with the topic as its title; no click is needed, the popover ignores the pointer so it never
blocks the form, and there is no second native tooltip. A form that opens with the button
focused does not pop the help open by itself.
There is no Organize menu: profiles are edited and deleted from their own rail rows (below) and
created with the "+" beside PROFILES. Each of those controls opens its form in its own modal
dialog, transient for the main window, titled "Edit profile" or "New profile", with a one-line
subtitle; the Base note is a callout with a cyan accent edge; the footer is the single
right-aligned action row: Cancel (with the real Esc hint) then the primary (Save changes in Edit,
Create profile in New). Edit no longer carries a Delete profile button: the rail row's Delete icon
covers every profile and asks first. Escape or Cancel hides the dialog without saving; Cancel and
Esc discard a New draft. Deleting asks for confirmation (below); Base cannot be deleted.
The profile tabs form a vertical rail at the left of the content, identical at every window
width, full height and never scrolling (artboard 03-profile-rail), top-down: the connection
badge, the OBS tool, then a "PROFILES" sub-heading (11px, regular weight, `desk_faint`, dimmer than
helper text so it reads as a label, not a control) with a compact "+" New profile action
beside it in full `desk_ink`, then the profile rows. A lone tool needs no "TOOLS" heading, so OBS
sits above PROFILES without one. The rail is 234px wide (it was 166px): 13 characters of name
(ellipsized from 16, with the full name in a tooltip) and two small icons per row. "+" alone
describes a new profile and is the only New profile control; it keeps its tooltip and accessible
name ("New profile: ..."). Each row is the tab's label: the profile name, then a frameless
pencil (Edit) and a frameless red trash (Delete), the same icons as the page bar, 18px hit areas
that fill on hover and show the keyboard focus ring. Their tooltips and accessible names include
the profile ("Edit Studio", "Delete Studio"). The icons are buttons inside the tab, so they take
their own clicks and never switch the profile; clicking the name still does. Edit opens the Edit
profile dialog pointed at that row's profile without switching to it. Delete opens the delete
confirmation for that profile; Base keeps a disabled Delete ("Base cannot be deleted.") so every
row's icons line up. Nothing is packed at the rail foot. OBS and "+" have a tooltip that says
what they do ("OBS: ...", "New profile: ...") and the same text as their accessible description.
The badge, OBS and PROFILES row are the Notebook's start action widget. Because the rail is
outside the scrolling region OBS is always visible and its popover opens fully, in a 1120x884
window and at the smallest window. OBS is frameless (hover shows the surface) so it stays
secondary to the tabs. Menu is the window's one configuration menu button and lives in the
header, not the rail: on the title row, completely at the right edge and vertically centred on the
"SDECK-PLUS" line (not on the detail line below). It keeps its icon + "Menu" caption, tooltip
("Menu: ..."), accessible name and description and headerless popover, and the same button serves
every model shown. The header has two rows: the title row (title, the virtual deck badge while
it is on, update indicator when shown, Current selector when two models are connected, Menu) and,
under it, the connection detail.
The Current selector sits immediately to the left of Menu on the same line. Only the content column (page bar, preview,
editor, side panel) scrolls; `rail_tests` guards the order, the fixed rail and that the tools stay
inside the window, and `profile_row_tests` the per-row icons. The page bar separates navigation from
management: separate flat previous/next/parent icon buttons lead the
breadcrumb with a 12px gap, and page-owned folders and New page (flat) trail it,
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
The **Page folders** popover heads its body with the title "Page folders" and a muted subtitle (no
Close; Escape or an outside click dismisses it) and reads
top to bottom: a context line "Folders on “page”" with a one-line helper about
linking; a linked create row (entry with the "New folder name" placeholder and
a suggested-action add icon; Enter creates) with live validation beneath it
(blank disables the button silently; duplicate or invalid names disable it and
show the reason in error color); then a boxed list of the page's folders,
sorted by name, scrolling past about 240px. Each 40px row shows a violet
folder icon, the bold name (ellipsized at the end) and frameless trailing
icon actions Open, Link to selected key or dial, Rename and red Delete, each with a tooltip
and accessible name that include the folder name ("Rename folder Studio").
Activating a row opens the folder. Creating keeps the popover open, clears
the entry and lists the new folder without leaving the page. Rename swaps the
name for a prefilled, selected entry: Enter saves, Escape or moving focus
away cancels, and validation errors stay inline without ending the edit.
Delete still confirms and is unavailable while artwork writes are disabled.
A page without folders shows a dim folder icon and "No folders on this page
yet". Rows use only `@desk_*` tokens, so light and dark themes match, and
keyboard focus draws the cyan focus ring. The physical preview and a minimum-300px
inspector form one vertical workspace, always beside the minimum-250px action
catalog (never wrapped below it). The outer `ScrolledWindow` owns primary
vertical overflow, and scrolls horizontally only when the window is narrower
than the two columns' combined minimum. A text editor may scroll its own text; native
popovers contain secondary content. No field forces horizontal scroll while the
two columns fit.

## 5. Components and states

- **Card:** neutral tonal separation, readable heading and inline feedback;
  subtle inset top light and no colored outline. Callouts (`.plugin-callout`) and the profile note
  use a 3px tone left edge as a deliberate exception: warning or cyan for info, error for errors.
- **Buttons** (`src/ui.css`, both themes; artboards `design/overlays/03-compact-command.html`
  and `design/main-window/03-profile-rail.html`):
  - *Default:* `desk_field` fill, 1px `desk_control_edge`, radius 8, padding 6px 12px, 34px high,
    6px icon-label gap; hover fills `desk_hover`.
  - *Primary* (`suggested-action`): `desk_violet` fill, weight 600, no border.
  - *Danger* (`destructive-action`): `desk_error` text, `alpha(desk_error, .14)` fill,
    `alpha(desk_error, .5)` border, weight 600. Flat destructive icons (page, folder, profile-row
    delete) stay frameless: error-coloured glyph, tint on hover. The destructive row of the key
    context menu tints on hover with `alpha(desk_error, .12)`, keeping error text at 4.5:1. The editor heading row keeps its
    neutral toolbar with a red glyph.
  - *Flat* (`flat` / `has_frame(false)`): transparent fill and transparent border until hovered.
  - *Icon-only* (`icon-only`): 34px square; per-profile-row Edit/Delete and the rail's + stay
    compact (18-20px) by design.
  - *Pressed / selected* (`:checked`, toggles): `desk_selection` fill with an inset 1px
    `desk_violet` edge. *Focus:* 2px `desk_cyan` outline, offset -2.
  - *Page bar:* Previous, Next and Parent are separate flat 34px icon buttons (no linked
    segment); Folders and New page are flat captioned buttons; rename and delete are flat icons.
  - Compact inspector groups (modifier toggles, artwork and button-text choices, the position grid,
    plugin source selector) keep their smaller sizes.
- **Icon action:** one GTK symbolic icon family at 16px, visible outcome
  label, accessible name and tooltip. Save is the editor's only primary action.
  Destructive actions use the native destructive style, and every delete or
  remove (binding, page, folder, touch page, profile) uses the same trash icon;
  clearing text or artwork keeps the clear icon. Profile rows use the pencil and
  trash of the page bar, and button text uses the plain text icon.
- **Binding icons:** one vocabulary (`src/binding_icons.rs`) maps every
  action to a duotone vector glyph on a 24-unit grid: a soft fill (35% of
  the ink) under a crisp 1.85-unit round stroke (2.2-2.6 for chevrons, bars
  and the text T), round caps and joins: go to page, next/previous page, folder, parent folder, command,
  application, shortcut, named event, play sound, media transport, volume,
  mute, audio output, scroll, text, open file, website, close application,
  shortcut toggle, timer, cancel timer, brightness, sleep, OBS scene, record,
  stream, microphone mute, visibility and `multi` for a multi action (three bars
  beside a timeline of three dots, in the system amber family), plus `more` for More actions, `none`
  (menus only), `missing-file` (a red cross in a soft circle, its own red
  family, drawn instead of an icon on a key or touch section whose sound or
  file target no longer exists, captioned "Missing file"; unlike other icons it
  also replaces the control's own picture and sits behind button text of any
  placement, large and centered) and a `generic` fallback (a four-pointed sparkle). The caller
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
  drawing, never stored, and never replace artwork; other actions' artwork
  hides their icon, but an idle Start timer key draws its icon and caption
  over its own artwork instead of black, so the key stays recognizable (its
  countdown still draws over the artwork, and Cancel timer keeps the plain
  artwork). None and unassigned
  controls stay empty; page indicators still paint last. Preview and hardware
  use identical pixels. Automatic captions wrap only between whole words and
  end in an ellipsis when a word does not fit; a website shows only its URL
  host (`example.com`). User button text is never shortened or rewrapped.
- **Live information:** readings use the same vocabulary (`for_glyph`): sun =
  `brightness`, muted speaker = `mute` (grey speaker, red cross), headphones =
  `audio-output`, microphone (both microphone readings, headsets included) =
  `audio-input`, muted microphone = `microphone-mute` (grey microphone, red
  slash), recording = `record`, live = `stream`, OBS idle =
  `obs-scene`, now playing = `play`, `pause` or `media-idle` (a struck-through
  note), plus `volume`, `cloud`, `partly-cloudy`, `rain`, `snow`,
  `storm`, `fog`, `cpu`, `memory` and `temperature`, duotone and shadowed
  like key icons and inked in the reading's accent color (cyan audio, amber sun/storm, violet
  load, green temperature turning amber/red, grey when inactive). Time and
  date has no icon. Now playing reads MPRIS players: playing shows a violet
  play icon, the title as value and the artist as detail; paused shows the
  pause icon and the same text in grey; no player or a stopped one shows a
  grey `media-idle` icon over a white "Nothing playing" caption; a failing
  session bus shows the usual grey `--`. A playing player wins (the first by
  bus name), then the paused one that played last. When the player's MPRIS
  `DesktopEntry` (or its `Identity` as an entry name) resolves to an icon
  (desktop file `Icon`, current theme and its parents, then hicolor: scalable
  SVG first, else the largest PNG up to 256px; then pixmaps), that icon
  replaces the glyph in the same slot in full color (60% opacity while
  paused), with a round dark badge at its bottom-right corner holding the
  `play` (violet) or `pause` (grey) icon; nothing playing keeps
  `media-idle`. Icons are resolved and decoded once per application on the
  info worker. A Now playing title
  shrinks no lower than 75% of the value size (18px on keys, 19px on
  sections); a playing title that still does not fit scrolls right to left at
  that size, clipped to its text column (never into the icon or chevron room):
  1.5 s still, then 30 px/s, a 28px gap before it repeats, looping seamlessly.
  A paused title stays still and ellipsized; the artist line never scrolls.
  The device and the preview draw the same frames from a monotonic clock, at
  about 10 fps and only while a title scrolls; unchanged frames are not
  resent and frames are never stored as artwork. A key stacks icon (32-44px, shrinks first), a 24px bold
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
- **Touch buttons** (Stream Deck Neo): bindable controls with their own actions,
  stored per keypad page as `tb:1` and `tb:2` in layers.conf. Selecting one heads
  the editor "Configuring: Touch button N" and shows the Event selector (Press
  only) and the Action selector; there is no hold check, no live information, no
  multi action and no picture. A touch runs the action once, when the touch
  starts: the held state the device repeats and the release run nothing, and
  there is no hold action. The preview pad's tooltip reads "Touch button N ·
  Press: <action>". Models without touch buttons offer none.
- **Information window** (Stream Deck Neo, 248x58, no touch): each keypad page
  shows its own content there, like a section of the + touch strip, and the
  window follows the shown page and profile. Selecting the window on the
  preview heads the editor "Configuring: Info window" and shows no Event or
  Action selector, no hold check and no page return: it runs nothing. Its
  Appearance section sets the page's window picture (target "Info window";
  Image… and Clear act on the active keypad page), and the Binding tab's "Show
  live information on the window" (any live-information source: time and date,
  now playing, volume, weather, ...) is saved with Save binding and cleared by
  Remove (with Undo). The reading is drawn by `info_render` over the window's
  picture without a section tile: icon on the left (42px), a 22px value, bar
  and detail beside it, or a centred 26px value with no icon (time and date);
  a long playing title scrolls as on keys. While a Start timer counts down on
  the shown page, the window shows its countdown too, then returns to its
  picture and reading. The preview tooltip reads "Info window · Live
  information: <source>" or "Info window · No live information". Stored per
  keypad page: the picture as the page's `touch-strip-background` artwork, the
  reading as a `window` entry (always `none`, it runs nothing) with its `info`
  row in layers.conf; the Neo has no touchscreen pages to follow. The device
  gets the whole window (Update Window Image 0x0B) as a JPEG turned 180°, as
  Elgato's Neo page says; this is validated with the simulator only.
- **Keypad background** (Stream Deck Classic, 480x272, the LCD behind its 15
  keys): each keypad page can have its own background picture, which follows
  the shown page and profile like the Neo's window picture. Selecting the
  Background bar under the keys heads the editor "Configuring: Background" and
  shows no Event or Action selector, no hold check, no page return and no live
  information ("Live information is not available for this control event.");
  Save binding and Test action say "The background runs no action; choose its
  picture under Appearance.". Its Appearance section ("Background picture"
  expander, target "Background") sets it with Image… and Clear, on the active
  keypad page. Remove clears it with the toast "Background removed." and an
  Undo that puts the same picture back once; Undo refuses with "Cannot undo: the
  background was changed after it was removed." when the page got another
  background since, and Remove on a page without one says "This page has no
  background.". The background shows behind every key without a picture of its
  own: such a key shows its share of the background (Elgato's "Unit Pixel
  Layout": 72px keys 25px apart, key 1 at 11,5), with its binding icon, button
  text or live information drawn over that share as over its own artwork; a
  key's own artwork covers it. Preview and device draw the same pixels. The
  bar's tooltip reads "Background · Behind every key without a picture of its
  own". The device gets the whole picture first (Update Full Screen Image
  `02 08`, 480x272 JPEG turned 180°, as Elgato's Classic page describes a Full
  LCD Image), then every key; a page without a background clears the LCD to
  black once after one was shown, and a Classic whose pages never had one
  gets key pictures only. Stored per keypad page as the page's
  `touch-strip-background` artwork, in `keypad-background.conf` beside
  artwork.conf (which keeps exactly the rows older readers accept; the file
  exists only while a page has a background and travels in bundles). No other
  model offers it (Elgato documents a full LCD image for the XL too; it is not
  offered there). Validated with the simulator only.
- **More actions:** each feature action's icon comes from the same mapping as
  saved controls (derived from a representative action). Category rows use
  System `application`, Multimedia `play-pause`, Organization `folder`, Device
  `brightness` and OBS `obs-scene`; Action rows, the guidance line (20px) and
  the closed Action button show the chosen action's icon, while the Action
  menu row keeps `more`.
- **Multi action:** a "Multi action" row in the catalog's System category, for a key press or
  key hold only: the More actions picker of a dial or the touch strip does not offer it, and
  dropping it on a dial selects nothing and says why. Choosing it replaces the action's fields
  (below its icon and description) with the step list: a "Steps" heading with the bundled
  `sdeck-multi-symbolic` icon, then a boxed, numbered list like the page folders (`desk_field`
  fill, 1px `desk_edge`, radius 10, 40px rows with a hairline between them). Each row shows its
  number (muted 13px/700), the step's binding icon (16px; a wait uses `timer`), its summary
  ("Run echo one", "Wait 2 s", "Wait 750 ms"; ellipsized, full text as tooltip) and four
  frameless 34px icon buttons: Move up, Move down, Edit step (pencil) and Remove step (red
  trash), each with a two-line tooltip (its name, then what it does to step n) and an accessible
  name naming the step ("Move step 2 up"). Up is disabled on the first row, Down on the last.
  An empty list reads "No steps yet. Add the actions to run, in order." Below it come the
  captioned **Add step** and **Add wait** (`list-add-symbolic`), disabled with an explaining
  tooltip at 16 steps. Adding, editing, moving or removing a step is an unsaved edit like any
  field; nothing runs until Test action or the key. Save refuses an invalid list with the
  model's sentence ("A multi action needs 1 to 16 steps", "A multi action needs at least one
  action step", "Step 1: Go to profile or page can only be the last step", "Could not save
  binding: Step 2: Unsupported physical key ...") in the toast and stores nothing.
  The **step dialog** (`ui::form_dialog`: modal, transient for the window, 400px, not
  resizable; the body scrolls past 460px) has a title ("Add step", "Edit step", "Add wait",
  "Edit wait"), a muted subtitle naming the step's number and one right-aligned action row:
  Cancel with its `Esc` hint, then the suggested **Add** (new step) or **Save** (edited step).
  A step shows the step form: an Action menu with the editor's action types except None (plus
  Plugin action once a plugin exists), then the editor's own fields, guidance and shortcut
  preview (`src/action_form.rs`), whose More actions offer every category except Multi action,
  Start timer, Cancel timer and the touchscreen page actions. Go to profile or page and the
  page actions are accepted here; Save of the binding allows them only as the last step. A wait
  shows a number field and an ms/s unit drop-down (10 ms to 60 s; seconds may have decimals; a
  whole number of seconds reopens in seconds). An invalid step keeps the dialog open with an
  error banner naming it ("Step 3: a wait must last 10 ms to 60 seconds", "Step 3: enter how
  long to wait, as a number"); Escape or Cancel changes nothing, and the dialog never runs the
  step.
- **Type text keys:** under the Text field (the binding editor and the step dialog share it), a
  row with a muted "Insert key:" label and one linked group of compact buttons (`text-keys`: 20px
  high, 4px/8px padding, at least 20px wide) Enter, Tab, Esc, ↑, ↓, ←, → inserts `{Enter}`,
  `{Tab}`, `{Esc}`, `{Up}`, `{Down}`, `{Left}` or `{Right}` at the cursor (replacing a selection,
  as typing would) and gives the focus back to the text. Each button has a two-line tooltip
  ("Insert {Tab}", then "Presses Tab at this point of the text.") and an accessible name such as
  "Insert Up arrow key"; the group is labelled by "Insert key:". Inserting is an unsaved edit like
  typing and never runs the action. Below the row, a muted line previews what is typed: the text
  as written and each key as ⏎ ⇥ ⎋ ↑ ↓ ← →, plus a final ⏎ when Press Enter after text is on
  ("Types: aldo ⇥ mi-clave ⏎"); it wraps to at most three lines (ellipsized, the full preview as
  its tooltip) and is hidden while nothing would be typed. The helper line under it explains the
  braces ("{{ types {"). The row fits the 400px step dialog and never widens the editor.
- **Artwork targets:** every row of the Artwork target menu starts with a
  26x22 mini map of the shown model's keys over its strip sections (eight keys
  over four sections on the +, the 9x4 grid over six sections on the + XL, at the
  same overall size; the Neo's one target reads "Info window", the Classic's
  "Background"), soft-filled
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
- **Live information option:** in the sidebar's Binding tab, a "Show live information
  on the key" check button ("on the window" for the Neo's information window)
  reveals a labeled source selector, plus a location field only for Weather.
  Keys, dials, a tap or hold on one touch section and the Neo's information
  window; the picture is drawn by `info_render` directly over the key's own
  artwork (120x120), the touch section's (200x100, icon left, value/bar/detail
  right) or the window's (248x58, the same layout without the tile), never on
  a solid background of its own. Icons and text carry the soft drop shadow
  for legibility and the bar a translucent track. Icon colors
  follow the palette: cyan audio, amber light, violet load that turns amber at
  70 and red at 85, media violet while playing, grey when inactive, paused or
  idle, red for the muted cross and recording.
  Readings come from a worker thread, never the GTK thread.
- **Action guidance:** explain the selected action and expected values beside
  its fields. Native file/folder/audio pickers populate absolute paths. A
  stale picker result must not write into another editor context.
- **Shortcut reference:** a cyan keyboard-focusable Key names and aliases link
  lives in Shortcut guidance without adding a form row. It opens a read-only
  native dialog from the parser's shared catalogue. Aliases are one table: a
  header (Key, Also accepted as), one row per canonical key with its aliases in
  monospace, columns aligned by a size group, thin row dividers and softly
  alternating rows (`reference-table`). Physical key names follow as compact
  selectable chips. The list owns vertical scrolling; Close
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
- **Profile manager:** a profile row's pencil opens Edit profile for that profile and
  the rail's "+" opens New profile, each a dedicated modal dialog; only one task is shown at a time.
  Editing groups the profile selector, optional auto-switch application, the
  "Return to the previous profile when focus leaves" check (wrapped text, enabled
  only while the application field has text and never for Base) and
  Save changes in the footer action row (Delete lives on the profile row). New profile has a name
  and the Create profile action in its footer; creating a profile closes that dialog and opens
  Edit profile on it. Deleting still asks for confirmation. Extended
  explanations use named question-mark controls with hover tooltips and
  click/keyboard help popovers, not paragraphs in the form. Dialogs use 14px
  padding, a 10px gap and 4px field-label gaps.
  Create profile is disabled for empty or duplicate names and says why under the
  field; Save changes is enabled only when the application rule or the return
  check differs from the saved state, and never for Base.
- **Popover:** OBS settings and Page folders stay anchored to the trigger, with no arrow, no
  title bar and no Close button, and native Escape/outside dismissal.
  Never open one automatically at startup.
- **Artwork disclosure:** independent key/strip/zone artwork editing remains
  reachable in a clearly named expandable section of the selected editor.
  An Appearance group separates text and artwork from the action form.
  Save binding, Test action and Remove binding are icon-only buttons that end
  the editor card's heading row, level with "Configuring: <control> · <event>"
  (artboard 03-profile-rail): a floppy disk (violet fill), a play triangle
  (neutral) and a red X in a circle. Each tooltip has two lines, its name then
  what it does, and doubles as the accessible name and test caption. The
  right-hand panel's toolbar holds only the plugin buttons, right above its
  tabs: no message ever shows below them (results and errors go to the toast at
  the top center of the window). The heading row is the editor card's fixed top:
  only the form below it scrolls, so Save, Test and Remove stay visible. The
  floppy and circled X are bundled symbolic icons (`sdeck-save-symbolic`,
  `sdeck-remove-symbolic`) so they do not depend on the desktop theme.
  The same row ends with **Add plugin…** and **Update plugins**, two icon-only buttons of the
  same size and normal toolbar spacing, add (`list-add-symbolic`) immediately before
  refresh so refresh stays right-most, pushed to the right by an expanding gap and a thin
  vertical divider so they read as separate from the three binding tools. Add plugin's
  tooltip is "Add plugin…" (accessible name "Add plugin"); it opens the add dialog below.
  Update plugins' tooltip is exactly "Update plugins" (also its accessible name); it rereads
  the plugin folder and never runs a plugin. The toolbar is always visible, for keys, dials
  and the touch strip alike, so both controls are always reachable.
  Test action remains neutral and explicitly
  labeled, with guidance that testing executes and saving only persists.
- **Button text and image:** right-clicking a key selects it and opens a small
  menu with "Add text and image…" or "Edit text and image…", which opens the
  popup anchored to that key, prefilled with its saved settings. The popup also
  has a "Background image" row naming the key's image, with Image… and Clear
  image (disabled when there is none). The Appearance group is hidden while a
  key or a dial is selected and stays for the touch strip and its zones. A muted context line under
  the title names what is edited, "Key N on “<view>”", where the view is the
  page indicator's label (profile / folders / page). The editor fields and
  enlarged preview appear only in the popup. A compact two-column body keeps
  the entire live key preview (120px, 112px on the + XL) next to the form, with no popup scrolling at
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
  outline sit above the key's background. The enlarged key preview (the model's key size) and the
  physical key preview show the key as the device does: its artwork, else its
  binding icon tile when the text leaves the icon a band, else the neutral
  black key; the derived picture is never stored. When the Pango ink extents
  exceed the key's text area (the key size less 4px), a warning-toned "Text does not fit; try a
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
  dismiss it or discard its draft. Escape, the trigger, Save/Remove and a
  key/profile change provide explicit exits.
- **Deletion confirmation:** the title names the item in curly quotes (`Delete “Studio”?`);
  the body says what goes with it and ends "This cannot be undone." (a profile counts its
  pages and bindings: "2 pages and 14 bindings go with it."). One right-aligned row: **Cancel**,
  with an `Esc` key hint and the default, then a red destructive **Delete**; Escape or Cancel changes
  nothing. Page, folder, touchscreen-page and profile deletion share this dialog; the import
  confirmation uses the same layout with an **Import** button.
- **Menu popover:** the header's Menu is a headerless command menu (6px padding, 38px rows, no
  title row, no arrow and no Close button) of Help, Updates, Import configuration... and Export
  configuration..., then a 1px separator and the **Virtual deck (Classic)** section below;
  Escape or a click outside dismisses it. The Menu's tooltip and accessible description read
  "Menu: help, updates, importing or exporting the configuration, and the virtual deck."
- **Virtual deck (Classic)** (`src/virtual_deck_ui.rs`; source artboard
  `design/virtual-classic/04-all-in-menu.html`): the menu's last section.
  - **Switch row:** 38px like a command row (8px 10px padding): a 16px `input-dialpad-symbolic`
    icon (presentation only), the bold title "Virtual deck (Classic)" and a `Switch` at the end,
    named "Virtual deck (Classic)" with the tooltip and description "Serve a virtual Stream Deck
    Classic in a web page on this network". The switch is off at every launch and insensitive while
    the server starts, moves to another port or stops. Off, the menu ends at the switch row; the
    details below it (10px side and bottom margins, 12px between groups, 344px wide so the menu
    is about 380px like the artboard) show only when there is something to show and scroll past
    620px or on a short screen. A wrapping feedback banner at their top says why turning on, or
    opening the browser from **Open**, failed. Turning on never opens a browser window by itself.
    Password fields (here and on the web login) hide characters as small bullets (`•`, 2px apart in
    the app; 13px with 0.24em spacing on the web, 16px on iOS so focus does not zoom) and reveal them
    with an eye icon (eye-off while shown), never a "Show" caption.
  - **First time** (no stored password): turning the switch on leaves it off and opens an inline
    form instead: the warning line "Set a password before turning the virtual deck on.", the
    labelled password entries "New password" and "Repeat it" (peek icon), an error line, then the
    right-aligned Cancel and suggested **Save and turn on**. Enter in either entry submits. While
    the password is hashed (on a worker) the whole form is insensitive. Errors read "At least 8
    characters.", "At most 128 characters." (length first) and "The passwords do not match."
  - **Running:** a 112px QR code of the page address (dark `desk_bg` modules on a white square
    with 8px corners, a two-module quiet zone; never the password) beside the address in a
    selectable monospace chip and the captioned **Copy** ("Copy the address", toast "Address
    copied.") and **Open** ("Open the address in the browser") buttons; the address chip's tooltip
    lists every address the page answers at. Then the helper "Certificate fingerprint · compare
    it with the browser's warning" over the whole "SHA-256 AA:BB:..." in a chip (three lines).
    Then two rows with 70px headings so their values line up: **Password** "Set" and
    **Change…**, which opens the same password form with **Change password**; and **Port**, a
    six-character digits entry ("1024 to 65535; press Enter to restart the server on it") and the
    helper "Restarts the server", with its own error banner ("Use a port from 1024 to 65535.",
    "Port N is already in use."). Then the "Viewers (n)" heading and a boxed list in the folder
    list style, named "Viewers": each 40px row a `phone-symbolic` (Android, iOS, iPadOS) or
    `computer-symbolic` icon, the browser ("Android · Chrome 131", ellipsized) over the muted
    "192.168.1.73 · since 21:04", and a **Disconnect** button named "Disconnect <browser> at
    <address>". Past four rows the list scrolls inside the menu; empty, it reads "No one is viewing
    the deck yet."
  - **Changing the password** confirms first in the shared dialog frame: "Change the password?",
    "Changing the password disconnects every viewer." (while the server restarts: "The server is restarting. Confirm once it is on; changing the password disconnects every viewer." with the accept button disabled; the form itself refuses with "The server is restarting. Change the password once it is on."), Cancel (the default) and a red **Change
    password**; success toasts "Password changed. Every viewer was disconnected."
  - **Header:** while on, the title row shows the badge "Virtual deck on · N viewers" ("1
    viewer"; violet, tooltip "Served at <address>") right after the title. The virtual Classic's
    connection line reads "Virtual Stream Deck Classic — served at <address>" in place of USB ids
    (with other devices connected it is the last line of the tooltip), "Virtual Stream Deck
    Classic turned off — turn it on from the Menu" once off, and the model comes last in the
    connected list and the Current selector. It is never remembered as the last shown model.
  - **Threads:** GTK never waits: hashing, starting, moving and stopping the server run on owned
    worker threads, presses and pictures cross an owned glue thread, and results and server
    events reach GTK through async channels. Quitting stops the server and joins every thread.
- **Feedback:** countdown, completion, connection and errors use inline text
  with semantic style. Busy workers do not block GTK.
- **OBS:** visible endpoint/password labels, masked password, Save settings
  and Test connection. Testing queries state without starting any output.
  Action choices include existing toggles plus explicit start/stop recording,
  pause/resume recording, and start/stop streaming. Choosing or saving an
  action never executes it.
- **Device model:** every model has a complete session of its own: its
  widget tree (built once, the model-driven preview and the editor), runtime,
  stores, action worker, information reader and event channel. A session is
  created the first time its model is needed (the last used model at startup,
  or the model of a device that connects) and kept alive afterwards, so two
  connected Stream Decks of different models run live at the same time, each
  with its own configuration, artwork and timers; two devices of one model
  share that model's session and mirror each other. The window is a shared
  header (title, connection status, "Current" selector, update indicator and
  menu) over a stack whose pages are the sessions' editors; the stack takes the
  size of the shown page only. The window shows one session. Automatically
  (until the user picks a model in this run) it shows the last used model when
  that model is connected, else the first connected model in the order Stream
  Deck +, Stream Deck + XL, Stream Deck Classic, Stream Deck XL, Stream Deck Neo,
  Stream Deck Mini (the registry's order); this is re-evaluated whenever a device connects or
  is unplugged, so a last used model that connects second is shown as soon as
  it connects, and the other connected model is shown when the shown model's
  last device is unplugged. With no device the window keeps what it shows (the
  last used model; the XL cannot be shown without a device). The header lists
  what is connected: one model reads exactly "<model> connected — keys, dials
  and touch strip are live", naming only the controls that model has ("Stream
  Deck Classic connected — keys are live"), two models "Stream Deck + and
  Stream Deck + XL connected — ...", three "Stream Deck +, Stream Deck Classic
  and Stream Deck XL connected — keys, dials and touch strip are live" (every
  control any of them has), two units of one model "<model> x2 connected"; models
  are named in the order Stream Deck +, Stream Deck + XL, Stream Deck Classic,
  Stream Deck XL, Stream Deck Neo, Stream Deck Mini whichever plugged in first, and with several devices the tooltip of the line starts with the whole
  sentence (the line itself may be ellipsized), then the USB ids.
  The **Current selector** sits on the title row, immediately left of Menu: the caption
  "Current" (muted, 12px) and a drop-down named "Current device" with the
  tooltip "Choose the Stream Deck to configure". It is visible only while two
  or more distinct models are connected; with none, one, or several units of
  one model it is not in the layout at all (not focusable either), so a
  window with one model looks exactly as it did before the selector existed.
  Its entries are the connected models, plain names, in the same order (Stream
  Deck +, Stream Deck + XL, Stream Deck Classic, Stream Deck XL, Stream Deck Neo,
  Stream Deck Mini), and it shows the shown model. Choosing an entry shows
  that model's session, rebinds the menu, toasts "Switched to <model>." and
  remembers the model for the next start only once the session opened; it
  never asks about unsaved edits, because the session left behind stays alive
  and keeps its draft. A pick sticks until the shown model has no connected
  device left, then the automatic rule applies again. The last shown model
  (device.json) follows every successful change of the shown model, whether
  automatic or picked. If a connected model's configuration cannot be opened,
  the shown session stays visible, read-only while none of its own devices is
  connected, with a persistent error, nothing is sent to that device and it is
  not offered in the selector. The menu's import and export, OBS and Test
  action act on the shown session; the focused application reaches every
  session. Each preview's brand line is the model's name in capitals ("STREAM
  DECK +", "STREAM DECK + XL", "STREAM DECK CLASSIC"), and the setup help
  follows the shown model.
- **Release updates:** beside the connection badge, a keyboard-operable text
  button reads "Update available: vX.Y.Z" only when a newer stable release is
  known, and opens the Updates dialog. It uses the shared cyan token in both palettes, never
  color alone. The Menu lists Help, Updates, Import configuration... and Export
  configuration... (then the virtual deck section); Updates opens a modal dialog (same width, margins and Close
  button as Help) that groups its status, Check for updates button and Check
  for updates automatically checkbox with 8px gaps. The Updates dialog never uses a
  scrolled window: text wraps at the shared helper measure and installer output
  shows at most six lines ending in an ellipsis, with the full text in the
  tooltip. Help no longer contains update controls. Checking disables
  the manual button; failures stay in the Updates status, never an automatic popup.
  A known release indicator survives retries and network failures. An owned
  worker makes one bounded HTTPS request (8 seconds, 256 KiB); shutdown
  disconnects and joins it. Automatic checks run on opening/re-presenting the
  configuration window, at most once per six hours after completion in this
  process. Manual checks bypass that interval, never the single-flight guard.
  The saved toggle changes behavior only after a successful save; malformed
  preferences remain untouched, disable writes and show an inline explanation.
  The Updates dialog shows **Update** only for an available release when the canonical running
  executable is `$HOME/.local/bin/sdeck-plus` and HOME is absolute. Other copies
  show guidance instead: `/usr` copies use their package manager; checkouts,
  custom prefixes and unknown origins must re-run the documented install command.
  Update explicitly downloads the detected tag's `install.sh` from the fixed
  upstream GitHub URL (HTTPS, 30 seconds, 512 KiB), then runs it as a file with
  system bash and `--tag vX.Y.Z`; it is never piped into a shell. The private
  script directory is removed on every exit path. Only essential PATH, HOME,
  XDG and locale environment entries survive; display and installer override
  variables do not. The owned worker captures the final 4 KiB of merged output
  and allows 30 minutes overall. While **Updating...**, Update and both check
  controls are disabled; repeated automatic/manual requests do nothing.
  Failure reads **Update failed**, with the real nonzero exit code and output
  tail when available; launch errors and deadlines explain the failure instead.
  Success reads **Updated to vX.Y.Z. Restart to use it.** and reveals the
  keyboard-operable **Restart now** button. No automatic restart or popup occurs.
  Restart requests normal quit, joins workers and releases HID and the
  GApplication bus name before spawning the same executable after `app.run()`
  returns, with null stdio and its own process group; no `--background` is added.
  Existing close behavior is unchanged: hide-to-tray keeps updating; Quit sends
  TERM to the owned process group, allows 10 seconds for installer cleanup,
  then KILLs remaining processes and joins before exit. Result callbacks hold
  weak references and cannot render into shutdown widgets. All controls use
  shared native spacing, visible English labels and existing theme tokens.
- **Tray close preference:** Keep running when window closes is checked by
  default. Checked closes hide configuration and retain device monitoring;
  unchecked closes quit the application and its workers. Persist the choice,
  and keep the previous behavior if saving fails. Explicit Quit always exits.

Shared native helpers live in `src/ui.rs`: `button(icon, caption)`,
`decorate_button(button, icon, caption)`, `popover(title, body)`,
`form_dialog(parent, title, subtitle, body, dismiss, primary)` (the dialog frame the profile
and step dialogs share), and `section_heading(icon, text)`. Colors and sizes trace to this contract.

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
toolbar/editor surfaces. The profile rail has no competing outer outline.
Profile tabs use a violet tonal fill and fine inset edge instead of an underline;
the active tab and keyboard focus remain different states.
The dimensional chassis, artwork cover/crop and circular encoder material
remain intact. No new fonts, heavy shadows, ambient backgrounds or accent outlines; the only accent
edges are the 3px left edges of the callouts and the profile note.
Secondary panels align their trailing edge with their trigger. Their title and subtitle head
the body; the form below sits in a bounded vertical scroller, and Escape or an outside click is
the exit. The scroller's natural content height is capped at 288px,
leaving room for the header and GTK popup material within a 480px-tall client.
Button text retains its full-size preview.

## 8. Accessibility and verification

Native keyboard-operable controls, visible associated labels and focus rings
are required. Icon-only controls have names/tooltips. Popovers provide a
visible way out. Error explanations say what to correct near the relevant
editor; passwords never appear in summaries or diagnostic text.

Preview controls are named after the device's own numbering: keys "Button 1"
through "Button N" (row-major), dials "Encoder 1" through "Encoder N", and
touch sections with the tooltip "Touch zone 1" through "Touch zone N", where N
is the shown model's count (36 keys, 6 dials and 6 sections for the + XL; the
+ keeps 8, 4 and 4), the Classic's background bar "Background", the Neo's
window "Info window", and touch buttons "Touch button 1" and "Touch button 2"
(the Neo's, left to right). Tab visits the keys row-major, then the Classic's
background bar, the touch strip and its sections (or the Neo's window), the
touch buttons and the dials; Space or Enter selects the focused control, and any
key of the shown model can be dragged onto another to swap them. Every model's
session uses this order and these names.

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
640x480, 1000x700 and 1280x800 allocations (the catalog beside the preview at
every width; horizontal scrolling only below the side-by-side minimum),
empty/loaded forms, long labels, every action family and all changed popovers in light/dark GTK variants.
The GTK owner test is the state/geometry harness; fresh native captures and
independent reviews establish visual quality. Web-only Lighthouse/React
tooling does not apply. Manual hardware input is reported only if observed.

## 9. Tray startup and recovery

Login startup (`--background`) and the saved **Start in tray** preference keep
configuration hidden while an owned worker waits up to 10 seconds for successful
StatusNotifierItem registration and a registered host. GTK, configuration and
HID initialization continue immediately; watcher ownership alone is not readiness.
Closing a visible configuration window while pending exits safely. After readiness,
the saved close preference applies without hiding a window the user has opened.
An additional launch always opens the existing configuration window.

If the deadline expires without a usable tray, present configuration and the
existing accessible error explanation, including how to enable a StatusNotifier
tray and that closing the window exits. Do not change saved preferences or leave
an inaccessible background process. Shutdown cancels registration, retries and
GTK callbacks, releases the application hold and joins the owned worker.

## 10. Virtual deck web page

The virtual Stream Deck Classic's pages (`src/virtual_deck/page/index.html` and `login.html`,
served by `src/virtual_deck.rs`) follow the chosen artboard "02 · Keys first"
(`design/virtual-classic/02-keys-first.html`); 01 and 03 are rejected alternatives. They are
self-contained (inline CSS and script, no external URL), dark only (`color-scheme: dark`, the
`desk_*` dark values as CSS variables) and in English. Validated in headless Chromium only (844x390,
915x412, 390x844, 1280x720), never on a real phone.

- **Keys first:** no chassis. The 5x3 keys fill a phone held sideways: the key side is the
  largest that fits five columns and three rows in the stage (gap 2.5% of the shorter side,
  6 to 14px; at least 24px), inside `env(safe-area-inset-*)` with at least 8px top and bottom,
  10px right and 12px left, and 56px kept free at the right for the menu button. No zoom
  (`user-scalable=no`), no page scroll (fixed body, `overflow: hidden`), `touch-action: none`,
  and no context menu, selection, drag or pinch.
- **Key:** black `#08090b` with corners of 7% of the key (6 to 14px) and the device's inset
  `#515b65` 2px edge; the picture covers it. The Classic's 480x272 background shows through every
  key at its own place (key 1 at 11,5, a 97px pitch), as on the device. Pressed: a 4px green
  `#22c55e` inset edge and the picture 25% brighter; with a mouse, hover shows the control edge.
  Brightness dims the pictures (0% darkens them by 90%).
- **Hold bar:** a key held for 0.3s shows a bar at its foot (8% side insets, 7% from the bottom,
  at least 4px high, a pale track) whose green fill grows linearly to full at 2s, when
  sdeck-plus runs the hold action. With reduced motion it jumps to full at 2s. Releasing hides it.
- **Input:** pointer events, one key per pointer, several at once; a key pressed by two pointers
  goes up when the last lets go; moving off a key past the gap (at least 6px of slack) lets go;
  only the mouse's left button presses; Enter or Space presses a focused key. Hiding the page,
  leaving the window or losing the connection lets go of every key.
- **Chrome behind one button:** a single 44px icon button at the right edge, vertically centred,
  named "Deck menu: status, full screen, leave", opens a sheet (role dialog "Deck menu", 280px,
  12px corners, 44px rows): the status with its dot ("Connected to sdeck-plus."), the address,
  then **Full screen** / **Exit full screen** (hidden where the browser has no Fullscreen API;
  on Android it also asks to lock landscape) and a red **Leave**, which logs out. Escape or a tap
  outside closes the sheet without pressing a key. An 8px dot by the button is amber while
  connecting or reconnecting, green while connected and red once disconnected, and has an
  accessible name ("Connecting", "Connected", "Reconnecting", "Disconnected").
- **Overlays:** while not connected the keys are inert, dimmed to 35% and partly greyed, and a
  centred card says why: "Connection lost. Reconnecting…" with a spinner, "Keys are paused until
  the deck is back. If sdeck-plus turned the virtual deck off, this page waits here." and **Try
  now** (retries back off from 1s to 10s); "The virtual deck was turned off in sdeck-plus." with
  "Keys are paused. This page connects again when the virtual deck is turned back on."; "This
  device was disconnected from sdeck-plus." or "The password changed. Log in again." with a violet
  **Log in again**. Status changes are announced in a polite live region.
- **Portrait:** above the keys, "Turn your phone sideways for bigger keys. The deck still works
  here." with a rotate glyph; the menu button and its sheet move to the top right.
- **Screen:** the page asks for a Screen Wake Lock when it opens, connects, takes a press or is
  shown again, where the browser offers it (HTTPS pages only).
- **Login page:** "Virtual Stream Deck", "Classic · served by sdeck-plus" (no host name), "Ask
  the person running sdeck-plus for the password.", a labelled **Password** entry with
  **Show**/**Hide**, an alert banner and a violet **Connect**. Wrong: "Wrong password." with "N
  tries left before a 30-second wait."; locked: "Too many tries." with "Try again in N s."
  counting down while Connect is disabled. Without JavaScript the form still posts.
