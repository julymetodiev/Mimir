//! The forms that bring names in or name them in their own way: imports and classes.

use mimir_parse_core::extracted::CallForm;
use mimir_parse_core::nodes::text;
use mimir_types::ReferenceKind;
use tree_sitter::Node;

use crate::references::Walk;

impl<'a> Walk<'a> {
    /// `import a.b` and `import a.b as c`: the name of the module, which binds no function.
    pub fn import(&mut self, node: &Node<'a>) {
        let mut cursor = node.walk();
        let names: Vec<_> = node.children_by_field_name("name", &mut cursor).collect();
        for name in names {
            let dotted = if name.kind() == "aliased_import" {
                name.child_by_field_name("name")
            } else {
                Some(name)
            };
            let Some(dotted) = dotted else {
                continue;
            };
            let module = text(&dotted, self.source);
            self.add(
                ReferenceKind::Import,
                &dotted,
                Some(CallForm::Bound),
                module,
            );
        }
    }

    /// `from a import b as c`. The context is the module as it is written, and `b` may be a module
    /// itself: a second reference says so, with a name no code can call.
    pub fn import_from(&mut self, node: &Node<'a>) {
        let Some(module) = node
            .child_by_field_name("module_name")
            .and_then(|module| text(&module, self.source))
        else {
            return;
        };
        let mut cursor = node.walk();
        let names: Vec<_> = node.children_by_field_name("name", &mut cursor).collect();
        for name in names {
            let (dotted, form) = if name.kind() == "aliased_import" {
                (name.child_by_field_name("name"), Some(CallForm::Bound))
            } else {
                (Some(name), None)
            };
            let Some(dotted) = dotted else {
                continue;
            };
            self.add(ReferenceKind::Import, &dotted, form, Some(module));
            if let Some(imported) = text(&dotted, self.source) {
                let joined = if module.ends_with('.') {
                    format!("{module}{imported}")
                } else {
                    format!("{module}.{imported}")
                };
                let mut as_module = self.found.last().cloned();
                if let Some(reference) = as_module.as_mut() {
                    reference.name = joined.clone();
                    reference.context = Some(joined);
                    reference.call_form = Some(CallForm::Bound);
                }
                self.found.extend(as_module);
            }
        }
    }

    /// `class A(B, metaclass=M)`: the context says it inherits, for the edge `Implements`.
    pub fn class(&mut self, node: &Node<'a>, depth: usize) {
        if let Some(bases) = node.child_by_field_name("superclasses") {
            let mut cursor = bases.walk();
            for base in bases.named_children(&mut cursor) {
                match base.kind() {
                    "identifier" => {
                        self.add(ReferenceKind::TypeUse, &base, None, Some("extends"));
                    }
                    "attribute" => {
                        if let Some(attribute) = base.child_by_field_name("attribute") {
                            self.add(ReferenceKind::TypeUse, &attribute, None, Some("extends"));
                        }
                    }
                    // `Generic[T]`, `Base[int]`.
                    "subscript" => {
                        if let Some(value) = base.child_by_field_name("value") {
                            match value.kind() {
                                "identifier" => {
                                    self.add(ReferenceKind::TypeUse, &value, None, Some("extends"))
                                }
                                _ => self.types(&value, depth + 1),
                            }
                        }
                        let mut inner = base.walk();
                        for argument in base.children_by_field_name("subscript", &mut inner) {
                            self.types(&argument, depth + 1);
                        }
                    }
                    _ => self.visit(&base, depth + 1),
                }
            }
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.visit(&body, depth + 1);
        }
    }
}
