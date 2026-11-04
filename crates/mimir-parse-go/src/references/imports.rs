//! What a file's imports bind, and where they lead.

use mimir_parse_core::extracted::{CallForm, ExtractedReference};
use mimir_parse_core::nodes::{range_of, span_of, text};
use mimir_types::{Confidence, ReferenceKind};
use tree_sitter::Node;

use crate::references::Walk;

/// The name a package is known by when its import does not give one: the last part of its path, or
/// the one before a major version. A package may declare another, and then its name is not found
/// here and it gets no syntax edge.
fn default_name(path: &str) -> &str {
    let mut parts = path.rsplit('/');
    let last = parts.next().unwrap_or(path);
    let is_version = last
        .strip_prefix('v')
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if is_version {
        parts.next().unwrap_or(last)
    } else {
        last
    }
}

impl<'a> Walk<'a> {
    pub fn imports(&mut self, root: &Node<'a>) {
        let mut cursor = root.walk();
        for declaration in root.named_children(&mut cursor) {
            if declaration.kind() != "import_declaration" {
                continue;
            }
            let mut cursor = declaration.walk();
            for spec in declaration.named_children(&mut cursor) {
                if spec.kind() == "import_spec" {
                    self.import(&spec);
                } else {
                    let mut cursor = spec.walk();
                    for spec in spec.named_children(&mut cursor) {
                        if spec.kind() == "import_spec" {
                            self.import(&spec);
                        }
                    }
                }
            }
        }
    }

    /// `import a "x/y"`: its name, and the path as context. An import that says its name is
    /// [`CallForm::Bound`], and one that does not must be checked against the package.
    fn import(&mut self, spec: &Node<'a>) {
        let Some(path) = spec
            .child_by_field_name("path")
            .and_then(|path| text(&path, self.source))
        else {
            return;
        };
        let path = path.trim_matches(['"', '`']);
        let alias = spec
            .child_by_field_name("name")
            .and_then(|name| text(&name, self.source));
        let name = alias.unwrap_or_else(|| default_name(path));
        let mut reference = ExtractedReference::new(
            name.to_string(),
            ReferenceKind::Import,
            range_of(spec),
            span_of(spec),
        )
        .with_confidence(Confidence::Low)
        .with_context(path.to_string());
        if alias.is_some() {
            reference = reference.with_call_form(CallForm::Bound);
        }
        self.found.push(reference);
        self.imported.insert(name.to_string());
    }
}
