//! The page of `latt view` (docs/ARCHITECTURE.md section 7): the graph, drawn and laid out on the
//! GPU. It reads and has no way to ask for a change. It runs only in a browser; what can be tested
//! without one is in `graph` and `layout`.

pub mod graph;
pub mod highlight;
pub mod layout;

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod events;
#[cfg(target_arch = "wasm32")]
mod gpu;
#[cfg(target_arch = "wasm32")]
mod ui;
