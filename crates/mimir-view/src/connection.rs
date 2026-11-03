//! One open page: its socket on one side, the daemon on the other.

use crate::ViewError;
use axum::extract::ws::{Message, WebSocket};
use mimir_client::{Client, ClientError};
use mimir_protocol::{ActivityKind, GraphEvent, GraphSnapshot, SnapshotEdgeKind};
use mimir_types::{SymbolId, SymbolKind};
use mimir_view_wire::{
    ClientMessage, EDGE_CALL, EDGE_IMPORT, EDGE_POSSIBLE, EDGE_USE, KIND_CONSTANT, KIND_FUNCTION,
    KIND_MEMBER, KIND_OTHER, KIND_TYPE, ServerMessage, Snapshot,
};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::warn;

/// How long to wait before looking for a daemon that went away.
const RETRY: std::time::Duration = std::time::Duration::from_secs(1);

fn kind_of(kind: SymbolKind) -> u8 {
    match kind {
        SymbolKind::Function
        | SymbolKind::Method
        | SymbolKind::Constructor
        | SymbolKind::Closure
        | SymbolKind::Macro => KIND_FUNCTION,
        SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Union
        | SymbolKind::Trait
        | SymbolKind::Interface
        | SymbolKind::Class
        | SymbolKind::Impl
        | SymbolKind::TypeAlias => KIND_TYPE,
        SymbolKind::Field | SymbolKind::Property | SymbolKind::EnumVariant => KIND_MEMBER,
        SymbolKind::Constant | SymbolKind::Variable => KIND_CONSTANT,
        _ => KIND_OTHER,
    }
}

fn edge_of(kind: SnapshotEdgeKind) -> u32 {
    match kind {
        SnapshotEdgeKind::Call => EDGE_CALL,
        SnapshotEdgeKind::Possible => EDGE_POSSIBLE,
        SnapshotEdgeKind::Use => EDGE_USE,
        SnapshotEdgeKind::Import => EDGE_IMPORT,
    }
}

pub fn to_wire(snapshot: GraphSnapshot) -> ServerMessage {
    let mut wire = Snapshot {
        generation: snapshot.generation,
        provisional: snapshot.provisional,
        files: snapshot
            .files
            .iter()
            .map(|file| file.display().to_string())
            .collect(),
        edges: snapshot
            .edges
            .iter()
            .map(|edge| [edge.from, edge.to, edge_of(edge.kind)])
            .collect(),
        ..Snapshot::default()
    };
    for symbol in snapshot.symbols {
        wire.ids.push(symbol.id.as_u64());
        wire.kinds.push(kind_of(symbol.kind));
        wire.symbol_files.push(symbol.file);
        wire.lines.push(symbol.line);
        wire.names.push(symbol.name);
    }
    ServerMessage::Snapshot(wire)
}

/// Read events into a channel, which unlike the stream can be waited on next to the socket. A daemon
/// that goes away is looked for again, and a new one is announced as a change.
async fn read_events(root: PathBuf, events: mpsc::Sender<GraphEvent>) {
    let mut first = true;
    loop {
        match watch(&root).await {
            Ok(mut stream) => {
                if !first && events.send(GraphEvent::Lagged).await.is_err() {
                    return;
                }
                first = false;
                loop {
                    let Ok(event) = stream.next().await else {
                        break;
                    };
                    if events.send(event).await.is_err() {
                        return;
                    }
                }
            }
            Err(e) => warn!("Cannot watch the graph: {e}"),
        }
        if events.is_closed() {
            return;
        }
        tokio::time::sleep(RETRY).await;
    }
}

async fn watch(root: &Path) -> Result<mimir_client::EventStream, ClientError> {
    Client::open(root).await?.watch().await
}

/// Answer a page until it goes away.
pub async fn serve(mut socket: WebSocket, root: PathBuf) {
    if let Err(e) = answer(&mut socket, root).await {
        warn!("A view's connection ended: {e}");
    }
}

async fn send(socket: &mut WebSocket, message: ServerMessage) -> Result<(), axum::Error> {
    socket.send(Message::Binary(message.encode().into())).await
}

async fn answer(socket: &mut WebSocket, root: PathBuf) -> Result<(), ViewError> {
    let (told, mut events) = mpsc::channel(64);
    // Watching starts before the first snapshot, so nothing between them is missed.
    let reader = tokio::spawn(read_events(root.clone(), told));
    let mut client = Client::open(&root).await?;
    send(socket, to_wire(client.snapshot().await?)).await?;
    let result = loop {
        tokio::select! {
            asked = socket.recv() => match asked {
                Some(Ok(Message::Binary(bytes))) => {
                    if let Ok(ClientMessage::Source { id }) = ClientMessage::decode(&bytes) {
                        match client.source(SymbolId::new(id), None).await {
                            Ok(source) => {
                                let answer = ServerMessage::Source {
                                    id,
                                    first_line: source.first_line,
                                    text: source.lines.join("\n"),
                                };
                                if let Err(e) = send(socket, answer).await {
                                    break Err(e.into());
                                }
                            }
                            // A symbol that an edit has since replaced has no source any more.
                            Err(e) => warn!("No source for a symbol: {e}"),
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break Ok(()),
                Some(Ok(_)) => {}
                Some(Err(e)) => break Err(e.into()),
            },
            event = events.recv() => {
                let Some(event) = event else { break Ok(()) };
                let message = match event {
                    GraphEvent::Activity { symbols, kind } => ServerMessage::Activity {
                        symbols: symbols.iter().map(|id| id.as_u64()).collect(),
                        edit: kind == ActivityKind::Edit,
                    },
                    GraphEvent::GraphChanged { .. } | GraphEvent::Lagged => {
                        match client.snapshot().await {
                            Ok(snapshot) => to_wire(snapshot),
                            Err(e) => {
                                warn!("No snapshot: {e}");
                                continue;
                            }
                        }
                    }
                };
                if let Err(e) = send(socket, message).await {
                    break Err(e.into());
                }
            }
        }
    };
    reader.abort();
    result
}
