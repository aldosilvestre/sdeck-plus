# streamdeck-plus-website-redesign

## Objective
Make the GitHub Pages site (docs/) much more modern, less centered (asymmetric,
editorial layouts), dynamic and interactive, keeping all current content.

## Problem
docs/index.html (860 lines, dark-only) is a long single page of centered
sections and card grids; the Configuration and Development material moved from
the README makes it very long and hard to scan. No interactivity beyond anchors.

## Scope / constraints
- Static GitHub Pages from docs/ (no build step). Vanilla HTML/CSS/JS, no
  frameworks; any external resource only from pinned, reputable CDNs or Google
  Fonts, preferably none (self-contained).
- Keep every fact currently on the site (features, actions, install,
  configuration, development); reorganize, do not drop.
- Accessible: semantic landmarks, keyboard operable, visible focus, WCAG AA
  contrast, `prefers-reduced-motion` respected, works without JS (content
  visible, progressive enhancement).
- Responsive from 360px to wide desktop, no horizontal scroll.
- Light and dark themes (system default + toggle remembered in localStorage,
  wrapped in try/catch).
- Existing screenshots, icon.svg, social-card.png reused; sitemap.xml updated.
- Branch `feat/website-redesign` from main 80a93f0.

## Tasks
- [x] W1 Structure: landing page (index.html) with asymmetric hero and feature storytelling; docs split into dedicated page(s) (e.g. docs.html with sticky sidebar table of contents and scroll-spy) for Configuration and Development; shared CSS/JS files; nav, footer, sitemap.
- [x] W2 Interactivity: interactive Stream Deck + mockup in the hero (click/hover keys, dials and strip to see example actions and live info), screenshot gallery with lightbox and keyboard navigation, tabbed/filterable actions table, copy buttons on code blocks, docs search/filter, scroll reveal animations (reduced-motion safe), theme toggle.
- [x] W3 Verification and polish: headless Chromium screenshots at 375/768/1440 in light and dark, link/anchor check, HTML validity check, no console errors; README/AGENTS links still valid; CHANGELOG entry.

## Route
Delegated direct (one writer): 2+ non-trivial files.

## Checks
HTML parse + nesting check; all href/src targets exist; all in-page anchors resolve;
`chromium --headless --screenshot` renders at 375/768/1440 light+dark without
horizontal overflow; JS has no console errors (chromium --enable-logging);
`git diff --check`.

## Progress
Created 2026-10-27. Engram mirror pending (engram tools unavailable this session).

### W1 Structure
- Route: delegated direct (single writer).
- Files: `docs/index.html` (landing: split hero with device preview, works-with
  ticker, capabilities bento, 14 alternating feature rows, gallery, actions,
  install summary, documentation teaser with `#configuration`/`#development`
  ids, CTA band, footer), `docs/docs.html` (Install, Configuration and
  Development with sticky TOC), `docs/assets/site.css` (tokens on `:root`,
  light/dark via `prefers-color-scheme` and `data-theme`), `docs/assets/site.js`
  (theme toggle, mobile menu, scroll reveal, TOC scroll-spy and collapsible
  mobile TOC), `docs/404.html`, `docs/sitemap.xml` (both pages), README links
  moved to `docs.html#install|#configuration|#development`.
- Old anchors: an inline head script on index.html forwards `#configuration`,
  `#development` and the docs sub-anchors to docs.html; without JS the landing
  teaser cards carry those ids.
- Font: one Google Font (Space Grotesk, headings); body uses system UI fonts.
- Checks: HTML parse/nesting and link/anchor check clean (see W3).
- Commit: 94dc54a
- Note: this commit's script holds only the W1 behaviour; the tab list and the
  docs filter field are styled but become interactive in W2.

### W2 Interactivity
- Route: delegated direct (single writer). File: `docs/assets/site.js`.
- Device preview: every key, touch section and dial is a button; hover, focus
  or click fills the caption panel (aria-live) with what it can do; arrow keys,
  Home and End move between controls. Live demo: real-time clock key, CPU
  random walk with load colors, Now playing marquee that scrolls only when the
  title overflows; Key 4 toggles a recording state, Key 5 play/pause (greys the
  title and stops it), Key 6 rotates outputs; dials turn on click (volume and
  brightness update the strip, brightness dims the preview, tracks change the
  title); strip edge arrows and pointer swipes switch between two touch pages.
  Motion-based demos stop under reduced motion.
- Lightbox: native modal `dialog` over all 26 screenshots, arrow/Home/End keys,
  Esc and backdrop close, focus returns to the thumbnail; without JS the
  thumbnails link to the image files.
- Actions: ARIA tabs (roving tabindex, arrows, Home/End); without JS all
  category panels are listed.
- Copy buttons on every code block: Clipboard API, `execCommand` fallback, and
  a final fallback that selects the code; status announced in a live region.
- Docs filter: hides non-matching cards, groups and sections, dims TOC links,
  highlights matches, shows a no-results card and a count; `/` focuses it,
  Esc clears, Enter jumps to the first match, `?q=` pre-fills it.
- Bento tiles follow the pointer with a soft glow.
- Checks: scripted headless interaction run at 375/768/1440 (see W3).
- Commit: e528860

### W3 Verification and polish
- Route: delegated direct (single writer), scratch outputs outside the repo.
- HTML parse/nesting (python3 -I html.parser tag stack) on index, docs, 404:
  0 errors, 0 unclosed, 0 duplicate ids.
- Links/anchors: 261 href/src (files, ids, sprite `#i-*` refs, `/sdeck-plus/`
  paths on 404) resolve; 0 broken.
- Chromium headless (`--force-prefers-reduced-motion` for deterministic frames)
  at 375, 768 and 1440, light and dark, both pages. Fixes made: caption card
  no longer covers the dials; marquee measured on the block so it scrolls;
  1000x950 captures cropped by CSS `aspect-ratio`/`object-fit` to their content
  (full image in the lightbox); small popovers inset over the large capture;
  gallery regrid; list bullets no longer split inline `<strong>`; mobile code
  blocks pad above the copy button; footer double spacing; scroll-spy defaults
  to the first section; lightbox focus return deferred past the browser's own.
- Console/overflow: 0 JS errors and no horizontal overflow at 360/375/768/1440
  on both pages; scripted interactions (keys, dials, strip pages, tabs,
  lightbox, theme, copy fallback, docs filter) behave as specified.
- Content coverage vs `main:docs/index.html`: 501 text blocks, 0 missing
  (only rewording: the Actions lead now ends "provides the categories below").
- Commit: e57647b
- `git diff --check` clean. CHANGELOG entry added; AGENTS.md row updated
  (git-excluded, not committed).
- Not verifiable here: real clipboard write (headless file:// exercised the
  select fallback), screen-reader pass, Space Grotesk loading (offline renders
  used the system fallback).

## Next step
Human review; push / PR are the user's decision. Engram mirror still pending.
