//! GTK-free checks of the two theme files: tone contrast, destructive hover and token hygiene.

use std::collections::BTreeMap;

const DARK: &str = include_str!("../src/ui.css");
const LIGHT: &str = include_str!("../src/ui-light.css");

type Rgb = [f64; 3];

fn tokens(css: &str) -> BTreeMap<String, Rgb> {
    let mut map = BTreeMap::new();
    for line in css.lines() {
        let Some(rest) = line.strip_prefix("@define-color ") else {
            continue;
        };
        let (name, value) = rest.split_once(' ').unwrap();
        let hex = value.trim_end_matches(';').trim_start_matches('#');
        let channel = |i: usize| f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap()) / 255.0;
        map.insert(name.to_string(), [channel(0), channel(2), channel(4)]);
    }
    map
}

fn luminance(c: Rgb) -> f64 {
    let f = |v: f64| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
}

fn ratio(a: Rgb, b: Rgb) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn blend(fg: Rgb, bg: Rgb, alpha: f64) -> Rgb {
    [0, 1, 2].map(|i| alpha * fg[i] + (1.0 - alpha) * bg[i])
}

/// Bodies of every rule whose selector is exactly `selector`, joined.
fn rule(css: &str, selector: &str) -> String {
    let needle = format!("\n{selector} {{");
    let mut bodies = String::new();
    for (at, _) in css.match_indices(&needle) {
        let start = at + needle.len();
        bodies.push_str(&css[start..start + css[start..].find('}').unwrap()]);
        bodies.push('\n');
    }
    assert!(!bodies.is_empty(), "rule {selector} missing");
    bodies
}

/// `alpha(@desk_<tone>, A)` used by a `background-color` declaration of the rule.
fn tint_alpha(body: &str, tone: &str) -> f64 {
    let key = format!("background-color: alpha(@desk_{tone}, ");
    let at = body
        .find(&key)
        .unwrap_or_else(|| panic!("no {tone} tint in {body}"))
        + key.len();
    body[at..at + body[at..].find(')').unwrap()]
        .parse()
        .unwrap()
}

#[test]
fn tone_text_on_its_own_tint_reaches_4_5_over_surface_and_bg() {
    let mut failures = Vec::new();
    for (theme, css) in [("dark", DARK), ("light", LIGHT)] {
        // The light file only overrides tokens; the rules live in the dark file.
        let map = tokens(css);
        for (tone, selectors) in [
            ("warning", vec![".status-idle", "label.feedback-warning"]),
            ("success", vec![".status-online", "label.feedback-success"]),
            ("error", vec![".status-error", "label.feedback-error"]),
        ] {
            for selector in selectors {
                let alpha = tint_alpha(&rule(DARK, selector), tone);
                for base in ["desk_surface", "desk_bg"] {
                    let fg = map[&format!("desk_{tone}")];
                    let r = ratio(fg, blend(fg, map[base], alpha));
                    if r < 4.5 {
                        failures.push(format!("{theme} {selector} on {base}: {r:.2}"));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "below 4.5:1: {failures:#?}");
}

#[test]
fn destructive_command_row_hover_reaches_4_5_in_both_themes() {
    let body = rule(
        DARK,
        "popover.command-menu button.command-row.destructive:hover",
    );
    let alpha = tint_alpha(&body, "error");
    assert!((alpha - 0.12).abs() < 1e-9, "alpha is {alpha}");
    for (theme, css) in [("dark", DARK), ("light", LIGHT)] {
        let map = tokens(css);
        let fg = map["desk_error"];
        let r = ratio(fg, blend(fg, map["desk_surface"], alpha));
        assert!(r >= 4.5, "{theme}: {r:.2}");
    }
}

#[test]
fn off_token_literals_are_gone_and_both_themes_define_the_same_tokens() {
    let dark_tokens: Vec<_> = tokens(DARK).into_keys().collect();
    let light_tokens: Vec<_> = tokens(LIGHT).into_keys().collect();
    assert_eq!(dark_tokens, light_tokens, "token sets differ");
    for css in [DARK, LIGHT] {
        for (n, line) in css.lines().enumerate() {
            if line.starts_with("@define-color") || line.contains(".strip-zone") {
                continue; // hardware-preview hover keeps its literal
            }
            for literal in [
                "#7c3aed", "#f0f0f2", "#22c55e", "#f59e0b", "#67e8f9", "#c7b6f5", "#a58cde",
                "#5939a9", "#4c2f95",
            ] {
                assert!(!line.contains(literal), "line {}: {line}", n + 1);
            }
        }
    }
}

#[test]
fn rail_badge_only_spaces_the_status_badge_so_its_tone_comes_from_status_rules() {
    // `.rail-badge` rides on a `.status-*` class (src/ui.rs); it must not recolour the badge.
    let body = rule(DARK, ".rail-badge");
    assert!(!body.contains("color"), "rail-badge sets a colour: {body}");
    for tone in [".status-idle", ".status-online", ".status-error"] {
        assert!(rule(DARK, tone).contains("color: @desk_"), "{tone}");
    }
}

#[test]
fn tone_tokens_as_plain_text_reach_4_5_on_surface_bg_field_and_hover() {
    let mut failures = Vec::new();
    for (theme, css) in [("dark", DARK), ("light", LIGHT)] {
        let map = tokens(css);
        for tone in ["desk_warning", "desk_success", "desk_error"] {
            for base in ["desk_surface", "desk_bg", "desk_field", "desk_hover"] {
                let r = ratio(map[tone], map[base]);
                if r < 4.5 {
                    failures.push(format!("{theme} {tone} on {base}: {r:.2}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "below 4.5:1: {failures:#?}");
}

/// Audit P3-2: the form and help popovers set a black 0.45 shadow with a class selector, which
/// outranks the light theme's `popover > contents` override, so the light theme must restate it.
#[test]
fn light_theme_overrides_the_dark_shadow_of_form_and_help_popovers() {
    for selector in [
        "popover.organize-popover > contents, .form-popover > contents",
        ".help-popover > contents",
    ] {
        assert!(
            rule(DARK, selector).contains("alpha(black, 0.45)"),
            "dark rule for {selector}"
        );
        let light = rule(LIGHT, selector);
        assert!(
            light.contains("alpha(@desk_ink, 0.18)") && !light.contains("black"),
            "light theme keeps the dark shadow on {selector}: {light:?}"
        );
    }
}
