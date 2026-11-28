//! A short, human name for a viewer's browser, from its `User-Agent`.
//!
//! The result is built only from fixed names and digits, so whatever a client
//! sends never reaches the app's viewer list verbatim.

const UNKNOWN: &str = "Unknown browser";

/// Operating systems, most specific first: Android and iOS user agents also
/// name Linux and "Mac OS X".
const SYSTEMS: &[(&str, &str)] = &[
    ("Android", "Android"),
    ("iPhone", "iOS"),
    ("iPad", "iPadOS"),
    ("iPod", "iOS"),
    ("CrOS", "ChromeOS"),
    ("Windows", "Windows"),
    ("Macintosh", "macOS"),
    ("Mac OS X", "macOS"),
    ("Linux", "Linux"),
];

/// Browser tokens, most specific first: Edge, Opera and Samsung Internet also
/// say "Chrome/", and every Chromium browser also says "Safari/".
const BROWSERS: &[(&str, &str)] = &[
    ("EdgA/", "Edge"),
    ("EdgiOS/", "Edge"),
    ("Edg/", "Edge"),
    ("OPR/", "Opera"),
    ("SamsungBrowser/", "Samsung Internet"),
    ("FxiOS/", "Firefox"),
    ("Firefox/", "Firefox"),
    ("CriOS/", "Chrome"),
    ("Chromium/", "Chromium"),
    ("Chrome/", "Chrome"),
];

/// "Android · Chrome 131", "macOS · Safari 18", or "Unknown browser".
pub(super) fn describe(user_agent: &str) -> String {
    let Some(browser) = browser(user_agent) else {
        return UNKNOWN.to_string();
    };
    match system(user_agent) {
        Some(system) => format!("{system} · {browser}"),
        None => browser,
    }
}

fn system(user_agent: &str) -> Option<&'static str> {
    SYSTEMS
        .iter()
        .find(|(token, _)| user_agent.contains(token))
        .map(|(_, name)| *name)
}

fn browser(user_agent: &str) -> Option<String> {
    for (token, name) in BROWSERS {
        if let Some(version) = major_after(user_agent, token) {
            return Some(format!("{name} {version}"));
        }
    }
    // Safari names its own version after "Version/" and the WebKit build after "Safari/".
    if user_agent.contains("Safari/") {
        if let Some(version) = major_after(user_agent, "Version/") {
            return Some(format!("Safari {version}"));
        }
    }
    None
}

/// The major version right after `token`: 1 to 4 digits.
fn major_after(user_agent: &str, token: &str) -> Option<u32> {
    let start = user_agent.find(token)? + token.len();
    let digits: String = user_agent[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::describe;

    #[test]
    fn names_common_browsers_with_their_system_and_major_version() {
        let cases = [
            (
                "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/131.0.6778.135 Mobile Safari/537.36",
                "Android · Chrome 131",
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/18.1 Safari/605.1.15",
                "macOS · Safari 18",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
                "iOS · Safari 17",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) CriOS/131.0.6778.73 Mobile/15E148 Safari/604.1",
                "iOS · Chrome 131",
            ),
            (
                "Mozilla/5.0 (X11; Linux x86_64; rv:133.0) Gecko/20100101 Firefox/133.0",
                "Linux · Firefox 133",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/131.0.0.0 Safari/537.36 Edg/131.0.2903.70",
                "Windows · Edge 131",
            ),
            (
                "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) \
                 SamsungBrowser/26.0 Chrome/122.0.0.0 Mobile Safari/537.36",
                "Android · Samsung Internet 26",
            ),
        ];
        for (user_agent, expected) in cases {
            assert_eq!(describe(user_agent), expected, "{user_agent}");
        }
    }

    #[test]
    fn anything_else_is_an_unknown_browser() {
        for user_agent in ["", "curl/8.11.0", "Safari/605", "Chrome/", "<script>"] {
            assert_eq!(describe(user_agent), "Unknown browser", "{user_agent}");
        }
    }
}
