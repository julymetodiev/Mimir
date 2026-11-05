//! The two steps that write an index (`docs/GO.md`, section 4): `go list`, with the network, which
//! fetches what is missing, compiles, and says what did not compile; then `scip-go`, with none.

use std::fs::{self, File};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mimir_lsp::Stopping;
use mimir_resolve_scip::{IndexFiles, ScipError, wait_for};
use serde::Deserialize;

use crate::tools;

/// For the two steps together. One that takes longer keeps syntax rules, and the others go on.
const TIMEOUT: Duration = Duration::from_secs(600);
/// How many packages a person is told of by name.
const NAMED_PACKAGES: usize = 20;
/// How the files of a Go index start in the cache directory.
pub const STEM_PREFIX: &str = "scip-go";
const INCOMPLETE: &str = "incomplete";
const LISTED: &str = "listed";

fn failed(e: impl std::fmt::Display) -> ScipError {
    ScipError::Failed(e.to_string())
}

/// Write the index of the root at this absolute path.
pub fn write(root: &Path, paths: &IndexFiles, stopping: &Stopping) -> Result<(), ScipError> {
    let until = Instant::now() + TIMEOUT;
    let incomplete = paths.other(INCOMPLETE);
    remove_if_there(&incomplete)?;
    list_packages(root, paths, until, stopping)?;
    let listed = fs::read(paths.other(LISTED)).map_err(failed)?;
    let problems = incomplete_in(&listed);
    remove_if_there(&paths.other(LISTED))?;
    fs::write(&incomplete, problems.join("\n")).map_err(failed)?;
    run_scip_go(root, paths, until, stopping)
}

fn remove_if_there(path: &Path) -> Result<(), ScipError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(failed(e)),
        _ => Ok(()),
    }
}

/// The log of a step, to hand to both its streams.
fn log_of(paths: &IndexFiles) -> Result<(Stdio, Stdio), ScipError> {
    let log = File::create(&paths.log).map_err(failed)?;
    let again = log.try_clone().map_err(failed)?;
    Ok((log.into(), again.into()))
}

/// Without `-export` an undefined name is not seen, and without `-test` nothing in a test file is.
fn list_packages(
    root: &Path,
    paths: &IndexFiles,
    until: Instant,
    stopping: &Stopping,
) -> Result<(), ScipError> {
    let listed = File::create(paths.other(LISTED)).map_err(failed)?;
    let log = File::create(&paths.log).map_err(failed)?;
    let mut child = Command::new(tools::GO)
        .args(["list", "-e", "-export", "-test"])
        .arg("-json=ImportPath,Incomplete,Error,DepsErrors")
        .arg("./...")
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(listed)
        .stderr(log)
        .spawn()
        .map_err(|e| ScipError::ToolMissing(format!("cannot run go: {e}")))?;
    wait_for(&mut child, until, tools::GO, stopping).map_err(|e| with_log(e, paths))
}

/// A tool's own reason, as a `go.mod` asking for a Go that is not here.
fn with_log(e: ScipError, paths: &IndexFiles) -> ScipError {
    let log = fs::read_to_string(&paths.log).unwrap_or_default();
    match log.lines().rev().find(|line| !line.trim().is_empty()) {
        Some(reason) => failed(format!("{e}: {reason}")),
        None => e,
    }
}

/// With a closed port as the proxy, since `scip-go` asks a module's host over HTTP for each package
/// of another module of a `go.work`, twice and with no memory of the answer (`hasSameRepoRoot` in
/// its `internal/loader/loader.go`), only to choose the version in a symbol, which Mimir drops. This
/// goes when that is fixed. Go itself has no network in this step, which is why `go list` comes first.
fn run_scip_go(
    root: &Path,
    paths: &IndexFiles,
    until: Instant,
    stopping: &Stopping,
) -> Result<(), ScipError> {
    let proxy = closed_port_address()?;
    run_scip_go_through(root, paths, until, &proxy, stopping)
}

/// A local address nothing listens on: a port taken and let go.
fn closed_port_address() -> Result<String, ScipError> {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map_err(failed)?
        .port();
    Ok(format!("http://127.0.0.1:{port}"))
}

pub fn run_scip_go_through(
    root: &Path,
    paths: &IndexFiles,
    until: Instant,
    proxy: &str,
    stopping: &Stopping,
) -> Result<(), ScipError> {
    let (stdout, stderr) = log_of(paths)?;
    tools::check_scip_go();
    let mut child = Command::new(tools::SCIP_GO)
        .arg("--output")
        .arg(&paths.index)
        .current_dir(root)
        .env("HTTPS_PROXY", proxy)
        .env("HTTP_PROXY", proxy)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(|e| ScipError::ToolMissing(format!("cannot run scip-go: {e}")))?;
    wait_for(&mut child, until, tools::SCIP_GO, stopping).map_err(|e| with_log(e, paths))
}

/// What `go list -json` says of one package.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Listed {
    import_path: String,
    #[serde(default)]
    incomplete: bool,
    error: Option<PackageError>,
    #[serde(default)]
    deps_errors: Vec<PackageError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PackageError {
    #[serde(default)]
    pos: String,
    #[serde(default)]
    err: String,
}

/// A line for each package that did not compile, with the reason. A package and its test variants
/// say the same thing, so a reason counts once.
fn incomplete_in(listed: &[u8]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    serde_json::Deserializer::from_slice(listed)
        .into_iter::<Listed>()
        .map_while(Result::ok)
        .filter(|package| package.incomplete)
        .filter_map(|package| {
            let error = package.error.as_ref().or(package.deps_errors.first());
            let reason = error.map_or(String::new(), |e| {
                if e.pos.is_empty() {
                    e.err.clone()
                } else {
                    format!("{}: {}", e.pos, e.err)
                }
            });
            seen.insert(reason.clone())
                .then(|| format!("{}: {reason}", package.import_path))
        })
        .collect()
}

/// What a person should know of a root's kept index: which packages did not compile. What resolved
/// in them stays.
pub fn incomplete_packages(cache_dir: &Path, root: &Path) -> Vec<String> {
    let path = IndexFiles::of(cache_dir, STEM_PREFIX, root).other(INCOMPLETE);
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut note = format!("Go: {} packages did not compile:", lines.len());
    for line in lines.iter().take(NAMED_PACKAGES) {
        note.push_str("\n  ");
        note.push_str(line);
    }
    vec![note]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::path::PathBuf;

    use mimir_resolve_scip::decode;

    use super::*;

    #[test]
    fn says_which_packages_did_not_compile() {
        let listed = br#"
            {"ImportPath": "a"}
            {"ImportPath": "b", "Incomplete": true,
             "Error": {"Pos": "b/b.go:3:2", "Err": "undefined: x"}}
            {"ImportPath": "b [b.test]", "Incomplete": true,
             "Error": {"Pos": "b/b.go:3:2", "Err": "undefined: x"}}
            {"ImportPath": "c", "Incomplete": true,
             "DepsErrors": [{"Err": "no required module provides package d"}]}
        "#;
        assert_eq!(
            incomplete_in(listed),
            [
                "b: b/b.go:3:2: undefined: x",
                "c: no required module provides package d"
            ]
        );
    }

    /// A server that refuses whatever a proxy is asked, for as long as the test runs.
    fn refusing_proxy() -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut connection in listener.incoming().map_while(Result::ok) {
                let _ = connection.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = connection.read(&mut [0; 4096]);
                let _ = connection.write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        format!("http://{address}")
    }

    /// What an index says, without the order within a document, which may differ.
    fn occurrences_with(root: &Path, proxy: &str) -> BTreeMap<PathBuf, Vec<String>> {
        let cache = tempfile::tempdir().unwrap();
        let paths = IndexFiles::of(cache.path(), STEM_PREFIX, root);
        let until = Instant::now() + Duration::from_secs(60);
        run_scip_go_through(root, &paths, until, proxy, &Stopping::default()).unwrap();
        let index = decode(&fs::read(&paths.index).unwrap()).unwrap();
        index
            .documents
            .into_iter()
            .map(|(path, occurrences)| {
                let mut said: Vec<String> = occurrences
                    .iter()
                    .map(|o| format!("{}:{} {}", o.line, o.column, o.symbol))
                    .collect();
                said.sort();
                (path, said)
            })
            .collect()
    }

    /// `scip-go` sends requests to a proxy it is named (`docs/GO.md`, section 4). The index must not
    /// depend on them: whether a proxy answers or nothing listens, it says the same.
    #[test]
    fn the_index_does_not_depend_on_the_proxy() {
        let installed = |tool: &str, flag: &str| Command::new(tool).arg(flag).output().is_ok();
        if !installed(tools::GO, "version") || !installed(tools::SCIP_GO, "--version") {
            eprintln!("skipped: go or scip-go is not installed");
            return;
        }
        let project = tempfile::tempdir().unwrap();
        for (path, text) in [
            ("go.work", "go 1.22\n\nuse (\n\t./a\n\t./b\n)\n"),
            ("a/go.mod", "module example.com/a\n\ngo 1.22\n"),
            ("a/a.go", "package a\n\nfunc F() int { return 1 }\n"),
            (
                "b/go.mod",
                "module example.com/b\n\ngo 1.22\n\nrequire example.com/a v0.0.0\n",
            ),
            (
                "b/b.go",
                "package b\n\nimport \"example.com/a\"\n\nfunc G() int { return a.F() }\n",
            ),
        ] {
            let path = project.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let root = project.path().join("b");

        let closed = occurrences_with(&root, &closed_port_address().unwrap());
        let refusing = occurrences_with(&root, &refusing_proxy());

        assert_eq!(closed, refusing);
        let uses_f = closed
            .values()
            .flatten()
            .any(|said| said.contains("example.com/a") && said.contains("F()."));
        assert!(uses_f, "{closed:?}");
    }

    #[test]
    fn a_closed_port_is_an_address_of_this_machine() {
        assert!(
            closed_port_address()
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
    }
}
