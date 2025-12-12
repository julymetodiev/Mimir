//! Programming language detection and metadata.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Supported programming languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    Kotlin,
    C,
    Cpp,
    Unknown,
}

impl Language {
    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Self::Rust,
            "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "py" | "pyi" | "pyw" => Self::Python,
            "go" => Self::Go,
            "java" => Self::Java,
            "kt" | "kts" => Self::Kotlin,
            "c" | "h" => Self::C,
            "cpp" | "hpp" | "cc" | "cxx" | "hh" | "hxx" => Self::Cpp,
            _ => Self::Unknown,
        }
    }

    /// Detect language from file path.
    pub fn from_path(path: &Path) -> Self {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(Self::from_extension)
            .unwrap_or(Self::Unknown)
    }

    /// Get the file extensions associated with this language.
    pub const fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["rs"],
            Self::TypeScript => &["ts", "tsx", "mts", "cts"],
            Self::JavaScript => &["js", "jsx", "mjs", "cjs"],
            Self::Python => &["py", "pyi", "pyw"],
            Self::Go => &["go"],
            Self::Java => &["java"],
            Self::Kotlin => &["kt", "kts"],
            Self::C => &["c", "h"],
            Self::Cpp => &["cpp", "hpp", "cc", "cxx", "hh", "hxx"],
            Self::Unknown => &[],
        }
    }

    /// Get the primary file extension for this language.
    pub const fn primary_extension(&self) -> Option<&'static str> {
        match self {
            Self::Rust => Some("rs"),
            Self::TypeScript => Some("ts"),
            Self::JavaScript => Some("js"),
            Self::Python => Some("py"),
            Self::Go => Some("go"),
            Self::Java => Some("java"),
            Self::Kotlin => Some("kt"),
            Self::C => Some("c"),
            Self::Cpp => Some("cpp"),
            Self::Unknown => None,
        }
    }

    /// Get the display name for this language.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::TypeScript => "TypeScript",
            Self::JavaScript => "JavaScript",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::Java => "Java",
            Self::Kotlin => "Kotlin",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Unknown => "Unknown",
        }
    }

    /// Check if this language is known (not Unknown).
    #[inline]
    pub const fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl Default for Language {
    fn default() -> Self {
        Self::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_extension() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("tsx"), Language::TypeScript);
        assert_eq!(Language::from_extension("py"), Language::Python);
        assert_eq!(Language::from_extension("xyz"), Language::Unknown);
    }

    #[test]
    fn test_from_path() {
        assert_eq!(
            Language::from_path(Path::new("main.rs")),
            Language::Rust
        );
        assert_eq!(
            Language::from_path(Path::new("src/components/App.tsx")),
            Language::TypeScript
        );
        assert_eq!(
            Language::from_path(Path::new("README.md")),
            Language::Unknown
        );
    }

    #[test]
    fn test_extensions() {
        let rust_exts = Language::Rust.extensions();
        assert!(rust_exts.contains(&"rs"));

        let ts_exts = Language::TypeScript.extensions();
        assert!(ts_exts.contains(&"ts"));
        assert!(ts_exts.contains(&"tsx"));
    }

    #[test]
    fn test_serialization() {
        let lang = Language::Rust;
        let json = serde_json::to_string(&lang).unwrap();
        assert_eq!(json, "\"rust\"");

        let restored: Language = serde_json::from_str(&json).unwrap();
        assert_eq!(lang, restored);
    }

    #[test]
    fn test_display() {
        assert_eq!(Language::Rust.to_string(), "Rust");
        assert_eq!(Language::Cpp.to_string(), "C++");
    }
}
