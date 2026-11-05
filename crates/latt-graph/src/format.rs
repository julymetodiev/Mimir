//! What the file's format fixes: its version, and the numbers of each enum in it.

use crate::relations::EdgeKind;
use latt_graph_fb as fb;
use latt_types::{Confidence, Language, SymbolKind, Visibility};
use std::collections::HashMap;

/// Another version of the format is treated as no file.
pub const VERSION: u32 = 3;

/// Another Lantern Lattice may read code otherwise, so its graph is treated as no file too.
pub const BUILT_BY: &str = env!("CARGO_PKG_VERSION");

/// One copy of each string in a file. The builder's own `create_shared_string` keeps a sorted vector
/// and shifts it on each new string, which at a hundred thousand symbols takes most of a write.
pub struct SharedStrings<'s, 'a> {
    offsets: HashMap<&'s str, fb::WIPOffset<&'a str>>,
}

impl<'s, 'a> SharedStrings<'s, 'a> {
    /// With no string yet.
    pub fn new() -> Self {
        Self {
            offsets: HashMap::new(),
        }
    }

    /// Where this string is, writing it the first time.
    pub fn get(
        &mut self,
        builder: &mut fb::FlatBufferBuilder<'a>,
        text: &'s str,
    ) -> fb::WIPOffset<&'a str> {
        *self
            .offsets
            .entry(text)
            .or_insert_with(|| builder.create_string(text))
    }
}

/// Numbers of an enum in the file, both ways from one list. A variant left out does not compile.
macro_rules! numbered {
    ($to:ident, $from:ident, $type:ident, [$($variant:ident = $number:literal),+ $(,)?]) => {
        pub fn $to(value: $type) -> u8 {
            match value { $($type::$variant => $number),+ }
        }
        pub fn $from(number: u8) -> Option<$type> {
            match number { $($number => Some($type::$variant),)+ _ => None }
        }
    };
}

numbered!(
    kind_number,
    kind_of,
    SymbolKind,
    [
        Module = 0,
        Class = 1,
        Interface = 2,
        Trait = 3,
        Namespace = 4,
        Package = 5,
        Function = 6,
        Method = 7,
        Constructor = 8,
        Closure = 9,
        Macro = 10,
        Struct = 11,
        Enum = 12,
        EnumVariant = 13,
        Union = 14,
        TypeAlias = 15,
        Field = 16,
        Property = 17,
        Constant = 18,
        Variable = 19,
        Parameter = 20,
        TypeParameter = 21,
        Impl = 22,
        Endpoint = 23,
        Test = 24,
    ]
);
numbered!(
    language_number,
    language_of,
    Language,
    [
        Rust = 0,
        TypeScript = 1,
        Tsx = 2,
        JavaScript = 3,
        Python = 4,
        Go = 5,
        Java = 6,
        Kotlin = 7,
        C = 8,
        Cpp = 9,
        Unknown = 10,
    ]
);
numbered!(
    visibility_number,
    visibility_of,
    Visibility,
    [
        Public = 0,
        Private = 1,
        Protected = 2,
        Internal = 3,
        Inherited = 4,
    ]
);
numbered!(
    edge_number,
    edge_of,
    EdgeKind,
    [
        Calls = 0,
        UsesAsValue = 1,
        UsesConstant = 2,
        UsesType = 3,
        UsesField = 4,
        Imports = 5,
        Implements = 6,
    ]
);
numbered!(
    confidence_number,
    confidence_of,
    Confidence,
    [Certain = 0, High = 1, Medium = 2, Low = 3,]
);
