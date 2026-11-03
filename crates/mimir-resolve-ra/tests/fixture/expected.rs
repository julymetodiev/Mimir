//! What the fixture is expected to hold, listed by hand from reading it, so that neither resolver
//! is graded by itself.

/// Every call in the fixture to a function in the fixture: `crate::owner::name` -> same.
pub const CALLS: &[(&str, &str)] = &[
    ("alpha::Store::load_twice", "alpha::Store::load"),
    ("alpha::Disk::get", "alpha::Disk::read"),
    ("alpha::Disk::read", "alpha::helper"),
    ("alpha::Disk::load", "alpha::Disk::new"),
    ("alpha::Disk::load", "alpha::Disk::get"),
    ("alpha::helper", "alpha::inner"),
    ("beta::direct", "beta::helper"),
    ("beta::imported", "alpha::helper"),
    ("beta::by_type", "alpha::Memory::new"),
    ("beta::by_type", "alpha::Memory::get"),
    ("beta::other_type", "alpha::Disk::new"),
    ("beta::other_type", "alpha::Disk::get"),
    ("beta::in_macro", "alpha::Disk::new"),
    ("beta::in_macro", "alpha::Disk::get"),
    ("beta::through_trait", "alpha::Store::load_twice"),
    ("beta::turbofish", "alpha::generic"),
    ("beta::local_use", "alpha::generic"),
    ("beta::doc_link", "beta::nested"),
    ("beta::forwarding", "alpha::Disk::new"),
    ("alpha::one_calls", "alpha::shared"),
    ("alpha::two_calls", "alpha::shared"),
];

/// Calls syntax leaves out: the receiver is not `self`, or the name is not unique.
pub const NEEDS_TYPES: &[(&str, &str)] = &[
    ("alpha::Store::load_twice", "alpha::Store::load"),
    ("beta::imported", "alpha::helper"),
    ("beta::by_type", "alpha::Memory::get"),
    ("beta::other_type", "alpha::Disk::get"),
    ("beta::in_macro", "alpha::Disk::get"),
    ("beta::through_trait", "alpha::Store::load_twice"),
    ("alpha::Disk::load", "alpha::Disk::get"),
    ("beta::local_use", "alpha::generic"),
];

/// Every method implementing a trait of the fixture.
pub const IMPLEMENTS: &[(&str, &str)] = &[
    ("alpha::Disk::load", "alpha::Store::load"),
    ("alpha::Memory::load", "alpha::Store::load"),
    ("alpha::Disk::name", "alpha::Named::name"),
    ("beta::Own::load", "beta::Store::load"),
    ("beta::Aliased::name", "alpha::Named::name"),
];

/// Those syntax links: `Store` is two traits, so it leaves their methods out.
pub const IMPLEMENTS_BY_SYNTAX: &[(&str, &str)] = &[("alpha::Disk::name", "alpha::Named::name")];

/// Every function named without being called, by the symbol that holds it.
/// Every read of a constant that holds no function.
pub const CONSTANT_READS: &[(&str, &str)] = &[
    ("beta::uses_plain", "beta::PLAIN"),
    ("beta::uses_own_limit", "beta::LIMIT"),
    ("beta::uses_other_limit", "alpha::LIMIT"),
    ("beta::uses_limit_in_macro", "beta::LIMIT"),
    ("beta::uses_limit_in_format_string", "beta::LIMIT"),
    ("beta::prints_shown", "beta::SHOWN"),
    ("alpha::uses_limit_here", "alpha::LIMIT"),
];

/// Every use of a type, trait, alias or variant of the fixture, written by name.
pub const TYPE_USES: &[(&str, &str)] = &[
    ("alpha::Alias", "alpha::Config"),
    ("alpha::Disk::eq", "alpha::Disk"),
    ("alpha::Disk::new", "alpha::Disk"),
    ("alpha::is_shape", "alpha::Kind"),
    ("alpha::is_shape", "alpha::Kind::Shape"),
    ("alpha::Memory::default", "alpha::Memory"),
    ("alpha::Memory::new", "alpha::Memory"),
    ("alpha::String::from", "alpha::Disk"),
    ("alpha::impl Config", "alpha::Config"),
    ("alpha::impl Default for Memory", "alpha::Memory"),
    ("alpha::impl Disk", "alpha::Disk"),
    ("alpha::impl From<Disk> for String", "alpha::Disk"),
    ("alpha::impl Memory", "alpha::Memory"),
    ("alpha::impl Named for Disk", "alpha::Disk"),
    ("alpha::impl Named for Disk", "alpha::Named"),
    ("alpha::impl PartialEq for Disk", "alpha::Disk"),
    ("alpha::impl Store for Disk", "alpha::Disk"),
    ("alpha::impl Store for Disk", "alpha::Store"),
    ("alpha::impl Store for Memory", "alpha::Memory"),
    ("alpha::impl Store for Memory", "alpha::Store"),
    ("alpha::impl std::fmt::Display for Disk", "alpha::Disk"),
    ("beta::Holder::inner", "alpha::Config"),
    ("beta::builds_other", "alpha::Other"),
    ("beta::by_type", "alpha::Memory"),
    ("beta::destructures", "alpha::Config"),
    ("beta::dyn_method_as_value", "alpha::Store"),
    ("beta::field_in_macro", "alpha::Config"),
    ("beta::forwarding", "alpha::Disk"),
    ("beta::impl Renamed for Aliased", "alpha::Named"),
    ("beta::impl Renamed for Aliased", "beta::Aliased"),
    ("beta::impl Store for Own", "beta::local::Own"),
    ("beta::impl Store for Own", "beta::local::Store"),
    ("beta::in_macro", "alpha::Disk"),
    ("beta::method_as_value", "alpha::Disk"),
    ("beta::operators", "alpha::Disk"),
    ("beta::other_type", "alpha::Disk"),
    ("beta::picks_mode", "alpha::Mode"),
    ("beta::picks_mode", "alpha::Mode::Fast"),
    ("beta::picks_mode", "alpha::Mode::Slow"),
    ("beta::reads_field", "alpha::Config"),
    ("beta::shorthand", "beta::Spec"),
    ("beta::shorthand_field", "alpha::Other"),
    ("beta::shorthand_local", "beta::Spec"),
    ("beta::through_alias", "alpha::Alias"),
    ("beta::through_trait", "alpha::Store"),
    ("beta::trait_method_as_value", "alpha::Disk"),
    ("beta::trait_method_as_value", "alpha::Store"),
];

/// Every use of a named field of the fixture.
pub const FIELD_USES: &[(&str, &str)] = &[
    ("alpha::Config::own_limit", "alpha::Config::limit"),
    ("beta::reads_field", "alpha::Config::limit"),
    ("beta::builds_other", "alpha::Other::limit"),
    ("beta::shorthand_field", "alpha::Other::limit"),
    ("beta::destructures", "alpha::Config::name"),
    ("beta::field_in_macro", "alpha::Config::name"),
    ("beta::through_alias", "alpha::Config::name"),
    ("beta::shorthand", "beta::Spec::helper"),
    ("beta::shorthand_local", "beta::Spec::helper"),
];

/// Those syntax links: a type of the same file, a variant of its enum, and `self.field`.
pub const DATA_USES_BY_SYNTAX: &[(&str, &str)] = &[
    ("alpha::Disk::new", "alpha::Disk"),
    ("alpha::Config::own_limit", "alpha::Config::limit"),
    ("alpha::impl Config", "alpha::Config"),
    ("beta::shorthand", "beta::Spec"),
    ("alpha::is_shape", "alpha::Kind::Shape"),
];

pub const VALUES: &[(&str, &str)] = &[
    ("beta::as_value", "beta::helper"),
    ("beta::method_as_value", "alpha::Disk::get"),
    ("beta::trait_method_as_value", "alpha::Disk::load"),
    ("beta::dyn_method_as_value", "alpha::Store::load"),
    ("beta::generic_as_value", "alpha::generic"),
    ("beta::shorthand", "beta::helper"),
    ("beta::value_in_macro", "beta::helper"),
    ("beta::HANDLERS", "beta::helper"),
    ("beta::HANDLERS", "alpha::helper"),
    ("beta::run_handlers", "beta::HANDLERS"),
    ("beta::handlers_in_macro", "beta::HANDLERS"),
    ("beta::NESTED", "beta::HANDLERS"),
    ("beta::run_nested", "beta::NESTED"),
];

/// Those syntax leaves out: macro tokens, a trait name used twice, a name used twice.
pub const VALUES_NEED_TYPES: &[(&str, &str)] = &[
    ("beta::value_in_macro", "beta::helper"),
    ("beta::trait_method_as_value", "alpha::Disk::load"),
    ("beta::dyn_method_as_value", "alpha::Store::load"),
    ("beta::HANDLERS", "alpha::helper"),
];

pub const SOURCES: [&str; 4] = [
    "alpha/src/lib.rs",
    "beta/src/lib.rs",
    "alpha/tests/one.rs",
    "alpha/tests/two.rs",
];
