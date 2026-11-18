//! Keys inside a Type text value, such as `aldo{Tab}secret{Enter}`.
//!
//! `{Enter}`, `{Tab}`, `{Esc}`, `{Up}`, `{Down}`, `{Left}` and `{Right}` (in any letter case)
//! press that key at their place in the text; `{{` types a literal `{`. Any other brace is typed
//! as written, so a text saved before keys existed never becomes an error.

/// A key named inside a Type text value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKey {
    Enter,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
}

impl TextKey {
    /// Every key a text can name, in the order the editor offers them.
    pub const ALL: [Self; 7] = [
        Self::Enter,
        Self::Tab,
        Self::Escape,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
    ];

    /// The token that names the key in a text, such as `{Esc}`.
    pub fn token(self) -> &'static str {
        match self {
            Self::Enter => "{Enter}",
            Self::Tab => "{Tab}",
            Self::Escape => "{Esc}",
            Self::Up => "{Up}",
            Self::Down => "{Down}",
            Self::Left => "{Left}",
            Self::Right => "{Right}",
        }
    }

    /// The symbol a preview shows for the key.
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Enter => "⏎",
            Self::Tab => "⇥",
            Self::Escape => "⎋",
            Self::Up => "↑",
            Self::Down => "↓",
            Self::Left => "←",
            Self::Right => "→",
        }
    }

    /// The XKB key name `wtype -k` presses.
    pub(super) fn keysym(self) -> &'static str {
        match self {
            Self::Enter => "Return",
            Self::Tab => "Tab",
            Self::Escape => "Escape",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }

    /// The physical key (a `keyboard::parse_chord` name) pressed when wtype is missing.
    pub(super) fn chord(self) -> &'static str {
        match self {
            Self::Enter => "ENTER",
            Self::Tab => "TAB",
            Self::Escape => "ESCAPE",
            Self::Up => "UP",
            Self::Down => "DOWN",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
        }
    }

    /// The key a token's inner name (between the braces) names, ignoring letter case.
    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| {
            let token = key.token();
            token[1..token.len() - 1].eq_ignore_ascii_case(name)
        })
    }
}

/// One part of a Type text value: text typed as written, or a key pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextSegment {
    Text(String),
    Key(TextKey),
}

/// Split a Type text value into the text and the keys it types, in order.
///
/// Adjacent text is one segment and empty text is left out, so a value without keys is one
/// text segment (none when it is empty). Never fails: unknown braces stay text.
pub fn text_segments(value: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut text = String::new();
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        text.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        if let Some(tail) = after.strip_prefix('{') {
            text.push('{');
            rest = tail;
            continue;
        }
        // A token ends at the first brace; "{Foo{Tab}" is "{Foo" then the Tab key.
        let key = after
            .find(['{', '}'])
            .filter(|&close| after[close..].starts_with('}'))
            .and_then(|close| Some((TextKey::named(&after[..close])?, close)));
        match key {
            Some((key, close)) => {
                if !text.is_empty() {
                    segments.push(TextSegment::Text(std::mem::take(&mut text)));
                }
                segments.push(TextSegment::Key(key));
                rest = &after[close + 1..];
            }
            None => {
                text.push('{');
                rest = after;
            }
        }
    }
    text.push_str(rest);
    if !text.is_empty() {
        segments.push(TextSegment::Text(text));
    }
    segments
}

/// The wtype runs that type `segments`, then Return when `enter`: each run's arguments and its
/// stdin. wtype reads stdin ("-") once per run, so each text segment gets its own run, holding
/// the keys after it (and the first one the keys before it). Text with no keys stays the single
/// run `wtype -` (`wtype - -k Return` with Enter).
pub(super) fn wtype_runs(segments: &[TextSegment], enter: bool) -> Vec<(Vec<String>, String)> {
    let mut runs = Vec::new();
    let mut args: Vec<String> = Vec::new();
    let mut input: Option<String> = None;
    let press = |args: &mut Vec<String>, keysym: &str| {
        args.push("-k".into());
        args.push(keysym.into());
    };
    for segment in segments {
        match segment {
            TextSegment::Text(text) => {
                if let Some(typed) = input.take() {
                    runs.push((std::mem::take(&mut args), typed));
                }
                args.push("-".into());
                input = Some(text.clone());
            }
            TextSegment::Key(key) => press(&mut args, key.keysym()),
        }
    }
    if enter {
        press(&mut args, TextKey::Enter.keysym());
    }
    if !args.is_empty() {
        runs.push((args, input.unwrap_or_default()));
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::{text_segments, TextKey, TextSegment};

    fn text(value: &str) -> TextSegment {
        TextSegment::Text(value.into())
    }

    fn key(key: TextKey) -> TextSegment {
        TextSegment::Key(key)
    }

    #[test]
    fn each_token_is_its_key() {
        for (value, expected) in [
            ("{Enter}", TextKey::Enter),
            ("{Tab}", TextKey::Tab),
            ("{Esc}", TextKey::Escape),
            ("{Up}", TextKey::Up),
            ("{Down}", TextKey::Down),
            ("{Left}", TextKey::Left),
            ("{Right}", TextKey::Right),
        ] {
            assert_eq!(text_segments(value), [key(expected)], "{value}");
            assert_eq!(expected.token(), value);
        }
        assert_eq!(TextKey::ALL.map(TextKey::token).len(), 7);
    }

    #[test]
    fn tokens_ignore_letter_case() {
        assert_eq!(
            text_segments("{ENTER}{tab}{eSc}{UP}{down}{LeFt}{RIGHT}"),
            TextKey::ALL.map(key)
        );
    }

    #[test]
    fn a_double_brace_types_one_literal_brace() {
        assert_eq!(text_segments("{{"), [text("{")]);
        assert_eq!(text_segments("a{{b"), [text("a{b")]);
        // Escaping a token types the token itself.
        assert_eq!(text_segments("{{Enter}"), [text("{Enter}")]);
        assert_eq!(text_segments("{{{Tab}"), [text("{"), key(TextKey::Tab)]);
        assert_eq!(text_segments("{{{{"), [text("{{")]);
    }

    #[test]
    fn unknown_braces_are_typed_literally() {
        for value in [
            "{Foo}",
            "{}",
            "{",
            "}",
            "}}",
            "{Enter",
            "Enter}",
            "{ Enter }",
            "{Escape}",
            "{Return}",
            "{日本}",
            "fn main() { println!(\"{x}\"); }",
        ] {
            assert_eq!(text_segments(value), [text(value)], "{value}");
        }
        assert_eq!(
            text_segments("{Foo{Enter}"),
            [text("{Foo"), key(TextKey::Enter)]
        );
        assert_eq!(text_segments("{Tab}}"), [key(TextKey::Tab), text("}")]);
    }

    #[test]
    fn adjacent_tokens_and_tokens_at_both_ends_keep_their_order() {
        assert_eq!(
            text_segments("{Tab}{Tab}"),
            [key(TextKey::Tab), key(TextKey::Tab)]
        );
        assert_eq!(
            text_segments("{Tab}aldo{Enter}"),
            [key(TextKey::Tab), text("aldo"), key(TextKey::Enter)]
        );
        assert_eq!(
            text_segments("aldo{Tab}mi-clave{Enter}"),
            [
                text("aldo"),
                key(TextKey::Tab),
                text("mi-clave"),
                key(TextKey::Enter)
            ]
        );
    }

    #[test]
    fn empty_text_is_left_out() {
        assert_eq!(text_segments(""), []);
        assert_eq!(
            text_segments("{Up}{Down}"),
            [key(TextKey::Up), key(TextKey::Down)]
        );
        // Spaces and line breaks are text, never dropped.
        assert_eq!(
            text_segments(" {Tab}\n"),
            [text(" "), key(TextKey::Tab), text("\n")]
        );
    }

    #[test]
    fn unicode_text_around_tokens_is_kept_exactly() {
        assert_eq!(
            text_segments("日本{Tab}λ\n{Enter}é  "),
            [
                text("日本"),
                key(TextKey::Tab),
                text("λ\n"),
                key(TextKey::Enter),
                text("é  ")
            ]
        );
        assert_eq!(text_segments("  日本語\nλ  "), [text("  日本語\nλ  ")]);
    }
}
