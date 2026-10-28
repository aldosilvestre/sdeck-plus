/* sdeck-plus website behaviour. Progressive enhancement only: every page is
   complete without this file. No dependencies, no tracking, no network. */
(function () {
  "use strict";

  var doc = document;
  var root = doc.documentElement;
  var motionQuery = window.matchMedia ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  var darkQuery = window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)") : null;

  function reducedMotion() { return !!(motionQuery && motionQuery.matches); }
  function qs(selector, context) { return (context || doc).querySelector(selector); }
  function qsa(selector, context) { return Array.prototype.slice.call((context || doc).querySelectorAll(selector)); }

  var liveRegion = qs("#live-status");
  function announce(message) {
    if (!liveRegion) return;
    liveRegion.textContent = "";
    window.setTimeout(function () { liveRegion.textContent = message; }, 40);
  }

  /* ---------- Theme ---------- */
  var THEME_KEY = "sdeck-plus-theme";
  function effectiveTheme() {
    var forced = root.getAttribute("data-theme");
    if (forced === "light" || forced === "dark") return forced;
    return darkQuery && darkQuery.matches ? "dark" : "light";
  }
  function syncThemeButtons() {
    var next = effectiveTheme() === "dark" ? "light" : "dark";
    qsa(".theme-toggle").forEach(function (button) {
      var label = "Switch to " + next + " theme";
      button.setAttribute("aria-label", label);
      button.setAttribute("title", label);
    });
  }
  qsa(".theme-toggle").forEach(function (button) {
    button.addEventListener("click", function () {
      var next = effectiveTheme() === "dark" ? "light" : "dark";
      root.setAttribute("data-theme", next);
      try { window.localStorage.setItem(THEME_KEY, next); } catch (e) { /* not persisted */ }
      syncThemeButtons();
      announce((next === "dark" ? "Dark" : "Light") + " theme");
    });
  });
  if (darkQuery) {
    var onSchemeChange = function () { syncThemeButtons(); };
    if (darkQuery.addEventListener) darkQuery.addEventListener("change", onSchemeChange);
    else if (darkQuery.addListener) darkQuery.addListener(onSchemeChange);
  }
  syncThemeButtons();

  /* ---------- Mobile navigation ---------- */
  var menuButton = qs(".menu-toggle");
  var siteNav = qs("#site-nav");
  function setMenu(open) {
    if (!menuButton || !siteNav) return;
    menuButton.setAttribute("aria-expanded", open ? "true" : "false");
    menuButton.setAttribute("aria-label", open ? "Close menu" : "Open menu");
    siteNav.classList.toggle("is-open", open);
  }
  if (menuButton && siteNav) {
    menuButton.addEventListener("click", function () {
      setMenu(menuButton.getAttribute("aria-expanded") !== "true");
    });
    siteNav.addEventListener("click", function (event) {
      if (event.target.closest("a")) setMenu(false);
    });
    doc.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && menuButton.getAttribute("aria-expanded") === "true") {
        setMenu(false);
        menuButton.focus();
      }
    });
  }

  /* ---------- Scroll reveal ---------- */
  var revealItems = qsa(".reveal");
  if (!reducedMotion() && "IntersectionObserver" in window) {
    var revealObserver = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-visible");
          revealObserver.unobserve(entry.target);
        }
      });
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.08 });
    revealItems.forEach(function (item) { revealObserver.observe(item); });
  } else {
    revealItems.forEach(function (item) { item.classList.add("is-visible"); });
  }

  /* ---------- Docs: scroll-spy, collapsible TOC ---------- */
  var toc = qs("#toc");
  if (toc) initToc(toc);

  function initToc(tocEl) {
    var narrow = window.matchMedia ? window.matchMedia("(max-width: 960px)") : null;
    if (narrow && narrow.matches) tocEl.open = false;
    tocEl.addEventListener("click", function (event) {
      if (event.target.closest("a") && narrow && narrow.matches) tocEl.open = false;
    });
    var links = qsa("a[href^='#']", tocEl);
    var targets = links.map(function (link) { return doc.getElementById(link.getAttribute("href").slice(1)); });
    var ticking = false;
    function update() {
      ticking = false;
      var line = (parseFloat(getComputedStyle(root).getPropertyValue("--header-h")) || 64) + 120;
      var currentSub = -1;
      var currentTop = -1;
      targets.forEach(function (target, i) {
        if (!target || target.hidden || target.offsetParent === null) return;
        if (target.getBoundingClientRect().top <= line) {
          if (links[i].parentNode.parentNode === tocEl.querySelector("ol")) currentTop = i;
          else currentSub = i;
        }
      });
      if (currentSub >= 0 && currentTop >= 0 && !targets[currentTop].contains(targets[currentSub])) currentSub = -1;
      if (currentTop < 0) {
        currentTop = targets.findIndex(function (t, i) { return t && !t.hidden && links[i].parentNode.parentNode === tocEl.querySelector("ol"); });
      }
      links.forEach(function (link, i) {
        var current = i === currentSub || i === currentTop;
        link.classList.toggle("is-current", current);
        if (i === currentSub || (currentSub < 0 && i === currentTop)) link.setAttribute("aria-current", "location");
        else link.removeAttribute("aria-current");
      });
      var toTop = qs(".to-top");
      if (toTop) toTop.classList.toggle("is-visible", window.scrollY > 900);
    }
    window.addEventListener("scroll", function () {
      if (!ticking) { ticking = true; window.requestAnimationFrame(update); }
    }, { passive: true });
    window.addEventListener("resize", update);
    update();
  }
})();
