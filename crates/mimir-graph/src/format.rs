//! What the file's format fixes: its version, and the numbers of each enum in it.

use crate::relations::EdgeKind;
use mimir_types::{Confidence, Language, SymbolKind, Visibility};

/// Another version of the format is treated as no file.
pub const VERSION: u32 = 2;

/// Another Mimir may read code otherwise, so its graph is treated as no file too.
pub const BUILT_BY: &str = env!("CARGO_PKG_VERSION");

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
