# sdeck-plus design system

Synced from the native GTK4 app, read only. Sources read: `src/ui.css` (dark default, token definitions and component rules), `src/ui-light.css` (light overrides of the same tokens), `DESIGN.md` (visual and interaction contract, Add plugin dialog section). `docs/assets/site.css` is the marketing site and is deliberately ignored.

Every value below was found in those files. Where a value is not declared (the font family, widget heights that GTK derives) it says so.

## Colors

Tokens are `@define-color desk_*` in `src/ui.css` (dark); `src/ui-light.css` redefines the same names. Dark is the default.

| Token | Dark | Light | Usage | Source |
| --- | --- | --- | --- | --- |
| `desk_bg` | `#15191f` | `#f3f5f7` | window, scrolled area and app-shell background; text on violet buttons | src/ui.css, src/ui-light.css |
| `desk_surface` | `#20262e` | `#ffffff` | cards, popovers, tab header, reference table | src/ui.css, src/ui-light.css |
| `desk_field` | `#191e25` | `#f7f8fa` | buttons, entries, inset rows, chips, disabled controls | src/ui.css, src/ui-light.css |
| `desk_hover` | `#303943` | `#e6eaef` | hover fill, toast fill | src/ui.css, src/ui-light.css |
| `desk_edge` | `#353e49` | `#d7dde4` | card, chip, separator and list borders | src/ui.css, src/ui-light.css |
| `desk_control_edge` | `#76828f` | `#7a8491` | border of buttons, entries and check boxes (3:1 control boundary) | src/ui.css, src/ui-light.css |
| `desk_ink` | `#f1f4f7` | `#242c35` | primary text | src/ui.css, src/ui-light.css |
| `desk_muted` | `#b0bac6` | `#536170` | secondary text, helper, disabled text | src/ui.css, src/ui-light.css |
| `desk_violet` | `#b6a1ef` | `#6746bb` | primary (suggested) action, check box on, text selection, folder icon | src/ui.css, src/ui-light.css |
| `desk_selection` | `#4e426d` | `#e8e1f6` | selected segment / tab / toggle fill | src/ui.css, src/ui-light.css |
| `desk_cyan` | `#85dce8` | `#0e7183` | focus outline, entry focus edge, links, caret, profile-note edge | src/ui.css, src/ui-light.css |
| `desk_error` | `#fb7185` | `#c0243f` | error text, destructive actions, `.status-error` | src/ui.css, src/ui-light.css |
| `desk_success` | `#7de2b3` | `#1b7f55` | success text, `.status-online` | src/ui.css, src/ui-light.css |
| `desk_warning` | `#f9cc7a` | `#9a6a00` | warning text, `.status-idle` | src/ui.css, src/ui-light.css |

Derived (also from the sources):

| Name | Dark | Light | Usage | Source |
| --- | --- | --- | --- | --- |
| suggested hover | `#c7b6f5` | `#5939a9` | `.suggested-action:hover` | src/ui.css, src/ui-light.css |
| suggested active | `#a58cde` | `#4c2f95` | `.suggested-action:active` | src/ui.css, src/ui-light.css |
| tone fill | `alpha(tone, 0.16)` | same | `.status-*` badge fill | src/ui.css |
| tone edge | `alpha(tone, 0.55)` | same | `.status-*` badge border | src/ui.css |
| destructive fill / edge | `alpha(error, 0.14)` / `alpha(error, 0.5)` | same | `.confirm-dialog button.destructive-action` | src/ui.css |
| checked fill | `alpha(violet, 0.20)` | same | `button:checked` | src/ui.css |
| hairline | `alpha(edge, 0.6-0.8)` | same | catalog section and toolbar dividers | src/ui.css |

## Typography

- Family: not declared in the CSS; the app uses the GTK system font. Artboards use a system stack (`system-ui`), and `monospace` for chips (declared: `.reference-chip`, `.reference-row label`).
- Base size: `.app-shell` 14px (src/ui.css).

| Role | Size / weight | Class | Source |
| --- | --- | --- | --- |
| Title | 20 / 700 | `.title` | src/ui.css |
| Control heading | 18 / 700 | `.control-heading` | src/ui.css |
| Confirm title | 16 / 700 | `.confirm-dialog label.title` | src/ui.css |
| Section label | 14 / 600 | `.section-label` | src/ui.css |
| Body / field label | 14 / 400 | `.field-label`, `.app-shell` | src/ui.css |
| Muted / helper | 13 / 400, `desk_muted` | `.muted`, `.helper` | src/ui.css |
| Status / inspector heading / binding status | 13 / 600-700 | `.status`, `.inspector-heading`, `.binding-status` | src/ui.css |
| Plugin dialog preview section label | 13 / 600 | `.plugin-add-dialog .plugin-preview .section-label` | src/ui.css |
| Chip | 12 monospace | `.reference-chip` | src/ui.css |
| Small muted label | 12 | `.model-selector label.muted` | src/ui.css |
| Plugin heading / catalog subtitle / count | 11 / 700 (heading), 11 (subtitle, count) | `.plugin-heading`, `.catalog-subtitle`, `.catalog-count` | src/ui.css |

## Spacing

Declared paddings and gaps in the sources: 2, 4, 6, 8, 10, 12, 14, 16, 20 px (cards 12, editor card 16, buttons 6px 10px, confirm buttons 4px 12px, list rows 4px 6px 4px 12px, catalog 14px 12px, toast 12px 16px). Source: src/ui.css. DESIGN.md prescribes no numeric grid beyond these.

## Radius

| Element | Radius | Source |
| --- | --- | --- |
| Card, tab header, popover | 14px | src/ui.css (`.card`, `.profile-tabs > header`, `popover > contents`) |
| Toast, help popover | 12px | src/ui.css (`.toast`, `.help-popover`) |
| Boxed lists, reference table | 10px | src/ui.css (`.folder-list`, `.reference-table`) |
| Buttons, fields, tabs, notes, status badges | 8px | src/ui.css (`button`, `.status`, `.profile-note`) |
| Chip, small toggle | 6px | src/ui.css (`.reference-chip`, `.button-text-position button`) |
| Count pill | 999px | src/ui.css (`.catalog-count`) |

## Shadows

| Name | Value | Source |
| --- | --- | --- |
| Card inset highlight | `inset 0 1px alpha(desk_ink, 0.025)` | src/ui.css `.card` |
| Popover / tooltip | dark `0 8px 24px alpha(desk_bg, 0.55)`; light `0 8px 24px alpha(desk_ink, 0.18)` | src/ui.css, src/ui-light.css |
| Toast | dark `0 4px 16px alpha(#000000, 0.30)`; light `0 4px 16px alpha(desk_ink, 0.18)` | src/ui.css, src/ui-light.css |
| Organize popover | `0 14px 34px alpha(black, 0.28)` | src/ui.css |
| Entry focus | `inset 0 0 0 1px desk_cyan` | src/ui.css `entry:focus-within` |
| Selected toggle edge | `inset 0 0 0 1px desk_violet` | src/ui.css `.modifier-toggles > button:checked`, `.profile-tabs tab:checked` |

No blur, backdrop filter or complex gradients exist outside the hardware preview chassis.

## Focus

Keyboard focus is a 2px `desk_cyan` outline: `outline: 2px solid @desk_cyan; outline-offset: -2px` on buttons, dropdowns, text views, tabs, rows and catalog entries (offset 2px on links). Entries and spin buttons use `entry:focus-within` (1px inset cyan edge). Focus is never the same as selection (selection is violet). Source: src/ui.css; DESIGN.md.

## Components

| Component | Rule | Source |
| --- | --- | --- |
| Button | fill `desk_field`, ink text, 1px `desk_control_edge`, radius 8, no shadow; hover `desk_hover`; active mix of hover and violet 18%; checked `alpha(violet, 0.20)`; disabled muted text on field | src/ui.css `button` |
| Flat button | transparent fill and edge, `desk_hover` on hover | src/ui.css `button.flat` |
| Suggested action (violet) | fill `desk_violet`, text `desk_bg`, weight 600, no edge; hover/active per Colors; disabled `desk_field`, muted text, `desk_edge` border | src/ui.css `.suggested-action` |
| Destructive | error-coloured text while enabled; confirm variant: fill `alpha(error, 0.14)`, 1px `alpha(error, 0.5)`, weight 600 | src/ui.css `.destructive-action`, `.confirm-dialog` |
| Icon-only toolbar button | 34px square, padding 0 (`.binding-toolbar button.icon-only`); generic `button.icon-only` min 20px, padding 6px; needs an accessible name | src/ui.css |
| Entry | field fill and control edge like a button, caret `desk_cyan`, selection violet on `desk_bg` text; `:focus-within` 1px inset cyan | src/ui.css |
| Card | `desk_surface`, 1px `desk_edge`, radius 14, padding 12 (editor card 16), inset 1px highlight | src/ui.css `.card` |
| Chip | monospace 12, padding 3px 8px, radius 6, `desk_field` fill, 1px `desk_edge` | src/ui.css `.reference-chip` |
| Count pill | 11px muted on `desk_field`, radius 999, padding 1px 7px | src/ui.css `.catalog-count` |
| Feedback text | `.feedback-error` error; `.feedback-success` success; `.feedback-warning` warning; always paired with words, never colour alone | src/ui.css |
| Status badge | 13 / 600, padding 8px 12px, radius 8, tone text on `alpha(tone, 0.16)` with `alpha(tone, 0.55)` border (`.status-idle` warning, `.status-online` success, `.status-error` error) | src/ui.css `.status` |
| Profile note | padding 8px 10px, radius 8, `desk_field` at 90% with a 3px `desk_cyan` left edge | src/ui.css `.profile-note` |
| Toast | `desk_hover` fill, 1px `desk_edge`, radius 12, padding 12px 16px, shadow per Shadows | src/ui.css `.toast` |
| Segmented tabs / linked buttons | linked buttons: radius 8 on the outer corners only, shared edge; checked: `desk_selection` fill, ink text, weight 600, 1px inset violet edge (`.modifier-toggles`, `.profile-tabs tab:checked`); unchecked muted on transparent | src/ui.css `.linked`, `stackswitcher`, `.profile-tabs` |
| Boxed list | `desk_field`, 1px `desk_edge`, radius 10; rows min-height 40, 1px dividers, hover `desk_hover` | src/ui.css `.folder-list` |
| Separator | 1px `desk_edge` | src/ui.css `separator` |
| Check box | field fill, control edge; checked fill `desk_violet` | src/ui.css `checkbutton` |
| Plugin heading | 11 / 700 muted label with padding 2px 8px 0 | src/ui.css `.plugin-heading` |
| Plugin add dialog preview | padding 2px 2px 8px; section labels 13px with 6px top padding; add button 28px square | src/ui.css `.plugin-add-dialog`, `button.plugin-add` |

## Motion

Transitions are 140ms ease-out on background-color and color, only when the app's motion setting is on (`.motion-enabled`); otherwise 0ms. Artboards honour `prefers-reduced-motion`. Source: src/ui.css.
