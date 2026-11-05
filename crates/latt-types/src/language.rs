//! Programming language detection and metadata.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Supported programming languages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
    Java,
    Kotlin,
    C,
    Cpp,
    #[default]
    Unknown,
}

impl Language {
    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Self::Rust,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "py" | "pyi" | "pyw" => Self::Python,
            "go" => Self::Go,
            "java" => Self::Java,
            "kt" | "kts" => Self::Kotlin,
            "c" => Self::C,
            // A `.h` may hold either, and the C++ grammar reads what headers hold.
            "cpp" | "h" | "hpp" | "cc" | "cxx" | "hh" | "hxx" => Self::Cpp,
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

    /// Get the display name for this language.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::TypeScript => "TypeScript",
            Self::Tsx => "TSX",
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

    /// Languages whose files can call each other, and so are linked together.
    pub const fn family(&self) -> Self {
        match self {
            Self::Tsx | Self::JavaScript => Self::TypeScript,
            Self::Cpp => Self::C,
            other => *other,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_extension() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("tsx"), Language::Tsx);
        assert_eq!(Language::from_extension("py"), Language::Python);
        assert_eq!(Language::from_extension("xyz"), Language::Unknown);
        assert_eq!(Language::from_extension("c"), Language::C);
        assert_eq!(Language::from_extension("h"), Language::Cpp);
        assert_eq!(Language::Cpp.family(), Language::C.family());
    }

    #[test]
    fn test_from_path() {
        assert_eq!(Language::from_path(Path::new("main.rs")), Language::Rust);
        assert_eq!(
            Language::from_path(Path::new("src/components/App.tsx")),
            Language::Tsx
        );
        assert_eq!(
            Language::from_path(Path::new("README.md")),
            Language::Unknown
        );
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
