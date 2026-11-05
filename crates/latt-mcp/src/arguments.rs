//! What each tool takes.

use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

/// The direction of a trace.
#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Who calls it and what it calls.
    #[default]
    Both,
    /// Only who calls it.
    Callers,
    /// Only what it calls.
    Callees,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TraceArguments {
    /// The symbol's name.
    pub symbol: String,
    /// Only a symbol in this file, when several share the name.
    pub file: Option<PathBuf>,
    /// How many levels to follow. The default is 1.
    pub depth: Option<u32>,
    /// Which way to follow. The default is both.
    #[serde(default)]
    pub direction: Direction,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImpactArguments {
    /// Compare with where this branch or commit split off. The default is the last commit.
    pub base: Option<String>,
    /// How many levels of dependents to follow. The default is 2.
    pub depth: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArguments {
    /// A regular expression over symbol names and qualified names.
    pub pattern: String,
    /// Only this kind of symbol: function (methods too), method, struct, enum and so on.
    pub kind: Option<String>,
    /// The most results to return. The default is 20.
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GrepArguments {
    /// A regular expression, or with `literal` the text itself.
    pub pattern: String,
    /// Take the pattern as text, not as a regular expression.
    #[serde(default)]
    pub literal: bool,
    /// Letter case does not matter.
    #[serde(default)]
    pub ignore_case: bool,
    /// Only files matching this glob, such as `*.md` or `crates/**/*.rs`.
    pub path: Option<String>,
    /// Lines to show around each match.
    #[serde(default)]
    pub context: u32,
    /// The most matches to return. The default is 50.
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FilesArguments {
    /// Only files matching this glob, such as `*.md` or `crates/**/*.rs`.
    pub glob: Option<String>,
    /// The most files to return. The default is 500.
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadFileArguments {
    /// The file, by absolute path or relative to the project.
    pub file: PathBuf,
    /// The first line to return, from 1.
    pub first: Option<u32>,
    /// The last line to return. Without it, up to 2000 lines.
    pub last: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArguments {
    /// The symbol's name.
    pub symbol: String,
    /// Only a symbol in this file, when several share the name.
    pub file: Option<PathBuf>,
    /// Lines of context before it.
    #[serde(default)]
    pub before: u32,
    /// Lines of context after it.
    #[serde(default)]
    pub after: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolEdit {
    /// The symbol's name. A qualified name such as `Kind::Variant` also works.
    pub symbol: String,
    /// The file it is in. Needed when several symbols share the name: the edit fails otherwise.
    pub file: Option<PathBuf>,
    /// Its whole new source. Doc comments and attributes above it are not part of the symbol.
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditSymbolsArguments {
    /// The symbols to replace, applied together.
    pub edits: Vec<SymbolEdit>,
    /// Only show what would change.
    #[serde(default)]
    pub preview: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InsertAfterArguments {
    /// The symbol the new code goes after. A qualified name such as `Kind::Variant` also works.
    pub symbol: String,
    /// The file it is in. Needed when several symbols share the name: the edit fails otherwise.
    pub file: Option<PathBuf>,
    /// The new code, indented as it should be.
    pub content: String,
    /// Only show what would change.
    #[serde(default)]
    pub preview: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditLinesArguments {
    /// The file, by absolute path or relative to the project.
    pub file: PathBuf,
    /// The first line to replace, from 1.
    pub start: u32,
    /// The last line to replace, inclusive.
    pub end: u32,
    /// The new text.
    pub content: String,
    /// Only show what would change.
    #[serde(default)]
    pub preview: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TextEdit {
    /// The file, by absolute path or relative to the project.
    pub file: PathBuf,
    /// The exact text to find, with enough around it to occur once. It may span lines.
    pub old: String,
    /// What to put in its place.
    pub new: String,
    /// Replace every place it occurs, instead of refusing when there are several.
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReplaceTextArguments {
    /// The replacements, applied together: if one does not apply, no file changes. In one file they
    /// apply in this order, each to the result of the one before.
    pub edits: Vec<TextEdit>,
    /// Only show what would change.
    #[serde(default)]
    pub preview: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteFileArguments {
    /// The file, by absolute path or relative to the project. Missing directories are made.
    pub file: PathBuf,
    /// The whole text of the file.
    pub content: String,
    /// Replace the file if it exists. Without this, an existing file is refused.
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SyncArguments {
    /// Write a new semantic index instead of asking a running analyzer about edited files.
    #[serde(default)]
    pub full: bool,
}
