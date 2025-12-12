//! Tree-sitter queries for Rust.

/// Query for extracting function definitions.
pub const FUNCTION_QUERY: &str = r#"
(function_item
  name: (identifier) @name
  parameters: (parameters) @params
  return_type: (type_identifier)? @return_type
) @function
"#;

/// Query for extracting struct definitions.
pub const STRUCT_QUERY: &str = r#"
(struct_item
  name: (type_identifier) @name
) @struct
"#;

/// Query for extracting enum definitions.
pub const ENUM_QUERY: &str = r#"
(enum_item
  name: (type_identifier) @name
) @enum
"#;

/// Query for extracting trait definitions.
pub const TRAIT_QUERY: &str = r#"
(trait_item
  name: (type_identifier) @name
) @trait
"#;

/// Query for extracting impl blocks.
pub const IMPL_QUERY: &str = r#"
(impl_item
  type: (type_identifier) @type
  trait: (type_identifier)? @trait
) @impl
"#;

/// Query for extracting module definitions.
pub const MODULE_QUERY: &str = r#"
(mod_item
  name: (identifier) @name
) @module
"#;

/// Query for extracting function calls.
pub const CALL_QUERY: &str = r#"
(call_expression
  function: [
    (identifier) @name
    (field_expression
      field: (field_identifier) @name
    )
    (scoped_identifier
      name: (identifier) @name
    )
  ]
) @call
"#;

/// Query for extracting use statements.
pub const USE_QUERY: &str = r#"
(use_declaration
  argument: [
    (use_wildcard) @wildcard
    (scoped_identifier) @path
    (identifier) @name
    (use_as_clause
      path: [
        (scoped_identifier) @path
        (identifier) @name
      ]
    )
    (use_list) @list
    (scoped_use_list
      path: (scoped_identifier)? @path
      list: (use_list) @list
    )
  ]
) @use
"#;

/// Query for extracting type annotations.
pub const TYPE_QUERY: &str = r#"
(type_identifier) @type_use
"#;

/// Combined query for all symbols.
pub fn symbols_query() -> String {
    format!(
        "{}{}{}{}{}{}",
        FUNCTION_QUERY, STRUCT_QUERY, ENUM_QUERY, TRAIT_QUERY, IMPL_QUERY, MODULE_QUERY
    )
}

/// Combined query for all references.
pub fn references_query() -> String {
    format!("{}{}", CALL_QUERY, USE_QUERY)
}
