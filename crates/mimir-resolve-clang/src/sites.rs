//! The sites `clangd` is asked about, and what its answers name.

use std::path::Path;

use mimir_lsp::{LspError, Server};
use mimir_parse_core::ExtractedReference;
use mimir_parse_core::extracted::CallForm;
use mimir_types::ReferenceKind;
use serde_json::Value;

use crate::scip::utf16_column;

const SYMBOL_INFO: &str = "textDocument/symbolInfo";

pub fn is_site(kind: ReferenceKind) -> bool {
    matches!(
        kind,
        ReferenceKind::Call
            | ReferenceKind::FunctionValue
            | ReferenceKind::TypeUse
            | ReferenceKind::FieldUse
            | ReferenceKind::FieldWrite
    )
}

/// A site, by what the parser found: a 1-based line, a byte column and the name written.
pub struct Site<'a> {
    pub line: u32,
    pub column: u32,
    pub name: &'a str,
    /// Where the name ends in the file, if what it runs could be a constructor.
    pub builds_after: Option<usize>,
}

/// The ids `clangd` gives of one site.
#[derive(Default)]
pub struct SiteIds {
    /// What the name is.
    pub named: Vec<String>,
    /// The constructor run there, from the bracket after the name.
    pub built: Vec<String>,
}

/// The id of each symbol at these sites. None at a site means no answer: what `clangd` would say of it is
/// a guess from a word's text. At the name in `T(1)` it says the class, and at the bracket the constructor
/// that this call runs.
pub fn ids_at(
    server: &mut Server,
    path: &Path,
    source: &str,
    sites: &[Site<'_>],
) -> Result<Vec<SiteIds>, LspError> {
    let lines: Vec<&str> = source.split('\n').collect();
    let positions: Vec<(u32, u32)> = sites
        .iter()
        .map(|site| {
            let text = lines.get(site.line as usize - 1).copied().unwrap_or("");
            (site.line - 1, utf16_column(text, site.column))
        })
        .collect();
    let answers = server.answers(SYMBOL_INFO, path, &positions)?;
    let mut ids: Vec<SiteIds> = answers
        .iter()
        .zip(sites)
        .zip(&positions)
        .map(|((answer, site), &at)| SiteIds {
            named: answer
                .as_ref()
                .map_or_else(|_| Vec::new(), |found| ids_in(found, site.name, at)),
            built: Vec::new(),
        })
        .collect();
    let brackets: Vec<(usize, (u32, u32))> = sites
        .iter()
        .enumerate()
        .filter_map(|(i, site)| {
            let end = site.builds_after?;
            Some((
                i,
                position_of(source, opening_after(source, end)?, end, site.line),
            ))
        })
        .collect();
    let at: Vec<(u32, u32)> = brackets.iter().map(|&(_, at)| at).collect();
    let answers = server.answers(SYMBOL_INFO, path, &at)?;
    for ((i, _), answer) in brackets.iter().zip(&answers) {
        if let Ok(found) = answer {
            ids[*i].built = constructors_in(found);
        }
    }
    Ok(ids)
}

/// The bracket that opens the arguments after a name, past any template arguments.
fn opening_after(source: &str, end: usize) -> Option<usize> {
    let after = source.get(end..)?;
    let mut at = end + (after.len() - after.trim_start().len());
    if source[at..].starts_with('<') {
        let mut depth = 0;
        let close = source[at..].char_indices().find_map(|(i, c)| {
            match c {
                '<' => depth += 1,
                '>' => depth -= 1,
                ';' => return Some(None),
                _ => {}
            }
            (depth == 0).then_some(Some(i))
        })??;
        at += close + 1;
        at += source[at..].len() - source[at..].trim_start().len();
    }
    source[at..].starts_with(['(', '{']).then_some(at)
}

/// A 0-based line and UTF-16 column of a byte offset, from a place before it on a known 1-based line.
fn position_of(source: &str, at: usize, from: usize, line: u32) -> (u32, u32) {
    let start = source[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let lines = source[from..at].matches('\n').count() as u32;
    (
        line - 1 + lines,
        utf16_column(&source[start..at], (at - start) as u32),
    )
}

/// The ids of the constructors in an answer: what is named as its own class is.
fn constructors_in(found: &Value) -> Vec<String> {
    found
        .as_array()
        .into_iter()
        .flatten()
        .filter(|symbol| is_constructor(symbol))
        .filter_map(|symbol| symbol["id"].as_str().map(str::to_string))
        .collect()
}

fn is_constructor(symbol: &Value) -> bool {
    let (Some(name), Some(container)) = (symbol["name"].as_str(), symbol["containerName"].as_str())
    else {
        return false;
    };
    let name = name.split('<').next().unwrap_or(name);
    let Some(owner) = container.strip_suffix("::") else {
        return false;
    };
    without_arguments(owner).rsplit("::").next() == Some(name)
}

/// `a::B` of `a::B<c::D>`.
fn without_arguments(name: &str) -> &str {
    if !name.ends_with('>') {
        return name;
    }
    let mut depth = 0;
    for (at, c) in name.char_indices().rev() {
        match c {
            '>' => depth += 1,
            '<' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return &name[..at];
        }
    }
    name
}

/// The ids of what a site names. Under an alias `clangd` gives what it stands for too, by another name, and
/// the site names the alias. A site where a symbol is declared or defined is no use of it, though the
/// parser may have read it as one, behind a macro.
fn ids_in(found: &Value, name: &str, at: (u32, u32)) -> Vec<String> {
    found
        .as_array()
        .into_iter()
        .flatten()
        .filter(|symbol| symbol["name"].as_str() == Some(name))
        .filter(|symbol| {
            !["declarationRange", "definitionRange"]
                .iter()
                .any(|range| starts_at(&symbol[*range]["range"], at))
        })
        .filter_map(|symbol| symbol["id"].as_str().map(str::to_string))
        .collect()
}

fn starts_at(range: &Value, at: (u32, u32)) -> bool {
    let start = &range["start"];
    start["line"].as_u64() == Some(u64::from(at.0))
        && start["character"].as_u64() == Some(u64::from(at.1))
}

/// `T x(1)`, whose site has the variable's name and runs a constructor of a class that may be local.
pub fn runs_at_variable(reference: &ExtractedReference) -> bool {
    reference.kind == ReferenceKind::Call && reference.call_form.is_none()
}

pub fn site_of(reference: &ExtractedReference) -> Site<'_> {
    Site {
        line: reference.range.start_line,
        column: reference.range.start_col,
        name: &reference.name,
        builds_after: (!matches!(
            reference.call_form,
            Some(CallForm::Method | CallForm::Bound)
        ))
        .then_some(reference.span.end as usize),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_bracket_is_past_template_arguments_and_on_any_line() {
        assert_eq!(opening_after("T(1)", 1), Some(1));
        assert_eq!(opening_after("T  {1}", 1), Some(3));
        assert_eq!(opening_after("B<a<b>, c> (1)", 1), Some(11));
        assert_eq!(opening_after("T\n(1)", 1), Some(2));
        assert_eq!(opening_after("T x;", 1), None);
        assert_eq!(opening_after("a < b; (c)", 1), None);
        assert_eq!(position_of("щ T(1)", 4, 3, 1), (0, 3));
        assert_eq!(position_of("\nT\n  (1)", 5, 2, 2), (2, 2));
    }

    #[test]
    fn a_constructor_is_named_as_its_class_is() {
        let of = |name: &str, container: &str| json!({"name": name, "containerName": container});
        assert!(is_constructor(&of("T", "ns::T::")));
        assert!(is_constructor(&of("B<U>", "ns::B::")));
        assert!(is_constructor(&of("M", "t::M<std::a::b>::")));
        assert!(!is_constructor(&of("T", "ns::")));
        assert!(!is_constructor(&of("T", "ns::OtherT::")));
        assert!(!is_constructor(&of("f", "ns::T::")));
        assert!(!is_constructor(&json!({"name": "T"})));
    }

    #[test]
    fn the_ids_of_a_site_are_of_what_it_names_and_only_a_use() {
        let at = (3, 4);
        let found = json!([
            {"id": "BASE", "name": "iter_base"},
            {"id": "ALIAS", "name": "iter"},
            {"name": "iter"},
        ]);
        assert_eq!(ids_in(&found, "iter", at), ["ALIAS"]);
        assert!(ids_in(&Value::Null, "iter", at).is_empty());

        let declared = json!([{
            "id": "A1", "name": "f",
            "declarationRange": {"range": {"start": {"line": 3, "character": 4}}},
        }]);
        assert!(ids_in(&declared, "f", at).is_empty());
        let defined = json!([{
            "id": "A1", "name": "f",
            "definitionRange": {"range": {"start": {"line": 3, "character": 4}}},
        }]);
        assert!(ids_in(&defined, "f", at).is_empty());
        let used = json!([{
            "id": "A1", "name": "f",
            "declarationRange": {"range": {"start": {"line": 9, "character": 4}}},
        }]);
        assert_eq!(ids_in(&used, "f", at), ["A1"]);
    }
}
