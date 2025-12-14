//! Built-in tool implementations.

mod apply_patch;
mod edit;
mod read;
mod search;
mod shell;
mod trace;
mod inspect;
mod write;

pub use apply_patch::ApplyPatchTool;
pub use edit::EditTool;
pub use read::ReadTool;
pub use search::SearchTool;
pub use shell::ShellTool;
pub use trace::TraceTool;
pub use inspect::InspectTool;
pub use write::WriteTool;

use crate::ToolRegistry;

/// Create a registry with all built-in tools.
pub fn create_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(SearchTool);
    registry.register(ReadTool);
    registry.register(EditTool);  // Primary edit tool (string replacement)
    registry.register(WriteTool);
    registry.register(ShellTool);
    registry.register(TraceTool);
    registry
}
