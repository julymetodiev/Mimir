//! What `fixtures/go-workspace` must give, written by hand from its source so that no resolver is
//! graded by itself. Names are as a symbol's qualified name has them.

pub const SOURCES: [&str; 6] = [
    "shapes.go",
    "uses.go",
    "uses_test.go",
    "inner/inner.go",
    "win_windows.go",
    "tagged.go",
];

pub const CALLS: &[(&str, &str)] = &[
    ("Handler", "helper"),
    ("started", "helper"),
    ("direct", "helper"),
    ("qualified", "inner::Plain"),
    ("spawn", "helper"),
    ("generics", "Map"),
    ("generics", "Stack::Push"),
    // On an interface: the interface's method, and what implements it through `Implements`.
    ("onInterface", "Area::Area"),
    ("onConcrete", "Circle::Area"),
    ("onConcrete", "Square::Area"),
    // Through an embedded field: the method of the embedded type.
    ("throughEmbedded", "Circle::Area"),
    ("fromOtherPackage", "inner::Base::Hello"),
    ("typeSwitch", "Circle::Area"),
    ("embeddedInterface", "Named::Name"),
    ("callsVar", "Handler"),
    ("TestDirect", "direct"),
    ("inner::Base::Hello", "inner::Plain"),
    // Outside the build, by syntax.
    ("onWindows", "helper"),
    ("deployed", "helper"),
];

/// A name a function binds is no value and calling it is no call: `f := helper; f()`.
pub const VALUES: &[(&str, &str)] = &[
    ("table", "helper"),
    ("bound", "helper"),
    ("methodValues", "Circle::Area"),
    ("conversions", "helper"),
];

pub const IMPLEMENTS: &[(&str, &str)] = &[
    ("Circle::Area", "Area::Area"),
    ("Square::Area", "Area::Area"),
    ("Tag::Name", "Named::Name"),
    ("Tag::Size", "Both::Size"),
];

pub const CONSTANT_READS: &[(&str, &str)] =
    &[("reads", "Limit"), ("reads", "Count"), ("init", "Count")];

pub const TYPE_USES: &[(&str, &str)] = &[
    ("Both", "Named"),
    ("Circle::Area", "Circle"),
    ("Square::Area", "Square"),
    ("Tag::Name", "Tag"),
    ("Tag::Size", "Tag"),
    ("Tag::Circle", "Circle"),
    ("Outer::Base", "inner::Base"),
    ("Stack::Push", "Stack"),
    ("generics", "Stack"),
    ("onInterface", "Area"),
    ("onConcrete", "Circle"),
    ("onConcrete", "Square"),
    ("throughEmbedded", "Tag"),
    ("fromOtherPackage", "Outer"),
    ("typeSwitch", "Circle"),
    ("embeddedInterface", "Both"),
    ("methodValues", "Circle"),
    ("conversions", "Fn"),
    ("conversions", "MyInt"),
    ("fields", "Circle"),
    ("literal", "Circle"),
    ("notSpelled", "Tag"),
    ("inner::Base::Hello", "inner::Base"),
];

pub const FIELD_USES: &[(&str, &str)] = &[
    ("Circle::Area", "Circle::R"),
    ("Square::Area", "Square::S"),
    ("Tag::Name", "Tag::Label"),
    ("Tag::Size", "Tag::Label"),
    ("Stack::Push", "Stack::items"),
    ("fields", "Circle::R"),
    ("literal", "Circle::R"),
];

/// What syntax rules reach, with no index and no server: no methods, and the two values that are
/// plain names.
pub const CALLS_BY_SYNTAX: &[(&str, &str)] = &[
    ("Handler", "helper"),
    ("started", "helper"),
    ("direct", "helper"),
    ("qualified", "inner::Plain"),
    ("spawn", "helper"),
    ("generics", "Map"),
    ("callsVar", "Handler"),
    ("TestDirect", "direct"),
    ("inner::Base::Hello", "inner::Plain"),
    ("onWindows", "helper"),
    ("deployed", "helper"),
];

pub const VALUES_BY_SYNTAX: &[(&str, &str)] = &[
    ("table", "helper"),
    ("bound", "helper"),
    ("conversions", "helper"),
];
