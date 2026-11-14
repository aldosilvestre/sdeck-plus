# Popups and buttons follow the approved artboards
Bring every popup (popover, menu, dropdown list, dialog) and button family in line with design/overlays/03-compact-command.html and design/main-window/03-profile-rail.html.
Branch: feat/popups-buttons-design (from main 6e1b151).

## Specs
- S1: "lo que vi es que no mejoraste el diseño de los popus y botones segun el diseño que me pasaste." -> popups and buttons follow the approved artboards (gap audit L2).
- S2: Buttons: default (field fill, 1px control edge, radius 8, padding 6px 12px, min-height 34, icon-label gap 6); primary (violet fill, weight 600, no border); danger (error text, tinted fill .14, border .5, weight 600) for destructive buttons everywhere; flat = transparent fill AND border (today flat buttons keep a grey outline); icon-only 34px square; pressed/selected = selection fill + 1px inset violet; focus = 2px cyan outline. Catalog filters become radius-8 34px buttons with the pressed style (not pills).
- S3: Popovers: surface, 1px edge, radius 12, shadow 0 8px 24px, padding 6, no arrow, no title bar and no Close button (dismiss with Esc or clicking outside). Rows min-height 38, padding 8px 10px, radius 8, gap 10, hover fill, selected = sel + inset violet; separators 1px with 6px vertical margin; real keyboard shortcuts shown as trailing kbd hints. Dropdown list popups and the key context menu use the same rows. Form popovers (OBS settings, Button text, Page folders) drop the title bar and Close: a bold title line plus muted subtitle inside the body, like the help popover.
- S4: Dialogs (confirmations, page name, profile dialogs, setup help, shortcut reference, artwork error): radius 12, padding 14, gap 10, title 16/700, muted body; one right-aligned action row with natural-width buttons, gap 8: [Cancel Esc] [primary or danger action]. No full-width action buttons; no actions split across two rows.
- S5: Page bar: Previous/Next (and Parent) as flat 34px icon buttons; Folders and New page flat; rename/delete flat icons.
- S6: Later user decisions stay: Menu at the header's top-right; + beside PROFILES is the only New profile control; per-row Edit/Delete icons; fixed rail; catalog beside the preview; hardware preview look unchanged.
- S8: "tambien si podemos hacer el cambio que cuando se abra la ventana de sdeck-plus lo haga como una ventana flotante. Por que sino se genera un scroll horizontal incomodo e antiintuitivo" -> the main window opens floating (not tiled) so it gets its default size; approach pending user choice.
- S7: Docs (DESIGN.md, docs/*.html, CHANGELOG.md, design/system.md stale values) and screenshots match.

## Tasks
- T1 (S2,S5,S6) button system in CSS + page bar + catalog filters. done; worker mwfuk0l3-p-w2rk; commit: see L4.
- T2 (S3,S4,S6) popovers, dropdown rows, key context menu, form popovers and dialogs. pending; route: bounded worker after T1; commit: pending.
- T4 (S8) main window opens floating. blocked: approach choice.
- T3 (S7) docs, design/system.md, screenshots; final verification. pending; commit: pending.

## Log
L1: "lo que vi es que no mejoraste el diseño de los popus y botones segun el diseño que me pasaste."
L2: explorer mwfufka2-o-dbrh gap audit: flat buttons keep a grey border (ui.css:74 clears fill only); ui::popover (ui.rs:696) adds title + Close + 16px margins for OBS/Button text/Folders; popover arrows on; dropdown rows unsized; command menu rows/hints; key context menu is an outlined button; MessageDialog action area full-width; artwork error dialog unstyled (main.rs:4823); profile dialogs split actions across two rows; setup help/shortcut reference unpadded without Esc hint; destructive buttons red text only; page bar framed+linked; catalog chips are pills; toast padding 6 vs 8. Artboards do not draw form popovers/dialogs or dropdown lists: S3/S4 apply the menu/dialog rules to them. Placement gaps contradicting later user decisions (Menu in header, + only) are kept per S6.
L3: "tambien si podemos hacer el cambio que cuando se abra la ventana de sdeck-plus lo haga como una ventana flotante. Por que sino se genera un scroll horizontal incomodo e antiintuitivo" -> S8, T4.
L4: T1: base buttons 34px radius 8, flat = no fill and no border, danger style, 34px icon buttons, pressed = sel + inset violet, chips radius 8 34px, page bar unlinked flat. RED 4/4 then GREEN 5/5; full GTK 384 pass + 3 baseline + selector intermittent + model_session widget-count test (passes alone); headless 0; fmt/clippy 0. Parent: fmt, diff-check, button_system 5/5, viewed main-window-dark capture. Worker left stash@{0} (subset of the tree) — awaiting OK to drop.
