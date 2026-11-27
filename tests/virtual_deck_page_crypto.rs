//! The login page's own cryptography and login flow (S4, S5): over plain
//! HTTP the page cannot use `crypto.subtle`, so `page/login.html` bundles
//! SHA-256, HMAC-SHA-256 and PBKDF2-HMAC-SHA-256 in plain JavaScript.
//!
//! The page's scripts are taken from the file the server serves and run with
//! Node.js (skipped, with a note, when `node` is not installed): against
//! published test vectors (FIPS 180-4 examples, RFC 4231, RFC 7914, RFC
//! 7677), against `ring` for other inputs, and as a whole page (with a stub
//! DOM) logging in to a real server on 127.0.0.1, which shows the password
//! never leaves the page. No browser, no display, no port beyond loopback.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ring::{digest, hmac, pbkdf2};
use sdeck_plus::virtual_deck::{build_verifier, ServerConfig, VirtualDeckServer, ITERATIONS};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, TcpStream},
    num::NonZeroU32,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

const LOGIN_PAGE: &str = include_str!("../src/virtual_deck/page/login.html");
const DECK_PAGE: &str = include_str!("../src/virtual_deck/page/index.html");
const PASSWORD: &str = "contraseña ñandú 🦀";

/// The inline scripts of `page`, in order.
fn scripts(page: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("<script>") {
        let body = &rest[start + "<script>".len()..];
        let end = body.find("</script>").expect("a closed script");
        found.push(&body[..end]);
        rest = &body[end..];
    }
    found
}

/// Runs `script` with Node.js and returns what it printed as JSON; `None`
/// when Node.js is not installed.
fn node(script: &str) -> Option<Value> {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node is not installed: the page's JavaScript was not run.");
        return None;
    }
    let mut child = Command::new("node")
        .arg("-")
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
        "node failed: {}\n{stdout}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{error}: {stdout}")))
}

/// The page's crypto script, then `driver`.
fn with_crypto(driver: &str) -> String {
    format!(
        "var window = globalThis;\n{}\n{driver}",
        scripts(LOGIN_PAGE)[0]
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, length: usize) -> Vec<u8> {
    let mut out = vec![0u8; length];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(iterations).unwrap(),
        salt,
        password,
        &mut out,
    );
    out
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data)
        .as_ref()
        .to_vec()
}

#[test]
fn the_login_page_needs_javascript_and_has_no_way_to_post_the_password() {
    assert_eq!(scripts(LOGIN_PAGE).len(), 2, "crypto, then the page");
    assert!(LOGIN_PAGE.contains(
        "<noscript><p class=\"needs\">This page needs JavaScript to log in.</p></noscript>"
    ));
    let form = &LOGIN_PAGE[LOGIN_PAGE.find("<form").unwrap()..];
    let form_tag = &form[..form.find('>').unwrap()];
    assert!(form_tag.contains(" hidden"), "{form_tag}");
    assert!(
        !form_tag.contains("action") && !form_tag.contains("method"),
        "{form_tag}"
    );
    let input = &LOGIN_PAGE[LOGIN_PAGE.find("<input id=\"password\"").unwrap()..];
    let input_tag = &input[..input.find('>').unwrap()];
    assert!(
        !input_tag.contains("name="),
        "a field without a name is never sent by a form: {input_tag}"
    );
    assert!(
        !LOGIN_PAGE.contains("/login\""),
        "no request to the old /login"
    );
    assert!(!LOGIN_PAGE.contains("subtle."), "no crypto.subtle call");
    for page in [LOGIN_PAGE, DECK_PAGE] {
        assert!(
            !page.contains("http://") && !page.contains("https://"),
            "no external address"
        );
    }
}

#[test]
fn the_page_crypto_matches_published_test_vectors() {
    let Some(results) = node(&with_crypto(
        r#"
const C = SdeckCrypto, h = C.toHex, u = C.utf8;
const bytes = (n, v) => new Uint8Array(n).fill(v);
const range = (from, to) => Uint8Array.from({ length: to - from + 1 }, (_, i) => from + i);
const salted = { salt: "W22ZaJ0SNY7soEsUEjb6gQ==", iterations: 4096, nonce: "" };
const nonce = "rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0";
const auth = "n=user,r=rOprNGfwEbeRWgbNEkqO,r=" + nonce + ",s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096,c=biws,r=" + nonce;
C.respond("pencil", salted, auth, function (answer) {
  process.stdout.write(JSON.stringify({
    sha256: [
      h(C.sha256(u(""))),
      h(C.sha256(u("abc"))),
      h(C.sha256(u("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"))),
      h(C.sha256(bytes(1000000, 0x61)))
    ],
    hmac: [
      h(C.hmac(bytes(20, 0x0b), u("Hi There"))),
      h(C.hmac(u("Jefe"), u("what do ya want for nothing?"))),
      h(C.hmac(bytes(20, 0xaa), bytes(50, 0xdd))),
      h(C.hmac(range(1, 25), bytes(50, 0xcd))),
      h(C.hmac(bytes(131, 0xaa), u("Test Using Larger Than Block-Size Key - Hash Key First")))
    ],
    pbkdf2: [
      h(C.pbkdf2(u("passwd"), u("salt"), 1, 64)),
      h(C.pbkdf2(u("Password"), u("NaCl"), 80000, 64))
    ],
    scram: answer
  }));
  process.exit(0);
});
"#,
    )) else {
        return;
    };
    // FIPS 180-4 examples ("", "abc", two blocks, a million "a").
    assert_eq!(
        results["sha256"],
        json!([
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        ])
    );
    // RFC 4231 test cases 1, 2, 3, 4 and 6.
    assert_eq!(
        results["hmac"],
        json!([
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
            "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b",
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        ])
    );
    // RFC 7914 section 11.
    assert_eq!(
        results["pbkdf2"],
        json!([
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
             49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783",
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56\
             a1d425a1225833549adb841b51c9b3176a272bdebba1d078478f62b397f33c8d"
        ])
    );
    // RFC 7677 section 3 (password "pencil"): the proof and the server signature.
    assert_eq!(
        results["scram"],
        json!({
            "proof": "dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=",
            "serverSignature": "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4="
        })
    );
}

#[test]
fn the_page_crypto_agrees_with_ring_on_other_inputs() {
    // Lengths around the 55/56/64-byte padding edges and the 64-byte HMAC key size.
    let lengths = [
        0usize, 1, 31, 32, 55, 56, 57, 63, 64, 65, 119, 120, 128, 200,
    ];
    let data = |length: usize, seed: u8| -> Vec<u8> {
        (0..length)
            .map(|index| (index as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    };
    let cases: Vec<Value> = lengths
        .iter()
        .map(|&length| json!({"data": hex(&data(length, 7)), "key": hex(&data(length, 3))}))
        .collect();
    let derivations = [
        (PASSWORD, 16usize, 1u32, 32usize),
        ("12345678", 16, 2, 20),
        ("x", 64, 1000, 65),
        (PASSWORD, 16, ITERATIONS, 32),
    ];
    let derivation_cases: Vec<Value> = derivations
        .iter()
        .map(|(password, salt, iterations, length)| {
            json!({
                "password": password,
                "salt": hex(&data(*salt, 9)),
                "iterations": iterations,
                "length": length,
            })
        })
        .collect();
    let driver = format!(
        r#"
const C = SdeckCrypto;
const bytes = (text) => Uint8Array.from(text.match(/../g) || [], (pair) => parseInt(pair, 16));
const cases = {cases};
const derivations = {derivations};
process.stdout.write(JSON.stringify({{
  sha256: cases.map((c) => C.toHex(C.sha256(bytes(c.data)))),
  hmac: cases.map((c) => C.toHex(C.hmac(bytes(c.key), bytes(c.data)))),
  pbkdf2: derivations.map((d) => C.toHex(C.pbkdf2(C.utf8(d.password), bytes(d.salt), d.iterations, d.length)))
}}));
"#,
        cases = Value::from(cases.clone()),
        derivations = Value::from(derivation_cases),
    );
    let Some(results) = node(&with_crypto(&driver)) else {
        return;
    };
    for (index, &length) in lengths.iter().enumerate() {
        let (key, message) = (data(length, 3), data(length, 7));
        assert_eq!(
            results["sha256"][index],
            hex(digest::digest(&digest::SHA256, &message).as_ref()),
            "SHA-256 of {length} bytes"
        );
        assert_eq!(
            results["hmac"][index],
            hex(&hmac_sha256(&key, &message)),
            "HMAC with a {length}-byte key"
        );
    }
    for (index, (password, salt, iterations, length)) in derivations.iter().enumerate() {
        assert_eq!(
            results["pbkdf2"][index],
            hex(&pbkdf2_sha256(
                password.as_bytes(),
                &data(*salt, 9),
                *iterations,
                *length
            )),
            "PBKDF2 of {password:?}, {iterations} iterations, {length} bytes"
        );
    }
}

/// A stub DOM and `fetch` for the login page's script; `FAKE` answers in
/// place of a server, or requests go to the real server at `PORT`.
const PAGE_HARNESS: &str = r#"
var window = globalThis;
const requests = [];
const cookies = [];
let replaced = null;
let idle = null;
function element(id) {
  return {
    id, hidden: id === "login" || id === "needs", disabled: false, value: "",
    type: id === "password" ? "password" : "", textContent: "", className: "",
    attributes: {}, listeners: {},
    classList: { toggle() {}, add() {}, remove() {} },
    setAttribute(name, value) { this.attributes[name] = String(value); },
    removeAttribute(name) {
      delete this.attributes[name];
      if (name === "aria-busy" && idle) { const done = idle; idle = null; done(); }
    },
    addEventListener(type, listener) { (this.listeners[type] = this.listeners[type] || []).push(listener); },
    append(...nodes) { for (const node of nodes) this.textContent += typeof node === "string" ? node : node.textContent; },
    focus() {}
  };
}
const elements = {};
var document = {
  getElementById(id) { return elements[id] || (elements[id] = element(id)); },
  createElement(tag) { return element(tag); }
};
var location = { replace(url) { replaced = url; }, assign(url) { replaced = url; } };
globalThis.matchMedia = () => ({ matches: false });
const realFetch = globalThis.fetch;
globalThis.fetch = function (path, init) {
  requests.push({ path, method: init.method, referrerPolicy: init.referrerPolicy,
    headers: init.headers, body: init.body === undefined ? null : init.body });
  if (typeof FAKE === "function") {
    const [status, data] = FAKE(path, init.body === undefined ? null : JSON.parse(init.body));
    return Promise.resolve({ ok: status >= 200 && status < 300, status, json: () => Promise.resolve(data) });
  }
  return realFetch("http://127.0.0.1:" + PORT + path,
    { method: init.method, headers: init.headers, body: init.body }).then((response) => {
    cookies.push(...response.headers.getSetCookie());
    return response;
  });
};
function submit(password) {
  return new Promise((resolve) => {
    idle = resolve;
    replaced = null;
    document.getElementById("password").value = password;
    document.getElementById("login").listeners.submit[0]({ preventDefault() {} });
  }).then(() => ({ banner: document.getElementById("error").textContent, replaced }));
}
"#;

fn page_script(prelude: &str, driver: &str) -> String {
    let page = scripts(LOGIN_PAGE);
    format!(
        "{prelude}\n{PAGE_HARNESS}\n{}\n{}\n{driver}",
        page[0], page[1]
    )
}

fn get_home(port: u16, cookie: &str) -> String {
    let mut tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(
        tcp,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\n\r\n"
    )
    .unwrap();
    let mut answer = String::new();
    tcp.read_to_string(&mut answer).unwrap();
    answer
}

#[test]
fn the_login_page_logs_in_to_the_real_server_without_sending_the_password() {
    let (sender, _inputs) = mpsc::channel();
    let server = VirtualDeckServer::start(
        ServerConfig {
            port: 0,
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        },
        build_verifier(PASSWORD).unwrap(),
        sender,
        Box::new(Vec::new),
    )
    .unwrap();
    let port = server.local_port();
    let driver = format!(
        r#"
(async () => {{
  const form = document.getElementById("login");
  const shown = !form.hidden;
  const wrong = await submit("wrong password");
  const right = await submit({password});
  process.stdout.write(JSON.stringify({{ shown, wrong, right, requests, cookies }}));
  process.exit(0);
}})();
"#,
        password = Value::from(PASSWORD)
    );
    let Some(results) = node(&page_script(&format!("const PORT = {port};"), &driver)) else {
        return;
    };
    assert_eq!(results["shown"], true, "with JavaScript the form shows");
    assert_eq!(
        results["wrong"],
        json!({
            "banner": "Wrong password. 4 tries left before a 30-second wait.",
            "replaced": null
        })
    );
    assert_eq!(results["right"], json!({"banner": "", "replaced": "/"}));
    let requests = results["requests"].as_array().unwrap();
    let paths: Vec<&str> = requests
        .iter()
        .map(|request| request["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "/login/start",
            "/login/finish",
            "/login/start",
            "/login/finish"
        ]
    );
    let sent = serde_json::to_string(requests).unwrap();
    let escaped = serde_json::to_string(PASSWORD).unwrap();
    for form in [
        PASSWORD,
        escaped.trim_matches('"'),
        "contrase",
        "wrong password",
        &STANDARD.encode(PASSWORD),
        &hex(PASSWORD.as_bytes()),
    ] {
        assert!(!sent.contains(form), "{form:?} was sent: {sent}");
    }
    for request in requests {
        assert_eq!(request["method"], "POST");
        assert_eq!(request["referrerPolicy"], "same-origin");
    }
    let finish: Value = serde_json::from_str(requests[3]["body"].as_str().unwrap()).unwrap();
    let mut keys: Vec<&String> = finish.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["client_nonce", "nonce", "proof"]);
    // The cookie the server set opens the deck page.
    let cookies = results["cookies"].as_array().unwrap();
    assert_eq!(cookies.len(), 1, "{cookies:?}");
    let cookie = cookies[0].as_str().unwrap().split(';').next().unwrap();
    let home = get_home(port, cookie);
    assert!(
        home.contains("<meta name=\"sdeck-page\" content=\"deck\">"),
        "{home}"
    );
    server.stop();
}

#[test]
fn the_login_page_refuses_a_server_that_cannot_prove_the_password() {
    let driver = r#"
var FAKE = (path) => {
  if (path === "/login/start") {
    return [200, { salt: "c2FsdHNhbHRzYWx0c2FsdA==", iterations: 1000, nonce: "ab".repeat(32) }];
  }
  if (path === "/login/finish") {
    return [200, { server_signature: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=" }];
  }
  return [303, {}];
};
(async () => {
  const outcome = await submit("12345678");
  process.stdout.write(JSON.stringify({ outcome, paths: requests.map((r) => r.path) }));
  process.exit(0);
})();
"#;
    let Some(results) = node(&page_script("", driver)) else {
        return;
    };
    assert_eq!(
        results["outcome"],
        json!({
            "banner": "Could not log in. This server could not prove it knows the password. \
                       Check the address.",
            "replaced": null
        })
    );
    assert_eq!(
        results["paths"],
        json!(["/login/start", "/login/finish", "/logout"]),
        "the session it may have set is dropped"
    );
}

#[test]
fn the_login_page_shows_the_wait_and_the_expired_login_the_server_reports() {
    let driver = r#"
let answers = [];
var FAKE = (path) => answers.shift();
(async () => {
  answers = [[429, { error: "Too many tries.", retry_after: 30 }]];
  const locked = await submit("12345678");
  const disabled = document.getElementById("submit").disabled;
  process.stdout.write(JSON.stringify({ locked, disabled }));
  process.exit(0);
})();
"#;
    let Some(results) = node(&page_script("", driver)) else {
        return;
    };
    assert_eq!(
        results,
        json!({
            "locked": {"banner": "Too many tries. Try again in 30 s.", "replaced": null},
            "disabled": true
        })
    );
    let driver = r#"
let answers = [];
var FAKE = (path) => answers.shift();
(async () => {
  answers = [
    [200, { salt: "c2FsdHNhbHRzYWx0c2FsdA==", iterations: 1000, nonce: "cd".repeat(32) }],
    [400, { error: "The login expired. Try again." }]
  ];
  const expired = await submit("12345678");
  process.stdout.write(JSON.stringify({ expired }));
  process.exit(0);
})();
"#;
    let Some(results) = node(&page_script("", driver)) else {
        return;
    };
    assert_eq!(
        results["expired"],
        json!({"banner": "Could not log in. The login expired. Try again.", "replaced": null})
    );
}
