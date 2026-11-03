//! Typed channel wrappers for the actor system.

use crate::command::GraphCommand;
use tokio::sync::mpsc;

/// Sender for graph commands.
pub type CommandSender = mpsc::Sender<GraphCommand>;

/// Receiver for graph commands.
pub type CommandReceiver = mpsc::Receiver<GraphCommand>;

/// Create a new command channel with the specified buffer size.
pub fn command_channel(buffer: usize) -> (CommandSender, CommandReceiver) {
    mpsc::channel(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_command_channel() {
        let (tx, mut rx) = command_channel(16);

        tx.send(GraphCommand::Shutdown).await.unwrap();

        let cmd = rx.recv().await.unwrap();
        assert!(matches!(cmd, GraphCommand::Shutdown));
    }
}
