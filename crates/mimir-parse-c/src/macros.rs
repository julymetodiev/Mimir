//! What tree-sitter cannot read, since a grammar does not know what a macro is. `class EXPORT Name {`
//! reads as a function called `Name`, and `int f() LOCKS(mu) {` as two declarations, which loses a whole
//! class or a method. So such a macro becomes spaces before parsing: every position stays what it is,
//! and nothing here says what a call refers to. See `docs/CPP.md`, section 3.

/// What a token is. A word is a name or a keyword.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Punctuation,
}

#[derive(Clone, Copy)]
struct Token {
    start: usize,
    end: usize,
    kind: Kind,
}

/// The source with such macros as spaces, or `None` if it has none.
pub fn blanked(source: &str) -> Option<String> {
    let bytes = source.as_bytes();
    let tokens = tokens(bytes);
    let word = |at: usize| -> &[u8] {
        tokens
            .get(at)
            .map_or(&[][..], |token| &bytes[token.start..token.end])
    };
    let mut blank: Vec<(usize, usize)> = Vec::new();
    let mut opened: Vec<usize> = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        match word(at) {
            b"(" => opened.push(at),
            b")" => {
                let opener = opened.pop();
                let controls =
                    opener
                        .and_then(|opener| opener.checked_sub(1))
                        .is_some_and(|before| {
                            matches!(
                                word(before),
                                b"if" | b"while" | b"for" | b"switch" | b"return" | b"sizeof"
                            )
                        });
                if !controls {
                    after_parameters(&tokens, bytes, at, &mut blank);
                }
            }
            // `API auto f(int a) {`: at the start of a declaration, before a type and a name.
            name if is_macro(name)
                && starts_declaration(&tokens, bytes, at)
                && before_a_function(&tokens, bytes, at + 1) =>
            {
                blank.push((token.start, token.end));
            }
            // `class EXPORT Name {`, `class EXPORT Name : Base`, `class EXPORT Name final`.
            b"class" | b"struct" | b"union"
                if is_macro(word(at + 1))
                    && tokens.get(at + 2).is_some_and(|t| t.kind == Kind::Word)
                    && matches!(word(at + 3), b"{" | b":" | b"final" | b"<") =>
            {
                blank.push((tokens[at + 1].start, tokens[at + 1].end));
            }
            _ => {}
        }
    }
    if blank.is_empty() {
        return None;
    }
    let mut out = bytes.to_vec();
    for (start, end) in blank {
        out[start..end].fill(b' ');
    }
    String::from_utf8(out).ok()
}

/// After the `)` of parameters: `NOEXCEPT`, `LOCKS(mu)`, with `const` and the like around them. They go
/// only before what starts a body or ends a definition: `if (x) LOG(y);` stays a call.
fn after_parameters(tokens: &[Token], bytes: &[u8], close: usize, blank: &mut Vec<(usize, usize)>) {
    let word = |at: usize| -> &[u8] {
        tokens
            .get(at)
            .map_or(&[][..], |token| &bytes[token.start..token.end])
    };
    let mut at = close + 1;
    let mut found = Vec::new();
    loop {
        match word(at) {
            b"const" | b"override" | b"final" | b"noexcept" | b"volatile" => at += 1,
            macro_name if is_macro(macro_name) => {
                let start = tokens[at].start;
                let mut end = tokens[at].end;
                at += 1;
                if word(at) == b"(" {
                    let mut depth = 0usize;
                    while let Some(token) = tokens.get(at) {
                        match word(at) {
                            b"(" => depth += 1,
                            b")" => depth = depth.saturating_sub(1),
                            _ => {}
                        }
                        at += 1;
                        if depth == 0 {
                            end = token.end;
                            break;
                        }
                    }
                }
                found.push((start, end));
            }
            _ => break,
        }
    }
    if !found.is_empty() && matches!(word(at), b"{" | b"=" | b":") {
        blank.extend(found);
    }
}

/// Whether a token comes where a declaration starts.
fn starts_declaration(tokens: &[Token], bytes: &[u8], at: usize) -> bool {
    let Some(before) = at.checked_sub(1).and_then(|before| tokens.get(before)) else {
        return true;
    };
    matches!(
        &bytes[before.start..before.end],
        b";" | b"}" | b"{" | b">" | b":"
    )
}

/// Whether what follows is a return type and a name with parameters: at least two words, or a word and
/// `*`, before the `(`. `FOO x;`, `FOO(x);` and `FOO x = 1;` are not: there `FOO` may well be a type or a
/// call.
fn before_a_function(tokens: &[Token], bytes: &[u8], from: usize) -> bool {
    let mut words = 0;
    for (seen, token) in tokens[from.min(tokens.len())..].iter().enumerate() {
        if seen > 12 {
            return false;
        }
        match (&bytes[token.start..token.end], token.kind) {
            (b"(", _) => return words >= 2,
            (_, Kind::Word) => words += 1,
            (b"*" | b"&" | b"::" | b"<" | b">" | b",", _) => {}
            _ => return false,
        }
    }
    false
}

/// A name only a macro has by custom: capitals, digits and `_`, of two characters or more.
fn is_macro(word: &[u8]) -> bool {
    word.len() >= 2
        && word
            .first()
            .is_some_and(|c| c.is_ascii_uppercase() || *c == b'_')
        && word
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

/// The words and punctuation of a source, outside comments, strings and preprocessor lines.
fn tokens(bytes: &[u8]) -> Vec<Token> {
    let mut found = Vec::new();
    let mut at = 0;
    let mut line_start = true;
    while at < bytes.len() {
        let c = bytes[at];
        match c {
            b'\n' => {
                line_start = true;
                at += 1;
            }
            b' ' | b'\t' | b'\r' => at += 1,
            b'#' if line_start => {
                // To the end of the line, and past a `\` at its end.
                while at < bytes.len() && bytes[at] != b'\n' {
                    if bytes[at] == b'\\' && bytes.get(at + 1) == Some(&b'\n') {
                        at += 1;
                    }
                    at += 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at += 2;
                while at < bytes.len() && !bytes[at..].starts_with(b"*/") {
                    at += 1;
                }
                at += 2;
            }
            b'"' | b'\'' => {
                let quote = c;
                at += 1;
                while at < bytes.len() && bytes[at] != quote && bytes[at] != b'\n' {
                    if bytes[at] == b'\\' {
                        at += 1;
                    }
                    at += 1;
                }
                at += 1;
            }
            c if c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80 => {
                let start = at;
                while at < bytes.len()
                    && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_' || bytes[at] >= 0x80)
                {
                    at += 1;
                }
                found.push(Token {
                    start,
                    end: at,
                    kind: Kind::Word,
                });
                line_start = false;
            }
            _ => {
                // `::` is one token, so that it is not taken for the `:` of an initializer list.
                let long = c == b':' && bytes.get(at + 1) == Some(&b':');
                let end = at + if long { 2 } else { 1 };
                found.push(Token {
                    start: at,
                    end,
                    kind: Kind::Punctuation,
                });
                at = end;
                line_start = false;
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(source: &str) -> String {
        let out = blanked(source).unwrap_or_else(|| source.to_string());
        assert_eq!(out.len(), source.len(), "every position stays");
        out
    }

    #[test]
    fn a_macro_before_a_class_name() {
        assert_eq!(
            blank("class EXPORT_API Slice {"),
            "class            Slice {"
        );
        assert_eq!(blank("struct API S : B {"), "struct     S : B {");
        assert_eq!(blank("class API C final {"), "class     C final {");
        assert_eq!(
            blank("template <> struct API Box<char> {"),
            "template <> struct     Box<char> {"
        );
    }

    #[test]
    fn a_class_without_one_stays() {
        for source in [
            "class A {};",
            "struct A a;",
            "class FOO;",
            "struct STAT buf;",
            "enum class E {};",
        ] {
            assert_eq!(blanked(source), None, "{source}");
        }
    }

    #[test]
    fn a_macro_after_parameters() {
        assert_eq!(blank("int f() NOEXCEPT {"), "int f()          {");
        assert_eq!(
            blank("bool f(int a) LOCKS(mu_) {"),
            "bool f(int a)            {"
        );
        assert_eq!(
            blank("int f() const LOCKS(a, (b)) {"),
            format!("int f() const {} {{", " ".repeat(13))
        );
        assert_eq!(
            blank("A::~A() NOEXCEPT = default;"),
            "A::~A()          = default;"
        );
        assert_eq!(
            blank("A::A() NOEXCEPT : x(1) {}"),
            "A::A()          : x(1) {}"
        );
        assert_eq!(
            blank("int f() ONE TWO(x) override {"),
            "int f()            override {"
        );
    }

    #[test]
    fn a_macro_before_a_function() {
        assert_eq!(blank("API int f(void) {"), "    int f(void) {");
        assert_eq!(
            blank("; FMT_INLINE auto f(T a) -> bool {"),
            "; {} auto f(T a) -> bool {".replace("{}", "          ")
        );
        assert_eq!(
            blank("template <class T> API T* f(T a) {"),
            "template <class T>     T* f(T a) {"
        );
    }

    #[test]
    fn a_macro_as_a_type_or_a_call_stays() {
        for source in [
            "DWORD x;",
            "DWORD x = 1;",
            "FOO(x);",
            "; TEST_F(a, b) {",
            "x = FOO y(1);",
        ] {
            assert_eq!(blanked(source), None, "{source}");
        }
    }

    #[test]
    fn a_call_stays_a_call() {
        for source in [
            "if (x) LOG(y);",
            "if (x) FOO {",
            "while (x) FOO {",
            "void f() ANNOTATE(x);",
            "foo(a) BAR;",
            "a ? (b) : c;",
            "x::y(a)::z;",
        ] {
            assert_eq!(blanked(source), None, "{source}");
        }
    }

    #[test]
    fn a_lowercase_name_stays() {
        assert_eq!(blanked("int f() try {"), None);
        assert_eq!(blanked("class api Slice {"), None);
    }

    #[test]
    fn comments_strings_and_preprocessor_lines_are_not_read() {
        for source in [
            "// class API Slice {\n",
            "/* int f() NOEXCEPT { */",
            "const char* s = \"class API Slice {\";",
            "#define C class API Slice {\n",
            "#define D \\\n class API Slice {\n",
        ] {
            assert_eq!(blanked(source), None, "{source}");
        }
    }

    #[test]
    fn text_beyond_ascii_keeps_its_bytes() {
        let source = "// щ\nclass API Щ {";
        assert_eq!(blank(source), "// щ\nclass     Щ {");
    }
}
