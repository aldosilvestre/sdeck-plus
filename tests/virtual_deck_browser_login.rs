//! The real login and deck pages in a real browser against the real server
//! (S4, S5).
//!
//! Each test starts a `VirtualDeckServer` on 127.0.0.1 and drives the pages
//! it serves in headless Chromium (the DevTools protocol over
//! `--remote-debugging-pipe`, from a Node.js script) as a phone would: an
//! Android Chrome user agent, a 844x390 touch screen held sideways, taps and
//! keyboard (IME) text. The page is opened as `http://deck.test:<port>/`,
//! which Chromium maps to 127.0.0.1 (`--host-resolver-rules`, no DNS query;
//! `.test` never resolves on the internet): like a phone's
//! `http://192.168.x.y:8765/`, and unlike `127.0.0.1`, that address is not a
//! secure context. Every request, response, cookie decision and WebSocket
//! event is recorded, so a failure prints what the browser saw.
//!
//! Skipped with a note when Chromium or Node.js is missing (`SDECK_CHROMIUM`
//! names another Chromium). No display is used: DISPLAY and WAYLAND_DISPLAY
//! are removed from Chromium's environment, and its profile lives in a
//! fresh directory under the system temp directory, removed afterwards.
//! Set `SDECK_BROWSER_REPORT=1` to print the whole record.
use sdeck_plus::virtual_deck::{
    build_verifier, DeckFrame, DeckInput, ServerConfig, VirtualDeckServer,
};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::Write,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const PASSWORD: &str = "correct horse";
const WRONG: &str = "wrong password";
/// The host name the page is opened at; Chromium maps it to 127.0.0.1.
const HOST: &str = "deck.test";
/// The session cookie the server sets.
const SESSION_COOKIE: &str = "sdeck_http_session";
/// The cookie an earlier development build, served over HTTPS, set: `Secure`.
const HTTPS_BUILD_COOKIE: &str = "sdeck_session";
/// Upper bound of every wait on a real signal; never a fixed sleep.
const WAIT: Duration = Duration::from_secs(20);

/// The first of `SDECK_CHROMIUM`, then chromium, chromium-browser,
/// google-chrome-stable and google-chrome on PATH.
fn chromium() -> Option<PathBuf> {
    if let Some(path) = env::var_os("SDECK_CHROMIUM") {
        return Some(PathBuf::from(path));
    }
    let names = [
        "chromium",
        "chromium-browser",
        "google-chrome-stable",
        "google-chrome",
    ];
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// A fresh browser profile in the system temp directory, removed on drop.
/// Each browser needs its own: one profile refuses a second Chromium.
struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path = env::temp_dir().join(format!(
            "sdeck-browser-login-{}-{}-{nanos}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Not create_dir_all: an existing directory is never someone else's.
        fs::create_dir(&path).expect("a new browser profile directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Drives the pages through the DevTools protocol (Chromium reads fd 3 and
/// writes fd 4, one NUL-terminated JSON message each) and prints one JSON
/// report: `steps`, `requests` (one entry per request or WebSocket, with
/// the headers actually sent and received and the cookies the browser
/// excluded or refused), `console`, `outcome` and `error`. `CONFIG` comes
/// first: url, map, chrome, profile, password, optional wrong, stale_cookie
/// and press_key.
const DRIVER: &str = r##"
"use strict";
const { spawn } = require("node:child_process");

const STEP_MS = 20000;
const ANDROID_CHROME = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 " +
  "(KHTML, like Gecko) Chrome/131.0.6778.135 Mobile Safari/537.36";

const report = { steps: [], requests: [], console: [], outcome: null, error: null };
const env = Object.assign({}, process.env);
delete env.DISPLAY;
delete env.WAYLAND_DISPLAY;
const chrome = spawn(CONFIG.chrome, [
  "--headless=new", "--remote-debugging-pipe", "--user-data-dir=" + CONFIG.profile,
  "--no-first-run", "--no-default-browser-check", "--disable-gpu", "--disable-extensions",
  "--disable-background-networking", "--disable-component-update", "--disable-sync",
  "--disable-default-apps", "--mute-audio", "--password-store=basic", "--no-proxy-server",
  "--host-resolver-rules=MAP " + CONFIG.map + " 127.0.0.1", "about:blank"
], { env, stdio: ["ignore", "ignore", "pipe", "pipe", "pipe"] });
let chromeErr = "";
chrome.stderr.on("data", (data) => {
  if (chromeErr.length < 4000) {
    chromeErr += data;
  }
});
const calls = new Map();
const exited = new Promise((resolve) => {
  const end = (why) => {
    for (const waiting of calls.values()) {
      waiting.reject(new Error("Chromium ended (" + why + "): " + chromeErr));
    }
    calls.clear();
    resolve(why);
  };
  chrome.on("exit", (code) => end("exit " + code));
  chrome.on("error", (error) => end(String(error)));
});
const toChrome = chrome.stdio[3];
const fromChrome = chrome.stdio[4];
toChrome.on("error", () => {});
fromChrome.on("error", () => {});

let nextId = 0;
let session = null;
const listeners = new Set();
let unread = Buffer.alloc(0);
fromChrome.on("data", (chunk) => {
  unread = Buffer.concat([unread, chunk]);
  let end;
  while ((end = unread.indexOf(0)) >= 0) {
    const message = JSON.parse(unread.subarray(0, end).toString("utf8"));
    unread = unread.subarray(end + 1);
    if (message.id !== undefined) {
      const waiting = calls.get(message.id);
      calls.delete(message.id);
      if (waiting && message.error) {
        waiting.reject(new Error(waiting.method + ": " + message.error.message));
      } else if (waiting) {
        waiting.resolve(message.result);
      }
    } else {
      record(message);
      for (const listener of Array.from(listeners)) {
        listener(message);
      }
    }
  }
});

function send(method, params, sessionId) {
  const id = ++nextId;
  const message = { id, method, params: params || {} };
  if (sessionId) {
    message.sessionId = sessionId;
  }
  return new Promise((resolve, reject) => {
    calls.set(id, { method, resolve, reject });
    toChrome.write(JSON.stringify(message) + "\0");
  });
}

function page(method, params) {
  return send(method, params, session);
}

// The next page event that passes `test`, within STEP_MS.
function waitFor(label, test) {
  return new Promise((resolve, reject) => {
    const listener = (message) => {
      if (message.sessionId === session && test(message)) {
        done();
        resolve(message);
      }
    };
    const timer = setTimeout(() => {
      done();
      reject(new Error("timed out: " + label));
    }, STEP_MS);
    function done() {
      clearTimeout(timer);
      listeners.delete(listener);
    }
    listeners.add(listener);
  });
}

const byId = new Map();
function entry(id) {
  let item = byId.get(id);
  if (!item) {
    item = { id };
    byId.set(id, item);
    report.requests.push(item);
  }
  return item;
}

function lower(headers) {
  const out = {};
  for (const name of Object.keys(headers || {})) {
    out[name.toLowerCase()] = headers[name];
  }
  return out;
}

function record(message) {
  const p = message.params || {};
  switch (message.method) {
    case "Network.requestWillBeSent":
      if (!p.request.url.startsWith("data:")) {
        const item = entry(p.requestId);
        item.method = p.request.method;
        item.url = p.request.url;
        item.body = p.request.postData === undefined ? null : p.request.postData;
      }
      break;
    case "Network.requestWillBeSentExtraInfo": {
      const item = entry(p.requestId);
      item.sent = lower(p.headers);
      item.excluded = (p.associatedCookies || []).filter((c) => c.blockedReasons.length)
        .map((c) => c.cookie.name + ": " + c.blockedReasons.join(","));
      break;
    }
    case "Network.responseReceived":
      entry(p.requestId).status = p.response.status;
      break;
    case "Network.responseReceivedExtraInfo": {
      const item = entry(p.requestId);
      item.status = p.statusCode;
      item.received = lower(p.headers);
      item.refused = (p.blockedCookies || []).map((c) => c.blockedReasons.join(","));
      break;
    }
    case "Network.loadingFailed":
      entry(p.requestId).failed = p.errorText;
      break;
    case "Network.webSocketCreated": {
      const item = entry(p.requestId);
      item.method = "WEBSOCKET";
      item.url = p.url;
      item.frames = [];
      break;
    }
    case "Network.webSocketWillSendHandshakeRequest":
      entry(p.requestId).sent = lower(p.request.headers);
      break;
    case "Network.webSocketHandshakeResponseReceived": {
      const item = entry(p.requestId);
      item.status = p.response.status;
      item.received = lower(p.response.headers);
      break;
    }
    case "Network.webSocketFrameReceived": {
      let type;
      try {
        type = JSON.parse(p.response.payloadData).type;
      } catch (error) {
        type = "opcode " + p.response.opcode;
      }
      entry(p.requestId).frames.push(type);
      break;
    }
    case "Network.webSocketFrameError":
      entry(p.requestId).failed = p.errorMessage;
      break;
    case "Network.webSocketClosed":
      entry(p.requestId).closed = true;
      break;
    case "Runtime.consoleAPICalled":
      report.console.push({ type: p.type, text: p.args.map((a) => a.value !== undefined ? String(a.value) : (a.description || a.type)).join(" ") });
      break;
    case "Runtime.exceptionThrown":
      report.console.push({ type: "exception", text: (p.exceptionDetails.exception && p.exceptionDetails.exception.description) || p.exceptionDetails.text });
      break;
    case "Log.entryAdded":
      report.console.push({ type: p.entry.source + ":" + p.entry.level, text: p.entry.text });
      break;
    case "Page.frameNavigated":
      if (!p.frame.parentId) {
        report.steps.push({ step: "navigated", url: p.frame.url });
      }
      break;
  }
}

async function evaluate(expression) {
  const result = await page("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) {
    const details = result.exceptionDetails;
    throw new Error("page: " + (details.exception ? details.exception.description : details.text));
  }
  return result.result.value;
}

// The truthy value of `condition` once the page shows it, within STEP_MS;
// checked again on every DOM change, never on a timer.
function until(condition) {
  return evaluate(`new Promise((resolve, reject) => {
    const check = () => { try { return (${condition}); } catch (error) { return null; } };
    const first = check();
    if (first) { resolve(first); return; }
    const observer = new MutationObserver(() => {
      const value = check();
      if (value) { observer.disconnect(); clearTimeout(timer); resolve(value); }
    });
    const timer = setTimeout(() => {
      observer.disconnect();
      reject(new Error("timed out: " + ${JSON.stringify(condition)}));
    }, ${STEP_MS});
    observer.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  })`);
}

function state() {
  return evaluate(`(() => {
    const meta = document.querySelector("meta[name='sdeck-page']");
    const banner = document.getElementById("error");
    const dot = document.getElementById("dot");
    return {
      url: location.href,
      page: meta ? meta.content : null,
      banner: banner ? banner.textContent : null,
      body: document.body ? document.body.className : null,
      dot: dot ? dot.getAttribute("aria-label") : null,
      secure_context: window.isSecureContext
    };
  })()`).catch((error) => ({ error: String(error) }));
}

async function tap(selector) {
  const box = await evaluate(`(() => {
    const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`);
  await page("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: box.x, y: box.y }] });
  await page("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
}

async function type(text) {
  await tap("#password");
  await page("Input.insertText", { text });
  const typed = await evaluate(`document.getElementById("password").value === ${JSON.stringify(text)}`);
  if (!typed) {
    throw new Error("the password field did not take the text");
  }
}

// Taps Connect: "banner" when the page settles with a message, "navigated"
// when it loads another page, "none" when neither happens within STEP_MS.
async function submit() {
  const navigation = waitFor("a page load", (m) => m.method === "Page.loadEventFired")
    .then(() => "navigated", () => "none");
  // Settled with a message: the form was busy (checking) and is not any more.
  await evaluate("window.sdeckSawBusy = false");
  const banner = until(`(window.sdeckSawBusy = window.sdeckSawBusy ||
      document.getElementById("login").getAttribute("aria-busy") === "true") &&
    document.getElementById("login").getAttribute("aria-busy") === null &&
    document.getElementById("error").classList.contains("shown") && "banner"`)
    .then(() => "banner", () => navigation);
  const started = Date.now();
  await tap("#submit");
  const result = await Promise.race([navigation, banner]);
  return { result, ms: Date.now() - started };
}

async function main() {
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  session = (await send("Target.attachToTarget", { targetId, flatten: true })).sessionId;
  await page("Network.enable");
  await page("Page.enable");
  await page("Runtime.enable");
  await page("Log.enable");
  await page("Emulation.setUserAgentOverride", { userAgent: ANDROID_CHROME, platform: "Linux armv8l" });
  await page("Emulation.setDeviceMetricsOverride", { width: 844, height: 390, deviceScaleFactor: 3,
    mobile: true, screenOrientation: { type: "landscapePrimary", angle: 90 } });
  await page("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
  if (CONFIG.stale_cookie) {
    const planted = await page("Network.setCookie", CONFIG.stale_cookie);
    report.steps.push({ step: "planted", ok: planted.success });
  }

  const loaded = waitFor("the first page load", (m) => m.method === "Page.loadEventFired");
  await page("Page.navigate", { url: CONFIG.url });
  await loaded;
  await until(`!document.getElementById("login").hidden`);
  report.steps.push({ step: "opened", state: await state() });

  if (CONFIG.wrong) {
    await type(CONFIG.wrong);
    const wrong = await submit();
    report.steps.push({ step: "wrong", result: wrong.result, ms: wrong.ms, state: await state() });
  }

  await type(CONFIG.password);
  const right = await submit();
  report.steps.push({ step: "right", result: right.result, ms: right.ms, state: await state() });
  if (right.result !== "navigated") {
    report.outcome = "no navigation: " + right.result;
    return;
  }
  report.outcome = await until(`(document.querySelector("meta[name='sdeck-page']").content === "login" && "login") ||
    (document.body.classList.contains("connected") && "deck")`).catch((error) => "stuck: " + error.message);
  report.steps.push({ step: "end", state: await state() });
  if (report.outcome === "deck" && CONFIG.press_key) {
    await tap(".key:nth-child(" + CONFIG.press_key + ")");
    report.steps.push({ step: "pressed", key: CONFIG.press_key });
  }
}

main().catch((error) => {
  report.error = String((error && error.stack) || error);
}).then(async () => {
  const timeout = (ms, value) => new Promise((resolve) => setTimeout(() => resolve(value), ms));
  await Promise.race([send("Browser.close").catch(() => {}), timeout(5000)]);
  if ((await Promise.race([exited, timeout(5000, "running")])) === "running") {
    chrome.kill("SIGKILL");
  }
  if (report.error) {
    report.chrome_stderr = chromeErr;
  }
  process.stdout.write(JSON.stringify(report));
  process.exit(0);
});
"##;

/// A server on 127.0.0.1 whose snapshot is one brightness frame.
struct Deck {
    server: Option<VirtualDeckServer>,
    inputs: mpsc::Receiver<DeckInput>,
    port: u16,
}

impl Deck {
    fn start() -> Self {
        let (sender, inputs) = mpsc::channel();
        let server = VirtualDeckServer::start(
            ServerConfig {
                port: 0,
                bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            },
            build_verifier(PASSWORD).expect("a verifier of the test password"),
            sender,
            Box::new(|| vec![DeckFrame::Brightness(80)]),
        )
        .expect("start the virtual deck server");
        let port = server.local_port();
        Self {
            server: Some(server),
            inputs,
            port,
        }
    }

    fn origin(&self) -> String {
        format!("http://{HOST}:{}", self.port)
    }

    /// Opens the page in the browser and logs in as `config` says; `None`
    /// (with a note) when Chromium or Node.js is missing.
    fn browse(&self, config: Value) -> Option<Report> {
        let Some(chrome) = chromium() else {
            eprintln!("Chromium is not installed: the pages were not run in a browser.");
            return None;
        };
        if !node_available() {
            eprintln!("node is not installed: the pages were not run in a browser.");
            return None;
        }
        let profile = Profile::new();
        let mut config = config;
        config["url"] = json!(format!("{}/", self.origin()));
        config["map"] = json!(HOST);
        config["password"] = json!(PASSWORD);
        config["chrome"] = json!(chrome);
        config["profile"] = json!(profile.path());
        let script = format!("const CONFIG = {config};\n{DRIVER}");
        let mut child = Command::new("node")
            .arg("-")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start node");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "the browser driver failed: {}\n{stdout}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = Report(
            serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{error}: {stdout}")),
        );
        if env::var_os("SDECK_BROWSER_REPORT").is_some() {
            eprintln!("{}", serde_json::to_string_pretty(&report.0).unwrap());
        }
        assert_eq!(report.0["error"], Value::Null, "{report}");
        Some(report)
    }

    fn next_input(&self) -> DeckInput {
        self.inputs
            .recv_timeout(WAIT)
            .expect("a key input from the page")
    }
}

impl Drop for Deck {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.stop();
        }
    }
}

/// What the browser did and saw.
struct Report(Value);

impl Report {
    fn step(&self, name: &str) -> &Value {
        self.0["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|step| step["step"] == name)
            .unwrap_or_else(|| panic!("no step {name}: {self}"))
    }

    fn outcome(&self) -> &str {
        self.0["outcome"].as_str().unwrap_or_default()
    }

    /// The requests (or WebSockets) `method` sent to `url`, in order.
    fn requests(&self, method: &str, url: &str) -> Vec<&Value> {
        self.0["requests"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|request| request["method"] == method && request["url"] == url)
            .collect()
    }

    fn console_exceptions(&self) -> Vec<&Value> {
        self.0["console"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["type"] == "exception")
            .collect()
    }
}

/// A short account for failure messages: steps, requests, console.
impl std::fmt::Display for Report {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(formatter, "outcome: {}", self.0["outcome"])?;
        writeln!(formatter, "error: {}", self.0["error"])?;
        for step in self.0["steps"].as_array().into_iter().flatten() {
            writeln!(formatter, "step: {step}")?;
        }
        for request in self.0["requests"].as_array().into_iter().flatten() {
            let header = |side: &str, name: &str| request[side][name].clone();
            writeln!(
                formatter,
                "{} {} -> {} cookie={} origin={} set-cookie={} excluded={} refused={} failed={}",
                request["method"],
                request["url"],
                request["status"],
                header("sent", "cookie"),
                header("sent", "origin"),
                header("received", "set-cookie"),
                request["excluded"],
                request["refused"],
                request["failed"],
            )?;
        }
        for entry in self.0["console"].as_array().into_iter().flatten() {
            writeln!(formatter, "console: {entry}")?;
        }
        Ok(())
    }
}

/// `name=value` of the session cookie in a Set-Cookie header.
fn cookie_pair(set_cookie: &str) -> &str {
    set_cookie.split(';').next().unwrap().trim()
}

/// A `Secure` cookie for this host, as a page served over HTTPS sets it.
fn secure_cookie(deck: &Deck, name: &str) -> Value {
    json!({
        "name": name,
        "value": "5e".repeat(32),
        "url": format!("https://{HOST}:{}/", deck.port),
        "path": "/",
        "secure": true,
        "httpOnly": true,
        "sameSite": "Strict",
    })
}

#[test]
fn a_phone_with_the_right_password_reaches_the_deck_and_a_wrong_one_sees_the_error() {
    let mut deck = Deck::start();
    let Some(report) = deck.browse(json!({"wrong": WRONG, "press_key": 3})) else {
        return;
    };
    let origin = deck.origin();
    let home = format!("{origin}/");

    let opened = &report.step("opened")["state"];
    assert_eq!(opened["page"], "login", "{report}");
    assert_eq!(
        opened["secure_context"], false,
        "like a phone at a LAN address: {report}"
    );

    // Wrong: the banner says so, the page stays.
    let wrong = report.step("wrong");
    assert_eq!(wrong["result"], "banner", "{report}");
    assert_eq!(
        wrong["state"]["banner"], "Wrong password. 4 tries left before a 30-second wait.",
        "{report}"
    );
    assert_eq!(wrong["state"]["page"], "login", "{report}");

    // Right: the login sets the cookie, the browser keeps it and sends it home.
    assert_eq!(report.step("right")["result"], "navigated", "{report}");
    assert_eq!(report.outcome(), "deck", "{report}");
    let end = &report.step("end")["state"];
    assert_eq!(end["page"], "deck", "{report}");
    assert_eq!(end["dot"], "Connected", "{report}");
    let finishes = report.requests("POST", &format!("{origin}/login/finish"));
    assert_eq!(finishes.len(), 2, "{report}");
    assert_eq!(finishes[0]["status"], 401, "{report}");
    assert_eq!(finishes[1]["status"], 200, "{report}");
    assert_eq!(finishes[1]["refused"], json!([]), "{report}");
    for request in report.requests("POST", &format!("{origin}/login/start")) {
        assert_eq!(request["sent"]["origin"], origin.as_str(), "{report}");
    }
    let set_cookie = finishes[1]["received"]["set-cookie"].as_str().unwrap();
    assert_eq!(
        set_cookie.split_once('=').unwrap().0,
        SESSION_COOKIE,
        "{report}"
    );
    let pair = cookie_pair(set_cookie);
    let homes = report.requests("GET", &home);
    assert_eq!(
        homes.len(),
        3,
        "the login page, the check that the browser kept the session, the deck: {report}"
    );
    assert_eq!(homes[0]["sent"]["cookie"], Value::Null, "{report}");
    for home in &homes[1..] {
        assert_eq!(home["sent"]["cookie"], pair, "{report}");
        assert_eq!(home["excluded"], json!([]), "{report}");
    }

    // The deck's WebSocket: the cookie, the page's own origin, 101, frames.
    let sockets = report.requests("WEBSOCKET", &format!("ws://{HOST}:{}/ws", deck.port));
    assert_eq!(sockets.len(), 1, "{report}");
    assert_eq!(sockets[0]["sent"]["cookie"], pair, "{report}");
    assert_eq!(sockets[0]["sent"]["origin"], origin.as_str(), "{report}");
    assert_eq!(sockets[0]["status"], 101, "{report}");
    assert_eq!(sockets[0]["frames"], json!(["brightness"]), "{report}");

    // A tap on key 3 reaches the deck through that live connection.
    assert_eq!(
        deck.next_input(),
        DeckInput {
            key: 3,
            pressed: true
        }
    );
    assert_eq!(
        deck.next_input(),
        DeckInput {
            key: 3,
            pressed: false
        }
    );

    // The password never left the page.
    let sent = serde_json::to_string(&report.0["requests"]).unwrap();
    assert!(!sent.contains(PASSWORD) && !sent.contains(WRONG), "{sent}");
    assert_eq!(
        report.console_exceptions(),
        Vec::<&Value>::new(),
        "{report}"
    );
    deck.server.take().unwrap().stop();
}

/// The phone and the PC had opened an earlier build served over HTTPS, which
/// left a `Secure` cookie named `sdeck_session`. A browser never lets a plain
/// HTTP page replace a `Secure` cookie of the same name, nor sends it over
/// HTTP, so a right password used to land on the login page again.
#[test]
fn a_secure_cookie_left_by_the_https_build_does_not_send_the_login_back() {
    let deck = Deck::start();
    let stale = secure_cookie(&deck, HTTPS_BUILD_COOKIE);
    let Some(report) = deck.browse(json!({"stale_cookie": stale})) else {
        return;
    };
    assert_eq!(report.step("planted")["ok"], true, "{report}");
    assert_eq!(report.step("right")["result"], "navigated", "{report}");
    assert_eq!(report.outcome(), "deck", "{report}");
    let finish = report.requests("POST", &format!("{}/login/finish", deck.origin()));
    assert_eq!(finish.len(), 1, "{report}");
    assert_eq!(finish[0]["status"], 200, "{report}");
    assert_eq!(finish[0]["refused"], json!([]), "{report}");
    let sockets = report.requests("WEBSOCKET", &format!("ws://{HOST}:{}/ws", deck.port));
    assert_eq!(sockets.len(), 1, "{report}");
    assert_eq!(sockets[0]["status"], 101, "{report}");
}

/// Whatever makes a browser refuse the session cookie (here, a `Secure`
/// cookie of the same name; a "block all cookies" setting does the same),
/// the login page says so instead of asking for the password again.
#[test]
fn a_browser_that_refuses_the_session_cookie_is_told_so_instead_of_asked_again() {
    let deck = Deck::start();
    let blocking = secure_cookie(&deck, SESSION_COOKIE);
    let Some(report) = deck.browse(json!({"stale_cookie": blocking})) else {
        return;
    };
    let right = report.step("right");
    assert_eq!(right["result"], "banner", "{report}");
    assert_eq!(report.outcome(), "no navigation: banner", "{report}");
    assert_eq!(right["state"]["page"], "login", "{report}");
    assert_eq!(
        right["state"]["banner"],
        "Could not log in. This browser did not keep the login cookie. \
         Allow cookies for this address, then try again.",
        "{report}"
    );
    let finish = report.requests("POST", &format!("{}/login/finish", deck.origin()));
    assert_eq!(finish.len(), 1, "{report}");
    assert_eq!(finish[0]["status"], 200, "{report}");
    assert_eq!(finish[0]["refused"], json!(["OverwriteSecure"]), "{report}");
    let homes = report.requests("GET", &format!("{}/", deck.origin()));
    assert_eq!(
        homes.len(),
        2,
        "the login page, then the check, and no move: {report}"
    );
    assert_eq!(homes[1]["sent"]["cookie"], Value::Null, "{report}");
    assert_eq!(
        report
            .0
            .get("steps")
            .and_then(Value::as_array)
            .map(|steps| steps.iter().filter(|s| s["step"] == "navigated").count()),
        Some(1),
        "only the first page load: {report}"
    );
    assert_eq!(
        report.requests("WEBSOCKET", &format!("ws://{HOST}:{}/ws", deck.port)),
        Vec::<&Value>::new(),
        "{report}"
    );
}
