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

/// How a call edge was resolved from the AST.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallEdgeKind {
    /// Direct function call: `foo()`.
    Direct,
    /// Method call via member access: `obj.method()`, `self.method()`.
    Member,
    /// Scoped/qualified call: `module::func()`, `pkg.Func()`.
    Scoped,
    /// Macro invocation or dynamic dispatch — low confidence.
    DynamicGuess,
}

impl std::fmt::Display for CallEdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct => write!(f, "direct"),
            Self::Member => write!(f, "member"),
            Self::Scoped => write!(f, "scoped"),
            Self::DynamicGuess => write!(f, "dynamic_guess"),
        }
    }
}

/// A call edge representing a function/method call found in source code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallEdge {
    /// Name of the calling function/method (the caller).
    pub caller: String,
    /// Name of the called function/method (the callee).
    pub callee: String,
    /// 1-based line number where the call occurs.
    pub line: u32,
    /// 1-based column where the call occurs.
    pub col: u32,
    /// How this call was resolved from the AST.
    pub kind: CallEdgeKind,
    /// Confidence of the resolution (0.0–1.0).
    /// Direct = 1.0, Member = 0.9, Scoped = 0.85, DynamicGuess = 0.5.
    pub confidence: f32,
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
