//! `mimir view`: a browser on the graph (docs/ARCHITECTURE.md section 7). This serves the built page
//! and passes snapshots and events on. It holds no logic and has no way to change anything.

mod connection;

use axum::Router;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use mimir_client::{Client, ClientError};
use mimir_view_wire::encode_sources;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::net::TcpListener;

const INDEX: &str = include_str!("../page/index.html");
const SCRIPT: &str = include_str!("../page/mimir_view_page.js");
const PROGRAM: &[u8] = include_bytes!("../page/mimir_view_page_bg.wasm");

/// What can go wrong serving a view.
#[derive(Debug, Error)]
pub enum ViewError {
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error("cannot listen on a local port: {0}")]
    Listen(std::io::Error),
    #[error("cannot make a token: {0}")]
    Token(getrandom::Error),
    #[error("the web server stopped: {0}")]
    Serve(std::io::Error),
    #[error("a page's socket failed: {0}")]
    Socket(#[from] axum::Error),
}

/// What every request is checked against.
struct Access {
    token: String,
    host: String,
    origin: String,
    root: PathBuf,
}

impl Access {
    /// Any site open in the browser can reach a local port. The token keeps it from reading, and
    /// the host keeps it from getting here under a name of its own.
    fn allows(&self, token: &str, headers: &HeaderMap) -> bool {
        token == self.token
            && headers
                .get(header::HOST)
                .is_some_and(|host| host == self.host.as_str())
    }

    /// A socket can be opened from any page, so it must also come from this one.
    fn allows_socket(&self, token: &str, headers: &HeaderMap) -> bool {
        self.allows(token, headers)
            && headers
                .get(header::ORIGIN)
                .is_some_and(|origin| origin == self.origin.as_str())
    }
}

fn new_token() -> Result<String, ViewError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(ViewError::Token)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn router(access: Access) -> Router {
    Router::new()
        .route("/{token}/", get(index))
        .route("/{token}/mimir_view_page.js", get(script))
        .route("/{token}/mimir_view_page_bg.wasm", get(program))
        .route("/{token}/socket", get(socket))
        .with_state(Arc::new(access))
}

fn refused() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

async fn index(
    State(access): State<Arc<Access>>,
    UrlPath(token): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    if !access.allows(&token, &headers) {
        return refused();
    }
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], INDEX).into_response()
}

async fn script(
    State(access): State<Arc<Access>>,
    UrlPath(token): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    if !access.allows(&token, &headers) {
        return refused();
    }
    ([(header::CONTENT_TYPE, "text/javascript")], SCRIPT).into_response()
}

async fn program(
    State(access): State<Arc<Access>>,
    UrlPath(token): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    if !access.allows(&token, &headers) {
        return refused();
    }
    ([(header::CONTENT_TYPE, "application/wasm")], PROGRAM).into_response()
}

async fn socket(
    State(access): State<Arc<Access>>,
    UrlPath(token): UrlPath<String>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !access.allows_socket(&token, &headers) {
        return refused();
    }
    let root = access.root.clone();
    upgrade.on_upgrade(move |socket| connection::serve(socket, root))
}

/// Bind a local port and say where the view is.
async fn listen(root: PathBuf) -> Result<(TcpListener, Router, String), ViewError> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .map_err(ViewError::Listen)?;
    let port = listener.local_addr().map_err(ViewError::Listen)?.port();
    let token = new_token()?;
    let address = format!("http://127.0.0.1:{port}/{token}/");
    let access = Access {
        host: format!("127.0.0.1:{port}"),
        origin: format!("http://127.0.0.1:{port}"),
        token,
        root,
    };
    Ok((listener, router(access), address))
}

/// Say in the page that it is one with no server. The page reads this when it starts.
fn static_index() -> String {
    INDEX.replacen(
        "<head>",
        "<head>\n<meta name=\"mimir-static\" content=\"true\">",
        1,
    )
}

/// Write the view of the project at or above `from` as files for a static host. Waits until the
/// graph is complete, since nothing will update it. Says how many symbols it holds.
pub async fn export(from: &Path, directory: &Path) -> Result<usize, ViewError> {
    let mut client = Client::open(from).await?;
    client.sync(false).await?;
    let snapshot = client.snapshot().await?;
    let mut sources = Vec::with_capacity(snapshot.symbols.len());
    for symbol in &snapshot.symbols {
        let source = client.source(symbol.id, None).await?;
        sources.push((
            symbol.id.as_u64(),
            source.first_line,
            source.lines.join("\n"),
        ));
    }
    let count = snapshot.symbols.len();
    let graph = connection::to_wire(snapshot).encode();
    let sources = encode_sources(&sources);
    let index = static_index();
    mimir_client::write_files(
        directory,
        &[
            ("index.html", index.as_bytes()),
            ("mimir_view_page.js", SCRIPT.as_bytes()),
            ("mimir_view_page_bg.wasm", PROGRAM),
            ("graph.bin", &graph),
            ("sources.bin", &sources),
        ],
    )?;
    Ok(count)
}

/// Serve a view of the project at or above `from`, until stopped.
pub async fn serve(from: &Path, open: bool) -> Result<(), ViewError> {
    let root = Client::open(from).await?.root().to_path_buf();
    let (listener, app, address) = listen(root).await?;
    println!("Mimir view: {address}");
    println!("Press Ctrl-C to stop.");
    if open {
        mimir_client::open_in_browser(&address)?;
    }
    axum::serve(listener, app).await.map_err(ViewError::Serve)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn access() -> Access {
        Access {
            token: "secret".to_string(),
            host: "127.0.0.1:9".to_string(),
            origin: "http://127.0.0.1:9".to_string(),
            root: PathBuf::new(),
        }
    }

    fn headers(host: &'static str, origin: Option<&'static str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static(host));
        if let Some(origin) = origin {
            headers.insert(header::ORIGIN, HeaderValue::from_static(origin));
        }
        headers
    }

    #[test]
    fn test_a_request_needs_the_token_and_our_own_host() {
        let access = access();
        let own = headers("127.0.0.1:9", None);
        assert!(access.allows("secret", &own));
        assert!(!access.allows("guess", &own));
        // A site pointing its own name at this machine.
        assert!(!access.allows("secret", &headers("evil.example:9", None)));
        assert!(!access.allows("secret", &HeaderMap::new()));
    }

    #[test]
    fn test_a_socket_must_come_from_our_own_page() {
        let access = access();
        let ours = headers("127.0.0.1:9", Some("http://127.0.0.1:9"));
        assert!(access.allows_socket("secret", &ours));
        let other = headers("127.0.0.1:9", Some("https://evil.example"));
        assert!(!access.allows_socket("secret", &other));
        assert!(!access.allows_socket("secret", &headers("127.0.0.1:9", None)));
    }

    #[test]
    fn test_a_static_page_says_so() {
        assert!(!INDEX.contains("mimir-static"));
        assert!(static_index().contains("<meta name=\"mimir-static\""));
    }

    #[test]
    fn test_each_token_is_new_and_long_enough() {
        let (first, second) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(first.len(), 32);
        assert_ne!(first, second);
    }

    /// The whole way: the page comes only with the token, and a socket passes on the graph.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_the_page_is_served_only_with_the_token() {
        let (listener, app, address) = listen(PathBuf::new()).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await });

        let get = |path: String| async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let request = format!(
                "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut answer = Vec::new();
            stream.read_to_end(&mut answer).await.unwrap();
            String::from_utf8_lossy(&answer).into_owned()
        };

        let token_path = address
            .split_once(&format!(":{port}"))
            .unwrap()
            .1
            .to_string();
        let page = get(token_path).await;
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        assert!(page.contains("mimir_view_page"), "{page}");
        let refused = get("/wrong/".to_string()).await;
        assert!(refused.starts_with("HTTP/1.1 404"), "{refused}");
    }
}
