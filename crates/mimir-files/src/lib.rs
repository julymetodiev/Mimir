//! File scanning for the Mimir semantic code intelligence engine.
//!
//! Provides gitignore-aware directory scanning and language detection.

mod ignore;
mod language;
mod scanner;

pub use scanner::{ScannedFile, Scanner};
