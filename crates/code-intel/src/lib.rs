//! Code intelligence via Tree-sitter parsing and symbol extraction.
//!
//! This crate provides language-aware code parsing, symbol extraction, and
//! cross-file symbol indexing. The v1 implementation uses a regex-based
//! [`FallbackParser`](parser::FallbackParser) that works without Tree-sitter
//! grammars. The [`CodeParser`](parser::CodeParser) trait is designed so that
//! grammar-backed parsers can be added later.
//!
//! # Quick start
//!
//! ```
//! use oco_code_intel::parser::{FallbackParser, CodeParser};
//!
//! let parser = FallbackParser::new();
//! let result = parser.parse("pub fn hello() {}", "rust").unwrap();
//! assert_eq!(result.symbols[0].name, "hello");
//! ```

pub mod error;
pub mod indexer;
pub mod languages;
pub mod parser;
pub mod symbols;

pub use error::CodeIntelError;
pub use indexer::SymbolIndexer;
pub use languages::{language_from_extension, language_from_path};
pub use parser::{CodeParser, FallbackParser, ParsedFile};
pub use symbols::{ImportInfo, SymbolInfo, SymbolKind};
