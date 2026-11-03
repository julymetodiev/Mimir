//! Colouring Rust source by its words, for reading. It looks at text only, so it can be wrong about
//! what a name is; nothing depends on it but the colours.

/// What a piece of source looks like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Class {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Function,
    Macro,
    Attribute,
    Lifetime,
}

impl Class {
    /// The name the page's style knows it by, if it has a colour.
    pub fn style(self) -> Option<&'static str> {
        Some(match self {
            Self::Plain => return None,
            Self::Keyword => "k",
            Self::String => "s",
            Self::Comment => "c",
            Self::Number => "n",
            Self::Type => "t",
            Self::Function => "f",
            Self::Macro => "m",
            Self::Attribute => "a",
            Self::Lifetime => "l",
        })
    }
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

fn is_word_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where a run of characters that `wanted` accepts ends.
fn end_of_run(text: &str, from: usize, wanted: fn(char) -> bool) -> usize {
    text[from..]
        .find(|c| !wanted(c))
        .map_or(text.len(), |offset| from + offset)
}

/// Where a string that opens at `open`, just after its quote, ends: past its closing quote.
fn end_of_string(text: &str, open: usize) -> usize {
    let mut escaped = false;
    for (offset, c) in text[open..].char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return open + offset + 1,
            _ => {}
        }
    }
    text.len()
}

/// Where a block comment that opens at `start` ends. They nest.
fn end_of_block_comment(text: &str, start: usize) -> usize {
    let mut depth = 0;
    let mut at = start;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with("/*") {
            depth += 1;
            at += 2;
        } else if rest.starts_with("*/") {
            depth -= 1;
            at += 2;
            if depth == 0 {
                return at;
            }
        } else {
            at += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    text.len()
}

/// A raw string at `start`, such as `r#"a"#`: where it ends.
fn end_of_raw_string(text: &str, start: usize) -> Option<usize> {
    let rest = text[start..].strip_prefix('r')?;
    let hashes = rest.len() - rest.trim_start_matches('#').len();
    let body = rest[hashes..].strip_prefix('"')?;
    let closing = format!("\"{}", "#".repeat(hashes));
    let body_start = text.len() - body.len();
    Some(
        body.find(&closing)
            .map_or(text.len(), |end| body_start + end + closing.len()),
    )
}

/// Where an attribute that opens at `start` ends: past the bracket that closes it.
fn end_of_attribute(text: &str, start: usize) -> usize {
    let mut depth = 0;
    for (offset, c) in text[start..].char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return start + offset + 1;
                }
            }
            _ => {}
        }
    }
    text.len()
}

/// A character at `start`, just past its opening quote: where it ends, if it is one and not a
/// lifetime.
fn end_of_char(text: &str, open: usize) -> Option<usize> {
    let mut chars = text[open..].char_indices();
    let (_, first) = chars.next()?;
    if first == '\\' {
        return text[open + 1..]
            .find('\'')
            .map(|offset| open + 1 + offset + 1);
    }
    let (second_at, second) = chars.next()?;
    (second == '\'').then_some(open + second_at + 1)
}

fn class_of_word(word: &str, after: &str) -> Class {
    if KEYWORDS.contains(&word) {
        Class::Keyword
    } else if after.starts_with('!') && !after.starts_with("!=") {
        Class::Macro
    } else if after.starts_with('(') || after.starts_with("::<") {
        Class::Function
    } else if word.starts_with(char::is_uppercase) {
        Class::Type
    } else {
        Class::Plain
    }
}

/// The source in pieces, which put together give it back exactly.
pub fn pieces(text: &str) -> Vec<(Class, &str)> {
    let mut found: Vec<(Class, &str)> = Vec::new();
    let mut at = 0;
    let mut plain = 0;
    while at < text.len() {
        let rest = &text[at..];
        let Some(c) = rest.chars().next() else {
            break;
        };
        let span = if rest.starts_with("//") {
            Some((
                Class::Comment,
                rest.find('\n').map_or(text.len(), |end| at + end),
            ))
        } else if rest.starts_with("/*") {
            Some((Class::Comment, end_of_block_comment(text, at)))
        } else if rest.starts_with("#[") || rest.starts_with("#![") {
            Some((Class::Attribute, end_of_attribute(text, at)))
        } else if c == '"' {
            Some((Class::String, end_of_string(text, at + 1)))
        } else if rest.starts_with("b\"") {
            Some((Class::String, end_of_string(text, at + 2)))
        } else if let Some(end) = end_of_raw_string(text, at) {
            Some((Class::String, end))
        } else if c == '\'' {
            match end_of_char(text, at + 1) {
                Some(end) => Some((Class::String, end)),
                None => Some((Class::Lifetime, end_of_run(text, at + 1, is_word))),
            }
        } else if c.is_ascii_digit() {
            Some((
                Class::Number,
                end_of_run(text, at, |c| is_word(c) || c == '.'),
            ))
        } else if is_word_start(c) {
            let end = end_of_run(text, at, is_word);
            Some((class_of_word(&text[at..end], &text[end..]), end))
        } else {
            None
        };
        match span {
            Some((Class::Plain, end)) => at = end,
            Some((class, end)) => {
                if plain < at {
                    found.push((Class::Plain, &text[plain..at]));
                }
                found.push((class, &text[at..end]));
                at = end;
                plain = end;
            }
            None => at += c.len_utf8(),
        }
    }
    if plain < text.len() {
        found.push((Class::Plain, &text[plain..]));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(text: &str) -> Vec<(Class, &str)> {
        pieces(text)
            .into_iter()
            .filter(|(class, piece)| *class != Class::Plain || !piece.trim().is_empty())
            .collect()
    }

    #[test]
    fn test_the_pieces_give_the_source_back_exactly() {
        for text in [
            "fn main() { println!(\"{}\", 'a'); } // done",
            "/* a /* b */ c */ let s = r#\"x\"y\"#;",
            "#[derive(Debug)]\nstruct Имe<'a> { x: &'a str }\n",
            "\"unclosed",
            "'",
            "r#",
            "",
        ] {
            let joined: String = pieces(text).iter().map(|(_, piece)| *piece).collect();
            assert_eq!(joined, text);
        }
    }

    #[test]
    fn test_each_kind_of_word() {
        assert_eq!(
            classes("pub fn run(x: Config) -> u32 { vec![helper(1.5)] }"),
            [
                (Class::Keyword, "pub"),
                (Class::Keyword, "fn"),
                (Class::Function, "run"),
                (Class::Plain, "(x: "),
                (Class::Type, "Config"),
                (Class::Plain, ") -> u32 { "),
                (Class::Macro, "vec"),
                (Class::Plain, "!["),
                (Class::Function, "helper"),
                (Class::Plain, "("),
                (Class::Number, "1.5"),
                (Class::Plain, ")] }"),
            ]
        );
    }

    #[test]
    fn test_what_looks_like_code_inside_strings_and_comments_is_not() {
        assert_eq!(classes("\"fn x\""), [(Class::String, "\"fn x\"")]);
        assert_eq!(classes("// fn x"), [(Class::Comment, "// fn x")]);
        assert_eq!(classes("\"a\\\"b\""), [(Class::String, "\"a\\\"b\"")]);
        assert_eq!(
            classes("/* a /* b */ c */"),
            [(Class::Comment, "/* a /* b */ c */")]
        );
        assert_eq!(classes("r#\"\"fn\"\"#"), [(Class::String, "r#\"\"fn\"\"#")]);
    }

    #[test]
    fn test_a_lifetime_is_not_a_character() {
        assert_eq!(classes("'a'"), [(Class::String, "'a'")]);
        assert_eq!(classes("'\\n'"), [(Class::String, "'\\n'")]);
        assert_eq!(
            classes("&'static"),
            [(Class::Plain, "&"), (Class::Lifetime, "'static")]
        );
    }

    #[test]
    fn test_an_attribute_ends_at_its_own_bracket() {
        assert_eq!(
            classes("#[a(b[1])] fn"),
            [(Class::Attribute, "#[a(b[1])]"), (Class::Keyword, "fn")]
        );
    }
}
