//! Built-in tool implementations.

mod append;
mod edit;
mod edit_batch;
mod read;
mod search;
mod shell;
mod trace;

pub use append::AppendTool;
pub use edit::EditTool;
pub use edit_batch::EditBatchTool;
pub use read::ReadTool;
pub use search::SearchTool;
pub use shell::ShellTool;
pub use trace::TraceTool;

use crate::ToolRegistry;

/// Create a registry with all built-in tools.
pub fn create_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(SearchTool);
    registry.register(ReadTool);
    registry.register(EditTool);
    registry.register(EditBatchTool);
    registry.register(AppendTool);
    registry.register(ShellTool);
    registry
}
