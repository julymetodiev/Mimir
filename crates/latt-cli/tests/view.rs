//! `latt view` from end to end: the socket passes the graph on, sees a change, and refuses strangers.

mod common;

use assert_cmd::cargo::cargo_bin;
use futures_util::{SinkExt, StreamExt};
use latt_view_wire::{ClientMessage, ServerMessage, Snapshot, decode_sources};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{self, Message};

/// The view's process, stopped when the test ends however it ends.
struct View(Child);

impl Drop for View {
    fn drop(&mut self) {
        if self.0.kill().is_err() {
            eprintln!("the view had already stopped");
        }
        if self.0.wait().is_err() {
            eprintln!("the view could not be waited for");
        }
    }
}

fn start_view(dir: &std::path::Path) -> (View, String) {
    let mut child = Command::new(cargo_bin("latt"))
        .args(["view", "--no-open"])
        .current_dir(dir)
        .env("LATT_NO_SCIP", "1")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    stdout.read_line(&mut line).unwrap();
    // Kept open: closed, the view's next line of output would end it.
    child.stdout = Some(stdout.into_inner());
    let address = line
        .trim()
        .strip_prefix("Lantern Lattice view: ")
        .unwrap()
        .to_string();
    (View(child), address)
}

fn request(address: &str, origin: &str) -> tungstenite::handshake::client::Request {
    use tungstenite::client::IntoClientRequest;
    let socket = format!("{}socket", address.replacen("http://", "ws://", 1));
    let mut request = socket.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Origin", origin.parse().unwrap());
    request
}

async fn next_message<S>(socket: &mut S) -> ServerMessage
where
    S: StreamExt<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    loop {
        let message = tokio::time::timeout(Duration::from_secs(30), socket.next())
            .await
            .expect("nothing came")
            .unwrap()
            .unwrap();
        if let Message::Binary(bytes) = message {
            return ServerMessage::decode(&bytes).unwrap();
        }
    }
}

fn named<'a>(snapshot: &'a Snapshot, name: &str) -> Option<&'a u64> {
    let place = snapshot.names.iter().position(|n| n == name)?;
    snapshot.ids.get(place)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_the_socket_passes_on_the_graph_and_its_changes() {
    let dir = common::project();
    let (_view, address) = start_view(dir.path());
    let origin = address
        .trim_end_matches('/')
        .rsplit_once('/')
        .unwrap()
        .0
        .to_string();

    let (mut socket, _) = tokio_tungstenite::connect_async(request(&address, &origin))
        .await
        .unwrap();
    let ServerMessage::Snapshot(first) = next_message(&mut socket).await else {
        panic!("the first message is not a snapshot");
    };
    let helper = *named(&first, "helper").expect("helper is in the graph");
    assert!(!first.edges.is_empty());

    // The page may ask for a symbol's source.
    let asked = ClientMessage::Source { id: helper }.encode();
    socket.send(Message::Binary(asked.into())).await.unwrap();
    loop {
        if let ServerMessage::Source { id, text, .. } = next_message(&mut socket).await {
            assert_eq!(id, helper);
            assert!(text.contains("fn helper"), "{text}");
            break;
        }
    }

    // A change on disk reaches the page without its asking.
    let lib = dir.path().join("src/lib.rs");
    let text = std::fs::read_to_string(&lib).unwrap();
    std::fs::write(&lib, format!("{text}\npub fn added_later() {{}}\n")).unwrap();
    loop {
        if let ServerMessage::Snapshot(snapshot) = next_message(&mut socket).await
            && named(&snapshot, "added_later").is_some()
        {
            break;
        }
    }
    common::stop(dir.path());
}

/// An export holds all a static host needs, and a page that knows it has no server.
#[test]
fn test_an_export_writes_the_page_the_graph_and_the_source() {
    let dir = common::project();
    let out = dir.path().join("site");
    common::latt(dir.path())
        .args(["view", "--export"])
        .arg(&out)
        .assert()
        .success();

    let index = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(index.contains("latt-static"));
    assert!(out.join("latt_view_page.js").is_file());
    assert!(out.join("latt_view_page_bg.wasm").is_file());
    let ServerMessage::Snapshot(graph) =
        ServerMessage::decode(&std::fs::read(out.join("graph.bin")).unwrap()).unwrap()
    else {
        panic!("graph.bin is not a snapshot");
    };
    let helper = *named(&graph, "helper").unwrap();
    let sources = decode_sources(&std::fs::read(out.join("sources.bin")).unwrap()).unwrap();
    let (_, _, text) = sources.iter().find(|(id, _, _)| *id == helper).unwrap();
    assert!(text.contains("fn helper"), "{text}");
    common::stop(dir.path());
}

/// Any site open in the browser can try: only our own page gets the socket.
#[tokio::test(flavor = "multi_thread")]
async fn test_a_stranger_gets_no_socket() {
    let dir = common::project();
    let (_view, address) = start_view(dir.path());

    let from_a_site = request(&address, "https://evil.example");
    assert!(tokio_tungstenite::connect_async(from_a_site).await.is_err());

    let origin = address
        .trim_end_matches('/')
        .rsplit_once('/')
        .unwrap()
        .0
        .to_string();
    let wrong_token = format!("{origin}/wrong/");
    assert!(
        tokio_tungstenite::connect_async(request(&wrong_token, &origin))
            .await
            .is_err()
    );
    common::stop(dir.path());
}
