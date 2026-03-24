//! Code parsing trait and fallback regex-based implementation.

use regex::Regex;
use std::sync::LazyLock;

use crate::error::CodeIntelError;
use crate::symbols::{ImportInfo, SymbolInfo, SymbolKind};

/// A parsed source file with extracted symbols and imports.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    /// File path (may be empty if parsed from a raw string).
    pub path: String,
    /// Detected or specified language.
    pub language: String,
    /// Symbols extracted from the file.
    pub symbols: Vec<SymbolInfo>,
    /// Import statements extracted from the file.
    pub imports: Vec<ImportInfo>,
    /// Total line count.
    pub line_count: u32,
}

/// Trait for code parsers. Implementors provide language-aware symbol extraction.
///
/// The default implementation is [`FallbackParser`] which uses regex heuristics.
/// Future implementations can use Tree-sitter grammars for accurate AST-based parsing.
pub trait CodeParser: Send + Sync {
    /// Parse source code and extract symbols and imports.
    fn parse(&self, source: &str, language: &str) -> Result<ParsedFile, CodeIntelError>;

    /// Return the list of languages this parser supports.
    fn supported_languages(&self) -> Vec<String>;
}

/// Regex-based fallback parser that extracts symbols using pattern matching.
///
/// This works without Tree-sitter grammars installed and covers common patterns
/// across Rust, TypeScript/JavaScript, Python, Go, and Java.
pub struct FallbackParser;

// ---------------------------------------------------------------------------
// Regex patterns compiled once
// ---------------------------------------------------------------------------

/// Rust: `(pub/pub(crate)/...) (fn|struct|trait|enum|type|mod|const|static) name`
static RE_RUST_SYMBOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*(pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(fn|struct|trait|enum|type|mod|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("rust symbol regex")
});

/// Rust: `use path::{items}` or `use path::item;` or `use path;`
static RE_RUST_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*use\s+([^\s;]+)\s*;").expect("rust import regex"));

/// TypeScript/JavaScript: `(export)? (function|class|interface|type|enum|const|let|var) name`
static RE_TS_SYMBOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*(export\s+)?(default\s+)?(async\s+)?(function\*?|class|interface|type|enum|const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)",
    )
    .expect("ts symbol regex")
});

/// TypeScript/JavaScript: `import ... from 'path'` or `import 'path'`
static RE_TS_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^[ \t]*import\s+(?:\{([^}]*)\}\s+from\s+)?(?:(?:[A-Za-z_$][A-Za-z0-9_$]*)\s+from\s+)?['"]([^'"]+)['"]"#)
        .expect("ts import regex")
});

/// Python: `(def|class|async def) name`
static RE_PY_SYMBOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(async\s+)?(def|class)\s+([A-Za-z_][A-Za-z0-9_]*)")
        .expect("python symbol regex")
});

/// Python: `import x` or `from x import y`
static RE_PY_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:from\s+(\S+)\s+import\s+([^#\n]+)|import\s+([^#\n]+))")
        .expect("python import regex")
});

/// Go: `func (receiver) name` or `type name struct/interface`
static RE_GO_SYMBOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^(?:func\s+(?:\([^)]*\)\s+)?([A-Za-z_][A-Za-z0-9_]*))|(?:type\s+([A-Za-z_][A-Za-z0-9_]*)\s+(struct|interface))|(?:const\s+([A-Za-z_][A-Za-z0-9_]*))|(?:var\s+([A-Za-z_][A-Za-z0-9_]*))",
    )
    .expect("go symbol regex")
});

/// Go: `import "path"` or entries inside `import (...)` block
static RE_GO_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:import\s+"([^"]+)")|(?:^\s*(?:[a-zA-Z_]\w*\s+)?"([^"]+)")"#)
        .expect("go import regex")
});

/// Java: `(public|private|protected)? (class|interface|enum|@interface|abstract class) name`
static RE_JAVA_SYMBOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*(public|private|protected)?\s*(?:static\s+)?(?:final\s+)?(?:abstract\s+)?(class|interface|enum|@interface)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("java symbol regex")
});

/// Java: methods — `(visibility)? (static)? ReturnType name(`
static RE_JAVA_METHOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*(public|private|protected)?\s*(?:static\s+)?(?:final\s+)?(?:synchronized\s+)?(?:abstract\s+)?(?:[A-Za-z_<>\[\]?,\s]+)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
    )
    .expect("java method regex")
});

/// Java: `import (static)? path;`
static RE_JAVA_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*import\s+(?:static\s+)?([^\s;]+)\s*;").expect("java import regex")
});

/// Generic fallback: tries common patterns across languages.
static RE_GENERIC_FUNC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*(?:export\s+)?(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:fn|function\*?|def|func)\s+([A-Za-z_$][A-Za-z0-9_$]*)",
    )
    .expect("generic func regex")
});

static RE_GENERIC_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*(?:export\s+)?(?:pub(?:\([^)]*\))?\s+)?(?:class|struct|trait|interface|enum|type)\s+([A-Za-z_$][A-Za-z0-9_$]*)",
    )
    .expect("generic type regex")
});

static RE_GENERIC_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^[ \t]*(?:use|import|from|require|include)\s+[^\n]+"#)
        .expect("generic import regex")
});

impl FallbackParser {
    /// Create a new fallback parser.
    pub fn new() -> Self {
        Self
    }

    fn parse_rust(&self, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
        let lines: Vec<&str> = source.lines().collect();
        let mut symbols = Vec::new();
        let mut imports = Vec::new();

        for cap in RE_RUST_SYMBOL.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let visibility = cap.get(1).map(|m| m.as_str().trim().to_string());
            let keyword = &cap[2];
            let name = cap[3].to_string();

            let kind = match keyword {
                "fn" => {
                    // Heuristic: if indented, it's likely a method
                    let line_text = lines.get((line - 1) as usize).unwrap_or(&"");
                    let indent = line_text.len() - line_text.trim_start().len();
                    if indent > 0 {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    }
                }
                "struct" => SymbolKind::Struct,
                "trait" => SymbolKind::Trait,
                "enum" => SymbolKind::Enum,
                "type" => SymbolKind::Type,
                "mod" => SymbolKind::Module,
                "const" | "static" => SymbolKind::Constant,
                _ => SymbolKind::Variable,
            };

            let signature = lines.get((line - 1) as usize).map(|l| l.trim().to_string());

            symbols.push(SymbolInfo {
                name,
                kind,
                line,
                end_line: None,
                signature,
                visibility,
            });
        }

        for cap in RE_RUST_IMPORT.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let path = cap[1].to_string();

            // Try to extract items from braces in the path
            let (clean_path, items) = if let Some(brace_start) = path.find('{') {
                let prefix = path[..brace_start].trim_end_matches("::").to_string();
                let items_str = &path[brace_start + 1..];
                let items_str = items_str.trim_end_matches('}');
                let items: Vec<String> = items_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                (prefix, items)
            } else {
                (path, Vec::new())
            };

            imports.push(ImportInfo {
                path: clean_path,
                items,
                line,
            });
        }

        (symbols, imports)
    }

    fn parse_typescript(&self, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
        let mut symbols = Vec::new();
        let mut imports = Vec::new();

        for cap in RE_TS_SYMBOL.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let exported = cap.get(1).is_some();
            let keyword = &cap[4];
            let name = cap[5].to_string();

            let visibility = if exported {
                Some("export".to_string())
            } else {
                None
            };

            let kind = match keyword {
                "function" | "function*" => SymbolKind::Function,
                "class" => SymbolKind::Class,
                "interface" => SymbolKind::Interface,
                "type" => SymbolKind::Type,
                "enum" => SymbolKind::Enum,
                "const" => SymbolKind::Constant,
                "let" | "var" => SymbolKind::Variable,
                _ => SymbolKind::Variable,
            };

            let signature = source
                .lines()
                .nth((line - 1) as usize)
                .map(|l| l.trim().to_string());

            symbols.push(SymbolInfo {
                name,
                kind,
                line,
                end_line: None,
                signature,
                visibility,
            });
        }

        for cap in RE_TS_IMPORT.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let items: Vec<String> = cap
                .get(1)
                .map(|m| {
                    m.as_str()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let path = cap[2].to_string();

            imports.push(ImportInfo { path, items, line });
        }

        (symbols, imports)
    }

    fn parse_python(&self, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let lines: Vec<&str> = source.lines().collect();

        for cap in RE_PY_SYMBOL.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let is_async = cap.get(1).is_some();
            let keyword = &cap[2];
            let name = cap[3].to_string();

            let kind = match keyword {
                "def" => {
                    let line_text = lines.get((line - 1) as usize).unwrap_or(&"");
                    let indent = line_text.len() - line_text.trim_start().len();
                    if indent > 0 {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    }
                }
                "class" => SymbolKind::Class,
                _ => SymbolKind::Variable,
            };

            let signature = lines.get((line - 1) as usize).map(|l| l.trim().to_string());

            let _ = is_async; // reflected in the signature already

            let visibility = if name.starts_with('_') {
                Some("private".to_string())
            } else {
                Some("public".to_string())
            };

            symbols.push(SymbolInfo {
                name,
                kind,
                line,
                end_line: None,
                signature,
                visibility,
            });
        }

        for cap in RE_PY_IMPORT.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());

            if let Some(from_path) = cap.get(1) {
                // from X import Y, Z
                let path = from_path.as_str().to_string();
                let items: Vec<String> = cap
                    .get(2)
                    .map(|m| {
                        m.as_str()
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                imports.push(ImportInfo { path, items, line });
            } else if let Some(import_path) = cap.get(3) {
                // import X, Y
                let modules: Vec<&str> = import_path.as_str().split(',').collect();
                for module in modules {
                    let module = module.trim();
                    if !module.is_empty() {
                        imports.push(ImportInfo {
                            path: module.to_string(),
                            items: Vec::new(),
                            line,
                        });
                    }
                }
            }
        }

        (symbols, imports)
    }

    fn parse_go(&self, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
        let mut symbols = Vec::new();
        let mut imports = Vec::new();

        for cap in RE_GO_SYMBOL.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());

            if let Some(func_name) = cap.get(1) {
                let name = func_name.as_str().to_string();
                let is_exported = name.starts_with(|c: char| c.is_uppercase());
                let signature = source
                    .lines()
                    .nth((line - 1) as usize)
                    .map(|l| l.trim().to_string());

                // Check if it's a method (has receiver)
                let line_text = source.lines().nth((line - 1) as usize).unwrap_or("");
                let kind = if line_text.contains("func (") || line_text.contains("func(") {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };

                symbols.push(SymbolInfo {
                    name,
                    kind,
                    line,
                    end_line: None,
                    signature,
                    visibility: Some(if is_exported { "public" } else { "private" }.to_string()),
                });
            } else if let Some(type_name) = cap.get(2) {
                let name = type_name.as_str().to_string();
                let is_exported = name.starts_with(|c: char| c.is_uppercase());
                let type_kind = cap.get(3).map(|m| m.as_str()).unwrap_or("");
                let kind = match type_kind {
                    "struct" => SymbolKind::Struct,
                    "interface" => SymbolKind::Interface,
                    _ => SymbolKind::Type,
                };
                symbols.push(SymbolInfo {
                    name,
                    kind,
                    line,
                    end_line: None,
                    signature: None,
                    visibility: Some(if is_exported { "public" } else { "private" }.to_string()),
                });
            } else if let Some(const_name) = cap.get(4) {
                let name = const_name.as_str().to_string();
                symbols.push(SymbolInfo {
                    name,
                    kind: SymbolKind::Constant,
                    line,
                    end_line: None,
                    signature: None,
                    visibility: None,
                });
            } else if let Some(var_name) = cap.get(5) {
                let name = var_name.as_str().to_string();
                symbols.push(SymbolInfo {
                    name,
                    kind: SymbolKind::Variable,
                    line,
                    end_line: None,
                    signature: None,
                    visibility: None,
                });
            }
        }

        for cap in RE_GO_IMPORT.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let path = cap
                .get(1)
                .or_else(|| cap.get(2))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();

            if !path.is_empty() {
                imports.push(ImportInfo {
                    path,
                    items: Vec::new(),
                    line,
                });
            }
        }

        (symbols, imports)
    }

    fn parse_java(&self, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
        let mut symbols = Vec::new();
        let mut imports = Vec::new();

        for cap in RE_JAVA_SYMBOL.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let visibility = cap.get(1).map(|m| m.as_str().to_string());
            let keyword = &cap[2];
            let name = cap[3].to_string();

            let kind = match keyword {
                "class" => SymbolKind::Class,
                "interface" | "@interface" => SymbolKind::Interface,
                "enum" => SymbolKind::Enum,
                _ => SymbolKind::Type,
            };

            symbols.push(SymbolInfo {
                name,
                kind,
                line,
                end_line: None,
                signature: source
                    .lines()
                    .nth((line - 1) as usize)
                    .map(|l| l.trim().to_string()),
                visibility,
            });
        }

        // Java methods — filter out keywords that look like methods but aren't
        let java_keywords: &[&str] = &[
            "if",
            "for",
            "while",
            "switch",
            "catch",
            "return",
            "new",
            "throw",
            "class",
            "interface",
            "enum",
            "import",
            "package",
        ];
        for cap in RE_JAVA_METHOD.captures_iter(source) {
            let name = cap[2].to_string();
            if java_keywords.contains(&name.as_str()) {
                continue;
            }
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let visibility = cap.get(1).map(|m| m.as_str().to_string());

            symbols.push(SymbolInfo {
                name,
                kind: SymbolKind::Method,
                line,
                end_line: None,
                signature: source
                    .lines()
                    .nth((line - 1) as usize)
                    .map(|l| l.trim().to_string()),
                visibility,
            });
        }

        for cap in RE_JAVA_IMPORT.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let path = cap[1].to_string();
            imports.push(ImportInfo {
                path,
                items: Vec::new(),
                line,
            });
        }

        (symbols, imports)
    }

    fn parse_generic(&self, source: &str) -> (Vec<SymbolInfo>, Vec<ImportInfo>) {
        let mut symbols = Vec::new();
        let mut imports = Vec::new();

        for cap in RE_GENERIC_FUNC.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let name = cap[1].to_string();
            symbols.push(SymbolInfo {
                name,
                kind: SymbolKind::Function,
                line,
                end_line: None,
                signature: source
                    .lines()
                    .nth((line - 1) as usize)
                    .map(|l| l.trim().to_string()),
                visibility: None,
            });
        }

        for cap in RE_GENERIC_TYPE.captures_iter(source) {
            let full_match = cap.get(0).expect("regex group 0");
            let line = line_number(source, full_match.start());
            let name = cap[1].to_string();
            symbols.push(SymbolInfo {
                name,
                kind: SymbolKind::Type,
                line,
                end_line: None,
                signature: None,
                visibility: None,
            });
        }

        for m in RE_GENERIC_IMPORT.find_iter(source) {
            let line = line_number(source, m.start());
            let text = m.as_str().trim().to_string();
            imports.push(ImportInfo {
                path: text,
                items: Vec::new(),
                line,
            });
        }

        (symbols, imports)
    }
}

impl Default for FallbackParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeParser for FallbackParser {
    fn parse(&self, source: &str, language: &str) -> Result<ParsedFile, CodeIntelError> {
        let line_count = source.lines().count() as u32;

        let (symbols, imports) = match language {
            "rust" => self.parse_rust(source),
            "typescript" | "javascript" => self.parse_typescript(source),
            "python" => self.parse_python(source),
            "go" => self.parse_go(source),
            "java" => self.parse_java(source),
            _ => self.parse_generic(source),
        };

        Ok(ParsedFile {
            path: String::new(),
            language: language.to_string(),
            symbols,
            imports,
            line_count,
        })
    }

    fn supported_languages(&self) -> Vec<String> {
        vec![
            "rust".into(),
            "typescript".into(),
            "javascript".into(),
            "python".into(),
            "go".into(),
            "java".into(),
        ]
    }
}

/// Compute the 1-based line number for a byte offset in `source`.
fn line_number(source: &str, byte_offset: usize) -> u32 {
    source[..byte_offset].matches('\n').count() as u32 + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rust_functions() {
        let source = r#"
pub fn hello(name: &str) -> String {
    format!("hello {name}")
}

fn private_fn() {}

pub(crate) async fn async_task() {}
"#;
        let parser = FallbackParser::new();
        let result = parser.parse(source, "rust").unwrap();
        assert_eq!(result.symbols.len(), 3);
        assert_eq!(result.symbols[0].name, "hello");
        assert_eq!(result.symbols[0].kind, SymbolKind::Function);
        assert_eq!(result.symbols[0].visibility.as_deref(), Some("pub"));
        assert_eq!(result.symbols[1].name, "private_fn");
        assert_eq!(result.symbols[2].name, "async_task");
    }

    #[test]
    fn parse_rust_types() {
        let source = r#"
pub struct Config {
    name: String,
}

pub trait Parser {
    fn parse(&self);
}

enum Color { Red, Green, Blue }
"#;
        let parser = FallbackParser::new();
        let result = parser.parse(source, "rust").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Config"));
        assert!(names.contains(&"Parser"));
        assert!(names.contains(&"Color"));
    }

    #[test]
    fn parse_rust_imports() {
        let source = r#"
use std::collections::HashMap;
use crate::error::CodeIntelError;
"#;
        let parser = FallbackParser::new();
        let result = parser.parse(source, "rust").unwrap();
        assert_eq!(result.imports.len(), 2);
        assert_eq!(result.imports[0].path, "std::collections::HashMap");
    }

    #[test]
    fn parse_typescript() {
        let source = r#"
import { useState } from 'react';
import axios from 'axios';

export function greet(name: string): string {
    return `Hello ${name}`;
}

export class UserService {
    async fetchUser(id: number) {}
}

interface Config {
    apiUrl: string;
}

export const API_KEY = "abc";
"#;
        let parser = FallbackParser::new();
        let result = parser.parse(source, "typescript").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"UserService"));
        assert!(names.contains(&"Config"));
        assert!(names.contains(&"API_KEY"));
        assert!(!result.imports.is_empty());
    }

    #[test]
    fn parse_python() {
        let source = r#"
import os
from pathlib import Path, PurePath

class MyClass:
    def method(self):
        pass

def top_level():
    pass

async def async_handler(request):
    pass
"#;
        let parser = FallbackParser::new();
        let result = parser.parse(source, "python").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"MyClass"));
        assert!(names.contains(&"method"));
        assert!(names.contains(&"top_level"));
        assert!(names.contains(&"async_handler"));
        assert!(result.imports.len() >= 2);
    }

    #[test]
    fn parse_go() {
        let source = r#"
package main

import "fmt"

func main() {
    fmt.Println("hello")
}

func (s *Server) Start() error {
    return nil
}

type Config struct {
    Port int
}
"#;
        let parser = FallbackParser::new();
        let result = parser.parse(source, "go").unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"Start"));
        assert!(names.contains(&"Config"));
    }

    #[test]
    fn line_count_correct() {
        let source = "line1\nline2\nline3\n";
        let parser = FallbackParser::new();
        let result = parser.parse(source, "unknown").unwrap();
        assert_eq!(result.line_count, 3);
    }

    #[test]
    fn generic_fallback() {
        let source = r#"
function doSomething() {}
class Widget {}
"#;
        let parser = FallbackParser::new();
        let result = parser.parse(source, "unknown_lang").unwrap();
        assert!(!result.symbols.is_empty());
    }
}
