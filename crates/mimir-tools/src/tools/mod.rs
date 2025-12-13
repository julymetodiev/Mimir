//! Built-in tool implementations.

mod append;
mod edit;
mod edit_batch;
mod read;
mod search;
mod shell;
mod trace;
mod write;

pub use append::AppendTool;
pub use edit::EditTool;
pub use edit_batch::EditBatchTool;
pub use read::ReadTool;
pub use search::SearchTool;
pub use shell::ShellTool;
pub use trace::TraceTool;
pub use write::WriteTool;

use crate::ToolRegistry;

/// Create a registry with all built-in tools.
pub fn create_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(SearchTool);
    registry.register(EditTool);
    registry.register(EditBatchTool);
    registry.register(ShellTool);
    // ReadTool and AppendTool available but not in agent's default set
    registry
}
