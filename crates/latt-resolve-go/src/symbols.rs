//! What `scip-go`'s symbols hold that Lantern Lattice drops. The same symbol in another module's index has
//! another version: `example.com/a 1b5c3cabf341` where it is defined, `example.com/a .` where it is
//! used. Definitions are looked up across all indexes, so a version would keep them apart.

use latt_resolve::SemanticIndex;

/// Take the version out of every symbol of an index.
pub fn drop_versions(index: &mut SemanticIndex) {
    for occurrences in index.documents.values_mut() {
        for occurrence in occurrences {
            if let Some(without) = without_version(&occurrence.symbol) {
                occurrence.symbol = without;
            }
        }
    }
    for (method, declared) in &mut index.implementations {
        for symbol in [method, declared] {
            if let Some(without) = without_version(symbol) {
                *symbol = without;
            }
        }
    }
}

/// A symbol is `scheme manager package version descriptors`, with spaces, and a space inside a part is
/// written twice. `None` for a local, which has no version.
fn without_version(symbol: &str) -> Option<String> {
    if symbol.starts_with("local ") {
        return None;
    }
    let bytes = symbol.as_bytes();
    let (mut at, mut part, mut version_starts) = (0, 0, 0);
    while at < bytes.len() {
        if bytes[at] == b' ' {
            if bytes.get(at + 1) == Some(&b' ') {
                at += 2;
                continue;
            }
            part += 1;
            match part {
                3 => version_starts = at + 1,
                4 => {
                    return Some(format!("{}.{}", &symbol[..version_starts], &symbol[at..]));
                }
                _ => {}
            }
        }
        at += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_versions_of_a_symbol_become_one() {
        let defined = without_version("scip-go gomod example.com/a 1b5c3cabf341 `inner`/F().");
        let used = without_version("scip-go gomod example.com/a . `inner`/F().");
        assert_eq!(
            defined.as_deref(),
            Some("scip-go gomod example.com/a . `inner`/F().")
        );
        assert_eq!(defined, used);
    }

    #[test]
    fn a_space_written_twice_stays_in_its_part() {
        assert_eq!(
            without_version("scip-go gomod a  b v1 F().").as_deref(),
            Some("scip-go gomod a  b . F().")
        );
    }

    #[test]
    fn a_local_or_a_short_symbol_stays() {
        assert_eq!(without_version("local 3"), None);
        assert_eq!(without_version("a b c"), None);
    }
}
