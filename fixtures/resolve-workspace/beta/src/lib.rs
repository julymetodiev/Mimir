use alpha::Named as Renamed;
use alpha::{Disk, Memory, Store};

/// Named like `helper` in alpha, so a lookup by name alone picks the wrong one.
pub fn helper() -> u32 {
    2
}

pub fn direct() -> u32 {
    helper()
}

pub fn imported() -> u32 {
    alpha::helper()
}

pub fn by_type() -> u32 {
    let memory = Memory::new();
    memory.get()
}

pub fn other_type() -> u32 {
    let disk = Disk::new();
    disk.get()
}

pub fn in_macro() -> String {
    let disk = Disk::new();
    format!("{}", disk.get())
}

pub fn through_trait(store: &dyn Store) -> u32 {
    store.load_twice()
}

pub fn turbofish() -> u32 {
    alpha::generic::<u32>()
}

pub fn as_value() -> Vec<u32> {
    // Passing a function is not calling it.
    vec![1].into_iter().map(|_| 0).chain(std::iter::once_with(helper)).collect()
}

pub fn method_as_value(disks: &[Disk]) -> Vec<u32> {
    disks.iter().map(Disk::get).collect()
}

pub fn trait_method_as_value(disks: &[Disk]) -> Vec<u32> {
    disks.iter().map(Store::load).collect()
}

pub fn dyn_method_as_value(stores: &[&dyn Store]) -> Vec<u32> {
    stores.iter().copied().map(Store::load).collect()
}

/// `alpha` has one `From` for `String`, but `std` has the one used here.
pub fn std_impl_as_value() -> Vec<String> {
    ["a"].map(String::from).to_vec()
}

pub fn std_impl_called() -> String {
    String::from("a")
}

pub fn operators(values: &[u32], first: &Disk, second: &Disk) -> bool {
    values[0] == 1 && first == second
}

pub struct Spec {
    pub helper: fn() -> u32,
}

/// The index names the field here, not the function.
pub fn shorthand() -> Spec {
    Spec { helper }
}

/// A shorthand for a local variable.
pub fn shorthand_local(helper: fn() -> u32) -> Spec {
    Spec { helper }
}

/// These run `fmt` and `from`, through `std`: not calls written here.
pub fn forwarding(disk: &Disk) -> String {
    let text = disk.to_string();
    let other: String = Disk::new().into();
    text + &other
}

pub struct Aliased;

/// The trait goes by another name here.
impl Renamed for Aliased {
    fn name(&self) -> u32 {
        1
    }
}

pub fn generic_as_value() -> Option<u32> {
    std::iter::once_with(alpha::generic::<u32>).next()
}

pub fn value_in_macro() -> Vec<fn() -> u32> {
    vec![helper]
}

/// Holds functions outside any function; two of them share a name.
pub const HANDLERS: &[fn() -> u32] = &[helper, alpha::helper];

/// Whoever uses `HANDLERS` can reach the functions in it.
pub fn run_handlers() -> u32 {
    HANDLERS.iter().map(|h| h()).sum()
}

pub fn handlers_in_macro() -> String {
    format!("{}", HANDLERS.len())
}

/// Holds the functions through another constant.
pub const NESTED: &[&[fn() -> u32]] = &[HANDLERS];

pub fn run_nested() -> usize {
    NESTED.len()
}

/// Holds no function, so using it says nothing about what runs.
pub const PLAIN: u32 = 1;

pub fn uses_plain() -> u32 {
    PLAIN
}

/// A parameter named like a function is not that function.
pub fn shadowed(helper: fn() -> u32) -> Option<u32> {
    std::iter::once_with(helper).next()
}

/// A project function named like the one in `std` is not it.
pub fn std_path_as_value(mut values: Vec<u32>) -> Vec<u32> {
    values.iter_mut().map(std::mem::take).collect()
}

/// The path in a `use` inside a function is not a use of the function.
pub fn local_use() -> u32 {
    use alpha::generic as renamed;
    renamed::<u32>()
}

pub fn doc_link() -> u32 {
    /// A link in a doc comment is not a use: [`helper`].
    fn nested() -> u32 {
        1
    }
    nested()
}

pub fn only_std() -> usize {
    "abc".len()
}

/// Named like `Store` in alpha, so a lookup by name alone cannot tell them apart.
pub mod local {
    pub trait Store {
        fn load(&self) -> u32;
    }

    pub struct Own;

    impl Store for Own {
        fn load(&self) -> u32 {
            3
        }
    }
}
