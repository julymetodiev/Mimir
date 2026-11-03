pub trait Store {
    fn load(&self) -> u32;
    fn load_twice(&self) -> u32 {
        self.load() + self.load()
    }
}

pub struct Disk;

impl Disk {
    pub fn new() -> Self {
        Disk
    }
    pub fn get(&self) -> u32 {
        self.read()
    }
    fn read(&self) -> u32 {
        helper()
    }
}

impl Store for Disk {
    fn load(&self) -> u32 {
        Self::new().get()
    }
}

pub struct Memory;

impl Memory {
    pub fn new() -> Self {
        Memory
    }
    pub fn get(&self) -> u32 {
        7
    }
}

impl Store for Memory {
    fn load(&self) -> u32 {
        0
    }
}

/// From outside the project: its method has nothing here to link to.
impl Default for Memory {
    fn default() -> Self {
        Memory
    }
}

/// The only trait of this name, so syntax can link its implementation.
pub trait Named {
    fn name(&self) -> u32;
}

impl Named for Disk {
    fn name(&self) -> u32 {
        9
    }
}

pub fn helper() -> u32 {
    fn inner() -> u32 {
        1
    }
    inner()
}

pub fn generic<T: Default>() -> T {
    T::default()
}

/// Named like `std::mem::take`, the only function of this name in the project.
pub fn take() -> u32 {
    4
}

impl From<Disk> for String {
    fn from(_: Disk) -> String {
        String::new()
    }
}

/// An operator is not a function named as a value.
impl PartialEq for Disk {
    fn eq(&self, _: &Disk) -> bool {
        true
    }
}

impl std::fmt::Display for Disk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "disk")
    }
}

/// Shares its name with a constant in another crate.
pub const LIMIT: u32 = 2;

pub fn uses_limit_here() -> u32 {
    LIMIT
}

/// Two structs share a field's name, so a lookup by name alone cannot tell them apart.
pub struct Config {
    pub limit: u32,
    pub name: u32,
}

pub struct Other {
    pub limit: u32,
}

impl Config {
    pub fn own_limit(&self) -> u32 {
        self.limit
    }
}

pub enum Mode {
    Fast,
    Slow(u32),
}

pub type Alias = Config;

/// A variant named like a struct, written among a macro's tokens.
pub struct Shape;

pub enum Kind {
    Shape,
}

pub fn is_shape(kind: Kind) -> bool {
    matches!(kind, Kind::Shape)
}
