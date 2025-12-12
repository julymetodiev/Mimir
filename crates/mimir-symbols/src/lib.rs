//! Symbol table for the Mimir code graph.
//!
//! This crate provides the symbol storage and indexing system:
//! - `SymbolTable`: The main storage for symbols
//! - `SymbolsHandle`: Thread-safe handle for accessing symbols
//! - Name and file indices for efficient lookups

mod handle;
mod index;
mod table;

pub use handle::SymbolsHandle;
pub use table::SymbolTable;
