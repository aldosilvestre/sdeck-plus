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
- [ ] W2 Interactivity: interactive Stream Deck + mockup in the hero (click/hover keys, dials and strip to see example actions and live info), screenshot gallery with lightbox and keyboard navigation, tabbed/filterable actions table, copy buttons on code blocks, docs search/filter, scroll reveal animations (reduced-motion safe), theme toggle.
- [ ] W3 Verification and polish: headless Chromium screenshots at 375/768/1440 in light and dark, link/anchor check, HTML validity check, no console errors; README/AGENTS links still valid; CHANGELOG entry.

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
- Note: this commit's script holds only the W1 behaviour; the tab list and the
  docs filter field are styled but become interactive in W2.

## Next step
W2.
