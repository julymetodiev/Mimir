//! Typed channel wrappers for the actor system.

use crate::command::GraphCommand;
use crate::event::GraphEvent;
use tokio::sync::{broadcast, mpsc};

/// Sender for graph commands.
pub type CommandSender = mpsc::Sender<GraphCommand>;

/// Receiver for graph commands.
pub type CommandReceiver = mpsc::Receiver<GraphCommand>;

/// Sender for graph events.
pub type EventSender = broadcast::Sender<GraphEvent>;

/// Receiver for graph events.
pub type EventReceiver = broadcast::Receiver<GraphEvent>;

/// Create a new command channel with the specified buffer size.
pub fn command_channel(buffer: usize) -> (CommandSender, CommandReceiver) {
    mpsc::channel(buffer)
}

/// Create a new event broadcast channel with the specified capacity.
pub fn event_channel(capacity: usize) -> (EventSender, EventReceiver) {
    let (tx, rx) = broadcast::channel(capacity);
    (tx, rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_types::FileId;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_command_channel() {
        let (tx, mut rx) = command_channel(16);

        tx.send(GraphCommand::Shutdown).await.unwrap();

        let cmd = rx.recv().await.unwrap();
        assert!(matches!(cmd, GraphCommand::Shutdown));
    }

    #[tokio::test]
    async fn test_event_channel() {
        let (tx, mut rx) = event_channel(16);

        let event = GraphEvent::FileAdded {
            file_id: FileId::new(1),
            path: PathBuf::from("test.rs"),
            symbols_count: 5,
        };

        tx.send(event.clone()).unwrap();

        let received = rx.recv().await.unwrap();
        assert!(matches!(received, GraphEvent::FileAdded { .. }));
    }

    #[tokio::test]
    async fn test_event_broadcast() {
        let (tx, mut rx1) = event_channel(16);
        let mut rx2 = tx.subscribe();

        let event = GraphEvent::SyncStarted;
        tx.send(event).unwrap();

        let r1 = rx1.recv().await.unwrap();
        let r2 = rx2.recv().await.unwrap();

        assert!(matches!(r1, GraphEvent::SyncStarted));
        assert!(matches!(r2, GraphEvent::SyncStarted));
    }
}
