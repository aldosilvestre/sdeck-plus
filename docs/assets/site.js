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

  /* ---------- Copy buttons on code blocks ---------- */
  function fallbackCopy(text) {
    var area = doc.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    doc.body.appendChild(area);
    area.select();
    var ok = false;
    try { ok = doc.execCommand("copy"); } catch (e) { ok = false; }
    doc.body.removeChild(area);
    return ok;
  }
  function copyText(text) {
    if (navigator.clipboard && window.isSecureContext) {
      return navigator.clipboard.writeText(text).then(function () { return true; }, function () { return fallbackCopy(text); });
    }
    return Promise.resolve(fallbackCopy(text));
  }
  var COPY_ICON = '<svg aria-hidden="true"><use href="#i-copy"/></svg><span>Copy</span>';
  var COPIED_ICON = '<svg aria-hidden="true"><use href="#i-check"/></svg><span>Copied</span>';
  qsa(".code-block").forEach(function (block) {
    if (!qs("pre > code", block) || qs(".copy-btn", block)) return;
    var button = doc.createElement("button");
    button.type = "button";
    button.className = "copy-btn";
    button.setAttribute("aria-label", "Copy code to clipboard");
    button.innerHTML = COPY_ICON;
    block.classList.add("has-copy");
    block.appendChild(button);
  });
  /* Delegated, so it keeps working when docs search re-renders cards. */
  doc.addEventListener("click", function (event) {
    var button = event.target.closest(".copy-btn");
    if (!button) return;
    var code = qs("pre > code", button.parentNode);
    if (!code) return;
    copyText(code.textContent).then(function (ok) {
      if (ok) {
        button.classList.add("is-copied");
        button.innerHTML = COPIED_ICON;
        announce("Copied to clipboard");
      } else {
        var range = doc.createRange();
        range.selectNodeContents(code);
        var selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
        announce("Copy is unavailable; the code is selected, press Ctrl+C");
      }
      window.setTimeout(function () {
        button.classList.remove("is-copied");
        button.innerHTML = COPY_ICON;
      }, 2000);
    });
  });

  /* ---------- Lightbox ---------- */
  var lightbox = qs("#lightbox");
  var zoomLinks = qsa("a.zoom");
  if (lightbox && typeof lightbox.showModal === "function" && zoomLinks.length) {
    var lbImage = qs("#lb-img", lightbox);
    var lbCaption = qs("#lb-caption", lightbox);
    var lbCount = qs("#lb-count", lightbox);
    var lbIndex = 0;
    var lbTrigger = null;
    var showShot = function (index) {
      lbIndex = (index + zoomLinks.length) % zoomLinks.length;
      var link = zoomLinks[lbIndex];
      var thumb = qs("img", link);
      lbImage.src = link.getAttribute("href");
      lbImage.alt = thumb ? thumb.alt : "";
      lbCaption.textContent = link.getAttribute("data-caption") || (thumb ? thumb.alt : "");
      lbCount.textContent = (lbIndex + 1) + " / " + zoomLinks.length;
    };
    zoomLinks.forEach(function (link, index) {
      link.setAttribute("aria-haspopup", "dialog");
      link.addEventListener("click", function (event) {
        if (event.metaKey || event.ctrlKey || event.shiftKey || event.button === 1) return;
        event.preventDefault();
        lbTrigger = link;
        showShot(index);
        if (!lightbox.open) lightbox.showModal();
        var closeButton = qs('[data-lb="close"]', lightbox);
        if (closeButton) closeButton.focus();
      });
    });
    lightbox.addEventListener("click", function (event) {
      var action = event.target.closest("[data-lb]");
      if (action) {
        var kind = action.getAttribute("data-lb");
        if (kind === "close") lightbox.close();
        else showShot(lbIndex + (kind === "next" ? 1 : -1));
        return;
      }
      if (event.target === lightbox) lightbox.close();
    });
    lightbox.addEventListener("keydown", function (event) {
      if (event.key === "ArrowRight") { event.preventDefault(); showShot(lbIndex + 1); }
      else if (event.key === "ArrowLeft") { event.preventDefault(); showShot(lbIndex - 1); }
      else if (event.key === "Home") { event.preventDefault(); showShot(0); }
      else if (event.key === "End") { event.preventDefault(); showShot(zoomLinks.length - 1); }
    });
    lightbox.addEventListener("close", function () {
      lbImage.removeAttribute("src");
      /* After the browser's own focus restoration, return focus to the thumbnail. */
      if (lbTrigger) window.setTimeout(function () { lbTrigger.focus(); }, 0);
    });
  }

  /* ---------- Tabs (actions) ---------- */
  qsa('[role="tablist"]').forEach(function (tablist) {
    var tabs = qsa('[role="tab"]', tablist);
    var panels = tabs.map(function (tab) { return doc.getElementById(tab.getAttribute("aria-controls")); });
    function select(index, focus) {
      tabs.forEach(function (tab, i) {
        var selected = i === index;
        tab.setAttribute("aria-selected", selected ? "true" : "false");
        tab.tabIndex = selected ? 0 : -1;
        if (panels[i]) {
          panels[i].hidden = !selected;
          panels[i].classList.add("is-visible");
        }
      });
      if (focus) tabs[index].focus();
    }
    tabs.forEach(function (tab, index) {
      tab.addEventListener("click", function () { select(index, false); });
      tab.addEventListener("keydown", function (event) {
        var next = null;
        if (event.key === "ArrowDown" || event.key === "ArrowRight") next = (index + 1) % tabs.length;
        else if (event.key === "ArrowUp" || event.key === "ArrowLeft") next = (index - 1 + tabs.length) % tabs.length;
        else if (event.key === "Home") next = 0;
        else if (event.key === "End") next = tabs.length - 1;
        if (next !== null) { event.preventDefault(); select(next, true); }
      });
    });
    panels.forEach(function (panel) { if (panel) panel.setAttribute("tabindex", "0"); });
    select(Math.max(0, tabs.findIndex(function (t) { return t.getAttribute("aria-selected") === "true"; })), false);
  });

  /* ---------- Bento pointer glow ---------- */
  qsa(".tile").forEach(function (tile) {
    tile.addEventListener("pointermove", function (event) {
      var box = tile.getBoundingClientRect();
      tile.style.setProperty("--mx", (event.clientX - box.left) + "px");
      tile.style.setProperty("--my", (event.clientY - box.top) + "px");
    });
  });

  /* ---------- Interactive Stream Deck + ---------- */
  var device = qs("#device");
  if (device) initDevice(device);

  function initDevice(deviceEl) {
    var caption = qs("#device-caption");
    var capKind = qs(".caption-kind", caption);
    var capTitle = qs(".caption-title", caption);
    var capDesc = qs(".caption-desc", caption);
    var capList = qs(".caption-list", caption);
    var capHint = qs(".caption-hint", caption);
    var defaultHint = capHint ? capHint.textContent : "";
    var strip = qs("#strip", deviceEl);
    var zones = qsa(".zone", deviceEl);
    var pageLabel = qs('[data-live="strip-page"]', deviceEl);
    var state = {
      pinned: null,
      volume: 65, volumeDir: 1,
      brightness: 80, brightnessDir: 1,
      playing: true, track: 0, output: 0, recording: false,
      page: 0
    };
    var tracks = ["Midnight Drive · The Example Band", "Open Roads · Demo Ensemble", "Low Light · Placeholder Trio"];
    var outputs = ["Speakers", "Headset", "HDMI"];

    function famColor(fam) { return "var(--fam-" + (fam || "media") + ")"; }

    function describe(ctl, hint) {
      if (!ctl || !ctl.getAttribute("data-title")) return;
      qsa(".ctl.is-active", deviceEl).forEach(function (el) { if (el !== ctl) el.classList.remove("is-active"); });
      ctl.classList.add("is-active");
      caption.style.setProperty("--tone-text", famColor(ctl.getAttribute("data-fam")));
      capKind.textContent = ctl.getAttribute("data-kind");
      capTitle.textContent = ctl.getAttribute("data-title");
      capDesc.textContent = ctl.getAttribute("data-desc");
      capList.innerHTML = "";
      (ctl.getAttribute("data-ex") || "").split("|").forEach(function (item) {
        if (!item) return;
        var li = doc.createElement("li");
        li.textContent = item;
        capList.appendChild(li);
      });
      if (capHint) capHint.textContent = hint || defaultHint;
    }

    function press(el) {
      el.classList.add("is-pressed");
      window.setTimeout(function () { el.classList.remove("is-pressed"); }, 180);
    }

    /* Touch strip pages: page 1 is the static markup, page 2 is rendered here. */
    var page1 = zones.map(function (zone) {
      return {
        fam: zone.getAttribute("data-fam"), kind: zone.getAttribute("data-kind"), title: zone.getAttribute("data-title"),
        desc: zone.getAttribute("data-desc"), ex: zone.getAttribute("data-ex"), label: zone.getAttribute("aria-label")
      };
    });
    var page2 = [
      { fam: "obs", title: "OBS state", desc: "OBS state shows recording, streaming and the current scene.", ex: "Select a scene on Tap|Toggle streaming on Hold|Names must match OBS", icon: "i-scene", value: "Studio", cap: "OBS scene" },
      { fam: "load", title: "CPU", desc: "Bars turn amber and red under load.", ex: "Readings sit on a see-through tile beside the icon|The binding's action still runs", icon: "i-cpu", meterKey: "cpu" },
      { fam: "load", title: "Memory", desc: "Memory usage, drawn over the section's own image.", ex: "Each section shows information for its Tap or Hold binding", icon: "i-memory", value: "41%", meter: 41 },
      { fam: "system", title: "Temperature", desc: "Temperature bars turn amber and red under load.", ex: "Flick, swipes and whole-strip gestures cannot show information", icon: "i-thermo", value: "52°C", meter: 52 }
    ];
    var cpu = 23;

    function meterHTML(value, live, tone) {
      return '<span class="meter"><span' + (live ? ' data-live="' + live + '"' : "") +
        ' style="--v: ' + value + '%' + (tone ? "; --tone: " + tone : "") + '"></span></span>';
    }
    function zoneHTML(index) {
      var icon, body;
      if (state.page === 0) {
        if (index === 0) {
          icon = state.volume === 0 ? "i-mute" : "i-speaker";
          body = '<span class="zone-value" data-live="volume">' + state.volume + "%</span>" + meterHTML(state.volume, "volume-bar");
        } else if (index === 1) {
          icon = "i-cloud";
          body = '<span class="zone-value">18°</span><span class="zone-cap">Lisbon</span>';
        } else if (index === 2) {
          icon = state.playing ? "i-music" : "i-stop";
          body = '<span class="marquee"><span class="marquee-track">' + tracks[state.track] + "</span></span>" +
            '<span class="zone-cap">' + (state.playing ? "Now playing" : "Paused") + "</span>";
        } else {
          icon = "i-sun";
          body = '<span class="zone-value" data-live="brightness">' + state.brightness + "%</span>" + meterHTML(state.brightness, "brightness-bar");
        }
      } else {
        var z = page2[index];
        icon = z.icon;
        if (z.meterKey === "cpu") body = '<span class="zone-value" data-live="cpu2">' + cpu + "%</span>" + meterHTML(cpu, "cpu2-bar", loadTone(cpu));
        else if (z.meter !== undefined) body = '<span class="zone-value">' + z.value + "</span>" + meterHTML(z.meter, null);
        else body = '<span class="zone-value">' + z.value + '</span><span class="zone-cap">' + z.cap + "</span>";
      }
      return '<svg class="zone-icon" aria-hidden="true"><use href="#' + icon + '"/></svg><span class="zone-body">' + body + "</span>";
    }
    function renderZone(index) {
      var zone = zones[index];
      if (state.page === 0) {
        var p = page1[index];
        zone.setAttribute("data-fam", p.fam); zone.setAttribute("data-title", p.title);
        zone.setAttribute("data-desc", p.desc); zone.setAttribute("data-ex", p.ex);
        zone.setAttribute("aria-label", p.label);
      } else {
        var z = page2[index];
        zone.setAttribute("data-fam", z.fam); zone.setAttribute("data-title", z.title);
        zone.setAttribute("data-desc", z.desc); zone.setAttribute("data-ex", z.ex);
        zone.setAttribute("aria-label", "Touch section " + (index + 1) + ": " + z.title.toLowerCase());
      }
      zone.classList.toggle("is-paused", state.page === 0 && index === 2 && !state.playing);
      zone.innerHTML = zoneHTML(index);
      if (state.page === 0 && index === 2) startMarquee();
    }
    function setPage(page) {
      if (page < 0 || page > 1 || page === state.page) return;
      var focused = doc.activeElement;
      state.page = page;
      zones.forEach(function (_, i) { renderZone(i); });
      var prev = qs('[data-strip="prev"]', deviceEl);
      var next = qs('[data-strip="next"]', deviceEl);
      prev.hidden = page === 0;
      next.hidden = page === 1;
      if (focused === prev && prev.hidden) next.focus();
      if (focused === next && next.hidden) prev.focus();
      if (pageLabel) pageLabel.textContent = "Touch page " + (page + 1) + " / 2";
      if (!reducedMotion()) {
        strip.classList.remove("is-paging-next", "is-paging-prev");
        void strip.offsetWidth;
        strip.classList.add(page === 1 ? "is-paging-next" : "is-paging-prev");
      }
      describe(zones[0], "Touch page " + (page + 1) + " of 2. Swipe or use the edge arrows.");
      announce("Touch page " + (page + 1) + " of 2");
    }

    /* Now playing marquee: scrolls only when the title does not fit. */
    function startMarquee() {
      var marquee = qs(".marquee", zones[2]);
      if (!marquee) return;
      var track = qs(".marquee-track", marquee);
      marquee.classList.remove("is-scrolling");
      if (reducedMotion() || !state.playing) return;
      if (marquee.scrollWidth > marquee.clientWidth + 2) {
        var text = track.textContent;
        track.innerHTML = "<span>" + text + '</span><span aria-hidden="true">' + text + "</span>";
        marquee.classList.add("is-scrolling");
      }
    }
    startMarquee();

    /* Clock key: real local time. */
    var clockEl = qs('[data-live="clock"]', deviceEl);
    var dateEl = qs('[data-live="date"]', deviceEl);
    function tick() {
      var now = new Date();
      var hh = String(now.getHours()).padStart(2, "0");
      var mm = String(now.getMinutes()).padStart(2, "0");
      if (clockEl) clockEl.textContent = hh + ":" + mm;
      if (dateEl) dateEl.textContent = now.toLocaleDateString("en-US", { weekday: "short" }) + " " + now.getDate();
    }
    tick();
    window.setInterval(tick, 10000);

    /* CPU key: a demonstration random walk; static under reduced motion. */
    function loadTone(value) { return value >= 80 ? "var(--dev-red)" : value >= 60 ? "var(--dev-amber)" : "var(--dev-green)"; }
    function renderCpu() {
      var text = qs('[data-live="cpu"]', deviceEl);
      var bar = qs('[data-live="cpu-bar"]', deviceEl);
      if (text) text.textContent = cpu + "%";
      if (bar) { bar.style.setProperty("--v", cpu + "%"); bar.style.setProperty("--tone", loadTone(cpu)); }
      var text2 = qs('[data-live="cpu2"]', deviceEl);
      var bar2 = qs('[data-live="cpu2-bar"]', deviceEl);
      if (text2) text2.textContent = cpu + "%";
      if (bar2) { bar2.style.setProperty("--v", cpu + "%"); bar2.style.setProperty("--tone", loadTone(cpu)); }
    }
    if (!reducedMotion()) {
      window.setInterval(function () {
        if (doc.hidden) return;
        var step = Math.round((Math.random() - 0.45) * 22);
        cpu = Math.min(94, Math.max(6, cpu + step));
        renderCpu();
      }, 1800);
    }

    function setBrightness() {
      deviceEl.style.setProperty("--dev-bright", String(0.45 + 0.55 * state.brightness / 100));
      if (state.page === 0) renderZone(3);
    }

    /* Key behaviours that mirror the configured demo actions. */
    function keyAction(key) {
      var label = key.getAttribute("aria-label") || "";
      if (label.indexOf("Key 4") === 0) {
        state.recording = !state.recording;
        key.classList.toggle("is-recording", state.recording);
        return state.recording ? "Recording started (demonstration)." : "Recording stopped (demonstration).";
      }
      if (label.indexOf("Key 5") === 0) {
        state.playing = !state.playing;
        qs(".key-face use", key).setAttribute("href", state.playing ? "#i-play" : "#i-stop");
        qs(".key-label", key).textContent = state.playing ? "Play" : "Paused";
        if (state.page === 0) renderZone(2);
        return state.playing ? "Playing: the title scrolls again." : "Paused: the title stays still and greys out.";
      }
      if (label.indexOf("Key 6") === 0) {
        state.output = (state.output + 1) % outputs.length;
        qs('[data-live="output"]', key).textContent = outputs[state.output];
        return "Rotated to " + outputs[state.output] + ".";
      }
      return null;
    }

    function dialAction(dial) {
      var kind = dial.getAttribute("data-dial");
      var knob = qs(".knob", dial);
      var dir = 1;
      var hint = null;
      if (kind === "volume") {
        if (state.volume >= 100) state.volumeDir = -1;
        if (state.volume <= 0) state.volumeDir = 1;
        dir = state.volumeDir;
        state.volume = Math.max(0, Math.min(100, state.volume + 5 * dir));
        if (state.page === 0) renderZone(0);
        hint = (dir > 0 ? "Turned clockwise" : "Turned counterclockwise") + " · volume " + state.volume + "%";
      } else if (kind === "brightness") {
        if (state.brightness >= 100) state.brightnessDir = -1;
        if (state.brightness <= 10) state.brightnessDir = 1;
        dir = state.brightnessDir;
        state.brightness = Math.max(10, Math.min(100, state.brightness + 10 * dir));
        setBrightness();
        hint = (dir > 0 ? "Turned clockwise" : "Turned counterclockwise") + " · brightness " + state.brightness + "%";
      } else if (kind === "track") {
        state.track = (state.track + 1) % tracks.length;
        if (state.page === 0) renderZone(2);
        hint = "Turned clockwise · next track";
      } else {
        hint = "Turned clockwise · scrolled down";
      }
      var rot = (parseFloat(knob.style.getPropertyValue("--rot")) || 0) + 30 * dir;
      knob.style.setProperty("--rot", rot + "deg");
      dial.classList.add("is-turning");
      window.setTimeout(function () { dial.classList.remove("is-turning"); }, 320);
      return hint;
    }

    var swiped = false;
    deviceEl.addEventListener("click", function (event) {
      var ctl = event.target.closest(".ctl");
      if (!ctl || !deviceEl.contains(ctl)) return;
      var stripNav = ctl.getAttribute("data-strip");
      if (stripNav) { setPage(state.page + (stripNav === "next" ? 1 : -1)); return; }
      if (swiped) { swiped = false; return; }
      var hint = null;
      if (ctl.classList.contains("dial")) hint = dialAction(ctl);
      else if (ctl.classList.contains("key")) { press(ctl); hint = keyAction(ctl); }
      else if (ctl.classList.contains("zone")) press(ctl);
      state.pinned = ctl;
      describe(ctl, hint);
    });
    deviceEl.addEventListener("mouseover", function (event) {
      var ctl = event.target.closest(".ctl");
      if (ctl && ctl.getAttribute("data-title")) describe(ctl);
    });
    deviceEl.addEventListener("focusin", function (event) {
      var ctl = event.target.closest(".ctl");
      if (ctl && ctl.getAttribute("data-title")) describe(ctl);
    });
    deviceEl.addEventListener("mouseleave", function () {
      if (state.pinned && deviceEl.contains(state.pinned)) describe(state.pinned);
    });

    /* Arrow keys move between the controls in reading order. */
    deviceEl.addEventListener("keydown", function (event) {
      var keys = ["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp", "Home", "End"];
      if (keys.indexOf(event.key) < 0) return;
      var controls = qsa(".ctl", deviceEl).filter(function (el) { return !el.hidden; });
      var index = controls.indexOf(doc.activeElement);
      if (index < 0) return;
      event.preventDefault();
      if (event.key === "Home") index = 0;
      else if (event.key === "End") index = controls.length - 1;
      else if (event.key === "ArrowRight" || event.key === "ArrowDown") index = (index + 1) % controls.length;
      else index = (index - 1 + controls.length) % controls.length;
      controls[index].focus();
    });

    /* Swipe on the strip: left shows the next touch page, right the previous. */
    var startX = null;
    strip.addEventListener("pointerdown", function (event) { startX = event.clientX; });
    strip.addEventListener("pointerup", function (event) {
      if (startX === null) return;
      var dx = event.clientX - startX;
      startX = null;
      if (Math.abs(dx) > 36) {
        swiped = true;
        window.setTimeout(function () { swiped = false; }, 50);
        setPage(state.page + (dx < 0 ? 1 : -1));
      }
    });
    strip.addEventListener("pointercancel", function () { startX = null; });

    setBrightness();
    renderCpu();
  }

  /* ---------- Docs: filter, scroll-spy, collapsible TOC ---------- */
  var searchInput = qs("#doc-search");
  var toc = qs("#toc");
  if (toc) initToc(toc);
  if (searchInput) initSearch(searchInput);

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

  function initSearch(input) {
    var content = qs("#doc-content");
    var status = qs("#search-status");
    var empty = qs("#no-results");
    var cards = qsa(".card", content).map(function (card) {
      var group = card.closest(".doc-group");
      var section = card.closest(".doc-section");
      var heading = [group && qs("h3", group), section && qs("h2", section)]
        .filter(Boolean).map(function (h) { return h.textContent; }).join(" ");
      return { el: card, html: card.innerHTML, text: (card.textContent + " " + heading).toLowerCase().replace(/\s+/g, " ") };
    });
    var tocLinks = toc ? qsa("a[href^='#']", toc) : [];
    var timer = null;

    function escapeRegExp(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"); }
    function highlight(el, words) {
      if (!words.length) return;
      var pattern = new RegExp("(" + words.map(escapeRegExp).join("|") + ")", "gi");
      var walker = doc.createTreeWalker(el, NodeFilter.SHOW_TEXT, {
        acceptNode: function (node) {
          if (!node.nodeValue.trim() || node.parentNode.closest("button, pre")) return NodeFilter.FILTER_REJECT;
          return NodeFilter.FILTER_ACCEPT;
        }
      });
      var nodes = [];
      while (walker.nextNode()) nodes.push(walker.currentNode);
      nodes.forEach(function (node) {
        pattern.lastIndex = 0;
        if (!pattern.test(node.nodeValue)) return;
        var span = doc.createElement("span");
        span.innerHTML = node.nodeValue.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(pattern, "<mark>$1</mark>");
        node.parentNode.replaceChild(span, node);
      });
    }

    function apply() {
      var query = input.value.trim().toLowerCase();
      var words = query ? query.split(/\s+/) : [];
      var shown = 0;
      cards.forEach(function (card) {
        var match = words.every(function (w) { return card.text.indexOf(w) >= 0; });
        card.el.innerHTML = card.html;
        card.el.hidden = !match;
        if (match) {
          shown += 1;
          if (query.length >= 2) highlight(card.el, words);
        }
      });
      qsa(".doc-group", content).forEach(function (group) {
        group.hidden = !qsa(".card", group).some(function (c) { return !c.hidden; });
      });
      qsa(".doc-section", content).forEach(function (section) {
        section.hidden = !qsa(".card", section).some(function (c) { return !c.hidden; });
      });
      tocLinks.forEach(function (link) {
        var target = doc.getElementById(link.getAttribute("href").slice(1));
        var hidden = !target || target.hidden || !!target.closest("[hidden]");
        link.classList.toggle("is-filtered", hidden);
      });
      if (empty) empty.hidden = shown !== 0;
      if (status) {
        status.textContent = query
          ? (shown === 0 ? "No sections match “" + input.value.trim() + "”." : shown + " of " + cards.length + " sections match “" + input.value.trim() + "”.")
          : "";
      }
      window.dispatchEvent(new Event("scroll"));
    }

    input.addEventListener("input", function () {
      window.clearTimeout(timer);
      timer = window.setTimeout(apply, 90);
    });
    input.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && input.value) { event.preventDefault(); input.value = ""; apply(); }
      if (event.key === "Enter") {
        event.preventDefault();
        var first = cards.filter(function (c) { return !c.el.hidden; })[0];
        if (first) {
          first.el.setAttribute("tabindex", "-1");
          first.el.scrollIntoView({ behavior: reducedMotion() ? "auto" : "smooth", block: "start" });
          first.el.focus({ preventScroll: true });
        }
      }
    });
    doc.addEventListener("keydown", function (event) {
      var tag = (event.target.tagName || "").toLowerCase();
      if (event.key === "/" && tag !== "input" && tag !== "textarea" && !event.target.isContentEditable) {
        event.preventDefault();
        input.focus();
      }
    });
    var initial = /[?&]q=([^&#]*)/.exec(location.search);
    if (initial) {
      try { input.value = decodeURIComponent(initial[1].replace(/\+/g, " ")); } catch (e) { input.value = ""; }
      apply();
    }
  }
})();
