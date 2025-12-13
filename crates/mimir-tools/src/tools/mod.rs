//! Built-in tool implementations.

mod read;
mod search;
mod trace;
mod edit;
mod edit_batch;
mod shell;

pub use read::ReadTool;
pub use search::SearchTool;
pub use trace::TraceTool;
pub use edit::EditTool;
pub use edit_batch::EditBatchTool;
pub use shell::ShellTool;

use crate::ToolRegistry;

/// Create a registry with all built-in tools.
pub fn create_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(ReadTool);
    registry.register(SearchTool);
    registry.register(TraceTool);
    registry.register(EditTool);
    registry.register(EditBatchTool);
    registry.register(ShellTool);
    registry
}
