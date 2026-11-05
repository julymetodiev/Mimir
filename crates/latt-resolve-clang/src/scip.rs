//! What `clangd-indexer` says of a project, as the fields of `scip.proto` that `decode` reads. A symbol
//! has its id, so that overloads stay apart, and the form that says what it is.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::dex::{Dex, DexSymbol, Place};

const ROLE_DEFINITION: u64 = 1;

/// The symbol of an index, in the form SCIP's symbols have: `().` a function, `.` a value or a field, `#`
/// a type. What it belongs to is in it, since an edited file finds its definitions by that. `None` for
/// what is no symbol of the graph, as a `using`.
pub fn symbol_of(id: &str, symbol: &DexSymbol) -> Option<String> {
    let name = symbol.name.split('<').next().unwrap_or(&symbol.name);
    let owner = symbol
        .scope
        .trim_end_matches("::")
        .rsplit("::")
        .next()
        .and_then(|last| last.split(['<', '(']).next())
        .filter(|last| !last.is_empty());
    let member = |suffix: &str| match owner {
        Some(owner) => format!("cxx {id}/{owner}#{name}{suffix}"),
        None => format!("cxx {id} {name}{suffix}"),
    };
    match symbol.kind.as_str() {
        "Function" | "Constructor" => Some(format!("cxx {id} {name}().")),
        "InstanceMethod" | "StaticMethod" | "Destructor" | "ConversionFunction" => {
            Some(member("()."))
        }
        "Field" | "StaticProperty" | "EnumConstant" => Some(member(".")),
        "Variable" => Some(format!("cxx {id} {name}.")),
        "Class" | "Struct" | "Union" | "Enum" | "TypeAlias" => Some(format!("cxx {id} {name}#")),
        _ => None,
    }
}

/// The index as a SCIP file, for the project at this absolute path.
pub fn encode(dex: &Dex, root: &Path) -> Vec<u8> {
    let symbols: HashMap<&str, String> = dex
        .symbols
        .iter()
        .filter_map(|(id, symbol)| Some((id.as_str(), symbol_of(id, symbol)?)))
        .collect();
    let mut documents: BTreeMap<&str, Document> = BTreeMap::new();
    // A symbol of several programs, as `main`, has its definition in each. A definition is said once,
    // however many units include it, as each counts when a name is looked up.
    let mut defined: HashSet<(&str, &Place)> = HashSet::new();
    let places = dex
        .symbols
        .iter()
        .map(|(id, symbol)| (id.as_str(), &symbol.at))
        .chain(dex.definitions.iter().map(|d| (d.symbol.as_str(), &d.at)));
    for (id, at) in places {
        if let Some(written) = symbols.get(id)
            && defined.insert((id, at))
        {
            documents.entry(&at.path).or_default().occurrences.push((
                at.clone(),
                written.clone(),
                ROLE_DEFINITION,
            ));
        }
    }
    for reference in dex.references.iter() {
        if let Some(written) = symbols.get(reference.symbol.as_str()) {
            documents
                .entry(&reference.at.path)
                .or_default()
                .occurrences
                .push((reference.at.clone(), written.clone(), 0));
        }
    }
    for (overrider, overridden) in &dex.overrides {
        if let (Some(symbol), Some(a), Some(b)) = (
            dex.symbols.get(overrider),
            symbols.get(overrider.as_str()),
            symbols.get(overridden.as_str()),
        ) {
            documents
                .entry(&symbol.at.path)
                .or_default()
                .overrides
                .push((a.clone(), b.clone()));
        }
    }
    let mut out = Vec::new();
    for (path, document) in documents {
        let lines = fs::read_to_string(root.join(path)).unwrap_or_default();
        let lines: Vec<&str> = lines.split('\n').collect();
        put_message(&mut out, 2, &document.encoded(path, &lines));
    }
    out
}

/// A column in UTF-16, from one in bytes.
pub fn utf16_column(line: &str, bytes: u32) -> u32 {
    let before = line.get(..bytes as usize).unwrap_or(line);
    before.encode_utf16().count() as u32
}

#[derive(Default)]
struct Document {
    occurrences: Vec<(Place, String, u64)>,
    overrides: Vec<(String, String)>,
}

impl Document {
    fn encoded(&self, path: &str, lines: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        put_message(&mut out, 1, path.as_bytes());
        for (place, symbol, roles) in &self.occurrences {
            // A file that cannot be read keeps its columns: they are right for most lines.
            let column = |line: u32, column| {
                lines
                    .get(line as usize)
                    .map_or(column, |text| byte_column(text, column))
            };
            // Three numbers if it ends on the line it starts on, as `scip.proto` says, or else four.
            let mut numbers = vec![place.line, column(place.line, place.column)];
            if place.end_line != place.line {
                numbers.push(place.end_line);
            }
            numbers.push(column(place.end_line, place.end_column));
            let mut range = Vec::new();
            for number in numbers {
                put_varint(&mut range, u64::from(number));
            }
            let mut occurrence = Vec::new();
            put_message(&mut occurrence, 1, &range);
            put_message(&mut occurrence, 2, symbol.as_bytes());
            if *roles != 0 {
                put_varint(&mut occurrence, 3 << 3);
                put_varint(&mut occurrence, *roles);
            }
            put_message(&mut out, 2, &occurrence);
        }
        for (overrider, overridden) in &self.overrides {
            let mut relationship = Vec::new();
            put_message(&mut relationship, 1, overridden.as_bytes());
            put_varint(&mut relationship, 3 << 3);
            put_varint(&mut relationship, 1);
            let mut information = Vec::new();
            put_message(&mut information, 1, overrider.as_bytes());
            put_message(&mut information, 4, &relationship);
            put_message(&mut out, 3, &information);
        }
        out
    }
}

/// A column in UTF-16, as `clangd` counts it, as a column in bytes, as an index here has it.
pub fn byte_column(line: &str, column: u32) -> u32 {
    let mut units = 0;
    for (at, c) in line.char_indices() {
        if units >= column {
            return at as u32;
        }
        units += c.len_utf16() as u32;
    }
    line.len() as u32
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// A field that holds bytes: a string, a message or packed numbers.
fn put_message(out: &mut Vec<u8>, number: u64, bytes: &[u8]) {
    put_varint(out, number << 3 | 2);
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::Reference;
    use latt_resolve_scip::decode;

    fn symbol(name: &str, scope: &str, kind: &str, path: &str, line: u32) -> DexSymbol {
        DexSymbol {
            name: name.to_string(),
            scope: scope.to_string(),
            kind: kind.to_string(),
            local: false,
            at: Place {
                path: path.to_string(),
                line,
                column: 0,
                end_line: line,
                end_column: name.len() as u32,
            },
        }
    }

    #[test]
    fn each_kind_has_its_form() {
        let of = |name, scope, kind| symbol_of("ID", &symbol(name, scope, kind, "a", 0));
        assert_eq!(of("f", "geo::", "Function").unwrap(), "cxx ID f().");
        assert_eq!(
            of("m", "geo::A::", "InstanceMethod").unwrap(),
            "cxx ID/A#m()."
        );
        assert_eq!(
            of("m", "Box<char>::", "InstanceMethod").unwrap(),
            "cxx ID/Box#m()."
        );
        assert_eq!(of("x", "A::", "Field").unwrap(), "cxx ID/A#x.");
        assert_eq!(
            of("RED", "color::", "EnumConstant").unwrap(),
            "cxx ID/color#RED."
        );
        assert_eq!(of("v", "", "Variable").unwrap(), "cxx ID v.");
        assert_eq!(of("T", "geo::", "Struct").unwrap(), "cxx ID T#");
        assert_eq!(of("A", "A::", "Constructor").unwrap(), "cxx ID A().");
        assert_eq!(of("Box<T>", "", "Class").unwrap(), "cxx ID Box#");
        assert_eq!(of("u", "", "Using"), None);
        assert_eq!(of("n", "", "Namespace"), None);
    }

    #[test]
    fn a_column_in_utf16_becomes_one_in_bytes() {
        assert_eq!(byte_column("abc", 2), 2);
        assert_eq!(byte_column("/* щ */ f", 8), 9);
        assert_eq!(byte_column("😀 f", 3), 5);
        assert_eq!(byte_column("ab", 9), 2);
    }

    #[test]
    fn a_definition_is_said_once_and_in_each_program() {
        let project = tempfile::tempdir().unwrap();
        let mut dex = Dex::default();
        let main = symbol("main", "", "Function", "a.c", 0);
        let at = |path: &str| Reference {
            symbol: "M".to_string(),
            at: Place {
                path: path.to_string(),
                line: 0,
                column: 0,
                end_line: 0,
                end_column: 4,
            },
        };
        dex.symbols.insert("M".to_string(), main);
        dex.definitions.extend([at("a.c"), at("b.c"), at("b.c")]);
        let index = decode(&encode(&dex, project.path())).unwrap();
        assert_eq!(index.documents[Path::new("a.c")].len(), 1);
        assert_eq!(index.documents[Path::new("b.c")].len(), 1);
    }

    #[test]
    fn what_is_encoded_is_what_is_decoded() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("a.c"), "int f(void);\nint g(void);\n").unwrap();
        let mut dex = Dex::default();
        dex.symbols
            .insert("F".to_string(), symbol("f", "", "Function", "a.c", 0));
        dex.symbols
            .insert("G".to_string(), symbol("g", "", "Function", "a.c", 1));
        dex.references.push(Reference {
            symbol: "F".to_string(),
            at: Place {
                path: "b.c".to_string(),
                line: 5,
                column: 4,
                end_line: 5,
                end_column: 5,
            },
        });
        dex.references.push(Reference {
            symbol: "G".to_string(),
            at: Place {
                path: "c.cc".to_string(),
                line: 3,
                column: 20,
                end_line: 7,
                end_column: 2,
            },
        });
        dex.overrides.push(("G".to_string(), "F".to_string()));

        let index = decode(&encode(&dex, project.path())).unwrap();
        let in_a = &index.documents[Path::new("a.c")];
        assert_eq!(in_a.len(), 2);
        assert!(in_a.iter().all(|o| o.roles == ROLE_DEFINITION));
        let in_b = &index.documents[Path::new("b.c")];
        assert_eq!(in_b.len(), 1);
        assert_eq!(in_b[0].symbol, "cxx F f().");
        assert_eq!(
            (in_b[0].line, in_b[0].column, in_b[0].end_column),
            (5, 4, 5)
        );
        assert_eq!(in_b[0].roles, 0);
        let over_lines = &index.documents[Path::new("c.cc")][0];
        assert_eq!(
            (
                over_lines.line,
                over_lines.column,
                over_lines.end_line,
                over_lines.end_column
            ),
            (3, 20, 7, 2)
        );
        assert_eq!(
            index.implementations,
            [("cxx G g().".to_string(), "cxx F f().".to_string())]
        );
    }
}
