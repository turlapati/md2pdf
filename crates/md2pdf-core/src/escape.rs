//! Escaping helpers for emitting Typst source.

/// Escape a prose run for Typst *markup* mode.
///
/// Typst gives meaning to many ASCII punctuation characters in markup mode
/// (`*`, `_`, `` ` ``, `#`, `$`, `@`, `<`, `>`, `[`, `]`, `\`, `/`, `~`,
/// line-leading `-`, `+`, `=`, and the `--`/`---`/`'`/`"` smart-typography
/// conversions). Markdown already resolved all formatting, so every one of
/// these must render literally. Typst accepts a backslash before any ASCII
/// punctuation character, so we escape the full set and never have to reason
/// about line position.
pub fn markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '\\' | '#' | '*' | '_' | '`' | '$' | '@' | '<' | '>' | '[' | ']' | '/' | '~' | '-'
            | '+' | '=' | '\'' | '"' | ':' => {
                out.push('\\');
                out.push(ch);
            }
            // Non-breaking / control characters that Typst would reject or
            // mangle are normalised to plain spaces.
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape text for a Typst *string literal* (`"..."`) in code mode.
pub fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// GitHub-style heading slug (lowercase, strip punctuation, spaces to `-`).
pub fn slug(text: &str) -> String {
    let mut s = String::new();
    for ch in text.trim().chars() {
        if ch.is_alphanumeric() {
            s.extend(ch.to_lowercase());
        } else if ch == ' ' || ch == '-' {
            s.push('-');
        }
    }
    s
}

/// Only letters, digits, `-`, `_` are safe inside a Typst label.
pub fn label_safe(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_escapes_specials() {
        assert_eq!(markup("a*b_c#d"), "a\\*b\\_c\\#d");
        assert_eq!(markup("x -- y [z]"), "x \\-\\- y \\[z\\]");
    }

    #[test]
    fn string_escapes() {
        assert_eq!(string("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Hello, World! 2"), "hello-world-2");
    }
}
