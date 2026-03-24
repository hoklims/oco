//! Symbol and import information extracted from source code.

use serde::{Deserialize, Serialize};

/// The kind of symbol extracted from source code.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Trait,
    Interface,
    Enum,
    Constant,
    Variable,
    Module,
    Type,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Trait => "trait",
            Self::Interface => "interface",
            Self::Enum => "enum",
            Self::Constant => "constant",
            Self::Variable => "variable",
            Self::Module => "module",
            Self::Type => "type",
        };
        write!(f, "{label}")
    }
}

/// Information about a symbol found in source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    /// The symbol name.
    pub name: String,
    /// What kind of symbol this is.
    pub kind: SymbolKind,
    /// 1-based line number where the symbol is defined.
    pub line: u32,
    /// Optional 1-based end line of the symbol's body.
    pub end_line: Option<u32>,
    /// Optional full signature (e.g. `fn foo(x: i32) -> bool`).
    pub signature: Option<String>,
    /// Optional visibility modifier (e.g. `pub`, `pub(crate)`, `private`, `public`).
    pub visibility: Option<String>,
}

/// Information about an import statement found in source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportInfo {
    /// The module/package path being imported.
    pub path: String,
    /// Specific items imported (empty if importing the whole module).
    pub items: Vec<String>,
    /// 1-based line number of the import statement.
    pub line: u32,
}
