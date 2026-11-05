//! What `clangd-indexer --format=yaml` says of a project. It is read line by line, as it is large and its
//! form simple. What is outside the project is left out here, as most of it is.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use mimir_types::Language;

/// What an index keeps of a symbol of the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexSymbol {
    pub name: String,
    pub scope: String,
    pub kind: String,
    /// The file relative to the project, and where its name is: at its definition, or at its
    /// declaration if it has none, as a pure virtual.
    pub at: Place,
    /// Known only in the source file that defines it. The index records no use of such a symbol.
    pub local: bool,
}

/// Lines and columns from 0, a column in UTF-16 as `clangd` counts it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Place {
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub end_column: u32,
}

/// A use of a symbol, and how the index says it is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub symbol: String,
    pub at: Place,
}

#[derive(Debug, Default)]
pub struct Dex {
    pub symbols: HashMap<String, DexSymbol>,
    pub references: Vec<Reference>,
    /// Each definition a reference names, as a symbol of several programs has one in each.
    pub definitions: Vec<Reference>,
    /// An override, and what it overrides.
    pub overrides: Vec<(String, String)>,
}

/// `RefKind`: this is a use, and neither the declaration nor the definition.
const REFERENCE: u32 = 4;
/// `RefKind`: this is its definition.
const DEFINITION: u32 = 2;
/// `RelationKind::OverriddenBy`: the subject is what is overridden.
const OVERRIDDEN_BY: u32 = 1;

/// Read an index, for a project at this absolute path.
pub fn read(input: impl BufRead, root: &Path) -> std::io::Result<Dex> {
    let mut roots = vec![format!("{}/", root.display())];
    if let Ok(canonical) = root.canonicalize() {
        roots.push(format!("{}/", canonical.display()));
    }
    let mut dex = Dex::default();
    let mut record: Vec<String> = Vec::new();
    let mut references = Refs::default();
    let mut relations = Vec::new();
    for line in input.lines() {
        let line = line?;
        if line.starts_with("--- !") || line == "..." {
            take(&record, &roots, &mut dex, &mut references, &mut relations);
            record.clear();
        }
        record.push(line);
    }
    take(&record, &roots, &mut dex, &mut references, &mut relations);
    // Only now are all symbols known: what is outside the project goes.
    let known = |r: &Reference| dex.symbols.contains_key(&r.symbol);
    references.uses.retain(known);
    references.definitions.retain(known);
    dex.references = references.uses;
    dex.definitions = references.definitions;
    dex.overrides = relations
        .into_iter()
        .filter(|(a, b)| dex.symbols.contains_key(a) && dex.symbols.contains_key(b))
        .collect();
    Ok(dex)
}

#[derive(Default)]
struct Refs {
    uses: Vec<Reference>,
    definitions: Vec<Reference>,
}

fn take(
    record: &[String],
    roots: &[String],
    dex: &mut Dex,
    references: &mut Refs,
    relations: &mut Vec<(String, String)>,
) {
    match record.first().map(String::as_str) {
        Some("--- !Symbol") => {
            if let Some((id, symbol)) = symbol(record, roots) {
                dex.symbols.insert(id, symbol);
            }
        }
        Some("--- !Refs") => read_references(record, roots, references),
        Some("--- !Relations") => {
            if let Some(pair) = relation(record) {
                relations.push(pair);
            }
        }
        _ => {}
    }
}

/// The value of `Key: value`, without quotes.
fn value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim_start().strip_prefix(key)?.strip_prefix(':')?;
    Some(rest.trim().trim_matches('\''))
}

fn field<'a>(record: &'a [String], key: &str) -> Option<&'a str> {
    record
        .iter()
        .find_map(|line| (!line.starts_with(' ')).then(|| value(line, key)).flatten())
}

fn symbol(record: &[String], roots: &[String]) -> Option<(String, DexSymbol)> {
    let id = field(record, "ID")?.to_string();
    let definition = place_after(record, "Definition", roots);
    let declaration = place_after(record, "CanonicalDeclaration", roots);
    let local = match (&definition, &declaration) {
        (Some(definition), Some(declaration)) => {
            definition.path == declaration.path && !is_header(&definition.path)
        }
        _ => false,
    };
    let at = definition.or(declaration)?;
    let kind = record
        .iter()
        .find_map(|line| {
            line.starts_with("  ")
                .then(|| value(line, "Kind"))
                .flatten()
        })?
        .to_string();
    Some((
        id,
        DexSymbol {
            name: field(record, "Name")?.to_string(),
            scope: field(record, "Scope").unwrap_or("").to_string(),
            kind,
            at,
            local,
        },
    ))
}

fn is_header(path: &str) -> bool {
    Language::from_path(Path::new(path)) == Language::Cpp
        && matches!(
            Path::new(path).extension().and_then(|e| e.to_str()),
            Some("h" | "hpp" | "hh" | "hxx")
        )
}

/// The place in the block under this key, if its file is in the project.
fn place_after(record: &[String], key: &str, roots: &[String]) -> Option<Place> {
    let heading = format!("{key}:");
    let start = record.iter().position(|line| *line == heading)?;
    place(&record[start + 1..], roots)
}

/// The first `FileURI`, `Start` and `End` in these lines.
fn place(lines: &[String], roots: &[String]) -> Option<Place> {
    let path = lines.iter().find_map(|line| value(line, "FileURI"))?;
    let path = relative(path, roots)?;
    let numbers: Vec<u32> = lines
        .iter()
        .filter_map(|line| value(line, "Line").or_else(|| value(line, "Column")))
        .take(4)
        .filter_map(|number| number.parse().ok())
        .collect();
    match numbers[..] {
        [line, column, _, end_column] => Some(Place {
            path,
            line,
            column,
            end_column,
        }),
        _ => None,
    }
}

/// A file's path relative to the project, if it is in the project.
fn relative(uri: &str, roots: &[String]) -> Option<String> {
    let path = percent_decoded(uri.strip_prefix("file://")?);
    roots
        .iter()
        .find_map(|root| path.strip_prefix(root.as_str()))
        .map(str::to_string)
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = (bytes[at] == b'%')
            .then(|| bytes.get(at + 1..at + 3))
            .flatten()
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn read_references(record: &[String], roots: &[String], references: &mut Refs) {
    let Some(id) = field(record, "ID") else {
        return;
    };
    let starts: Vec<usize> = record
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("  - Kind:"))
        .map(|(at, _)| at)
        .collect();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(record.len());
        let kind: u32 = record[start]
            .trim_start_matches([' ', '-'])
            .strip_prefix("Kind:")
            .and_then(|kind| kind.trim().parse().ok())
            .unwrap_or(0);
        let list = if kind & REFERENCE != 0 {
            &mut references.uses
        } else if kind & DEFINITION != 0 {
            &mut references.definitions
        } else {
            continue;
        };
        if let Some(at) = place(&record[start + 1..end], roots) {
            list.push(Reference {
                symbol: id.to_string(),
                at,
            });
        }
    }
}

/// The pair of an override and what it overrides, of a `Relations` record of that kind.
fn relation(record: &[String]) -> Option<(String, String)> {
    let predicate: u32 = field(record, "Predicate")?.parse().ok()?;
    if predicate != OVERRIDDEN_BY {
        return None;
    }
    let mut ids = record
        .iter()
        .filter(|line| line.starts_with("  "))
        .filter_map(|line| value(line, "ID"));
    let overridden = ids.next()?.to_string();
    let overrider = ids.next()?.to_string();
    Some((overrider, overridden))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "--- !Symbol
ID:              AAAA
Name:            run
Scope:           'geo::Circle::'
SymInfo:
  Kind:            InstanceMethod
  Lang:            C
CanonicalDeclaration:
  FileURI:         'file:///p/a%2B%2B/s.h'
  Start:
    Line:            3
    Column:          7
  End:
    Line:            3
    Column:          10
Definition:
  FileURI:         'file:///p/a%2B%2B/s.cc'
  Start:
    Line:            8
    Column:          12
  End:
    Line:            8
    Column:          15
Flags:           9
...
--- !Symbol
ID:              '1234'
Name:            pure
Scope:           ''
SymInfo:
  Kind:            Function
  Lang:            C
CanonicalDeclaration:
  FileURI:         'file:///p/a%2B%2B/s.h'
  Start:
    Line:            1
    Column:          2
  End:
    Line:            1
    Column:          6
...
--- !Symbol
ID:              BBBB
Name:            printf
Scope:           ''
SymInfo:
  Kind:            Function
  Lang:            C
CanonicalDeclaration:
  FileURI:         'file:///usr/include/stdio.h'
  Start:
    Line:            1
    Column:          2
  End:
    Line:            1
    Column:          8
...
--- !Refs
ID:              AAAA
References:
  - Kind:            9
    Location:
      FileURI:         'file:///p/a%2B%2B/s.h'
      Start:
        Line:            3
        Column:          7
      End:
        Line:            3
        Column:          10
    Container:
      ID:              '0000000000000000'
  - Kind:            28
    Location:
      FileURI:         'file:///p/a%2B%2B/m.cc'
      Start:
        Line:            5
        Column:          4
      End:
        Line:            5
        Column:          7
    Container:
      ID:              CCCC
...
--- !Refs
ID:              BBBB
References:
  - Kind:            28
    Location:
      FileURI:         'file:///p/a%2B%2B/m.cc'
      Start:
        Line:            6
        Column:          4
      End:
        Line:            6
        Column:          10
...
--- !Relations
Subject:
  ID:              '1234'
Predicate:       1
Object:
  ID:              AAAA
...
--- !Relations
Subject:
  ID:              '1234'
Predicate:       0
Object:
  ID:              AAAA
...
";

    fn read_it() -> Dex {
        read(INDEX.as_bytes(), Path::new("/p/a++")).unwrap()
    }

    #[test]
    fn a_symbol_is_at_its_definition_or_else_its_declaration() {
        let dex = read_it();
        let run = &dex.symbols["AAAA"];
        assert_eq!(run.name, "run");
        assert_eq!(run.scope, "geo::Circle::");
        assert_eq!(run.kind, "InstanceMethod");
        assert_eq!(
            run.at,
            Place {
                path: "s.cc".to_string(),
                line: 8,
                column: 12,
                end_column: 15
            }
        );
        assert_eq!(dex.symbols["1234"].at.path, "s.h");
    }

    #[test]
    fn what_is_outside_the_project_goes() {
        let dex = read_it();
        assert!(!dex.symbols.contains_key("BBBB"));
        assert!(dex.references.iter().all(|r| r.symbol != "BBBB"));
    }

    #[test]
    fn only_uses_are_references() {
        let dex = read_it();
        assert_eq!(dex.references.len(), 1);
        assert_eq!(dex.references[0].at.path, "m.cc");
        assert_eq!(dex.references[0].at.line, 5);
    }

    #[test]
    fn a_symbol_defined_and_declared_in_one_source_file_is_local() {
        let local = |definition: &str, declaration: &str| {
            let mut text =
                String::from("--- !Symbol\nID: A\nName: f\nSymInfo:\n  Kind: Function\n");
            for (key, file) in [
                ("CanonicalDeclaration", declaration),
                ("Definition", definition),
            ] {
                text.push_str(&format!(
                    "{key}:\n  FileURI: 'file:///p/{file}'\n  Start:\n    Line: 1\n    Column: 2\n  End:\n    Line: 1\n    Column: 3\n"
                ));
            }
            read(text.as_bytes(), Path::new("/p")).unwrap().symbols["A"].local
        };
        assert!(local("a.c", "a.c"));
        assert!(!local("a.c", "a.h"));
        assert!(
            !local("a.h", "a.h"),
            "an inline function in a header has its uses recorded"
        );
    }

    #[test]
    fn a_definition_a_reference_names_is_kept() {
        let text = "--- !Symbol\nID: A\nName: main\nSymInfo:\n  Kind: Function\nDefinition:\n  FileURI: 'file:///p/a.c'\n  Start:\n    Line: 1\n    Column: 4\n  End:\n    Line: 1\n    Column: 8\n...\n--- !Refs\nID: A\nReferences:\n  - Kind: 10\n    Location:\n      FileURI: 'file:///p/b.c'\n      Start:\n        Line: 3\n        Column: 4\n      End:\n        Line: 3\n        Column: 8\n";
        let dex = read(text.as_bytes(), Path::new("/p")).unwrap();
        assert_eq!(dex.definitions.len(), 1);
        assert_eq!(dex.definitions[0].at.path, "b.c");
        assert!(dex.references.is_empty());
    }

    #[test]
    fn an_override_goes_with_what_it_overrides_and_no_other_relation_does() {
        assert_eq!(
            read_it().overrides,
            [("AAAA".to_string(), "1234".to_string())]
        );
    }
}
