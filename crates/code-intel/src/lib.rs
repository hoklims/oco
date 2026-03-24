//! Code intelligence via Tree-sitter parsing and symbol extraction.
//!
//! This crate provides language-aware code parsing, symbol extraction, and
//! cross-file symbol indexing. The primary implementation uses Tree-sitter
//! grammars via [`TreeSitterParser`](ts_parser::TreeSitterParser) for accurate
//! AST-based parsing of Rust, Python, TypeScript, JavaScript, Go, and Java.
//! A regex-based [`FallbackParser`](parser::FallbackParser) handles other
//! languages. [`CompositeParser`](composite_parser::CompositeParser) combines
//! both: tree-sitter where available, regex fallback otherwise.
//!
//! # Quick start
//!
//! ```
//! use oco_code_intel::CompositeParser;
//! use oco_code_intel::parser::CodeParser;
//!
//! let parser = CompositeParser::new();
//! let result = parser.parse("pub fn hello() {}", "rust").unwrap();
//! assert_eq!(result.symbols[0].name, "hello");
//! assert!(result.symbols[0].end_line.is_some()); // tree-sitter provides end_line
//! ```

pub mod composite_parser;
pub mod error;
pub mod indexer;
pub mod languages;
pub mod parser;
pub mod symbols;
pub mod ts_parser;

pub use composite_parser::CompositeParser;
pub use error::CodeIntelError;
pub use indexer::SymbolIndexer;
pub use languages::{language_from_extension, language_from_path};
pub use parser::{CodeParser, FallbackParser, ParsedFile};
pub use symbols::{ImportInfo, SymbolInfo, SymbolKind};
pub use ts_parser::TreeSitterParser;
