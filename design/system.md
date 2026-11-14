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
| danger fill / edge | `alpha(error, 0.14)` / `alpha(error, 0.5)` (hover fill `alpha(error, 0.24)`) | same | `button.destructive-action`, `.confirm-dialog button.destructive-action` | src/ui.css |
| pressed / selected | `desk_selection` fill + `inset 0 0 0 1px desk_violet` | same | `button:checked`, `popover row:selected` | src/ui.css |
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

Declared paddings and gaps in the sources: 2, 4, 6, 8, 10, 12, 14, 16, 20 px (cards 12, editor card 16, buttons 6px 12px, popover 6, popover rows 8px 10px, dialog content 14 with 10px gap, folder list rows 4px 6px 4px 12px, catalog 14px 12px, catalog filters 6px 8px, toast 8px 8px 8px 16px, banner 6px 10px). Source: src/ui.css. DESIGN.md prescribes no numeric grid beyond these.

## Radius

| Element | Radius | Source |
| --- | --- | --- |
| Card, tab header | 14px | src/ui.css (`.card`, `.profile-tabs > header`) |
| Popover (menus, dropdown lists, form and help popovers), dialog | 12px | src/ui.css (`popover > contents`, `.form-popover > contents`, `.help-popover > contents`, `window.popup-dialog`) |
| Toast, toast buttons | 999px | src/ui.css (`.toast`, `.toast button`) |
| Boxed lists, reference table | 10px | src/ui.css (`.folder-list`, `.reference-table`) |
| Buttons (34px), catalog filters, popover rows, tooltips, banners, fields, tabs, notes, status badges | 8px | src/ui.css (`button`, `.catalog-chip`, `popover row`, `tooltip`, `.banner`, `.status`, `.profile-note`) |
| Chip, small toggle | 6px | src/ui.css (`.reference-chip`, `.button-text-position button`) |
| Count pill | 999px | src/ui.css (`.catalog-count`) |

## Shadows

| Name | Value | Source |
| --- | --- | --- |
| Card inset highlight | `inset 0 1px alpha(desk_ink, 0.025)` | src/ui.css `.card` |
| Popover / tooltip | dark `0 8px 24px alpha(desk_bg, 0.55)`; light `0 8px 24px alpha(desk_ink, 0.18)` | src/ui.css, src/ui-light.css |
| Toast | dark `0 4px 16px alpha(#000000, 0.30)`; light `0 4px 16px alpha(desk_ink, 0.18)` | src/ui.css, src/ui-light.css |
| Pressed button / selected row | `inset 0 0 0 1px desk_violet` | src/ui.css `button:checked`, `popover row:selected` |
| Form and help popover | `0 8px 24px alpha(black, 0.45)` | src/ui.css `.form-popover > contents`, `.help-popover > contents` |
| Entry focus | `inset 0 0 0 1px desk_cyan` | src/ui.css `entry:focus-within` |
| Selected toggle edge | `inset 0 0 0 1px desk_violet` | src/ui.css `.modifier-toggles > button:checked`, `.profile-tabs tab:checked` |

No blur, backdrop filter or complex gradients exist outside the hardware preview chassis.

## Focus

Keyboard focus is a 2px `desk_cyan` outline: `outline: 2px solid @desk_cyan; outline-offset: -2px` on buttons, dropdowns, text views, tabs, rows and catalog entries (offset 2px on links). Entries and spin buttons use `entry:focus-within` (1px inset cyan edge). Focus is never the same as selection (selection is violet). Source: src/ui.css; DESIGN.md.

## Components

| Component | Rule | Source |
| --- | --- | --- |
| Button | fill `desk_field`, ink text, 1px `desk_control_edge`, radius 8, padding 6px 12px, `min-height` 20px (content box; GTK adds padding and edge, 34px total), icon-label gap 6 (`border-spacing`), no shadow; hover `desk_hover`; active mix of hover and violet 18%; pressed (`:checked`) `desk_selection` fill with `inset 0 0 0 1px desk_violet`; disabled muted text on field | src/ui.css `button`, `button:checked` |
| Flat button | transparent fill AND transparent edge, `desk_hover` on hover, `desk_selection` when checked | src/ui.css `button.flat` |
| Suggested action (violet) | fill `desk_violet`, text `desk_bg`, weight 600, no edge; hover/active per Colors; disabled `desk_field`, muted text, `desk_edge` border | src/ui.css `.suggested-action` |
| Danger (destructive) | error text while enabled, fill `alpha(error, 0.14)`, 1px `alpha(error, 0.5)` edge, weight 600; disabled falls back to field fill, control edge, weight 400; flat destructive icons stay transparent with `alpha(error, 0.12)` on hover; the editor heading row keeps the neutral field fill | src/ui.css `.destructive-action:not(:disabled)`, `button.destructive-action`, `.editor-heading-row button.destructive-action` |
| Icon-only button | `button.icon-only` min 32px square + 1px edge = 34px, padding 0; the editor toolbar and heading row set 34px; needs an accessible name | src/ui.css `button.icon-only`, `.binding-toolbar button.icon-only` |
| Catalog filter | toggle button: radius 8, padding 6px 8px, `min-height` 20px (34px total), 12px text, weight 600 when checked, pressed style from `button:checked` | src/ui.css `.catalog-chip` |
| Page bar | Previous, Next and Parent are separate flat buttons, 4px apart (`.page-navigation`); edits use `button.flat` | src/ui.css `.page-navigation`, `.page-edits button.flat` |
| Entry | field fill and control edge like a button, caret `desk_cyan`, selection violet on `desk_bg` text; `:focus-within` 1px inset cyan | src/ui.css |
| Card | `desk_surface`, 1px `desk_edge`, radius 14, padding 12 (editor card 16), inset 1px highlight | src/ui.css `.card` |
| Chip | monospace 12, padding 3px 8px, radius 6, `desk_field` fill, 1px `desk_edge` | src/ui.css `.reference-chip` |
| Count pill | 11px muted on `desk_field`, radius 999, padding 1px 7px | src/ui.css `.catalog-count` |
| Feedback text | `.feedback-error` error; `.feedback-success` success; `.feedback-warning` warning; always paired with words, never colour alone | src/ui.css |
| Status badge | 13 / 600, padding 8px 12px, radius 8, tone text on `alpha(tone, 0.16)` with `alpha(tone, 0.55)` border (`.status-idle` warning, `.status-online` success, `.status-error` error) | src/ui.css `.status` |
| Profile note | padding 8px 10px, radius 8, `desk_field` at 90% with a 3px `desk_cyan` left edge | src/ui.css `.profile-note` |
| Popover | `desk_surface`, 1px `desk_edge`, radius 12, padding 6, shadow per Shadows; `popover arrow` is still styled but the artboard popovers declare no arrow | src/ui.css `popover > contents` |
| Popover row (menu, dropdown list) | `min-height` 22px + padding 8px 10px = 38px, radius 8, 10px gap, hover `desk_hover`, selected `desk_selection` + inset violet; separator 1px `desk_edge` with 6px vertical margin | src/ui.css `popover row`, `popover listview > row`, `popover separator` |
| Command row (key context menu) | button row: padding 8px 10px, radius 8, min-width 160, transparent fill and edge, hover `desk_hover`, checked selection + inset violet; destructive variant in error text | src/ui.css `popover.command-menu button.command-row` |
| Form popover | bold `.popover-title` and muted `.popover-subtitle` inside the body (no title bar), violet `.popover-icon` | src/ui.css `.form-popover` |
| Dialog | `desk_surface`, 1px `desk_edge`, radius 12; content padding 14, gap 10; title 16 / 700 (`.dialog-title`); body 13 muted; action box margin 0, padding 0 | src/ui.css `window.popup-dialog`, `.popup-dialog .dialog-vbox`, `.dialog-title`, `.confirm-dialog label` |
| Toast | `desk_hover` fill, 1px `desk_edge`, radius 999, padding 8px 8px 8px 16px, shadow per Shadows; buttons 28px minimum with radius 999; action in `desk_violet` weight 600 | src/ui.css `.toast`, `.toast button`, `.toast .toast-action` |
| Key hint | monospace 11, muted on `desk_field`, 1px `desk_edge`, radius 6, padding 0 6px, after a button caption (`Cancel Esc`) | src/ui.css `.key-hint` |
| Banner | padding 6px 10px, radius 8, `alpha(tone, 0.16)` fill, message in ink, tone on the glyph only | src/ui.css `.banner` |
| Segmented tabs / linked buttons | linked buttons: radius 8 on the outer corners only, shared edge; checked: `desk_selection` fill, ink text, weight 600, 1px inset violet edge (`.modifier-toggles`, `.profile-tabs tab:checked`); unchecked muted on transparent | src/ui.css `.linked`, `stackswitcher`, `.profile-tabs` |
| Boxed list | `desk_field`, 1px `desk_edge`, radius 10; rows min-height 40, padding 4px 6px 4px 12px, 1px dividers, hover `desk_hover` | src/ui.css `.folder-list` |
| Separator | 1px `desk_edge` | src/ui.css `separator` |
| Check box | field fill, control edge; checked fill `desk_violet` | src/ui.css `checkbutton` |
| Plugin heading | 11 / 700 muted label with padding 2px 8px 0 | src/ui.css `.plugin-heading` |
| Plugin add dialog preview | padding 2px 2px 8px; section labels 13px with 6px top padding; add button 28px square | src/ui.css `.plugin-add-dialog`, `button.plugin-add` |

## Motion

Transitions are 140ms ease-out on background-color and color, only when the app's motion setting is on (`.motion-enabled`); otherwise 0ms. Artboards honour `prefers-reduced-motion`. Source: src/ui.css.
