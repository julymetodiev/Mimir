//! Call and function value edges from a SCIP index, written by `rust-analyzer scip`.
//!
//! The index is kept beside the graph with a list of the sources it was made from. An unchanged
//! project does not pay for it again, and edited files go to a running analyzer (see
//! [`crate::live`]) until too many have changed. Set `MIMIR_NO_SCIP` to do without.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mimir_relations::EdgeKind;
use mimir_types::{Confidence, ContentHash, FileId, ReferenceKind, Span, Symbol};
use tracing::{info, warn};

use crate::live::{Analyzers, LiveDocument};
use crate::lookup::{SymbolTable, trait_of};
use crate::{EdgeSet, ProjectFile, files_by_path};

const ROLE_DEFINITION: u64 = 1;
const ROLE_IMPORT: u64 = 2;
const MAX_ROOTS: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(600);
/// An edit can change what files outside it resolve to, so edits may not pile up forever.
const MAX_EDITED_FILES: usize = 50;

/// One occurrence of a symbol in a document. Lines and columns are 0-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub symbol: String,
    pub roles: u64,
}

/// A decoded index. Document paths are relative to `root`, itself relative to the project.
#[derive(Debug, Clone, Default)]
pub struct ScipIndex {
    pub root: PathBuf,
    pub documents: HashMap<PathBuf, Vec<Occurrence>>,
    /// Files edited since the index, by path relative to the project.
    pub live: HashMap<PathBuf, LiveDocument>,
    /// Files edited since the index with no answers yet, which syntax rules resolve for now.
    pub unanswered: HashSet<PathBuf>,
}

/// Whether an index may be kept after an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Refresh {
    /// Keep it, and resolve edited files with a running analyzer.
    #[default]
    Edits,
    /// Write it again if anything changed.
    Full,
    /// Only what is on disk, at once: no analyzer, no new index, and edited files left to syntax.
    Saved,
}

/// The indexes for a project's Rust files: from the cache, or by running `rust-analyzer`.
/// Files in no index, because the tool is missing or failed, keep their syntax edges.
pub fn load_or_generate(
    project: &Path,
    cache_dir: &Path,
    files: &[ProjectFile],
    analyzers: &mut Analyzers,
    refresh: Refresh,
) -> Vec<ScipIndex> {
    if std::env::var_os("MIMIR_NO_SCIP").is_some() {
        return Vec::new();
    }
    let rust: Vec<&Path> = files
        .iter()
        .map(|f| f.path.as_path())
        .filter(|p| p.is_relative() && p.extension().is_some_and(|e| e == "rs"))
        .collect();
    let mut indexes: Vec<ScipIndex> = Vec::new();
    let mut tried: HashSet<PathBuf> = HashSet::new();
    while tried.len() < MAX_ROOTS {
        let uncovered = rust
            .iter()
            .filter(|p| !indexes.iter().any(|i| i.covers(p)))
            .find_map(|p| next_cargo_root(project, p, &tried, &indexes));
        let Some(root) = uncovered else {
            break;
        };
        tried.insert(root.clone());
        match load_root(project, cache_dir, &root, files, analyzers, refresh) {
            Ok(index) => indexes.push(index),
            Err(LoadError::ToolMissing(e)) => {
                warn!("No semantic index, using syntax rules: {e}");
                break;
            }
            Err(LoadError::Failed(e)) => warn!("No semantic index for {:?}, using syntax rules: {e}", root),
            Err(LoadError::NotSaved) => {}
        }
    }
    indexes
}

enum LoadError {
    /// `rust-analyzer` is not installed, so no other root can do better.
    ToolMissing(String),
    Failed(String),
    /// [`Refresh::Saved`] found no index.
    NotSaved,
}

impl From<String> for LoadError {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}

impl ScipIndex {
    fn covers(&self, path: &Path) -> bool {
        path.strip_prefix(&self.root).is_ok_and(|p| self.documents.contains_key(p))
    }

    /// Whether a file changed since the index, so its entries here no longer hold.
    fn is_edited(&self, path: &Path) -> bool {
        self.live.contains_key(path) || self.unanswered.contains(path)
    }

    /// Whether the index holds any file under a directory.
    fn reaches(&self, dir: &Path) -> bool {
        self.documents.keys().any(|path| self.root.join(path).starts_with(dir))
    }
}

/// The outermost untried directory above a file with a `Cargo.toml`, relative to the project.
/// Outermost, because a member crate's index would stop at the crate.
///
/// A crate an index already reaches is left out. Its files missing from that index are behind
/// `cfg` or a feature, or in no module tree, and indexing the crate alone does not find them.
fn next_cargo_root(project: &Path, file: &Path, tried: &HashSet<PathBuf>, indexes: &[ScipIndex]) -> Option<PathBuf> {
    let dirs: Vec<&Path> = file.ancestors().skip(1).collect();
    dirs.into_iter()
        .rev()
        .find(|dir| {
            !tried.contains(*dir)
                && project.join(dir).join("Cargo.toml").is_file()
                && !indexes.iter().any(|index| index.reaches(dir))
        })
        .map(Path::to_path_buf)
}

fn load_root(
    project: &Path,
    cache_dir: &Path,
    root: &Path,
    files: &[ProjectFile],
    analyzers: &mut Analyzers,
    refresh: Refresh,
) -> Result<ScipIndex, LoadError> {
    let abs = project.join(root);
    let snapshot = Snapshot::of(project, root, files);
    let stem = format!("scip-{}", ContentHash::compute(root.to_string_lossy().as_bytes()).to_hex());
    let (index_file, key_file) = (cache_dir.join(format!("{stem}.scip")), cache_dir.join(format!("{stem}.key")));
    let read = || -> Result<ScipIndex, LoadError> {
        let bytes = fs::read(&index_file).map_err(|e| LoadError::Failed(e.to_string()))?;
        let mut index = decode(&bytes)?;
        index.root = root.to_path_buf();
        Ok(index)
    };

    let saved = fs::read_to_string(&key_file).ok().filter(|_| index_file.is_file()).map(|text| Snapshot::parse(&text));
    if refresh == Refresh::Saved {
        let saved = saved.ok_or(LoadError::NotSaved)?;
        let mut index = read()?;
        let edited = edited_files(&index, &saved, &snapshot, files);
        index.unanswered = edited.into_iter().map(|f| f.path.clone()).collect();
        return Ok(index);
    }
    if let Some(saved) = saved {
        if saved == snapshot {
            return read();
        }
        if refresh == Refresh::Edits && saved.manifests == snapshot.manifests {
            let mut index = read()?;
            let edited = edited_files(&index, &saved, &snapshot, files);
            if edited.is_empty() {
                return Ok(index);
            }
            if edited.len() <= MAX_EDITED_FILES {
                match analyzers.resolve(project, &abs, files, &edited) {
                    Ok(live) => {
                        index.live = live;
                        return Ok(index);
                    }
                    Err(e) => warn!("No running analyzer for {:?}, writing a new index: {e}", root),
                }
            }
        }
    }

    fs::create_dir_all(cache_dir).map_err(|e| LoadError::Failed(e.to_string()))?;
    let _ = fs::remove_file(&key_file);
    let started = Instant::now();
    run_rust_analyzer(&abs, &index_file)?;
    fs::write(&key_file, snapshot.text()).map_err(|e| LoadError::Failed(e.to_string()))?;
    info!("Semantic index for {:?} written in {:?}", root, started.elapsed());
    read()
}

/// Files whose index entries no longer hold: changed ones it covered, and new ones.
/// A changed file it did not cover is behind `cfg`, where an analyzer knows no more than it did.
fn edited_files<'a>(
    index: &ScipIndex,
    saved: &Snapshot,
    now: &Snapshot,
    files: &'a [ProjectFile],
) -> Vec<&'a ProjectFile> {
    files
        .iter()
        .filter(|f| {
            let current = now.sources.get(&f.path);
            let before = saved.sources.get(&f.path);
            current.is_some() && current != before && (before.is_none() || index.covers(&f.path))
        })
        .collect()
}

/// What an index was made from: the hash of the manifests, and of each source by path.
#[derive(Debug, Default, PartialEq, Eq)]
struct Snapshot {
    manifests: String,
    sources: HashMap<PathBuf, String>,
}

impl Snapshot {
    fn text(&self) -> String {
        let mut sources: Vec<_> = self.sources.iter().collect();
        sources.sort();
        let mut text = format!("{}\n", self.manifests);
        for (path, hash) in sources {
            text.push_str(&format!("{hash} {}\n", path.display()));
        }
        text
    }

    fn parse(text: &str) -> Self {
        let mut lines = text.lines();
        let manifests = lines.next().unwrap_or("").to_string();
        let sources = lines
            .filter_map(|line| line.split_once(' '))
            .map(|(hash, path)| (PathBuf::from(path), hash.to_string()))
            .collect();
        Self { manifests, sources }
    }

    fn of(project: &Path, root: &Path, files: &[ProjectFile]) -> Self {
        let mut sources: Vec<(&Path, ContentHash)> = files
            .iter()
            .filter(|f| f.path.starts_with(root))
            .map(|f| (f.path.as_path(), ContentHash::compute(f.source.as_bytes())))
            .collect();
        let mut manifests: HashSet<PathBuf> = sources
            .iter()
            .flat_map(|(path, _)| path.ancestors().skip(1))
            .filter(|dir| dir.starts_with(root))
            .flat_map(|dir| [dir.join("Cargo.toml"), dir.join("Cargo.lock")])
            .collect();
        manifests.insert(root.join("Cargo.lock"));
        let mut manifests: Vec<_> = manifests.into_iter().collect();
        manifests.sort();
        let mut text = String::new();
        for manifest in manifests {
            if let Ok(bytes) = fs::read(project.join(&manifest)) {
                text.push_str(&format!("{} {}\n", manifest.display(), ContentHash::compute(&bytes).to_hex()));
            }
        }
        sources.sort_by_key(|(path, _)| *path);
        Self {
            manifests: ContentHash::compute(text.as_bytes()).to_hex(),
            sources: sources.into_iter().map(|(path, hash)| (path.to_path_buf(), hash.to_hex())).collect(),
        }
    }
}

fn run_rust_analyzer(root: &Path, output: &Path) -> Result<(), LoadError> {
    let mut child = Command::new("rust-analyzer")
        .args(["scip", "."])
        .arg("--output")
        .arg(output)
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| LoadError::ToolMissing(format!("cannot run rust-analyzer: {e}")))?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(format!("rust-analyzer exited with {status}").into()),
            None if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("rust-analyzer timed out".to_string().into());
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Add an edge for each call and each function named as a value in the indexes.
/// Returns the files they cover.
pub(crate) fn resolve(
    table: &SymbolTable,
    files: &[ProjectFile],
    indexes: &[ScipIndex],
    edges: &mut EdgeSet,
) -> HashSet<FileId> {
    let by_path = files_by_path(files);
    let mut covered = HashSet::new();
    for index in indexes {
        let documents: Vec<(&ProjectFile, Vec<&str>, &Vec<Occurrence>)> = index
            .documents
            .iter()
            .filter_map(|(path, occurrences)| {
                let file = by_path.get(index.root.join(path).as_path())?;
                Some((*file, file.source.split('\n').collect(), occurrences))
            })
            .collect();

        let mut definitions = Definitions::default();
        for (file, lines, occurrences) in &documents {
            if !index.unanswered.contains(&file.path) {
                covered.insert(file.file_id);
            }
            if index.is_edited(&file.path) {
                // The positions are old, so find each function again by name and order.
                let mut seen: HashMap<&str, usize> = HashMap::new();
                let mut own: Vec<_> = occurrences.iter().filter(|o| o.roles & ROLE_DEFINITION != 0 && is_function(&o.symbol)).collect();
                own.sort_by_key(|o| (o.line, o.column));
                for o in own {
                    let name = symbol_name(&o.symbol);
                    let n = seen.entry(name).or_default();
                    if let Some(symbol) = table.nth_definition(file.file_id, name, *n) {
                        definitions.add(o.symbol.as_str(), symbol);
                    }
                    *n += 1;
                }
                continue;
            }
            for o in occurrences.iter().filter(|o| o.roles & ROLE_DEFINITION != 0 && is_function(&o.symbol)) {
                let Some(name) = lines.get(o.line as usize).and_then(|l| l.get(o.column as usize..o.end_column as usize)) else {
                    continue;
                };
                if let Some(symbol) = table.definition_at(file.file_id, o.line + 1, name) {
                    definitions.add(o.symbol.as_str(), symbol);
                }
            }
        }

        for (file, lines, occurrences) in documents.iter().filter(|(file, ..)| !index.is_edited(&file.path)) {
            // rust-analyzer does not set the import role on the paths in a `use`.
            let imports: Vec<Span> = file
                .references
                .iter()
                .filter(|r| r.kind == ReferenceKind::Import)
                .map(|r| r.span)
                .collect();
            for o in occurrences.iter().filter(|o| o.roles & (ROLE_DEFINITION | ROLE_IMPORT) == 0 && is_function(&o.symbol)) {
                let Some(target) = definitions.get(o.symbol.as_str(), file.file_id) else {
                    continue;
                };
                let Some(after) = lines.get(o.end_line as usize).and_then(|l| l.get(o.end_column as usize..)) else {
                    continue;
                };
                let start = byte_offset(lines, o.line, o.column);
                let span = Span::new(start, byte_offset(lines, o.end_line, o.end_column));
                // An operator's occurrence is its token: `a[i]` is `index`, `a == b` is `eq`.
                let named = lines
                    .get(o.line as usize)
                    .and_then(|l| l.get(o.column as usize..))
                    .is_some_and(|t| t.starts_with(|c: char| c.is_alphabetic() || c == '_'));
                let (from, kind) = if is_call(after) {
                    (table.callable_at(file.file_id, o.line + 1, o.column), EdgeKind::Calls)
                } else if !named || imports.iter().any(|i| i.start <= start && start < i.end) {
                    continue;
                } else {
                    (table.any_at(file.file_id, start), EdgeKind::UsesAsValue)
                };
                if let Some(from) = from {
                    edges.add(from.id, target.id, kind, span, Confidence::Certain);
                }
            }
        }

        for (file, lines, occurrences) in documents.iter().filter(|(file, ..)| !index.is_edited(&file.path)) {
            for (method, trait_name) in table.trait_impls_in(file.file_id) {
                let declaration = table.impl_at(file.file_id, method.range.start_line).and_then(|block| {
                    declared_method(block, method, trait_name, lines, occurrences, &definitions, file.file_id)
                });
                if let Some(declaration) = declaration {
                    edges.add(method.id, declaration.id, EdgeKind::Implements, method.span, Confidence::Certain);
                }
            }
        }

        for (path, document) in &index.live {
            let Some(file) = by_path.get(path.as_path()) else {
                continue;
            };
            covered.insert(file.file_id);
            resolve_live(table, file, document, &by_path, edges);
        }
    }
    covered
}

/// Functions by SCIP symbol. A symbol can name several: a package's test binaries are crates
/// of their own, yet `main` or a helper in each gets the same symbol.
#[derive(Default)]
struct Definitions<'a> {
    by_symbol: HashMap<&'a str, Vec<Arc<Symbol>>>,
}

impl<'a> Definitions<'a> {
    fn add(&mut self, symbol: &'a str, definition: &Arc<Symbol>) {
        self.by_symbol.entry(symbol).or_default().push(definition.clone());
    }

    /// What a symbol means in a file: the one defined there, or else the only one there is.
    fn get(&self, symbol: &str, file: FileId) -> Option<&Arc<Symbol>> {
        let found = self.by_symbol.get(symbol)?;
        found.iter().find(|s| s.file_id == file).or(match found.as_slice() {
            [one] => Some(one),
            _ => None,
        })
    }
}

/// Edges of an edited file, from a running analyzer's answers.
fn resolve_live(
    table: &SymbolTable,
    file: &ProjectFile,
    document: &LiveDocument,
    by_path: &HashMap<&Path, &ProjectFile>,
    edges: &mut EdgeSet,
) {
    for reference in &file.references {
        let (line, column) = (reference.range.start_line, reference.range.start_col);
        let (from, kind) = match reference.kind {
            ReferenceKind::Call => (table.callable_at(file.file_id, line, column), EdgeKind::Calls),
            ReferenceKind::FunctionValue => (table.any_at(file.file_id, reference.span.start), EdgeKind::UsesAsValue),
            _ => continue,
        };
        let target = document
            .sites
            .get(&(line, column))
            .and_then(|t| table.definition_at(by_path.get(t.path.as_path())?.file_id, t.line, &t.name));
        if let (Some(from), Some(target)) = (from, target) {
            edges.add(from.id, target.id, kind, reference.span, Confidence::Certain);
        }
    }
    for (method, trait_name) in table.trait_impls_in(file.file_id) {
        let declared = table
            .impl_at(file.file_id, method.range.start_line)
            .and_then(|block| document.traits.get(&block.range.start_line))
            .and_then(|t| by_path.get(t.path.as_path()))
            .and_then(|defined_in| {
                table
                    .methods_of(trait_name, &method.name)
                    .iter()
                    .find(|s| s.file_id == defined_in.file_id && trait_of(s).is_none())
            });
        if let Some(declared) = declared {
            edges.add(method.id, declared.id, EdgeKind::Implements, method.span, Confidence::Certain);
        }
    }
}

/// The trait's method for a method of an `impl`. The trait's name in the impl's header is a
/// reference to the exact trait, `crate/path/Tr#`, whose method is `crate/path/Tr#name().`.
/// A trait from outside the project has no definition here, and gets no edge.
fn declared_method<'a>(
    block: &Symbol,
    method: &Symbol,
    trait_name: &str,
    lines: &[&str],
    occurrences: &[Occurrence],
    definitions: &'a Definitions<'_>,
    file: FileId,
) -> Option<&'a Arc<Symbol>> {
    occurrences
        .iter()
        .filter(|o| o.symbol.ends_with('#') && o.line + 1 >= block.range.start_line && o.line + 1 < method.range.start_line.max(block.range.start_line + 1))
        .filter(|o| lines.get(o.line as usize).and_then(|l| l.get(o.column as usize..o.end_column as usize)) == Some(trait_name))
        .find_map(|o| definitions.get(format!("{}{}().", o.symbol, method.name).as_str(), file))
}

/// Whether the text after a function's name makes it a call: `(`, or `::<...>(`.
fn is_call(after: &str) -> bool {
    let after = after.trim_start();
    let Some(generics) = after.strip_prefix("::<") else {
        return after.starts_with('(');
    };
    let (mut depth, mut previous) = (1, '<');
    for (at, c) in generics.char_indices() {
        match c {
            '<' => depth += 1,
            '>' if previous != '-' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return generics[at + 1..].trim_start().starts_with('(');
        }
        previous = c;
    }
    // Generics running past the end of the line: nearly always a call.
    true
}

fn byte_offset(lines: &[&str], line: u32, column: u32) -> u32 {
    let before: usize = lines.iter().take(line as usize).map(|l| l.len() + 1).sum();
    before as u32 + column
}

/// The name in a function's SCIP symbol: `run` for `a 0.1.0 impl#[T]run().`.
fn symbol_name(symbol: &str) -> &str {
    let descriptor = symbol.strip_suffix("().").unwrap_or(symbol);
    let start = descriptor.rfind(['/', '#', ']', ' ']).map_or(0, |at| at + 1);
    &descriptor[start..]
}

/// In SCIP's symbol syntax a function or method ends in `().`.
fn is_function(symbol: &str) -> bool {
    symbol.ends_with("().") && !symbol.starts_with("local ")
}

/// Decode what is needed of `scip.proto`: `Index.documents = 2`; `Document.relative_path = 1`,
/// `occurrences = 2`; `Occurrence.range = 1`, `symbol = 2`, `symbol_roles = 3`.
pub fn decode(bytes: &[u8]) -> Result<ScipIndex, String> {
    let mut index = ScipIndex::default();
    for field in Fields::new(bytes) {
        if let (2, Value::Bytes(document)) = field? {
            let mut path = None;
            let mut occurrences = Vec::new();
            for field in Fields::new(document) {
                match field? {
                    (1, Value::Bytes(p)) => path = Some(PathBuf::from(String::from_utf8_lossy(p).into_owned())),
                    (2, Value::Bytes(o)) => occurrences.push(decode_occurrence(o)?),
                    _ => {}
                }
            }
            let path = path.ok_or("document without a path")?;
            index.documents.insert(path, occurrences);
        }
    }
    Ok(index)
}

fn decode_occurrence(bytes: &[u8]) -> Result<Occurrence, String> {
    let (mut range, mut symbol, mut roles) = (Vec::new(), String::new(), 0);
    for field in Fields::new(bytes) {
        match field? {
            (1, Value::Bytes(packed)) => {
                let mut at = 0;
                while at < packed.len() {
                    range.push(varint(packed, &mut at)? as u32);
                }
            }
            (1, Value::Varint(v)) => range.push(v as u32),
            (2, Value::Bytes(s)) => symbol = String::from_utf8_lossy(s).into_owned(),
            (3, Value::Varint(v)) => roles = v,
            _ => {}
        }
    }
    // [line, column, end_column], or [line, column, end_line, end_column].
    let (line, column, end_line, end_column) = match range[..] {
        [l, c, ec] => (l, c, l, ec),
        [l, c, el, ec] => (l, c, el, ec),
        _ => return Err(format!("bad occurrence range {range:?}")),
    };
    Ok(Occurrence { line, column, end_line, end_column, symbol, roles })
}

enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

/// The fields of a protobuf message.
struct Fields<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn read(&mut self) -> Result<(u64, Value<'a>), String> {
        let key = varint(self.bytes, &mut self.at)?;
        let value = match key & 7 {
            0 => Value::Varint(varint(self.bytes, &mut self.at)?),
            1 => self.skip(8)?,
            2 => {
                let len = varint(self.bytes, &mut self.at)? as usize;
                let end = self.at.checked_add(len).filter(|e| *e <= self.bytes.len()).ok_or("truncated field")?;
                let value = &self.bytes[self.at..end];
                self.at = end;
                Value::Bytes(value)
            }
            5 => self.skip(4)?,
            wire => return Err(format!("unsupported wire type {wire}")),
        };
        Ok((key >> 3, value))
    }

    fn skip(&mut self, n: usize) -> Result<Value<'a>, String> {
        if self.at + n > self.bytes.len() {
            return Err("truncated field".to_string());
        }
        self.at += n;
        Ok(Value::Fixed)
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = Result<(u64, Value<'a>), String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.at >= self.bytes.len() {
            return None;
        }
        let item = self.read();
        if item.is_err() {
            self.at = self.bytes.len();
        }
        Some(item)
    }
}

fn varint(bytes: &[u8], at: &mut usize) -> Result<u64, String> {
    let (mut value, mut shift) = (0u64, 0);
    loop {
        let byte = *bytes.get(*at).ok_or("truncated varint")?;
        *at += 1;
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(value);
        }
        shift += 7;
        if shift > 63 {
            return Err("varint too long".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(number: u8, bytes: &[u8]) -> Vec<u8> {
        let mut out = vec![number << 3 | 2, bytes.len() as u8];
        out.extend_from_slice(bytes);
        out
    }

    #[test]
    fn decodes_three_and_four_part_ranges() {
        let short = [field(1, &[3, 4, 9]), field(2, b"a()."), vec![3 << 3, 1]].concat();
        let long = [field(1, &[3, 4, 5, 2]), field(2, b"b().")].concat();
        let document = [field(1, b"src/a.rs"), field(2, &short), field(2, &long)].concat();
        let index = decode(&field(2, &document)).unwrap();

        let occurrences = &index.documents[Path::new("src/a.rs")];
        assert_eq!(occurrences[0].end_line, 3);
        assert_eq!(occurrences[0].end_column, 9);
        assert_eq!(occurrences[0].roles, ROLE_DEFINITION);
        assert_eq!(occurrences[1].end_line, 5);
        assert_eq!(occurrences[1].symbol, "b().");
    }

    #[test]
    fn refuses_truncated_input() {
        assert!(decode(&[2 << 3 | 2, 200]).is_err());
    }

    #[test]
    fn a_crate_an_index_reaches_gets_no_index_of_its_own() {
        let project = std::env::temp_dir().join(format!("mimir-resolve-roots-{}", std::process::id()));
        for dir in ["", "crates/member", "other"] {
            fs::create_dir_all(project.join(dir)).unwrap();
            fs::write(project.join(dir).join("Cargo.toml"), "").unwrap();
        }
        let index = ScipIndex {
            root: PathBuf::new(),
            documents: HashMap::from([(PathBuf::from("crates/member/src/lib.rs"), Vec::new())]),
            ..ScipIndex::default()
        };
        let tried = HashSet::from([PathBuf::new()]);
        let next = |file: &str| next_cargo_root(&project, Path::new(file), &tried, std::slice::from_ref(&index));

        let behind_cfg = next("crates/member/src/linux_only.rs");
        let own_workspace = next("other/src/lib.rs");
        let _ = fs::remove_dir_all(&project);

        assert_eq!(behind_cfg, None);
        assert_eq!(own_workspace, Some(PathBuf::from("other")));
    }

    #[test]
    fn tells_a_call_from_a_value() {
        assert!(is_call("(1)"));
        assert!(is_call(" (1)"));
        assert!(is_call("::<u32>()"));
        assert!(is_call("::<Vec<u32>>()"));
        assert!(is_call("::<fn() -> u32>()"));
        assert!(!is_call(")"));
        assert!(!is_call(", other)"));
        assert!(!is_call("::<u32>)"));
        assert!(!is_call(""));
    }

    #[test]
    fn a_snapshot_survives_being_saved() {
        let mut snapshot = Snapshot { manifests: "m".to_string(), ..Snapshot::default() };
        snapshot.sources.insert(PathBuf::from("a b/c.rs"), "h".to_string());
        assert_eq!(Snapshot::parse(&snapshot.text()), snapshot);
    }

    #[test]
    fn finds_the_name_in_a_symbol() {
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 run()."), "run");
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 m/T#run()."), "run");
        assert_eq!(symbol_name("rust-analyzer cargo a 0.1.0 m/impl#[T]run()."), "run");
    }

    #[test]
    fn locals_are_not_functions() {
        assert!(is_function("rust-analyzer cargo a 0.1.0 foo()."));
        assert!(!is_function("local 3"));
        assert!(!is_function("rust-analyzer cargo a 0.1.0 Foo#"));
    }
}
