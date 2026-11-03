//! The application layer: one actor owns the graph and is the only writer. It scans and watches the
//! project, resolves edges, answers queries and applies edits. [`start`] runs it for a project and
//! gives a [`GraphHandle`], a cheap handle to send it commands.

mod actor;
mod channel;
mod command;
mod edit;
mod error;
mod handle;
mod save;
mod scanner;
mod source;
mod start;
mod watcher;

pub use actor::GraphActor;
pub use channel::{CommandSender, command_channel};
pub use command::GraphCommand;
pub use error::GraphError;
pub use handle::GraphHandle;
pub use start::{Engine, start};
